//! Go `encoding/json` for the wire contracts shared between the plugin host and the
//! server's built-in helpers.
//!
//! Encoding follows `json.Marshal`: fields in declaration order, `omitempty`,
//! HTML-escaped strings, `[]byte` as padded standard base64, maps with sorted keys,
//! `time.Time` as RFC 3339 with nanoseconds. Decoding follows `json.Unmarshal`: a key
//! matches its field exactly or, failing that, case-insensitively; unknown keys are
//! ignored; the last duplicate wins; `null` leaves scalars alone and clears slices and
//! maps; numbers into integer fields must be integer literals.
//!
//! Decoding walks a [`Node`] tree that keeps every object member and number literal,
//! and follows Go's decode-into-existing rules: struct fields and map keys merge into
//! what is already there (a map element itself is decoded fresh), a pointer decodes
//! into its pointee, a slice reuses its elements.
//!
//! Structs are declared with [`go_struct!`](crate::go_struct). `[]byte` is
//! [`bytes::Bytes`]; `map[string]any` and `any` are [`serde_json::Value`].
//!
//! ponytail: an empty slice, map or `[]byte` encodes as `null` (Go's nil). Fields Go
//! builds non-nil while empty use [`NonNil`].

#[macro_use]
mod struct_macro;

use std::fmt;

mod node;
mod quota;
mod scalars;

pub use node::{Node, parse, parse_first};
pub use quota::*;
pub use scalars::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

/// A value with Go `encoding/json` semantics.
pub trait GoJson: Sized + Default {
    /// Go type name, for error messages.
    const GO_TYPE: &'static str;
    /// The Go type as decode errors spell it; containers name their element type
    /// (`[]string`, `map[string]interface {}`).
    fn go_type() -> String {
        Self::GO_TYPE.to_owned()
    }
    /// Appends `json.Marshal` output.
    fn encode(&self, out: &mut Vec<u8>);
    /// `json.Unmarshal` into a zero value.
    fn decode(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        let mut out = Self::default();
        out.decode_into(v)?;
        Ok(out)
    }
    /// `json.Unmarshal` into an existing value. `null` resets slices, maps, pointers and
    /// `any`, and leaves everything else alone.
    fn decode_into(&mut self, v: &Node) -> Result<(), crate::gojson::DecodeError> {
        if v.is_null() {
            if Self::NULL_CLEARS {
                *self = Self::default();
            }
            return Ok(());
        }
        *self = Self::decode_value(v)?;
        Ok(())
    }
    /// Decodes a non-null value from scratch. Implement this or [`Self::decode_into`].
    fn decode_value(v: &Node) -> Result<Self, crate::gojson::DecodeError> {
        let mut out = Self::default();
        out.decode_into(v)?;
        Ok(out)
    }
    /// Go `omitempty`: false, 0, "", nil or empty slices and maps, nil pointers. Structs
    /// (including `time.Time`) are never empty.
    fn is_empty(&self) -> bool;
    /// Whether `null` resets the field (slices, maps, pointers, `any`); scalars and
    /// structs keep their value.
    const NULL_CLEARS: bool = false;
}

/// `json.Marshal`.
pub fn to_vec<T: GoJson>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode(&mut out);
    out
}

/// `json.Unmarshal` of a complete document.
pub fn from_slice<T: GoJson>(raw: &[u8]) -> Result<T, crate::gojson::DecodeError> {
    T::decode(&parse(raw)?)
}

/// `json.NewDecoder(bytes.NewReader(raw)).Decode`: the first value; see [`parse_first`].
pub fn from_slice_first<T: GoJson>(raw: &[u8]) -> Result<T, crate::gojson::DecodeError> {
    T::decode(&parse_first(raw)?)
}

fn kind(v: &Node) -> &'static str {
    match v {
        Node::Null => "null",
        Node::Bool(_) => "bool",
        Node::Number(_) => "number",
        Node::String(_) => "string",
        Node::Array(_) => "array",
        Node::Object(_) => "object",
    }
}

