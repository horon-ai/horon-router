//! Goldens recorded from Go's own LCP tests (sdk/cliproxy/session/lcp_test.go,
//! lcp_lookup_test.go and the sdk/cliproxy/auth selector tests): every outermost call
//! of the public surface with its inputs, the matcher clock and its result, replayed
//! here call by call. The recorder is tests/reference/lcp/zz_trace.go.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::Value;

use super::*;

fn trace() -> Vec<Value> {
    let packed = include_bytes!("../tests/fixtures/lcp_go.jsonl.zst");
    let text = zstd::stream::decode_all(&packed[..]).unwrap();
    String::from_utf8(text)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn bytes(v: &Value) -> Vec<u8> {
    match v.as_str() {
        Some(s) => base64::engine::general_purpose::STANDARD.decode(s).unwrap(),
        None => Vec::new(),
    }
}

fn turns(v: &Value) -> Vec<Turn> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|t| Turn {
            role: t["role"].as_str().unwrap().into(),
            parts: t["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| Part {
                    kind: p["kind"].as_str().unwrap().into(),
                    mime: p["mime"].as_str().unwrap().into(),
                    value: bytes(&p["value"]),
                    digest: p["digest"].as_str().unwrap().into(),
                    original_size: p["original_size"].as_i64().unwrap(),
                    sampled: p["sampled"].as_bool().unwrap(),
                })
                .collect(),
        })
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

fn prepared(r: &Value) -> Prepared {
    Prepared {
        fingerprints: strings(&r["fps"]),
        min_prefix: r["min"].as_u64().unwrap() as usize,
        tails: strings(&r["tails"]),
        environment: r["env"].as_str().unwrap_or_default().into(),
    }
}

fn go_match(out: &Value) -> Match {
    let s = |k: &str| out[k].as_str().unwrap_or_default().to_owned();
    Match {
        auth: s("AuthID"),
        session: s("SessionID"),
        parent: s("ParentSessionID"),
        prefix_length: out["PrefixLength"].as_u64().unwrap_or_default() as usize,
        fork: out["IsFork"].as_bool().unwrap(),
        compaction: out["IsCompaction"].as_bool().unwrap(),
        node_kind: s("NodeKind"),
        access: out["AccessNumber"].as_u64().unwrap(),
    }
}

#[test]
fn canonical_turns_and_fingerprints_match_go() {
    let (mut extracts, mut fingerprints) = (0, 0);
    for r in trace() {
        match r["op"].as_str().unwrap() {
            "extract" => {
                extracts += 1;
                let got = extract(r["format"].as_str().unwrap(), &bytes(&r["payload"]));
                assert_eq!(got, turns(&r["turns"]), "{} {}", r["test"], r["format"]);
            }
            "fingerprint" => {
                fingerprints += 1;
                let turn = turns(&Value::Array(vec![r["turn"].clone()])).remove(0);
                assert_eq!(fingerprint(&turn), r["out"].as_str().unwrap(), "{}", r["test"]);
            }
            "prepare" | "prepare_ext" => {
                let input = turns(&r["turns"]);
                let got = Prepared::new(&input);
                assert_eq!(got.fingerprints, strings(&r["fps"]), "{}", r["m"]);
                assert_eq!(got.min_prefix, r["min"].as_u64().unwrap() as usize);
                if r["op"] == "prepare_ext" {
                    assert_eq!(got.tails, strings(&r["tails"]));
                    assert_eq!(got.environment, r["env"].as_str().unwrap());
                }
            }
            _ => {}
        }
    }
    assert!(extracts >= 80 && fingerprints >= 20, "{extracts} {fingerprints}");
}

