//! GitHub Copilot: device-flow sign-in, the `copilot_internal` token exchange and an OpenAI
//! Chat executor (no Go counterpart; follows opencodex's `src/oauth/github-copilot.ts`).
//! Unofficial: it uses the public VS Code OAuth client and GitHub may revoke the path.

use std::time::Duration;

use hr_core::config::Config;
use hr_core::credential::{Credential, MetadataPatch};
use hr_core::exec::{ExecError, ExecRequest, ExecResponse, FailureScope, Operation};
use serde_json::{Map, Value, json};

use crate::kimi_auth::{encode_credential, form, open_browser, write_private};
use crate::openai_compat::OpenAICompatExecutor;
use crate::proxy::{GoClients, GoHeaders, Hooks, MAX_ERROR_BODY, Proxy, default_client, read_all, request, send};

pub const PROVIDER: &str = "github-copilot";
pub const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
pub const DEFAULT_API_BASE: &str = "https://api.githubcopilot.com";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const USER_URL: &str = "https://api.github.com/user";
const VERIFY_URL: &str = "https://github.com/login/device";
const SCOPE: &str = "read:user";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);
const DEVICE_TTL: Duration = Duration::from_secs(15 * 60);
const FALLBACK_TTL_SECS: i64 = 25 * 60;
const EXPIRY_SKEW_SECS: i64 = 2 * 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Models that reject `/chat/completions` for agent traffic (function tools with reasoning).
/// opencodex sends these over the Responses wire; this executor has only the Chat wire, so
/// they are listed to fail fast with a clear message instead of an opaque upstream 400.
const RESPONSES_ONLY: &[&str] = &[
    "gpt-5.3-codex",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.5",
    "gpt-5.6-luna",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-6-astra",
    "gpt-6-sol",
    "gpt-6-luna",
    "gpt-6.1-sol",
    "grok-4.5",
    "grok-4.6",
    "mai-code-1.1-flash",
    "mai-code-1-flash-picker",
];

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

/// The bearer only ever goes to a `*.githubcopilot.com` host over HTTPS on the default port.
pub fn validate_api_base(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw.trim()).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let ok = url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && (host == "api.githubcopilot.com" || host.ends_with(".githubcopilot.com"));
    ok.then(|| format!("https://{host}"))
}

pub fn verify_url(user_code: &str) -> Option<String> {
    let code = user_code.trim();
    (!code.is_empty() && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .then(|| format!("{VERIFY_URL}?user_code={code}"))
}

fn headers(authorization: &str) -> GoHeaders {
    let mut h = GoHeaders::new();
    h.set("Accept", "application/json");
    h.set("Authorization", authorization);
    h.set("User-Agent", "horon-router");
    h.set("Editor-Version", "horon-router/0.1.0");
    h.set("Editor-Plugin-Version", "horon-router/0.1.0");
    h.set("Copilot-Integration-Id", "vscode-chat");
    h
}

#[derive(Debug, Clone, Default)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub url: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubGrant {
    pub access: String,
    pub refresh: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub copilot_token: String,
    pub expires_at: i64,
    pub api_base: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub account_id: String,
    pub email: Option<String>,
}

#[derive(Clone)]
pub struct CopilotAuth {
    client: wreq::Client,
    min_interval: Option<Duration>,
    device_url: String,
    token_url: String,
    copilot_url: String,
    user_url: String,
}

impl CopilotAuth {
    pub fn new(client: wreq::Client) -> Self {
        Self {
            client,
            min_interval: None,
            device_url: DEVICE_CODE_URL.to_owned(),
            token_url: ACCESS_TOKEN_URL.to_owned(),
            copilot_url: COPILOT_TOKEN_URL.to_owned(),
            user_url: USER_URL.to_owned(),
        }
    }