/// A value of the wrong JSON kind for `go_type`. Go names a number without its
/// literal here; [`number_error`] is the numeric-range case that includes it.
pub fn type_error(v: &Node, go_type: &str) -> crate::gojson::DecodeError {
    let what = match v {
        Node::Number(_) => "number",
        other => kind(other),
    };
    crate::gojson::DecodeError(format!("json: cannot unmarshal {what} into Go value of type {go_type}"))
}

/// A number that does not fit the numeric `go_type` (Go `"number " + literal`).
pub fn number_error(literal: &str, go_type: &str) -> crate::gojson::DecodeError {
    crate::gojson::DecodeError(format!(
        "json: cannot unmarshal number {literal} into Go value of type {go_type}"
    ))
}

/// Adds the struct field to a decode error, like Go's `UnmarshalTypeError`: the
/// innermost struct's name and the field path from the outermost struct (array
/// indexes and map keys are not part of it).
pub fn in_field(err: crate::gojson::DecodeError, strukt: &str, field: &str) -> crate::gojson::DecodeError {
    if let Some((head, ty)) = err.0.split_once(" into Go value of type ") {
        return crate::gojson::DecodeError(format!("{head} into Go struct field {strukt}.{field} of type {ty}"));
    }
    // A field of a nested struct: this field goes in front of its path.
    if let Some((head, rest)) = err.0.split_once(" into Go struct field ")
        && let Some((inner, path)) = rest.split_once('.')
    {
        return crate::gojson::DecodeError(format!("{head} into Go struct field {inner}.{field}.{path}"));
    }
    err
}

/// Writes one JSON object field by field.
pub struct ObjWriter<'a> {
    out: &'a mut Vec<u8>,
    first: bool,
}

impl<'a> ObjWriter<'a> {
    pub fn begin(out: &'a mut Vec<u8>) -> Self {
        out.push(b'{');
        Self { out, first: true }
    }

    fn key(&mut self, name: &str) {
        if !self.first {
            self.out.push(b',');
        }
        self.first = false;
        crate::json::marshal_str(self.out, name.as_bytes(), true);
        self.out.push(b':');
    }

    pub fn field<T: GoJson>(&mut self, name: &str, v: &T, omitempty: bool) {
        if omitempty && v.is_empty() {
            return;
        }
        self.key(name);
        v.encode(self.out);
    }

    /// Fields of an embedded struct, flattened like Go embedding.
    pub fn embed<T: GoStruct>(&mut self, v: &T) {
        let mut inner = ObjWriter {
            out: &mut *self.out,
            first: self.first,
        };
        v.encode_fields(&mut inner);
        self.first = inner.first;
    }

    pub fn end(self) {
        self.out.push(b'}');
    }
}

/// A Go struct: encodes its fields in order and decodes them by name.
pub trait GoStruct: GoJson {
    fn encode_fields(&self, w: &mut ObjWriter<'_>);
    /// Decodes `v` into the field named `key` (exact match first, then
    /// case-insensitive). `None` when no field matches.
    fn decode_field(&mut self, key: &str, v: &Node) -> Option<Result<(), crate::gojson::DecodeError>>;
}

/// Decodes an object into a fresh struct.
pub fn decode_struct<T: GoStruct>(v: &Node) -> Result<T, crate::gojson::DecodeError> {
    let mut out = T::default();
    decode_struct_into(&mut out, v)?;
    Ok(out)
}

/// Decodes an object member by member into an existing struct; later duplicates decode
/// over earlier ones.
pub fn decode_struct_into<T: GoStruct>(out: &mut T, v: &Node) -> Result<(), crate::gojson::DecodeError> {
    match v {
        Node::Null => Ok(()),
        Node::Object(members) => {
            // Go keeps decoding after a type mismatch and reports the first one.
            let mut first_err = None;
            for (key, value) in members {
                if let Some(Err(e)) = out.decode_field(key, value) {
                    first_err.get_or_insert(e);
                }
            }
            first_err.map_or(Ok(()), Err)
        }
        other => Err(type_error(other, &T::go_type())),
    }
}
