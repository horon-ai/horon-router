//! Single-pass offset-splice editing of a JSON request body.
//!
//! Request shaping in the Claude, Codex, xAI, [OI]-compat and Kimi paths applies dozens
//! of `sjson`-style edits (`set_raw`/`set_str`/`delete`) to one body. Applied one at a
//! time, each edit rebuilds the whole document; [`splice`] resolves every edit's byte
//! range against the source body once and writes the result in a single buffer pass.
//!
//! The output is byte-identical to applying the same edits in order with
//! [`crate::json::try_set_raw`], [`crate::json::try_set_str`] and
//! [`crate::json::try_delete`]. Where a range cannot be resolved against the source (a
//! path that would append, a non-simple path, overlapping edits, a malformed body) the
//! helper falls back to sequential application, so the guarantee holds for every input.
//!
//! This generalizes the offset edits already in the repo: `horon-exec`'s Claude
//! `signing::Scanner` (offset-based in-place edits) and `alias::apply_at_offsets`
//! (gjson offsets resolved to spans, then one splice).

use crate::json;

/// One edit in a [`splice`] batch, standing for the matching `sjson` call.
#[derive(Clone, Debug)]
pub enum Edit {
    /// `sjson.SetRawBytes`: replace the value at `path` with raw JSON bytes.
    SetRaw { path: String, raw: Vec<u8> },
    /// `sjson.SetBytes` with a Go string value.
    SetStr { path: String, value: Vec<u8> },
    /// `sjson.DeleteBytes`.
    Delete { path: String },
}

impl Edit {
    pub fn set_raw(path: impl Into<String>, raw: impl Into<Vec<u8>>) -> Self {
        Self::SetRaw {
            path: path.into(),
            raw: raw.into(),
        }
    }

    pub fn set_str(path: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        Self::SetStr {
            path: path.into(),
            value: value.into(),
        }
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self::Delete { path: path.into() }
    }

    fn path(&self) -> &str {
        match self {
            Edit::SetRaw { path, .. } | Edit::SetStr { path, .. } | Edit::Delete { path } => path,
        }
    }
}

/// Applies `edits` to `body`. The result is byte-identical to applying the same edits in
/// order with [`crate::json::try_set_raw`], [`crate::json::try_set_str`] and
/// [`crate::json::try_delete`] (an edit that errors leaves the body unchanged).
pub fn splice(body: &[u8], edits: &[Edit]) -> Vec<u8> {
    try_splice(body, edits).unwrap_or_else(|| sequential(body.to_vec(), edits))
}

/// The single-pass path. `Some` only when every edit's range resolves against `body`
/// and the ranges are pairwise disjoint; `None` tells the caller to apply sequentially.
pub(crate) fn try_splice(body: &[u8], edits: &[Edit]) -> Option<Vec<u8>> {
    let mut spans: Vec<Span> = Vec::with_capacity(edits.len());
    for edit in edits {
        let path = edit.path();
        if !spliceable_path(path.as_bytes()) {
            return None;
        }
        let res = json::get(body, path);
        let start = res.index;
        let end = start + res.raw().len();
        if !res.exists() || start == 0 || end <= start || end > body.len() {
            return None;
        }
        let (start, end, replacement) = match edit {
            Edit::SetRaw { raw, .. } => (start, end, raw.clone()),
            Edit::SetStr { value, .. } => (start, end, json::stringify(value)),
            Edit::Delete { .. } => {
                let (remove_start, remove_end) = json::delete_span(body, start, end);
                (remove_start, remove_end, Vec::new())
            }
        };
        spans.push(Span {
            start,
            end,
            replacement,
        });
    }
    spans.sort_by_key(|s| (s.start, s.end));
    let mut out = Vec::with_capacity(body.len());
    let mut cursor = 0;
    for span in &spans {
        if span.start < cursor || span.end > body.len() {
            return None;
        }
        out.extend_from_slice(&body[cursor..span.start]);
        out.extend_from_slice(&span.replacement);
        cursor = span.end;
    }
    out.extend_from_slice(&body[cursor..]);
    Some(out)
}

struct Span {
    start: usize,
    end: usize,
    replacement: Vec<u8>,
}

fn sequential(mut body: Vec<u8>, edits: &[Edit]) -> Vec<u8> {
    for edit in edits {
        match edit {
            Edit::SetRaw { path, raw } => {
                json::set_raw(&mut body, path.as_str(), raw);
            }
            Edit::SetStr { path, value } => {
                json::set_str(&mut body, path.as_str(), value);
            }
            Edit::Delete { path } => {
                json::delete(&mut body, path.as_str());
            }
        }
    }
    body
}

