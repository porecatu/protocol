// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed JSON <-> [`porecatu_protocol::cbor::Value`].
//!
//! JSON cannot tell a byte string from a text string, nor guarantee integer
//! precision, so the CBOR value of a vector is written as a typed JSON tree
//! (see `docs/especificacao/vetores-de-teste/README.md`):
//!
//! | CBOR | typed JSON |
//! |---|---|
//! | integer | JSON integer |
//! | text string | JSON string |
//! | byte string | `{"bstr": "<lowercase hex>"}` |
//! | array | JSON array |
//! | map | `{"map": [["key", value], ...]}` (pairs in canonical order) |
//! | `true`, `false`, `null` | the same JSON literals |

use std::fmt;

use porecatu_protocol::cbor::{CborError, Value};
use porecatu_protocol::encoding::{decode_hex, encode_hex};
use serde_json::{Map, Number, Value as Json};

/// Why a typed JSON tree could not be turned into a CBOR value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedJsonError {
    /// A JSON number that is not an integer within the CBOR domain.
    Number(String),
    /// `{"bstr": ...}` whose payload is not lowercase even-length hex.
    Hex(String),
    /// An object that is neither `{"bstr": ...}` nor `{"map": ...}`.
    Object(String),
    /// `{"map": ...}` that is not a list of `[text, value]` pairs.
    MapPairs(String),
}

impl fmt::Display for TypedJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypedJsonError::Number(n) => write!(f, "number {n} is not a valid CBOR integer"),
            TypedJsonError::Hex(h) => write!(f, "invalid lowercase hex: {h:?}"),
            TypedJsonError::Object(o) => {
                write!(f, "object must be {{\"bstr\": ..}} or {{\"map\": ..}}: {o}")
            }
            TypedJsonError::MapPairs(m) => write!(f, "invalid map pairs: {m}"),
        }
    }
}

impl std::error::Error for TypedJsonError {}

impl From<CborError> for TypedJsonError {
    fn from(e: CborError) -> Self {
        TypedJsonError::Number(e.to_string())
    }
}

