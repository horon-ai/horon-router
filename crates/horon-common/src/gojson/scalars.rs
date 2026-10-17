use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, FixedOffset, Timelike};
use serde_json::Value;

use base64::Engine as _;
use bytes::Bytes;

use super::node;
use super::{GoJson, Node, in_field, number_error, type_error};

/// Decodes one field into its current value.
pub fn decode_field_value<T: GoJson>(
    slot: &mut T,
    v: &Node,
    strukt: &str,
    field: &str,
) -> Result<(), crate::gojson::DecodeError> {
    slot.decode_into(v)
        .map_err(|e| in_field(e, strukt.rsplit('.').next().unwrap_or(strukt), field))
}

// ----------------------------------------------------------------------------------------
// Scalars

impl GoJson for String {
    const GO_TYPE: &'static str = "string";
    fn encode(&self, out: &mut Vec<u8>) {
        crate::json::marshal_str(out, self.as_bytes(), true);
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        match v {
            Node::String(s) => Ok(s.clone()),
            other => Err(type_error(other, Self::GO_TYPE)),
        }
    }
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl GoJson for bool {
    const GO_TYPE: &'static str = "bool";
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(if *self { b"true" } else { b"false" });
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        match v {
            Node::Bool(b) => Ok(*b),
            other => Err(type_error(other, Self::GO_TYPE)),
        }
    }
    fn is_empty(&self) -> bool {
        !*self
    }
}

macro_rules! go_int {
    ($t:ty, $go:literal) => {
        impl GoJson for $t {
            const GO_TYPE: &'static str = $go;
            fn encode(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(self.to_string().as_bytes());
            }
            /// Go parses the literal with `strconv.ParseInt`/`ParseUint`: `1.0`, `1e2` and
            /// out-of-range values are errors.
            fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
                match v {
                    Node::Number(n) => n.parse::<$t>().map_err(|_| number_error(n, Self::GO_TYPE)),
                    other => Err(type_error(other, Self::GO_TYPE)),
                }
            }
            fn is_empty(&self) -> bool {
                *self == 0
            }
        }
    };
}

// Go `int` has the platform's pointer width, like `isize`.
go_int!(isize, "int");
go_int!(i64, "int64");
go_int!(i32, "int32");
go_int!(u32, "uint32");
go_int!(u64, "uint64");

impl GoJson for f64 {
    const GO_TYPE: &'static str = "float64";
    fn encode(&self, out: &mut Vec<u8>) {
        // json.Marshal rejects NaN and Inf; the host never produces them.
        out.extend_from_slice(crate::json::json_float(*self).unwrap_or_else(|| "0".into()).as_bytes());
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        match v {
            Node::Number(n) => node::parse_f64(n),
            other => Err(type_error(other, Self::GO_TYPE)),
        }
    }
    fn is_empty(&self) -> bool {
        *self == 0.0
    }
}

/// `encoding/base64.StdEncoding.Decode` as `encoding/json` uses it: padding required,
/// CR and LF skipped anywhere, non-zero trailing bits accepted. The error is Go's
/// `CorruptInputError`, naming the same input offset (`decodeQuantum`).
fn decode_base64(s: &str) -> Result<Bytes, crate::gojson::DecodeError> {
    let src = s.as_bytes();
    let corrupt = |at: usize| crate::gojson::DecodeError(format!("illegal base64 data at input byte {at}"));
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let newline = |c: u8| c == b'\n' || c == b'\r';
    let mut out = Vec::with_capacity(src.len() / 4 * 3);
    let mut si = 0;
    while si < src.len() {
        let mut quantum = [0u8; 4];
        let mut len = 4;
        let mut trailing = None;
        let mut j = 0;
        while j < 4 {
            if si == src.len() {
                if j == 0 {
                    return Ok(out.into());
                }
                return Err(corrupt(si - j));
            }
            let c = src[si];
            si += 1;
            if let Some(v) = value(c) {
                quantum[j] = v;
                j += 1;
                continue;
            }
            if newline(c) {
                continue;
            }
            if c != b'=' || j < 2 {
                return Err(corrupt(si - 1));
            }
            if j == 2 {
                // "==" is expected; the first `=` is consumed.
                while si < src.len() && newline(src[si]) {
                    si += 1;
                }
                if si == src.len() {
                    return Err(corrupt(src.len()));
                }
                if src[si] != b'=' {
                    return Err(corrupt(si - 1));
                }
                si += 1;
            }
            while si < src.len() && newline(src[si]) {
                si += 1;
            }
            if si < src.len() {
                trailing = Some(si);
            }
            len = j;
            break;
        }
        let v = (u32::from(quantum[0]) << 18)
            | (u32::from(quantum[1]) << 12)
            | (u32::from(quantum[2]) << 6)
            | u32::from(quantum[3]);
        out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8][..len - 1]);
        if let Some(at) = trailing {
            return Err(corrupt(at));
        }
    }
    Ok(out.into())
}

