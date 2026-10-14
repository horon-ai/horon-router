//! Request classification used by cloaking, betas and caching
//! (helps/claude_diagnostics.go): quota probes, title helpers, subagents, 1h cache
//! pools, prompt turns and billing continuity tags.

use http::HeaderMap;

use super::detect::{header, header_values};
use super::session::{valid_prompt_id, valid_request_id};
use crate::rawjson;

fn probe(body: &str) -> bool {
    let max_tokens = rawjson::get(body, "max_tokens");
    if !max_tokens.exists() || max_tokens.i64() != 1 {
        return false;
    }
    let tools = rawjson::get(body, "tools");
    if tools.exists() && !tools.array().is_empty() {
        return false;
    }
    let messages = rawjson::get(body, "messages");
    if messages.kind() != gjson::Kind::Array || messages.array().is_empty() {
        return true;
    }
    let messages = messages.array();
    if messages.len() != 1 || messages[0].get("role").str() != "user" {
        return false;
    }
    let content = messages[0].get("content");
    let probe_text = |t: &str| matches!(t, "quota" | "test" | "." | "probe");
    match content.kind() {
        gjson::Kind::String => probe_text(content.str().trim()),
        gjson::Kind::Array => {
            let (mut count, mut matched) = (0, false);
            for part in content.array() {
                let t = part.get("text");
                let t = t.str().trim();
                if t.contains("<system-reminder>") {
                    continue;
                }
                count += 1;
                if probe_text(t) || (t == "Hi" && part.get("cache_control").exists()) {
                    matched = true;
                }
            }
            count == 1 && matched
        }
        _ => false,
    }
}

fn any_text(value: &gjson::Value, matches: impl Fn(&str) -> bool) -> bool {
    if value.kind() == gjson::Kind::Array {
        value.array().iter().any(|p| matches(p.get("text").str()))
    } else {
        matches(value.str())
    }
}

fn title_helper(body: &str) -> bool {
    let system_title = |t: &str| {
        t.contains("naming a coding session")
            || t.contains("Return a short title")
            || t.contains("Write the title in the predominant language")
    };
    let props = rawjson::get(body, "output_config.format.schema.properties");
    if props.exists() {
        let mut count = 0;
        props.each(|_, _| {
            count += 1;
            true
        });
        if !(props.get("title").exists() && count == 1) {
            return false;
        }
        let prompt = |t: &str| system_title(t) || t.contains("<session>");
        let messages = rawjson::get(body, "messages");
        return any_text(&rawjson::get(body, "system"), prompt)
            || messages.array().iter().any(|m| any_text(&m.get("content"), prompt));
    }
    if any_text(&rawjson::get(body, "system"), system_title) {
        return true;
    }
    rawjson::get(body, "messages")
        .array()
        .iter()
        .filter(|m| m.get("role").str() == "system")
        .any(|m| any_text(&m.get("content"), system_title))
}

/// `IsClaudeProbeOrHelperRequest`.
pub(crate) fn probe_or_helper(body: &str) -> bool {
    probe(body) || title_helper(body)
}

/// `IsClaudeSubagentRequest`.
pub(crate) fn subagent(headers: &HeaderMap, body: &str) -> bool {
    if !header(headers, "x-claude-code-agent-id").is_empty()
        || !header(headers, "x-claude-code-parent-agent-id").is_empty()
    {
        return true;
    }
    if rawjson::get(body, "metadata.user_id.parent_session_id").exists()
        || rawjson::get(body, "metadata.user_id")
            .str()
            .contains("\"parent_session_id\"")
    {
        return true;
    }
    let system = rawjson::get(body, "system");
    match system.kind() {
        gjson::Kind::Array => system
            .array()
            .first()
            .is_some_and(|b| b.get("text").str().contains("cc_is_subagent=true")),
        gjson::Kind::String => system.str().contains("cc_is_subagent=true"),
        _ => false,
    }
}

/// Whether the request body is valid JSON. Go re-checks this at every cache-control
/// helper; the body only ever changes through JSON edits, which keep a valid body valid,
/// so a request computes it once and threads the flag down.
pub(crate) fn body_valid(body: &str) -> bool {
    #[cfg(test)]
    valid_calls::bump();
    gjson::valid(body)
}

