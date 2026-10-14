//! Nous Portal: device-grant sign-in, single-use refresh, and an OpenAI Chat executor
//! (no Go counterpart; follows opencodex's `src/oauth/nous.ts`).
//!
//! The Portal's access token is the per-request inference JWT (scope `inference:invoke`),
//! used as the bearer against `https://inference-api.nousresearch.com/v1`. Refresh tokens are
//! single use: the Portal rotates one on every refresh and treats a replay as theft, revoking
//! the session. So a refresh whose outcome is unknown is never retried (see [`prepare`]).

use std::time::Duration;

use base64::Engine as _;
use horon_core::config::Config;
use horon_core::credential::{Credential, MetadataPatch};
use horon_core::exec::{ExecError, ExecRequest, ExecResponse, FailureScope};
use serde_json::{Map, Value, json};

use crate::kimi_auth::{encode_credential, form, open_browser, write_private};
use crate::openai_compat::OpenAICompatExecutor;
use crate::proxy::{GoClients, GoHeaders, Hooks, MAX_ERROR_BODY, Proxy, default_client, read_all, send};

pub const PROVIDER: &str = "nous";
pub const PORTAL_URL: &str = "https://portal.nousresearch.com";
pub const INFERENCE_URL: &str = "https://inference-api.nousresearch.com/v1";
pub const CLIENT_ID: &str = "hermes-cli";
pub const SCOPE: &str = "inference:invoke";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);
const MAX_INTERVAL: Duration = Duration::from_secs(30);
const DEVICE_TTL: Duration = Duration::from_secs(15 * 60);
const DEFAULT_TOKEN_TTL_SECS: i64 = 12 * 60 * 60;
const MAX_TOKEN_TTL_SECS: i64 = 30 * 24 * 60 * 60;
pub const REFRESH_LEAD: Duration = Duration::from_secs(2 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

fn auth_error(status: u16, message: impl Into<String>) -> ExecError {
    ExecError::local(status, FailureScope::Transport, message)
}

fn json_or_empty(body: &[u8]) -> Value {
    serde_json::from_slice::<Value>(body)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (_, payload, _, None) = (parts.next()?, parts.next()?, parts.next()?, parts.next()) else {
        return None;
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: i64,
    pub subject: Option<String>,
    pub email: Option<String>,
}

/// Validates a token response. A reply without a replacement refresh token, or with the one
/// we sent, leaves us holding a consumed credential, so both are errors rather than a
/// silent fallback. The access token must grant `inference:invoke`.
pub fn parse_tokens(body: &Value, submitted_refresh: &str, now: i64) -> Result<Tokens, ExecError> {
    let Some(access) = text(body, "access_token") else {
        return Err(auth_error(502, "nous: token response did not include an access token"));
    };
    let Some(refresh) = text(body, "refresh_token") else {
        return Err(auth_error(
            502,
            if submitted_refresh.is_empty() {
                "nous: token response did not include a refresh token"
            } else {
                "nous: no replacement refresh token; refusing to reuse the consumed one"
            },
        ));
    };
    if !submitted_refresh.is_empty() && refresh == submitted_refresh {
        return Err(auth_error(
            502,
            "nous: the Portal returned the refresh token we submitted; refusing to reuse it",
        ));
    }
    let claims = jwt_claims(&access);
    let granted = claims
        .as_ref()
        .and_then(|c| text(c, "scope"))
        .is_some_and(|s| s.split_whitespace().any(|s| s == SCOPE));
    if !granted {
        return Err(auth_error(
            403,
            "nous: the access token does not grant the inference:invoke scope",
        ));
    }
    let exp = claims
        .as_ref()
        .and_then(|c| c.get("exp"))
        .and_then(Value::as_i64)
        .filter(|exp| *exp > now && *exp <= now + MAX_TOKEN_TTL_SECS);
    let expires_at = exp.unwrap_or_else(|| {
        now + body
            .get("expires_in")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_TOKEN_TTL_SECS)
    });
    Ok(Tokens {
        access,
        refresh,
        expires_at,
        subject: claims.as_ref().and_then(|c| text(c, "sub")),
        email: claims.as_ref().and_then(|c| text(c, "email")).map(|e| e.to_lowercase()),
    })
}

#[derive(Debug, Clone, Default)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub url: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

#[derive(Clone)]
pub struct NousAuth {
    client: wreq::Client,
    portal: String,
    min_interval: Option<Duration>,
}

impl NousAuth {
    pub fn new(client: wreq::Client) -> Self {
        Self {
            client,
            portal: PORTAL_URL.to_owned(),
            min_interval: None,
        }
    }

    pub fn with_portal(mut self, portal: &str) -> Self {
        self.portal = portal.trim_end_matches('/').to_owned();
        self
    }

    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = Some(interval);
        self
    }

    async fn post(&self, path: &str, body: String, refresh: Option<&str>) -> Result<(u16, Vec<u8>), ExecError> {
        let mut h = GoHeaders::new();
        h.set("Accept", "application/json");
        h.set("Content-Type", "application/x-www-form-urlencoded");
        if let Some(refresh) = refresh {
            h.set("x-nous-refresh-token", refresh);
        }
        let url = format!("{}{path}", self.portal);
        let upstream = send(&self.client, &url, h, body, Some(HTTP_TIMEOUT))
            .await
            .map_err(|_| auth_error(502, "nous: token request failed"))?;
        let status = upstream.status;
        let body = read_all(upstream.body, MAX_ERROR_BODY, false)
            .await
            .map_err(|_| auth_error(502, "nous: failed to read token response"))?;
        Ok((status, body.to_vec()))
    }

    pub async fn request_device_code(&self) -> Result<DeviceCode, ExecError> {
        let body = form(&[("client_id", CLIENT_ID), ("scope", SCOPE)]);
        let (status, raw) = self.post("/api/oauth/device/code", body, None).await?;
        if !(200..300).contains(&status) {
            return Err(auth_error(
                status,
                format!("nous: device code request failed with status {status}"),
            ));
        }
        let v = json_or_empty(&raw);
        let (Some(device_code), Some(user_code)) = (text(&v, "device_code"), text(&v, "user_code")) else {
            return Err(auth_error(
                502,
                "nous: device authorization response missing required fields",
            ));
        };
        let Some(url) = text(&v, "verification_uri_complete").or_else(|| text(&v, "verification_uri")) else {
            return Err(auth_error(
                502,
                "nous: device authorization response missing required fields",
            ));
        };
        let secs = |k: &str| {
            v.get(k)
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
                .map(Duration::from_secs)
        };
        Ok(DeviceCode {
            device_code,
            user_code,
            url,
            expires_in: secs("expires_in").unwrap_or(DEVICE_TTL),
            interval: secs("interval").unwrap_or(DEFAULT_INTERVAL),
        })
    }

    pub async fn poll(&self, code: &DeviceCode) -> Result<Tokens, ExecError> {
        let deadline = tokio::time::Instant::now() + code.expires_in;
        let mut wait = self
            .min_interval
            .unwrap_or_else(|| code.interval.max(Duration::from_secs(1)));
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(auth_error(408, "nous: device flow timed out"));
            }
            let body = form(&[
                ("client_id", CLIENT_ID),
                ("device_code", &code.device_code),
                ("grant_type", DEVICE_GRANT),
            ]);
            match self.post("/api/oauth/token", body, None).await {
                // A dropped connection does not end a device session that is still open.
                Err(_) => {}
                Ok((status, raw)) => {
                    let v = json_or_empty(&raw);
                    if (200..300).contains(&status) {
                        return parse_tokens(&v, "", chrono::Utc::now().timestamp());
                    }
                    match v.get("error").and_then(Value::as_str).unwrap_or_default() {
                        "authorization_pending" => {}
                        "slow_down" => {
                            wait = (wait + Duration::from_secs(5)).min(MAX_INTERVAL);
                            if let Some(n) = v.get("interval").and_then(Value::as_u64) {
                                wait = wait.max(Duration::from_secs(n).min(MAX_INTERVAL));
                            }
                        }
                        "expired_token" => return Err(auth_error(400, "nous: device code expired")),
                        "access_denied" => return Err(auth_error(403, "nous: access denied by user")),
                        "" => {
                            return Err(auth_error(
                                status,
                                format!("nous: token request failed with status {status}"),
                            ));
                        }
                        other => return Err(auth_error(400, format!("nous: OAuth error: {other}"))),
                    }
                }
            }
            tokio::time::sleep(wait.min(deadline.saturating_duration_since(tokio::time::Instant::now()))).await;
        }
    }

    /// Single-use refresh: the token travels in a header and the reply carries its
    /// replacement. Any answer other than 2xx may still have consumed the token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Tokens, RefreshError> {
        let body = form(&[("grant_type", "refresh_token"), ("client_id", CLIENT_ID)]);
        let (status, raw) = self
            .post("/api/oauth/token", body, Some(refresh_token))
            .await
            .map_err(|e| RefreshError {
                error: e,
                consumed: true,
            })?;
        let v = json_or_empty(&raw);
        if !(200..300).contains(&status) {
            let code = v.get("error").and_then(Value::as_str).unwrap_or_default();
            return Err(RefreshError {
                error: auth_error(
                    status,
                    format!("nous: token refresh failed with status {status} {code}")
                        .trim()
                        .to_owned(),
                ),
                consumed: true,
            });
        }
        parse_tokens(&v, refresh_token, chrono::Utc::now().timestamp())
            .map_err(|error| RefreshError { error, consumed: true })
    }
}

