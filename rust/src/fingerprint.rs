// SPDX-License-Identifier: GPL-3.0-or-later
//! Human-comparable fingerprint of a static public key.
//!
//! ```text
//! fingerprint(pub) = base32_upper_nopad( BLAKE2s-256(pub)[0..10] )   ; 16 chars
//! display          = "ABCD EFGH IJKL MNOP"
//! ```

use core::fmt;

use blake2::{Blake2s256, Digest};

use crate::encoding;

/// Number of digest bytes kept in a [`Fingerprint`].
pub const FINGERPRINT_BYTES: usize = 10;

/// Fingerprint of a public key: 16 uppercase base32 characters.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    text: String,
}

impl Fingerprint {
    /// The 16 characters, without separators.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Fingerprint {
    /// Four groups of four characters separated by a space.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, group) in self.text.as_bytes().chunks(4).enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            // The text is ASCII, so every chunk is valid UTF-8.
            f.write_str(core::str::from_utf8(group).map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.text)
    }
}

/// Computes the fingerprint of a static public key.
pub fn fingerprint(public_key: &[u8; 32]) -> Fingerprint {
    let digest = Blake2s256::digest(public_key);
    Fingerprint {
        text: encoding::encode_base32_upper(&digest[..FINGERPRINT_BYTES]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values computed independently with Python:
    //   base64.b32encode(hashlib.blake2s(key).digest()[:10]).decode().rstrip('=')
    const FP_A: &str = "AWBFMB6X7XZNQLXU";
    const FP_B: &str = "MHJ67ICRSCNSVBOF";

    #[test]
    fn fixed_keys() {
        let key_a: [u8; 32] = core::array::from_fn(|i| i as u8);
        let a = fingerprint(&key_a);
        assert_eq!(a.as_str(), FP_A);
        assert_eq!(a.to_string(), "AWBF MB6X 7XZN QLXU");
        let b = fingerprint(&[0xff; 32]);
        assert_eq!(b.as_str(), FP_B);
        assert_eq!(b.to_string(), "MHJ6 7ICR SCNS VBOF");
    }

    #[test]
    fn has_sixteen_characters() {
        assert_eq!(fingerprint(&[7; 32]).as_str().len(), 16);
    }
}
