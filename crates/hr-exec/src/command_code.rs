//! Command Code executor (no Go counterpart; the wire follows opencodex's `command-code`
//! adapter, `.vendor/opencodex/src/adapters/command-code.ts`).
//!
//! Every client format is translated to OpenAI Chat, rebuilt as the `/alpha/generate`
//! body (AI SDK style messages, `tool-call` and `tool-result` parts) and sent with the
//! account's API key. The answer is always an NDJSON stream of AI SDK parts; they become
//! OpenAI chat chunks, which the registered translator turns into the client's format.
//! Non-streaming clients get one chat completion built from the same parts.

use std::collections::VecDeque;

use bytes::Bytes;
use futures_util::StreamExt;
use hr_common::json as gj;
use hr_common::thinking::{ModelCaps, parse_suffix};
use hr_core::config::Config;
use hr_core::credential::Credential;
use hr_core::exec::{
    ExecError, ExecRequest, ExecResponse, ExecStream, FailureScope, Operation, ResponseBody, UsageSink,
};
use hr_core::format::Format;
use hr_core::registry::command_code as catalog;
use hr_translate::{ResponseCtx, StreamTranslator};
use serde_json::{Value, json};

use crate::openai_compat as compat;
use crate::openai_compat_http::{self as wire, Clients, GoHeaders};

pub const PROVIDER: &str = "command-code";
pub const DEFAULT_BASE_URL: &str = "https://api.commandcode.ai";
const MAX_LINE: usize = 52_428_800;
const DEFAULT_MAX_TOKENS: i64 = 64_000;
const CLI_VERSION: &str = "0.52.1";
const MISSING_RESULT: &str = "[cpa] no tool result was recorded for this tool call; execution status unknown.";

pub fn handles(provider: &str) -> bool {
    provider == PROVIDER
}

#[derive(Default)]
pub struct CommandCodeExecutor {
    clients: Clients,
}

fn creds(c: &Credential) -> (String, String) {
    let attr = |k: &str| c.attributes.get(k).map(|v| v.trim().to_owned()).unwrap_or_default();
    let meta = |k: &str| c.str(k).map(|v| v.trim().to_owned()).unwrap_or_default();
    let first = |values: [String; 2]| values.into_iter().find(|v| !v.is_empty()).unwrap_or_default();
    let key = first([attr("api_key"), meta("api_key")]);
    let key = if key.is_empty() { meta("access_token") } else { key };
    let base = first([attr("base_url"), meta("base_url")]);
    (
        if base.is_empty() {
            DEFAULT_BASE_URL.to_owned()
        } else {
            base
        },
        key,
    )
}

fn session_id(req: &ExecRequest) -> String {
    let key = [&req.execution_session, &req.session, &req.derived_session]
        .into_iter()
        .filter_map(|s| s.as_deref().map(str::trim))
        .find(|s| !s.is_empty());
    match key {
        Some(key) => {
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("command-code:{key}").as_bytes()).to_string()
        }
        None => uuid::Uuid::new_v4().to_string(),
    }
}

fn headers(key: &str, req: &ExecRequest) -> GoHeaders {
    let mut h = GoHeaders::new();
    h.set("Authorization", format!("Bearer {key}"));
    h.set("Content-Type", "application/json");
    h.set("User-Agent", "cli");
    h.set("x-command-code-version", CLI_VERSION);
    h.set("x-cli-environment", "production");
    h.set("x-taste-learning", "false");
    h.set("x-co-flag", "false");
    h.set("x-session-id", session_id(req));
    h
}

/// The effort this model's profile accepts for `requested`; `xhigh` folds into `max`
/// where the ladder stops there. Anything else the profile lacks is dropped.
fn wire_effort(model: &str, requested: &str) -> Option<String> {
    let requested = requested.trim().to_lowercase();
    if requested.is_empty() || requested == "none" {
        return None;
    }
    let ladder = catalog::efforts(model)?;
    if ladder.contains(&requested.as_str()) {
        return Some(requested);
    }
    (requested == "xhigh" && ladder.contains(&"max")).then(|| "max".to_owned())
}

