//! Memory gate: the heap a large request costs on its way through the proxy.
//!
//! A counting global allocator (this test binary only; release builds use the system
//! allocator untouched) records live bytes and allocation calls while one streamed
//! request of about 306 KB, 1 MB, then 1.9 MB goes from a client through the router to a
//! local fake upstream and back, on each route: `/v1/messages` through the Claude
//! executor to a fake Anthropic upstream; `/v1/responses` through the Codex executor (an
//! API key whose base-url is the fake upstream) and through the xAI executor (a Grok API
//! key); `/v1/chat/completions` through the [OI]-compatible executor; and
//! `/v1/messages/count_tokens` with the same Claude bodies, counted locally as for an
//! OAuth login with the shared o200k_base encoder, which the warm-up builds. The request
//! body is built before counting starts and handed to the router directly. The fake
//! upstream runs on its own thread and runtime, which the allocator does not count: hyper
//! sizes its read buffers from how much the previous read returned, so the upstream's
//! allocations depend on scheduling, not on the proxy. Each size is sent three times and
//! gated on the smallest peak and call count. The ceilings are the `alloc.*` lines of
//! `bench/budgets.txt`; the measured values are printed so a change can update them with a
//! dated note. Every route runs in one test, one after the other, because the counters are
//! process-wide.
//!
//! The same pass also measures CPU: per route and size, the process-wide CPU delta over N
//! requests minus a scaled idle window and the fake upstream's own thread CPU, divided by
//! the request count (plan T18's mechanism, recorded as the pre-optimization baseline).

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::response::IntoResponse;
use futures_util::StreamExt;
use horon_core::config::Config;
use horon_exec::Executors;
use horon_exec::claude::ClaudeExecutor;
use tower::ServiceExt;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static COUNTING: AtomicBool = AtomicBool::new(false);
/// CPU nanoseconds the fake upstream's own thread spent inside its handler, summed over
/// the run. `CLOCK_PROCESS_CPUTIME_ID` counts every thread of this process, so the router
/// measurement subtracts this to isolate the proxy's own cost.
static UPSTREAM_CPU_NS: AtomicUsize = AtomicUsize::new(0);

/// `clock_gettime` for one clock, in nanoseconds.
fn clock_ns(id: libc::clockid_t) -> usize {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid, writable `timespec` for the duration of the call.
    unsafe { libc::clock_gettime(id, &mut ts) };
    (ts.tv_sec as usize) * 1_000_000_000 + (ts.tv_nsec as usize)
}

/// CPU time of every thread in this process.
fn process_cpu_ns() -> usize {
    clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID)
}

/// CPU time of the calling thread.
fn thread_cpu_ns() -> usize {
    clock_ns(libc::CLOCK_THREAD_CPUTIME_ID)
}

thread_local! {
    /// Set on the fake upstream's thread. `const`, so reading it never allocates.
    static UNCOUNTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn counted() -> bool {
    !UNCOUNTED.try_with(std::cell::Cell::get).unwrap_or(false)
}

// SAFETY: every call is forwarded to the system allocator unchanged; the counters are
// plain atomics and never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() && counted() {
            grow(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        if counted() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() && counted() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            grow(new_size);
        }
        p
    }
}

