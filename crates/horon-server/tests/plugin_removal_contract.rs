//! The server's public surface after the native plugin removal (plan task 2).
//!
//! Serves the real composition (`horon_server::app`) on a real TCP listener with an
//! isolated auth-dir, a client key, a management secret and a local mock upstream, then
//! checks the contract the removal has to keep: ordinary client-key auth on the API and
//! realtime paths, a normal request and an SSE stream, the management guard, the
//! credential quota routes, the removed plugin routes answering like any unregistered
//! path, the support header being gone, and the OpenAPI document not promising plugin
//! operations. Nothing leaves the machine.

#![cfg(unix)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ConnectInfo;
use horon_core::config::Config;
use horon_exec::Executors;
use horon_exec::claude::ClaudeExecutor;
use horon_server::management::{self, Management, Options};
use serde_json::Value;

const CHAT: &str = r#"{"id":"c1","object":"chat.completion","created":1,"model":"up-model","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#;
const CHUNK: &str = r#"{"id":"c1","object":"chat.completion.chunk","created":1,"model":"up-model","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":"stop"}]}"#;
const QUOTA: &str = r#"{"subscription":{"plan":"pro"},"summary":[{"key":"k","label":"L","value":1.5,"unit":"USD","format":"currency","currency":"USD"}]}"#;