#[derive(Debug)]
pub struct RefreshError {
    pub error: ExecError,
    pub consumed: bool,
}

fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map_or_else(String::new, |t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

pub struct LoginRecord {
    pub file_name: String,
    pub metadata: Map<String, Value>,
    pub label: String,
}

pub fn login_record(tokens: &Tokens, now_ms: i64) -> LoginRecord {
    let who = tokens
        .email
        .clone()
        .or_else(|| tokens.subject.clone())
        .unwrap_or_default();
    let safe: String = who
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut metadata = Map::new();
    for (k, v) in [
        ("type", json!(PROVIDER)),
        ("access_token", json!(tokens.access)),
        ("refresh_token", json!(tokens.refresh)),
        ("expired", json!(rfc3339(tokens.expires_at))),
        ("base_url", json!(INFERENCE_URL)),
        ("timestamp", json!(now_ms)),
        ("disabled", json!(false)),
    ] {
        metadata.insert(k.into(), v);
    }
    if let Some(email) = &tokens.email {
        metadata.insert("email".into(), json!(email));
    }
    if let Some(sub) = &tokens.subject {
        metadata.insert("account_id".into(), json!(sub));
    }
    LoginRecord {
        file_name: if safe.is_empty() {
            format!("nous-{now_ms}.json")
        } else {
            format!("nous-{safe}.json")
        },
        metadata,
        label: if who.is_empty() { "Nous Portal user".into() } else { who },
    }
}

pub fn write_login(auth_dir: &std::path::Path, record: &LoginRecord) -> Result<std::path::PathBuf, ExecError> {
    let path = auth_dir.join(&record.file_name);
    write_private(&path, encode_credential(&record.metadata).as_bytes())
        .map_err(|_| auth_error(500, "nous: cannot write credential file"))?;
    Ok(path)
}

pub async fn login(cfg: &Config, no_browser: bool) -> Result<std::path::PathBuf, ExecError> {
    let proxy = cfg
        .document
        .get("requests")
        .and_then(|r| r.get("proxy-url"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let auth = NousAuth::new(GoClients::new(Hooks::default()).get(&Proxy::parse(proxy)));
    println!("Starting Nous Portal authentication...");
    let code = auth.request_device_code().await?;
    print!(
        "\nTo authenticate, please visit:\n{}\n\nUser code: {}\n\n",
        code.url, code.user_code
    );
    if !no_browser && open_browser(&code.url) {
        println!("Browser opened automatically.");
    }
    println!("Waiting for authorization...");
    let tokens = auth.poll(&code).await?;
    let record = login_record(&tokens, chrono::Utc::now().timestamp_millis());
    let path = write_login(&cfg.auth_dir, &record)?;
    println!("Authentication saved to {}", path.display());
    println!("Authenticated as {}", record.label);
    Ok(path)
}

fn refresh_token(c: &Credential) -> Option<&str> {
    c.str("refresh_token").filter(|s| !s.trim().is_empty())
}

fn expiry(c: &Credential) -> Option<chrono::DateTime<chrono::Utc>> {
    c.str("expired")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.to_utc())
}

fn base_url(c: &Credential) -> String {
    let attr = c.attributes.get("base_url").map(|s| s.trim()).unwrap_or_default();
    let meta = c.str("base_url").map(str::trim).unwrap_or_default();
    [attr, meta, INFERENCE_URL]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or(INFERENCE_URL)
        .trim_end_matches('/')
        .to_owned()
}

pub struct NousExecutor {
    clients: GoClients,
    chat: OpenAICompatExecutor,
    portal: Option<String>,
}

impl Default for NousExecutor {
    fn default() -> Self {
        Self::with_client(default_client())
    }
}

impl NousExecutor {
    pub fn with_client(client: wreq::Client) -> Self {
        Self {
            clients: GoClients::with_default(client.clone()),
            chat: OpenAICompatExecutor::with_client(client),
            portal: None,
        }
    }

    pub fn with_portal(mut self, portal: &str) -> Self {
        self.portal = Some(portal.to_owned());
        self
    }

    pub fn needs_prepare_at(&self, credential: &Credential, _cfg: &Config, now: chrono::DateTime<chrono::Utc>) -> bool {
        refresh_token(credential).is_some()
            && expiry(credential)
                .is_some_and(|t| t - chrono::Duration::from_std(REFRESH_LEAD).unwrap_or_default() <= now)
    }

    /// The rotated token pair, to be stored by the caller. A refresh whose outcome is unknown
    /// is not retried: the old token stays in the file, so the next attempt fails with the
    /// Portal's own answer and the account asks for a new sign-in instead of replaying it.
    pub async fn prepare(&self, credential: &Credential, cfg: &Config) -> Result<MetadataPatch, ExecError> {
        let Some(refresh) = refresh_token(credential) else {
            return Ok(MetadataPatch::default());
        };
        let mut auth = NousAuth::new(self.clients.get(&Proxy::effective(credential, cfg)));
        if let Some(portal) = &self.portal {
            auth = auth.with_portal(portal);
        }
        let tokens = auth.refresh(refresh).await.map_err(|e| e.error)?;
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
        Ok(patch)
    }

    pub async fn execute(
        &self,
        credential: &Credential,
        req: ExecRequest,
        cfg: &Config,
    ) -> Result<ExecResponse, ExecError> {
        if req.operation == horon_core::exec::Operation::CountTokens {
            return self.chat.execute(credential, req, cfg).await;
        }
        let key = credential.str("access_token").map(str::trim).unwrap_or_default();
        if key.is_empty() {
            return Err(ExecError::local(
                401,
                FailureScope::Credential,
                "nous executor: missing access token",
            ));
        }
        let url = base_url(credential);
        self.chat.chat_with(credential, req, cfg, &url, key, &[]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(claims: Value) -> String {
        let enc = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        format!("{}.{}.sig", enc(&json!({"alg": "none"})), enc(&claims))
    }

    #[test]
    fn tokens_need_the_inference_scope_and_a_rotated_refresh_token() {
        let now = 1_000_000;
        let ok = jwt(json!({"scope": "inference:invoke other", "sub": "u1", "email": "A@B.test", "exp": now + 3600}));
        let t = parse_tokens(&json!({"access_token": ok, "refresh_token": "r2"}), "r1", now).unwrap();
        assert_eq!(
            (t.expires_at, t.subject.as_deref(), t.email.as_deref()),
            (now + 3600, Some("u1"), Some("a@b.test"))
        );
        for (body, submitted, why) in [
            (json!({"access_token": ok}), "r1", "no replacement"),
            (json!({"access_token": ok, "refresh_token": "r1"}), "r1", "same token"),
            (json!({"refresh_token": "r2"}), "r1", "no access token"),
            (
                json!({"access_token": jwt(json!({"scope": "other"})), "refresh_token": "r2"}),
                "r1",
                "wrong scope",
            ),
            (
                json!({"access_token": "opaque", "refresh_token": "r2"}),
                "",
                "opaque token",
            ),
        ] {
            assert!(parse_tokens(&body, submitted, now).is_err(), "{why}");
        }
    }

    #[test]
    fn an_implausible_exp_falls_back_to_expires_in() {
        let now = 1_000_000;
        for exp in [now - 5, now + MAX_TOKEN_TTL_SECS + 1] {
            let access = jwt(json!({"scope": SCOPE, "exp": exp}));
            let t = parse_tokens(
                &json!({"access_token": access, "refresh_token": "r2", "expires_in": 600}),
                "",
                now,
            )
            .unwrap();
            assert_eq!(t.expires_at, now + 600, "exp {exp}");
        }
    }

    #[test]
    fn login_record_is_keyed_by_the_account() {
        let t = Tokens {
            access: "a".into(),
            refresh: "r".into(),
            expires_at: 0,
            subject: Some("u/1".into()),
            email: None,
        };
        let r = login_record(&t, 5);
        assert_eq!(r.file_name, "nous-u_1.json");
        assert_eq!(r.metadata["type"], "nous");
        assert_eq!(r.metadata["base_url"], INFERENCE_URL);
        assert_eq!(base_url_of(&r.metadata), INFERENCE_URL);
    }

    async fn portal_replying(status: u16, body: String) -> (String, std::sync::Arc<std::sync::Mutex<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let seen: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
        let sink = seen.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16384];
            let n = socket.read(&mut buf).await.unwrap();
            *sink.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).into_owned();
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        (addr, seen)
    }

    fn nous_credential(refresh: &str) -> Credential {
        Credential::from_file(
            std::path::Path::new("/a"),
            std::path::Path::new("/a/n.json"),
            json!({"type": "nous", "access_token": "old", "refresh_token": refresh, "expired": "2000-01-01T00:00:00Z"})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_refresh_sends_the_token_in_the_header_and_stores_the_rotated_pair() {
        let now = chrono::Utc::now().timestamp();
        let access = jwt(json!({"scope": SCOPE, "exp": now + 3600}));
        let (portal, seen) =
            portal_replying(200, json!({"access_token": access, "refresh_token": "r2"}).to_string()).await;
        let ex = NousExecutor::default().with_portal(&portal);
        assert!(ex.needs_prepare_at(&nous_credential("r1"), &Config::default(), chrono::Utc::now()));
        let patch = ex.prepare(&nous_credential("r1"), &Config::default()).await.unwrap();
        assert_eq!(patch.set["refresh_token"], "r2");
        assert_eq!(patch.set["access_token"], json!(access));
        let raw = seen.lock().unwrap().to_ascii_lowercase();
        assert!(raw.contains("x-nous-refresh-token: r1"), "{raw}");
        assert!(
            !raw.contains("refresh_token=r1"),
            "the token must not travel in the body: {raw}"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_changes_nothing_so_the_consumed_token_is_never_replayed() {
        let (portal, _) = portal_replying(400, r#"{"error":"invalid_grant"}"#.to_owned()).await;
        let ex = NousExecutor::default().with_portal(&portal);
        let err = ex
            .prepare(&nous_credential("r1"), &Config::default())
            .await
            .unwrap_err();
        assert_eq!(err.status, 400);
        assert!(!String::from_utf8_lossy(&err.body).contains("r1"));
    }

    fn base_url_of(m: &Map<String, Value>) -> String {
        let mut c =
            Credential::from_file(std::path::Path::new("/a"), std::path::Path::new("/a/n.json"), m.clone()).unwrap();
        c.attributes.clear();
        base_url(&c)
    }
}
