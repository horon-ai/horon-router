//! Google Antigravity (Cloud Code Assist) sign-in, after opencodex's
//! `src/oauth/google-antigravity.ts`. The client id and secret are the public values of the
//! Antigravity desktop client, not user secrets. Requests send that client's `User-Agent`,
//! which the backend requires for newer models; that is impersonation of a first-party
//! client and may breach Google's terms (see `docs/DIFFERENCES-FROM-GO.md`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use horon_core::config::Config;
use horon_core::exec::{ExecError, FailureScope};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::kimi_auth::{encode_credential, form, open_browser, write_private};
use crate::proxy::{GoClients, GoHeaders, Hooks, MAX_ERROR_BODY, Proxy, read_all, send};

pub const PROVIDER: &str = "antigravity";
pub const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const CLIENT_SECRET: &str = "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";
pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const PROD_API: &str = "https://cloudcode-pa.googleapis.com";
pub const DAILY_API: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const API_VERSION: &str = "v1internal";
pub const IDE_VERSION: &str = "2.5.5";
pub const CALLBACK_PORT: u16 = 51121;
const CALLBACK_PATH: &str = "/callback";
const SCOPES: [&str; 5] = [
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];
pub const REFRESH_LEAD: Duration = Duration::from_secs(5 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const ONBOARD_ATTEMPTS: usize = 5;
const ONBOARD_POLL: Duration = Duration::from_secs(2);
const MAX_REQUEST: usize = 64 << 10;

fn auth_error(status: u16, message: impl Into<String>) -> ExecError {
    ExecError::local(status, FailureScope::Transport, message)
}

pub fn user_agent() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("antigravity/ide/{IDE_VERSION} (os_type={os}; arch={arch}; aidev_client; auth_method=oauth)")
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

fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map_or_else(String::new, |t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

pub fn project_id(data: &Value) -> Option<String> {
    ["cloudaicompanionProject", "projectId", "project"]
        .iter()
        .find_map(|key| match data.get(*key)? {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Object(o) => o
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            _ => None,
        })
}

fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn email_of(access: &str, id_token: Option<&str>) -> Option<String> {
    id_token
        .and_then(claims)
        .or_else(|| claims(access))
        .and_then(|c| text(&c, "email"))
        .map(|e| e.to_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: i64,
    pub email: Option<String>,
}

pub fn parse_tokens(body: &Value, refresh_fallback: &str, now: i64) -> Result<Tokens, ExecError> {
    let Some(access) = text(body, "access_token") else {
        return Err(auth_error(
            502,
            "antigravity: token response did not include an access token",
        ));
    };
    let refresh = text(body, "refresh_token").unwrap_or_else(|| refresh_fallback.to_owned());
    if refresh.is_empty() {
        return Err(auth_error(
            502,
            "antigravity: token response did not include a refresh token",
        ));
    }
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .unwrap_or(3600);
    let email = email_of(&access, body.get("id_token").and_then(Value::as_str));
    Ok(Tokens {
        access,
        refresh,
        expires_at: now + expires_in,
        email,
    })
}

#[derive(Clone)]
pub struct AntigravityAuth {
    client: wreq::Client,
    token_url: String,
    prod_api: String,
    daily_api: String,
    poll: Duration,
}

impl AntigravityAuth {
    pub fn new(client: wreq::Client) -> Self {
        Self {
            client,
            token_url: TOKEN_ENDPOINT.to_owned(),
            prod_api: PROD_API.to_owned(),
            daily_api: DAILY_API.to_owned(),
            poll: ONBOARD_POLL,
        }
    }

    pub fn with_endpoints(mut self, token_url: &str, api: &str) -> Self {
        self.token_url = token_url.to_owned();
        self.prod_api = api.trim_end_matches('/').to_owned();
        self.daily_api = api.trim_end_matches('/').to_owned();
        self.poll = Duration::from_millis(10);
        self
    }

    async fn post(
        &self,
        url: &str,
        content_type: &str,
        headers: GoHeaders,
        body: String,
    ) -> Result<(u16, Value), ExecError> {
        let mut h = headers;
        h.set("Content-Type", content_type);
        let up = send(&self.client, url, h, body, Some(HTTP_TIMEOUT))
            .await
            .map_err(|_| auth_error(502, "antigravity: request failed"))?;
        let status = up.status;
        let raw = read_all(up.body, MAX_ERROR_BODY, false)
            .await
            .map_err(|_| auth_error(502, "antigravity: failed to read response"))?;
        Ok((status, json_or_empty(&raw)))
    }

    async fn token(&self, pairs: &[(&str, &str)], fallback: &str) -> Result<Tokens, ExecError> {
        let mut h = GoHeaders::new();
        h.set("Accept", "application/json");
        let (status, v) = self
            .post(&self.token_url, "application/x-www-form-urlencoded", h, form(pairs))
            .await?;
        if !(200..300).contains(&status) {
            return Err(auth_error(
                status,
                format!("antigravity: token request failed: {status}"),
            ));
        }
        parse_tokens(&v, fallback, chrono::Utc::now().timestamp())
    }

    pub async fn exchange(&self, code: &str, redirect_uri: &str, verifier: &str) -> Result<Tokens, ExecError> {
        self.token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
            ],
            "",
        )
        .await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<Tokens, ExecError> {
        self.token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("refresh_token", refresh_token),
            ],
            refresh_token,
        )
        .await
    }

    fn api_headers(access: &str) -> GoHeaders {
        let mut h = GoHeaders::new();
        h.set("Authorization", format!("Bearer {access}"));
        h.set("Accept", "*/*");
        h.set("User-Agent", user_agent());
        h
    }

    pub async fn load_project(&self, access: &str) -> Option<String> {
        let url = format!("{}/{API_VERSION}:loadCodeAssist", self.prod_api);
        let body = json!({"metadata": {"ideType": "ANTIGRAVITY"}}).to_string();
        let (status, v) = self
            .post(&url, "application/json", Self::api_headers(access), body)
            .await
            .ok()?;
        (200..300).contains(&status).then(|| project_id(&v)).flatten()
    }

    async fn onboard(&self, access: &str) -> Option<String> {
        let url = format!("{}/{API_VERSION}:onboardUser", self.daily_api);
        let body = json!({
            "tier_id": "free-tier",
            "metadata": {"ide_type": "ANTIGRAVITY", "ide_name": "antigravity", "ide_version": IDE_VERSION},
        })
        .to_string();
        for _ in 0..ONBOARD_ATTEMPTS {
            let answer = self
                .post(&url, "application/json", Self::api_headers(access), body.clone())
                .await;
            match answer {
                Ok((status, v)) if (200..300).contains(&status) => {
                    if v.get("done").and_then(Value::as_bool) == Some(true) {
                        return v.get("response").and_then(project_id);
                    }
                }
                Ok((status, _)) if status != 429 && status < 500 => return None,
                _ => {}
            }
            tokio::time::sleep(self.poll).await;
        }
        None
    }

    pub async fn discover_project(&self, access: &str) -> Option<String> {
        match self.load_project(access).await {
            Some(project) => Some(project),
            None => self.onboard(access).await,
        }
    }
}

