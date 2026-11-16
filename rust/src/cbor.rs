// SPDX-License-Identifier: GPL-3.0-or-later
//! Deterministic CBOR helpers.
//!
//! A small value tree, a deterministic encoder (RFC 8949 §4.2.1) and a strict
//! hand-written decoder that rejects everything specification 03 §1.1 says to
//! reject: non-minimal integers and lengths, indefinite lengths, tags, floats,
//! simple values other than `false`/`true`/`null`, integers outside
//! `-(2^53 - 1) ..= 2^53 - 1`, non-text map keys, keys out of canonical order,
//! duplicate keys, invalid UTF-8, nesting deeper than [`MAX_DEPTH`] and
//! trailing bytes.
//!
//! This module knows nothing about messages.

use std::cmp::Ordering;
use std::fmt;

/// Maximum nesting of arrays and maps accepted by the decoder.
pub const MAX_DEPTH: usize = 32;

/// Largest integer magnitude allowed on the wire (`2^53 - 1`, the largest
/// exact integer in JavaScript).
pub const MAX_SAFE_INT: i64 = (1 << 53) - 1;

/// A CBOR data item restricted to the subset the protocol uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Integer within `-MAX_SAFE_INT ..= MAX_SAFE_INT`.
    Int(i64),
    /// Byte string.
    Bytes(Vec<u8>),
    /// Text string.
    Text(String),
    /// Array.
    Array(Vec<Value>),
    /// Map with text keys, kept as a list of pairs. Order of a map built by
    /// the caller does not matter: the encoder sorts it.
    Map(Vec<(String, Value)>),
    /// Boolean.
    Bool(bool),
    /// Null.
    Null,
}

/// Why a CBOR item was rejected, or why a typed read failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CborError {
    /// The input ended before the item did, or a declared length exceeds the
    /// remaining input.
    Truncated,
    /// Bytes left over after the top-level item.
    TrailingBytes,
    /// Integer or length not in its shortest form.
    NonMinimal,
    /// Indefinite-length string, array or map.
    IndefiniteLength,
    /// Additional-information value reserved by RFC 8949.
    Reserved,
    /// Tag (major type 6).
    Tag,
    /// Floating-point number.
    Float,
    /// Simple value other than `false`, `true` and `null`.
    UnsupportedSimple,
    /// Integer outside `-(2^53 - 1) ..= 2^53 - 1` (or a negative value read
    /// as unsigned).
    IntOutOfRange,
    /// Map key that is not a text string.
    NonTextKey,
    /// Map keys not in canonical (encoded-bytes lexicographic) order.
    UnorderedKeys,
    /// Same map key twice.
    DuplicateKey,
    /// Text string that is not valid UTF-8.
    InvalidUtf8,
    /// Arrays and maps nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// A value had a different type than the one asked for.
    WrongType,
    /// A required map key is absent.
    MissingKey,
    /// A positional array has a different number of elements than defined.
    WrongArity,
}

impl fmt::Display for CborError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            CborError::Truncated => "truncated input",
            CborError::TrailingBytes => "trailing bytes",
            CborError::NonMinimal => "non-minimal integer or length",
            CborError::IndefiniteLength => "indefinite length",
            CborError::Reserved => "reserved additional information",
            CborError::Tag => "tag not allowed",
            CborError::Float => "float not allowed",
            CborError::UnsupportedSimple => "unsupported simple value",
            CborError::IntOutOfRange => "integer out of range",
            CborError::NonTextKey => "map key is not text",
            CborError::UnorderedKeys => "map keys out of order",
            CborError::DuplicateKey => "duplicate map key",
            CborError::InvalidUtf8 => "invalid UTF-8",
            CborError::TooDeep => "nesting too deep",
            CborError::WrongType => "wrong type",
            CborError::MissingKey => "missing key",
            CborError::WrongArity => "wrong array length",
        };
        f.write_str(text)
    }
}

impl std::error::Error for CborError {}

