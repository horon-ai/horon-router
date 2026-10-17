//! HTTP smoke tests for the local Scalar reference routes, plus the source-contract
//! checks that keep `GET /openapi.json` equal to the routes the server actually
//! registers. Everything runs through the real router and app over a loopback socket
//! with a fresh fake auth-dir and local-only executors: no provider, no real credential
//! and no external network.
//!
//! The expected path/method inventories below are authored from the router registration
//! in `src/lib.rs`, `src/codex_alpha.rs`, `src/websocket.rs`, `src/realtime.rs`,
//! `src/relay.rs`, `src/management.rs` and `src/management/legacy.rs` - never read back
//! from the document under test, so a spec that drifts from the source fails here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use horon_core::config::Config;
use horon_exec::Executors;
use horon_exec::claude::ClaudeExecutor;
use horon_server::management::{self, Management, Options};
use horon_server::{Runtime, app, router, safe_mode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The pinned management password, bcrypt-hashed into each fixture config and passed
/// explicitly as `Options::management_password` so the process `MANAGEMENT_PASSWORD`
/// environment variable can never change what these tests authenticate against.
const MANAGEMENT_PASSWORD: &str = "fake-management-only";
const CLIENT_KEY: &str = "fake-client";
const PINNED_ASSET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/scalar/scalar-standalone.js");
/// The SHA-256 recorded for the bundle in `assets/scalar/PROVENANCE.md`.
const PINNED_ASSET_SHA256: &str = "62c34a0dda38945864011ebaf6e7fa7812a332429120d18922b34d01c4f7ccc8";

/// `(path template, registered methods)` for the general API. The Gemini wildcard
/// `/v1beta/models/{*action}` is written as the document spells it (`{action}`).
/// HEAD is registered for `/healthz` and the docs routes but is not an OpenAPI
/// operation, so it is not part of this inventory.
const GENERAL_INVENTORY: &[(&str, &[&str])] = &[
    ("/healthz", &["get"]),
    ("/", &["get"]),
    ("/anthropic/callback", &["get"]),
    ("/codex/callback", &["get"]),
    ("/antigravity/callback", &["get"]),
    ("/callback", &["get"]),
    ("/devin/callback", &["get"]),
    ("/v1/models", &["get"]),
    ("/v1/chat/completions", &["post"]),
    ("/v1/completions", &["post"]),
    ("/v1/responses", &["get", "post"]),
    ("/v1/responses/compact", &["post"]),
    ("/backend-api/codex/responses", &["get", "post"]),
    ("/backend-api/codex/responses/compact", &["post"]),
    ("/v1/messages", &["post"]),
    ("/v1/messages/count_tokens", &["post"]),
    ("/v1beta/models", &["get"]),
    ("/v1beta/interactions", &["post"]),
    ("/v1beta/models/{action}", &["get", "post"]),
    ("/v1/images/generations", &["post"]),
    ("/v1/images/edits", &["post"]),
    ("/v1/videos", &["post"]),
    ("/v1/videos/generations", &["post"]),
    ("/v1/videos/edits", &["post"]),
    ("/v1/videos/extensions", &["post"]),
    ("/v1/videos/{request_id}", &["get"]),
    ("/openai/v1/videos", &["post"]),
    ("/openai/v1/videos/{video_id}", &["get"]),
    ("/openai/v1/videos/{video_id}/content", &["get"]),
    ("/v1/alpha/search", &["post"]),
    ("/backend-api/codex/alpha/search", &["post"]),
    ("/v1/live", &["post"]),
    ("/v1/live/{call_id}", &["get"]),
    ("/v1/realtime", &["get", "post"]),
    ("/v1/realtime/calls", &["post"]),
    ("/v1/realtime/calls/{call_id}", &["get"]),
    ("/v1/realtime/client_secrets", &["post"]),
    ("/v1/realtime/sessions", &["post"]),
    ("/v1/realtime/transcription_sessions", &["post"]),
    ("/v1/realtime/translations", &["get", "post"]),
    ("/v1/realtime/translations/client_secrets", &["post"]),
    ("/v1/realtime/calls/{call_id}/hangup", &["post"]),
    ("/v1/realtime/calls/{call_id}/accept", &["post"]),
    ("/v1/realtime/calls/{call_id}/reject", &["post"]),
    ("/v1/realtime/calls/{call_id}/refer", &["post"]),
    ("/v1/ws", &["get"]),
];

/// `(path template, registered methods)` for the management API: the fixed v8 tree, the
/// deprecated v0 tree, the loop-generated credentials/auth-files pairs,
/// and the legacy field/list/auth-url arrays in `src/management/legacy.rs`. The
/// `/v8/management/config/{*path}` wildcard is written as the document spells it.
const MANAGEMENT_INVENTORY: &[(&str, &[&str])] = &[
    // v8 fixed tree.
    ("/v8/management/config", &["get", "put", "patch"]),
    ("/v8/management/config.yaml", &["get", "put"]),
    ("/v8/management/config/", &["get", "put", "patch", "delete"]),
    ("/v8/management/config/{path}", &["get", "put", "patch", "delete"]),
    ("/v8/management/server/latest-version", &["get"]),
    ("/v8/management/server/tailscale", &["get"]),
    ("/v8/management/requests/api-call", &["post"]),
    ("/v8/management/routing/cooldown/reset", &["post"]),
    ("/v8/management/observability/usage/api-keys", &["get"]),
    ("/v8/management/observability/usage/queue", &["get"]),
    ("/v8/management/observability/usage/stats", &["get"]),
    ("/v8/management/observability/logs", &["get", "delete"]),
    ("/v8/management/observability/logs/errors", &["get"]),
    ("/v8/management/observability/logs/errors/{name}", &["get"]),
    ("/v8/management/observability/logs/requests/{id}", &["get"]),
    ("/v8/management/oauth/auth-url", &["get"]),
    ("/v8/management/oauth/status", &["get"]),
    ("/v8/management/oauth/session", &["delete"]),
    ("/v8/management/oauth/import", &["post"]),
    ("/v8/management/oauth/callback", &["get", "post"]),
    ("/v8/management/quota/fetch", &["post"]),
    // The `for (base, files, definitions)` loop in `management::router`.
    ("/v8/management/credentials", &["get", "post", "delete"]),
    ("/v8/management/credentials/models", &["get"]),
    ("/v8/management/credentials/download", &["get"]),
    ("/v8/management/credentials/status", &["patch"]),
    ("/v8/management/credentials/fields", &["patch"]),
    ("/v8/management/credentials/refresh", &["post"]),
    ("/v8/management/routing/model-definitions/{channel}", &["get"]),
    // v0 fixed tree (the deprecated spellings).
    ("/v0/management/config.yaml", &["get", "put"]),
    ("/v0/management/config", &["get"]),
    ("/v0/management/latest-version", &["get"]),
    ("/v0/management/api-call", &["post"]),
    ("/v0/management/reset-quota", &["post"]),
    ("/v0/management/api-key-usage", &["get"]),
    ("/v0/management/usage-queue", &["get"]),
    ("/v0/management/usage-stats", &["get"]),
    ("/v0/management/get-auth-status", &["get"]),
    ("/v0/management/oauth-session", &["delete"]),
    ("/v0/management/oauth-callback", &["get", "post"]),
    ("/v0/management/quota/fetch", &["post"]),
    ("/v0/management/quota/reset", &["post"]),
    ("/v0/management/vertex/import", &["post"]),
    ("/v0/management/logs", &["get", "delete"]),
    ("/v0/management/request-error-logs", &["get"]),
    ("/v0/management/request-error-logs/{name}", &["get"]),
    ("/v0/management/request-log-by-id/{id}", &["get"]),
    ("/v0/management/auth-files", &["get", "post", "delete"]),
    ("/v0/management/auth-files/models", &["get"]),
    ("/v0/management/auth-files/download", &["get"]),
    ("/v0/management/auth-files/status", &["patch"]),
    ("/v0/management/auth-files/fields", &["patch"]),
    ("/v0/management/auth-files/refresh", &["post"]),
    ("/v0/management/model-definitions/{channel}", &["get"]),
    ("/v0/management/api-keys", &["get", "put", "patch", "delete"]),
    // `legacy::FIELD_ROUTES` (15): get/put/patch, plus DELETE for `proxy-url` only.
    ("/v0/management/debug", &["get", "put", "patch"]),
    ("/v0/management/logging-to-file", &["get", "put", "patch"]),
    ("/v0/management/logs-max-total-size-mb", &["get", "put", "patch"]),
    ("/v0/management/error-logs-max-files", &["get", "put", "patch"]),
    ("/v0/management/usage-statistics-enabled", &["get", "put", "patch"]),
    ("/v0/management/proxy-url", &["get", "put", "patch", "delete"]),
    ("/v0/management/quota-exceeded/switch-project", &["get", "put", "patch"]),
    (
        "/v0/management/quota-exceeded/switch-preview-model",
        &["get", "put", "patch"],
    ),
    ("/v0/management/request-log", &["get", "put", "patch"]),
    ("/v0/management/ws-auth", &["get", "put", "patch"]),
    ("/v0/management/request-retry", &["get", "put", "patch"]),
    ("/v0/management/max-retry-credentials", &["get", "put", "patch"]),
    ("/v0/management/max-retry-interval", &["get", "put", "patch"]),
    ("/v0/management/force-model-prefix", &["get", "put", "patch"]),
    ("/v0/management/routing/strategy", &["get", "put", "patch"]),
    // `legacy::LIST_ROUTES` (11): get/put/patch/delete.
    ("/v0/management/gemini-api-key", &["get", "put", "patch", "delete"]),
    (
        "/v0/management/interactions-api-key",
        &["get", "put", "patch", "delete"],
    ),
    ("/v0/management/claude-api-key", &["get", "put", "patch", "delete"]),
    ("/v0/management/codex-api-key", &["get", "put", "patch", "delete"]),
    ("/v0/management/xai-api-key", &["get", "put", "patch", "delete"]),
    ("/v0/management/meta-api-key", &["get", "put", "patch", "delete"]),
    (
        "/v0/management/openai-compatibility",
        &["get", "put", "patch", "delete"],
    ),
    ("/v0/management/vertex-api-key", &["get", "put", "patch", "delete"]),
    (
        "/v0/management/oauth-excluded-models",
        &["get", "put", "patch", "delete"],
    ),
    ("/v0/management/oauth-model-alias", &["get", "put", "patch", "delete"]),
    (
        "/v0/management/oauth-request-scoped-errors",
        &["get", "put", "patch", "delete"],
    ),
    // `legacy::AUTH_URL_ROUTES` (8): get only.
    ("/v0/management/anthropic-auth-url", &["get"]),
    ("/v0/management/codex-auth-url", &["get"]),
    ("/v0/management/antigravity-auth-url", &["get"]),
    ("/v0/management/kimi-auth-url", &["get"]),
    ("/v0/management/kimi-ai-auth-url", &["get"]),
    ("/v0/management/xai-auth-url", &["get"]),
    ("/v0/management/devin-auth-url", &["get"]),
    ("/v0/management/meta-auth-url", &["get"]),
];

/// Routes that are registered but deliberately left out of the document, with the reason.
/// The documents surface serves the document it would otherwise describe, and the control
/// panel paths are static assets rather than API operations.
const EXCLUDED_FROM_SPEC: &[(&str, &str)] = &[
    ("/docs", "the reference page itself (openapi/docs.rs)"),
    ("/openapi.json", "the document itself"),
    ("/docs/assets/scalar.js", "the vendored Scalar bundle"),
    ("/ui", "control-panel asset alias (management.rs panel_alias)"),
    ("/management.html", "static control-panel page"),
    ("/assets/probe", "static control-panel assets under /assets/{*path}"),
    ("/fonts/probe", "static control-panel fonts under /fonts/{*path}"),
    ("/favicon.svg", "static control-panel favicon"),
];

/// General paths that answer without a client key, and the status they answer with.
const PUBLIC_GENERAL: &[(&str, u16)] = &[
    ("/healthz", 200),
    ("/", 200),
    ("/anthropic/callback", 200),
    ("/codex/callback", 200),
    ("/antigravity/callback", 200),
    ("/callback", 400),
    ("/devin/callback", 400),
];

/// Management paths whose handlers run without a key (the OAuth callbacks). They answer
/// 400 for the missing input, never 401.
const PUBLIC_MANAGEMENT: &[&str] = &["/v8/management/oauth/callback", "/v0/management/oauth-callback"];

struct Fixture {
    dir: PathBuf,
    rt: Arc<Runtime>,
    state: Arc<Management>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::with_yaml(name, |dir, hash| {
            format!(
                "config-version: 8\nmanagement:\n  secret-key: '{hash}'\noauth:\n  auth-dir: {}\naccess:\n  api-keys: [{CLIENT_KEY}]\n",
                dir.join("auth").display()
            )
        })
    }

    fn with_yaml(name: &str, yaml: impl Fn(&Path, &str) -> String) -> Self {
        let dir = std::env::temp_dir().join(format!("cpa-openapi-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("auth")).unwrap();
        let path = dir.join("config.yaml");
        let hash = bcrypt::hash(MANAGEMENT_PASSWORD, 4).unwrap();
        std::fs::write(&path, yaml(&dir, &hash)).unwrap();
        let config = Config::load(&path).unwrap();
        let rt = Arc::new(horon_server::testing::runtime(
            config.clone(),
            horon_core::config::credentials::load(&config),
            executors(),
        ));
        let state = Management::with_options(
            rt.clone(),
            path,
            Options {
                management_password: Some(MANAGEMENT_PASSWORD.to_owned()),
                ..Default::default()
            },
        );
        Self { dir, rt, state }
    }

    fn api_router(&self) -> axum::Router {
        router(self.rt.clone())
    }

    fn served_app(&self) -> axum::Router {
        app(self.rt.clone(), management::router(self.state.clone()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
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

async fn serve(application: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            application.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (base, handle)
}

async fn request(
    base: &str,
    method: wreq::Method,
    path: &str,
    bearer: Option<&str>,
) -> (u16, wreq::header::HeaderMap, Vec<u8>) {
    let mut req = wreq::Client::new().request(method, format!("{base}{path}"));
    if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    let headers = res.headers().clone();
    (status, headers, res.bytes().await.unwrap().to_vec())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A concrete, syntactically valid URL path for a template, so a probe reaches the route
/// instead of a 404 from the path matcher. The values are deliberately nonexistent: no
/// probe may touch a real resource.
fn concrete(template: &str) -> String {
    template
        .replace("{action}", "gemini-2.5-flash:generateContent")
        .replace("{request_id}", "probe")
        .replace("{video_id}", "probe")
        .replace("{call_id}", "probe")
        .replace("{channel}", "probe")
        .replace("{path}", "probe")
        .replace("{name}", "probe")
        .replace("{id}", "probe")
}

fn http_method(name: &str) -> wreq::Method {
    match name {
        "get" => wreq::Method::GET,
        "post" => wreq::Method::POST,
        "put" => wreq::Method::PUT,
        "patch" => wreq::Method::PATCH,
        "delete" => wreq::Method::DELETE,
        other => panic!("unsupported method in the inventory: {other}"),
    }
}

// ---- Contract validators (shared by the equality and corruption tests) ----

const HTTP_METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// The document's `paths` as a path -> method-set map, restricted to the surface `keep`
/// selects (`/v8/management` and `/v0/management` are management; everything else is the
/// general API).
fn spec_inventory(document: &Value, keep: impl Fn(&str) -> bool) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    let Some(paths) = document["paths"].as_object() else {
        return out;
    };
    for (path, item) in paths {
        if !keep(path) {
            continue;
        }
        let mut methods = BTreeSet::new();
        if let Some(item) = item.as_object() {
            for method in item.keys() {
                if HTTP_METHODS.contains(&method.as_str()) {
                    methods.insert(method.clone());
                }
            }
        }
        out.insert(path.clone(), methods);
    }
    out
}

fn is_management(path: &str) -> bool {
    path.starts_with("/v8/management") || path.starts_with("/v0/management")
}

fn expected_inventory(expected: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for (path, methods) in expected {
        let set: BTreeSet<String> = methods.iter().map(|m| (*m).to_owned()).collect();
        assert!(
            out.insert((*path).to_owned(), set).is_none(),
            "the expected inventory lists {path} twice"
        );
    }
    out
}

/// Every difference between the selected surface of the document and the expected
/// inventory, as one line each: a path only in the document, a path only in the
/// expectation, or a method-set difference. An empty result means exact equality.
fn inventory_diffs(document: &Value, expected: &[(&str, &[&str])], keep: impl Fn(&str) -> bool) -> Vec<String> {
    let spec = spec_inventory(document, keep);
    let want = expected_inventory(expected);
    let mut diffs = Vec::new();
    for (path, methods) in &spec {
        match want.get(path) {
            None => diffs.push(format!("document has unregistered path {path}")),
            Some(want) if want != methods => {
                diffs.push(format!("{path}: document methods {methods:?} != registered {want:?}"));
            }
            Some(_) => {}
        }
    }
    for (path, methods) in &want {
        if !spec.contains_key(path) {
            diffs.push(format!(
                "registered path {path} is missing from the document ({methods:?})"
            ));
        }
    }
    diffs
}

fn collect_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref") {
                out.push(reference.clone());
            }
            for child in map.values() {
                collect_refs(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_refs(child, out);
            }
        }
        _ => {}
    }
}

fn resolves(root: &Value, pointer: &str) -> bool {
    let Some(rest) = pointer.strip_prefix("#/") else {
        return false;
    };
    let mut cur = root;
    for segment in rest.split('/') {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        match cur {
            Value::Object(map) => match map.get(&segment) {
                Some(next) => cur = next,
                None => return false,
            },
            Value::Array(items) => match segment.parse::<usize>().ok().and_then(|i| items.get(i)) {
                Some(next) => cur = next,
                None => return false,
            },
            _ => return false,
        }
    }
    true
}

/// Local `#/...` references that do not resolve inside the document.
fn dangling_refs(document: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs(document, &mut refs);
    refs.into_iter()
        .filter(|r| r.starts_with("#/") && !resolves(document, r))
        .collect()
}

/// Security requirements that name a scheme `components.securitySchemes` does not declare,
/// plus operations missing an `operationId` or a response.
fn operation_violations(document: &Value) -> Vec<String> {
    let schemes: BTreeSet<String> = document["components"]["securitySchemes"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    let Some(paths) = document["paths"].as_object() else {
        return vec!["the document has no `paths`".to_owned()];
    };
    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            out.push(format!("{path}: path item is not an object"));
            continue;
        };
        for (method, operation) in item {
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            let where_ = format!("{method} {path}");
            if operation["operationId"].as_str().is_none_or(str::is_empty) {
                out.push(format!("{where_}: missing operationId"));
            }
            if !operation["responses"].as_object().is_some_and(|r| !r.is_empty()) {
                out.push(format!("{where_}: missing responses"));
            }
            for alternative in operation["security"].as_array().into_iter().flatten() {
                let Some(alternative) = alternative.as_object() else {
                    out.push(format!("{where_}: security alternative is not an object"));
                    continue;
                };
                for scheme in alternative.keys() {
                    if !schemes.contains(scheme) {
                        out.push(format!("{where_}: security scheme `{scheme}` is not declared"));
                    }
                }
            }
        }
    }
    out
}

/// Operations whose request or response `content` carries no media type, or a media type
/// with no schema.
fn media_violations(document: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(paths) = document["paths"].as_object() else {
        return vec!["the document has no `paths`".to_owned()];
    };
    let check = |where_: &str, content: &Value, out: &mut Vec<String>| {
        let Some(content) = content.as_object() else {
            return;
        };
        if content.is_empty() {
            out.push(format!("{where_}: content declares no media type"));
        }
        for (media, entry) in content {
            if media.is_empty() {
                out.push(format!("{where_}: an empty media type"));
            }
            if !entry["schema"].is_object() {
                out.push(format!("{where_}: {media} has no schema"));
            }
        }
    };
    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        for (method, operation) in item {
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            let where_ = format!("{method} {path}");
            if let Some(body) = operation.get("requestBody") {
                check(&format!("{where_} request"), &body["content"], &mut out);
            }
            if let Some(responses) = operation["responses"].as_object() {
                for (status, response) in responses {
                    if let Some(content) = response.get("content") {
                        check(&format!("{where_} {status} response"), content, &mut out);
                    }
                }
            }
        }
    }
    out
}

// ---- Existing smoke tests (unchanged) ----

#[tokio::test]
async fn docs_spec_and_asset_are_public_and_truthful() {
    let f = Fixture::new("api");
    let (base, server) = serve(f.api_router()).await;

    let (status, headers, body) = request(&base, wreq::Method::GET, "/docs", None).await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert_eq!(headers["cache-control"], "no-cache");
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("Scalar.createApiReference('#app'"), "{html}");
    assert!(html.contains("url: '/openapi.json'"), "{html}");
    assert!(html.contains("servers: [{ url: '/' }]"), "{html}");
    assert!(html.contains(r#"src="/docs/assets/scalar.js""#), "{html}");
    for option in [
        "persistAuth: false",
        "telemetry: false",
        "withDefaultFonts: false",
        "agent: { disabled: true }",
        "showDeveloperTools: 'never'",
        "proxyUrl: ''",
        "mcp: { disabled: true }",
    ] {
        assert!(html.contains(option), "the page must configure {option}: {html}");
    }
    assert!(!html.contains(CLIENT_KEY), "no client key may be injected");
    assert!(!html.contains(MANAGEMENT_PASSWORD), "no management key may be injected");

    let (status, headers, body) = request(&base, wreq::Method::HEAD, "/docs", None).await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert_eq!(headers["content-length"].to_str().unwrap(), html.len().to_string());
    assert!(body.is_empty());

    let (status, headers, body) = request(&base, wreq::Method::GET, "/openapi.json", None).await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-type"], "application/json; charset=utf-8");
    assert_eq!(headers["cache-control"], "no-cache");
    let spec: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(spec["openapi"], "3.1.0");
    assert_eq!(spec["info"]["title"], "CLIProxyAPI");
    assert!(!spec["paths"].as_object().unwrap().is_empty());

    let (status, _, body) = request(&base, wreq::Method::HEAD, "/openapi.json", None).await;
    assert_eq!(status, 200);
    assert!(body.is_empty());

    let (status, headers, body) = request(&base, wreq::Method::GET, "/docs/assets/scalar.js", None).await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-type"], "text/javascript; charset=utf-8");
    let pinned = std::fs::read(PINNED_ASSET).unwrap();
    assert_eq!(body.len(), pinned.len());
    assert_eq!(digest(&body), digest(&pinned));
    assert_eq!(digest(&body), PINNED_ASSET_SHA256);
    assert!(
        String::from_utf8_lossy(&body).contains("createApiReference"),
        "the served bundle must expose Scalar.createApiReference"
    );

    let (status, headers, body) = request(&base, wreq::Method::HEAD, "/docs/assets/scalar.js", None).await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-length"].to_str().unwrap(), pinned.len().to_string());
    assert!(body.is_empty());

    server.abort();
}

#[tokio::test]
async fn new_routes_keep_the_existing_no_route_and_head_rules() {
    let f = Fixture::new("no-route");
    let (base, server) = serve(f.api_router()).await;

    for (method, path) in [
        (wreq::Method::POST, "/docs"),
        (wreq::Method::PUT, "/docs"),
        (wreq::Method::POST, "/openapi.json"),
        (wreq::Method::GET, "/openapi.json/extra"),
        (wreq::Method::GET, "/docs/assets/scalar.js/extra"),
    ] {
        let (status, headers, _) = request(&base, method.clone(), path, None).await;
        assert_eq!(status, 404, "{method} {path}");
        assert!(headers.get("allow").is_none(), "{method} {path}");
    }

    for path in ["/v1/models", "/", "/v2/nothing", "/docs/"] {
        let (status, _, _) = request(&base, wreq::Method::HEAD, path, None).await;
        assert_eq!(status, 404, "HEAD {path}");
    }

    let (status, _, _) = request(&base, wreq::Method::GET, "/v1/models", None).await;
    assert_eq!(status, 401, "the API key check must stay in front of /v1");
    let (status, _, _) = request(&base, wreq::Method::GET, "/v1/models", Some(CLIENT_KEY)).await;
    assert_eq!(status, 200);

    server.abort();
}

#[tokio::test]
async fn served_app_serves_the_reference() {
    let f = Fixture::new("app");
    let (base, server) = serve(f.served_app()).await;

    for path in ["/docs", "/openapi.json", "/docs/assets/scalar.js"] {
        let (status, _, _) = request(&base, wreq::Method::GET, path, None).await;
        assert_eq!(status, 200, "GET {path}");
    }
    let (status, _, _) = request(&base, wreq::Method::GET, "/management.html", None).await;
    assert_eq!(status, 200);

    server.abort();
}

#[tokio::test]
async fn management_auth_rejections_use_a_fresh_state() {
    let f = Fixture::new("mgmt-auth");
    let (base, server) = serve(f.served_app()).await;

    for bearer in [None, Some("wrong-management-key")] {
        let (status, _, _) = request(&base, wreq::Method::GET, "/v8/management/config", bearer).await;
        assert_eq!(status, 401, "{bearer:?}");
    }
    let (status, _, _) = request(
        &base,
        wreq::Method::GET,
        "/v8/management/config",
        Some(MANAGEMENT_PASSWORD),
    )
    .await;
    assert_eq!(status, 200);

    server.abort();
}

#[tokio::test]
async fn safe_mode_keeps_the_reference_public_and_blocks_the_proxy_api() {
    let f = Fixture::with_yaml("safe-mode", |dir, hash| {
        format!(
            "config-version: 8\nmanagement:\n  secret-key: '{hash}'\noauth:\n  auth-dir: {}\naccess:\n  api-keys: [your-api-key-1]\n",
            dir.join("auth").display()
        )
    });
    let (base, server) = serve(safe_mode::router(&f.rt, f.served_app())).await;

    for path in ["/docs", "/openapi.json", "/docs/assets/scalar.js"] {
        let (status, _, _) = request(&base, wreq::Method::GET, path, None).await;
        assert_eq!(status, 200, "GET {path}");
    }
    let (status, headers, _) = request(&base, wreq::Method::GET, "/v1/models", None).await;
    assert_eq!(status, 403);
    assert_eq!(headers["x-cpa-safe-mode"], "example-api-key");
    let (status, _, _) = request(&base, wreq::Method::GET, "/healthz", None).await;
    assert_eq!(status, 200);

    server.abort();
}

#[tokio::test]
async fn disable_control_panel_does_not_suppress_the_reference() {
    let f = Fixture::with_yaml("no-panel", |dir, hash| {
        format!(
            "config-version: 8\nmanagement:\n  secret-key: '{hash}'\n  disable-control-panel: true\noauth:\n  auth-dir: {}\naccess:\n  api-keys: [{CLIENT_KEY}]\n",
            dir.join("auth").display()
        )
    });
    let (base, server) = serve(f.served_app()).await;

    let (status, _, _) = request(&base, wreq::Method::GET, "/management.html", None).await;
    assert_eq!(status, 404);
    for path in ["/docs", "/openapi.json", "/docs/assets/scalar.js"] {
        let (status, _, _) = request(&base, wreq::Method::GET, path, None).await;
        assert_eq!(status, 200, "GET {path}");
    }

    server.abort();
}

// ---- Inventory equality ----

/// The document's `paths` equals the general inventory exactly: 46 paths, 51 operations.
#[test]
fn general_inventory_matches_the_document() {
    let document = horon_server::openapi::document();
    let diffs = inventory_diffs(&document, GENERAL_INVENTORY, |p| !is_management(p));
    assert!(
        diffs.is_empty(),
        "the general contract drifted from the router:\n{}",
        diffs.join("\n")
    );

    let spec = spec_inventory(&document, |p| !is_management(p));
    let ops: usize = GENERAL_INVENTORY.iter().map(|(_, m)| m.len()).sum();
    assert_eq!(spec.len(), 46, "expected 46 general paths");
    assert_eq!(ops, 51, "expected 51 general operations");
}

/// The document's `paths` equals the management inventory exactly: 88 paths, 173
/// operations, so the two surfaces together are 134 paths.
#[test]
fn management_inventory_matches_the_document() {
    let document = horon_server::openapi::document();
    let diffs = inventory_diffs(&document, MANAGEMENT_INVENTORY, is_management);
    assert!(
        diffs.is_empty(),
        "the management contract drifted from the router:\n{}",
        diffs.join("\n")
    );

    let spec = spec_inventory(&document, is_management);
    let ops: usize = MANAGEMENT_INVENTORY.iter().map(|(_, m)| m.len()).sum();
    assert_eq!(spec.len(), 88, "expected 88 management paths");
    assert_eq!(ops, 173, "expected 173 management operations");
    let total = spec_inventory(&document, |_| true).len();
    assert_eq!(total, 134, "the two surfaces together must be exactly 134 paths");
}

/// The docs surface and the static control-panel assets are registered but must not be
/// described as API operations. The docs routes are served, which proves the exclusion
/// is deliberate rather than a missing registration.
#[tokio::test]
async fn excluded_surfaces_are_served_but_absent_from_the_document() {
    let document = horon_server::openapi::document();
    let spec = spec_inventory(&document, |_| true);
    for (path, reason) in EXCLUDED_FROM_SPEC {
        assert!(
            !spec.contains_key(&concrete(path)),
            "{path} is excluded from the document ({reason})"
        );
    }
    for path in spec.keys() {
        assert!(
            !path.starts_with("/assets/") && !path.starts_with("/fonts/"),
            "{path} is a static control-panel asset, not an API operation"
        );
        assert!(
            !path.starts_with("/v0/resource/"),
            "{path} is a plugin resource route, which no longer exists"
        );
    }

    // The excluded routes are real: the app serves the docs page and the panel.
    let f = Fixture::new("excluded");
    let (base, server) = serve(f.served_app()).await;
    for path in ["/docs", "/openapi.json", "/docs/assets/scalar.js"] {
        let (status, _, _) = request(&base, wreq::Method::GET, path, None).await;
        assert_eq!(status, 200, "GET {path} is served but must not be in the document");
    }
    server.abort();
}

// ---- Reference, operation, security and media validity ----

#[test]
fn references_operations_security_and_media_are_valid() {
    let document = horon_server::openapi::document();

    let dangling = dangling_refs(&document);
    assert!(dangling.is_empty(), "dangling $refs: {dangling:?}");

    let operations = operation_violations(&document);
    assert!(operations.is_empty(), "operation violations: {operations:?}");

    let media = media_violations(&document);
    assert!(media.is_empty(), "media violations: {media:?}");

    let schemes = document["components"]["securitySchemes"].as_object().unwrap();
    for expected in [
        "clientBearer",
        "clientGoogApiKey",
        "clientApiKey",
        "clientQueryKey",
        "clientQueryAuthToken",
        "managementBearer",
        "managementKey",
    ] {
        assert!(schemes.contains_key(expected), "missing security scheme {expected}");
    }
}

/// Each validator must fail on the corruption it exists to catch: a removed path, a method
/// added to the wrong path, a dangling `$ref`, an undeclared security scheme and a media
/// type without a schema.
#[test]
fn contract_validators_reject_corruption() {
    let document = horon_server::openapi::document();

    // A registered general path removed from the document.
    let mut removed = document.clone();
    removed["paths"].as_object_mut().unwrap().remove("/v1/models");
    let diffs = inventory_diffs(&removed, GENERAL_INVENTORY, |p| !is_management(p));
    assert!(
        diffs.iter().any(|d| d.contains("/v1/models")),
        "a removed path must be caught: {diffs:?}"
    );

    // A method added to a path that never registers it.
    let mut extra_method = document.clone();
    extra_method["paths"]["/healthz"]["delete"] = extra_method["paths"]["/healthz"]["get"].clone();
    let diffs = inventory_diffs(&extra_method, GENERAL_INVENTORY, |p| !is_management(p));
    assert!(
        diffs.iter().any(|d| d.contains("/healthz") && d.contains("delete")),
        "an added wrong method must be caught: {diffs:?}"
    );

    // A registered management path removed, and a legacy method added to the wrong path.
    let mut removed_management = document.clone();
    removed_management["paths"]
        .as_object_mut()
        .unwrap()
        .remove("/v0/management/proxy-url");
    let diffs = inventory_diffs(&removed_management, MANAGEMENT_INVENTORY, is_management);
    assert!(
        diffs.iter().any(|d| d.contains("/v0/management/proxy-url")),
        "a removed management path must be caught: {diffs:?}"
    );
    let mut extra_management = document.clone();
    extra_management["paths"]["/v0/management/debug"]["post"] =
        extra_management["paths"]["/v0/management/debug"]["get"].clone();
    let diffs = inventory_diffs(&extra_management, MANAGEMENT_INVENTORY, is_management);
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("/v0/management/debug") && d.contains("post")),
        "an added wrong management method must be caught: {diffs:?}"
    );

    // A missing local reference.
    let mut dangling = document.clone();
    dangling["components"]["schemas"]["Broken"] =
        json!({ "type": "object", "properties": { "x": { "$ref": "#/components/schemas/DoesNotExist" } } });
    let refs = dangling_refs(&dangling);
    assert!(
        refs.iter().any(|r| r.contains("DoesNotExist")),
        "a missing $ref must be caught: {refs:?}"
    );

    // An undeclared security scheme.
    let mut bad_security = document.clone();
    bad_security["paths"]["/v1/models"]["get"]["security"] = json!([{ "ghostScheme": [] }]);
    let operations = operation_violations(&bad_security);
    assert!(
        operations.iter().any(|v| v.contains("ghostScheme")),
        "an undeclared security scheme must be caught: {operations:?}"
    );

    // A media type without a schema.
    let mut bad_media = document.clone();
    bad_media["paths"]["/v1/chat/completions"]["post"]["responses"]["200"]["content"]["application/json"] = json!({});
    let media = media_violations(&bad_media);
    assert!(
        media.iter().any(|v| v.contains("application/json")),
        "a media type without a schema must be caught: {media:?}"
    );
}