fn effort_rejected(body: &[u8]) -> bool {
    regex::Regex::new(r"(?i)reasoning[_ -]?effort|unsupported effort|invalid effort")
        .is_ok_and(|re| re.is_match(&String::from_utf8_lossy(body)))
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn media_type(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("data:") {
        return rest
            .split([';', ','])
            .next()
            .filter(|m| !m.is_empty())
            .map(str::to_owned);
    }
    let ext = url.split(['?', '#']).next()?.rsplit_once('.')?.1.to_lowercase();
    let known = [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("webp", "image/webp"),
        ("bmp", "image/bmp"),
    ];
    known.iter().find(|(e, _)| *e == ext).map(|(_, m)| (*m).to_owned())
}

fn user_parts(content: &Value) -> Vec<Value> {
    match content {
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                Some("text") => Some(json!({"type": "text", "text": p["text"]})),
                Some("image_url") => {
                    let url = p["image_url"]["url"].as_str().or_else(|| p["image_url"].as_str())?;
                    let mut part = json!({"type": "image", "image": url});
                    if let Some(m) = media_type(url) {
                        part["mediaType"] = m.into();
                    }
                    Some(part)
                }
                _ => None,
            })
            .collect(),
        other => vec![json!({"type": "text", "text": text_of(other)})],
    }
}

fn tool_result(id: &str, name: &str, error: bool, value: &str) -> Value {
    json!({"role": "tool", "content": [{
        "type": "tool-result",
        "toolCallId": id,
        "toolName": name,
        "output": {"type": if error { "error-text" } else { "text" }, "value": value},
    }]})
}

/// The wire pairs every assistant `tool-call` with a following `tool-result`, which chat
/// history does not guarantee: a call without a result gets an error result, a result
/// without a call rides a user message.
fn wire_messages(messages: &[Value]) -> (Vec<Value>, String) {
    let mut out: Vec<Value> = Vec::new();
    let mut system: Vec<String> = Vec::new();
    let mut pending: Vec<(String, String)> = Vec::new();
    fn close(out: &mut Vec<Value>, pending: &mut Vec<(String, String)>) {
        for (id, name) in pending.drain(..) {
            out.push(tool_result(&id, &name, true, MISSING_RESULT));
        }
    }
    for m in messages {
        let content = &m["content"];
        match m["role"].as_str().unwrap_or_default() {
            "system" | "developer" => system.push(text_of(content)),
            "assistant" => {
                close(&mut out, &mut pending);
                let mut parts = Vec::new();
                let text = text_of(content);
                if !text.is_empty() {
                    parts.push(json!({"type": "text", "text": text}));
                }
                for call in m["tool_calls"].as_array().into_iter().flatten() {
                    let id = call["id"].as_str().unwrap_or_default();
                    let name = call["function"]["name"].as_str().unwrap_or_default();
                    let input = call["function"]["arguments"]
                        .as_str()
                        .and_then(|a| serde_json::from_str::<Value>(a).ok())
                        .unwrap_or_else(|| json!({}));
                    parts.push(json!({"type": "tool-call", "toolCallId": id, "toolName": name, "input": input}));
                    pending.push((id.to_owned(), name.to_owned()));
                }
                out.push(json!({"role": "assistant", "content": parts}));
            }
            "tool" => {
                let id = m["tool_call_id"].as_str().unwrap_or_default();
                let text = text_of(content);
                match pending.iter().position(|(pid, _)| pid == id) {
                    Some(i) => {
                        let (id, name) = pending.remove(i);
                        out.push(tool_result(&id, &name, false, &text));
                    }
                    None => {
                        close(&mut out, &mut pending);
                        let note = format!("[tool result without adjacent tool call: {id}]\n{text}");
                        out.push(json!({"role": "user", "content": [{"type": "text", "text": note}]}));
                    }
                }
            }
            _ => {
                close(&mut out, &mut pending);
                out.push(json!({"role": "user", "content": user_parts(content)}));
            }
        }
    }
    close(&mut out, &mut pending);
    (out, system.join("\n\n"))
}