/// A path `json::get` and the `sjson` edit resolve to the same value: a non-empty dotted
/// path with no gjson metacharacter (wildcards, `#`, `|`, `@`, `:`, `\`). Any other path
/// is left to sequential application.
fn spliceable_path(path: &[u8]) -> bool {
    !path.is_empty()
        && !path
            .iter()
            .any(|&c| matches!(c, b'\\' | b'|' | b'#' | b'@' | b'*' | b'?' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequential_ref(body: &[u8], edits: &[Edit]) -> Vec<u8> {
        let mut out = body.to_vec();
        for edit in edits {
            let next = match edit {
                Edit::SetRaw { path, raw } => json::try_set_raw(&out, path.as_str(), raw),
                Edit::SetStr { path, value } => json::try_set_str(&out, path.as_str(), value),
                Edit::Delete { path } => json::try_delete(&out, path.as_str()),
            };
            if let Ok(next) = next {
                out = next;
            }
        }
        out
    }

    #[test]
    fn single_pass_matches_sequential_on_independent_edits() {
        let body = br#"{"model":"claude","max_tokens":100,"stream":true,"keep":1}"#;
        let edits = vec![
            Edit::set_str("model", "claude-3"),
            Edit::set_raw("max_tokens", "4096"),
            Edit::delete("stream"),
            Edit::set_raw("keep", "2"),
        ];
        let want = sequential_ref(body, &edits);
        assert_eq!(want, br#"{"model":"claude-3","max_tokens":4096,"keep":2}"#);
        assert_eq!(try_splice(body, &edits).as_deref(), Some(want.as_slice()));
        assert_eq!(splice(body, &edits), want);
    }

    #[test]
    fn nested_and_array_edits_match_sequential() {
        let body = br#"{"a":{"b":"x","c":1},"m":[1,2,3]}"#;
        let edits = vec![
            Edit::set_str("a.b", "y"),
            Edit::set_raw("a.c", "9"),
            Edit::set_raw("m.1", "7"),
            Edit::delete("m.0"),
        ];
        let want = sequential_ref(body, &edits);
        assert_eq!(try_splice(body, &edits).as_deref(), Some(want.as_slice()));
        assert_eq!(splice(body, &edits), want);
    }

    #[test]
    fn falls_back_and_still_matches_sequential() {
        let body = br#"{"a":1}"#;
        let edits = vec![Edit::set_str("missing", "x"), Edit::set_raw("a", "2")];
        assert!(try_splice(body, &edits).is_none());
        assert_eq!(splice(body, &edits), sequential_ref(body, &edits));

        let edits = vec![Edit::delete("a"), Edit::delete("b")];
        assert_eq!(
            splice(br#"{"a":1,"b":2}"#, &edits),
            sequential_ref(br#"{"a":1,"b":2}"#, &edits)
        );

        for body in [&b"not json"[..], b"", b"{\"a\":", b"{,}", b"  "] {
            let edits = vec![Edit::set_raw("a", "1"), Edit::delete("a"), Edit::set_str("b", "x")];
            assert!(try_splice(body, &edits).is_none(), "{body:?}");
            assert_eq!(splice(body, &edits), sequential_ref(body, &edits), "{body:?}");
        }
    }

    #[test]
    fn random_edit_lists_match_sequential() {
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        let (mut spliced, mut fell_back) = (0usize, 0usize);
        for case in 0..3000 {
            let keys: Vec<String> = (0..rng.below(8) + 1).map(|i| format!("k{i}")).collect();
            let mut body = b"{".to_vec();
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    body.push(b',');
                }
                body.push(b'"');
                body.extend_from_slice(key.as_bytes());
                body.extend_from_slice(b"\":");
                body.extend_from_slice(&body_value(&mut rng));
            }
            body.push(b'}');

            let mut edits = Vec::new();
            for _ in 0..rng.below(6) + 1 {
                let key = &keys[rng.below(keys.len())];
                edits.push(match rng.below(3) {
                    0 => Edit::set_raw(key, body_value(&mut rng)),
                    1 => Edit::set_str(key, edit_text(&mut rng)),
                    _ => Edit::delete(key),
                });
            }

            let want = sequential_ref(&body, &edits);
            assert_eq!(
                splice(&body, &edits),
                want,
                "case {case}: body={} edits={edits:?}",
                String::from_utf8_lossy(&body)
            );
            if try_splice(&body, &edits).is_some() {
                spliced += 1;
            } else {
                fell_back += 1;
            }
        }
        assert!(spliced > 0, "the single-pass path was never exercised");
        assert!(fell_back > 0, "the fallback path was never exercised");
    }

    fn body_value(rng: &mut Rng) -> Vec<u8> {
        match rng.below(5) {
            0 => rng.below(1000).to_string().into_bytes(),
            1 => b"true".to_vec(),
            2 => b"false".to_vec(),
            3 => b"null".to_vec(),
            _ => {
                let alphabet = b"abcxyz0123 ";
                let mut s = b"\"".to_vec();
                s.extend((0..rng.below(6)).map(|_| alphabet[rng.below(alphabet.len())]));
                s.push(b'"');
                s
            }
        }
    }

    fn edit_text(rng: &mut Rng) -> Vec<u8> {
        let alphabet = b"ab,\":\\{}<>&\x01\xc3\xa9 ";
        (0..rng.below(6)).map(|_| alphabet[rng.below(alphabet.len())]).collect()
    }

    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }
}
