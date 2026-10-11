//! Command Code sign-in (no Go counterpart; follows opencodex's `src/oauth/command-code.ts`).
//!
//! The studio page signs the user in and POSTs `{apiKey, state, userId, userName, keyName}`
//! to a loopback callback; that API key is the credential and never expires. An existing
//! Command Code CLI login (`~/.commandcode/auth.json`) or a pasted key works too, and every
//! key is checked against `/alpha/whoami` before it is saved.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hr_core::exec::{ExecError, FailureScope};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::command_code::DEFAULT_BASE_URL;
use crate::kimi_auth::{encode_credential, open_browser, write_private};
use crate::proxy::{GoClients, GoHeaders, Hooks, MAX_ERROR_BODY, Proxy, read_all, request};

pub const STUDIO_URL: &str = "https://commandcode.ai";
pub const CALLBACK_PORT: u16 = 5959;
const CALLBACK_PATH: &str = "/callback";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST: usize = 64 << 10;

fn auth_error(message: impl Into<String>) -> ExecError {
    ExecError::local(502, FailureScope::Transport, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub user_id: String,
    pub user_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub api_key: String,
    pub user_id: String,
    pub user_name: String,
    pub key_name: String,
}

pub struct LoginRecord {
    pub file_name: String,
    pub metadata: Map<String, Value>,
    pub label: String,
}

pub fn auth_url(port: u16, state: &str) -> String {
    let callback = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    format!(
        "{STUDIO_URL}/studio/auth/cli?callback={}&state={}",
        enc(&callback),
        enc(state)
    )
}

pub fn random_state() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn parse_callback(body: &[u8], state: &str) -> Result<Callback, String> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "Command Code callback must be an object".to_owned())?;
    if !value.is_object() {
        return Err("Command Code callback must be an object".into());
    }
    if value.get("state").and_then(Value::as_str) != Some(state) {
        return Err("Command Code OAuth state mismatch".into());
    }
    let field = |k: &str| text(&value, k).ok_or_else(|| format!("Command Code callback missing {k}"));
    Ok(Callback {
        api_key: field("apiKey")?,
        user_id: field("userId")?,
        user_name: field("userName")?,
        key_name: field("keyName")?,
    })
}

pub async fn whoami(client: &wreq::Client, base_url: &str, key: &str) -> Option<Identity> {
    let mut headers = GoHeaders::new();
    headers.set("Authorization", format!("Bearer {key}"));
    headers.set("Accept", "application/json");
    let url = format!("{}/alpha/whoami", base_url.strip_suffix('/').unwrap_or(base_url));
    let upstream = request(client, wreq::Method::GET, &url, headers, None, Some(HTTP_TIMEOUT))
        .await
        .ok()?;
    if !(200..300).contains(&upstream.status) {
        return None;
    }
    let body = read_all(upstream.body, MAX_ERROR_BODY, false).await.ok()?;
    let value: Value = serde_json::from_slice(&body).ok()?;
    let value = value.get("data").filter(|d| d.is_object()).unwrap_or(&value);
    let user = value.get("user")?;
    Some(Identity {
        user_id: text(user, "id")?,
        user_name: text(user, "userName")?,
    })
}

fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|p| !p.as_os_str().is_empty())
}

pub fn local_cli_key() -> Option<String> {
    let raw = std::fs::read(home_dir()?.join(".commandcode").join("auth.json")).ok()?;
    text(&serde_json::from_slice::<Value>(&raw).ok()?, "apiKey")
}

pub fn login_record(key: &str, identity: &Identity, key_name: &str, now_ms: i64) -> LoginRecord {
    let safe: String = identity
        .user_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut metadata = Map::new();
    for (k, v) in [
        ("type", json!("command-code")),
        ("api_key", json!(key)),
        ("user_id", json!(identity.user_id)),
        ("user_name", json!(identity.user_name)),
        ("label", json!(identity.user_name)),
        ("key_name", json!(key_name)),
        ("base_url", json!(DEFAULT_BASE_URL)),
        ("timestamp", json!(now_ms)),
        ("disabled", json!(false)),
    ] {
        metadata.insert(k.into(), v);
    }
    LoginRecord {
        file_name: format!("command-code-{safe}.json"),
        metadata,
        label: identity.user_name.clone(),
    }
}

pub fn write_login(auth_dir: &Path, record: &LoginRecord) -> Result<PathBuf, ExecError> {
    let path = auth_dir.join(&record.file_name);
    write_private(&path, encode_credential(&record.metadata).as_bytes())
        .map_err(|_| auth_error("command-code: cannot write credential file"))?;
    Ok(path)
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
    pub async fn start(state: &str, preferred: u16) -> Result<Self, ExecError> {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", preferred)).await {
            Ok(l) => l,
            Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|_| auth_error("failed to start callback server"))?,
        };
        let port = listener
            .local_addr()
            .map_err(|_| auth_error("failed to start callback server"))?
            .port();
        let (tx, results) = mpsc::channel(1);
        let state = state.to_owned();
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(serve(socket, state.clone(), tx.clone()));
            }
        });
        Ok(Self { port, results, task })
    }
}

