//! Credential-level quota routes against the recorded Go 6fecc6e answers.
//!
//! `tests/fixtures/quota_probe_go.json` holds the credential cases extracted verbatim
//! from the Go plugin-routes capture; the plugin-routing cases (a plugin quota provider,
//! `plugin_id` routing, `quota/providers`) were removed with the plugin host in plan
//! task 2. What is left is what Go answers with an empty provider registry: the
//! credential's declarative `quota_probe`, the command-code endpoint, and the reset
//! answers. The last consumer of the plugin host lived in the deleted plugin test, so
//! this file keeps the general behavior covered with a local mock upstream.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ConnectInfo;
use horon_core::config::Config;
use horon_exec::Executors;
use horon_exec::claude::ClaudeExecutor;
use horon_server::management::{self, Management, Options};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/quota_probe_go.json")).expect("fixture")
}

#[derive(Clone)]
struct Route {
    status: u16,
    body: String,
    location: String,
}

/// The capture's probe upstream: fixed answers by path, no `Date` header.
async fn start_upstream(routes: BTreeMap<String, Route>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let head_end = loop {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                let length: usize = head
                    .split("\r\n")
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                let mut body = buf[head_end + 4..].to_vec();
                while body.len() < length {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..n]);
                }
                let path = head
                    .split("\r\n")
                    .next()
                    .and_then(|l| l.split(' ').nth(1))
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let (status, text, location) = match routes.get(path.as_str()) {
                    Some(r) => (r.status, r.body.clone(), r.location.clone()),
                    None => (404, String::new(), String::new()),
                };
                let mut response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    text.len()
                );
                if !location.is_empty() {
                    response.push_str(&format!("Location: {location}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&text);
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    base
}

/// The binary's composition with every request coming from `127.0.0.1:40000`.
async fn serve(rt: Arc<horon_server::Runtime>, state: Arc<Management>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = horon_server::app(rt, management::router(state)).layer(axum::middleware::from_fn(
        |mut req: axum::extract::Request, next: axum::middleware::Next| async move {
            let peer: SocketAddr = "127.0.0.1:40000".parse().unwrap();
            req.extensions_mut().insert(ConnectInfo(peer));
            next.run(req).await
        },
    ));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn executors() -> Executors {
    Executors {
        claude: ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
        codex: Default::default(),
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_quota_routes_match_the_recorded_go_answers() {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "quota-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&work);
    let auths_dir = work.join("auths");
    std::fs::create_dir_all(&auths_dir).unwrap();

    let f = fixture();
    let routes: BTreeMap<String, Route> = serde_json::from_value::<BTreeMap<String, Value>>(f["upstream"].clone())
        .expect("upstream routes")
        .into_iter()
        .map(|(path, v)| {
            (
                path,
                Route {
                    status: v["Status"].as_u64().expect("status") as u16,
                    body: v["Body"].as_str().unwrap_or_default().to_owned(),
                    location: v["Location"].as_str().unwrap_or_default().to_owned(),
                },
            )
        })
        .collect();
    let upstream = start_upstream(routes).await;

    use std::os::unix::fs::PermissionsExt as _;
    let files: BTreeMap<String, String> = serde_json::from_value(f["auths"].clone()).expect("auths");
    for (name, body) in &files {
        let path = auths_dir.join(name);
        std::fs::write(&path, body.replace("UPSTREAM", &upstream)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let hash = bcrypt::hash("fake-secret", 4).unwrap();
    let config_path = work.join("config.yaml");
    std::fs::write(
        &config_path,
        format!(
            "config-version: 8\nauth-dir: {}\naccess:\n  api-keys: [client-key]\nmanagement:\n  secret-key: {hash}\n",
            auths_dir.display()
        ),
    )
    .unwrap();
    let config = Config::load(&config_path).unwrap();
    let credentials: Vec<_> = horon_core::config::credentials::load(&config)
        .into_iter()
        .map(horon_server::testing::local)
        .collect();
    let mut indexes: BTreeMap<String, String> = BTreeMap::new();
    for c in &credentials {
        if let horon_core::credential::Source::File(path) = &c.source {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            indexes.insert(name, horon_core::config::credentials::auth_index(c));
        }
    }
    assert_eq!(indexes.len(), files.len(), "every fixture credential loaded");

    let rt = Arc::new(horon_server::testing::runtime(config, credentials, executors()));
    let state = Management::with_options(rt.clone(), config_path, Options::default());
    let base = serve(rt, state).await;
    let client = wreq::Client::new();

    let steps = f["steps"].as_array().expect("steps");
    let mut mismatches = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        let path = step["path"].as_str().unwrap();
        let mut body = step["body"].as_str().unwrap_or_default().to_owned();
        if let Some(name) = step["auth_file"].as_str() {
            body = body.replace(&format!("AUTHINDEX({name})"), &indexes[name]);
        }
        let method = wreq::Method::from_bytes(step["method"].as_str().unwrap_or("POST").as_bytes()).unwrap();
        let res = client
            .request(method, format!("{base}{path}"))
            .header("Authorization", "Bearer fake-secret")
            .body(body)
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        let content_type = res
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        let mut got = res.text().await.unwrap();
        for (name, index) in &indexes {
            got = got.replace(index, &format!("AUTHINDEX({name})"));
        }
        let want = &step["result"];
        let want_status = want["status"].as_u64().unwrap() as u16;
        let want_content_type = want["content_type"].as_str().unwrap_or_default();
        let want_body = want["body"].as_str().unwrap_or_default();
        if status != want_status || content_type != want_content_type || got != want_body {
            mismatches.push(format!(
                "step {i} {path}\n  go:   {want_status} {want_content_type} {want_body}\n  rust: {status} {content_type} {got}"
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(
        steps.len(),
        38,
        "the extraction keeps every credential quota case the Go capture covered"
    );
    assert!(
        mismatches.is_empty(),
        "{} step(s) differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// The reset handler answers an empty provider registry: 501 for a credential-level
/// request (the recorded Go answer) and 404 for one that names a plugin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quota_reset_without_providers_keeps_the_recorded_answers() {
    let f = fixture();
    let steps: Vec<&Value> = f["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["path"].as_str().unwrap().contains("/quota/reset"))
        .collect();
    assert_eq!(steps.len(), 2, "the extraction kept both reset cases");
    let statuses: Vec<u64> = steps.iter().map(|s| s["result"]["status"].as_u64().unwrap()).collect();
    assert_eq!(statuses, [501, 404], "credential 501, plugin-named 404");
    let bodies: Vec<&str> = steps.iter().map(|s| s["result"]["body"].as_str().unwrap()).collect();
    assert_eq!(
        bodies[0],
        r#"{"error":"no quota provider available for credential to reset"}"#
    );
    assert_eq!(bodies[1], r#"{"error":"quota provider not found for plugin"}"#);
}
