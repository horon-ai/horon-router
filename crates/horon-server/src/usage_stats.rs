//! Bounded cumulative token/cache usage per dimension (model, provider, auth index).
//!
//! Every published usage record folds into three independent maps; each map keeps at
//! most [`MAX_ENTRIES`] rows and evicts the row whose `last_seen` is oldest when it is
//! full. The snapshot view mirrors the management usage surface's `group_by` shape and
//! never carries request identifiers, keys or client metadata.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::usage_record::{Breakdown, Detail, Quality, Record};

/// Maximum rows kept per dimension; the oldest `last_seen` row is evicted past this.
const MAX_ENTRIES: usize = 4096;

/// Maximum distinct providers remembered per row.
const MAX_PROVIDERS: usize = 64;

/// One dimension key's cumulative counters.
#[derive(Debug, Default)]
struct Row {
    requests: u64,
    failed: u64,
    input: i64,
    output: i64,
    reasoning: i64,
    cached: i64,
    cache_read: i64,
    cache_creation: i64,
    total: i64,
    unclassified: i64,
    quality_complete: u64,
    quality_unclassified: u64,
    quality_inconsistent: u64,
    first_seen: String,
    last_seen: String,
    providers: BTreeSet<String>,
}

/// Grouping dimension of a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dim {
    Model,
    Provider,
    AuthIndex,
}

impl Dim {
    /// The `group_by` label of this dimension.
    fn as_str(self) -> &'static str {
        match self {
            Dim::Model => "model",
            Dim::Provider => "provider",
            Dim::AuthIndex => "auth_index",
        }
    }
}

/// Parse a `group_by` query value; case-sensitive, surrounding whitespace ignored.
pub fn parse(s: &str) -> Option<Dim> {
    match s.trim() {
        "model" => Some(Dim::Model),
        "provider" => Some(Dim::Provider),
        "auth_index" => Some(Dim::AuthIndex),
        _ => None,
    }
}

/// Bounded per-dimension usage aggregator. One record updates the model, provider and
/// auth-index maps; a dimension whose key is empty is skipped for that record.
#[derive(Default)]
pub struct UsageStats {
    model: Mutex<HashMap<String, Row>>,
    provider: Mutex<HashMap<String, Row>>,
    auth_index: Mutex<HashMap<String, Row>>,
    /// Total evicted (dimension, key) rows across every map.
    dropped: AtomicU64,
}

impl UsageStats {
    /// Fold one usage record into the model, provider and auth-index maps.
    pub fn record(&self, record: &Record) {
        let detail =
            crate::usage_record::ensure_breakdown(record.detail.clone(), &record.provider, &record.executor_type);
        let breakdown = detail.breakdown;
        Self::fold(&self.model, &self.dropped, &record.model, record, &detail, breakdown);
        Self::fold(
            &self.provider,
            &self.dropped,
            &record.provider,
            record,
            &detail,
            breakdown,
        );
        Self::fold(
            &self.auth_index,
            &self.dropped,
            &record.auth_index,
            record,
            &detail,
            breakdown,
        );
    }

    /// A `{group_by, rows, totals, dropped}` object for one dimension. `rows` is sorted
    /// by `last_seen` descending.
    pub fn snapshot(&self, dim: Dim) -> serde_json::Value {
        let map = match dim {
            Dim::Model => &self.model,
            Dim::Provider => &self.provider,
            Dim::AuthIndex => &self.auth_index,
        };
        let rows = map.lock().unwrap_or_else(PoisonError::into_inner);
        let mut ordered: Vec<(&String, &Row)> = rows.iter().collect();
        ordered.sort_by(|a, b| b.1.last_seen.cmp(&a.1.last_seen).then_with(|| a.0.cmp(b.0)));
        let mut totals = Totals::default();
        let list: Vec<serde_json::Value> = ordered
            .iter()
            .map(|(key, row)| {
                totals.add(row);
                row_json(key, row)
            })
            .collect();
        serde_json::json!({
            "group_by": dim.as_str(),
            "rows": list,
            "totals": totals.json(),
            "dropped": self.dropped.load(Ordering::Relaxed),
        })
    }

