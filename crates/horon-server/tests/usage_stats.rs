//! `GET /observability/usage/stats`: cumulative per-dimension token/cache usage over
//! the real router, management auth and a loopback mock upstream. One proxied xai
//! response folds into the model, provider and auth-index dimensions, and the endpoint
//! reads them back without draining the usage queue.

use std::sync::Arc;
use std::time::Duration;

use horon_core::config::Config;
use horon_exec::Executors;
use horon_exec::claude::ClaudeExecutor;
use horon_server::usage_stats::Dim;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MGMT_KEY: &str = "fake-management-only";
const MODEL: &str = "grok-4.7";

const CREATED: &str = r#"{"type":"response.created","response":{"id":"resp_1","object":"response","model":"grok-4.7","status":"in_progress","output":[]}}"#;
const COMPLETED_USAGE: &str = r#"{"type":"response.completed","response":{"id":"resp_1","object":"response","model":"grok-4.7","status":"completed","output":[],"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}}"#;

fn chunk(data: &str) -> String {
    format!("{:x}\r\n{data}\r\n", data.len())
}

fn sse(event: &str) -> String {
    chunk(&format!("data: {event}\n\n"))
}

const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";

/// Reads one HTTP/1.1 request (headers and Content-Length body).
async fn read_request(conn: &mut TcpStream) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = conn.read(&mut tmp).await.unwrap();
        assert!(n > 0, "upstream connection closed before the request was read");
        buf.extend_from_slice(&tmp[..n]);
        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
        let length: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .map_or(0, |v| v.trim().parse().unwrap());
        if buf.len() >= end + 4 + length {
            return;
        }
    }
}

/// A loopback upstream that answers each request with one complete SSE body ending in
/// `response.completed` carrying usage tokens.
async fn mock() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        loop {
            let (mut conn, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                read_request(&mut conn).await;
                let body = format!("{SSE_HEAD}{}{}0\r\n\r\n", sse(CREATED), sse(COMPLETED_USAGE));
                conn.write_all(body.as_bytes()).await.unwrap();
                conn.flush().await.unwrap();
                // Keep the connection open until the test runtime ends.
                std::future::pending::<()>().await;
                drop(conn);
            });
        }
    });
    addr
}

struct Fixture {
    base: String,
    addr: String,
    rt: Arc<horon_server::Runtime>,
    dir: std::path::PathBuf,
}

