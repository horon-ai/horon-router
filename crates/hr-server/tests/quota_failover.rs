//! Rust-only quota strategy through the real HTTP router and local provider.
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use hr_core::{config::Config, credential::Credential};
use hr_exec::Executors;
use serde_json::{Value, json};

const MODEL: &str = "gpt-5.6-luna";

struct Seen {
    credits: Value,
    requests: Mutex<Vec<String>>,
}

async fn upstream(State(seen): State<Arc<Seen>>, req: Request) -> Response {
    let path = req.uri().path().to_owned();
    seen.requests.lock().unwrap().push(path.clone());
    match path.as_str() {
        "/exa.seat_management_pb.SeatManagementService/GetUserStatus" => {
            // Root.user(1).plan_status(13): daily/weekly remaining = 0,
            // reset timestamps = 2030-03-17 (both explicitly present).
            let mut plan = vec![0x70, 0, 0x78, 0];
            for field in [17u64, 18] {
                for mut n in [field << 3, 1_900_000_000] {
                    while n >= 128 {
                        plan.push((n as u8 & 127) | 128);
                        n >>= 7;
                    }
                    plan.push(n as u8);
                }
            }
            let mut user = vec![0x6a, u8::try_from(plan.len()).unwrap()];
            user.extend(plan);
            let mut root = vec![0x0a, u8::try_from(user.len()).unwrap()];
            root.extend(user);
            ([("content-type", "application/proto")], root).into_response()
        }
        "/alpha/whoami" => axum::Json(json!({})).into_response(),
        "/alpha/billing/credits" => axum::Json(seen.credits.clone()).into_response(),
        "/alpha/billing/subscriptions" => axum::Json(json!({})).into_response(),
        "/go/v1/usage" => axum::Json(seen.credits["goUsage"].clone()).into_response(),
        "/alpha/generate" if seen.credits["generateQuota"] == true => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(
                json!({"error":{"code":"BAD_REQUEST","message":"You have insufficient credits to make this request."}}),
            ),
        )
            .into_response(),
        "/alpha/generate" => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({"error":{"type":"invalid_request_error","message":"mock invalid prompt"}})),
        )
            .into_response(),
        "/v1/chat/completions" | "/go/v1/chat/completions" => axum::Json(json!({
            "id":"local","object":"chat.completion","created":1,"model":MODEL,
            "choices":[{"index":0,"message":{"role":"assistant","content":"fallback-ok"},"finish_reason":"stop"}]
        }))
        .into_response(),
        _ => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn serve(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, task)
}