async fn serve(mut socket: tokio::net::TcpStream, state: String, tx: mpsc::Sender<Callback>) {
    let work = async {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 || buf.len() > MAX_REQUEST {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut lines = head.split("\r\n");
        let mut first = lines.next()?.split_whitespace();
        let (method, path) = (first.next()?.to_owned(), first.next()?.split('?').next()?.to_owned());
        let header = |name: &str| {
            head.split("\r\n")
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_owned())
        };
        let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        if length > MAX_REQUEST {
            return None;
        }
        while buf.len() < head_end + length {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let origin = header("origin")
            .filter(|o| o == STUDIO_URL)
            .unwrap_or_else(|| STUDIO_URL.to_owned());
        let (status, body) = match (method.as_str(), path.as_str()) {
            ("OPTIONS", _) => (204, String::new()),
            (_, p) if p != CALLBACK_PATH => (404, r#"{"success":false,"error":"Not found"}"#.to_owned()),
            ("POST", _) => match parse_callback(&buf[head_end..head_end + length], &state) {
                Ok(callback) => {
                    let _ = tx.try_send(callback);
                    (200, r#"{"success":true}"#.to_owned())
                }
                Err(e) => (400, json!({"success": false, "error": e}).to_string()),
            },
            _ => (405, r#"{"success":false,"error":"Method not allowed"}"#.to_owned()),
        };
        let response = format!(
            "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            http::StatusCode::from_u16(status)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or(""),
            body.len()
        );
        socket.write_all(response.as_bytes()).await.ok()
    };
    let _ = tokio::time::timeout(Duration::from_secs(10), work).await;
}

pub async fn login(cfg: &hr_core::config::Config, no_browser: bool, callback_port: u16) -> Result<PathBuf, ExecError> {
    let proxy = cfg
        .document
        .get("requests")
        .and_then(|r| r.get("proxy-url"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let client = GoClients::new(Hooks::default()).get(&Proxy::parse(proxy));
    println!("Starting Command Code authentication...");
    if let Some(key) = local_cli_key()
        && let Some(identity) = whoami(&client, DEFAULT_BASE_URL, &key).await
    {
        println!("Imported existing Command Code CLI authentication.");
        return save(&cfg.auth_dir, &key, &identity, "cli");
    }
    let state = random_state().ok_or_else(|| auth_error("failed to generate state parameter"))?;
    let mut server = CallbackServer::start(
        &state,
        if callback_port == 0 {
            CALLBACK_PORT
        } else {
            callback_port
        },
    )
    .await?;
    let url = auth_url(server.port, &state);
    print!("\nTo authenticate, please visit:\n{url}\n\n");
    if !no_browser && open_browser(&url) {
        println!("Browser opened automatically.");
    }
    println!("Waiting for authorization... (or paste a Command Code API key and press Enter)");
    let pasted = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .ok()
            .map(|_| line.trim().to_owned())
    });
    let waited = tokio::select! {
        callback = server.results.recv() => callback.map(|c| (c.api_key, Some(c.user_id), Some(c.user_name), c.key_name)),
        line = pasted => line.ok().flatten().filter(|l| !l.is_empty()).map(|k| (k, None, None, "manual".to_owned())),
        () = tokio::time::sleep(LOGIN_TIMEOUT) => None,
    };
    let (key, user_id, user_name, key_name) =
        waited.ok_or_else(|| auth_error("Command Code OAuth callback timed out"))?;
    let identity = match whoami(&client, DEFAULT_BASE_URL, &key).await {
        Some(identity) => identity,
        None => match (user_id, user_name) {
            (Some(user_id), Some(user_name)) => Identity { user_id, user_name },
            _ => return Err(auth_error("Command Code rejected the API key")),
        },
    };
    save(&cfg.auth_dir, &key, &identity, &key_name)
}

fn save(auth_dir: &Path, key: &str, identity: &Identity, key_name: &str) -> Result<PathBuf, ExecError> {
    let record = login_record(key, identity, key_name, chrono::Utc::now().timestamp_millis());
    let path = write_login(auth_dir, &record)?;
    println!("Authentication saved to {}", path.display());
    println!("Authenticated as {}", record.label);
    println!("Command Code authentication successful!");
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_needs_matching_state_and_every_field() {
        let ok = br#"{"apiKey":"k","state":"s","userId":"u","userName":"n","keyName":"cli"}"#;
        assert_eq!(parse_callback(ok, "s").unwrap().user_id, "u");
        assert!(parse_callback(ok, "other").unwrap_err().contains("state mismatch"));
        let missing = br#"{"apiKey":"k","state":"s","userId":"u","userName":" ","keyName":"cli"}"#;
        assert!(parse_callback(missing, "s").unwrap_err().contains("userName"));
        assert!(parse_callback(b"[]", "s").is_err());
    }

    #[test]
    fn record_is_keyed_by_account_and_url_carries_the_callback() {
        let identity = Identity {
            user_id: "u/1".into(),
            user_name: "alice".into(),
        };
        let record = login_record("k", &identity, "cli", 5);
        assert_eq!(record.file_name, "command-code-u_1.json");
        assert_eq!(record.metadata["type"], "command-code");
        assert_eq!(record.metadata["api_key"], "k");
        let url = auth_url(5959, "st");
        assert!(url.starts_with(
            "https://commandcode.ai/studio/auth/cli?callback=http%3A%2F%2F127.0.0.1%3A5959%2Fcallback&state=st"
        ));
    }
}