async fn fixture() -> Fixture {
    let upstream = mock().await;
    // A private auth-dir: without one, credential loading reads the real auth store.
    let dir = std::env::temp_dir().join(format!("cpa-usage-stats-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("auth")).unwrap();
    let path = dir.join("config.yaml");
    let hash = bcrypt::hash(MGMT_KEY, 4).unwrap();
    std::fs::write(
        &path,
        format!(
            "config-version: 8\nmanagement:\n  secret-key: '{hash}'\noauth:\n  auth-dir: {}\naccess:\n  api-keys: [client-key]\napi-keys:\n  xai:\n    - name: xai-1\n      base-url: http://{upstream}/v1\n      keys:\n        - api-key: sk-fake-xai\n",
            dir.join("auth").display()
        ),
    )
    .unwrap();
    let cfg = Config::load(&path).unwrap();
    let credentials = horon_core::config::credentials::load(&cfg);
    let executors = Executors {
        claude: ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
        codex: Default::default(),
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    };
    let rt = Arc::new(horon_server::testing::runtime(cfg, credentials, executors));
    horon_server::install_registry(&rt);
    // Management first (it configures the queue from the file config), then turn the
    // statistics flag on with the parsed observability document.
    let state = horon_server::management::Management::new(rt.clone(), path);
    rt.usage_queue().configure(
        true,
        &Config::parse("observability: {usage: {usage-statistics-enabled: true}}\n").unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let base = format!("http://{addr}");
    // The served app: the proxy API first, the management router behind it.
    let app = horon_server::app(rt.clone(), horon_server::management::router(state));
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    Fixture { base, addr, rt, dir }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Sends one proxied `/v1/responses` request and drains the streamed reply.
async fn proxy_request(addr: &str) {
    let body = format!(r#"{{"model":"{MODEL}","input":"hi","stream":true}}"#);
    let mut conn = TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "POST /v1/responses HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer client-key\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    conn.write_all(head.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    conn.read_to_end(&mut out).await.unwrap();
    assert!(
        String::from_utf8_lossy(&out).contains("response.completed"),
        "{}",
        String::from_utf8_lossy(&out)
    );
}

/// Polls the model dimension (non-destructive) until the row appears.
async fn wait_for_model_row(rt: &horon_server::Runtime, model: &str) -> Value {
    for _ in 0..500 {
        let snap = rt.usage_stats().snapshot(Dim::Model);
        if let Some(row) = snap["rows"].as_array().unwrap().iter().find(|r| r["key"] == model) {
            return row.clone();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("model row {model} never appeared");
}

#[tokio::test]
async fn usage_stats_endpoint_reports_rollups_and_never_drains() {
    let f = fixture().await;
    proxy_request(&f.addr).await;
    let row = wait_for_model_row(&f.rt, MODEL).await;
    // Exact rollup from the mock's `usage` block.
    assert_eq!(row["input_tokens"], 7);
    assert_eq!(row["output_tokens"], 3);
    assert_eq!(row["total_tokens"], 10);
    // The row carries the full management shape.
    for key in [
        "cache_read_tokens",
        "cache_creation_tokens",
        "quality",
        "requests",
        "failed",
    ] {
        assert!(row.get(key).is_some(), "missing {key}: {row}");
    }
    assert_eq!(row["requests"], 1);
    assert_eq!(row["failed"], 0);

    let client = wreq::Client::new();
    let get = |query: &str| {
        let url = format!("{}/v8/management/observability/usage/stats{query}", f.base);
        let client = client.clone();
        async move { client.get(url).bearer_auth(MGMT_KEY).send().await.unwrap() }
    };

    // Default: the model dimension.
    let r = get("").await;
    assert_eq!(r.status(), 200);
    let first: Value = r.json().await.unwrap();
    assert_eq!(first["group_by"], "model");
    let model_row = first["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == MODEL)
        .unwrap();
    assert_eq!(model_row["input_tokens"], 7);
    assert_eq!(model_row["output_tokens"], 3);
    assert_eq!(model_row["total_tokens"], 10);

    // Provider dimension.
    let r = get("?group_by=provider").await;
    assert_eq!(r.status(), 200);
    let provider: Value = r.json().await.unwrap();
    assert_eq!(provider["group_by"], "provider");
    assert!(
        provider["rows"].as_array().unwrap().iter().any(|r| r["key"] == "xai"),
        "{provider}"
    );

    // Auth-index dimension.
    let r = get("?group_by=auth_index").await;
    assert_eq!(r.status(), 200);
    let auth: Value = r.json().await.unwrap();
    assert_eq!(auth["group_by"], "auth_index");
    assert!(!auth["rows"].as_array().unwrap().is_empty(), "{auth}");

    // Unknown dimension: 400.
    let r = get("?group_by=bogus").await;
    assert_eq!(r.status(), 400, "{}", r.text().await.unwrap());

    // Non-destructive: a second call returns the identical rows (cumulative, not
    // drained).
    let second: Value = get("").await.json().await.unwrap();
    assert_eq!(first["rows"], second["rows"]);
    assert_eq!(first["totals"], second["totals"]);

    // The stats call did not consume the queue.
    let queued: Value = client
        .get(format!("{}/v8/management/observability/usage/queue?count=10", f.base))
        .bearer_auth(MGMT_KEY)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let records = queued.as_array().unwrap();
    assert_eq!(records.len(), 1, "{queued}");
    assert_eq!(records[0]["model"], MODEL);
    assert_eq!(records[0]["tokens"]["input_tokens"], 7);
    assert_eq!(records[0]["tokens"]["output_tokens"], 3);
    assert_eq!(records[0]["tokens"]["total_tokens"], 10);

    // Sensitive fields never reach the stats surface.
    let text = get("").await.text().await.unwrap();
    for needle in ["api_key", "client_ip", "user_agent", "x_forwarded_for"] {
        assert!(!text.contains(needle), "stats leaked {needle}: {text}");
    }
}
