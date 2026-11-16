// SPDX-License-Identifier: GPL-3.0-or-later
//! Text encodings shared by the protocol (base32, base64url, hex).
//!
//! Decoders are strict: padding, characters outside the alphabet, the wrong
//! letter case and non-canonical trailing bits are all refused.

use core::fmt;

use data_encoding::{BASE32_NOPAD, BASE64URL_NOPAD, HEXLOWER};

/// The input is not valid for the requested encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid encoded input")
    }
}

impl core::error::Error for DecodeError {}

/// Base32 (RFC 4648), lowercase alphabet, no padding.
pub fn encode_base32_lower(bytes: &[u8]) -> String {
    BASE32_NOPAD.encode(bytes).to_ascii_lowercase()
}

/// Strict inverse of [`encode_base32_lower`].
pub fn decode_base32_lower(text: &str) -> Result<Vec<u8>, DecodeError> {
    if !text
        .bytes()
        .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    {
        return Err(DecodeError);
    }
    BASE32_NOPAD
        .decode(text.to_ascii_uppercase().as_bytes())
        .map_err(|_| DecodeError)
}

/// Base32 (RFC 4648), uppercase alphabet, no padding.
pub fn encode_base32_upper(bytes: &[u8]) -> String {
    BASE32_NOPAD.encode(bytes)
}

/// Strict inverse of [`encode_base32_upper`].
pub fn decode_base32_upper(text: &str) -> Result<Vec<u8>, DecodeError> {
    BASE32_NOPAD
        .decode(text.as_bytes())
        .map_err(|_| DecodeError)
}

/// Base64url (RFC 4648 section 5), no padding.
pub fn encode_base64url(bytes: &[u8]) -> String {
    BASE64URL_NOPAD.encode(bytes)
}

/// Strict inverse of [`encode_base64url`]: refuses `=` padding, the standard
/// base64 alphabet (`+`, `/`) and non-zero trailing bits.
pub fn decode_base64url(text: &str) -> Result<Vec<u8>, DecodeError> {
    BASE64URL_NOPAD
        .decode(text.as_bytes())
        .map_err(|_| DecodeError)
}

/// Lowercase hexadecimal.
pub fn encode_hex(bytes: &[u8]) -> String {
    HEXLOWER.encode(bytes)
}

/// Strict inverse of [`encode_hex`]: lowercase only, even length.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, DecodeError> {
    HEXLOWER.decode(text.as_bytes()).map_err(|_| DecodeError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_lower_roundtrip_and_rfc_vectors() {
        // RFC 4648 section 10, lowercased and without padding.
        assert_eq!(encode_base32_lower(b""), "");
        assert_eq!(encode_base32_lower(b"f"), "my");
        assert_eq!(encode_base32_lower(b"fo"), "mzxq");
        assert_eq!(encode_base32_lower(b"foo"), "mzxw6");
        assert_eq!(encode_base32_lower(b"foob"), "mzxw6yq");
        assert_eq!(encode_base32_lower(b"fooba"), "mzxw6ytb");
        assert_eq!(encode_base32_lower(b"foobar"), "mzxw6ytboi");
        assert_eq!(decode_base32_lower("mzxw6ytboi").unwrap(), b"foobar");
    }

    #[test]
    fn base32_upper_roundtrip() {
        assert_eq!(encode_base32_upper(b"foobar"), "MZXW6YTBOI");
        assert_eq!(decode_base32_upper("MZXW6YTBOI").unwrap(), b"foobar");
    }

    #[test]
    fn base32_refuses_padding_case_and_alphabet() {
        assert_eq!(decode_base32_lower("my======"), Err(DecodeError));
        assert_eq!(decode_base32_lower("MY"), Err(DecodeError));
        assert_eq!(decode_base32_lower("m1"), Err(DecodeError));
        assert_eq!(decode_base32_upper("MY======"), Err(DecodeError));
        assert_eq!(decode_base32_upper("my"), Err(DecodeError));
        // Non-zero trailing bits ("mz" would be 'f' plus stray bits).
        assert_eq!(decode_base32_lower("mz"), Err(DecodeError));
    }

    #[test]
    fn base64url_roundtrip_and_strictness() {
        let bytes = [0xfb, 0xff, 0xfe];
        assert_eq!(encode_base64url(&bytes), "-__-");
        assert_eq!(decode_base64url("-__-").unwrap(), bytes);
        assert_eq!(encode_base64url(b"fo"), "Zm8");
        assert!(decode_base64url("Zm8=").is_err());
        assert!(decode_base64url("+/+/").is_err());
        // Non-zero trailing bits.
        assert_eq!(decode_base64url("Zm9"), Err(DecodeError));
        assert!(decode_base64url("Z").is_err());
    }

    #[test]
    fn hex_lowercase_only() {
        assert_eq!(encode_hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(decode_hex("00abff").unwrap(), [0x00, 0xab, 0xff]);
        assert!(decode_hex("00ABFF").is_err());
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("zz").is_err());
    }
}
