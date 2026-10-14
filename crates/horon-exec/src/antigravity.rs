//! Antigravity executor: requests wrapped in the Cloud Code Assist envelope and sent to
//! `v1internal:{generateContent,streamGenerateContent}`. The `* -> Antigravity` translators
//! leave `project` empty; this executor fills it and the other fields the real client sends,
//! following opencodex's `src/adapters/google.ts` (`cloud-code-assist`).

use bytes::Bytes;
use horon_common::json as gj;
use horon_common::thinking::parse_suffix;
use horon_core::config::Config;
use horon_core::credential::{Credential, MetadataPatch};
use horon_core::exec::{ExecError, ExecRequest, ExecResponse, FailureScope, Operation, ResponseBody};
use horon_core::format::Format;
use serde_json::json;

use crate::antigravity_auth::{self as auth, AntigravityAuth};
use crate::gemini::{self as g, Emit, LineState, Output};
use crate::proxy::{self, GoClients, GoHeaders, Proxy, default_client};

pub use crate::antigravity_auth::PROVIDER;

fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map_or_else(String::new, |t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn refresh_token(c: &Credential) -> Option<&str> {
    c.str("refresh_token").filter(|s| !s.trim().is_empty())
}

fn project(c: &Credential) -> &str {
    c.attributes
        .get("project_id")
        .map(String::as_str)
        .or_else(|| c.str("project_id"))
        .map(str::trim)
        .unwrap_or_default()
}

fn base_url(c: &Credential) -> String {
    let attr = c.attributes.get("base_url").map(String::as_str).unwrap_or_default();
    let meta = c.str("base_url").unwrap_or_default();
    [attr, meta, auth::DAILY_API]
        .into_iter()
        .map(|s| s.trim().trim_end_matches('/'))
        .find(|s| !s.is_empty())
        .unwrap_or(auth::DAILY_API)
        .to_owned()
}

fn expiry(c: &Credential) -> Option<chrono::DateTime<chrono::Utc>> {
    c.str("expired")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.to_utc())
}

pub fn stable_session_id(first_user_text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(first_user_text.as_bytes());
    let mut head = [0u8; 8];
    head.copy_from_slice(&digest[..8]);
    format!("-{}", u64::from_be_bytes(head) & 0x7fff_ffff_ffff_ffff)
}

fn first_user_text(request: &[u8]) -> Option<String> {
    let contents = gj::get(request, "request.contents");
    contents
        .array()
        .iter()
        .find(|c| c.get("role").str() == "user")
        .and_then(|c| {
            c.get("parts")
                .array()
                .iter()
                .map(|p| p.get("text").str().into_owned())
                .find(|t| !t.is_empty())
        })
}

pub fn finish_envelope(mut body: Vec<u8>, project: &str, session_hint: Option<&str>) -> Vec<u8> {
    gj::set_str(&mut body, "project", project);
    gj::set_str(&mut body, "userAgent", "antigravity");
    gj::set_str(&mut body, "requestType", "agent");
    gj::set_str(&mut body, "requestId", format!("agent-{}", uuid::Uuid::new_v4()));
    if !gj::get(&body, "request.sessionId").exists() {
        let id = match session_hint
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| first_user_text(&body))
        {
            Some(anchor) => stable_session_id(&anchor),
            None => format!("-{}", uuid::Uuid::new_v4().as_u128() & 0x7fff_ffff_ffff_ffff),
        };
        gj::set_str(&mut body, "request.sessionId", &id);
    }
    body
}

pub struct AntigravityExecutor {
    clients: GoClients,
    token_url: Option<String>,
}

impl Default for AntigravityExecutor {
    fn default() -> Self {
        Self::with_client(default_client())
    }
}

struct AntigravityLines(Output);

impl LineState for AntigravityLines {
    fn line(&mut self, line: &[u8]) -> Emit {
        let filtered = crate::gemini_stream::filter_sse_usage_metadata(line);
        self.0.usage.response_line(Format::Antigravity, &filtered);
        match crate::gemini_stream::json_payload(&filtered) {
            Some(payload) => self.0.translate(payload),
            None => Emit::default(),
        }
    }

