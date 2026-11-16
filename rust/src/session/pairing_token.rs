// SPDX-License-Identifier: GPL-3.0-or-later

//! The pairing token of the QR code (spec 02 §3.1 and §3.2, item 3).
//!
//! A token is 16 random bytes, single use, valid for five minutes. The host
//! keeps at most one live token: the application owns that rule (generating
//! another token means dropping this one). This type enforces what is local to
//! one token: the deadline, the single use and the constant-time comparison.
//!
//! Sans-IO: the clock is a monotonic millisecond number received from outside.

use core::fmt;

use snow::resolvers::{CryptoResolver, DefaultResolver};
use zeroize::Zeroizing;

use crate::ct::ct_eq;

/// How long a token stays valid: five minutes (spec 02 §3.1).
pub const TOKEN_TTL_MS: u64 = 300_000;

/// Size of a token in bytes.
pub const TOKEN_LEN: usize = 16;

/// A pairing token with its deadline and its single-use state.
///
/// Deliberately not `Clone`, and its `Debug` output never shows the bytes.
pub struct PairingToken {
    bytes: Zeroizing<[u8; TOKEN_LEN]>,
    expires_at_ms: u64,
    consumed: bool,
}

impl PairingToken {
    /// Wraps given bytes (a token read back from somewhere, or injected in a
    /// test). It is valid for [`TOKEN_TTL_MS`] after `now_ms`.
    #[must_use]
    pub fn new(bytes: [u8; TOKEN_LEN], now_ms: u64) -> PairingToken {
        PairingToken {
            bytes: Zeroizing::new(bytes),
            expires_at_ms: now_ms.saturating_add(TOKEN_TTL_MS),
            consumed: false,
        }
    }

    /// Generates a token from the operating system random generator.
    ///
    /// # Panics
    ///
    /// Panics if the operating system random generator is unavailable: there
    /// is no safe fallback for a secret.
    #[must_use]
    pub fn generate(now_ms: u64) -> PairingToken {
        let mut rng = DefaultResolver
            .resolve_rng()
            .expect("the default resolver always provides a random generator");
        let mut bytes = [0u8; TOKEN_LEN];
        rng.try_fill_bytes(&mut bytes)
            .expect("operating system random generator unavailable");
        PairingToken::new(bytes, now_ms)
    }

    /// The token bytes, for the QR code. A secret: never log it.
    #[must_use]
    pub fn bytes(&self) -> [u8; TOKEN_LEN] {
        *self.bytes
    }

    /// The monotonic time at which the token stops being valid.
    #[must_use]
    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }

    /// Whether the token can still be presented: not expired and not used.
    #[must_use]
    pub fn is_live(&self, now_ms: u64) -> bool {
        !self.consumed && now_ms < self.expires_at_ms
    }

    /// Whether the token was already presented, matching or not.
    #[must_use]
    pub fn is_consumed(&self) -> bool {
        self.consumed
    }

    /// Checks a token presented by a client (spec 02 §3.2, item 3).
    ///
    /// The comparison is constant-time. The token is **consumed even when the
    /// check fails**: whoever presents a wrong token burns the right one, so a
    /// guess costs a new QR code. An expired or already consumed token never
    /// matches.
    #[must_use]
    pub fn matches_and_consume(&mut self, presented: &[u8], now_ms: u64) -> bool {
        // The secret comparison always runs, so the time does not tell which
        // of the (public) conditions below decided.
        let equal = ct_eq(&self.bytes[..], presented);
        let live = self.is_live(now_ms);
        self.consumed = true;
        equal && live
    }
}

impl fmt::Debug for PairingToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingToken")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("consumed", &self.consumed)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BYTES: [u8; TOKEN_LEN] = [9; TOKEN_LEN];

    #[test]
    fn matching_token_is_accepted_once() {
        let mut token = PairingToken::new(BYTES, 1_000);
        assert!(token.is_live(1_000));
        assert!(token.matches_and_consume(&BYTES, 2_000));
        assert!(token.is_consumed());
        assert!(!token.matches_and_consume(&BYTES, 2_001));
    }

    #[test]
    fn wrong_token_burns_the_right_one() {
        let mut token = PairingToken::new(BYTES, 0);
        assert!(!token.matches_and_consume(&[8; TOKEN_LEN], 10));
        assert!(token.is_consumed());
        assert!(!token.matches_and_consume(&BYTES, 11));
    }

    #[test]
    fn wrong_length_never_matches_and_still_consumes() {
        let mut token = PairingToken::new(BYTES, 0);
        assert!(!token.matches_and_consume(&BYTES[..15], 10));
        assert!(token.is_consumed());
    }

    #[test]
    fn expires_after_five_minutes() {
        let mut token = PairingToken::new(BYTES, 1_000);
        assert_eq!(token.expires_at_ms(), 1_000 + TOKEN_TTL_MS);
        assert!(token.is_live(1_000 + TOKEN_TTL_MS - 1));
        assert!(!token.is_live(1_000 + TOKEN_TTL_MS));
        assert!(!token.matches_and_consume(&BYTES, 1_000 + TOKEN_TTL_MS));
        assert!(token.is_consumed());
    }

    #[test]
    fn last_millisecond_still_matches() {
        let mut token = PairingToken::new(BYTES, 0);
        assert!(token.matches_and_consume(&BYTES, TOKEN_TTL_MS - 1));
    }

    #[test]
    fn deadline_does_not_overflow() {
        let token = PairingToken::new(BYTES, u64::MAX);
        assert_eq!(token.expires_at_ms(), u64::MAX);
    }

    #[test]
    fn generate_gives_distinct_tokens() {
        let a = PairingToken::generate(0);
        let b = PairingToken::generate(0);
        assert_ne!(a.bytes(), b.bytes());
        assert_ne!(a.bytes(), [0u8; TOKEN_LEN]);
    }

    #[test]
    fn debug_hides_the_bytes() {
        let token = PairingToken::new([7; TOKEN_LEN], 0);
        let shown = format!("{token:?}");
        assert!(shown.contains("redacted"));
        assert!(!shown.contains("[7, 7"));
    }
}
