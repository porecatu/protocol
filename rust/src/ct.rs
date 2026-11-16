// SPDX-License-Identifier: GPL-3.0-or-later

//! Constant-time comparisons for secrets and identities: the host key from
//! the pairing QR (spec 02 §3.2) and the pairing token (spec 02 §3.2, item 3).

use subtle::ConstantTimeEq;

/// Compares two byte slices in constant time with respect to their contents.
///
/// Slices of different lengths compare unequal; the length itself is not
/// secret in any use of this crate (keys and tokens have fixed sizes).
#[must_use]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

#[cfg(test)]
mod tests {
    use super::ct_eq;

    #[test]
    fn equal_and_different() {
        assert!(ct_eq(&[1, 2, 3], &[1, 2, 3]));
        assert!(!ct_eq(&[1, 2, 3], &[1, 2, 4]));
        assert!(!ct_eq(&[1, 2, 3], &[1, 2]));
        assert!(ct_eq(&[], &[]));
    }
}