fn wire_tools(chat: &Value) -> (Vec<Value>, Option<String>) {
    let choice = &chat["tool_choice"];
    if choice.as_str() == Some("none") {
        return (Vec::new(), None);
    }
    let forced = choice["function"]["name"].as_str();
    let tools: Vec<Value> = chat["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| forced.is_none_or(|f| t["function"]["name"].as_str() == Some(f)))
        .map(|t| {
            let f = &t["function"];
            json!({
                "name": f["name"],
                "description": f["description"],
                "input_schema": if f["parameters"].is_null() { json!({"type": "object"}) } else { f["parameters"].clone() },
            })
        })
        .collect();
    let instruction = match (forced, choice.as_str()) {
        (Some(name), _) => Some(format!(
            "Tool choice is required for this turn. Call the advertised tool named {name} before answering."
        )),
        (None, Some("required")) => Some(
            "Tool choice is required for this turn. Make at least one call from the advertised tool catalog before answering."
                .to_owned(),
        ),
        _ => None,
    };
    (tools, instruction)
}

fn build_body(chat: &Value, model: &str, effort: Option<&str>) -> Value {
    let (messages, mut system) = wire_messages(chat["messages"].as_array().map_or(&[][..], Vec::as_slice));
    let (tools, instruction) = wire_tools(chat);
    if let Some(instruction) = instruction {
        system = if system.is_empty() {
            instruction
        } else {
            format!("{system}\n\n{instruction}")
        };
    }
    let max_tokens = chat["max_completion_tokens"]
        .as_i64()
        .or_else(|| chat["max_tokens"].as_i64())
        .unwrap_or(DEFAULT_MAX_TOKENS);
    let mut params = json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "system": system,
        "max_tokens": max_tokens,
        "stream": true,
    });
    if let Some(t) = chat["temperature"].as_f64() {
        params["temperature"] = t.into();
    }
    if let Some(e) = effort {
        params["reasoning_effort"] = e.into();
    }
    json!({
        "config": {
            "date": chrono::Utc::now().format("%Y-%m-%d").to_string(),
            "environment": std::env::consts::OS,
            "structure": [],
            "isGitRepo": false,
            "currentBranch": "",
            "mainBranch": "",
            "gitStatus": "",
            "recentCommits": [],
        },
        "memory": "",
        "taste": null,
        "skills": null,
        "permissionMode": "standard",
        "mode": "agent",
        "params": params,
    })
}

fn usage_json(u: &Value) -> Value {
    let input = u["inputTokens"].as_i64().unwrap_or(0);
    let output = u["outputTokens"].as_i64().unwrap_or(0);
    let mut out = json!({"prompt_tokens": input, "completion_tokens": output, "total_tokens": input + output});
    if let Some(cached) = u["inputTokenDetails"]["cacheReadTokens"].as_i64() {
        out["prompt_tokens_details"] = json!({"cached_tokens": cached});
    }
    out
}

fn event_error(ev: &Value) -> String {
    let e = &ev["error"];
    e.as_str()
        .or_else(|| e["message"].as_str())
        .filter(|m| !m.is_empty())
        .unwrap_or("Command Code stream error")
        .to_owned()
}

struct Wire {
    id: String,
    model: String,
    created: i64,
    role_sent: bool,
    tool_index: usize,
    saw_finish: bool,
    collect: bool,
    text: String,
    reasoning: String,
    calls: Vec<Value>,
    finish: Option<&'static str>,
    usage: Option<Value>,
}

impl Wire {
    fn new(model: &str, collect: bool) -> Self {
        Self {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            model: model.to_owned(),
            created: chrono::Utc::now().timestamp(),
            role_sent: false,
            tool_index: 0,
            saw_finish: false,
            collect,
            text: String::new(),
            reasoning: String::new(),
            calls: Vec::new(),
            finish: None,
            usage: None,
        }
    }

