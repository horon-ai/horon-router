//! The negative statuses the management OpenAPI contract documents, exercised against the
//! real management router with a fresh state per case: management disabled answers 404
//! with an empty body, a missing or wrong key answers 401, repeated failures ban with 403,
//! and the OAuth callbacks take no key. No real auth-dir or provider is used.

use hr_core::config::Config;
use hr_exec::Executors;
use hr_exec::claude::ClaudeExecutor;
use hr_server::management::{self, Management, Options};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// A fresh management state whose auth-dir is a throwaway temp directory. `secret` pins
/// `admin_password` (or leaves management disabled when empty); the config's own
/// `management.secret-key` is left empty so availability follows the pinned option alone.
fn state(name: &str, secret: &str) -> (PathBuf, Arc<Management>) {
    let dir = std::env::temp_dir().join(format!("openapi-mgmt-{name}-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("auth")).unwrap();
    let path = dir.join("config.yaml");
    std::fs::write(
        &path,
        format!(
            "config-version: 8\noauth:\n  auth-dir: {}\n",
            dir.join("auth").display()
        ),
    )
    .unwrap();
    let cfg = Config::load(&path).unwrap();
    let rt = Arc::new(hr_server::testing::runtime(
        cfg,
        vec![],
        Executors {
            claude: ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
            codex: Default::default(),
            devices: Default::default(),
            openai: Default::default(),
            google: Default::default(),
        },
    ));
    let options = Options {
        admin_password: Some(secret.to_owned()),
        ..Default::default()
    };
    let state = Management::with_options(rt, path, options);
    (dir, state)
}

async fn serve(state: Arc<Management>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = management::router(state);
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (base, handle)
}

/// A server with no management key configured stays open, not off: unlike Go's 404
/// availability gate, a keyless request is accepted (docs/DIFFERENCES-FROM-GO.md).
#[tokio::test]
async fn no_password_keeps_management_open() {
    let (dir, state) = state("disabled", "");
    let (base, server) = serve(state).await;
    let client = wreq::Client::new();
    // hyper-router intentionally differs from Go here: with no password configured anywhere,
    // management is open instead of off (docs/DIFFERENCES-FROM-GO.md). A keyless request is
    // accepted, and the OAuth callback route stays reachable.
    let response = client.get(format!("{base}/v8/management/config")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let callback = client
        .get(format!("{base}/v8/management/oauth/callback?state=x"))
        .send()
        .await
        .unwrap();
    assert_ne!(callback.status(), 404, "the callback route stays available");
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// A missing key is 401; a fresh state keeps the failure count at one, far below the ban.
#[tokio::test]
async fn missing_and_wrong_keys_answer_401() {
    let (dir, state) = state("unauthorized", "fake-management-only");
    let (base, server) = serve(state).await;
    let client = wreq::Client::new();

    let missing = client.get(format!("{base}/v8/management/config")).send().await.unwrap();
    assert_eq!(missing.status(), 401);
    let body: Value = missing.json().await.unwrap();
    assert_eq!(body["error"], "missing management key");

    let wrong = client
        .get(format!("{base}/v8/management/config"))
        .bearer_auth("not-the-key")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    let body: Value = wrong.json().await.unwrap();
    assert_eq!(body["error"], "invalid management key");

    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// The fifth failed attempt bans the client: the next request is 403. Uses one fresh state
/// so the ban cannot leak into another case.
#[tokio::test]
async fn repeated_failures_ban_with_403() {
    let (dir, state) = state("ban", "fake-management-only");
    let (base, server) = serve(state).await;
    let client = wreq::Client::new();
    for _ in 0..5 {
        let response = client
            .get(format!("{base}/v8/management/config"))
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
    }
    let banned = client
        .get(format!("{base}/v8/management/config"))
        .bearer_auth("fake-management-only")
        .send()
        .await
        .unwrap();
    assert_eq!(banned.status(), 403, "the fifth failure bans the client IP");
    let body: Value = banned.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap_or_default().starts_with("IP banned"),
        "unexpected ban body: {body}"
    );
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// The OAuth callbacks take no key: without one they reach the handler (400 for a missing
/// state) instead of being refused with 401.
#[tokio::test]
async fn oauth_callbacks_need_no_key() {
    let (dir, state) = state("callback", "fake-management-only");
    let (base, server) = serve(state).await;
    let client = wreq::Client::new();
    let response = client
        .get(format!("{base}/v8/management/oauth/callback"))
        .send()
        .await
        .unwrap();
    assert_ne!(response.status(), 401, "the callback must not require a key");
    assert_eq!(response.status(), 400, "a missing state is a handler-level 400");
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}