fn grow(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    if COUNTING.load(Ordering::Relaxed) {
        CALLS.fetch_add(1, Ordering::Relaxed);
        PEAK.fetch_max(live, Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The fake upstream drains the request without keeping it and streams a short reply:
/// [OI] chat chunks on the compatible path (`/chat/completions`), Responses events on the
/// Codex and xAI path (`/responses`), Claude events otherwise. Its own thread's CPU is
/// accumulated so the router measurement can subtract it.
async fn upstream(req: Request) -> axum::response::Response {
    let started = thread_cpu_ns();
    let response = upstream_reply(req).await;
    UPSTREAM_CPU_NS.fetch_add(thread_cpu_ns().saturating_sub(started), Ordering::Relaxed);
    response
}

async fn upstream_reply(req: Request) -> axum::response::Response {
    let codex = req.uri().path().ends_with("/responses");
    let compat = req.uri().path().ends_with("/chat/completions");
    let mut body = req.into_body().into_data_stream();
    while let Some(chunk) = body.next().await {
        drop(chunk.unwrap());
    }
    if compat {
        let sse = "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"up-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        return ([("content-type", "text/event-stream")], sse).into_response();
    }
    if codex {
        let sse = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"in_progress\",\"model\":\"gpt-5.5\",\"output\":[]}}\n\n\
                   event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}\n\n\
                   event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"ok\"}\n\n\
                   event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\",\"annotations\":[]}]}}\n\n\
                   event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"gpt-5.5\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n";
        return ([("content-type", "text/event-stream")], sse).into_response();
    }
    let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n\
               event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
               event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
               event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
               event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n\
               event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    ([("content-type", "text/event-stream")], sse).into_response()
}

/// A coding-agent conversation of about `target` bytes, shaped like `bench/messages`:
/// an 18 KB system prompt, 24 tools, then assistant `tool_use` and user `tool_result`
/// turns.
fn conversation(target: usize) -> Bytes {
    use serde_json::json;
    let filler = |n: usize| "Explain the change in this diff and list any risks. ".repeat(n / 52 + 1);
    let tools: Vec<_> = (0..24)
        .map(|i| {
            json!({
                "name": format!("tool_{i}"),
                "description": filler(400),
                "input_schema": {"type": "object", "properties": {"path": {"type": "string"}, "text": {"type": "string"}}},
            })
        })
        .collect();
    let mut messages = vec![json!({"role": "user", "content": filler(2000)})];
    let mut size = 30_000;
    let mut turn = 0;
    while size < target {
        let id = format!("toolu_{turn:06}");
        messages.push(json!({"role": "assistant", "content": [
            {"type": "text", "text": filler(1000)},
            {"type": "tool_use", "id": id, "name": "tool_1", "input": {"path": "src/main.rs"}},
        ]}));
        messages.push(json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": id, "content": filler(20_000)},
        ]}));
        size += 22_000;
        turn += 1;
    }
    let body = json!({
        "model": "claude-opus-5-5",
        "max_tokens": 1024,
        "stream": true,
        "system": [{"type": "text", "text": filler(18_000)}],
        "tools": tools,
        "messages": messages,
    });
    Bytes::from(serde_json::to_vec(&body).unwrap())
}

/// The Claude conversation with its `model` set to `kimi-k2`, the id only the Kimi
/// credential registers, so the request routes to the Kimi executor's Claude-delegated
/// path. Built and re-serialized before counting starts, so it does not affect the
/// measured allocations.
fn kimi_conversation(target: usize) -> Bytes {
    let mut body: serde_json::Value = serde_json::from_slice(&conversation(target)).unwrap();
    body["model"] = serde_json::json!("kimi-k2");
    Bytes::from(serde_json::to_vec(&body).unwrap())
}

/// The same conversation for Codex, in the Responses format: 18 KB of instructions, 24
/// function tools, then reasoning items with encrypted content, function calls and
/// their large outputs.
fn responses_conversation(target: usize, model: &str) -> Bytes {
    use serde_json::json;
    let filler = |n: usize| "Explain the change in this diff and list any risks. ".repeat(n / 52 + 1);
    let tools: Vec<_> = (0..24)
        .map(|i| {
            json!({
                "type": "function",
                "name": format!("tool_{i}"),
                "description": filler(400),
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "text": {"type": "string"}}},
            })
        })
        .collect();
    let mut input =
        vec![json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": filler(2000)}]})];
    let mut size = 30_000;
    let mut turn = 0;
    while size < target {
        let call = format!("call_{turn:06}");
        input.push(json!({"type": "reasoning", "id": format!("rs_{turn:06}"), "summary": [{"type": "summary_text", "text": filler(600)}], "encrypted_content": "gAAAAABo".repeat(150)}));
        input.push(json!({"type": "function_call", "id": format!("fc_{turn:06}"), "call_id": call, "name": "tool_1", "arguments": "{\"path\":\"src/main.rs\"}"}));
        input.push(json!({"type": "function_call_output", "call_id": call, "output": filler(20_000)}));
        size += 22_000;
        turn += 1;
    }
    let body = json!({
        "model": model,
        "instructions": filler(18_000),
        "input": input,
        "tools": tools,
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "reasoning": {"effort": "medium", "summary": "auto"},
        "store": false,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        "prompt_cache_key": "alloc-budget",
    });
    Bytes::from(serde_json::to_vec(&body).unwrap())
}