#[test]
fn matcher_replays_go_call_by_call() {
    // Mock clocks in Go's tests also move backwards; offsets are signed.
    let base = Instant::now() + Duration::from_secs(400 * 24 * 3600);
    let mut matchers: HashMap<String, (Matcher, String)> = HashMap::new();
    let mut tests = std::collections::BTreeSet::new();
    let mut calls = 0;
    for r in trace() {
        let op = r["op"].as_str().unwrap();
        if op == "new" {
            let test = r["test"].as_str().unwrap().to_owned();
            let matcher = Matcher::new(Limits {
                ttl: Duration::from_nanos(r["ttl"].as_u64().unwrap()),
                max_turns: r["max_turns"].as_u64().unwrap() as usize,
                max_groups: r["max_groups"].as_u64().unwrap() as usize,
                max_prefixes: r["max_prefixes"].as_u64().unwrap() as usize,
                ..Limits::default()
            });
            matchers.insert(r["m"].as_str().unwrap().to_owned(), (matcher, test));
            continue;
        }
        let Some(id) = r["m"].as_str() else { continue };
        let Some((m, test)) = matchers.get_mut(id) else {
            continue;
        };
        // These edit groups behind the public API (ported below through it). The
        // recorder leaves TestMerklePrefixMatcherConcurrentAccess out: its goroutines race.
        if [
            "TestMerklePrefixMatcher_LookupSession_ConflictingTrajectories",
            "TestMerklePrefixMatcher_LookupSession_PartialExpiration_ControllableClock",
        ]
        .iter()
        .any(|t| test.ends_with(t))
        {
            continue;
        }
        tests.insert(test.clone());
        calls += 1;
        let at = r["at"].as_i64().unwrap();
        let now = if at >= 0 {
            base + Duration::from_nanos(at as u64)
        } else {
            base - Duration::from_nanos(at.unsigned_abs())
        };
        let ns = r["ns"].as_str().unwrap_or_default();
        let auth = r["auth"].as_str().unwrap_or_default();
        let ctx = format!("{test} {op} #{calls}");
        match op {
            "match" => {
                let got = m.find(ns, &prepared(&r), now);
                let want = r["ok"].as_bool().unwrap().then(|| go_match(&r["out"]));
                assert_eq!(got, want, "{ctx}");
            }
            "bind" => {
                let got = m.bind(ns, &prepared(&r), auth, now);
                let out = &r["out"];
                let want = (!out["SessionID"].as_str().unwrap().is_empty()).then(|| go_match(out));
                assert_eq!(got, want, "{ctx}");
            }
            "touch" => {
                assert_eq!(
                    m.touch(ns, &prepared(&r), auth, now),
                    r["ok"].as_bool().unwrap(),
                    "{ctx}"
                );
            }
            "remove" => {
                let generation = r["gen"].as_u64().unwrap();
                let ok = m.remove(ns, &strings(&r["fps"]), auth, generation, now);
                assert_eq!(ok, r["ok"].as_bool().unwrap(), "{ctx}");
            }
            "invalidate" => m.invalidate(auth),
            "clear" => m.clear(),
            "lookup" => {
                let got = m.lookup(r["session"].as_str().unwrap(), now);
                let want = r["ok"]
                    .as_bool()
                    .unwrap()
                    .then(|| (strings(&r["auths"]), r["ns"].as_str().unwrap().to_owned()));
                assert_eq!(got, want, "{ctx}");
            }
            "prepare" | "prepare_ext" => {}
            other => panic!("unknown op {other}"),
        }
    }
    assert!(
        calls > 900 && tests.len() > 80,
        "{calls} calls over {} tests",
        tests.len()
    );
}

/// Go `TestMerklePrefixMatcherConfigBoundsSanitization` and the constructor defaults.
#[test]
fn limits_default_and_cover_max_turns() {
    let m = Matcher::new(Limits {
        max_turns: 10,
        max_prefixes: 2,
        ..Limits::default()
    });
    let l = m.limits();
    assert_eq!(
        (l.ttl, l.max_turns, l.max_groups, l.max_prefixes),
        (Duration::from_secs(3600), 10, 4096, 10)
    );
    assert_eq!(
        (l.max_bytes, l.max_group_bytes),
        (DEFAULT_MAX_BYTES, DEFAULT_MAX_GROUP_BYTES)
    );
}

/// Go `TestNormalizeCanonicalTurnToolPartsDigestTieBreak`.
#[test]
fn tool_parts_sort_by_value_then_digest() {
    let part = |digest: &str| Part {
        kind: "tool:call".into(),
        value: b"same_value".to_vec(),
        digest: digest.into(),
        ..Part::default()
    };
    let one = Turn {
        role: "assistant".into(),
        parts: vec![part("digest_b"), part("digest_a")],
    };
    let two = Turn {
        role: "assistant".into(),
        parts: vec![part("digest_a"), part("digest_b")],
    };
    assert_eq!(normalize_turn(&one).parts[0].digest, "digest_a");
    assert_eq!(normalize_turn(&two).parts[0].digest, "digest_a");
    assert_eq!(fingerprint(&one), fingerprint(&two));
}