/// Converts a typed JSON tree into a CBOR value.
pub fn to_value(json: &Json) -> Result<Value, TypedJsonError> {
    match json {
        Json::Null => Ok(Value::Null),
        Json::Bool(b) => Ok(Value::Bool(*b)),
        Json::Number(n) => {
            let int = n
                .as_i64()
                .ok_or_else(|| TypedJsonError::Number(n.to_string()))?;
            Ok(Value::int(int)?)
        }
        Json::String(s) => Ok(Value::Text(s.clone())),
        Json::Array(items) => items
            .iter()
            .map(to_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Json::Object(obj) => {
            if obj.len() == 1 {
                if let Some(hex) = obj.get("bstr") {
                    let text = hex
                        .as_str()
                        .ok_or_else(|| TypedJsonError::Hex(hex.to_string()))?;
                    let bytes =
                        decode_hex(text).map_err(|_| TypedJsonError::Hex(text.to_owned()))?;
                    return Ok(Value::Bytes(bytes));
                }
                if let Some(pairs) = obj.get("map") {
                    return map_from_pairs(pairs);
                }
            }
            Err(TypedJsonError::Object(
                Json::Object(obj.clone()).to_string(),
            ))
        }
    }
}

fn map_from_pairs(pairs: &Json) -> Result<Value, TypedJsonError> {
    let list = pairs
        .as_array()
        .ok_or_else(|| TypedJsonError::MapPairs(pairs.to_string()))?;
    let mut entries = Vec::with_capacity(list.len());
    for pair in list {
        let [key, value] = pair
            .as_array()
            .map(Vec::as_slice)
            .and_then(|p| <&[Json; 2]>::try_from(p).ok())
            .ok_or_else(|| TypedJsonError::MapPairs(pair.to_string()))?;
        let key = key
            .as_str()
            .ok_or_else(|| TypedJsonError::MapPairs(pair.to_string()))?;
        entries.push((key.to_owned(), to_value(value)?));
    }
    Ok(Value::Map(entries))
}

/// Converts a CBOR value into its typed JSON tree. Map pairs keep the order of
/// the value; the caller is responsible for canonical order.
pub fn from_value(value: &Value) -> Json {
    match value {
        Value::Int(i) => Json::Number(Number::from(*i)),
        Value::Bytes(b) => {
            let mut obj = Map::new();
            obj.insert("bstr".to_owned(), Json::String(encode_hex(b)));
            Json::Object(obj)
        }
        Value::Text(s) => Json::String(s.clone()),
        Value::Array(items) => Json::Array(items.iter().map(from_value).collect()),
        Value::Map(entries) => {
            let pairs = entries
                .iter()
                .map(|(k, v)| Json::Array(vec![Json::String(k.clone()), from_value(v)]))
                .collect();
            let mut obj = Map::new();
            obj.insert("map".to_owned(), Json::Array(pairs));
            Json::Object(obj)
        }
        Value::Bool(b) => Json::Bool(*b),
        Value::Null => Json::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        Value::Array(vec![
            Value::Int(23),
            Value::Map(vec![
                ("pane".to_owned(), Value::Int(7)),
                ("blob".to_owned(), Value::Bytes(vec![0x00, 0xff])),
                ("name".to_owned(), Value::Text("ação".to_owned())),
                ("ok".to_owned(), Value::Bool(true)),
                ("none".to_owned(), Value::Null),
                ("neg".to_owned(), Value::Int(-5)),
            ]),
        ])
    }

    #[test]
    fn roundtrip_every_kind() {
        let value = sample();
        let json = from_value(&value);
        assert_eq!(to_value(&json).unwrap(), value);
    }

    #[test]
    fn matches_the_readme_mapping() {
        let json = json!([23, {"map": [["pane", 7], ["blob", {"bstr": "00ff"}]]}]);
        let value = to_value(&json).unwrap();
        assert_eq!(
            value,
            Value::Array(vec![
                Value::Int(23),
                Value::Map(vec![
                    ("pane".to_owned(), Value::Int(7)),
                    ("blob".to_owned(), Value::Bytes(vec![0, 255])),
                ]),
            ])
        );
        assert_eq!(from_value(&value), json);
    }

    #[test]
    fn map_pairs_keep_their_order() {
        let json = json!({"map": [["b", 1], ["a", 2]]});
        let Value::Map(entries) = to_value(&json).unwrap() else {
            panic!("not a map");
        };
        assert_eq!(entries[0].0, "b");
        assert_eq!(entries[1].0, "a");
    }

    #[test]
    fn duplicate_keys_are_representable() {
        // A JSON object would collapse them; the pair list does not.
        let json = json!({"map": [["k", 1], ["k", 2]]});
        let Value::Map(entries) = to_value(&json).unwrap() else {
            panic!("not a map");
        };
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn rejects_floats_and_out_of_range_integers() {
        assert!(matches!(
            to_value(&json!(1.5)),
            Err(TypedJsonError::Number(_))
        ));
        assert!(matches!(
            to_value(&json!(9_007_199_254_740_992_i64)),
            Err(TypedJsonError::Number(_))
        ));
        assert!(to_value(&json!(9_007_199_254_740_991_i64)).is_ok());
        assert!(to_value(&json!(-9_007_199_254_740_991_i64)).is_ok());
    }

    #[test]
    fn rejects_uppercase_and_odd_hex() {
        assert!(matches!(
            to_value(&json!({"bstr": "AB"})),
            Err(TypedJsonError::Hex(_))
        ));
        assert!(matches!(
            to_value(&json!({"bstr": "abc"})),
            Err(TypedJsonError::Hex(_))
        ));
        assert!(matches!(
            to_value(&json!({"bstr": 1})),
            Err(TypedJsonError::Hex(_))
        ));
    }

    #[test]
    fn rejects_unknown_objects_and_bad_pairs() {
        assert!(matches!(
            to_value(&json!({"tstr": "x"})),
            Err(TypedJsonError::Object(_))
        ));
        assert!(matches!(
            to_value(&json!({"bstr": "00", "map": []})),
            Err(TypedJsonError::Object(_))
        ));
        assert!(matches!(
            to_value(&json!({"map": [["k"]]})),
            Err(TypedJsonError::MapPairs(_))
        ));
        assert!(matches!(
            to_value(&json!({"map": [[1, 2]]})),
            Err(TypedJsonError::MapPairs(_))
        ));
        assert!(matches!(
            to_value(&json!({"map": 3})),
            Err(TypedJsonError::MapPairs(_))
        ));
    }
}
