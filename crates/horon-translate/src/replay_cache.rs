//! Adapters for two internal/cache stores: the Antigravity reasoning replay cache
//! (antigravity_reasoning_replay_cache.go), which the Gemini Responses translator uses to
//! keep thought signatures that trail visible text out of the client-visible output, and
//! the thinking signature cache (signature_cache.go), which the Claude -> Antigravity
//! translators use to recover signatures of thinking text the client sent back unsigned.
//!
//! ponytail: owner is whoever ports internal/cache with the Antigravity executor (Google
//! thread). The replay cache is the in-process store only, keyed, normalized and bounded
//! like Go (1 h sliding TTL, 10,240 entries including absent-key tombstones, oldest 128
//! evicted); Go's Home KV backend, snapshots, revisions and compare-and-swap writes (used
//! by executors, not translators) are not ported, and only `thought_signature` items (the
//! kind translators write) are accepted. Go's 10-minute background purge runs lazily, at
//! the nominal tick times, on the next cache access (`Caches::sweep`). Go purges when its
//! goroutine wakes, a scheduler-dependent instant after the tick, so an entry exactly at
//! its TTL on a tick can differ within that jitter; Go's own timing is not deterministic
//! there either. Swap these functions for the shared cache at integration.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use horon_common::json::{self as gj, Kind};

use crate::common::trim_space;

const TTL: Duration = Duration::from_secs(3600);
const MAX_ENTRIES: usize = 10240;
const EVICT_BATCH: usize = 128;
const MIN_SIGNATURE_LEN: usize = 16;
const MAX_ITEMS_PER_ENTRY: usize = 4096;
const MAX_BYTES_PER_ENTRY: usize = 16 << 20;

/// One replay entry; `items` is empty for an absent-key tombstone (Go's `Deleted`).
struct Entry {
    items: Vec<Vec<u8>>,
    at: Instant,
}

/// CacheCleanupInterval: Go's purge ticker, started by the first write to the signature
/// cache or the first access to the replay cache (`cacheCleanupOnce`).
const CLEANUP_INTERVAL: Duration = Duration::from_secs(600);

/// Both in-process stores and Go's shared purge ticker, behind one lock.
#[derive(Default)]
struct Caches {
    replay: HashMap<String, Entry>,
    /// model group -> text hash -> (signature, last use).
    signatures: HashMap<String, HashMap<String, (Vec<u8>, Instant)>>,
    /// Lower bound on the oldest signature last-use; lets an over-cap write skip the
    /// expired-entry scan when nothing can be expired yet.
    sig_oldest_at: Option<Instant>,
    /// (ticker start, ticks already purged).
    ticker: Option<(Instant, u32)>,
}

static CACHES: LazyLock<Mutex<Caches>> = LazyLock::new(Default::default);