/// The same conversation for an [OI]-compatible provider, in the Chat Completions
/// format: a system message, 24 function tools, then assistant tool calls and their
/// large outputs.
fn chat_conversation(target: usize, model: &str) -> Bytes {
    use serde_json::json;
    let filler = |n: usize| "Explain the change in this diff and list any risks. ".repeat(n / 52 + 1);
    let tools: Vec<_> = (0..24)
        .map(|i| {
            json!({
                "type": "function",
                "function": {
                    "name": format!("tool_{i}"),
                    "description": filler(400),
                    "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "text": {"type": "string"}}},
                },
            })
        })
        .collect();
    let mut messages = vec![
        json!({"role": "system", "content": filler(18_000)}),
        json!({"role": "user", "content": filler(2000)}),
    ];
    let mut size = 30_000;
    let mut turn = 0;
    while size < target {
        let id = format!("call_{turn:06}");
        messages.push(json!({"role": "assistant", "content": null, "tool_calls": [
            {"id": id, "type": "function", "function": {"name": "tool_1", "arguments": "{\"path\":\"src/main.rs\"}"}},
        ]}));
        messages.push(json!({"role": "tool", "tool_call_id": id, "content": filler(20_000)}));
        size += 22_000;
        turn += 1;
    }
    let body = json!({
        "model": model,
        "stream": true,
        "messages": messages,
        "tools": tools,
    });
    Bytes::from(serde_json::to_vec(&body).unwrap())
}

