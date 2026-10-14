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
//! path that would append, a non-simple path, overlapping edits, a malformed body), or
//! where two edits could interact through an index shift, the helper falls back to
//! sequential application, so the guarantee holds for every input.
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

/// The single-pass path. `Some` only when no two edits can interact through an index
/// shift, every edit's range resolves against `body`, and the ranges are pairwise
/// disjoint; `None` tells the caller to apply sequentially.
pub(crate) fn try_splice(body: &[u8], edits: &[Edit]) -> Option<Vec<u8>> {
    if edits_have_shared_ancestor(body, edits) {
        return None;
    }
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

/// Whether two edits can interact through an index shift, so that resolving every edit
/// against the source body would diverge from sequential application.
///
/// The edits are order-dependent when their paths are equal, one path is an ancestor of
/// the other, or they share an ancestor: an earlier edit that deletes an array element or
/// rewrites an object member moves the node a later edit addresses. For the dotted paths
/// this helper accepts, "equal, an ancestor, or a shared ancestor" is exactly "the same
/// first path component", so a repeated first component forces the fallback. A body whose
/// root is an array is one index space, so any two edits on it interact as well.
fn edits_have_shared_ancestor(body: &[u8], edits: &[Edit]) -> bool {
    if edits.len() < 2 {
        return false;
    }
    if body.iter().copied().find(|&c| c > b' ') == Some(b'[') {
        return true;
    }
    let mut seen: Vec<&str> = Vec::with_capacity(edits.len());
    for edit in edits {
        let first = edit.path().split('.').next().unwrap_or("");
        if seen.contains(&first) {
            return true;
        }
        seen.push(first);
    }
    false
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
        // Distinct top-level subtrees, so no two paths share an ancestor and the single
        // pass is sound. Covers a nested object member and an array element.
        let body = br#"{"a":{"b":"x"},"m":[1,2,3]}"#;
        let edits = vec![Edit::set_str("a.b", "y"), Edit::set_raw("m.1", "7")];
        let want = sequential_ref(body, &edits);
        assert_eq!(want, br#"{"a":{"b":"y"},"m":[1,7,3]}"#);
        assert_eq!(try_splice(body, &edits).as_deref(), Some(want.as_slice()));
        assert_eq!(splice(body, &edits), want);
    }

    #[test]
    fn order_dependent_array_edits_fall_back_and_match_sequential() {
        // A delete shifts the array, so a later edit on the same array resolves to a
        // different node in the intermediate body. The source ranges stay disjoint, so the
        // byte-range check cannot see it; the shared-ancestor check must force the fallback.
        let body = br#"{"m":[1,2,3]}"#;
        let edits = vec![Edit::delete("m.0"), Edit::set_raw("m.1", "7")];
        assert!(try_splice(body, &edits).is_none());
        let want = sequential_ref(body, &edits);
        assert_eq!(want, br#"{"m":[2,7]}"#);
        assert_eq!(splice(body, &edits), want);

        let body = br#"{"a":1,"b":[1,2,3],"c":"s"}"#;
        let edits = vec![Edit::delete("b.1"), Edit::delete("b.2")];
        assert!(try_splice(body, &edits).is_none());
        let want = sequential_ref(body, &edits);
        assert_eq!(want, br#"{"a":1,"b":[1,3],"c":"s"}"#);
        assert_eq!(splice(body, &edits), want);

        // The nested case above with the array delete moved before the index set: the order
        // flip that the fixed order of the old test hid.
        let body = br#"{"a":{"b":"x","c":1},"m":[1,2,3]}"#;
        let edits = vec![
            Edit::delete("m.0"),
            Edit::set_raw("m.1", "7"),
            Edit::set_str("a.b", "y"),
            Edit::set_raw("a.c", "9"),
        ];
        assert!(try_splice(body, &edits).is_none());
        let want = sequential_ref(body, &edits);
        assert_eq!(want, br#"{"a":{"b":"y","c":9},"m":[2,7]}"#);
        assert_eq!(splice(body, &edits), want);
    }

    #[test]
    fn every_edit_order_matches_sequential() {
        // A body with an array and a nested object and a fixed edit multiset that deletes
        // and sets shared paths. Every permutation must stay byte-identical: a shared
        // ancestor forces the fallback, and the fallback applies edits in order.
        let body = br#"{"a":{"b":"x","c":1},"m":[1,2,3],"z":true}"#;
        let base = [
            Edit::delete("m.0"),
            Edit::set_raw("m.1", "7"),
            Edit::set_str("a.b", "y"),
            Edit::delete("m.2"),
            Edit::set_raw("z", "false"),
        ];
        let mut order: Vec<usize> = (0..base.len()).collect();
        permute(&mut order, 0, &mut |perm: &[usize]| {
            let edits: Vec<Edit> = perm.iter().map(|&i| base[i].clone()).collect();
            assert_eq!(splice(body, &edits), sequential_ref(body, &edits), "perm={perm:?}");
        });
    }

    fn permute(items: &mut [usize], k: usize, f: &mut impl FnMut(&[usize])) {
        if k == items.len() {
            f(items);
            return;
        }
        for i in k..items.len() {
            items.swap(k, i);
            permute(items, k + 1, f);
            items.swap(k, i);
        }
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
        for case in 0..4000 {
            // Bodies mix scalars, arrays and nested objects, so edit paths reach array
            // indices and a delete can shift a later index; edits are drawn and then the
            // order is randomly flipped.
            let mut body = b"{".to_vec();
            let mut paths: Vec<String> = Vec::new();
            let keys = rng.below(4) + 1;
            for i in 0..keys {
                if i > 0 {
                    body.push(b',');
                }
                let key = format!("k{i}");
                body.push(b'"');
                body.extend_from_slice(key.as_bytes());
                body.extend_from_slice(b"\":");
                match rng.below(3) {
                    0 => body.extend_from_slice(&body_value(&mut rng)),
                    1 => {
                        body.push(b'[');
                        for j in 0..rng.below(4) + 1 {
                            if j > 0 {
                                body.push(b',');
                            }
                            body.extend_from_slice(&body_value(&mut rng));
                            paths.push(format!("{key}.{j}"));
                        }
                        body.push(b']');
                    }
                    _ => {
                        body.push(b'{');
                        for j in 0..rng.below(3) + 1 {
                            if j > 0 {
                                body.push(b',');
                            }
                            let member = format!("s{j}");
                            body.push(b'"');
                            body.extend_from_slice(member.as_bytes());
                            body.extend_from_slice(b"\":");
                            body.extend_from_slice(&body_value(&mut rng));
                            paths.push(format!("{key}.{member}"));
                        }
                        body.push(b'}');
                    }
                }
                paths.push(key);
            }
            body.push(b'}');

            let mut edits = Vec::new();
            for _ in 0..rng.below(6) + 1 {
                let path = paths[rng.below(paths.len())].clone();
                edits.push(match rng.below(3) {
                    0 => Edit::set_raw(path, body_value(&mut rng)),
                    1 => Edit::set_str(path, edit_text(&mut rng)),
                    _ => Edit::delete(path),
                });
            }
            if rng.below(2) == 0 {
                edits.reverse();
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