    /// Fold one record into a single map under `key`; empty keys are skipped.
    fn fold(
        map: &Mutex<HashMap<String, Row>>,
        dropped: &AtomicU64,
        key: &str,
        record: &Record,
        detail: &Detail,
        breakdown: Option<Breakdown>,
    ) {
        if key.is_empty() {
            return;
        }
        let mut rows = map.lock().unwrap_or_else(PoisonError::into_inner);
        if !rows.contains_key(key) && rows.len() >= MAX_ENTRIES {
            // ponytail: linear scan for the oldest row, bounded by MAX_ENTRIES per
            // eviction; swap for an ordered index only if the bound ever grows.
            if let Some(oldest) = rows
                .iter()
                .min_by(|a, b| a.1.last_seen.cmp(&b.1.last_seen))
                .map(|(k, _)| k.clone())
            {
                rows.remove(&oldest);
                dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        let row = rows.entry(key.to_owned()).or_default();
        row.requests += 1;
        if record.failed {
            row.failed += 1;
        }
        row.input += detail.input;
        row.output += detail.output;
        row.reasoning += detail.reasoning;
        row.cached += detail.cached;
        row.cache_read += detail.cache_read;
        row.cache_creation += detail.cache_creation;
        row.total += detail.total;
        row.unclassified += breakdown.map_or(0, |b| b.unclassified_tokens());
        match breakdown.map(|b| b.quality()) {
            Some(Quality::Complete) => row.quality_complete += 1,
            Some(Quality::Unclassified) => row.quality_unclassified += 1,
            Some(Quality::Inconsistent) => row.quality_inconsistent += 1,
            None => {}
        }
        // RFC 3339 sorts lexicographically, so string compare orders by time.
        let ts = record.timestamp.as_str();
        if row.first_seen.is_empty() || ts < row.first_seen.as_str() {
            row.first_seen = ts.to_owned();
        }
        if ts > row.last_seen.as_str() {
            row.last_seen = ts.to_owned();
        }
        if row.providers.len() < MAX_PROVIDERS {
            row.providers.insert(record.provider.clone());
        }
    }
}

/// The serialized row object.
fn row_json(key: &str, row: &Row) -> serde_json::Value {
    serde_json::json!({
        "key": key,
        "requests": row.requests,
        "failed": row.failed,
        "input_tokens": row.input,
        "output_tokens": row.output,
        "reasoning_tokens": row.reasoning,
        "cached_tokens": row.cached,
        "cache_read_tokens": row.cache_read,
        "cache_creation_tokens": row.cache_creation,
        "total_tokens": row.total,
        "unclassified_tokens": row.unclassified,
        "quality": {
            "complete": row.quality_complete,
            "unclassified": row.quality_unclassified,
            "inconsistent": row.quality_inconsistent,
        },
        "first_seen": row.first_seen,
        "last_seen": row.last_seen,
        "providers": row.providers.iter().collect::<Vec<_>>(),
    })
}

/// Running sums over the rows of one snapshot.
#[derive(Default)]
struct Totals {
    requests: u64,
    failed: u64,
    input: i64,
    output: i64,
    reasoning: i64,
    cached: i64,
    cache_read: i64,
    cache_creation: i64,
    total: i64,
    unclassified: i64,
    complete: u64,
    unclassified_q: u64,
    inconsistent: u64,
}

impl Totals {
    fn add(&mut self, row: &Row) {
        self.requests += row.requests;
        self.failed += row.failed;
        self.input += row.input;
        self.output += row.output;
        self.reasoning += row.reasoning;
        self.cached += row.cached;
        self.cache_read += row.cache_read;
        self.cache_creation += row.cache_creation;
        self.total += row.total;
        self.unclassified += row.unclassified;
        self.complete += row.quality_complete;
        self.unclassified_q += row.quality_unclassified;
        self.inconsistent += row.quality_inconsistent;
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "requests": self.requests,
            "failed": self.failed,
            "input_tokens": self.input,
            "output_tokens": self.output,
            "reasoning_tokens": self.reasoning,
            "cached_tokens": self.cached,
            "cache_read_tokens": self.cache_read,
            "cache_creation_tokens": self.cache_creation,
            "total_tokens": self.total,
            "unclassified_tokens": self.unclassified,
            "quality": {
                "complete": self.complete,
                "unclassified": self.unclassified_q,
                "inconsistent": self.inconsistent,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(input: i64, output: i64, reasoning: i64, cache_read: i64, cache_creation: i64, total: i64) -> Detail {
        Detail {
            input,
            output,
            reasoning,
            cached: cache_read,
            cache_read,
            cache_creation,
            total,
            ..Detail::default()
        }
    }

    fn record(model: &str, provider: &str, auth: &str, ts: &str, failed: bool, detail: Detail) -> Record {
        Record {
            model: model.to_owned(),
            provider: provider.to_owned(),
            auth_index: auth.to_owned(),
            timestamp: ts.to_owned(),
            failed,
            detail,
            ..Record::default()
        }
    }

    /// A zero-padded, strictly increasing RFC 3339 timestamp for index `i`.
    fn ts(i: usize) -> String {
        format!("2026-01-01T00:00:00.{i:09}Z")
    }

    fn keys(snap: &serde_json::Value) -> Vec<String> {
        snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["key"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn parse_dimension() {
        assert_eq!(parse("model"), Some(Dim::Model));
        assert_eq!(parse(" provider "), Some(Dim::Provider));
        assert_eq!(parse("auth_index"), Some(Dim::AuthIndex));
        assert_eq!(parse("Model"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn snapshot_model_sums_and_quality() {
        let stats = UsageStats::default();
        // m1/provider claude: Independent semantics, both complete.
        stats.record(&record(
            "m1",
            "claude",
            "a1",
            "2026-01-01T00:00:00Z",
            false,
            detail(100, 50, 20, 10, 5, 185),
        ));
        stats.record(&record(
            "m1",
            "claude",
            "a2",
            "2026-01-01T00:00:05Z",
            true,
            detail(200, 60, 10, 0, 0, 270),
        ));
        // m2/provider unknown: Unknown semantics, total is unclassified.
        stats.record(&record(
            "m2",
            "unknown",
            "a1",
            "2026-01-01T00:00:10Z",
            false,
            detail(0, 0, 0, 0, 0, 40),
        ));

        let snap = stats.snapshot(Dim::Model);
        assert_eq!(snap["group_by"], "model");
        assert_eq!(snap["dropped"], 0);
        let rows = snap["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        // last_seen descending: m2 (…10Z) before m1 (…05Z).
        assert_eq!(rows[0]["key"], "m2");
        assert_eq!(rows[1]["key"], "m1");

        let m1 = &rows[1];
        assert_eq!(m1["requests"], 2);
        assert_eq!(m1["failed"], 1);
        assert_eq!(m1["input_tokens"], 300);
        assert_eq!(m1["output_tokens"], 110);
        assert_eq!(m1["reasoning_tokens"], 30);
        assert_eq!(m1["cached_tokens"], 10);
        assert_eq!(m1["cache_read_tokens"], 10);
        assert_eq!(m1["cache_creation_tokens"], 5);
        assert_eq!(m1["total_tokens"], 455);
        assert_eq!(m1["unclassified_tokens"], 0);
        assert_eq!(m1["quality"]["complete"], 2);
        assert_eq!(m1["quality"]["unclassified"], 0);
        assert_eq!(m1["quality"]["inconsistent"], 0);
        assert_eq!(m1["first_seen"], "2026-01-01T00:00:00Z");
        assert_eq!(m1["last_seen"], "2026-01-01T00:00:05Z");
        assert_eq!(m1["providers"], serde_json::json!(["claude"]));

        let m2 = &rows[0];
        assert_eq!(m2["requests"], 1);
        assert_eq!(m2["failed"], 0);
        assert_eq!(m2["total_tokens"], 40);
        assert_eq!(m2["unclassified_tokens"], 40);
        assert_eq!(m2["quality"]["complete"], 0);
        assert_eq!(m2["quality"]["unclassified"], 1);
        assert_eq!(m2["quality"]["inconsistent"], 0);

        let totals = &snap["totals"];
        assert_eq!(totals["requests"], 3);
        assert_eq!(totals["failed"], 1);
        assert_eq!(totals["input_tokens"], 300);
        assert_eq!(totals["output_tokens"], 110);
        assert_eq!(totals["total_tokens"], 495);
        assert_eq!(totals["unclassified_tokens"], 40);
        assert_eq!(totals["quality"]["complete"], 2);
        assert_eq!(totals["quality"]["unclassified"], 1);

        // Provider and auth-index dimensions see the same records.
        let provider = stats.snapshot(Dim::Provider);
        assert_eq!(keys(&provider), vec!["unknown".to_owned(), "claude".to_owned()]);
        assert_eq!(provider["totals"]["requests"], 3);
        let auth = stats.snapshot(Dim::AuthIndex);
        let a1 = auth["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["key"] == "a1")
            .unwrap();
        assert_eq!(a1["requests"], 2);
        assert_eq!(a1["input_tokens"], 100);
        assert_eq!(a1["total_tokens"], 225);
        assert_eq!(a1["unclassified_tokens"], 40);
    }

    #[test]
    fn eviction_caps_each_dimension_and_counts_dropped() {
        let stats = UsageStats::default();
        let n = 4100usize;
        for i in 0..n {
            stats.record(&record(
                &format!("m{i}"),
                &format!("p{i}"),
                &format!("a{i}"),
                &ts(i),
                false,
                detail(0, 0, 0, 0, 0, 0),
            ));
        }
        for dim in [Dim::Model, Dim::Provider, Dim::AuthIndex] {
            let snap = stats.snapshot(dim);
            assert_eq!(snap["rows"].as_array().unwrap().len(), MAX_ENTRIES, "{}", dim.as_str());
        }
        let snap = stats.snapshot(Dim::Model);
        // 4100 distinct keys per map, 4096 kept, 4 evicted in each of three maps.
        assert_eq!(snap["dropped"], 12);
        let present = keys(&snap);
        assert!(!present.contains(&"m0".to_owned()));
        assert!(!present.contains(&"m3".to_owned()));
        assert!(present.contains(&"m4".to_owned()));
        assert!(present.contains(&"m4099".to_owned()));
    }

    #[test]
    fn empty_snapshot_is_zero() {
        let stats = UsageStats::default();
        for dim in [Dim::Model, Dim::Provider, Dim::AuthIndex] {
            let snap = stats.snapshot(dim);
            assert_eq!(snap["rows"].as_array().unwrap().len(), 0);
            assert_eq!(snap["dropped"], 0);
            assert_eq!(snap["totals"]["requests"], 0);
            assert_eq!(snap["totals"]["total_tokens"], 0);
            assert_eq!(snap["totals"]["unclassified_tokens"], 0);
            assert_eq!(snap["totals"]["quality"]["complete"], 0);
        }
    }

    #[test]
    fn empty_key_is_skipped() {
        let stats = UsageStats::default();
        stats.record(&record(
            "",
            "claude",
            "",
            "2026-01-01T00:00:00Z",
            false,
            detail(5, 5, 0, 0, 0, 10),
        ));
        assert_eq!(stats.snapshot(Dim::Model)["rows"].as_array().unwrap().len(), 0);
        assert_eq!(stats.snapshot(Dim::AuthIndex)["rows"].as_array().unwrap().len(), 0);
        // The provider key is non-empty, so only that dimension keeps the row.
        assert_eq!(stats.snapshot(Dim::Provider)["rows"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn totals_match_remaining_rows_after_eviction() {
        let stats = UsageStats::default();
        for i in 0..4100usize {
            let total = i as i64 + 1;
            stats.record(&record(
                &format!("m{i}"),
                &format!("p{i}"),
                &format!("a{i}"),
                &ts(i),
                i % 2 == 0,
                detail(i as i64, 1, 0, 0, 0, total),
            ));
        }
        let snap = stats.snapshot(Dim::Model);
        let rows = snap["rows"].as_array().unwrap();
        let sum = |field: &str| -> i64 { rows.iter().map(|r| r[field].as_i64().unwrap()).sum() };
        assert_eq!(snap["totals"]["total_tokens"].as_i64().unwrap(), sum("total_tokens"));
        assert_eq!(snap["totals"]["input_tokens"].as_i64().unwrap(), sum("input_tokens"));
        assert_eq!(snap["totals"]["output_tokens"].as_i64().unwrap(), sum("output_tokens"));
        assert_eq!(
            snap["totals"]["unclassified_tokens"].as_i64().unwrap(),
            sum("unclassified_tokens")
        );
        let requests: i64 = rows.iter().map(|r| r["requests"].as_i64().unwrap()).sum();
        assert_eq!(snap["totals"]["requests"].as_i64().unwrap(), requests);
        let failed: i64 = rows.iter().map(|r| r["failed"].as_i64().unwrap()).sum();
        assert_eq!(snap["totals"]["failed"].as_i64().unwrap(), failed);
    }

    #[test]
    fn providers_set_is_bounded() {
        let stats = UsageStats::default();
        for i in 0..100usize {
            stats.record(&record(
                "m",
                &format!("p{i}"),
                "a",
                &ts(i),
                false,
                detail(0, 0, 0, 0, 0, 0),
            ));
        }
        let snap = stats.snapshot(Dim::Model);
        let rows = snap["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["providers"].as_array().unwrap().len(), MAX_PROVIDERS);
        assert_eq!(MAX_PROVIDERS, 64);
    }

    #[test]
    fn snapshot_has_no_sensitive_fields() {
        let stats = UsageStats::default();
        stats.record(&record(
            "m1",
            "claude",
            "a1",
            "2026-01-01T00:00:00Z",
            false,
            detail(1, 1, 0, 0, 0, 2),
        ));
        let s = stats.snapshot(Dim::Model).to_string();
        for needle in ["api_key", "client_ip", "user_agent", "x_forwarded_for", "session_id"] {
            assert!(!s.contains(needle), "snapshot leaked {needle}: {s}");
        }
    }
}