    pub fn with_origin(mut self, origin: &str) -> Self {
        let o = origin.trim_end_matches('/');
        self.device_url = format!("{o}/login/device/code");
        self.token_url = format!("{o}/login/oauth/access_token");
        self.copilot_url = format!("{o}/copilot_internal/v2/token");
        self.user_url = format!("{o}/user");
        self
    }

    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = Some(interval);
        self
    }

    async fn post_form(&self, url: &str, body: String) -> Result<(u16, Value), ExecError> {
        let mut h = GoHeaders::new();
        h.set("Accept", "application/json");
        h.set("Content-Type", "application/x-www-form-urlencoded");
        h.set("User-Agent", "horon-router");
        let up = send(&self.client, url, h, body, Some(HTTP_TIMEOUT))
            .await
            .map_err(|_| auth_error(502, "github-copilot: request failed"))?;
        let status = up.status;
        let raw = read_all(up.body, MAX_ERROR_BODY, false)
            .await
            .map_err(|_| auth_error(502, "github-copilot: failed to read response"))?;
        Ok((status, json_or_empty(&raw)))
    }

    async fn get_json(&self, url: &str, authorization: &str, action: &str) -> Result<Value, ExecError> {
        let up = request(
            &self.client,
            wreq::Method::GET,
            url,
            headers(authorization),
            None,
            Some(HTTP_TIMEOUT),
        )
        .await
        .map_err(|_| auth_error(502, format!("github-copilot: {action} failed")))?;
        let status = up.status;
        let raw = read_all(up.body, MAX_ERROR_BODY, false)
            .await
            .map_err(|_| auth_error(502, format!("github-copilot: {action} failed")))?;
        if !(200..300).contains(&status) {
            return Err(auth_error(
                status,
                format!("github-copilot: {action} failed ({status})"),
            ));
        }
        Ok(json_or_empty(&raw))
    }

    pub async fn request_device_code(&self) -> Result<DeviceCode, ExecError> {
        let (status, v) = self
            .post_form(&self.device_url, form(&[("client_id", CLIENT_ID), ("scope", SCOPE)]))
            .await?;
        if !(200..300).contains(&status) {
            return Err(auth_error(
                status,
                format!("github-copilot: device authorization failed ({status})"),
            ));
        }
        let (Some(device_code), Some(user_code)) = (text(&v, "device_code"), text(&v, "user_code")) else {
            return Err(auth_error(
                502,
                "github-copilot: device authorization response missing required fields",
            ));
        };
        let Some(url) = verify_url(&user_code) else {
            return Err(auth_error(
                502,
                "github-copilot: device flow returned an invalid user code",
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

    /// GitHub wants the wait before every poll, not after.
    pub async fn poll(&self, code: &DeviceCode) -> Result<GithubGrant, ExecError> {
        let deadline = tokio::time::Instant::now() + code.expires_in;
        let mut wait = self
            .min_interval
            .unwrap_or_else(|| code.interval.max(Duration::from_secs(1)));
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return Err(auth_error(408, "github-copilot: device flow timed out"));
            }
            tokio::time::sleep(wait.min(left)).await;
            let body = form(&[
                ("client_id", CLIENT_ID),
                ("device_code", &code.device_code),
                ("grant_type", DEVICE_GRANT),
            ]);
            let (status, v) = self.post_form(&self.token_url, body).await?;
            if let Some(access) = text(&v, "access_token").filter(|_| (200..300).contains(&status)) {
                return Ok(GithubGrant {
                    access,
                    refresh: text(&v, "refresh_token"),
                });
            }
            match v.get("error").and_then(Value::as_str).unwrap_or_default() {
                "authorization_pending" => {}
                "slow_down" => {
                    let asked = v.get("interval").and_then(Value::as_u64).map(Duration::from_secs);
                    wait = (wait + Duration::from_secs(5)).max(asked.unwrap_or_default());
                }
                "expired_token" => return Err(auth_error(400, "github-copilot: device authorization expired")),
                "access_denied" => return Err(auth_error(403, "github-copilot: device authorization denied")),
                "" => {
                    return Err(auth_error(
                        status,
                        format!("github-copilot: device token poll failed ({status})"),
                    ));
                }
                other => return Err(auth_error(400, format!("github-copilot: device flow failed ({other})"))),
            }
        }
    }

    /// Only an expiring-token app issues a refresh token (`ghr_`); a classic app's `gho_`
    /// access token never expires and is traded again directly.
    pub async fn refresh_github(&self, refresh_token: &str) -> Result<GithubGrant, ExecError> {
        let body = form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ]);
        let (status, v) = self.post_form(&self.token_url, body).await?;
        if !(200..300).contains(&status) {
            let code = v.get("error").and_then(Value::as_str).unwrap_or_default();
            return Err(auth_error(
                status,
                format!("github-copilot: token refresh failed ({status}) {code}")
                    .trim()
                    .to_owned(),
            ));
        }
        let Some(access) = text(&v, "access_token") else {
            return Err(auth_error(502, "github-copilot: token refresh missing access token"));
        };
        Ok(GithubGrant {
            access,
            refresh: Some(text(&v, "refresh_token").unwrap_or_else(|| refresh_token.to_owned())),
        })
    }

    pub async fn exchange(&self, github_access: &str, now: i64) -> Result<Session, ExecError> {
        let v = self
            .get_json(&self.copilot_url, &format!("token {github_access}"), "token exchange")
            .await?;
        let Some(copilot_token) = text(&v, "token") else {
            return Err(auth_error(502, "github-copilot: token exchange missing token"));
        };
        let expires_at = v
            .get("expires_at")
            .and_then(Value::as_i64)
            .map(|t| t - EXPIRY_SKEW_SECS)
            .or_else(|| {
                v.get("refresh_in")
                    .and_then(Value::as_i64)
                    .filter(|n| *n > 0)
                    .map(|n| now + n - EXPIRY_SKEW_SECS)
            })
            .unwrap_or(now + FALLBACK_TTL_SECS - EXPIRY_SKEW_SECS);
        let api_base = v
            .get("endpoints")
            .and_then(|e| e.get("api"))
            .and_then(Value::as_str)
            .and_then(validate_api_base)
            .unwrap_or_else(|| DEFAULT_API_BASE.to_owned());
        Ok(Session {
            copilot_token,
            expires_at,
            api_base,
        })
    }

    /// Without a stable identity a second account would overwrite the first one's file.
    pub async fn identity(&self, github_access: &str) -> Result<Identity, ExecError> {
        let v = self
            .get_json(&self.user_url, &format!("Bearer {github_access}"), "identity lookup")
            .await?;
        let account_id = v
            .get("id")
            .and_then(Value::as_i64)
            .map(|n| n.to_string())
            .or_else(|| text(&v, "login"))
            .ok_or_else(|| auth_error(502, "github-copilot: could not verify the GitHub account identity"))?;
        Ok(Identity {
            account_id,
            email: text(&v, "email").filter(|e| e.contains('@')),
        })
    }
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