pub fn authorize_url(state: &str, challenge: &str, redirect_uri: &str) -> String {
    let mut url = url::Url::parse(AUTH_ENDPOINT).expect("constant URL");
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", &SCOPES.join(" ")),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", state),
    ]);
    url.into()
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Callback {
    pub code: String,
    pub state: String,
    pub error: String,
}

pub fn parse_callback_query(query: &str) -> Callback {
    let mut cb = Callback::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let slot = match key.as_ref() {
            "code" => &mut cb.code,
            "state" => &mut cb.state,
            "error" => &mut cb.error,
            _ => continue,
        };
        *slot = value.into_owned();
    }
    cb
}

pub struct CallbackServer {
    pub port: u16,
    pub results: mpsc::Receiver<Callback>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for CallbackServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl CallbackServer {
    pub async fn start(preferred: u16) -> Result<Self, ExecError> {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", preferred)).await {
            Ok(l) => l,
            Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|_| auth_error(500, "failed to start callback server"))?,
        };
        let port = listener
            .local_addr()
            .map_err(|_| auth_error(500, "failed to start callback server"))?
            .port();
        let (tx, results) = mpsc::channel(1);
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(serve(socket, tx.clone()));
            }
        });
        Ok(Self { port, results, task })
    }
}

async fn serve(mut socket: tokio::net::TcpStream, tx: mpsc::Sender<Callback>) {
    let work = async {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 || buf.len() > MAX_REQUEST {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let head = String::from_utf8_lossy(&buf).into_owned();
        let target = head.lines().next()?.split_whitespace().nth(1)?.to_owned();
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let (status, page) = if path == CALLBACK_PATH {
            let cb = parse_callback_query(query);
            let ok = cb.error.is_empty() && !cb.code.is_empty();
            let _ = tx.try_send(cb);
            if ok {
                (
                    200,
                    "<html><body>Authentication successful. You can close this window.</body></html>",
                )
            } else {
                (
                    400,
                    "<html><body>Authentication failed. Return to the terminal.</body></html>",
                )
            }
        } else {
            (404, "404 page not found\n")
        };
        let response = format!(
            "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            http::StatusCode::from_u16(status)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or(""),
            page.len()
        );
        socket.write_all(response.as_bytes()).await.ok()
    };
    let _ = tokio::time::timeout(Duration::from_secs(10), work).await;
}

pub struct LoginRecord {
    pub file_name: String,
    pub metadata: Map<String, Value>,
    pub label: String,
}

pub fn login_record(tokens: &Tokens, project: &str, now_ms: i64) -> LoginRecord {
    let who = tokens.email.clone().unwrap_or_default();
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
        ("project_id", json!(project)),
        ("timestamp", json!(now_ms)),
        ("disabled", json!(false)),
    ] {
        metadata.insert(k.into(), v);
    }
    if let Some(email) = &tokens.email {
        metadata.insert("email".into(), json!(email));
    }
    LoginRecord {
        file_name: if safe.is_empty() {
            format!("antigravity-{now_ms}.json")
        } else {
            format!("antigravity-{safe}.json")
        },
        metadata,
        label: if who.is_empty() { "Antigravity user".into() } else { who },
    }
}