async fn upstream(req: axum::extract::Request) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    let path = req.uri().path().to_owned();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap();
    let stream = String::from_utf8_lossy(&body).contains(r#""stream":true"#);
    match path.as_str() {
        "/v1/chat/completions" if stream => {
            let sse = format!("data: {CHUNK}\n\ndata: [DONE]\n\n");
            ([("content-type", "text/event-stream")], Bytes::from(sse)).into_response()
        }
        "/v1/chat/completions" => ([("content-type", "application/json")], CHAT).into_response(),
        "/quota" => ([("content-type", "application/json")], QUOTA).into_response(),
        _ => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

struct Case {
    label: String,
    status: u16,
    content_type: String,
    body: String,
    version_header: bool,
    allow: Option<String>,
    support_header: Option<String>,
    expose_headers: String,
}

async fn call(client: &wreq::Client, base: &str, method: &str, path: &str, key: Option<&str>, body: &str) -> Case {
    let mut req = client
        .request(
            wreq::Method::from_bytes(method.as_bytes()).unwrap(),
            format!("{base}{path}"),
        )
        .body(body.to_owned());
    if let Some(key) = key {
        req = req.header("Authorization", key);
    }
    let res = req.send().await.unwrap();
    let header = |name: &str| res.headers().get(name).map(|v| v.to_str().unwrap().to_owned());
    let label = format!("{method} {path} key={}", key.unwrap_or("<none>"));
    let status = res.status().as_u16();
    let content_type = header("content-type").unwrap_or_default();
    let version_header = res.headers().contains_key("x-cpa-version");
    let allow = header("allow");
    let support_header = header("x-cpa-support-plugin");
    let expose_headers = header("access-control-expose-headers").unwrap_or_default();
    let body = res.text().await.unwrap();
    Case {
        label,
        status,
        content_type,
        body,
        version_header,
        allow,
        support_header,
        expose_headers,
    }
}

fn check(failures: &mut Vec<String>, case: Case, want_status: u16, want_body: Option<&str>) -> Case {
    let mut notes = Vec::new();
    if case.status != want_status {
        notes.push(format!("status {} != {}", case.status, want_status));
    }
    if let Some(want) = want_body
        && case.body != want
    {
        notes.push(format!("body {:?} != {:?}", case.body, want));
    }
    if case.support_header.is_some() {
        notes.push(format!("support header present: {:?}", case.support_header));
    }
    if case.expose_headers.contains("X-CPA-SUPPORT-PLUGIN") {
        notes.push("expose headers still name the support header".into());
    }
    if !notes.is_empty() {
        failures.push(format!("{}: {}", case.label, notes.join("; ")));
    }
    println!(
        "{} {} -> {} {}",
        if notes.is_empty() { "PASS" } else { "FAIL" },
        case.label,
        case.status,
        case.body
    );
    case
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_removal_keeps_the_ordinary_surface() {
    let work = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "plugin-removal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&work);
    let auths_dir = work.join("auths");
    std::fs::create_dir_all(&auths_dir).unwrap();

    let mock = serve(axum::Router::new().fallback(upstream)).await;

    use std::os::unix::fs::PermissionsExt as _;
    let probe_path = auths_dir.join("probe.json");
    std::fs::write(
        &probe_path,
        format!(
            r#"{{"type":"codex","email":"probe@example.invalid","access_token":"fake-probe","quota_probe":{{"url":"{mock}/quota"}}}}"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(&probe_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let hash = bcrypt::hash("fake-secret", 4).unwrap();
    let config_path = work.join("config.yaml");
    std::fs::write(
        &config_path,
        format!(
            "config-version: 8\nauth-dir: {}\naccess:\n  api-keys: [client-key]\nmanagement:\n  secret-key: {hash}\n\
             api-keys:\n  openai-compatibility:\n    - name: LocalMock\n      base-url: {mock}/v1\n      \
             models:\n        - name: up-model\n          alias: fast\n      keys:\n        - api-key: sk-fake-upstream\n",
            auths_dir.display()
        ),
    )
    .unwrap();
    let config = Config::load(&config_path).unwrap();
    let credentials = horon_core::config::credentials::load(&config);
    let probe = credentials
        .iter()
        .find(|c| c.id == "probe.json")
        .expect("the probe credential loaded")
        .clone();
    let probe_index = horon_core::config::credentials::auth_index(&probe);
    let openai = credentials
        .iter()
        .find(|c| c.id != "probe.json")
        .unwrap_or_else(|| {
            panic!(
                "the mock provider credential loaded: {:?}",
                credentials.iter().map(|c| (&c.id, &c.provider)).collect::<Vec<_>>()
            )
        })
        .clone();
    let openai_index = horon_core::config::credentials::auth_index(&openai);

    let rt = Arc::new(horon_server::testing::runtime(
        config,
        credentials,
        Executors {
            claude: ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
            codex: Default::default(),
            devices: Default::default(),
            openai: Default::default(),
            google: Default::default(),
        },
    ));
    let state = Management::with_options(rt.clone(), config_path.clone(), Options::default());
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

    let client = wreq::Client::new();
    let mut failures: Vec<String> = Vec::new();

    // Ordinary API client-key auth.
    check(
        &mut failures,
        call(&client, &base, "GET", "/v1/models", Some("Bearer client-key"), "").await,
        200,
        None,
    );
    check(
        &mut failures,
        call(&client, &base, "GET", "/v1/models", Some("Bearer wrong"), "").await,
        401,
        Some(r#"{"error":"Invalid API key"}"#),
    );
    check(
        &mut failures,
        call(&client, &base, "GET", "/v1/models", None, "").await,
        401,
        Some(r#"{"error":"Missing API key"}"#),
    );

    // A normal request and an SSE stream through the local mock.
    let chat = call(
        &client,
        &base,
        "POST",
        "/v1/chat/completions",
        Some("Bearer client-key"),
        r#"{"model":"fast","messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    let chat = check(&mut failures, chat, 200, Some(CHAT));
    assert!(
        chat.content_type.starts_with("application/json"),
        "{}",
        chat.content_type
    );
    let sse = call(
        &client,
        &base,
        "POST",
        "/v1/chat/completions",
        Some("Bearer client-key"),
        r#"{"model":"fast","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    if sse.status != 200
        || !sse.content_type.starts_with("text/event-stream")
        || !sse.body.trim_end().ends_with("data: [DONE]")
    {
        failures.push(format!(
            "SSE stream: {} {} {:?}",
            sse.status, sse.content_type, sse.body
        ));
    }
    println!(
        "{} SSE stream -> {} {}",
        if sse.status == 200 { "PASS" } else { "FAIL" },
        sse.status,
        sse.body
    );
    check(
        &mut failures,
        call(
            &client,
            &base,
            "POST",
            "/v1/chat/completions",
            Some("Bearer wrong"),
            r#"{"model":"fast","messages":[]}"#,
        )
        .await,
        401,
        Some(r#"{"error":"Invalid API key"}"#),
    );

    // Realtime paths keep the OpenAI error envelope.
    for (key, message) in [(None, "Missing API key"), (Some("Bearer wrong"), "Invalid API key")] {
        let case = call(&client, &base, "POST", "/v1/realtime/client_secrets", key, "{}").await;
        let error: Value = serde_json::from_str(&case.body).unwrap_or(Value::Null);
        let envelope = error["error"].clone();
        let ok = case.status == 401
            && envelope["code"] == "invalid_api_key"
            && envelope["message"] == message
            && envelope["type"] == "authentication_error";
        if !ok {
            failures.push(format!("realtime {}: {} {}", case.label, case.status, case.body));
        }
        println!(
            "{} {} -> {} {}",
            if ok { "PASS" } else { "FAIL" },
            case.label,
            case.status,
            case.body
        );
    }

    // Management: the config read and the guard.
    let config_read = call(
        &client,
        &base,
        "GET",
        "/v8/management/config",
        Some("Bearer fake-secret"),
        "",
    )
    .await;
    let document: Value = serde_json::from_str(&config_read.body).unwrap_or(Value::Null);
    if config_read.status != 200 || document.get("plugins").is_some() {
        failures.push(format!(
            "management config: {} {}",
            config_read.status, config_read.body
        ));
    }
    println!(
        "{} GET /v8/management/config -> {} plugins key: {}",
        if config_read.status == 200 && document.get("plugins").is_none() {
            "PASS"
        } else {
            "FAIL"
        },
        config_read.status,
        document.get("plugins").is_some()
    );
    let no_key = check(
        &mut failures,
        call(&client, &base, "GET", "/v8/management/config", None, "").await,
        401,
        Some(r#"{"error":"missing management key"}"#),
    );
    if !no_key.version_header {
        failures.push("management 401 lacks the version headers".into());
    }

    // Credential quota routes: the declarative probe and the empty provider registry.
    let probe_fetch = call(
        &client,
        &base,
        "POST",
        "/v0/management/quota/fetch",
        Some("Bearer fake-secret"),
        &format!(r#"{{"auth_index":"{probe_index}"}}"#),
    )
    .await;
    let quota: Value = serde_json::from_str(&probe_fetch.body).unwrap_or(Value::Null);
    let probe_ok =
        probe_fetch.status == 200 && quota["subscription"]["plan"] == "pro" && quota["summary"][0]["value"] == 1.5;
    if !probe_ok {
        failures.push(format!(
            "quota probe fetch: {} {}",
            probe_fetch.status, probe_fetch.body
        ));
    }
    println!(
        "{} POST /v0/management/quota/fetch (probe) -> {} {}",
        if probe_ok { "PASS" } else { "FAIL" },
        probe_fetch.status,
        probe_fetch.body
    );
    check(
        &mut failures,
        call(
            &client,
            &base,
            "POST",
            "/v0/management/quota/fetch",
            Some("Bearer fake-secret"),
            &format!(r#"{{"auth_index":"{openai_index}"}}"#),
        )
        .await,
        501,
        Some(r#"{"error":"no quota provider available for credential"}"#),
    );
    check(
        &mut failures,
        call(
            &client,
            &base,
            "POST",
            "/v0/management/quota/reset",
            Some("Bearer fake-secret"),
            &format!(r#"{{"auth_index":"{openai_index}"}}"#),
        )
        .await,
        501,
        Some(r#"{"error":"no quota provider available for credential to reset"}"#),
    );
    let reset = call(
        &client,
        &base,
        "POST",
        "/v0/management/reset-quota",
        Some("Bearer fake-secret"),
        &format!(r#"{{"auth_index":"{openai_index}"}}"#),
    )
    .await;
    if reset.status != 200 || !reset.body.contains(r#""status":"ok""#) {
        failures.push(format!("reset-quota: {} {}", reset.status, reset.body));
    }
    println!(
        "{} POST /v0/management/reset-quota -> {} {}",
        if reset.status == 200 { "PASS" } else { "FAIL" },
        reset.status,
        reset.body
    );

    // The removed plugin surface answers like any unregistered path.
    const REMOVED: [(&str, &str); 10] = [
        ("GET", "/v0/management/plugins"),
        ("GET", "/v8/management/plugins"),
        ("DELETE", "/v8/management/plugins/store"),
        ("POST", "/v8/management/plugins/store/x/install"),
        ("GET", "/v0/management/plugins/x/config"),
        ("PATCH", "/v0/management/plugins/x/enabled"),
        ("GET", "/v0/management/plugins/x/quota"),
        ("POST", "/v0/management/plugins/x/quota/reset"),
        ("GET", "/v0/management/quota/providers"),
        ("GET", "/v0/management/plugin-store"),
    ];
    for (method, path) in REMOVED {
        let case = call(&client, &base, method, path, Some("Bearer fake-secret"), "{}").await;
        let ok = case.status == 404 && case.body.is_empty() && case.allow.is_none();
        if !ok {
            failures.push(format!(
                "{method} {path}: {} {:?} allow={:?}",
                case.status, case.body, case.allow
            ));
        }
        println!(
            "{} {method} {path} -> {} allow={:?}",
            if ok { "PASS" } else { "FAIL" },
            case.status,
            case.allow
        );
    }
    let guarded = call(&client, &base, "GET", "/v0/management/plugins", None, "").await;
    if guarded.status != 401 || guarded.body != r#"{"error":"missing management key"}"# {
        failures.push(format!(
            "/v0/management/plugins without a key: {} {}",
            guarded.status, guarded.body
        ));
    }
    println!(
        "{} GET /v0/management/plugins (no key) -> {} {}",
        if guarded.status == 401 { "PASS" } else { "FAIL" },
        guarded.status,
        guarded.body
    );
    // A missing v8 route is unguarded NoRoute like any other unregistered path: a bare
    // 404 without any key, unlike a missing /v0/management path (guarded 401 above).
    for path in [
        "/v8/management/plugins",
        "/v8/management/plugins/store",
        "/v8/management/quota/providers",
        "/v8/management/unknown-path",
    ] {
        let case = call(&client, &base, "GET", path, None, "").await;
        let ok = case.status == 404 && case.body.is_empty() && case.allow.is_none();
        if !ok {
            failures.push(format!(
                "GET {path} without a key: {} {:?} allow={:?}",
                case.status, case.body, case.allow
            ));
        }
        println!(
            "{} GET {path} (no key) -> {} allow={:?}",
            if ok { "PASS" } else { "FAIL" },
            case.status,
            case.allow
        );
    }
    for (method, path) in [("POST", "/v0/resource/plugins/x"), ("GET", "/v0/resource/plugins/x")] {
        let case = call(&client, &base, method, path, None, "").await;
        let ok = case.status == 404 && case.body.is_empty() && case.allow.is_none();
        if !ok {
            failures.push(format!(
                "{method} {path}: {} {:?} allow={:?}",
                case.status, case.body, case.allow
            ));
        }
        println!(
            "{} {method} {path} -> {} allow={:?}",
            if ok { "PASS" } else { "FAIL" },
            case.status,
            case.allow
        );
    }
    let head = call(
        &client,
        &base,
        "HEAD",
        "/v0/management/plugins",
        Some("Bearer fake-secret"),
        "",
    )
    .await;
    if head.status != 404 || head.allow.is_some() {
        failures.push(format!(
            "HEAD /v0/management/plugins: {} allow={:?}",
            head.status, head.allow
        ));
    }
    println!(
        "{} HEAD /v0/management/plugins -> {} allow={:?}",
        if head.status == 404 && head.allow.is_none() {
            "PASS"
        } else {
            "FAIL"
        },
        head.status,
        head.allow
    );

    // The OpenAPI document describes only what is registered.
    let docs = call(&client, &base, "GET", "/openapi.json", None, "").await;
    let document: Value = serde_json::from_str(&docs.body).unwrap_or(Value::Null);
    let paths: Vec<&String> = document["paths"]
        .as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default();
    let plugin_paths: Vec<&&String> = paths.iter().filter(|p| p.contains("plugin")).collect();
    let schemas: Vec<&String> = document["components"]["schemas"]
        .as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default();
    let plugin_schemas: Vec<&&String> = schemas
        .iter()
        .filter(|s| s.starts_with("Plugin") || s.as_str() == "QuotaProviders")
        .collect();
    let described_plugin = document["info"]["description"]
        .as_str()
        .is_some_and(|d| d.contains("plugin"));
    if docs.status != 200 || !plugin_paths.is_empty() || !plugin_schemas.is_empty() || described_plugin {
        failures.push(format!(
            "openapi: {} plugin paths {:?} schemas {:?} description mentions plugin: {described_plugin}",
            docs.status, plugin_paths, plugin_schemas
        ));
    }
    println!(
        "{} GET /openapi.json -> {} paths {} plugin-free, schemas {} plugin-free",
        if failures.is_empty() { "PASS" } else { "FAIL" },
        docs.status,
        paths.len(),
        schemas.len()
    );

    let _ = std::fs::remove_dir_all(&work);
    assert!(
        failures.is_empty(),
        "{} check(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