fn caches() -> std::sync::MutexGuard<'static, Caches> {
    CACHES.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Caches {
    /// purgeExpiredCaches for every ticker tick that passed since the last access. No
    /// access happens in between, so purging once at the latest missed tick time removes
    /// exactly what Go's ticks would have removed by now. `start`: whether this access
    /// starts Go's ticker (signature reads do not).
    fn sweep(&mut self, now: Instant, start: bool) {
        if self.ticker.is_none() && !start {
            return;
        }
        let (begin, done) = self.ticker.get_or_insert((now, 0));
        let ticks = (now.duration_since(*begin).as_secs() / CLEANUP_INTERVAL.as_secs()) as u32;
        if ticks <= *done {
            return;
        }
        *done = ticks;
        let tick_at = *begin + CLEANUP_INTERVAL * ticks;
        self.signatures.retain(|_, entries| {
            entries.retain(|_, (_, at)| tick_at.saturating_duration_since(*at) <= SIGNATURE_TTL);
            !entries.is_empty()
        });
        self.replay
            .retain(|_, e| tick_at.saturating_duration_since(e.at) <= TTL);
    }

    /// evictOldestAntigravityReasoningReplayEntries.
    fn evict_oldest(&mut self) {
        let mut oldest: Vec<(Instant, String)> = self.replay.iter().map(|(k, e)| (e.at, k.clone())).collect();
        oldest.sort();
        for (_, k) in oldest.into_iter().take(EVICT_BATCH) {
            self.replay.remove(&k);
        }
    }

    fn put(&mut self, key: String, items: Vec<Vec<u8>>, now: Instant) {
        self.sweep(now, true);
        self.replay.insert(key, Entry { items, at: now });
        if self.replay.len() > MAX_ENTRIES {
            self.evict_oldest();
        }
    }

    /// A hit refreshes the entry (tombstones included); a miss or an expired entry
    /// reserves a tombstone that counts toward capacity
    /// (reserveAntigravityReasoningReplayAbsentLocked).
    fn get(&mut self, key: String, now: Instant) -> Option<Vec<Vec<u8>>> {
        self.sweep(now, true);
        if let Some(entry) = self.replay.get_mut(&key) {
            if now.duration_since(entry.at) <= TTL {
                entry.at = now;
                return (!entry.items.is_empty()).then(|| entry.items.clone());
            }
            self.replay.remove(&key);
        }
        if self.replay.len() >= MAX_ENTRIES {
            self.evict_oldest();
        }
        self.replay.insert(key, Entry { items: vec![], at: now });
        None
    }

    fn cache_signature(&mut self, group: &str, hash: String, signature: &[u8], now: Instant) {
        self.sweep(now, true);
        self.signatures
            .entry(group.to_owned())
            .or_default()
            .insert(hash, (signature.to_vec(), now));
        if self.sig_oldest_at.is_none() {
            self.sig_oldest_at = Some(now);
        }
        if self.signatures.get(group).map_or(0, |entries| entries.len()) > MAX_SIGNATURE_ENTRIES_PER_GROUP
            || self.signature_total() > MAX_SIGNATURE_ENTRIES
        {
            self.evict_signatures(now, group);
        }
    }

    /// Total signatures across every model group.
    fn signature_total(&self) -> usize {
        self.signatures.values().map(|entries| entries.len()).sum()
    }

    /// Enforces the signature-cache entry caps. Expired signatures (past the 3 h TTL) go
    /// first; only when none are expired does it drop least-recently-used signatures, oldest
    /// last-use first. Each call sheds only the overage (one entry per insertion), and the
    /// expired scan is skipped when the oldest last-use bound proves nothing can be expired,
    /// so the lock shared with the replay cache is never held for a long sweep.
    fn evict_signatures(&mut self, now: Instant, group: &str) {
        if self
            .sig_oldest_at
            .is_some_and(|oldest| now.saturating_duration_since(oldest) > SIGNATURE_TTL)
        {
            let mut oldest: Option<Instant> = None;
            self.signatures.retain(|_, entries| {
                entries.retain(|_, (_, at)| {
                    if now.saturating_duration_since(*at) <= SIGNATURE_TTL {
                        oldest = Some(oldest.map_or(*at, |seen| seen.min(*at)));
                        true
                    } else {
                        false
                    }
                });
                !entries.is_empty()
            });
            self.sig_oldest_at = oldest;
        }
        while self.signatures.get(group).map_or(0, |entries| entries.len()) > MAX_SIGNATURE_ENTRIES_PER_GROUP {
            if !self.evict_oldest_in_group(group) {
                break;
            }
        }
        while self.signature_total() > MAX_SIGNATURE_ENTRIES {
            if !self.evict_oldest_signature() {
                break;
            }
        }
    }

    /// Removes the least-recently-used signature of one group; false when it is empty.
    fn evict_oldest_in_group(&mut self, group: &str) -> bool {
        let Some(entries) = self.signatures.get_mut(group) else {
            return false;
        };
        let Some(hash) = entries
            .iter()
            .min_by_key(|(_, (_, at))| *at)
            .map(|(hash, _)| hash.clone())
        else {
            return false;
        };
        entries.remove(&hash);
        let empty = entries.is_empty();
        if empty {
            self.signatures.remove(group);
        }
        true
    }

    /// Removes the least-recently-used signature across every group.
    fn evict_oldest_signature(&mut self) -> bool {
        let mut oldest: Option<(Instant, String, String)> = None;
        for (group, entries) in &self.signatures {
            for (hash, (_, at)) in entries {
                if oldest.as_ref().is_none_or(|(seen, _, _)| at < seen) {
                    oldest = Some((*at, group.clone(), hash.clone()));
                }
            }
        }
        let Some((_, group, hash)) = oldest else {
            return false;
        };
        if let Some(entries) = self.signatures.get_mut(&group) {
            entries.remove(&hash);
            let empty = entries.is_empty();
            if empty {
                self.signatures.remove(&group);
            }
        }
        true
    }

    /// A hit refreshes the entry; an expired entry is removed.
    fn cached_signature(&mut self, group: &str, hash: &str, now: Instant) -> Option<Vec<u8>> {
        self.sweep(now, false);
        let entries = self.signatures.get_mut(group)?;
        let (signature, at) = entries.get_mut(hash)?;
        if now.duration_since(*at) > SIGNATURE_TTL {
            entries.remove(hash);
            return None;
        }
        *at = now;
        Some(signature.clone())
    }
}