impl Value {
    /// Builds an integer, checking the `±(2^53 - 1)` domain.
    pub fn int(value: i64) -> Result<Value, CborError> {
        if (-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&value) {
            Ok(Value::Int(value))
        } else {
            Err(CborError::IntOutOfRange)
        }
    }

    /// Reads an unsigned integer. A negative integer is `IntOutOfRange`.
    pub fn as_u64(&self) -> Result<u64, CborError> {
        match self {
            Value::Int(v) => u64::try_from(*v).map_err(|_| CborError::IntOutOfRange),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads a signed integer.
    pub fn as_i64(&self) -> Result<i64, CborError> {
        match self {
            Value::Int(v) => Ok(*v),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads a text string.
    pub fn as_text(&self) -> Result<&str, CborError> {
        match self {
            Value::Text(s) => Ok(s),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads a byte string.
    pub fn as_bytes(&self) -> Result<&[u8], CborError> {
        match self {
            Value::Bytes(b) => Ok(b),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads an array.
    pub fn as_array(&self) -> Result<&[Value], CborError> {
        match self {
            Value::Array(a) => Ok(a),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads a map as its list of pairs.
    pub fn as_map(&self) -> Result<&[(String, Value)], CborError> {
        match self {
            Value::Map(m) => Ok(m),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads a boolean.
    pub fn as_bool(&self) -> Result<bool, CborError> {
        match self {
            Value::Bool(b) => Ok(*b),
            _ => Err(CborError::WrongType),
        }
    }

    /// Whether the value is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

/// Read-only view over a decoded map. Unknown keys are simply never read.
#[derive(Debug, Clone, Copy)]
pub struct MapReader<'a> {
    entries: &'a [(String, Value)],
}

impl<'a> MapReader<'a> {
    /// Wraps a list of pairs.
    pub fn new(entries: &'a [(String, Value)]) -> Self {
        MapReader { entries }
    }

    /// Wraps a value that must be a map.
    pub fn from_value(value: &'a Value) -> Result<Self, CborError> {
        Ok(MapReader::new(value.as_map()?))
    }

    /// The value under `key`, or `MissingKey`.
    pub fn required(&self, key: &str) -> Result<&'a Value, CborError> {
        self.optional(key).ok_or(CborError::MissingKey)
    }

    /// The value under `key`, if present.
    pub fn optional(&self, key: &str) -> Option<&'a Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// Checks that `value` is an array with exactly `arity` elements.
pub fn expect_array(value: &Value, arity: usize) -> Result<&[Value], CborError> {
    let items = value.as_array()?;
    if items.len() == arity {
        Ok(items)
    } else {
        Err(CborError::WrongArity)
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Encodes `value` deterministically (RFC 8949 §4.2.1).
pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    out
}

/// Appends the deterministic encoding of `value` to `out`.
///
/// Integers outside `±MAX_SAFE_INT` are a caller bug (build them with
/// [`Value::int`]); the encoder only asserts it in debug builds.
pub fn encode_into(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Int(v) => {
            debug_assert!((-MAX_SAFE_INT..=MAX_SAFE_INT).contains(v));
            if *v >= 0 {
                write_head(out, 0, *v as u64);
            } else {
                write_head(out, 1, !*v as u64);
            }
        }
        Value::Bytes(b) => {
            write_head(out, 2, b.len() as u64);
            out.extend_from_slice(b);
        }
        Value::Text(s) => write_text(out, s),
        Value::Array(items) => {
            write_head(out, 4, items.len() as u64);
            for item in items {
                encode_into(item, out);
            }
        }
        Value::Map(entries) => {
            debug_assert!(
                {
                    let mut keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
                    keys.sort_unstable();
                    keys.windows(2).all(|w| w[0] != w[1])
                },
                "duplicate map key"
            );
            let mut encoded: Vec<(Vec<u8>, &Value)> = entries
                .iter()
                .map(|(k, v)| {
                    let mut key = Vec::with_capacity(k.len() + 9);
                    write_text(&mut key, k);
                    (key, v)
                })
                .collect();
            encoded.sort_by(|a, b| a.0.cmp(&b.0));
            write_head(out, 5, encoded.len() as u64);
            for (key, v) in encoded {
                out.extend_from_slice(&key);
                encode_into(v, out);
            }
        }
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Null => out.push(0xf6),
    }
}

fn write_text(out: &mut Vec<u8>, s: &str) {
    write_head(out, 3, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Writes a major type and its argument in the shortest form.
fn write_head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    if arg < 24 {
        out.push(m | arg as u8);
    } else if arg <= u64::from(u8::MAX) {
        out.push(m | 24);
        out.push(arg as u8);
    } else if arg <= u64::from(u16::MAX) {
        out.push(m | 25);
        out.extend_from_slice(&(arg as u16).to_be_bytes());
    } else if arg <= u64::from(u32::MAX) {
        out.push(m | 26);
        out.extend_from_slice(&(arg as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// Decodes exactly one CBOR item, strictly (see the module docs).
pub fn decode(bytes: &[u8]) -> Result<Value, CborError> {
    let mut decoder = Decoder {
        input: bytes,
        pos: 0,
    };
    let value = decoder.value(0)?;
    if decoder.pos != bytes.len() {
        return Err(CborError::TrailingBytes);
    }
    Ok(value)
}

struct Decoder<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn remaining(&self) -> usize {
        self.input.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CborError> {
        if n > self.remaining() {
            return Err(CborError::Truncated);
        }
        let slice = &self.input[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, CborError> {
        Ok(self.take(1)?[0])
    }

    /// Reads the argument that follows an initial byte, enforcing the
    /// shortest form.
    fn argument(&mut self, major: u8, info: u8) -> Result<u64, CborError> {
        match info {
            0..=23 => Ok(u64::from(info)),
            24 => {
                let v = u64::from(self.byte()?);
                if v < 24 {
                    return Err(CborError::NonMinimal);
                }
                Ok(v)
            }
            25 => {
                let b = self.take(2)?;
                let v = u64::from(u16::from_be_bytes([b[0], b[1]]));
                if v <= u64::from(u8::MAX) {
                    return Err(CborError::NonMinimal);
                }
                Ok(v)
            }
            26 => {
                let b = self.take(4)?;
                let v = u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
                if v <= u64::from(u16::MAX) {
                    return Err(CborError::NonMinimal);
                }
                Ok(v)
            }
            27 => {
                let b = self.take(8)?;
                let v = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
                if v <= u64::from(u32::MAX) {
                    return Err(CborError::NonMinimal);
                }
                Ok(v)
            }
            31 if (2..=5).contains(&major) => Err(CborError::IndefiniteLength),
            _ => Err(CborError::Reserved),
        }
    }

    /// Reads a declared length and checks it fits in the remaining input,
    /// given the minimum encoded size of one unit, before anything is
    /// allocated.
    fn length(&mut self, major: u8, info: u8, unit: usize) -> Result<usize, CborError> {
        let n = self.argument(major, info)?;
        let max = (self.remaining() / unit) as u64;
        if n > max {
            return Err(CborError::Truncated);
        }
        Ok(n as usize)
    }

    fn value(&mut self, depth: usize) -> Result<Value, CborError> {
        let initial = self.byte()?;
        let major = initial >> 5;
        let info = initial & 0x1f;
        match major {
            0 => {
                let n = self.argument(major, info)?;
                if n > MAX_SAFE_INT as u64 {
                    return Err(CborError::IntOutOfRange);
                }
                Ok(Value::Int(n as i64))
            }
            1 => {
                let n = self.argument(major, info)?;
                // -1 - n must be >= -MAX_SAFE_INT.
                if n > (MAX_SAFE_INT - 1) as u64 {
                    return Err(CborError::IntOutOfRange);
                }
                Ok(Value::Int(-1 - n as i64))
            }
            2 => {
                let len = self.length(major, info, 1)?;
                Ok(Value::Bytes(self.take(len)?.to_vec()))
            }
            3 => {
                let len = self.length(major, info, 1)?;
                let raw = self.take(len)?;
                let text = std::str::from_utf8(raw).map_err(|_| CborError::InvalidUtf8)?;
                Ok(Value::Text(text.to_owned()))
            }
            4 => {
                if depth + 1 > MAX_DEPTH {
                    return Err(CborError::TooDeep);
                }
                let count = self.length(major, info, 1)?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            5 => {
                if depth + 1 > MAX_DEPTH {
                    return Err(CborError::TooDeep);
                }
                let count = self.length(major, info, 2)?;
                let mut entries: Vec<(String, Value)> = Vec::with_capacity(count);
                let mut previous: Option<&'a [u8]> = None;
                for _ in 0..count {
                    let start = self.pos;
                    let key = self.key()?;
                    let encoded_key = &self.input[start..self.pos];
                    if let Some(prev) = previous {
                        match prev.cmp(encoded_key) {
                            Ordering::Less => {}
                            Ordering::Equal => return Err(CborError::DuplicateKey),
                            Ordering::Greater => return Err(CborError::UnorderedKeys),
                        }
                    }
                    previous = Some(encoded_key);
                    let value = self.value(depth + 1)?;
                    entries.push((key, value));
                }
                Ok(Value::Map(entries))
            }
            6 => Err(CborError::Tag),
            _ => match info {
                20 => Ok(Value::Bool(false)),
                21 => Ok(Value::Bool(true)),
                22 => Ok(Value::Null),
                25..=27 => Err(CborError::Float),
                _ => Err(CborError::UnsupportedSimple),
            },
        }
    }

    /// Reads a map key, which must be a text string.
    fn key(&mut self) -> Result<String, CborError> {
        let initial = self.byte()?;
        if initial >> 5 != 3 {
            return Err(CborError::NonTextKey);
        }
        let len = self.length(3, initial & 0x1f, 1)?;
        let raw = self.take(len)?;
        let text = std::str::from_utf8(raw).map_err(|_| CborError::InvalidUtf8)?;
        Ok(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_owned())
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn encodes_integers_in_shortest_form() {
        let cases: &[(i64, &str)] = &[
            (0, "00"),
            (23, "17"),
            (24, "1818"),
            (255, "18ff"),
            (256, "190100"),
            (65535, "19ffff"),
            (65536, "1a00010000"),
            (-1, "20"),
            (-24, "37"),
            (-25, "3818"),
            (MAX_SAFE_INT, "1b001fffffffffffff"),
            (-MAX_SAFE_INT, "3b001ffffffffffffe"),
        ];
        for (n, expected) in cases {
            assert_eq!(hex(&encode(&Value::Int(*n))), *expected, "value {n}");
            assert_eq!(decode(&unhex(expected)), Ok(Value::Int(*n)), "value {n}");
        }
    }

    #[test]
    fn encodes_strings_arrays_and_simples() {
        assert_eq!(hex(&encode(&text(""))), "60");
        assert_eq!(hex(&encode(&text("a"))), "6161");
        assert_eq!(hex(&encode(&Value::Bytes(vec![1, 2]))), "420102");
        assert_eq!(hex(&encode(&Value::Bytes(vec![]))), "40");
        assert_eq!(
            hex(&encode(&Value::Array(vec![Value::Int(1), Value::Int(2)]))),
            "820102"
        );
        assert_eq!(hex(&encode(&Value::Bool(false))), "f4");
        assert_eq!(hex(&encode(&Value::Bool(true))), "f5");
        assert_eq!(hex(&encode(&Value::Null)), "f6");
    }

    #[test]
    fn long_lengths_use_the_shortest_head() {
        let encoded = encode(&Value::Bytes(vec![7; 24]));
        assert_eq!(&encoded[..2], &[0x58, 24]);
        let encoded = encode(&Value::Bytes(vec![7; 256]));
        assert_eq!(&encoded[..3], &[0x59, 0x01, 0x00]);
        assert_eq!(decode(&encoded), Ok(Value::Bytes(vec![7; 256])));
    }

    #[test]
    fn encoder_sorts_map_keys_by_encoded_bytes() {
        let value = map(&[
            ("b", Value::Int(1)),
            ("a", Value::Int(2)),
            ("aa", Value::Int(3)),
        ]);
        // a, b, aa: shorter encoded key first.
        assert_eq!(hex(&encode(&value)), "a361610261620162616103");
        // The decoder accepts the sorted form and returns it in that order.
        let decoded = decode(&encode(&value)).unwrap();
        let keys: Vec<&str> = decoded
            .as_map()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys, ["a", "b", "aa"]);
    }

    #[test]
    fn value_int_checks_the_domain() {
        assert_eq!(Value::int(MAX_SAFE_INT), Ok(Value::Int(MAX_SAFE_INT)));
        assert_eq!(Value::int(MAX_SAFE_INT + 1), Err(CborError::IntOutOfRange));
        assert_eq!(Value::int(-MAX_SAFE_INT), Ok(Value::Int(-MAX_SAFE_INT)));
        assert_eq!(Value::int(-MAX_SAFE_INT - 1), Err(CborError::IntOutOfRange));
        assert_eq!(Value::int(i64::MIN), Err(CborError::IntOutOfRange));
    }

    #[test]
    fn rejects_non_minimal_forms() {
        for input in [
            "1817",
            "180f",
            "19 0001",
            "19 00ff",
            "1a 0000ffff",
            "1b 00000000ffffffff",
            "1b 0000000000000001",
            "5800",
            "5817 00",
            "9800",
            "b800",
            "3800",
        ] {
            assert_eq!(decode(&unhex(input)), Err(CborError::NonMinimal), "{input}");
        }
    }

    #[test]
    fn rejects_indefinite_and_reserved() {
        for input in ["9f01ff", "bf6161 01ff", "5f4101ff", "7f6161ff"] {
            assert_eq!(
                decode(&unhex(input)),
                Err(CborError::IndefiniteLength),
                "{input}"
            );
        }
        assert_eq!(decode(&unhex("1c")), Err(CborError::Reserved));
        assert_eq!(decode(&unhex("1f")), Err(CborError::Reserved));
    }

    #[test]
    fn rejects_tags_floats_and_other_simples() {
        assert_eq!(decode(&unhex("c000")), Err(CborError::Tag));
        assert_eq!(decode(&unhex("c11a514b67b0")), Err(CborError::Tag));
        assert_eq!(decode(&unhex("f93c00")), Err(CborError::Float));
        assert_eq!(decode(&unhex("fa3f800000")), Err(CborError::Float));
        assert_eq!(decode(&unhex("fb3ff0000000000000")), Err(CborError::Float));
        assert_eq!(decode(&unhex("f7")), Err(CborError::UnsupportedSimple));
        assert_eq!(decode(&unhex("f0")), Err(CborError::UnsupportedSimple));
        assert_eq!(decode(&unhex("f820")), Err(CborError::UnsupportedSimple));
        assert_eq!(decode(&unhex("ff")), Err(CborError::UnsupportedSimple));
    }

    #[test]
    fn rejects_integers_outside_the_safe_range() {
        // 2^53
        assert_eq!(
            decode(&unhex("1b0020000000000000")),
            Err(CborError::IntOutOfRange)
        );
        // -2^53 (n = 2^53 - 1)
        assert_eq!(
            decode(&unhex("3b001fffffffffffff")),
            Err(CborError::IntOutOfRange)
        );
        assert_eq!(
            decode(&unhex("1bffffffffffffffff")),
            Err(CborError::IntOutOfRange)
        );
        assert_eq!(
            decode(&unhex("3bffffffffffffffff")),
            Err(CborError::IntOutOfRange)
        );
        // Boundaries are accepted.
        assert_eq!(
            decode(&unhex("1b001fffffffffffff")),
            Ok(Value::Int(MAX_SAFE_INT))
        );
        assert_eq!(
            decode(&unhex("3b001ffffffffffffe")),
            Ok(Value::Int(-MAX_SAFE_INT))
        );
    }

    #[test]
    fn rejects_bad_map_keys() {
        // {"b": 1, "a": 2}
        assert_eq!(
            decode(&unhex("a2 6162 01 6161 02")),
            Err(CborError::UnorderedKeys)
        );
        // {"aa": 1, "b": 2}: longer encoded key first
        assert_eq!(
            decode(&unhex("a2 626161 01 6162 02")),
            Err(CborError::UnorderedKeys)
        );
        // {"a": 1, "a": 2}
        assert_eq!(
            decode(&unhex("a2 6161 01 6161 02")),
            Err(CborError::DuplicateKey)
        );
        // {1: 2}
        assert_eq!(decode(&unhex("a1 01 02")), Err(CborError::NonTextKey));
        // {h'01': 2}
        assert_eq!(decode(&unhex("a1 4101 02")), Err(CborError::NonTextKey));
        // A non-canonical map nested inside an array is still rejected.
        assert_eq!(
            decode(&unhex("81 a2 6162 01 6161 02")),
            Err(CborError::UnorderedKeys)
        );
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert_eq!(decode(&unhex("61ff")), Err(CborError::InvalidUtf8));
        assert_eq!(decode(&unhex("a1 61ff 00")), Err(CborError::InvalidUtf8));
        // Overlong encoding of '/'.
        assert_eq!(decode(&unhex("62c0af")), Err(CborError::InvalidUtf8));
    }

    #[test]
    fn nesting_limit_is_32() {
        let nested = |levels: usize| {
            let mut bytes = vec![0x81; levels];
            bytes.push(0x00);
            bytes
        };
        assert!(decode(&nested(32)).is_ok());
        assert_eq!(decode(&nested(33)), Err(CborError::TooDeep));
        assert_eq!(decode(&nested(10_000)), Err(CborError::TooDeep));

        // Maps count as levels too.
        let mut bytes = Vec::new();
        for _ in 0..33 {
            bytes.extend_from_slice(&[0xa1, 0x61, b'a']);
        }
        bytes.push(0x00);
        assert_eq!(decode(&bytes), Err(CborError::TooDeep));
    }

    #[test]
    fn rejects_trailing_and_truncated_input() {
        assert_eq!(decode(&unhex("0000")), Err(CborError::TrailingBytes));
        assert_eq!(decode(&unhex("f6f6")), Err(CborError::TrailingBytes));
        assert_eq!(decode(&[]), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("18")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("1a0001")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("6261")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("820102 03")), Err(CborError::TrailingBytes));
        assert_eq!(decode(&unhex("8201")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("a1 6161")), Err(CborError::Truncated));
    }

    #[test]
    fn huge_declared_lengths_fail_before_allocating() {
        assert_eq!(decode(&unhex("5affffffff")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("7affffffff")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("9affffffff")), Err(CborError::Truncated));
        assert_eq!(decode(&unhex("baffffffff")), Err(CborError::Truncated));
        assert_eq!(
            decode(&unhex("9bffffffffffffffff")),
            Err(CborError::Truncated)
        );
        assert_eq!(
            decode(&unhex("5b7fffffffffffffff")),
            Err(CborError::Truncated)
        );
        // An array declaring more elements than remaining bytes.
        assert_eq!(decode(&unhex("9a0001000000")), Err(CborError::Truncated));
    }

    fn sample_tree() -> Value {
        // Keys already in canonical order: "a" < "b" < "aa" < "zz" < "bcd".
        Value::Array(vec![
            Value::Int(0),
            Value::Int(-MAX_SAFE_INT),
            Value::Int(MAX_SAFE_INT),
            Value::Bytes(vec![0, 1, 2, 255]),
            text("héllo \u{1F600}"),
            Value::Bool(true),
            Value::Bool(false),
            Value::Null,
            Value::Array(vec![]),
            Value::Map(vec![]),
            Value::Map(vec![
                ("a".to_owned(), Value::Int(1)),
                ("b".to_owned(), Value::Array(vec![Value::Null, text("x")])),
                ("aa".to_owned(), Value::Bytes(vec![9; 30])),
                ("zz".to_owned(), map(&[("k", Value::Bool(true))])),
                ("bcd".to_owned(), text("")),
            ]),
        ])
    }

    #[test]
    fn round_trips_a_tree_with_every_type() {
        let tree = sample_tree();
        let bytes = encode(&tree);
        assert_eq!(decode(&bytes), Ok(tree.clone()));
        // Re-encoding the decoded tree gives the same bytes.
        assert_eq!(encode(&decode(&bytes).unwrap()), bytes);
    }

    #[test]
    fn unsorted_map_encodes_to_the_same_bytes_as_sorted() {
        let sorted = map(&[
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
            ("aa", Value::Int(3)),
        ]);
        let shuffled = map(&[
            ("aa", Value::Int(3)),
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
        ]);
        assert_eq!(encode(&sorted), encode(&shuffled));
    }

    #[test]
    fn typed_accessors() {
        assert_eq!(Value::Int(5).as_u64(), Ok(5));
        assert_eq!(Value::Int(-5).as_u64(), Err(CborError::IntOutOfRange));
        assert_eq!(Value::Int(-5).as_i64(), Ok(-5));
        assert_eq!(text("x").as_i64(), Err(CborError::WrongType));
        assert_eq!(text("x").as_text(), Ok("x"));
        assert_eq!(Value::Int(1).as_text(), Err(CborError::WrongType));
        assert_eq!(Value::Bytes(vec![1]).as_bytes(), Ok(&[1u8][..]));
        assert_eq!(text("x").as_bytes(), Err(CborError::WrongType));
        assert_eq!(Value::Array(vec![]).as_array(), Ok(&[][..]));
        assert_eq!(Value::Null.as_array(), Err(CborError::WrongType));
        assert!(Value::Map(vec![]).as_map().unwrap().is_empty());
        assert_eq!(Value::Null.as_map(), Err(CborError::WrongType));
        assert_eq!(Value::Bool(true).as_bool(), Ok(true));
        assert_eq!(Value::Int(1).as_bool(), Err(CborError::WrongType));
        assert!(Value::Null.is_null());
        assert!(!Value::Bool(false).is_null());
    }

    #[test]
    fn map_reader_and_expect_array() {
        let value = map(&[("a", Value::Int(1)), ("extra", Value::Null)]);
        let reader = MapReader::from_value(&value).unwrap();
        assert_eq!(reader.required("a"), Ok(&Value::Int(1)));
        assert_eq!(reader.required("missing"), Err(CborError::MissingKey));
        assert_eq!(reader.optional("missing"), None);
        assert_eq!(reader.optional("extra"), Some(&Value::Null));
        assert!(MapReader::from_value(&Value::Null).is_err());

        let array = Value::Array(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(expect_array(&array, 2).map(<[Value]>::len), Ok(2));
        assert_eq!(expect_array(&array, 3), Err(CborError::WrongArity));
        assert_eq!(expect_array(&array, 1), Err(CborError::WrongArity));
        assert_eq!(expect_array(&Value::Null, 0), Err(CborError::WrongType));
    }

    #[test]
    fn unknown_keys_still_need_canonical_form() {
        // {"a": 1, "zz": 2} is canonical; reading only "a" works.
        let value = decode(&unhex("a2 6161 01 62 7a7a 02")).unwrap();
        assert_eq!(
            MapReader::from_value(&value).unwrap().required("a"),
            Ok(&Value::Int(1))
        );
    }
}