    fn end(&mut self) -> Emit {
        self.0.end_with_done()
    }

    fn flush(&mut self) -> Vec<Bytes> {
        self.0.flush_frames()
    }
}

impl AntigravityExecutor {
    pub fn with_client(client: wreq::Client) -> Self {
        Self {
            clients: GoClients::with_default(client),
            token_url: None,
        }
    }

    pub fn with_token_url(mut self, url: &str) -> Self {
        self.token_url = Some(url.to_owned());
        self
    }

    fn client(&self, credential: &Credential, cfg: &Config) -> wreq::Client {
        self.clients.get(&Proxy::effective(credential, cfg))
    }

    pub fn needs_prepare_at(&self, credential: &Credential, _cfg: &Config, now: chrono::DateTime<chrono::Utc>) -> bool {
        refresh_token(credential).is_some()
            && expiry(credential)
                .is_none_or(|t| t - chrono::Duration::from_std(auth::REFRESH_LEAD).unwrap_or_default() <= now)
    }

    pub async fn prepare(&self, credential: &Credential, cfg: &Config) -> Result<MetadataPatch, ExecError> {
        let Some(refresh) = refresh_token(credential) else {
            return Ok(MetadataPatch::default());
        };
        let mut flow = AntigravityAuth::new(self.client(credential, cfg));
        if let Some(url) = &self.token_url {
            flow = flow.with_endpoints(url, auth::PROD_API);
        }
        let tokens = flow.refresh(refresh).await?;
        let mut patch = MetadataPatch::default();
        for (k, v) in [
            ("access_token", json!(tokens.access)),
            ("refresh_token", json!(tokens.refresh)),
            ("expired", json!(rfc3339(tokens.expires_at))),
            (
                "last_refresh",
                json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            ),
        ] {
            patch.set.insert(k.into(), v);
        }
        if project(credential).is_empty()
            && let Some(found) = flow.load_project(&tokens.access).await
        {
            patch.set.insert("project_id".into(), json!(found));
        }
        Ok(patch)
    }