/// `body_valid` calls on this thread (tests only): a request must call it exactly once.
#[cfg(test)]
pub(crate) mod valid_calls {
    use std::cell::Cell;
    thread_local! {
        static COUNT: Cell<u64> = const { Cell::new(0) };
    }
    pub(crate) fn bump() {
        COUNT.with(|c| c.set(c.get() + 1));
    }
    pub(crate) fn reset() {
        COUNT.with(|c| c.set(0));
    }
    pub(crate) fn count() -> u64 {
        COUNT.with(|c| c.get())
    }
}

/// [`Blocks::build`] calls on this thread (tests only): the cache-control region must
/// build the block index once per request and update it in place after each edit.
#[cfg(test)]
pub(crate) mod block_builds {
    use std::cell::Cell;
    thread_local! {
        static COUNT: Cell<u64> = const { Cell::new(0) };
    }
    pub(crate) fn bump() {
        COUNT.with(|c| c.set(c.get() + 1));
    }
    pub(crate) fn reset() {
        COUNT.with(|c| c.set(0));
    }
    pub(crate) fn count() -> u64 {
        COUNT.with(|c| c.get())
    }
}

/// The `cache_control` a block carries, parsed once when the index is built. Every
/// consumer reads these fields instead of re-parsing the block.
#[derive(Debug, Clone)]
pub(crate) struct CacheControl {
    /// `cache_control` is a JSON object.
    pub is_object: bool,
    /// The `ttl` member exists, whatever its type (Go's `get("ttl").exists()`).
    pub ttl_present: bool,
    /// The `ttl` value when it is a string.
    pub ttl_str: Option<String>,
    /// The `type` value when it is a string (`upgrade_ttl` re-emits it).
    pub cc_type: Option<String>,
    /// The raw JSON of `scope` when it exists (`upgrade_ttl` copies it verbatim).
    pub scope: Option<String>,
}

impl CacheControl {
    fn parse(cc: &gjson::Value<'_>) -> Self {
        let ttl = cc.get("ttl");
        let ty = cc.get("type");
        let scope = cc.get("scope");
        Self {
            is_object: cc.kind() == gjson::Kind::Object,
            ttl_present: ttl.exists(),
            ttl_str: (ttl.kind() == gjson::Kind::String).then(|| ttl.str().to_owned()),
            cc_type: (ty.kind() == gjson::Kind::String).then(|| ty.str().to_owned()),
            scope: scope.exists().then(|| scope.json().to_owned()),
        }
    }

    fn from_raw(raw: &str) -> Self {
        Self::parse(&gjson::parse(raw))
    }
}

/// One cache-control-capable block: its path and its `cache_control`.
#[derive(Debug)]
pub(crate) struct Block {
    pub path: String,
    pub cc: Option<CacheControl>,
}

/// Every cache-control-capable block in tools → system → messages order, built once per
/// request. The cloak helpers read it instead of re-walking the body; each edit updates
/// only the entries it changed (Go re-collects the whole list per helper).
#[derive(Debug, Default)]
pub(crate) struct Blocks {
    items: Vec<Block>,
}

impl Blocks {
    pub(crate) fn build(body: &str) -> Self {
        #[cfg(test)]
        block_builds::bump();
        let mut items = Vec::new();
        for section in ["tools", "system"] {
            let v = rawjson::get(body, section);
            if v.kind() == gjson::Kind::Array {
                for (i, item) in v.array().iter().enumerate() {
                    items.push(Block {
                        path: format!("{section}.{i}"),
                        cc: cc_of(item),
                    });
                }
            }
        }
        let messages = rawjson::get(body, "messages");
        if messages.kind() == gjson::Kind::Array {
            for (m, message) in messages.array().iter().enumerate() {
                let content = message.get("content");
                if content.kind() == gjson::Kind::Array {
                    for (i, item) in content.array().iter().enumerate() {
                        items.push(Block {
                            path: format!("messages.{m}.content.{i}"),
                            cc: cc_of(item),
                        });
                    }
                }
            }
        }
        Self { items }
    }

    #[cfg(test)]
    pub(crate) fn items(&self) -> &[Block] {
        &self.items
    }