    fn frame(&self, delta: Value, finish: Option<&str>, usage: Option<&Value>) -> String {
        let mut v = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        });
        if let Some(u) = usage {
            v["usage"] = u.clone();
        }
        format!("data: {v}")
    }

    fn open(&mut self, out: &mut Vec<String>) {
        if !self.role_sent {
            self.role_sent = true;
            out.push(self.frame(json!({"role": "assistant", "content": ""}), None, None));
        }
    }

    fn end(&mut self, reason: &str, usage: Option<Value>, out: &mut Vec<String>) {
        self.open(out);
        self.saw_finish = true;
        let reason = match reason {
            "tool_calls" | "tool-calls" | "tool_use" => "tool_calls",
            "length" | "max_tokens" => "length",
            "content-filter" | "content_filter" => "content_filter",
            _ if !self.calls.is_empty() || self.tool_index > 0 => "tool_calls",
            _ => "stop",
        };
        self.finish = Some(reason);
        out.push(self.frame(json!({}), Some(reason), usage.as_ref()));
        self.usage = usage;
    }

    fn event(&mut self, ev: &Value) -> Result<Vec<String>, ExecError> {
        let mut out = Vec::new();
        let text = ev["text"].as_str().or_else(|| ev["delta"].as_str());
        match ev["type"].as_str().unwrap_or_default() {
            "text-delta" => {
                if let Some(t) = text {
                    self.open(&mut out);
                    if self.collect {
                        self.text.push_str(t);
                    }
                    out.push(self.frame(json!({"content": t}), None, None));
                }
            }
            "reasoning-delta" => {
                if let Some(t) = text {
                    self.open(&mut out);
                    if self.collect {
                        self.reasoning.push_str(t);
                    }
                    out.push(self.frame(json!({"reasoning_content": t}), None, None));
                }
            }
            "tool-call" => {
                let id = ev["toolCallId"]
                    .as_str()
                    .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
                let name = ev["toolName"].as_str().unwrap_or("tool");
                let input = if ev["input"].is_null() {
                    &ev["args"]
                } else {
                    &ev["input"]
                };
                let arguments = match input {
                    Value::String(s) => s.clone(),
                    Value::Null => "{}".to_owned(),
                    other => other.to_string(),
                };
                self.open(&mut out);
                let call = json!({"id": id, "type": "function", "function": {"name": name, "arguments": arguments}});
                if self.collect {
                    self.calls.push(call.clone());
                }
                let mut indexed = call;
                indexed["index"] = self.tool_index.into();
                self.tool_index += 1;
                out.push(self.frame(json!({"tool_calls": [indexed]}), None, None));
            }
            "finish" | "finish-step" => {
                if !self.saw_finish {
                    let reason = ev["rawFinishReason"].as_str().or_else(|| ev["finishReason"].as_str());
                    let usage = ev.get("totalUsage").or_else(|| ev.get("usage")).map(usage_json);
                    if reason == Some("error") {
                        return Err(compat::status_err(
                            502,
                            "Command Code upstream ended the turn with finishReason \"error\"",
                        ));
                    }
                    self.end(reason.unwrap_or_default(), usage, &mut out);
                }
            }
            "error" => return Err(compat::status_err(502, event_error(ev))),
            _ => {}
        }
        Ok(out)
    }

    fn close(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.saw_finish {
            self.end("", None, &mut out);
        }
        out
    }

    fn completion(&self) -> Vec<u8> {
        let mut message = json!({"role": "assistant", "content": self.text});
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = self.reasoning.clone().into();
        }
        if !self.calls.is_empty() {
            message["tool_calls"] = Value::Array(self.calls.clone());
        }
        let mut out = json!({
            "id": self.id,
            "object": "chat.completion",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": 0, "message": message, "finish_reason": self.finish.unwrap_or("stop")}],
        });
        if let Some(u) = &self.usage {
            out["usage"] = u.clone();
        }
        out.to_string().into_bytes()
    }
}

fn parse_part(line: &[u8]) -> Option<Value> {
    let line = std::str::from_utf8(line).ok()?.trim();
    let line = line.strip_prefix("data:").map_or(line, str::trim);
    serde_json::from_str::<Value>(line).ok().filter(Value::is_object)
}

struct Run {
    lines: ExecStream,
    wire: Wire,
    translator: Box<dyn StreamTranslator>,
    ready: VecDeque<Result<Bytes, ExecError>>,
    capture: wire::Capture,
    usage: UsageSink,
    ended: bool,
}

impl Run {
    fn fail(&mut self, error: ExecError) {
        self.capture.error(&error);
        wire::publish_failure(&self.usage, &error);
        let flushed = self.translator.flush_frames();
        self.ready.extend(flushed.into_iter().map(Ok));
        self.ready.push_back(Err(error));
        self.ended = true;
    }

    fn translate(&mut self, frame: &str) -> bool {
        self.usage.response_line(Format::OpenAI, frame.as_bytes());
        let mut event = frame.as_bytes().to_vec();
        event.extend_from_slice(b"\n\n");
        match self.translator.event(&event) {
            Ok(out) => {
                self.ready.extend(out.into_iter().map(Ok));
                false
            }
            Err(e) => {
                self.fail(compat::status_err(502, e.to_string()));
                true
            }
        }
    }

