use super::*;

fn chat(messages: Value) -> Value {
    json!({"messages": messages})
}

#[test]
fn every_tool_call_gets_a_result_and_orphans_become_user_text() {
    let history = chat(json!([
        {"role": "system", "content": "be brief"},
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": null, "tool_calls": [
            {"id": "a", "type": "function", "function": {"name": "ls", "arguments": "{\"p\":1}"}},
            {"id": "b", "type": "function", "function": {"name": "cat", "arguments": "{}"}}
        ]},
        {"role": "tool", "tool_call_id": "a", "content": "file"},
        {"role": "tool", "tool_call_id": "zzz", "content": "stray"},
        {"role": "user", "content": "next"}
    ]));
    let (messages, system) = wire_messages(history["messages"].as_array().unwrap());
    assert_eq!(system, "be brief");
    let roles: Vec<_> = messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "tool", "tool", "user", "user"]);
    assert_eq!(messages[2]["content"][0]["toolCallId"], "a");
    assert_eq!(messages[2]["content"][0]["output"]["type"], "text");
    assert_eq!(messages[3]["content"][0]["toolCallId"], "b");
    assert_eq!(messages[3]["content"][0]["output"]["type"], "error-text");
    assert_eq!(messages[1]["content"][0]["input"], json!({"p": 1}));
    assert!(messages[4]["content"][0]["text"].as_str().unwrap().contains("stray"));
}

#[test]
fn efforts_follow_the_model_profile() {
    assert_eq!(
        wire_effort("deepseek/deepseek-v4-flash", "HIGH").as_deref(),
        Some("high")
    );
    assert_eq!(wire_effort("zai-org/GLM-5.2", "xhigh").as_deref(), Some("max"));
    assert_eq!(wire_effort("zai-org/GLM-5.2", "low"), None);
    assert_eq!(wire_effort("deepseek/deepseek-v4-flash", "none"), None);
    assert_eq!(wire_effort("not/listed", "high"), None);
}

#[test]
fn body_carries_the_alpha_generate_contract() {
    let request = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "ls", "description": "d", "parameters": {"type": "object"}}}],
        "tool_choice": {"type": "function", "function": {"name": "ls"}},
        "max_tokens": 99,
        "temperature": 0.5
    });
    let body = build_body(&request, "deepseek/deepseek-v4-flash", Some("high"));
    let params = &body["params"];
    assert_eq!(params["model"], "deepseek/deepseek-v4-flash");
    assert_eq!(params["max_tokens"], 99);
    assert_eq!(params["stream"], true);
    assert_eq!(params["reasoning_effort"], "high");
    assert_eq!(params["tools"][0]["name"], "ls");
    assert!(params["system"].as_str().unwrap().contains("named ls"));
    assert_eq!(body["mode"], "agent");
    let none = build_body(
        &json!({"messages": [], "tool_choice": "none", "tools": request["tools"]}),
        "m",
        None,
    );
    assert_eq!(none["params"]["tools"], json!([]));
    assert!(none["params"].get("reasoning_effort").is_none());
}

#[test]
fn parts_become_chat_chunks_and_a_completion() {
    let parts = [
        json!({"type": "reasoning-delta", "text": "think"}),
        json!({"type": "text-delta", "text": "hel"}),
        json!({"type": "text-delta", "text": "lo"}),
        json!({"type": "tool-call", "toolCallId": "c1", "toolName": "ls", "input": {"p": 1}}),
        json!({"type": "finish-step", "finishReason": "tool-calls",
               "usage": {"inputTokens": 3, "outputTokens": 4, "inputTokenDetails": {"cacheReadTokens": 1}}}),
        json!({"type": "finish", "finishReason": "stop"}),
    ];
    let mut wire = Wire::new("m", true);
    let mut frames = Vec::new();
    for part in &parts {
        frames.extend(wire.event(part).unwrap());
    }
    frames.extend(wire.close());
    assert!(frames[0].contains("\"role\":\"assistant\""));
    let last = frames.last().unwrap();
    assert!(last.contains("\"finish_reason\":\"tool_calls\""), "{last}");
    assert!(
        last.contains("\"total_tokens\":7") && last.contains("\"cached_tokens\":1"),
        "{last}"
    );
    assert_eq!(frames.iter().filter(|f| f.contains("finish_reason\":\"")).count(), 1);
    let done: Value = serde_json::from_slice(&wire.completion()).unwrap();
    let message = &done["choices"][0]["message"];
    assert_eq!(message["content"], "hello");
    assert_eq!(message["reasoning_content"], "think");
    assert_eq!(message["tool_calls"][0]["function"]["arguments"], "{\"p\":1}");
    assert_eq!(done["usage"]["completion_tokens"], 4);
}

#[test]
fn error_parts_and_error_finish_fail_the_turn() {
    let mut wire = Wire::new("m", false);
    let failed = wire
        .event(&json!({"type": "error", "error": {"message": "boom"}}))
        .unwrap_err();
    assert_eq!(failed.status, 502);
    assert!(String::from_utf8_lossy(&failed.body).contains("boom"));
    let mut wire = Wire::new("m", false);
    assert!(wire.event(&json!({"type": "finish", "finishReason": "error"})).is_err());
    let mut wire = Wire::new("m", false);
    let closed = wire.close();
    assert!(closed.last().unwrap().contains("\"finish_reason\":\"stop\""));
}

#[test]
fn ndjson_lines_tolerate_sse_framing_and_junk() {
    assert_eq!(parse_part(b"data: {\"type\":\"finish\"}").unwrap()["type"], "finish");
    assert!(parse_part(b"null").is_none());
    assert!(parse_part(b"not json").is_none());
    assert!(effort_rejected(b"{\"error\":\"Invalid reasoning_effort\"}"));
    assert!(!effort_rejected(b"{\"error\":\"quota\"}"));
}
