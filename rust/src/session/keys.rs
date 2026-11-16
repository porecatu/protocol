// SPDX-License-Identifier: GPL-3.0-or-later

//! X25519 static key pairs (spec 02 §1).
//!
//! The private half lives in a buffer that is zeroed when the pair is dropped.
//! The pair is deliberately not `Clone`, and its `Debug` output never shows the
//! private key.

use core::fmt;

use snow::params::DHChoice;
use snow::resolvers::{CryptoResolver, DefaultResolver};
use zeroize::{Zeroize, Zeroizing};

/// An X25519 static key pair: the host static key or the device static key.
pub struct StaticKeypair {
    private: Zeroizing<[u8; 32]>,
    public: [u8; 32],
}

impl StaticKeypair {
    /// Generates a new pair from the operating system random generator.
    ///
    /// # Panics
    ///
    /// Panics if the operating system random generator is unavailable: there
    /// is no safe fallback for key generation.
    #[must_use]
    pub fn generate() -> StaticKeypair {
        let resolver = DefaultResolver;
        let mut rng = resolver
            .resolve_rng()
            .expect("the default resolver always provides a random generator");
        let mut dh = resolver
            .resolve_dh(&DHChoice::Curve25519)
            .expect("the default resolver always provides X25519");
        dh.generate(&mut *rng)
            .expect("operating system random generator unavailable");
        Self::from_dh(&*dh)
    }

    /// Builds a pair from an existing private key, deriving the public half.
    #[must_use]
    pub fn from_private(private: [u8; 32]) -> StaticKeypair {
        let mut private = Zeroizing::new(private);
        let mut dh = DefaultResolver
            .resolve_dh(&DHChoice::Curve25519)
            .expect("the default resolver always provides X25519");
        dh.set(&private[..]);
        private.zeroize();
        Self::from_dh(&*dh)
    }

    fn from_dh(dh: &dyn snow::types::Dh) -> StaticKeypair {
        let mut private = Zeroizing::new([0u8; 32]);
        private.copy_from_slice(dh.privkey());
        let mut public = [0u8; 32];
        public.copy_from_slice(dh.pubkey());
        StaticKeypair { private, public }
    }

    /// The public key.
    #[must_use]
    pub fn public(&self) -> [u8; 32] {
        self.public
    }

    /// A second copy of the pair, for a state machine that must hold the key
    /// until the first handshake message arrives. The copy zeroes its private
    /// half on drop like the original; keep it only as long as needed.
    #[cfg(feature = "message")]
    pub(crate) fn duplicate(&self) -> StaticKeypair {
        StaticKeypair {
            private: Zeroizing::new(*self.private),
            public: self.public,
        }
    }

    /// The private key, for the handshake builder only.
    pub(crate) fn expose_private(&self) -> &[u8; 32] {
        &self.private
    }

    /// A copy of the private key, for test vectors only.
    #[cfg(any(test, feature = "test-vectors"))]
    #[must_use]
    pub fn private_bytes(&self) -> [u8; 32] {
        *self.private
    }
}

impl fmt::Debug for StaticKeypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StaticKeypair")
            .field("public", &self.public)
            .field("private", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::StaticKeypair;

    #[test]
    fn generate_gives_distinct_pairs() {
        let a = StaticKeypair::generate();
        let b = StaticKeypair::generate();
        assert_ne!(a.public(), b.public());
        assert_ne!(a.private_bytes(), b.private_bytes());
    }

    #[test]
    fn from_private_round_trips_and_matches_rfc7748() {
        // RFC 7748 §6.1, Alice's key pair.
        let private: [u8; 32] = [
            0x77, 0x07, 0x6d, 0x0a, 0x73, 0x18, 0xa5, 0x7d, 0x3c, 0x16, 0xc1, 0x72, 0x51, 0xb2,
            0x66, 0x45, 0xdf, 0x4c, 0x2f, 0x87, 0xeb, 0xc0, 0x99, 0x2a, 0xb1, 0x77, 0xfb, 0xa5,
            0x1d, 0xb9, 0x2c, 0x2a,
        ];
        let expected_public: [u8; 32] = [
            0x85, 0x20, 0xf0, 0x09, 0x89, 0x30, 0xa7, 0x54, 0x74, 0x8b, 0x7d, 0xdc, 0xb4, 0x3e,
            0xf7, 0x5a, 0x0d, 0xbf, 0x3a, 0x0d, 0x26, 0x38, 0x1a, 0xf4, 0xeb, 0xa4, 0xa9, 0x8e,
            0xaa, 0x9b, 0x4e, 0x6a,
        ];
        let pair = StaticKeypair::from_private(private);
        assert_eq!(pair.public(), expected_public);
        assert_eq!(pair.private_bytes(), private);
    }

    #[test]
    fn debug_hides_the_private_key() {
        let pair = StaticKeypair::from_private([7u8; 32]);
        let shown = format!("{pair:?}");
        assert!(shown.contains("redacted"));
        assert!(!shown.contains("[7, 7"));
    }
}