fn key(model: &str, session: &str) -> Option<String> {
    let (model, session) = (model.trim(), session.trim());
    (!model.is_empty() && !session.is_empty()).then(|| ["antigravity-reasoning-replay", model, session].join("\x00"))
}

/// normalizeAntigravityThoughtSignatureReplayItem.
fn normalize_item(item: &[u8]) -> Option<Vec<u8>> {
    let r = gj::parse(item);
    if trim_space(&r.get("type").bytes()) != b"thought_signature" {
        return None;
    }
    let mut sig = trim_space(&r.get("thoughtSignature").bytes()).to_vec();
    if sig.is_empty() {
        sig = trim_space(&r.get("thought_signature").bytes()).to_vec();
    }
    if sig.is_empty() || sig == b"skip_thought_signature_validator" || sig.len() < MIN_SIGNATURE_LEN {
        return None;
    }
    let mut out = br#"{"type":"thought_signature"}"#.to_vec();
    gj::set_str(&mut out, "thoughtSignature", &sig);
    for field in ["contentIndex", "partIndex"] {
        let v = r.get(field);
        if v.kind == Kind::Number {
            gj::set_int(&mut out, field, v.int());
        }
    }
    let target_kind = trim_space(&r.get("targetKind").bytes()).to_vec();
    if target_kind == b"text" || target_kind == b"thought" {
        gj::set_str(&mut out, "targetKind", &target_kind);
    }
    let target_hash = trim_space(&r.get("targetHash").bytes()).to_vec();
    if !target_hash.is_empty() {
        gj::set_str(&mut out, "targetHash", &target_hash);
    }
    let occurrence = r.get("targetOccurrence");
    if occurrence.kind == Kind::Number && occurrence.int() >= 0 {
        gj::set_int(&mut out, "targetOccurrence", occurrence.int());
    }
    let context_hash = trim_space(&r.get("contextHash").bytes()).to_vec();
    if !context_hash.is_empty() {
        gj::set_str(&mut out, "contextHash", &context_hash);
    }
    Some(out)
}

/// normalizeAntigravityReasoningReplayItems.
fn normalize(items: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    if items.len() > MAX_ITEMS_PER_ENTRY {
        return None;
    }
    let mut out = vec![];
    let mut total = 0;
    for item in items.iter().filter_map(|i| normalize_item(i)) {
        total += item.len();
        if total > MAX_BYTES_PER_ENTRY {
            return None;
        }
        out.push(item);
    }
    (!out.is_empty()).then_some(out)
}

