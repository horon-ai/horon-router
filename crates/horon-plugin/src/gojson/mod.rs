//! The plugin host's Go `encoding/json` layer.
//!
//! The Go JSON value tree, its error type, the `GoJson` trait, the scalar and container
//! codecs and the struct layer ([`GoStruct`], [`ObjWriter`], [`go_struct!`]) live in
//! [`horon_common::gojson`], shared with the server's built-in helpers. This module
//! re-exports them so `horon_plugin::gojson::…` keeps naming them.

pub use horon_common::gojson::*;

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use horon_common::go_struct;

    go_struct! {
        pub struct Sample("pluginapi.Sample") {
            "AuthID" => auth_id: String,
            "Body" => body: Bytes,
            "Headers" => headers: Header,
            "Count" => count: i64,
            "path" omitempty => path: String,
            "Meta" => meta: Metadata,
            "At" => at: GoTime,
            "Items" omitempty => items: Vec<String>,
        }
    }

    #[test]
    fn encodes_like_json_marshal() {
        let mut headers = Header::new();
        headers.insert("X-B".into(), vec!["2".into()]);
        headers.insert("A".into(), vec!["<1>".into()]);
        let s = Sample {
            auth_id: "a&b".into(),
            body: Bytes::from_static(b"hi"),
            headers,
            count: 7,
            path: String::new(),
            meta: [
                ("z".to_owned(), serde_json::json!(1)),
                ("a".to_owned(), serde_json::json!(0.5)),
            ]
            .into(),
            at: GoTime::default(),
            items: vec![],
        };
        assert_eq!(
            String::from_utf8(to_vec(&s)).unwrap(),
            r#"{"AuthID":"a\u0026b","Body":"aGk=","Headers":{"A":["\u003c1\u003e"],"X-B":["2"]},"Count":7,"Meta":{"a":0.5,"z":1},"At":"0001-01-01T00:00:00Z"}"#
        );
    }

    /// Go matches keys exactly first, then case-insensitively; unknown keys are ignored;
    /// integer fields reject non-integer literals.
    #[test]
    fn decodes_like_json_unmarshal() {
        let s: Sample = from_slice(br#"{"authid":"x","AuthID":"y","body":"aGk=","unknown":1,"count":3,"PATH":"/p","at":"2026-01-02T03:04:05.5+02:00"}"#).unwrap();
        assert_eq!(s.auth_id, "y");
        assert_eq!(s.body, Bytes::from_static(b"hi"));
        assert_eq!(s.count, 3);
        assert_eq!(s.path, "/p");
        assert_eq!(s.at.rfc3339_nano(), "2026-01-02T03:04:05.5+02:00");
        let err = from_slice::<Sample>(br#"{"Count":1.5}"#).unwrap_err();
        assert!(err.0.contains("Go struct field Sample.Count of type int64"), "{err}");
        assert!(from_slice::<Sample>(br#"{"Body":"!!"}"#).is_err());
        let s: Sample = from_slice(br#"{"Body":[104,105],"Count":null}"#).unwrap();
        assert_eq!(s.body, Bytes::from_static(b"hi"));
    }

    /// Go decodes member by member into the existing value: a second spelling of a map
    /// field merges keys; a later struct member decodes over the earlier one field by
    /// field; base64 accepts non-zero trailing bits; a null byte element is 0.
    #[test]
    fn decodes_into_existing_values_like_go() {
        go_struct! {
            pub struct Resp("pluginapi.ManagementResponse") {
                "Headers" => headers: Header,
                "Body" => body: Bytes,
                "Inner" => inner: Option<Sample>,
            }
        }
        let r: Resp = from_slice(
            br#"{"Headers":{"A":["1"]},"headers":{"B":["2"]},"Body":"Zh==","Inner":{"Count":1},"inner":{"Path":"/p"}}"#,
        )
        .unwrap();
        assert_eq!(r.headers.keys().collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(r.body, Bytes::from_static(b"f"));
        let inner = r.inner.unwrap();
        assert_eq!((inner.count, inner.path.as_str()), (1, "/p"));
        let r: Resp = from_slice(br#"{"Body":[null,65]}"#).unwrap();
        assert_eq!(r.body, Bytes::from_static(&[0, 65]));
        assert!(from_slice::<Resp>(br#"{"Body":[256]}"#).is_err());
        assert!(from_slice::<Resp>(br#"{"Headers":{"a":["x"]},"Body":1e400}"#).is_err());
        // An unknown member never needs to fit a Go type.
        assert!(from_slice::<Resp>(br#"{"unused":1e400}"#).is_ok());
    }

    #[test]
    fn non_nil_and_option_encode_empty_values() {
        assert_eq!(to_vec(&NonNil(Vec::<String>::new())), b"[]");
        assert_eq!(to_vec(&None::<Vec<String>>), b"null");
        assert_eq!(to_vec(&Vec::<String>::new()), b"null");
    }
}