fn texts(values: &[&str]) -> Prepared {
    let turns: Vec<Turn> = values
        .iter()
        .map(|v| Turn {
            role: "user".into(),
            parts: vec![Part {
                kind: "text".into(),
                value: v.as_bytes().to_vec(),
                ..Part::default()
            }],
        })
        .collect();
    Prepared::new(&turns)
}

/// Go `TestMerklePrefixMatcher_LookupSession_ConflictingTrajectories`: one session bound
/// to two credentials reports both, sorted.
#[test]
fn lookup_reports_every_credential_of_a_session() {
    let now = Instant::now();
    let ns = "lcp:v1::openai::gpt-4o::caller";
    let mut m = Matcher::new(Limits {
        ttl: Duration::from_secs(600),
        ..Limits::default()
    });
    let first = m.bind(ns, &texts(&["turnA"]), "auth-b", now).unwrap();
    // The extension inherits the session; a failover binds it to another credential.
    let grown = m.bind(ns, &texts(&["turnA", "turnB"]), "auth-a", now).unwrap();
    assert_eq!(grown.session, first.session);
    assert_eq!(
        m.lookup(&first.session, now),
        Some((vec!["auth-a".to_owned(), "auth-b".to_owned()], ns.to_owned()))
    );
}

/// Go `TestMerklePrefixMatcher_LookupSession_PartialExpiration_ControllableClock`:
/// lookup drops expired groups, keeps live ones untouched and refreshes nothing.
#[test]
fn lookup_drops_expired_groups_without_refreshing() {
    let t0 = Instant::now();
    let minutes = |n: u64| t0 + Duration::from_secs(60 * n);
    let ns = "lcp:v1::openai::gpt-4o::caller";
    let mut m = Matcher::new(Limits {
        ttl: Duration::from_secs(600),
        ..Limits::default()
    });
    let session = m.bind(ns, &texts(&["turn1"]), "auth-a", t0).unwrap().session;
    // Both groups live until minute 15; matching the longer one keeps only it alive.
    m.bind(ns, &texts(&["turn1", "turn2"]), "auth-a", minutes(5)).unwrap();
    let hit = m.find(ns, &texts(&["turn1", "turn2", "turn3"]), minutes(12)).unwrap();
    assert_eq!((hit.prefix_length, hit.fork), (2, false));
    assert_eq!(
        m.lookup(&session, minutes(16)),
        Some((vec!["auth-a".to_owned()], ns.to_owned()))
    );
    assert!(
        m.find(ns, &texts(&["turn1"]), minutes(16)).is_some(),
        "the extension keeps the prefix"
    );
    let generation = m.find(ns, &texts(&["turn1", "turn2"]), minutes(17)).unwrap().access;
    m.lookup(&session, minutes(18));
    assert_eq!(
        m.find(ns, &texts(&["turn1", "turn2"]), minutes(19)).unwrap().access,
        generation + 1
    );
    assert_eq!(m.lookup(&session, minutes(30)), None, "lookup never refreshed it");
}

/// A Gemini turn nested far deeper than any conversation skips the matcher instead of
/// recursing once per level, on a stack the size of an async worker's (2 MiB).
#[test]
fn deeply_nested_turns_skip_the_matcher_without_overflowing() {
    let deep = |levels: usize| {
        format!(
            r#"{{"contents":[{{"role":"user","parts":{}{{"text":"hi"}}{}}}]}}"#,
            "[".repeat(levels),
            "]".repeat(levels)
        )
    };
    let object = |levels: usize| {
        format!(
            r#"{{"contents":[{{"role":"user","parts":[{{"functionCall":{}1{}}}]}}]}}"#,
            r#"{"a":"#.repeat(levels),
            "}".repeat(levels)
        )
    };
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            assert!(Request::new("gemini", deep(200_000).as_bytes(), "client-key").is_none());
            assert!(Request::new("gemini", object(3_000).as_bytes(), "client-key").is_none());
            // At the ceiling the request is still matched.
            assert!(Request::new("gemini", deep(MAX_NESTING - 4).as_bytes(), "client-key").is_some());
            assert!(Request::new("gemini", deep(MAX_NESTING - 3).as_bytes(), "client-key").is_none());
        })
        .unwrap()
        .join()
        .unwrap();
}

