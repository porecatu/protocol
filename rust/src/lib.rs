// SPDX-License-Identifier: GPL-3.0-or-later
//! Porecatu remote access protocol.
//!
//! Sans-IO reference implementation: bytes and `now` go in, bytes and events
//! come out. No sockets, threads, clocks, disk or async runtime.
//!
//! Layers: relay envelope (always compiled), Noise handshake and records
//! (`session` feature) and CBOR messages (`message` feature).

/// Wire protocol version implemented by this crate, as `[major, minor]`
/// (spec 04). Not the version of the specification text.
pub const PROTOCOL_VERSION: [u64; 2] = [0, 1];

pub mod cbor;
pub mod ct;
pub mod encoding;
pub mod envelope;
pub mod fingerprint;
pub mod host_id;
pub mod pairing_uri;

#[cfg(feature = "message")]
pub mod message;
#[cfg(feature = "session")]
pub mod session;

#[cfg(all(feature = "session", feature = "message"))]
pub use session::{ClientError, ClientEvent, ClientSession, HostChannel, HostError, HostEvent};
#[cfg(feature = "session")]
pub use session::{PairingToken, StaticKeypair};
