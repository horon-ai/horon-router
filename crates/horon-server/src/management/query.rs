//! Query-string parsing shared by the management routes (Go `url.ParseQuery`).

/// Go `url.ParseQuery` as `URL.Query()` uses it: pairs in order; a pair with a raw
/// `;` or a bad percent escape is dropped, the rest kept; `+` is a space.
pub(crate) fn go_query(raw: &str) -> Vec<(String, String)> {
    fn unescape(s: &str) -> Option<String> {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'%' => {
                    let hex = b.get(i + 1..i + 3)?;
                    out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                    i += 3;
                }
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }
    raw.split('&')
        .filter(|pair| !pair.is_empty() && !pair.contains(';'))
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            Some((unescape(k)?, unescape(v)?))
        })
        .collect()
}