    fn line(&mut self, line: &[u8]) {
        let Some(part) = parse_part(line) else { return };
        match self.wire.event(&part) {
            Ok(frames) => {
                for frame in frames {
                    if self.translate(&frame) {
                        return;
                    }
                }
            }
            Err(e) => self.fail(e),
        }
    }

    fn finish(&mut self) {
        for frame in self.wire.close() {
            if self.translate(&frame) {
                return;
            }
        }
        if self.translate("data: [DONE]") {
            return;
        }
        match self.translator.finish() {
            Ok(out) => self.ready.extend(out.into_iter().map(Ok)),
            Err(e) => {
                self.fail(compat::status_err(502, e.to_string()));
                return;
            }
        }
        self.ended = true;
    }
}

fn run(run: Run) -> ExecStream {
    futures_util::stream::unfold(run, |mut run| async move {
        loop {
            if let Some(item) = run.ready.pop_front() {
                return Some((item, run));
            }
            if run.ended {
                return None;
            }
            match run.lines.next().await {
                Some(Ok(line)) => {
                    run.capture.chunk(&line);
                    run.line(&line);
                }
                Some(Err(e)) => run.fail(e),
                None => run.finish(),
            }
        }
    })
    .boxed()
}

fn not_registered(from: Format) -> ExecError {
    ExecError::local(
        501,
        FailureScope::Request,
        format!("response translation {} -> openai is not registered", from.as_str()),
    )
}

impl CommandCodeExecutor {
    pub fn with_client(client: wreq::Client) -> Self {
        Self {
            clients: Clients::new(client),
        }
    }

    pub async fn execute(
        &self,
        credential: &Credential,
        req: ExecRequest,
        cfg: &Config,
    ) -> Result<ExecResponse, ExecError> {
        match req.operation {
            Operation::CountTokens => count_tokens(&req, cfg),
            Operation::Generate => self.generate(credential, req, cfg).await,
        }
    }