// ---- Runtime probes against the real app ----

/// Every general operation is registered and behind its gate: a request without a client
/// key reaches the route (not the NoRoute 404) and is refused with 401, except the public
/// routes, which answer their documented status. No successful (mutating) request is sent.
#[tokio::test]
async fn general_inventory_is_registered_and_gated() {
    let f = Fixture::new("probe-general");
    let (base, server) = serve(f.served_app()).await;

    for (template, methods) in GENERAL_INVENTORY {
        let path = concrete(template);
        let expected = PUBLIC_GENERAL
            .iter()
            .find(|(p, _)| p == template)
            .map(|(_, status)| *status)
            .unwrap_or(401);
        for method in *methods {
            let (status, _, body) = request(&base, http_method(method), &path, None).await;
            assert_eq!(
                status, expected,
                "{method} {path} must answer {expected} without a client key"
            );
            if expected == 401 {
                assert!(
                    String::from_utf8_lossy(&body).contains("error"),
                    "{method} {path} must answer an error body"
                );
            }
        }
    }

    // A route the server does not register is NoRoute's bare 404.
    let (status, _, _) = request(&base, wreq::Method::GET, "/v1/not-registered", None).await;
    assert_eq!(status, 404);
    // An unregistered method on a registered path is 404 (gin runs no MethodNotAllowed).
    let (status, headers, _) = request(&base, wreq::Method::DELETE, "/v1/models", None).await;
    assert_eq!(status, 404);
    assert!(headers.get("allow").is_none());

    server.abort();
}

