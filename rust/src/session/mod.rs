// SPDX-License-Identifier: GPL-3.0-or-later
//! Noise handshake, encrypted records and the session state machines
//! (`session` feature; the state machines also need `message`).

pub mod handshake;
pub mod keys;
pub mod pairing_token;
pub mod record;
pub mod rekey;

#[cfg(feature = "message")]
pub mod client;
#[cfg(feature = "message")]
pub mod host;

/// How long a handshake may take: 15 seconds (spec 02 §2). The host counts
/// from the `channel-opened`, the client from the creation of its session.
pub const HANDSHAKE_TIMEOUT_MS: u64 = 15_000;

#[cfg(feature = "message")]
pub use client::{ClientError, ClientEvent, ClientSession};
pub use handshake::{
    ClientHandshake, ClientStep, HandshakeError, HandshakeKind, HostHandshake, HostStep,
    MAX_NOISE_MESSAGE, MAX_PLAINTEXT, NOISE_IK, NOISE_XX, NoiseFault, Transport, prologue,
};
#[cfg(feature = "message")]
pub use host::{HostChannel, HostError, HostEvent};
pub use keys::StaticKeypair;
pub use pairing_token::{PairingToken, TOKEN_LEN, TOKEN_TTL_MS};
pub use record::{FLAG_CONTINUE, FLAG_REKEY, MAX_FRAGMENT, MAX_MESSAGE, RecordError, Session};
pub use rekey::{RekeyPolicy, RekeyState};