async fn scenario(strategy: &str, credits: Value) -> (u16, Value, Vec<String>) {
    let seen = Arc::new(Seen {
        credits,
        requests: Default::default(),
    });
    let (base, upstream_task) = serve(axum::Router::new().fallback(upstream).with_state(seen.clone())).await;
    let auth_dir = std::env::temp_dir().join(format!("cpa-quota-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&auth_dir).unwrap();
    let go = if seen.credits.get("goUsage").is_some() {
        format!(
            "    - name: opencode-go\n      base-url: {base}/go/v1\n      keys: [{{api-key: fake-go, priority: 101}}]\n      models: [{{name: {MODEL}}}]\n"
        )
    } else {
        String::new()
    };
    let fallback_model = seen.credits["fallbackModel"].as_str().unwrap_or(MODEL);
    let cfg = Config::parse(&format!(
        "auth-dir: {}\nrouting:\n  strategy: {strategy}\n  session-affinity: true\n\
         api-keys:\n  openai-compatibility:\n    - name: local-fallback\n      base-url: {base}/v1\n\
         \x20     keys: [{{api-key: fake-fallback}}]\n      models: [{{name: {fallback_model}}}]\n{go}",
        auth_dir.display()
    ))
    .unwrap();
    let mut credentials = hr_core::config::credentials::load(&cfg);
    let metadata = json!({"type":"command-code","api_key":"fake-command","base_url":base,"priority":100});
    credentials.push(
        Credential::from_file(
            &auth_dir,
            &auth_dir.join("command.json"),
            metadata.as_object().unwrap().clone(),
        )
        .unwrap(),
    );
    if seen.credits["devin"] == true {
        let metadata = json!({"type":"devin","session_token":"fake-devin","base_url":base,"priority":102});
        let mut devin = Credential::from_file(
            &auth_dir,
            &auth_dir.join("devin.json"),
            metadata.as_object().unwrap().clone(),
        )
        .unwrap();
        devin.attributes.insert(
            "model_aliases".into(),
            json!([
                {"name":"devin/gpt-5-6-luna","alias":MODEL}
            ])
            .to_string(),
        );
        credentials.push(devin);
    }
    let executors = Executors {
        claude: hr_exec::claude::ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
        codex: Default::default(),
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    };
    let rt = Arc::new(hr_server::testing::runtime(cfg, credentials, executors));
    let (url, proxy_task) = serve(hr_server::router(rt)).await;
    let response = wreq::Client::new()
        .post(format!("{url}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(json!({"model":MODEL,"messages":[{"role":"user","content":"hello"}]}).to_string())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    if seen.credits["repeat"] == true {
        let response = wreq::Client::new()
            .post(format!("{url}/v1/chat/completions"))
            .header("content-type", "application/json")
            .body(json!({"model":MODEL,"messages":[{"role":"user","content":"hello"}]}).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        let reply = response.text().await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&reply).unwrap(), body);
    }
    let requests = seen.requests.lock().unwrap().clone();
    proxy_task.abort();
    upstream_task.abort();
    std::fs::remove_dir_all(&auth_dir).unwrap();
    (status, body, requests)
}

#[tokio::test]
async fn exhausted_high_priority_provider_falls_back_to_the_same_model() {
    let (status, body, requests) = scenario(
        "quota-failover",
        json!({"credits":{
            "monthlyCredits":0,"purchasedCredits":0,"freeCredits":0
        }}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["choices"][0]["message"]["content"], "fallback-ok");
    assert!(requests.iter().any(|p| p == "/v1/chat/completions"));
    assert!(!requests.iter().any(|p| p == "/alpha/generate"));
}

#[tokio::test]
async fn unknown_quota_does_not_exclude_the_preferred_provider() {
    let (status, _, requests) = scenario("quota-failover", json!({"error":"unavailable"})).await;
    assert_eq!(status, 400);
    assert!(requests.iter().any(|p| p == "/alpha/generate"));
}

#[tokio::test]
async fn existing_fill_first_does_not_probe_usage() {
    let (status, _, requests) = scenario(
        "fill-first",
        json!({"credits":{
            "monthlyCredits":0,"purchasedCredits":0,"freeCredits":0
        }}),
    )
    .await;
    assert_eq!(status, 400);
    assert!(!requests.iter().any(|p| p == "/alpha/billing/credits"));
}

#[tokio::test]
async fn billing_exhaustion_after_a_healthy_probe_retries_another_provider() {
    let (status, body, requests) = scenario(
        "quota-failover",
        json!({"generateQuota":true,"credits":{
            "monthlyCredits":10,"purchasedCredits":0,"freeCredits":0
        }}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["choices"][0]["message"]["content"], "fallback-ok");
    let generate = requests.iter().position(|p| p == "/alpha/generate").unwrap();
    let fallback = requests.iter().position(|p| p == "/v1/chat/completions").unwrap();
    assert!(generate < fallback);
}

#[tokio::test]
async fn exhausted_opencode_go_and_command_code_use_the_same_model_elsewhere() {
    let (status, body, requests) = scenario(
        "quota-failover",
        json!({
            "credits":{"monthlyCredits":0,"purchasedCredits":0,"freeCredits":0},
            "goUsage":{"usage":{"rolling":{"percent":100}}}
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(requests.iter().any(|p| p == "/go/v1/usage"));
    assert!(!requests.iter().any(|p| p == "/go/v1/chat/completions"));
    assert!(requests.iter().any(|p| p == "/v1/chat/completions"));
}

#[tokio::test]
async fn devin_status_exhaustion_falls_back_without_sending_generation() {
    let (status, body, requests) = scenario(
        "quota-failover",
        json!({"devin":true,"credits":{"monthlyCredits":0,"purchasedCredits":0,"freeCredits":0}}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(requests.iter().any(|p| p.ends_with("/GetUserStatus")));
    assert!(!requests.iter().any(|p| p.ends_with("/GetChatMessage")));
    assert!(requests.iter().any(|p| p == "/v1/chat/completions"));
}

#[tokio::test]
async fn repeated_turns_reuse_quota_observation_without_probing_again() {
    let (status, body, requests) = scenario(
        "quota-failover",
        json!({"repeat":true,"credits":{"monthlyCredits":0,"purchasedCredits":0,"freeCredits":0}}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(requests.iter().filter(|p| *p == "/alpha/billing/credits").count(), 1);
    assert_eq!(requests.iter().filter(|p| *p == "/v1/chat/completions").count(), 2);
}

#[tokio::test]
async fn another_model_is_not_used_when_all_same_model_accounts_are_exhausted() {
    let (status, _, requests) = scenario(
        "quota-failover",
        json!({"fallbackModel":"different-model","credits":{
            "monthlyCredits":0,"purchasedCredits":0,"freeCredits":0
        }}),
    )
    .await;
    assert_eq!(status, 503);
    assert!(
        !requests
            .iter()
            .any(|p| p == "/v1/chat/completions" || p == "/alpha/generate")
    );
}