/// Every management operation is registered and behind its gate. Each fresh `Management`
/// state answers at most four counted authentication failures (the ban threshold is five),
/// so the state is rebuilt every four probes; the probes are all unauthenticated or
/// keyless, never a successful mutation.
#[tokio::test]
async fn management_inventory_is_registered_and_gated() {
    let f = Fixture::new("probe-management");
    let config_path = f.dir.join("config.yaml");

    let probes: Vec<(String, &'static str)> = MANAGEMENT_INVENTORY
        .iter()
        .flat_map(|(path, methods)| methods.iter().map(move |m| ((*path).to_owned(), *m)))
        .collect();

    let mut index = 0;
    while index < probes.len() {
        let state = Management::with_options(
            f.rt.clone(),
            config_path.clone(),
            Options {
                management_password: Some(MANAGEMENT_PASSWORD.to_owned()),
                ..Default::default()
            },
        );
        let (base, server) = serve(app(f.rt.clone(), management::router(state))).await;
        let end = (index + 4).min(probes.len());
        for (template, method) in &probes[index..end] {
            let path = concrete(template);
            let (status, _, body) = request(&base, http_method(method), &path, None).await;
            if PUBLIC_MANAGEMENT.contains(&template.as_str()) {
                assert_eq!(
                    status, 400,
                    "{method} {path} is public and must answer 400 for missing input"
                );
            } else {
                assert_eq!(
                    status, 401,
                    "{method} {path} must require a management key (got {status})"
                );
                assert!(
                    String::from_utf8_lossy(&body).contains("management key"),
                    "{method} {path} must answer the management-key error"
                );
            }
        }
        server.abort();
        index = end;
    }

    // The management HEAD rule: a HEAD on a v8 path is NoRoute (a bare 404), never the GET
    // handler. A wrong method on a v8 path is NoRoute too, while a v0 path sends unknown
    // methods through the guard.
    let state = Management::with_options(
        f.rt.clone(),
        config_path.clone(),
        Options {
            management_password: Some(MANAGEMENT_PASSWORD.to_owned()),
            ..Default::default()
        },
    );
    let (base, server) = serve(app(f.rt.clone(), management::router(state))).await;
    let (status, headers, _) = request(&base, wreq::Method::HEAD, "/v8/management/config", None).await;
    assert_eq!(status, 404, "HEAD on a v8 management path is NoRoute");
    assert!(headers.get("allow").is_none());
    let (status, _, _) = request(&base, wreq::Method::TRACE, "/v8/management/config", None).await;
    assert_eq!(status, 404, "an unregistered method on a v8 path is NoRoute");
    let (status, _, _) = request(&base, wreq::Method::GET, "/v8/management/not-registered", None).await;
    assert_eq!(status, 404, "an unknown v8 path is NoRoute");
    server.abort();
}

/// A management state with no configured secret answers 404 with an empty body for every
/// path, the availability gate the contract describes.
#[tokio::test]
async fn disabled_management_answers_404_with_an_empty_body() {
    let dir = std::env::temp_dir().join(format!("cpa-openapi-disabled-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
    let rt = Arc::new(horon_server::testing::runtime(
        Config::load(&path).unwrap(),
        vec![],
        executors(),
    ));
    let state = Management::with_options(
        rt.clone(),
        path.clone(),
        Options {
            management_password: Some(String::new()),
            ..Default::default()
        },
    );
    let (base, server) = serve(app(rt.clone(), management::router(state))).await;

    for (method, path) in [
        (wreq::Method::GET, "/v8/management/config"),
        (wreq::Method::GET, "/v0/management/config"),
        (wreq::Method::GET, "/v8/management/oauth/callback"),
    ] {
        let (status, _, body) = request(&base, method.clone(), path, None).await;
        assert_eq!(status, 404, "{method} {path} with management disabled");
        assert!(body.is_empty(), "{method} {path} must have an empty body");
    }
    server.abort();
    let _ = std::fs::remove_dir_all(&dir);
}