    pub async fn execute(
        &self,
        credential: &Credential,
        req: ExecRequest,
        cfg: &Config,
    ) -> Result<ExecResponse, ExecError> {
        if req.operation == Operation::CountTokens {
            return Err(g::status_err(501, "antigravity: token counting is not supported"));
        }
        if req.alt.as_deref() == Some(g::COMPACT_ALT) {
            return Err(g::status_err(501, "/responses/compact not supported"));
        }
        let token = credential.str("access_token").map(str::trim).unwrap_or_default();
        if token.is_empty() {
            return Err(ExecError::local(
                401,
                FailureScope::Credential,
                "antigravity executor: missing access token",
            ));
        }
        let project = project(credential);
        if project.is_empty() {
            return Err(ExecError::local(
                401,
                FailureScope::Credential,
                "antigravity executor: no Cloud Code Assist project; sign in again with -antigravity-login",
            ));
        }
        let base_model = parse_suffix(&req.model).model_name;
        let (from, to) = (req.source_format, Format::Antigravity);
        let resolved = g::resolved(&req);
        let body = g::translate(&req, cfg, to, &base_model, &req.body, req.stream, false)?;
        let source = g::original_request(&req);
        let original = if *source == req.body {
            body.clone()
        } else {
            g::translate(&req, cfg, to, &base_model, source, req.stream, false)?
        };
        if body.is_empty() {
            return Err(g::bad_gateway());
        }
        let mut body = g::apply_thinking(&req, body, from, to, PROVIDER, resolved.as_ref())?;
        let rules = horon_common::payload::Rules::from_config(cfg);
        body = g::apply_payload_rules(&rules, &base_model, to.as_str(), body, &original, &req);
        body = finish_envelope(body, project, req.session.as_deref());
        req.usage.request(Format::Antigravity, &body);

        let method = if req.stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let mut url = format!("{}/{}:{method}", base_url(credential), auth::API_VERSION);
        if req.stream {
            url.push_str("?alt=sse");
        }
        let mut headers = GoHeaders::new();
        headers.set("Content-Type", "application/json");
        headers.set("Authorization", format!("Bearer {token}"));
        headers.set("User-Agent", auth::user_agent());
        if req.stream {
            headers.set("Accept", "text/event-stream");
        }
        g::set_custom_headers(&mut headers, credential, &req);
        let upstream = proxy::send(&self.client(credential, cfg), &url, headers, body.clone(), None).await?;
        if !(200..300).contains(&upstream.status) {
            return Err(g::error_from(upstream).await);
        }
        let response_headers = upstream.headers.clone();
        if !req.stream {
            let data = proxy::read_all(upstream.body, usize::MAX, false).await?.to_vec();
            req.usage.response_body(Format::Antigravity, &data);
            let out = g::translate_non_stream(&req, Format::Antigravity, &body, &data)?;
            return Ok(ExecResponse {
                status: 200,
                headers: response_headers,
                body: ResponseBody::Buffered(out),
            });
        }
        let output = Output::new(&req, Format::Antigravity, &body);
        let stream = g::drive(proxy::lines(upstream.body, g::MAX_LINE), AntigravityLines(output));
        Ok(ExecResponse {
            status: 200,
            headers: response_headers,
            body: ResponseBody::Stream(stream),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(meta: serde_json::Value) -> Credential {
        Credential::from_file(
            std::path::Path::new("/a"),
            std::path::Path::new("/a/ag.json"),
            meta.as_object().unwrap().clone(),
        )
        .unwrap()
    }

    #[test]
    fn the_session_id_is_stable_positive_and_dashed() {
        let a = stable_session_id("hello");
        assert_eq!(a, stable_session_id("hello"));
        assert_ne!(a, stable_session_id("hello!"));
        assert!(a.starts_with('-') && a[1..].parse::<u64>().unwrap() < (1 << 63));
    }

    #[test]
    fn the_envelope_gets_its_project_identity_and_session() {
        let open =
            br#"{"project":"","model":"m","request":{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}}"#.to_vec();
        let body = finish_envelope(open.clone(), "proj-1", None);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            (
                v["project"].as_str(),
                v["userAgent"].as_str(),
                v["requestType"].as_str()
            ),
            (Some("proj-1"), Some("antigravity"), Some("agent"))
        );
        assert!(v["requestId"].as_str().unwrap().starts_with("agent-"));
        assert_eq!(v["request"]["sessionId"], stable_session_id("hi"));
        let hinted: serde_json::Value = serde_json::from_slice(&finish_envelope(open, "p", Some("anchor"))).unwrap();
        assert_eq!(hinted["request"]["sessionId"], stable_session_id("anchor"));
        assert_ne!(v["requestId"], hinted["requestId"]);
    }

    #[test]
    fn an_existing_session_id_is_kept() {
        let body = br#"{"project":"","request":{"sessionId":"-7","contents":[]}}"#.to_vec();
        let v: serde_json::Value = serde_json::from_slice(&finish_envelope(body, "p", None)).unwrap();
        assert_eq!(v["request"]["sessionId"], "-7");
    }

    #[test]
    fn a_token_is_refreshed_before_it_expires() {
        let ex = AntigravityExecutor::default();
        let cfg = Config::default();
        let now = chrono::Utc::now();
        let soon =
            credential(json!({"type": "antigravity", "refresh_token": "r", "expired": rfc3339(now.timestamp() + 60)}));
        let later = credential(
            json!({"type": "antigravity", "refresh_token": "r", "expired": rfc3339(now.timestamp() + 3600)}),
        );
        let bare = credential(json!({"type": "antigravity", "expired": rfc3339(now.timestamp() - 5)}));
        assert!(ex.needs_prepare_at(&soon, &cfg, now));
        assert!(!ex.needs_prepare_at(&later, &cfg, now));
        assert!(!ex.needs_prepare_at(&bare, &cfg, now));
    }

    async fn serve_once(
        status: u16,
        content_type: &'static str,
        body: &'static str,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let seen: std::sync::Arc<std::sync::Mutex<Vec<u8>>> = Default::default();
        let sink = seen.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).into_owned();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text
                        .to_ascii_lowercase()
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            *sink.lock().unwrap() = buf;
            let reply = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        (addr, seen)
    }

    fn generate_request(stream: bool) -> ExecRequest {
        let body = bytes::Bytes::from_static(
            br#"{"model":"gemini-3-flash","messages":[{"role":"user","content":"hi there"}],"stream":true}"#,
        );
        ExecRequest {
            operation: Operation::Generate,
            source_format: Format::OpenAI,
            response_format: Format::OpenAI,
            requested_model: "gemini-3-flash".into(),
            model: "gemini-3-flash".into(),
            original_body: body.clone(),
            body,
            stream,
            alt: None,
            session: None,
            execution_session: None,
            derived_session: None,
            resolved_model: None,
            usage: Default::default(),
            request_path: String::new(),
            headers: http::HeaderMap::new(),
            caller: horon_core::exec::Caller {
                principal: "fake-client-key".into(),
                source: "authorization",
            },
        }
    }

    #[tokio::test]
    async fn a_request_goes_out_in_the_envelope_with_the_ide_headers() {
        use futures_util::StreamExt;
        let sse = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hel\"}]}}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":1,\"totalTokenCount\":4}}}\n\ndata: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5}}}\n\n";
        let (addr, seen) = serve_once(200, "text/event-stream", sse).await;
        let c = credential(json!({
            "type": "antigravity", "access_token": "tok-1", "project_id": "proj-9", "base_url": addr,
        }));
        let ex = AntigravityExecutor::default();
        let res = ex
            .execute(&c, generate_request(true), &Config::default())
            .await
            .unwrap();
        let ResponseBody::Stream(mut stream) = res.body else {
            panic!("stream")
        };
        let mut out = String::new();
        while let Some(chunk) = stream.next().await {
            out.push_str(&String::from_utf8_lossy(&chunk.unwrap()));
        }
        let raw = String::from_utf8(seen.lock().unwrap().clone()).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(
            head.starts_with("POST /v1internal:streamGenerateContent?alt=sse "),
            "{head}"
        );
        let head = head.to_ascii_lowercase();
        assert!(head.contains("authorization: bearer tok-1"), "{head}");
        assert!(head.contains("user-agent: antigravity/ide/"), "{head}");
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(sent["project"], "proj-9");
        assert_eq!(sent["userAgent"], "antigravity");
        assert_eq!(sent["requestType"], "agent");
        assert!(sent["requestId"].as_str().unwrap().starts_with("agent-"));
        assert!(sent["request"]["sessionId"].as_str().unwrap().starts_with('-'));
        assert_eq!(sent["model"], "gemini-3-flash");
        assert!(
            out.contains("\"content\":\"hel\"") && out.contains("\"content\":\"lo\""),
            "{out}"
        );
        let with_usage = out.matches("\"usage\"").count();
        assert_eq!(with_usage, 1, "only the final chunk may carry usage: {out}");
    }

    #[tokio::test]
    async fn a_credential_without_a_project_never_reaches_the_network() {
        let c = credential(json!({"type": "antigravity", "access_token": "tok-1"}));
        let err = AntigravityExecutor::default()
            .execute(&c, generate_request(false), &Config::default())
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.status, 401);
    }

    #[test]
    fn the_base_url_defaults_to_the_daily_endpoint() {
        assert_eq!(base_url(&credential(json!({"type": "antigravity"}))), auth::DAILY_API);
        assert_eq!(
            base_url(&credential(
                json!({"type": "antigravity", "base_url": "https://x.test/"})
            )),
            "https://x.test"
        );
    }
}
