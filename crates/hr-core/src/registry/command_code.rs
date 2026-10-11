//! Command Code model catalog.
//!
//! Command Code publishes no catalog this build can embed, so the ids and reasoning
//! ladders are the ones its model profiles list (captured 2026-09-23 by opencodex,
//! `src/providers/command-code-efforts.ts`). The same table gives the executor the
//! efforts each model accepts. Unlisted models still route; they just get no effort.
// ponytail: static list; a live `/provider/v1/models` refresh would add new ids without a release.

use std::sync::LazyLock;

use serde_json::{Map, Value, json};

use super::ModelInfo;

const FIVE: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const FOUR: &[&str] = &["low", "medium", "high", "xhigh"];
const LOW_HIGH_MAX: &[&str] = &["low", "high", "max"];
const HIGH_MAX: &[&str] = &["high", "max"];
const THREE: &[&str] = &["low", "medium", "high"];

const MODELS: &[(&str, &[&str])] = &[
    ("claude-fable-5-1", FIVE),
    ("claude-opus-5-5", FIVE),
    ("deepseek/deepseek-v4-flash", FIVE),
    ("deepseek/deepseek-v4-flash-vision-exp", FIVE),
    ("deepseek/deepseek-v4-flash-fast", LOW_HIGH_MAX),
    ("deepseek/deepseek-v4.1-flash", FIVE),
    ("gpt-5.6-luna", FIVE),
    ("google/gemini-3.7-flash", FIVE),
    ("google/gemini-3.8-flash", THREE),
    ("zai-org/GLM-5", HIGH_MAX),
    ("zai-org/GLM-5.1", HIGH_MAX),
    ("zai-org/GLM-5.2", HIGH_MAX),
    ("zai-org/GLM-5.2-Fast", HIGH_MAX),
    ("zai-org/GLM-5.3", FIVE),
    ("z-ai/glm-5.3-flash", FIVE),
    ("z-ai/glm-5.3-flashx", LOW_HIGH_MAX),
    ("meta/muse-spark-1.3", FIVE),
    ("meta/muse-spark-1.3-contributor", FIVE),
    ("meta/muse-spark-1.2", FIVE),
    ("meta/muse-spark-1.2-contributor", FIVE),
    ("meta/muse-spark-1.1", FIVE),
    ("Qwen/Qwen3.8-Flash", FIVE),
    ("Qwen/Qwen3.8-Omni-Flash", &["low", "medium", "xhigh"]),
    ("Qwen/Qwen3.8-Max-0902", &["low", "medium", "xhigh"]),
    ("Qwen/Qwen3.8-Max", FIVE),
    ("Qwen/Qwen3.8-27B", FIVE),
    ("Qwen/Qwen3.7-32B", FOUR),
    ("Qwen/Qwen3.7-72B", FOUR),
    ("Qwen/Qwen3.6-35B-A22B", FOUR),
    ("stepfun/Step-5-Preview", THREE),
    ("stepfun/Step-3.7-Flash", FIVE),
    ("tencent/hy4-preview", FIVE),
    ("tencent/hy3-paid", FIVE),
    ("xai/grok-4.7", FOUR),
    ("xai/grok-4.6", FIVE),
    ("xai/grok-4.5", FIVE),
    ("moonshotai/Kimi-K3", FIVE),
    ("moonshotai/Kimi-K2.7-Code", FOUR),
    ("moonshotai/Kimi-K2.7-Code-Highspeed", &["low", "high", "xhigh", "max"]),
    ("MiniMaxAI/MiniMax-M3", FIVE),
    ("xiaomi/mimo-v2.5", FIVE),
    ("xiaomi/mimo-v2.5-pro", THREE),
    ("nvidia/nemotron-3-ultra-550b-a55b", FIVE),
    ("meituan/LongCat-2.0:free", FIVE),
    ("inclusionai/ling-3.0-flash-sante:free", FIVE),
    ("thinkingmachines/inkling-small", FIVE),
    ("poolside/laguna-s-2.1-free", &["medium"]),
];

pub fn efforts(model: &str) -> Option<&'static [&'static str]> {
    MODELS
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(model.trim()))
        .map(|(_, levels)| *levels)
}

pub fn models() -> &'static [ModelInfo] {
    static LIST: LazyLock<Vec<ModelInfo>> = LazyLock::new(|| {
        MODELS
            .iter()
            .map(|(id, levels)| {
                let mut raw = Map::new();
                raw.insert("id".into(), (*id).into());
                raw.insert("object".into(), "model".into());
                raw.insert(
                    "owned_by".into(),
                    id.split_once('/').map_or("command-code", |(o, _)| o).into(),
                );
                raw.insert("type".into(), "command-code".into());
                raw.insert("display_name".into(), (*id).into());
                raw.insert("thinking".into(), json!({ "levels": levels }));
                ModelInfo::from_raw(raw).expect("command-code catalog renders a valid ModelInfo")
            })
            .collect()
    });
    &LIST
}

pub fn definitions() -> Vec<Value> {
    models().iter().map(|m| Value::Object(m.raw.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_unique_and_lookups_fold_case() {
        let mut ids: Vec<_> = MODELS.iter().map(|(id, _)| id.to_ascii_lowercase()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len());
        assert_eq!(models().len(), MODELS.len());
        assert_eq!(efforts("DeepSeek/DeepSeek-V4-Flash"), Some(FIVE));
        assert_eq!(efforts("unknown/model"), None);
    }
}
