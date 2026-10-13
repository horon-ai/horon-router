//! Model lists for the OAuth gateways that speak OpenAI Chat: Nous Portal and GitHub Copilot.
//!
//! Neither publishes a catalog this build can embed, so these are the fallback seeds from
//! opencodex's provider registry. Both gateways list models per account; a seed that the
//! account cannot use fails at the upstream with its own error.
// ponytail: static seeds; a live `GET /models` refresh would track each account's roster.

use std::sync::LazyLock;

use serde_json::Map;

use super::ModelInfo;

const NOUS: &[&str] = &[
    "tencent/hy3:free",
    "poolside/laguna-s-2.1:free",
    "stepfun/step-3.7-flash:free",
    "poolside/laguna-xs-2.1:free",
];

const COPILOT: &[&str] = &[
    "gpt-4o",
    "gpt-4.1",
    "gpt-4.1-mini",
    "claude-sonnet-4",
    "gemini-2.5-pro",
    "gpt-5-mini",
];

fn build(kind: &str, owned_by: &str, ids: &[&str]) -> Vec<ModelInfo> {
    ids.iter()
        .map(|id| {
            let mut raw = Map::new();
            raw.insert("id".into(), (*id).into());
            raw.insert("object".into(), "model".into());
            raw.insert("owned_by".into(), owned_by.into());
            raw.insert("type".into(), kind.into());
            raw.insert("display_name".into(), (*id).into());
            ModelInfo::from_raw(raw).expect("gateway catalog renders a valid ModelInfo")
        })
        .collect()
}

pub fn nous_models() -> &'static [ModelInfo] {
    static LIST: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| build("nous", "nous", NOUS));
    &LIST
}

pub fn copilot_models() -> &'static [ModelInfo] {
    static LIST: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| build("github-copilot", "github-copilot", COPILOT));
    &LIST
}

pub fn definitions(provider: &str) -> Vec<serde_json::Value> {
    let list = if provider == "nous" {
        nous_models()
    } else {
        copilot_models()
    };
    list.iter().map(|m| serde_json::Value::Object(m.raw.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_are_unique_and_chat_only() {
        for (list, ids) in [(nous_models(), NOUS), (copilot_models(), COPILOT)] {
            assert_eq!(list.len(), ids.len());
            let mut seen: Vec<_> = list.iter().map(|m| m.id.clone()).collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), ids.len());
        }
        assert!(
            copilot_models()
                .iter()
                .all(|m| !m.id.starts_with("gpt-5.") && !m.id.starts_with("gpt-6"))
        );
    }
}