/// cache.CacheAntigravityReasoningReplayItems.
pub(crate) fn put(model: &str, session: &str, items: &[Vec<u8>]) -> bool {
    let (Some(key), Some(items)) = (key(model, session), normalize(items)) else {
        return false;
    };
    caches().put(key, items, Instant::now());
    true
}

/// cache.GetAntigravityReasoningReplayItems.
pub(crate) fn get(model: &str, session: &str) -> Option<Vec<Vec<u8>>> {
    let key = key(model, session)?;
    caches().get(key, Instant::now())
}

// ---------------------------------------------------------------------------------------
// Thinking signature cache (internal/cache/signature_cache.go)
//
// ponytail: same owner and swap as above. In-process store only: Go's Home KV backend is
// not ported. Expired entries and empty groups go in `Caches::sweep`, as in Go.

const SIGNATURE_TTL: Duration = Duration::from_secs(3 * 3600);
const MIN_VALID_SIGNATURE_LEN: usize = 50;
const GEMINI_BYPASS: &str = "skip_thought_signature_validator";

/// Defensive entry caps for the signature cache. Go has no cap here, so under normal
/// operation (a 3 h TTL times a realistic write rate) neither is reached and a restore is
/// byte-identical to Go; only extreme load reaches them, and then the oldest signature is
/// evicted and that turn's restore falls back exactly like a cache miss (the no-cache path).
const MAX_SIGNATURE_ENTRIES: usize = 131_072;
const MAX_SIGNATURE_ENTRIES_PER_GROUP: usize = 8_192;

static SIGNATURE_CACHE_ENABLED: AtomicBool = AtomicBool::new(true);
static SIGNATURE_BYPASS_STRICT: AtomicBool = AtomicBool::new(false);

/// cache.SetSignatureCacheEnabled and SetSignatureBypassStrictMode (Go applies config
/// `antigravity-signature-cache-enabled`, default true, and
/// `antigravity-signature-bypass-strict`, default false, on load and reload).
pub fn set_signature_cache_config(enabled: bool, bypass_strict: bool) {
    SIGNATURE_CACHE_ENABLED.store(enabled, Ordering::Relaxed);
    SIGNATURE_BYPASS_STRICT.store(bypass_strict, Ordering::Relaxed);
}

/// cache.SignatureCacheEnabled.
pub(crate) fn signature_cache_enabled() -> bool {
    SIGNATURE_CACHE_ENABLED.load(Ordering::Relaxed)
}

/// cache.SignatureBypassStrictMode.
pub(crate) fn signature_bypass_strict() -> bool {
    SIGNATURE_BYPASS_STRICT.load(Ordering::Relaxed)
}

/// cache.GetModelGroup.
pub(crate) fn model_group(model: &str) -> &str {
    if model.contains("gpt") {
        "gpt"
    } else if model.contains("claude") {
        "claude"
    } else if model.contains("gemini") {
        "gemini"
    } else {
        model
    }
}

fn text_hash(text: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::common::hex(&Sha256::digest(text))[..16].to_owned()
}

/// cache.CacheSignatureBestEffort.
pub(crate) fn cache_signature(model: &str, text: &[u8], signature: &[u8]) -> bool {
    if text.is_empty() || signature.is_empty() || signature.len() < MIN_VALID_SIGNATURE_LEN {
        return false;
    }
    caches().cache_signature(model_group(model), text_hash(text), signature, Instant::now());
    true
}

/// cache.GetCachedSignatureRequired: a Gemini-group miss is the bypass sentinel.
pub(crate) fn cached_signature(model: &str, text: &[u8]) -> Vec<u8> {
    let group = model_group(model);
    let hit = if text.is_empty() {
        None
    } else {
        caches().cached_signature(group, &text_hash(text), Instant::now())
    };
    match hit {
        Some(signature) => signature,
        None if group == "gemini" => GEMINI_BYPASS.as_bytes().to_vec(),
        None => vec![],
    }
}