/// Byte bound: exceeding the total cap evicts the oldest groups, keeping the most recent
/// conversations matched. Each single-turn group holds a 64-byte fingerprint, its 64-byte
/// tail and a 66-byte prefix key (~194 bytes), so a 500-byte cap keeps two of eight.
#[test]
fn total_byte_cap_evicts_oldest_groups() {
    let now = Instant::now();
    let ns = "lcp:v1::openai::gpt-4o::caller";
    let mut m = Matcher::new(Limits {
        ttl: Duration::from_secs(600),
        max_bytes: 500,
        ..Limits::default()
    });
    let mut sessions = Vec::new();
    for t in ["a", "b", "c", "d", "e", "f", "g", "h"] {
        sessions.push(m.bind(ns, &texts(&[t]), "auth-a", now).unwrap().session);
    }
    assert!(m.byte_count <= 500, "byte_count {} over the cap", m.byte_count);
    assert!(m.groups.len() < 8, "no group was evicted");
    assert_eq!(m.lookup(&sessions[0], now), None, "the oldest group is evicted");
    assert!(
        m.lookup(sessions.last().unwrap(), now).is_some(),
        "the newest group survives"
    );
}

/// Byte bound: a single group over the per-group cap is dropped rather than retained.
#[test]
fn per_group_byte_cap_drops_the_oversized_group() {
    let now = Instant::now();
    let ns = "lcp:v1::openai::gpt-4o::caller";
    // A 100-byte per-group cap is below one single-turn group (~194 bytes).
    let mut m = Matcher::new(Limits {
        ttl: Duration::from_secs(600),
        max_group_bytes: 100,
        ..Limits::default()
    });
    let bound = m.bind(ns, &texts(&["a"]), "auth-a", now);
    assert!(bound.is_some(), "bind still reports the session identity");
    assert_eq!(m.groups.len(), 0, "an oversized group is not retained");
    assert_eq!(m.byte_count, 0);
    assert_eq!(m.find(ns, &texts(&["a"]), now), None, "nothing is remembered");
}

/// Byte bound: growing an existing group's environment re-checks the cap on the update
/// path and evicts the oldest group.
#[test]
fn updating_an_existing_group_rechecks_the_byte_cap() {
    let now = Instant::now();
    let ns = "lcp:v1::openai::gpt-4o::caller";
    let mut m = Matcher::new(Limits {
        ttl: Duration::from_secs(600),
        max_bytes: usize::MAX,
        ..Limits::default()
    });
    // One group's byte size, measured rather than assumed.
    m.bind(ns, &texts(&["a"]), "auth-a", now).unwrap();
    let unit = m.byte_count;
    assert!(unit > 0);
    // Cap at two groups; a third group's worth of bytes must evict the oldest.
    m.reset(Limits {
        ttl: Duration::from_secs(600),
        max_bytes: 2 * unit + 10,
        ..Limits::default()
    });
    let first = m.bind(ns, &texts(&["a"]), "auth-a", now).unwrap().session;
    m.bind(ns, &texts(&["b"]), "auth-a", now).unwrap();
    assert_eq!(m.byte_count, 2 * unit);
    // Grow the newest group's environment by one group's worth of bytes.
    let mut grown = texts(&["b"]);
    grown.environment = "x".repeat(unit);
    assert!(m.touch(ns, &grown, "auth-a", now));
    assert!(
        m.byte_count <= 2 * unit + 10,
        "byte_count {} over the cap",
        m.byte_count
    );
    assert!(m.groups.len() < 2, "the update did not re-check the cap");
    assert_eq!(m.lookup(&first, now), None, "the oldest group is evicted");
}

/// Under the cap nothing is evicted, so matching is unchanged.
#[test]
fn under_the_byte_cap_keeps_every_group() {
    let now = Instant::now();
    let ns = "lcp:v1::openai::gpt-4o::caller";
    let mut m = Matcher::new(Limits {
        ttl: Duration::from_secs(600),
        ..Limits::default()
    });
    let mut sessions = Vec::new();
    for t in ["a", "b", "c", "d", "e"] {
        sessions.push(m.bind(ns, &texts(&[t]), "auth-a", now).unwrap().session);
    }
    assert_eq!(m.groups.len(), 5, "no group is dropped under the cap");
    assert!(m.byte_count > 0 && m.byte_count < m.limits.max_bytes);
    for (t, session) in ["a", "b", "c", "d", "e"].into_iter().zip(&sessions) {
        let hit = m.find(ns, &texts(&[t]), now).expect("still matched");
        assert_eq!(hit.prefix_length, 1, "{t}");
        assert!(m.lookup(session, now).is_some(), "{t}");
    }
}