    pub(crate) fn items_mut(&mut self) -> &mut [Block] {
        &mut self.items
    }

    pub(crate) fn count_marked(&self) -> usize {
        self.items.iter().filter(|b| b.cc.is_some()).count()
    }

    /// `ClaudePayloadHas1hTTL`'s per-block test, answered from the index.
    pub(crate) fn has_1h(&self) -> bool {
        self.items.iter().any(|b| {
            b.cc
                .as_ref()
                .is_some_and(|c| c.is_object && c.ttl_str.as_deref() == Some("1h"))
        })
    }

    /// Record that `path` now carries the raw `cache_control` JSON `raw`.
    pub(crate) fn set_cc(&mut self, path: &str, raw: &str) {
        if let Some(b) = self.items.iter_mut().find(|b| b.path == path) {
            b.cc = Some(CacheControl::from_raw(raw));
        }
    }

    /// Record that `path` lost its `cache_control`.
    pub(crate) fn clear_cc(&mut self, path: &str) {
        if let Some(b) = self.items.iter_mut().find(|b| b.path == path) {
            b.cc = None;
        }
    }

    /// Insert a block a string→array rewrite just created, keeping the
    /// tools → system → messages order.
    pub(crate) fn insert(&mut self, path: String, raw_cc: &str) {
        let key = path_key(&path);
        let pos = self
            .items
            .iter()
            .position(|b| path_key(&b.path) > key)
            .unwrap_or(self.items.len());
        self.items.insert(
            pos,
            Block {
                path,
                cc: Some(CacheControl::from_raw(raw_cc)),
            },
        );
    }
}

fn cc_of(item: &gjson::Value<'_>) -> Option<CacheControl> {
    let cc = item.get("cache_control");
    cc.exists().then(|| CacheControl::parse(&cc))
}

/// Sort key preserving tools → system → messages and index order.
fn path_key(path: &str) -> (u8, usize, usize) {
    if let Some(rest) = path.strip_prefix("tools.") {
        (0, rest.parse().unwrap_or(0), 0)
    } else if let Some(rest) = path.strip_prefix("system.") {
        (1, rest.parse().unwrap_or(0), 0)
    } else if let Some(rest) = path.strip_prefix("messages.") {
        let mut parts = rest.split('.');
        let m = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let _ = parts.next();
        let i = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        (2, m, i)
    } else {
        (3, 0, 0)
    }
}

/// `ClaudePayloadHas1hTTL`.
pub(crate) fn has_1h_ttl(body: &str) -> bool {
    !body.is_empty() && gjson::valid(body) && Blocks::build(body).has_1h()
}

/// `extended-cache-ttl-2025-04-11` on the request's `anthropic-beta` header.
pub(crate) fn requests_extended_cache_ttl(headers: &HeaderMap) -> bool {
    header_values(headers, "anthropic-beta")
        .join(",")
        .contains("extended-cache-ttl-2025-04-11")
}

/// `ClaudeSubagentRequests1h`.
pub(crate) fn subagent_requests_1h(headers: &HeaderMap, body: &str) -> bool {
    has_1h_ttl(body) || requests_extended_cache_ttl(headers)
}

/// `IsClaudeNewPromptTurn`.
pub(crate) fn new_prompt_turn(body: &str) -> bool {
    if probe_or_helper(body) {
        return false;
    }
    let messages = rawjson::get(body, "messages");
    if messages.kind() != gjson::Kind::Array {
        return true;
    }
    let messages = messages.array();
    let Some(last) = messages.last() else { return true };
    if last.get("role").str() != "user" {
        return false;
    }
    let content = last.get("content");
    !(content.kind() == gjson::Kind::Array && content.array().iter().any(|p| p.get("type").str() == "tool_result"))
}

const BILLING: &str = "x-anthropic-billing-header:";

fn billing_text(body: &str) -> Option<String> {
    let system = rawjson::get(body, "system");
    let text = match system.kind() {
        gjson::Kind::Array => system.array().first()?.get("text").str().to_owned(),
        gjson::Kind::String => system.str().to_owned(),
        _ => return None,
    };
    text.starts_with(BILLING).then_some(text)
}