/// cache.HasValidSignature.
pub(crate) fn has_valid_signature(model: &str, signature: &[u8]) -> bool {
    (!signature.is_empty() && signature.len() >= MIN_VALID_SIGNATURE_LEN)
        || (signature == GEMINI_BYPASS.as_bytes() && model_group(model) == "gemini")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_normalized_thought_signatures_only() {
        let items = vec![
            br#"{"type":"thought_signature","thoughtSignature":" sig-aaaaaaaaaaaaaaaa ","targetKind":"text","targetHash":"h","extra":1}"#.to_vec(),
            br#"{"type":"thought_signature","thoughtSignature":"short"}"#.to_vec(),
            br#"{"type":"other","thoughtSignature":"sig-bbbbbbbbbbbbbbbb"}"#.to_vec(),
        ];
        assert!(put("replay-test-model", "replay-test-session", &items));
        assert_eq!(
            get("replay-test-model", "replay-test-session").unwrap(),
            vec![br#"{"type":"thought_signature","thoughtSignature":"sig-aaaaaaaaaaaaaaaa","targetKind":"text","targetHash":"h"}"#.to_vec()]
        );
        assert!(!put("replay-test-model", "replay-test-empty", &items[1..]));
        assert!(get("replay-test-model", "replay-test-empty").is_none());
        assert!(!put(" ", "s", &items));
    }

    fn min(n: u64) -> Duration {
        Duration::from_secs(60 * n)
    }

    #[test]
    fn misses_reserve_tombstones_that_count_toward_capacity() {
        let (mut c, t0) = (Caches::default(), Instant::now());
        c.put("a".into(), vec![b"item".to_vec()], t0);
        // 10,239 distinct misses fill the map; the next one evicts the oldest 128, `a` first.
        for i in 1..MAX_ENTRIES {
            assert!(
                c.get(format!("absent-{i}"), t0 + Duration::from_micros(i as u64))
                    .is_none()
            );
        }
        assert_eq!(c.replay.len(), MAX_ENTRIES);
        assert_eq!(c.get("a".into(), t0 + min(1)).as_deref(), Some(&[b"item".to_vec()][..]));
        c.get("absent-last".into(), t0 + min(2));
        assert_eq!(c.replay.len(), MAX_ENTRIES - EVICT_BATCH + 1);
        assert!(c.replay.contains_key("a"), "refreshed by the hit");
        assert!(!c.replay.contains_key("absent-1"));
        // A tombstone hit stays a miss and refreshes the tombstone.
        assert!(c.get("absent-last".into(), t0 + min(3)).is_none());
        assert_eq!(c.replay["absent-last"].at, t0 + min(3));
    }

    #[test]
    fn expired_entries_purge_at_go_tick_times() {
        let (mut c, t0) = (Caches::default(), Instant::now());
        c.put("first".into(), vec![b"i".to_vec()], t0); // starts the ticker
        c.put("x".into(), vec![b"i".to_vec()], t0 + min(5));
        // Tick 6 (t0+60m): `first` is exactly 60m old (Go deletes only past the TTL) and
        // `x` 55m old, so both stay, although `x` is 61m old now.
        c.get("y".into(), t0 + min(66));
        assert!(c.replay.contains_key("x"));
        assert!(c.replay.contains_key("first"));
        c.get("z".into(), t0 + min(70) + Duration::from_secs(1));
        assert!(!c.replay.contains_key("x") && !c.replay.contains_key("first"));
        assert!(c.replay.contains_key("y") && c.replay.contains_key("z"));
    }

    #[test]
    fn signature_groups_expire_and_reads_do_not_start_the_ticker() {
        let (mut c, t0) = (Caches::default(), Instant::now());
        assert!(c.cached_signature("claude", "h", t0).is_none());
        assert!(c.ticker.is_none());
        let sig = b"\xff raw bytes".to_vec();
        c.cache_signature("claude", "h".into(), &sig, t0 + min(5));
        c.cache_signature("gemini", "g".into(), &sig, t0 + min(5));
        assert_eq!(c.ticker, Some((t0 + min(5), 0)));
        assert_eq!(c.cached_signature("claude", "h", t0 + min(100)), Some(sig.clone()));
        // gemini/g is 180m old at tick 18 (t0+185m): kept; past the TTL at tick 19.
        c.cached_signature("claude", "none", t0 + min(186));
        assert!(c.signatures.contains_key("gemini"));
        c.cached_signature("claude", "none", t0 + min(196));
        assert!(!c.signatures.contains_key("gemini"), "empty group removed");
        assert!(c.signatures.contains_key("claude"), "refreshed at t0+100m");
    }

    /// Over the per-group cap with expired entries present: the expired signatures go first,
    /// so the group drops far below the cap (pure overage eviction would have shed only one).
    #[test]
    fn over_cap_eviction_drops_expired_signatures_first() {
        let (mut c, t0) = (Caches::default(), Instant::now());
        let sig = vec![0u8; MIN_VALID_SIGNATURE_LEN];
        for i in 0..MAX_SIGNATURE_ENTRIES_PER_GROUP {
            c.cache_signature("claude", format!("old-{i}"), &sig, t0);
        }
        assert_eq!(c.signatures["claude"].len(), MAX_SIGNATURE_ENTRIES_PER_GROUP);
        // A fresh write past the 3 h TTL makes every `old-*` entry expired and pushes the
        // group one over its cap, so eviction runs while expired entries are present.
        let now = t0 + SIGNATURE_TTL + Duration::from_secs(1);
        c.cache_signature("claude", "fresh".into(), &sig, now);
        let entries = &c.signatures["claude"];
        assert_eq!(
            entries.len(),
            1,
            "every expired entry was evicted, not just the overage"
        );
        assert!(entries.contains_key("fresh"), "the fresh signature survived");
    }

    /// Over the per-group cap with nothing expired: the least-recently-used entry goes and
    /// the group stays at or below the cap.
    #[test]
    fn over_cap_eviction_without_expired_drops_least_recently_used() {
        let (mut c, t0) = (Caches::default(), Instant::now());
        let sig = vec![0u8; MIN_VALID_SIGNATURE_LEN];
        for i in 0..MAX_SIGNATURE_ENTRIES_PER_GROUP {
            c.cache_signature("claude", format!("h-{i}"), &sig, t0 + Duration::from_micros(i as u64));
        }
        assert_eq!(c.signatures["claude"].len(), MAX_SIGNATURE_ENTRIES_PER_GROUP);
        c.cache_signature(
            "claude",
            "new".into(),
            &sig,
            t0 + Duration::from_micros(MAX_SIGNATURE_ENTRIES_PER_GROUP as u64),
        );
        let entries = &c.signatures["claude"];
        assert!(
            entries.len() <= MAX_SIGNATURE_ENTRIES_PER_GROUP,
            "group stays at or below its cap, got {}",
            entries.len()
        );
        assert!(
            !entries.contains_key("h-0"),
            "the least recently used entry was evicted"
        );
        assert!(entries.contains_key("new"));
    }

    /// Over the total cap while every group is under its per-group cap: the globally oldest
    /// signature goes and the total stays at or below the cap.
    #[test]
    fn total_cap_evicts_least_recently_used_across_groups() {
        let (mut c, t0) = (Caches::default(), Instant::now());
        let sig = vec![0u8; MIN_VALID_SIGNATURE_LEN];
        let groups = MAX_SIGNATURE_ENTRIES / MAX_SIGNATURE_ENTRIES_PER_GROUP;
        let mut n = 0u64;
        for g in 0..groups {
            let group = format!("g{g}");
            for i in 0..MAX_SIGNATURE_ENTRIES_PER_GROUP {
                c.cache_signature(&group, format!("h-{i}"), &sig, t0 + Duration::from_micros(n));
                n += 1;
            }
        }
        assert_eq!(c.signature_total(), MAX_SIGNATURE_ENTRIES);
        c.cache_signature("extra", "new".into(), &sig, t0 + Duration::from_micros(n));
        assert!(c.signature_total() <= MAX_SIGNATURE_ENTRIES, "total cap held");
        assert!(c.signatures.contains_key("extra"), "the newest signature survived");
        assert!(
            !c.signatures["g0"].contains_key("h-0"),
            "the globally oldest went first"
        );
    }

    /// Four threads mixing inserts, hits and misses keep both caps and never deadlock or panic.
    #[test]
    fn concurrent_signature_inserts_keep_the_cap_without_deadlock() {
        use std::sync::Arc;
        let caches = Arc::new(Mutex::new(Caches::default()));
        let sig = vec![0u8; MIN_VALID_SIGNATURE_LEN];
        let workers: Vec<_> = (0..4)
            .map(|t| {
                let caches = Arc::clone(&caches);
                let sig = sig.clone();
                let group = format!("g{t}");
                std::thread::spawn(move || {
                    for i in 0..10_000u32 {
                        let hash = format!("h-{t}-{i}");
                        let mut c = caches.lock().unwrap();
                        let now = Instant::now();
                        c.cache_signature(&group, hash.clone(), &sig, now);
                        let _ = c.cached_signature(&group, &hash, now);
                        let _ = c.cached_signature(&group, &format!("miss-{t}-{i}"), now);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let c = caches.lock().unwrap();
        assert!(
            c.signature_total() <= MAX_SIGNATURE_ENTRIES,
            "total cap held under concurrency"
        );
        for (group, entries) in &c.signatures {
            assert!(
                entries.len() <= MAX_SIGNATURE_ENTRIES_PER_GROUP,
                "group {group} exceeded its cap with {} entries",
                entries.len()
            );
        }
    }

    /// Eviction must not hold the lock shared with the replay cache for long: at the cap,
    /// 10,000 insert/evict cycles each hold it well under 5 ms.
    #[test]
    fn eviction_holds_the_lock_under_five_milliseconds() {
        let mut c = Caches::default();
        let sig = vec![0u8; MIN_VALID_SIGNATURE_LEN];
        let base = Instant::now();
        for i in 0..MAX_SIGNATURE_ENTRIES_PER_GROUP {
            c.cache_signature("claude", format!("h-{i}"), &sig, base + Duration::from_micros(i as u64));
        }
        let rounds = 10_000u64;
        let (mut worst, mut total) = (Duration::ZERO, Duration::ZERO);
        for i in 0..rounds {
            let now = base + Duration::from_micros(MAX_SIGNATURE_ENTRIES_PER_GROUP as u64 + i);
            let start = Instant::now();
            c.cache_signature("claude", format!("e-{i}"), &sig, now);
            let held = start.elapsed();
            total += held;
            worst = worst.max(held);
        }
        assert_eq!(c.signatures["claude"].len(), MAX_SIGNATURE_ENTRIES_PER_GROUP);
        eprintln!(
            "signature-cache eviction lock hold over {rounds} rounds: worst {worst:?}, mean {:?}",
            total / rounds as u32
        );
        // The 5 ms gate is the release-build criterion (plan task 14). A debug build runs the
        // eviction an order of magnitude slower and the harness may preempt a single call, so
        // debug only sanity-checks; `cargo test --release` exercises the gate itself.
        let limit = if cfg!(debug_assertions) {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(5)
        };
        assert!(
            worst < limit,
            "worst lock hold under eviction was {worst:?} (limit {limit:?})"
        );
    }
}