/// `[]byte`: a base64 string, or a JSON array of byte values (a `null` element is 0).
fn decode_byte_slice(v: &Node) -> Result<Bytes, crate::gojson::DecodeError> {
    match v {
        Node::String(s) => decode_base64(s),
        Node::Array(items) => items
            .iter()
            .map(|item| match item {
                Node::Null => Ok(0),
                Node::Number(n) => n.parse::<u8>().map_err(|_| number_error(n, "uint8")),
                other => Err(type_error(other, "uint8")),
            })
            .collect::<Result<Vec<u8>, _>>()
            .map(Bytes::from),
        other => Err(type_error(other, "[]uint8")),
    }
}

/// `[]byte`: standard padded base64; empty encodes as `null` (Go's nil).
impl GoJson for Bytes {
    const GO_TYPE: &'static str = "[]uint8";
    const NULL_CLEARS: bool = true;
    fn encode(&self, out: &mut Vec<u8>) {
        if self.is_empty() {
            out.extend_from_slice(b"null");
            return;
        }
        out.push(b'"');
        out.extend_from_slice(base64::engine::general_purpose::STANDARD.encode(self).as_bytes());
        out.push(b'"');
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        decode_byte_slice(v)
    }
    fn is_empty(&self) -> bool {
        Bytes::is_empty(self)
    }
}

/// A `[]byte` Go builds non-nil (for example from `io.ReadAll`): encodes `""` when
/// empty instead of `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NonNilBytes(pub Bytes);

impl GoJson for NonNilBytes {
    const GO_TYPE: &'static str = "[]uint8";
    const NULL_CLEARS: bool = true;
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(b'"');
        out.extend_from_slice(base64::engine::general_purpose::STANDARD.encode(&self.0).as_bytes());
        out.push(b'"');
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        decode_byte_slice(v).map(NonNilBytes)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `any` / `map[string]any` values: Go decodes numbers to float64 and encodes maps with
/// sorted keys.
impl GoJson for Value {
    const GO_TYPE: &'static str = "interface {}";
    const NULL_CLEARS: bool = true;
    fn encode(&self, out: &mut Vec<u8>) {
        encode_any(self, out);
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        v.to_value()
    }
    fn is_empty(&self) -> bool {
        self.is_null()
    }
}

/// Go's encoding of a value decoded into `any`.
pub fn encode_any(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => b.encode(out),
        Value::Number(n) => match n.as_f64() {
            Some(f) => f.encode(out),
            None => out.extend_from_slice(n.to_string().as_bytes()),
        },
        Value::String(s) => crate::json::marshal_str(out, s.as_bytes(), true),
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                encode_any(item, out);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.push(b'{');
            for (i, (key, item)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                crate::json::marshal_str(out, key.as_bytes(), true);
                out.push(b':');
                encode_any(item, out);
            }
            out.push(b'}');
        }
    }
}

// ----------------------------------------------------------------------------------------
// Containers

/// A slice: decoding reuses existing elements and truncates to the array's length.
impl<T: GoJson> GoJson for Vec<T> {
    const GO_TYPE: &'static str = "slice";
    const NULL_CLEARS: bool = true;
    fn go_type() -> String {
        format!("[]{}", T::go_type())
    }
    fn encode(&self, out: &mut Vec<u8>) {
        if self.is_empty() {
            out.extend_from_slice(b"null");
            return;
        }
        encode_items(self, out);
    }
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        match v {
            Node::Null => {
                self.clear();
                Ok(())
            }
            Node::Array(items) => {
                let mut first_err = None;
                for (i, item) in items.iter().enumerate() {
                    if i == self.len() {
                        self.push(T::default());
                    }
                    if let Err(e) = self[i].decode_into(item) {
                        first_err.get_or_insert(e);
                    }
                }
                self.truncate(items.len());
                first_err.map_or(Ok(()), Err)
            }
            other => Err(type_error(other, &Self::go_type())),
        }
    }
    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }
}

fn encode_items<T: GoJson>(items: &[T], out: &mut Vec<u8>) {
    out.push(b'[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        item.encode(out);
    }
    out.push(b']');
}

/// A pointer: `null` makes it nil; a value decodes into the existing pointee.
impl<T: GoJson> GoJson for Option<T> {
    const GO_TYPE: &'static str = T::GO_TYPE;
    const NULL_CLEARS: bool = true;
    fn go_type() -> String {
        T::go_type()
    }
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            None => out.extend_from_slice(b"null"),
            Some(v) => v.encode(out),
        }
    }
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        if v.is_null() {
            *self = None;
            return Ok(());
        }
        self.get_or_insert_with(T::default).decode_into(v)
    }
    fn is_empty(&self) -> bool {
        self.is_none()
    }
}