/// `durable` is what survives a Copilot token expiry: the `ghr_` refresh token, or for a
/// classic app the GitHub access token itself.
pub fn login_record(durable: &str, session: &Session, identity: &Identity, now_ms: i64) -> LoginRecord {
    let safe: String = identity
        .account_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut metadata = Map::new();
    for (k, v) in [
        ("type", json!(PROVIDER)),
        ("access_token", json!(session.copilot_token)),
        ("refresh_token", json!(durable)),
        ("expired", json!(rfc3339(session.expires_at))),
        ("base_url", json!(session.api_base)),
        ("account_id", json!(identity.account_id)),
        ("timestamp", json!(now_ms)),
        ("disabled", json!(false)),
    ] {
        metadata.insert(k.into(), v);
    }
    if let Some(email) = &identity.email {
        metadata.insert("email".into(), json!(email));
    }
    LoginRecord {
        file_name: format!("github-copilot-{safe}.json"),
        metadata,
        label: identity.email.clone().unwrap_or_else(|| identity.account_id.clone()),
    }
}

pub fn write_login(auth_dir: &std::path::Path, record: &LoginRecord) -> Result<std::path::PathBuf, ExecError> {
    let path = auth_dir.join(&record.file_name);
    write_private(&path, encode_credential(&record.metadata).as_bytes())
        .map_err(|_| auth_error(500, "github-copilot: cannot write credential file"))?;
    Ok(path)
}

pub async fn complete_login(auth: &CopilotAuth, code: &DeviceCode) -> Result<LoginRecord, ExecError> {
    let grant = auth.poll(code).await?;
    let now = chrono::Utc::now();
    let (session, identity) = tokio::try_join!(
        auth.exchange(&grant.access, now.timestamp()),
        auth.identity(&grant.access)
    )?;
    let durable = grant.refresh.as_deref().unwrap_or(&grant.access);
    Ok(login_record(durable, &session, &identity, now.timestamp_millis()))
}