/// `ExtractClaudeBillingTags`: (cc_prev_req, cc_prompt_id) when valid.
pub(crate) fn billing_tags(body: &str) -> (String, String) {
    let Some(text) = billing_text(body) else {
        return Default::default();
    };
    let tag = |name: &str| {
        text.find(name).map(|i| {
            let v = &text[i + name.len()..];
            v[..v.find(';').unwrap_or(v.len())].to_owned()
        })
    };
    let prev = tag("cc_prev_req=").filter(|v| valid_request_id(v)).unwrap_or_default();
    let prompt = tag("cc_prompt_id=")
        .filter(|v| valid_prompt_id(v))
        .map(|v| v.to_lowercase())
        .unwrap_or_default();
    (prev, prompt)
}

/// Removes `\s*cc_prev_req=...;` and `\s*cc_prompt_id=...;` occurrences.
fn remove_tags(text: &str) -> String {
    let mut out = text.to_owned();
    for name in ["cc_prev_req=", "cc_prompt_id="] {
        let mut result = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = rest.find(name) {
            let value = &rest[i + name.len()..];
            match value.find(';') {
                Some(end) if end > 0 => {
                    // RE2 `\s` is [\t\n\f\r ], without \v.
                    let before = rest[..i].trim_end_matches([' ', '\t', '\n', '\r', '\x0c']);
                    result.push_str(before);
                    rest = &value[end + 1..];
                }
                _ => {
                    result.push_str(&rest[..i + name.len()]);
                    rest = value;
                }
            }
        }
        result.push_str(rest);
        out = result;
    }
    out
}

/// `InjectClaudeBillingTags`: replaces the billing header's continuity tags.
pub(crate) fn inject_billing_tags(body: &str, prev: &str, prompt: &str) -> String {
    let system = rawjson::get(body, "system");
    if system.kind() != gjson::Kind::Array {
        return body.to_owned();
    }
    let Some(text) = system.array().first().map(|b| b.get("text").str().to_owned()) else {
        return body.to_owned();
    };
    if !text.starts_with(BILLING) {
        return body.to_owned();
    }
    let mut cleaned = remove_tags(&text).trim().to_owned();
    if !cleaned.ends_with(';') {
        cleaned.push(';');
    }
    if !prev.is_empty() {
        cleaned.push_str(&format!(" cc_prev_req={prev};"));
    }
    if !prompt.is_empty() {
        cleaned.push_str(&format!(" cc_prompt_id={prompt};"));
    }
    rawjson::set_str(body, "system.0.text", &cleaned)
}

/// `StripClaudeBillingTags` (probe and helper requests carry none).
pub(crate) fn strip_billing_tags(body: &str) -> String {
    let system = rawjson::get(body, "system");
    if system.kind() != gjson::Kind::Array {
        return body.to_owned();
    }
    let Some(text) = system.array().first().map(|b| b.get("text").str().to_owned()) else {
        return body.to_owned();
    };
    if !text.starts_with(BILLING) {
        return body.to_owned();
    }
    let cleaned = remove_tags(&text);
    if cleaned == text {
        return body.to_owned();
    }
    rawjson::set_str(body, "system.0.text", &cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_helper_and_tags() {
        assert!(probe_or_helper(
            r#"{"max_tokens":1,"messages":[{"role":"user","content":"quota"}]}"#
        ));
        assert!(!probe_or_helper(
            r#"{"max_tokens":2,"messages":[{"role":"user","content":"quota"}]}"#
        ));
        assert!(probe_or_helper(
            r#"{"max_tokens":9,"system":"Return a short title","messages":[]}"#
        ));
        let body = r#"{"system":[{"type":"text","text":"x-anthropic-billing-header: cc_version=2.1.280.abc; cc_entrypoint=cli; cch=00000; cc_prev_req=req_ab-1; cc_prompt_id=83EC619F-AB81-4D70-9C67-44C3865291B9;"}]}"#;
        assert_eq!(
            billing_tags(body),
            ("req_ab-1".into(), "83ec619f-ab81-4d70-9c67-44c3865291b9".into())
        );
        assert_eq!(
            rawjson::get(&strip_billing_tags(body), "system.0.text").str(),
            "x-anthropic-billing-header: cc_version=2.1.280.abc; cc_entrypoint=cli; cch=00000;"
        );
    }
}
