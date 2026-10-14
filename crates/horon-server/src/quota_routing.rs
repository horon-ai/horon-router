//! Opt-in same-model quota failover. Unknown quota never vetoes a credential.
//! Probes use the credential's inference proxy and are single-flighted per snapshot.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use futures_util::{StreamExt, stream};
use horon_core::config::Config;
use horon_core::credential::Credential;
use horon_exec::proxy::{GoClients, GoHeaders, MAX_ERROR_BODY, Proxy, read_all, request};
use serde_json::Value;

use crate::Runtime;

const TTL: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

struct Cache {
    clients: GoClients,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            clients: GoClients::new(Default::default()),
            entries: Mutex::default(),
        }
    }
}

struct Entry {
    credential: Arc<Credential>,
    observation: Arc<tokio::sync::Mutex<Option<Observation>>>,
}

struct Observation {
    at: Instant,
    raw: Option<Value>,
}

#[derive(Clone, Copy)]
enum Kind {
    CommandCode,
    Devin,
    OpenCodeGo,
}

fn text<'a>(c: &'a Credential, key: &str) -> &'a str {
    c.attributes
        .get(key)
        .map(String::as_str)
        .or_else(|| c.str(key))
        .unwrap_or_default()
        .trim()
}

fn kind(c: &Credential) -> Option<Kind> {
    match c.provider.as_str() {
        "command-code" => Some(Kind::CommandCode),
        "devin" => Some(Kind::Devin),
        // OpenCode Go is configured through the existing OpenAI-compatible executor.
        // Match the destination, not an arbitrary user-chosen provider name.
        _ if text(c, "base_url").trim_end_matches('/') == "https://opencode.ai/zen/go/v1"
            || text(c, "compat_name").eq_ignore_ascii_case("opencode-go") =>
        {
            Some(Kind::OpenCodeGo)
        }
        _ => None,
    }
}

pub(crate) async fn exhausted(rt: &Runtime, cfg: &Config, candidates: Vec<Arc<Credential>>) -> HashSet<String> {
    let cache = cfg.derived(|_| Cache::default());
    let credentials = rt.store().snapshot();
    {
        let mut entries = cache.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|id, _| credentials.iter().any(|c| &c.id == id));
    }
    stream::iter(candidates)
        .map(|c| {
            let cache = cache.clone();
            async move {
                let provider = c.provider.trim().to_lowercase();
                let native = match provider.as_str() {
                    "claude" => rt.executors.claude.quota().snapshot(&c.id),
                    "codex" => rt.executors.codex.quota().snapshot(&c.id),
                    _ => None,
                };
                if let Some(s) = native {
                    let now = SystemTime::now();
                    return crate::scheduler::Windows::observed(&provider, &s.signals, s.observed_at, now)
                        .exhausted
                        .then(|| c.id.clone());
                }
                let k = kind(&c)?;
                let slot = {
                    let mut entries = cache.entries.lock().unwrap_or_else(PoisonError::into_inner);
                    let entry = entries.entry(c.id.clone()).or_insert_with(|| Entry {
                        credential: c.clone(),
                        observation: Default::default(),
                    });
                    // Token, base URL or metadata changes must not reuse old quota evidence.
                    if !Arc::ptr_eq(&entry.credential, &c) {
                        *entry = Entry {
                            credential: c.clone(),
                            observation: Default::default(),
                        };
                    }
                    entry.observation.clone()
                };
                let mut observation = slot.lock().await;
                if observation.as_ref().is_none_or(|o| o.at.elapsed() >= TTL) {
                    let guarded = rt.for_executor(&c);
                    let raw = tokio::time::timeout(PROBE_TIMEOUT, probe(rt, &cache, &guarded, cfg, k))
                        .await
                        .ok()
                        .flatten();
                    *observation = Some(Observation {
                        at: Instant::now(),
                        raw,
                    });
                }
                let raw = observation.as_ref()?.raw.as_ref()?;
                is_exhausted(k, raw, SystemTime::now()).then(|| c.id.clone())
            }
        })
        .buffer_unordered(4)
        .filter_map(async |id| id)
        .collect()
        .await
}

async fn probe(rt: &Runtime, cache: &Cache, c: &Credential, cfg: &Config, k: Kind) -> Option<Value> {
    match k {
        Kind::Devin => {
            rt.executors.devices.devin.prepare(c, cfg).await.ok()?;
            let s = rt.executors.devices.devin.quota(&c.id)?;
            serde_json::to_value(s.signals).ok()
        }
        Kind::CommandCode => {
            let base = text(c, "base_url");
            let base = if base.is_empty() {
                horon_exec::command_code::DEFAULT_BASE_URL
            } else {
                base
            };
            let key = ["api_key", "access_token"]
                .into_iter()
                .map(|key| text(c, key))
                .find(|v| !v.is_empty())?;
            let client = cache.clients.get(&Proxy::effective(c, cfg));
            horon_exec::command_code_quota::fetch(&client, base, key).await
        }
        Kind::OpenCodeGo => {
            let key = text(c, "api_key");
            if key.is_empty() {
                return None;
            }
            let mut headers = GoHeaders::new();
            headers.set("Authorization", format!("Bearer {key}"));
            headers.set("Accept", "application/json");
            let client = cache.clients.get(&Proxy::effective(c, cfg));
            let url = format!("{}/usage", text(c, "base_url").trim_end_matches('/'));
            let response = request(&client, wreq::Method::GET, &url, headers, None, Some(PROBE_TIMEOUT))
                .await
                .ok()?;
            if !(200..300).contains(&response.status) {
                return None;
            }
            let body = read_all(response.body, MAX_ERROR_BODY, false).await.ok()?;
            serde_json::from_slice(&body).ok()
        }
    }
}

fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn reset(v: &Value) -> Option<SystemTime> {
    if let Some(s) = v.as_str() {
        if let Ok(at) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(at.into());
        }
        return s.parse::<f64>().ok().and_then(epoch);
    }
    number(v).and_then(epoch)
}

fn epoch(n: f64) -> Option<SystemTime> {
    let seconds = if n >= 1e12 { n / 1000.0 } else { n };
    SystemTime::UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(seconds).ok()?)
}

fn still_exhausted(exhausted: bool, reset_at: Option<SystemTime>, now: SystemTime) -> bool {
    exhausted && reset_at.is_none_or(|at| at > now)
}

fn is_exhausted(kind: Kind, raw: &Value, now: SystemTime) -> bool {
    match kind {
        Kind::CommandCode => {
            let body = raw.get("data").filter(|v| v.is_object()).unwrap_or(raw);
            let windows = &body["windowLimits"];
            let window_exhausted = ["fiveHour", "weekly"].iter().any(|name| {
                let w = &windows[name];
                let used = number(&w["used"]);
                let cap = number(&w["cap"]);
                let exhausted = used.zip(cap).is_some_and(|(used, cap)| cap > 0.0 && used >= cap);
                let at = ["resetAt", "resetsAt", "reset_at"]
                    .iter()
                    .find_map(|key| reset(&w[key]));
                still_exhausted(exhausted, at, now)
            });
            // Only a complete credit-pool observation proves that no credits remain.
            let pools: Option<Vec<_>> = ["monthlyCredits", "purchasedCredits", "freeCredits"]
                .iter()
                .map(|key| number(&body["credits"][key]))
                .collect();
            window_exhausted || pools.is_some_and(|p| p.iter().all(|n| *n <= 0.0))
        }
        Kind::OpenCodeGo => ["rolling", "weekly", "monthly"].iter().any(|name| {
            let w = &raw["usage"][name];
            still_exhausted(
                number(&w["percent"]).is_some_and(|n| n >= 100.0),
                reset(&w["resetsAt"]),
                now,
            )
        }),
        Kind::Devin => ["daily", "weekly"].iter().any(|name| {
            let remaining = raw[format!("{name}_quota_remaining_percent")]
                .as_str()
                .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                .filter(|n| n.is_finite());
            let at = reset(&raw[format!("{name}_quota_reset_at")]);
            // Proto3 omits zero scalar fields. A default zero without a dated quota
            // window does not prove that this account has exhausted its allowance.
            still_exhausted(remaining.is_some_and(|n| n <= 0.0) && at.is_some(), at, now)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    #[test]
    fn command_code_requires_complete_credit_evidence() {
        assert!(!is_exhausted(
            Kind::CommandCode,
            &json!({"credits":{"monthlyCredits":0}}),
            now()
        ));
        assert!(!is_exhausted(
            Kind::CommandCode,
            &json!({"data":{"credits":{
                "monthlyCredits":0,"purchasedCredits":2,"freeCredits":0
            }}}),
            now()
        ));
        assert!(is_exhausted(
            Kind::CommandCode,
            &json!({"data":{"credits":{
                "monthlyCredits":0,"purchasedCredits":0,"freeCredits":0
            }}}),
            now()
        ));
    }

    #[test]
    fn command_code_window_blocks_even_with_credits() {
        let raw = json!({"windowLimits":{"fiveHour":{"used":10,"cap":10,"resetAt":1_800_000_001}},
            "credits":{"monthlyCredits":20,"purchasedCredits":0,"freeCredits":0}});
        assert!(is_exhausted(Kind::CommandCode, &raw, now()));
        assert!(!is_exhausted(Kind::CommandCode, &raw, now() + Duration::from_secs(1)));
    }

    #[test]
    fn opencode_go_checks_each_window_and_reset_units() {
        let raw = json!({"usage":{"rolling":{"percent":10},"weekly":{"percent":100,"resetsAt":1_800_000_001_000u64}}});
        assert!(is_exhausted(Kind::OpenCodeGo, &raw, now()));
        assert!(!is_exhausted(Kind::OpenCodeGo, &raw, now() + Duration::from_secs(1)));
        assert!(!is_exhausted(Kind::OpenCodeGo, &json!({"usage":{"rolling":{}}}), now()));
    }

    #[test]
    fn devin_remaining_percent_is_not_used_percent() {
        assert!(!is_exhausted(
            Kind::Devin,
            &json!({"daily_quota_remaining_percent":"100%"}),
            now()
        ));
        assert!(is_exhausted(
            Kind::Devin,
            &json!({"daily_quota_remaining_percent":"0%","daily_quota_reset_at":1_800_000_001}),
            now()
        ));
        assert!(!is_exhausted(
            Kind::Devin,
            &json!({"daily_quota_remaining_percent":"0%"}),
            now()
        ));
        assert!(!is_exhausted(
            Kind::Devin,
            &json!({"weekly_quota_remaining_percent":"unknown"}),
            now()
        ));
        assert!(!is_exhausted(
            Kind::Devin,
            &json!({
                "weekly_quota_remaining_percent":"0%","weekly_quota_reset_at":"2026-01-01T00:00:00Z"
            }),
            now()
        ));
    }
}