/// The ceiling for `key` in bench/budgets.txt: `<key> <value> <tolerance %>`, read the way
/// bench/gate.sh reads it.
fn budget(key: &str) -> usize {
    include_str!("../../../bench/budgets.txt")
        .lines()
        .filter_map(|l| l.split('#').next())
        .find_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            (parts.first() == Some(&key)).then(|| {
                let value: f64 = parts[1].replace('_', "").parse().unwrap();
                let tolerance: f64 = parts.get(2).map_or(0.0, |t| t.parse().unwrap());
                (value * (1.0 + tolerance / 100.0)) as usize
            })
        })
        .unwrap_or_else(|| panic!("bench/budgets.txt has no {key}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_requests_stay_within_their_heap_budget() {
    // The fake upstream: its own thread and single-threaded runtime, not counted.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let upstream_url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        UNCOUNTED.with(|u| u.set(true));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, axum::Router::new().fallback(upstream))
                .await
                .unwrap()
        });
    });
    let dir: PathBuf = std::env::temp_dir().join(format!("cpa-alloc-budget-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("claude.json"),
        r#"{"type":"claude","access_token":"tok-A","email":"a@example.com"}"#,
    )
    .unwrap();
    // Kimi (a device-login provider) on its Claude-delegated path: a static token and a
    // base-url pointed at the fake upstream, so no device identity or token refresh runs
    // (an absent refresh token keeps the credential `Ready`). The body's model
    // `kimi-k2` is one only this credential registers, so the request routes here and
    // nowhere else.
    std::fs::write(
        dir.join("kimi.json"),
        format!(r#"{{"type":"kimi","access_token":"tok-kimi","base_url":"{upstream_url}"}}"#),
    )
    .unwrap();
    let mut config = Config::parse(&format!(
        r#"codex-api-key:
  - api-key: sk-FAKE-codex
    base-url: {upstream_url}
    models:
      - name: gpt-5.5
api-keys:
  openai-compatibility:
    - name: Acme
      base-url: {upstream_url}/v1
      models:
        - name: up-model
          alias: fast
      keys:
        - api-key: sk-fake-upstream
  xai:
    - name: xai-1
      base-url: {upstream_url}/v1
      keys:
        - api-key: sk-fake-xai
"#
    ))
    .unwrap();
    config.api_keys = vec!["client-key-1".into()];
    config.auth_dir = dir.clone();
    let mut creds: Vec<_> = horon_core::config::credentials::from_auth_dir(&config)
        .into_iter()
        .map(horon_server::testing::local)
        .collect();
    creds.extend(horon_core::config::credentials::from_config(&config));
    let executors = Executors {
        claude: ClaudeExecutor::new(&upstream_url).unwrap(),
        codex: Default::default(),
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    };
    let rt = Arc::new(horon_server::testing::runtime(config, creds, executors));
    horon_server::install_registry(&rt);
    let app = horon_server::router(rt);

    // `route` is "claude"/"kimi" (`/v1/messages`), "count" (`/v1/messages/count_tokens`),
    // "compat" (`/v1/chat/completions`) or "codex"/"xai" (`/v1/responses`).
    let send = |route: &'static str, body: Bytes| {
        let app = app.clone();
        async move {
            let (path, done): (_, &[u8]) = match route {
                "claude" | "kimi" => ("/v1/messages", b"message_stop"),
                "count" => ("/v1/messages/count_tokens", b"\"input_tokens\""),
                "compat" => ("/v1/chat/completions", b"[DONE]"),
                _ => ("/v1/responses", b"response.completed"),
            };
            let request = axum::http::Request::post(path)
                .header("x-api-key", "client-key-1")
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            let response = app.oneshot(request).await.unwrap();
            let status = response.status();
            let text = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert_eq!(status, 200, "{text:?}");
            assert!(text.windows(done.len()).any(|w| w == done), "{text:?}");
        }
    };

    // CPU baseline: the cost of an idle window (worker polling, upstream idle), measured
    // once, is scaled to each request window and subtracted, together with the fake
    // upstream's own CPU, from the process-wide CPU delta.
    let idle_wall = Instant::now();
    let idle_cpu = process_cpu_ns();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let idle_cpu = process_cpu_ns() - idle_cpu;
    let idle_wall = idle_wall.elapsed().as_nanos() as f64;
    let idle_per_ns = idle_cpu as f64 / idle_wall;

    let mut over = Vec::new();
    let mut cpu_rows = Vec::new();
    for (route, name, target) in [
        ("claude", "306k", 306_000),
        ("claude", "1000k", 1_000_000),
        ("claude", "1900k", 1_900_000),
        ("count", "count.306k", 306_000),
        ("count", "count.1000k", 1_000_000),
        ("count", "count.1900k", 1_900_000),
        ("codex", "codex.306k", 306_000),
        ("codex", "codex.1000k", 1_000_000),
        ("codex", "codex.1900k", 1_900_000),
        ("xai", "xai.306k", 306_000),
        ("xai", "xai.1000k", 1_000_000),
        ("xai", "xai.1900k", 1_900_000),
        ("compat", "compat.306k", 306_000),
        ("compat", "compat.1000k", 1_000_000),
        ("compat", "compat.1900k", 1_900_000),
        ("kimi", "kimi.306k", 306_000),
        ("kimi", "kimi.1000k", 1_000_000),
        ("kimi", "kimi.1900k", 1_900_000),
    ] {
        let body = match route {
            "codex" => responses_conversation(target, "gpt-5.5"),
            "xai" => responses_conversation(target, "grok-4.7"),
            "compat" => chat_conversation(target, "fast"),
            "kimi" => kimi_conversation(target),
            _ => conversation(target),
        };
        // Warm-up: connection pool, registries and lazy statics.
        send(route, body.clone()).await;
        let (mut peak, mut calls) = (usize::MAX, usize::MAX);
        for _ in 0..3 {
            let base = LIVE.load(Ordering::Relaxed);
            PEAK.store(base, Ordering::Relaxed);
            CALLS.store(0, Ordering::Relaxed);
            COUNTING.store(true, Ordering::Relaxed);
            send(route, body.clone()).await;
            COUNTING.store(false, Ordering::Relaxed);
            peak = peak.min(PEAK.load(Ordering::Relaxed).saturating_sub(base));
            calls = calls.min(CALLS.load(Ordering::Relaxed));
        }
        let (peak_max, calls_max) = (
            budget(&format!("alloc.{name}.peak_bytes")),
            budget(&format!("alloc.{name}.allocations")),
        );
        println!(
            "{name}: body {} B, peak live {peak} B ({:.1}x body, ceiling {peak_max}), {calls} allocations (ceiling {calls_max})",
            body.len(),
            peak as f64 / body.len() as f64,
        );
        // Per-request CPU: N sends, minus the scaled idle cost and the upstream's CPU.
        const N: u32 = 20;
        let up_before = UPSTREAM_CPU_NS.load(Ordering::Relaxed);
        let cpu_before = process_cpu_ns();
        let wall_before = Instant::now();
        for _ in 0..N {
            send(route, body.clone()).await;
        }
        let wall = wall_before.elapsed().as_nanos() as f64;
        let cpu_delta = (process_cpu_ns() - cpu_before) as f64;
        let upstream = (UPSTREAM_CPU_NS.load(Ordering::Relaxed) - up_before) as f64;
        let request_cpu = (cpu_delta - idle_per_ns * wall - upstream).max(0.0) / N as f64;
        let cpu_ms = request_cpu / 1_000_000.0;
        println!(
            "{name}: request CPU {cpu_ms:.3} ms ({N} requests, body {} B)",
            body.len()
        );
        cpu_rows.push((name, body.len(), cpu_ms));
        if peak > peak_max {
            over.push(format!("alloc.{name}.peak_bytes {peak} > {peak_max}"));
        }
        if calls > calls_max {
            over.push(format!("alloc.{name}.allocations {calls} > {calls_max}"));
        }
    }
    println!("--- CPU per request (ms) ---");
    for (name, bytes, ms) in &cpu_rows {
        println!("{name}\t{bytes}\t{ms:.3}");
    }
    let _ = std::fs::remove_dir_all(dir);
    assert!(over.is_empty(), "over budget (bench/budgets.txt): {over:?}");
}