pub async fn login(cfg: &Config, no_browser: bool) -> Result<std::path::PathBuf, ExecError> {
    let proxy = cfg
        .document
        .get("requests")
        .and_then(|r| r.get("proxy-url"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let auth = CopilotAuth::new(GoClients::new(Hooks::default()).get(&Proxy::parse(proxy)));
    println!("Starting GitHub Copilot authentication...");
    let code = auth.request_device_code().await?;
    print!(
        "\nTo authenticate, please visit:\n{}\n\nEnter code: {}\n\n",
        code.url, code.user_code
    );
    if !no_browser && open_browser(&code.url) {
        println!("Browser opened automatically.");
    }
    println!("Waiting for authorization...");
    let record = complete_login(&auth, &code).await?;
    let path = write_login(&cfg.auth_dir, &record)?;
    println!("Authentication saved to {}", path.display());
    println!("Authenticated as {}", record.label);
    Ok(path)
}

fn expiry(c: &Credential) -> Option<chrono::DateTime<chrono::Utc>> {
    c.str("expired")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.to_utc())
}

fn durable_grant(c: &Credential) -> Option<&str> {
    c.str("refresh_token").filter(|s| !s.trim().is_empty())
}

fn api_base(c: &Credential) -> String {
    let attr = c.attributes.get("base_url").map(String::as_str).unwrap_or_default();
    let meta = c.str("base_url").unwrap_or_default();
    [attr, meta]
        .into_iter()
        .find_map(validate_api_base)
        .unwrap_or_else(|| DEFAULT_API_BASE.to_owned())
}

pub struct CopilotExecutor {
    clients: GoClients,
    chat: OpenAICompatExecutor,
    origin: Option<String>,
}

impl Default for CopilotExecutor {
    fn default() -> Self {
        Self::with_client(default_client())
    }
}

impl CopilotExecutor {
    pub fn with_client(client: wreq::Client) -> Self {
        Self {
            clients: GoClients::with_default(client.clone()),
            chat: OpenAICompatExecutor::with_client(client),
            origin: None,
        }
    }

    pub fn with_origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.to_owned());
        self
    }

    pub fn needs_prepare_at(&self, credential: &Credential, _cfg: &Config, now: chrono::DateTime<chrono::Utc>) -> bool {
        durable_grant(credential).is_some() && expiry(credential).is_none_or(|t| t <= now)
    }

    pub async fn prepare(&self, credential: &Credential, cfg: &Config) -> Result<MetadataPatch, ExecError> {
        let Some(durable) = durable_grant(credential) else {
            return Ok(MetadataPatch::default());
        };
        let mut auth = CopilotAuth::new(self.clients.get(&Proxy::effective(credential, cfg)));
        if let Some(origin) = &self.origin {
            auth = auth.with_origin(origin);
        }
        let (github_access, durable) = if durable.starts_with("ghr_") {
            let grant = auth.refresh_github(durable).await?;
            let next = grant.refresh.unwrap_or_else(|| durable.to_owned());
            (grant.access, next)
        } else {
            (durable.to_owned(), durable.to_owned())
        };
        let session = auth.exchange(&github_access, chrono::Utc::now().timestamp()).await?;
        let mut patch = MetadataPatch::default();
        for (k, v) in [
            ("access_token", json!(session.copilot_token)),
            ("refresh_token", json!(durable)),
            ("expired", json!(rfc3339(session.expires_at))),
            ("base_url", json!(session.api_base)),
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
        if req.operation == Operation::CountTokens {
            return self.chat.execute(credential, req, cfg).await;
        }
        let model = hr_common::thinking::parse_suffix(&req.model).model_name;
        if RESPONSES_ONLY.iter().any(|m| m.eq_ignore_ascii_case(model.trim())) {
            return Err(ExecError::local(
                400,
                FailureScope::Request,
                format!(
                    "github-copilot: model {model} is served only over the Responses API, which this executor does not speak yet"
                ),
            ));
        }
        let key = credential.str("access_token").map(str::trim).unwrap_or_default();
        if key.is_empty() {
            return Err(ExecError::local(
                401,
                FailureScope::Credential,
                "github-copilot executor: missing access token",
            ));
        }
        let extra = [
            ("Editor-Version", "horon-router/0.1.0".to_owned()),
            ("Editor-Plugin-Version", "horon-router/0.1.0".to_owned()),
            ("Copilot-Integration-Id", "vscode-chat".to_owned()),
        ];
        self.chat
            .chat_with(credential, req, cfg, &api_base(credential), key, &extra)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bearer_only_goes_to_githubcopilot_hosts() {
        assert_eq!(
            validate_api_base("https://api.githubcopilot.com/x?y").as_deref(),
            Some("https://api.githubcopilot.com")
        );
        assert_eq!(
            validate_api_base("https://API.Business.GitHubCopilot.com").as_deref(),
            Some("https://api.business.githubcopilot.com")
        );
        for bad in [
            "http://api.githubcopilot.com",
            "https://evil.com",
            "https://githubcopilot.com.evil.com",
            "https://u:p@api.githubcopilot.com",
            "https://api.githubcopilot.com:8443",
            "https://127.0.0.1",
            "https://localhost",
            "",
        ] {
            assert_eq!(validate_api_base(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_verify_url_is_built_here_never_taken_from_the_server() {
        assert_eq!(
            verify_url("AB12-CD34").as_deref(),
            Some("https://github.com/login/device?user_code=AB12-CD34")
        );
        assert_eq!(verify_url("a b"), None);
        assert_eq!(verify_url("x&y=1"), None);
        assert_eq!(verify_url(""), None);
    }

    #[test]
    fn the_credential_keeps_the_durable_grant_and_a_per_account_file() {
        let session = Session {
            copilot_token: "tid=1".into(),
            expires_at: 100,
            api_base: DEFAULT_API_BASE.into(),
        };
        let identity = Identity {
            account_id: "42".into(),
            email: Some("a@b.test".into()),
        };
        let r = login_record("gho_x", &session, &identity, 5);
        assert_eq!(r.file_name, "github-copilot-42.json");
        assert_eq!(r.metadata["refresh_token"], "gho_x");
        assert_eq!(r.metadata["access_token"], "tid=1");
        assert_eq!(r.metadata["type"], "github-copilot");
        assert_eq!(r.label, "a@b.test");
    }

    fn credential(meta: Value) -> Credential {
        Credential::from_file(
            std::path::Path::new("/a"),
            std::path::Path::new("/a/c.json"),
            meta.as_object().unwrap().clone(),
        )
        .unwrap()
    }

    #[test]
    fn a_stored_api_base_outside_githubcopilot_is_ignored() {
        let c = credential(json!({"type": "github-copilot", "base_url": "https://evil.example"}));
        assert_eq!(api_base(&c), DEFAULT_API_BASE);
        let c = credential(json!({"type": "github-copilot", "base_url": "https://api.enterprise.githubcopilot.com"}));
        assert_eq!(api_base(&c), "https://api.enterprise.githubcopilot.com");
    }

    async fn github_replying(replies: Vec<(u16, String)>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let seen: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let sink = seen.clone();
        tokio::spawn(async move {
            for (status, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 16384];
                let n = socket.read(&mut buf).await.unwrap();
                sink.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn a_rotating_github_grant_is_refreshed_then_traded_for_a_copilot_token() {
        let now = chrono::Utc::now().timestamp();
        let (origin, seen) = github_replying(vec![
            (
                200,
                json!({"access_token": "gho_new", "refresh_token": "ghr_next"}).to_string(),
            ),
            (
                200,
                json!({"token": "tid=2", "expires_at": now + 1800,
                       "endpoints": {"api": "https://evil.example"}})
                .to_string(),
            ),
        ])
        .await;
        let ex = CopilotExecutor::default().with_origin(&origin);
        let c = credential(json!({"type": "github-copilot", "refresh_token": "ghr_old", "expired": rfc3339(now - 5)}));
        let patch = ex.prepare(&c, &Config::default()).await.unwrap();
        assert_eq!(patch.set["access_token"], "tid=2");
        assert_eq!(patch.set["refresh_token"], "ghr_next");
        assert_eq!(
            patch.set["base_url"], DEFAULT_API_BASE,
            "a hostile endpoint must be ignored"
        );
        let seen = seen.lock().unwrap();
        assert!(seen[0].contains("refresh_token=ghr_old"), "{}", seen[0]);
        assert!(
            seen[1].to_ascii_lowercase().contains("authorization: token gho_new"),
            "{}",
            seen[1]
        );
    }

    #[tokio::test]
    async fn a_classic_github_token_is_traded_directly_and_kept() {
        let now = chrono::Utc::now().timestamp();
        let (origin, seen) = github_replying(vec![(
            200,
            json!({"token": "tid=3", "expires_at": now + 1800}).to_string(),
        )])
        .await;
        let ex = CopilotExecutor::default().with_origin(&origin);
        let c =
            credential(json!({"type": "github-copilot", "refresh_token": "gho_classic", "expired": rfc3339(now - 5)}));
        let patch = ex.prepare(&c, &Config::default()).await.unwrap();
        assert_eq!(patch.set["refresh_token"], "gho_classic");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_token_needs_preparing_once_it_expires() {
        let ex = CopilotExecutor::default();
        let cfg = Config::default();
        let now = chrono::Utc::now();
        let expired = credential(
            json!({"type": "github-copilot", "refresh_token": "gho_x", "expired": rfc3339(now.timestamp() - 5)}),
        );
        let fresh = credential(
            json!({"type": "github-copilot", "refresh_token": "gho_x", "expired": rfc3339(now.timestamp() + 600)}),
        );
        let bare = credential(json!({"type": "github-copilot", "expired": rfc3339(now.timestamp() - 5)}));
        assert!(ex.needs_prepare_at(&expired, &cfg, now));
        assert!(!ex.needs_prepare_at(&fresh, &cfg, now));
        assert!(!ex.needs_prepare_at(&bare, &cfg, now));
    }
}
