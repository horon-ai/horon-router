//! The built-in credential quota request body (Go `credentialQuotaRequest`).
//!
//! The quota wire schemas (`QuotaFetchResponse`, `QuotaBucket`, ...) live in
//! [`horon_common::gojson`], shared with the plugin host and the declarative quota probe.

use horon_common::gojson::{self as pjson, Node};

pub(crate) use horon_common::gojson::{
    QuotaBucket, QuotaFetchResponse, QuotaGroup, QuotaMetric, QuotaSubscription, quota_fetch_from_value,
};

/// Go `credentialQuotaRequest` as gin's `ShouldBindJSON` fills it: exact field names
/// first, then case-insensitive in struct order; `None` when binding fails.
#[derive(Default)]
pub(crate) struct QuotaBody {
    auth_index: [Option<String>; 3],
    pub(crate) plugin_id: String,
    pub(crate) provider: String,
}

impl QuotaBody {
    /// gin `ShouldBindJSON`: `None` when binding fails.
    pub(crate) fn decode(body: &[u8]) -> Option<Self> {
        let (out, ok) = Self::bind(body);
        ok.then_some(out)
    }

    /// `json.Decoder.Decode` into `credentialQuotaRequest`: the first JSON value only;
    /// exact field names first, then case-insensitive in struct order. A type error
    /// leaves that field as it was and decoding goes on (Go reports it at the end);
    /// a syntax error yields nothing.
    pub(crate) fn bind(body: &[u8]) -> (Self, bool) {
        const FIELDS: [&str; 5] = ["auth_index", "authIndex", "AuthIndex", "plugin_id", "provider"];
        let mut out = Self::default();
        let fields = match pjson::parse(first_value(body)) {
            Ok(mut node) => match &mut node {
                Node::Object(fields) => std::mem::take(fields),
                Node::Null => return (out, true),
                _ => return (out, false),
            },
            Err(_) => return (out, false),
        };
        let mut ok = true;
        for (key, mut v) in fields {
            let field = FIELDS
                .iter()
                .position(|f| *f == key)
                .or_else(|| FIELDS.iter().position(|f| f.eq_ignore_ascii_case(&key)));
            match (field, &mut v) {
                (Some(i @ 0..=2), Node::String(s)) => out.auth_index[i] = Some(std::mem::take(s)),
                (Some(i @ 0..=2), Node::Null) => out.auth_index[i] = None,
                (Some(3), Node::String(s)) => out.plugin_id = std::mem::take(s),
                (Some(4), Node::String(s)) => out.provider = std::mem::take(s),
                (Some(_), Node::Null) | (None, _) => {}
                (Some(_), _) => ok = false,
            }
        }
        (out, ok)
    }

    /// Go `resolveAuthIndex`.
    pub(crate) fn auth_index(&self) -> String {
        self.auth_index
            .iter()
            .flatten()
            .map(|s| s.trim())
            .find(|s| !s.is_empty())
            .unwrap_or_default()
            .to_owned()
    }
}

/// The bytes of the first JSON value (`json.Decoder` ignores what follows it).
fn first_value(body: &[u8]) -> &[u8] {
    let start = body.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(body.len());
    let rest = &body[start..];
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (i, &b) in rest.iter().enumerate() {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => {
                    in_string = false;
                    if depth == 0 {
                        return &rest[..=i];
                    }
                }
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return &rest[..=i];
                }
            }
            b' ' | b'\t' | b'\r' | b'\n' | b',' if depth == 0 => return &rest[..i],
            _ => {}
        }
    }
    rest
}

/// Go `json.Unmarshal` into `map[string]any`: an object (or null) whose numbers all
/// fit a float64.
pub(crate) fn go_object(data: &[u8]) -> bool {
    fn numbers_fit(n: &Node) -> bool {
        match n {
            Node::Number(lit) => horon_common::json::parse_decimal_float(lit).is_some(),
            Node::Array(items) => items.iter().all(numbers_fit),
            Node::Object(fields) => fields.iter().all(|(_, v)| numbers_fit(v)),
            _ => true,
        }
    }
    matches!(pjson::parse(data), Ok(n @ (Node::Object(_) | Node::Null)) if numbers_fit(&n))
}
