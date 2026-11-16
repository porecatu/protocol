// SPDX-License-Identifier: GPL-3.0-or-later
//! The fixed keys and the small helpers the Noise, record and session cases
//! share: every one of those vectors is keyed from the same four private keys
//! and the same host id, so the files read alike.

use blake2::{Blake2s256, Digest};
use porecatu_protocol::encoding::encode_hex;
use porecatu_protocol::host_id::{HostId, host_id};
use porecatu_protocol::session::{
    ClientHandshake, ClientStep, HandshakeKind, HostHandshake, HostStep, StaticKeypair, Transport,
};

use super::{bytes, seq};
use crate::vector::SessionKeys;

/// The Ed25519 routing public key the fixed host id is derived from (any 32
/// bytes: the vectors never verify a signature).
const ROUTING_KEY: [u8; 32] = [0x42; 32];

/// The four fixed private keys of a conversation.
#[derive(Debug, Clone)]
pub struct Keys {
    /// Host static key.
    pub host_static: [u8; 32],
    /// Host ephemeral key.
    pub host_ephemeral: [u8; 32],
    /// Client static key.
    pub client_static: [u8; 32],
    /// Client ephemeral key.
    pub client_ephemeral: [u8; 32],
}

fn key(from: u8) -> [u8; 32] {
    seq(from, 32).try_into().expect("32 bytes")
}

impl Keys {
    /// The keys every vector uses unless a case says otherwise.
    pub fn standard() -> Keys {
        Keys {
            host_static: key(0x90),
            host_ephemeral: key(0x50),
            client_static: key(0x70),
            client_ephemeral: key(0x30),
        }
    }

    /// The keys as they are written in a vector.
    pub fn json(&self) -> SessionKeys {
        SessionKeys {
            host_estatica: bytes(&self.host_static),
            host_efemera: bytes(&self.host_ephemeral),
            cliente_estatica: bytes(&self.client_static),
            cliente_efemera: bytes(&self.client_ephemeral),
        }
    }

    /// The host static pair.
    pub fn host_pair(&self) -> StaticKeypair {
        StaticKeypair::from_private(self.host_static)
    }

    /// The client static pair.
    pub fn client_pair(&self) -> StaticKeypair {
        StaticKeypair::from_private(self.client_static)
    }
}

/// The host id every conversation is addressed to.
pub fn fixed_host_id() -> HostId {
    host_id(&ROUTING_KEY)
}

/// BLAKE2s-256 of `data`, lowercase hex.
pub fn blake2s_hex(data: &[u8]) -> String {
    encode_hex(&Blake2s256::digest(data))
}

/// The plaintext of a record test: byte `i` is `i mod 251`.
pub fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Two transports linked by an `IK` handshake with the fixed keys:
/// `(client, host)`. Every call gives byte-identical transports, so a case can
/// keep one as the sender and one as an independent receiver.
pub fn ik_transports(keys: &Keys) -> (Transport, Transport) {
    let id = fixed_host_id();
    let client = keys.client_pair();
    let host = keys.host_pair();
    let (mut handshake, msg1) = ClientHandshake::connect_with_ephemeral(
        &client,
        &host.public(),
        id.as_str(),
        &keys.client_ephemeral,
    )
    .expect("the client starts");
    let HostStep::Connect {
        msg2,
        transport: host_transport,
    } = HostHandshake::respond_with_ephemeral(
        HandshakeKind::Connect,
        &host,
        id.as_str(),
        &msg1,
        &keys.host_ephemeral,
    )
    .expect("the host answers")
    else {
        panic!("IK must give HostStep::Connect");
    };
    let ClientStep::Established(client_transport) =
        handshake.read_message2(&msg2).expect("the client reads")
    else {
        panic!("IK is established after msg2");
    };
    (client_transport, host_transport)
}