    async fn generate(
        &self,
        credential: &Credential,
        req: ExecRequest,
        cfg: &Config,
    ) -> Result<ExecResponse, ExecError> {
        if req.alt.as_deref() == Some("responses/compact") {
            return Err(ExecError::local(
                501,
                FailureScope::Request,
                "/responses/compact not supported",
            ));
        }
        let (base_url, key) = creds(credential);
        if key.is_empty() {
            return Err(ExecError::local(
                401,
                FailureScope::Credential,
                "command-code executor: missing API key",
            ));
        }
        let response_pair = hr_translate::pair(req.response_format, Format::OpenAI);
        if response_pair.is_none() && req.response_format != Format::OpenAI {
            return Err(not_registered(req.response_format));
        }
        let base_model = parse_suffix(&req.model).model_name;
        let caps = req.resolved_model.as_ref().map(|r| ModelCaps::from(&r.info));
        let is_compat = req.resolved_model.as_ref().is_some_and(|r| r.is_compat());
        let original = compat::translate_body(
            &req,
            cfg,
            Format::OpenAI,
            &base_model,
            true,
            compat::original_payload(&req),
            is_compat,
        )?;
        let mut chat = compat::translate_body(&req, cfg, Format::OpenAI, &base_model, true, &req.body, is_compat)?;
        chat = compat::apply_thinking(chat, &req, Format::OpenAI, &credential.provider, caps.as_ref())?;
        chat = compat::apply_payload_rules(cfg, &req, &base_model, Format::OpenAI, &original, chat);
        if req.usage.enabled() {
            req.usage.request(Format::OpenAI, &chat);
        }
        let effort = wire_effort(&base_model, gj::get(&chat, "reasoning_effort").str().trim());
        let parsed: Value = serde_json::from_slice(&chat).map_err(|e| {
            ExecError::local(
                400,
                FailureScope::Request,
                format!("command-code executor: invalid request: {e}"),
            )
        })?;
        let mut body = build_body(&parsed, &base_model, effort.as_deref());

        let client = self.clients.for_credential(credential, cfg);
        let url = format!("{}/alpha/generate", base_url.strip_suffix('/').unwrap_or(&base_url));
        let mut effort_sent = effort.is_some();
        let (upstream, capture) = loop {
            let bytes = Bytes::from(body.to_string());
            let h = headers(&key, &req);
            let capture =
                wire::Capture::request(&req, credential, &credential.provider, &url, "POST", h.pairs(), &bytes);
            let sent = wire::send(&client, &url, h, bytes).await;
            capture.sent(&sent);
            let upstream = sent?;
            if (200..300).contains(&upstream.status) {
                break (upstream, capture);
            }
            let (status, response_headers, error_body) = wire::read_error_body(upstream).await;
            capture.chunk(&error_body);
            if effort_sent && matches!(status, 400 | 422) && effort_rejected(&error_body) {
                if let Some(params) = body["params"].as_object_mut() {
                    params.remove("reasoning_effort");
                }
                effort_sent = false;
                continue;
            }
            let mut error = compat::status_error(status, response_headers, &error_body, true);
            if cfg.routing.strategy.trim().eq_ignore_ascii_case("quota-failover") && status == 400 {
                let message = serde_json::from_slice::<Value>(&error_body).ok().and_then(|body| {
                    body.pointer("/error/message")
                        .or_else(|| body.get("message"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
                if message.is_some_and(|m| m.to_ascii_lowercase().contains("insufficient credits")) {
                    // Command Code reports exhausted billing as BAD_REQUEST, not 429.
                    // In the opt-in policy this is credential-wide quota, not a bad prompt.
                    error.status = 429;
                    error.scope = FailureScope::Credential;
                }
            }
            return Err(error);
        };

        let original_request = if req.original_body.is_empty() {
            req.body.clone()
        } else {
            req.original_body.clone()
        };
        let translated = Bytes::from(chat);
        let ctx = ResponseCtx {
            model: &req.model,
            original_request: &original_request,
            translated_request: &translated,
        };
        let headers_out = upstream.headers.clone();
        let lines = capture.stream(wire::lines(upstream, MAX_LINE));
        if req.stream {
            let translator: Box<dyn StreamTranslator> = match response_pair {
                Some(pair) => (pair.stream)(&ctx),
                None => Box::new(compat::Identity),
            };
            let stream = run(Run {
                lines,
                wire: Wire::new(&req.model, false),
                translator,
                ready: VecDeque::new(),
                capture,
                usage: req.usage.clone(),
                ended: false,
            });
            return Ok(ExecResponse {
                status: 200,
                headers: headers_out,
                body: ResponseBody::Stream(stream),
            });
        }
        let mut wire_state = Wire::new(&req.model, true);
        let mut lines = lines;
        while let Some(line) = lines.next().await {
            let line = line?;
            if let Some(part) = parse_part(&line) {
                wire_state.event(&part)?;
            }
        }
        wire_state.close();
        let raw = wire_state.completion();
        req.usage.response_body(Format::OpenAI, &raw);
        let mut out = match response_pair {
            Some(pair) => (pair.non_stream)(&ctx, &raw)
                .map_err(|_| ExecError::local(502, FailureScope::Request, hr_translate::APPLY_PATCH_UPSTREAM_ERROR))?,
            None => raw,
        };
        if req.response_format == Format::OpenAIResponse {
            out = crate::openai_compat_payload::ensure_responses_usage_details(&out);
        }
        Ok(ExecResponse {
            status: 200,
            headers: headers_out,
            body: ResponseBody::Buffered(Bytes::from(out)),
        })
    }
}

fn count_tokens(req: &ExecRequest, cfg: &Config) -> Result<ExecResponse, ExecError> {
    let base_model = parse_suffix(&req.model).model_name;
    let is_compat = req.resolved_model.as_ref().is_some_and(|r| r.is_compat());
    let body = compat::translate_body(req, cfg, Format::OpenAI, &base_model, false, &req.body, is_compat)?;
    let count = crate::openai_compat_payload::count_chat_tokens(&base_model, &body).map_err(|e| {
        ExecError::local(
            500,
            FailureScope::Request,
            format!("command-code executor: token counting failed: {e}"),
        )
    })?;
    let usage = format!(r#"{{"usage":{{"prompt_tokens":{count},"completion_tokens":0,"total_tokens":{count}}}}}"#);
    let out = hr_translate::translate_token_count(req.response_format, Format::OpenAI, count, usage.as_bytes());
    Ok(ExecResponse {
        status: 200,
        headers: Default::default(),
        body: ResponseBody::Buffered(Bytes::from(out)),
    })
}

#[cfg(test)]
#[path = "command_code_tests.rs"]
mod tests;