pub fn write_login(auth_dir: &Path, record: &LoginRecord) -> Result<PathBuf, ExecError> {
    let path = auth_dir.join(&record.file_name);
    write_private(&path, encode_credential(&record.metadata).as_bytes())
        .map_err(|_| auth_error(500, "antigravity: cannot write credential file"))?;
    Ok(path)
}

/// Fails when no project is found: every later request would be rejected without one.
pub async fn complete_login(
    auth: &AntigravityAuth,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<LoginRecord, ExecError> {
    let tokens = auth.exchange(code, redirect_uri, verifier).await?;
    let Some(project) = auth.discover_project(&tokens.access).await else {
        return Err(auth_error(
            403,
            "antigravity: could not discover a Cloud Code Assist project for this account; \
             make sure it has Antigravity access and try again",
        ));
    };
    Ok(login_record(&tokens, &project, chrono::Utc::now().timestamp_millis()))
}

pub async fn login(cfg: &Config, no_browser: bool, callback_port: u16) -> Result<PathBuf, ExecError> {
    let proxy = cfg
        .document
        .get("requests")
        .and_then(|r| r.get("proxy-url"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let auth = AntigravityAuth::new(GoClients::new(Hooks::default()).get(&Proxy::parse(proxy)));
    println!("Starting Antigravity authentication...");
    let (verifier, challenge) = crate::oauth::pkce()?;
    let mut state_bytes = [0u8; 16];
    getrandom::fill(&mut state_bytes).map_err(|_| auth_error(500, "failed to generate state parameter"))?;
    let state: String = state_bytes.iter().map(|b| format!("{b:02x}")).collect();
    let mut server = CallbackServer::start(if callback_port == 0 {
        CALLBACK_PORT
    } else {
        callback_port
    })
    .await?;
    let redirect = redirect_uri(server.port);
    let url = authorize_url(&state, &challenge, &redirect);
    print!("\nTo authenticate, please visit:\n{url}\n\n");
    if !no_browser && open_browser(&url) {
        println!("Browser opened automatically.");
    }
    println!("Waiting for authorization...");
    let callback = tokio::time::timeout(LOGIN_TIMEOUT, server.results.recv())
        .await
        .map_err(|_| auth_error(408, "antigravity: OAuth callback timed out"))?
        .ok_or_else(|| auth_error(500, "antigravity: callback server stopped"))?;
    if !callback.error.is_empty() {
        return Err(auth_error(403, format!("antigravity: OAuth error: {}", callback.error)));
    }
    if callback.state != state {
        return Err(auth_error(400, "antigravity: OAuth state mismatch"));
    }
    let record = complete_login(&auth, &callback.code, &redirect, &verifier).await?;
    let path = write_login(&cfg.auth_dir, &record)?;
    println!("Authentication saved to {}", path.display());
    println!("Authenticated as {}", record.label);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_ids_come_from_any_of_the_three_shapes() {
        assert_eq!(
            project_id(&json!({"cloudaicompanionProject": "p1"})).as_deref(),
            Some("p1")
        );
        assert_eq!(project_id(&json!({"project": {"id": "p2"}})).as_deref(), Some("p2"));
        assert_eq!(
            project_id(&json!({"projectId": "p3", "project": "x"})).as_deref(),
            Some("p3")
        );
        assert_eq!(project_id(&json!({"cloudaicompanionProject": ""})), None);
        assert_eq!(project_id(&json!({})), None);
    }

    #[test]
    fn tokens_keep_the_old_refresh_token_when_google_sends_none() {
        let now = 100;
        let t = parse_tokens(&json!({"access_token": "a", "expires_in": 60}), "old", now).unwrap();
        assert_eq!((t.refresh.as_str(), t.expires_at), ("old", 160));
        assert!(parse_tokens(&json!({"access_token": "a"}), "", now).is_err());
        assert!(parse_tokens(&json!({"refresh_token": "r"}), "", now).is_err());
        assert_eq!(
            parse_tokens(&json!({"access_token": "a", "refresh_token": "r"}), "", now)
                .unwrap()
                .expires_at,
            3700
        );
    }

    #[test]
    fn the_email_is_read_from_the_id_token_first() {
        let enc = |v: Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        let jwt = |email: &str| format!("h.{}.s", enc(json!({"email": email})));
        let t = parse_tokens(
            &json!({"access_token": jwt("A@x.test"), "refresh_token": "r", "id_token": jwt("B@Y.test")}),
            "",
            0,
        )
        .unwrap();
        assert_eq!(t.email.as_deref(), Some("b@y.test"));
        let t = parse_tokens(&json!({"access_token": jwt("A@x.test"), "refresh_token": "r"}), "", 0).unwrap();
        assert_eq!(t.email.as_deref(), Some("a@x.test"));
    }

    #[test]
    fn the_authorize_url_asks_for_offline_pkce_access() {
        let url = authorize_url("st", "ch", &redirect_uri(51121));
        let parsed = url::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:51121/callback");
        assert_eq!(q["state"], "st");
        assert!(q["scope"].contains("cloud-platform"));
    }

    #[test]
    fn the_callback_query_is_decoded() {
        let cb = parse_callback_query("code=4%2Fabc&state=s1&scope=x");
        assert_eq!(
            (cb.code.as_str(), cb.state.as_str(), cb.error.as_str()),
            ("4/abc", "s1", "")
        );
        assert_eq!(parse_callback_query("error=access_denied").error, "access_denied");
    }

    #[test]
    fn the_record_is_keyed_by_the_email() {
        let tokens = Tokens {
            access: "a".into(),
            refresh: "r".into(),
            expires_at: 0,
            email: Some("U@x.test".into()),
        };
        let r = login_record(&tokens, "proj-1", 5);
        assert_eq!(r.file_name, "antigravity-U@x.test.json");
        assert_eq!(r.metadata["project_id"], "proj-1");
        assert_eq!(r.metadata["type"], "antigravity");
    }
}
