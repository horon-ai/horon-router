//! Command Code usage probe (no Go counterpart; follows opencodex
//! `src/providers/quota/vendor-probes-key.ts` `fetchCommandCodeQuota`).
//!
//! Rolling windows come from `/alpha/billing/credits`. Period spend is derived:
//! subscriptions supply `currentPeriodStart`/`End`, `/alpha/usage/summary?since=`
//! supplies used, and remaining credit pools plus used become the period cap.

use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::proxy::{GoHeaders, MAX_ERROR_BODY, read_all, request};

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub struct Period {
    pub used: f64,
    pub remaining: f64,
    pub limit: f64,
    /// RFC 3339, only when the remaining pool has no purchased credits (those roll over).
    pub reset: Option<String>,
}

fn envelope(value: &Value) -> &Value {
    value.get("data").filter(|d| d.is_object()).unwrap_or(value)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64).filter(|n| n.is_finite())
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn org_id(whoami: &Value) -> Option<&str> {
    text(envelope(whoami).get("org")?, "id")
}

fn org_query(whoami: Option<&Value>) -> String {
    whoami
        .and_then(org_id)
        .map(|id| format!("?orgId={}", urlencoding(id)))
        .unwrap_or_default()
}

/// ponytail: query-string encoding for orgId/since only; switch to `url::form_urlencoded`
/// if Command Code starts sending other query keys.
fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn get(client: &wreq::Client, url: &str, key: &str) -> Option<Value> {
    let mut headers = GoHeaders::new();
    headers.set("Authorization", format!("Bearer {key}"));
    headers.set("Accept", "application/json");
    let upstream = request(client, wreq::Method::GET, url, headers, None, Some(TIMEOUT))
        .await
        .ok()?;
    if !(200..300).contains(&upstream.status) {
        return None;
    }
    let body = read_all(upstream.body, MAX_ERROR_BODY, false).await.ok()?;
    serde_json::from_slice(&body).ok()
}

fn join(base: &str, path: &str) -> String {
    format!("{}{path}", base.strip_suffix('/').unwrap_or(base))
}

/// Period spend against remaining credit pools. `None` when the billing cycle
/// start is missing (unscoped summary is lifetime spend).
pub fn period_from(credits: &Value, subscription: &Value, summary: &Value) -> Option<Period> {
    let subscription = envelope(subscription);
    let period_start = text(subscription, "currentPeriodStart")?;
    if period_start.is_empty() {
        return None;
    }
    let summary = envelope(summary);
    let used = number(summary, "totalCost").or_else(|| number(summary, "totalMonthlyCredits"))?;
    if used < 0.0 {
        return None;
    }
    let credits = credits.get("credits").unwrap_or(credits);
    let pools: Vec<f64> = ["monthlyCredits", "purchasedCredits", "freeCredits"]
        .iter()
        .filter_map(|k| number(credits, k))
        .collect();
    if pools.is_empty() {
        return None;
    }
    let remaining = pools.iter().map(|n| n.max(0.0)).sum::<f64>();
    let limit = used + remaining;
    let purchased = number(credits, "purchasedCredits").unwrap_or(0.0);
    let reset = if purchased <= 0.0 {
        text(subscription, "currentPeriodEnd").map(str::to_owned)
    } else {
        None
    };
    Some(Period {
        used,
        remaining,
        limit,
        reset,
    })
}

/// Credits JSON plus an optional `period` object the dashboard draws as the full-range bar.
pub fn merge_period(credits: Value, period: Option<Period>) -> Value {
    let mut root = match credits {
        Value::Object(map) => map,
        other => {
            let mut map = Map::new();
            map.insert("data".into(), other);
            map
        }
    };
    if let Some(period) = period {
        let mut body = match root.remove("data") {
            Some(Value::Object(map)) => map,
            Some(other) => {
                root.insert("data".into(), other);
                Map::new()
            }
            None => std::mem::take(&mut root),
        };
        body.insert(
            "period".into(),
            json!({
                "used": period.used,
                "remaining": period.remaining,
                "limit": period.limit,
                "resetAt": period.reset,
            }),
        );
        root.insert("data".into(), Value::Object(body));
    }
    Value::Object(root)
}

pub async fn fetch(client: &wreq::Client, base_url: &str, key: &str) -> Option<Value> {
    let base = base_url.strip_suffix('/').unwrap_or(base_url);
    let whoami = get(client, &join(base, "/alpha/whoami"), key).await;
    let q = org_query(whoami.as_ref());
    let credits = get(client, &format!("{base}/alpha/billing/credits{q}"), key).await?;
    let body = envelope(&credits);
    if body.get("credits").is_none() && body.get("windowLimits").is_none() {
        return None;
    }
    let period = async {
        let start = {
            let sub = get(client, &format!("{base}/alpha/billing/subscriptions{q}"), key).await?;
            let start = text(envelope(&sub), "currentPeriodStart")?.to_owned();
            (sub, start)
        };
        let (sub, start) = start;
        let sep = if q.is_empty() { "?" } else { "&" };
        let summary = get(
            client,
            &format!("{base}/alpha/usage/summary{q}{sep}since={}", urlencoding(&start)),
            key,
        )
        .await?;
        period_from(body, &sub, &summary)
    }
    .await;
    Some(merge_period(credits, period))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits() -> Value {
        json!({"monthlyCredits": 10.0, "purchasedCredits": 0.0, "freeCredits": 2.5})
    }

    #[test]
    fn period_is_used_plus_remaining_pools() {
        let p = period_from(
            &json!({"credits": credits()}),
            &json!({"currentPeriodStart": "2026-10-01T00:00:00Z", "currentPeriodEnd": "2026-11-01T00:00:00Z"}),
            &json!({"totalCost": 7.5}),
        )
        .unwrap();
        assert_eq!(p.used, 7.5);
        assert_eq!(p.remaining, 12.5);
        assert_eq!(p.limit, 20.0);
        assert_eq!(p.reset.as_deref(), Some("2026-11-01T00:00:00Z"));
    }

    #[test]
    fn purchased_credits_hide_period_end() {
        let mut credits = credits();
        credits["purchasedCredits"] = json!(1.0);
        let p = period_from(
            &credits,
            &json!({"data": {"currentPeriodStart": "2026-10-01T00:00:00Z", "currentPeriodEnd": "2026-11-01T00:00:00Z"}}),
            &json!({"data": {"totalMonthlyCredits": 3.0}}),
        )
        .unwrap();
        assert_eq!(p.limit, 3.0 + 10.0 + 1.0 + 2.5);
        assert_eq!(p.reset, None);
    }

    #[test]
    fn missing_period_start_is_omitted() {
        assert!(period_from(&json!({"credits": credits()}), &json!({}), &json!({"totalCost": 1.0})).is_none());
    }

    #[test]
    fn merge_puts_period_beside_windows() {
        let merged = merge_period(
            json!({"data": {"windowLimits": {"fiveHour": {"cap": 1, "used": 0}}, "credits": credits()}}),
            Some(Period {
                used: 1.0,
                remaining: 2.0,
                limit: 3.0,
                reset: Some("later".into()),
            }),
        );
        assert_eq!(merged["data"]["period"]["limit"], 3.0);
        assert_eq!(merged["data"]["windowLimits"]["fiveHour"]["cap"], 1);
    }
}
