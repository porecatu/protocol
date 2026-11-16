// SPDX-License-Identifier: GPL-3.0-or-later
//! Host identifier derived from the routing key.
//!
//! ```text
//! host_id = base32_lower_nopad( BLAKE2s-256( routing_public_key )[0..20] )
//! ```
//!
//! 20 bytes, 32 characters of `abcdefghijklmnopqrstuvwxyz234567`.

use core::fmt;

use blake2::{Blake2s256, Digest};

use crate::encoding;

/// Number of digest bytes kept in a [`HostId`].
pub const HOST_ID_BYTES: usize = 20;
/// Length of the textual form of a [`HostId`].
pub const HOST_ID_CHARS: usize = 32;

/// The text is not a valid host identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostIdError {
    /// Not exactly 32 characters.
    Length,
    /// A character outside the lowercase base32 alphabet, or non-canonical
    /// trailing bits.
    Alphabet,
}

impl fmt::Display for HostIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => f.write_str("host id must have 32 characters"),
            Self::Alphabet => f.write_str("host id has characters outside the base32 alphabet"),
        }
    }
}

impl core::error::Error for HostIdError {}

/// Address of a host at the relay.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct HostId {
    bytes: [u8; HOST_ID_BYTES],
    text: String,
}

impl HostId {
    /// Parses the 32-character textual form.
    pub fn parse(text: &str) -> Result<Self, HostIdError> {
        if text.len() != HOST_ID_CHARS {
            return Err(HostIdError::Length);
        }
        let decoded = encoding::decode_base32_lower(text).map_err(|_| HostIdError::Alphabet)?;
        let bytes: [u8; HOST_ID_BYTES] = decoded
            .as_slice()
            .try_into()
            .map_err(|_| HostIdError::Length)?;
        Ok(Self {
            bytes,
            text: text.to_owned(),
        })
    }

    fn from_bytes(bytes: [u8; HOST_ID_BYTES]) -> Self {
        let text = encoding::encode_base32_lower(&bytes);
        Self { bytes, text }
    }

    /// The 32-character lowercase text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The 20 raw bytes.
    pub fn as_bytes(&self) -> &[u8; HOST_ID_BYTES] {
        &self.bytes
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl fmt::Debug for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HostId({})", self.text)
    }
}

/// Computes the `host_id` of a public routing key.
pub fn host_id(routing_key: &[u8; 32]) -> HostId {
    let digest = Blake2s256::digest(routing_key);
    let mut bytes = [0u8; HOST_ID_BYTES];
    bytes.copy_from_slice(&digest[..HOST_ID_BYTES]);
    HostId::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_a() -> [u8; 32] {
        core::array::from_fn(|i| i as u8)
    }

    // Expected values computed independently with Python:
    //   base64.b32encode(hashlib.blake2s(key).digest()[:20]).decode().lower().rstrip('=')
    const HOST_ID_A: &str = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";
    const HOST_ID_B: &str = "mhj67icrscnsvbofgekm46omai6nhln7";

    #[test]
    fn fixed_keys() {
        assert_eq!(host_id(&key_a()).as_str(), HOST_ID_A);
        assert_eq!(host_id(&[0xff; 32]).as_str(), HOST_ID_B);
    }

    #[test]
    fn parse_roundtrip() {
        let id = host_id(&key_a());
        let parsed = HostId::parse(HOST_ID_A).unwrap();
        assert_eq!(parsed, id);
        assert_eq!(parsed.as_bytes(), id.as_bytes());
        assert_eq!(parsed.to_string(), HOST_ID_A);
        assert_eq!(format!("{parsed:?}"), format!("HostId({HOST_ID_A})"));
    }

    #[test]
    fn parse_refuses_malformed() {
        assert_eq!(HostId::parse(""), Err(HostIdError::Length));
        assert_eq!(HostId::parse(&HOST_ID_A[..31]), Err(HostIdError::Length));
        assert_eq!(
            HostId::parse(&format!("{HOST_ID_A}a")),
            Err(HostIdError::Length)
        );
        assert_eq!(
            HostId::parse(&HOST_ID_A.to_ascii_uppercase()),
            Err(HostIdError::Alphabet)
        );
        assert_eq!(
            HostId::parse("awbfmb6x7xznqlxuypemflvjmgwzrvq1"),
            Err(HostIdError::Alphabet)
        );
        assert_eq!(HostId::parse("é"), Err(HostIdError::Length));
    }
}