/// A map: keys merge into the existing map; each element is decoded fresh.
impl<T: GoJson> GoJson for BTreeMap<String, T> {
    const GO_TYPE: &'static str = "map";
    const NULL_CLEARS: bool = true;
    fn go_type() -> String {
        format!("map[string]{}", T::go_type())
    }
    fn encode(&self, out: &mut Vec<u8>) {
        if self.is_empty() {
            out.extend_from_slice(b"null");
            return;
        }
        encode_map(self, out);
    }
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        match v {
            Node::Null => {
                self.clear();
                Ok(())
            }
            Node::Object(members) => {
                let mut first_err = None;
                for (k, item) in members {
                    match T::decode(item) {
                        Ok(value) => {
                            self.insert(k.clone(), value);
                        }
                        Err(e) => {
                            first_err.get_or_insert(e);
                        }
                    }
                }
                first_err.map_or(Ok(()), Err)
            }
            other => Err(type_error(other, &Self::go_type())),
        }
    }
    fn is_empty(&self) -> bool {
        BTreeMap::is_empty(self)
    }
}

fn encode_map<T: GoJson>(map: &BTreeMap<String, T>, out: &mut Vec<u8>) {
    out.push(b'{');
    for (i, (key, item)) in map.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        crate::json::marshal_str(out, key.as_bytes(), true);
        out.push(b':');
        item.encode(out);
    }
    out.push(b'}');
}

/// A slice or map Go builds non-nil even when empty: encodes `[]` / `{}`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NonNil<T>(pub T);

impl<T: GoJson> GoJson for NonNil<Vec<T>> {
    const GO_TYPE: &'static str = "slice";
    const NULL_CLEARS: bool = true;
    fn go_type() -> String {
        Vec::<T>::go_type()
    }
    fn encode(&self, out: &mut Vec<u8>) {
        encode_items(&self.0, out);
    }
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        self.0.decode_into(v)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: GoJson> GoJson for NonNil<BTreeMap<String, T>> {
    const GO_TYPE: &'static str = "map";
    const NULL_CLEARS: bool = true;
    fn go_type() -> String {
        BTreeMap::<String, T>::go_type()
    }
    fn encode(&self, out: &mut Vec<u8>) {
        encode_map(&self.0, out);
    }
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        self.0.decode_into(v)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `http.Header` and `url.Values`.
pub type Header = BTreeMap<String, Vec<String>>;
/// `map[string]any`.
pub type Metadata = BTreeMap<String, Value>;
/// `map[string]string`.
pub type StringMap = BTreeMap<String, String>;

// ----------------------------------------------------------------------------------------
// time.Time and json.RawMessage

/// `time.Time`; `None` is the zero time (`0001-01-01T00:00:00Z`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoTime(pub Option<DateTime<FixedOffset>>);

impl GoTime {
    pub fn now_utc() -> Self {
        Self(Some(chrono::Utc::now().fixed_offset()))
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_none()
    }

    /// `time.Format(time.RFC3339Nano)`.
    pub fn rfc3339_nano(&self) -> String {
        let Some(t) = self.0 else {
            return "0001-01-01T00:00:00Z".into();
        };
        let mut s = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            t.year(),
            t.month(),
            t.day(),
            t.hour(),
            t.minute(),
            t.second()
        );
        let nanos = t.nanosecond() % 1_000_000_000;
        if nanos != 0 {
            let frac = format!("{nanos:09}");
            s.push('.');
            s.push_str(frac.trim_end_matches('0'));
        }
        let offset = t.offset().local_minus_utc();
        if offset == 0 {
            s.push('Z');
        } else {
            let sign = if offset < 0 { '-' } else { '+' };
            let abs = offset.abs();
            s.push_str(&format!("{sign}{:02}:{:02}", abs / 3600, abs % 3600 / 60));
        }
        s
    }
}

impl GoJson for GoTime {
    const GO_TYPE: &'static str = "time.Time";
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(b'"');
        out.extend_from_slice(self.rfc3339_nano().as_bytes());
        out.push(b'"');
    }
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        match v {
            Node::String(s) => {
                let t = DateTime::parse_from_rfc3339(s)
                    .map_err(|e| crate::gojson::DecodeError(format!("parsing time {s:?} as RFC3339: {e}")))?;
                let zero = t.naive_utc()
                    == chrono::NaiveDate::from_ymd_opt(1, 1, 1)
                        .unwrap()
                        .and_hms_opt(0, 0, 0)
                        .unwrap();
                Ok(Self((!zero).then_some(t)))
            }
            other => Err(type_error(other, Self::GO_TYPE)),
        }
    }
    fn is_empty(&self) -> bool {
        false
    }
}

/// `json.RawMessage`: marshalled compact with HTML escaping; `null` when empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawJson(pub Bytes);

impl GoJson for RawJson {
    const GO_TYPE: &'static str = "json.RawMessage";
    const NULL_CLEARS: bool = true;
    fn encode(&self, out: &mut Vec<u8>) {
        if self.0.is_empty() {
            out.extend_from_slice(b"null");
        } else {
            out.extend_from_slice(&crate::json::compact(&self.0, true));
        }
    }
    /// The member's JSON text re-rendered compactly. ponytail: Go keeps the raw member
    /// text; whitespace and string escapes (`\u003c` vs `<`) can differ, the JSON value
    /// cannot. Keep source spans in `Node` if a byte-exact consumer appears.
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        self.0 = Bytes::from(v.to_json());
        Ok(())
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
