// SPDX-License-Identifier: GPL-3.0-or-later

//! The Noise handshakes of spec 02: `XX` for pairing (§3.2) and `IK` for
//! connections of an already paired device (§4), in both roles, plus the
//! transport state that the record layer builds on (§5).
//!
//! This layer knows nothing about CBOR, the relay envelope or clocks: it takes
//! and returns raw Noise messages. The `msg3` payload of the `XX` pattern (the
//! `pair-request`) is opaque bytes here.
//!
//! Rules enforced here, beyond what `snow` does by itself:
//!
//! - **Null DH result** (spec 02 §2): any DH operation whose output is 32 zero
//!   bytes fails the handshake with [`HandshakeError::DhZero`]. `snow` accepts
//!   such results by default, so the DH primitive is wrapped (see
//!   [`StrictResolver`]).
//! - **QR key check** (spec 02 §3.2, item 1): on pairing, the client compares
//!   the static key revealed by the host against the one from the QR code, in
//!   constant time, and writes no `msg3` on mismatch.
//! - **Empty payloads** in `msg1` and `msg2` (spec 02 §4): a non-empty payload
//!   there is rejected as malformed (the `IK` `msg1` payload has no forward
//!   secrecy, and the spec leaves them empty on purpose).

use core::fmt;

use snow::Builder;
use snow::HandshakeState;
use snow::TransportState;
use snow::error::StateProblem;
use snow::params::{CipherChoice, DHChoice, HashChoice, NoiseParams};
use snow::resolvers::{CryptoResolver, DefaultResolver};
use snow::types::{Cipher, Dh, Hash, Random};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use super::keys::StaticKeypair;
use crate::ct::ct_eq;

/// Noise protocol name of the pairing handshake.
pub const NOISE_XX: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// Noise protocol name of the handshake of an already paired device.
pub const NOISE_IK: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

/// Largest Noise message, in bytes (Noise spec §3).
pub const MAX_NOISE_MESSAGE: usize = 65_535;
/// Largest plaintext of one transport message: [`MAX_NOISE_MESSAGE`] minus the
/// 16-byte authentication tag.
pub const MAX_PLAINTEXT: usize = 65_519;

const TAG_LEN: usize = 16;
const PROLOGUE_PREFIX: &[u8] = b"porecatu-remote/0 ";

/// The Noise prologue of both handshakes: the ASCII bytes of
/// `"porecatu-remote/0 "` followed by the `host_id` (spec 02 §2).
#[must_use]
pub fn prologue(host_id: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(PROLOGUE_PREFIX.len() + host_id.len());
    out.extend_from_slice(PROLOGUE_PREFIX);
    out.extend_from_slice(host_id.as_bytes());
    out
}

/// Which handshake follows, as announced in `handshake-start` (spec 02 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HandshakeKind {
    /// `Noise_XX`: pairing of a new device.
    Pair = 0,
    /// `Noise_IK`: connection of a paired device.
    Connect = 1,
}

impl HandshakeKind {
    fn protocol(self) -> &'static str {
        match self {
            HandshakeKind::Pair => NOISE_XX,
            HandshakeKind::Connect => NOISE_IK,
        }
    }
}

/// Why a handshake or a transport operation failed.
///
/// Typed codes only: no prose and no `snow` types cross this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandshakeError {
    /// The static key revealed by the host is not the one from the QR code.
    HostKeyMismatch,
    /// A DH operation produced 32 zero bytes (low-order public key).
    DhZero,
    /// A Noise message failed to decrypt or authenticate (this includes a
    /// prologue mismatch).
    Decrypt,
    /// A message with the wrong length or a forbidden payload.
    Malformed,
    /// An operation called out of order, or on a handshake that already ended.
    WrongState,
    /// An outgoing message above the Noise limits.
    MessageTooLong,
    /// The 2^64 nonce space of a transport direction is exhausted.
    Exhausted,
    /// Any other failure inside the Noise implementation.
    Noise(NoiseFault),
}

/// Coarse class of an internal Noise failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NoiseFault {
    /// Pattern, protocol name or primitive setup.
    Setup,
    /// Missing key material for the chosen pattern.
    Prerequisite,
    /// The random generator failed.
    Rng,
    /// Unclassified.
    Other,
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            HandshakeError::HostKeyMismatch => "host static key does not match the QR code",
            HandshakeError::DhZero => "DH result is all zero",
            HandshakeError::Decrypt => "Noise message failed to decrypt",
            HandshakeError::Malformed => "malformed Noise message",
            HandshakeError::WrongState => "handshake called out of order",
            HandshakeError::MessageTooLong => "message too long for Noise",
            HandshakeError::Exhausted => "transport nonce space exhausted",
            HandshakeError::Noise(_) => "Noise implementation failure",
        };
        f.write_str(text)
    }
}

impl core::error::Error for HandshakeError {}

fn map_error(error: &snow::Error, input_means: HandshakeError) -> HandshakeError {
    match error {
        snow::Error::Dh => HandshakeError::DhZero,
        snow::Error::Decrypt => HandshakeError::Decrypt,
        snow::Error::Input => input_means,
        snow::Error::State(StateProblem::Exhausted) => HandshakeError::Exhausted,
        snow::Error::State(_) => HandshakeError::WrongState,
        snow::Error::Prereq(_) => HandshakeError::Noise(NoiseFault::Prerequisite),
        snow::Error::Pattern(_) | snow::Error::Init(_) => HandshakeError::Noise(NoiseFault::Setup),
        snow::Error::Rng => HandshakeError::Noise(NoiseFault::Rng),
        _ => HandshakeError::Noise(NoiseFault::Other),
    }
}

fn read_error(error: &snow::Error) -> HandshakeError {
    map_error(error, HandshakeError::Malformed)
}

fn write_error(error: &snow::Error) -> HandshakeError {
    map_error(error, HandshakeError::MessageTooLong)
}

// ---------------------------------------------------------------------------
// DH that refuses a null result
// ---------------------------------------------------------------------------

/// A DH primitive that fails when the shared secret is 32 zero bytes
/// (spec 02 §2). It delegates the arithmetic to the default X25519.
struct StrictDh(Box<dyn Dh>);

impl Dh for StrictDh {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn pub_len(&self) -> usize {
        self.0.pub_len()
    }

    fn priv_len(&self) -> usize {
        self.0.priv_len()
    }

    fn set(&mut self, privkey: &[u8]) {
        self.0.set(privkey);
    }

    fn generate(&mut self, rng: &mut dyn Random) -> Result<(), snow::Error> {
        self.0.generate(rng)
    }

    fn pubkey(&self) -> &[u8] {
        self.0.pubkey()
    }

    fn privkey(&self) -> &[u8] {
        self.0.privkey()
    }

    fn dh(&self, pubkey: &[u8], out: &mut [u8]) -> Result<(), snow::Error> {
        // The wrapped implementation asserts on short keys; refuse instead.
        if pubkey.len() < self.0.pub_len() {
            return Err(snow::Error::Input);
        }
        self.0.dh(pubkey, out)?;
        let len = self.0.dh_len();
        let zeros = [0u8; 64];
        let Some(zero) = zeros.get(..len) else {
            return Err(snow::Error::Dh);
        };
        let Some(secret) = out.get_mut(..len) else {
            return Err(snow::Error::Dh);
        };
        if bool::from(secret.ct_eq(zero)) {
            secret.zeroize();
            return Err(snow::Error::Dh);
        }
        Ok(())
    }

    fn dh_len(&self) -> usize {
        self.0.dh_len()
    }
}

/// The default `snow` resolver with the DH wrapped by [`StrictDh`].
struct StrictResolver;

impl CryptoResolver for StrictResolver {
    fn resolve_rng(&self) -> Option<Box<dyn Random>> {
        DefaultResolver.resolve_rng()
    }

    fn resolve_dh(&self, choice: &DHChoice) -> Option<Box<dyn Dh>> {
        DefaultResolver
            .resolve_dh(choice)
            .map(|dh| Box::new(StrictDh(dh)) as Box<dyn Dh>)
    }

    fn resolve_hash(&self, choice: &HashChoice) -> Option<Box<dyn Hash>> {
        DefaultResolver.resolve_hash(choice)
    }

    fn resolve_cipher(&self, choice: &CipherChoice) -> Option<Box<dyn Cipher>> {
        DefaultResolver.resolve_cipher(choice)
    }
}

/// Builds the `snow` builder shared by every handshake of this crate.
pub(crate) fn builder<'a>(
    protocol: &str,
    local: &'a StaticKeypair,
    remote_static: Option<&'a [u8; 32]>,
    prologue: &'a [u8],
    ephemeral: Option<&'a [u8; 32]>,
) -> Result<Builder<'a>, HandshakeError> {
    let params: NoiseParams = protocol
        .parse()
        .map_err(|_| HandshakeError::Noise(NoiseFault::Setup))?;
    let mut b = Builder::with_resolver(params, Box::new(StrictResolver))
        .prologue(prologue)
        .map_err(|e| read_error(&e))?
        .local_private_key(&local.expose_private()[..])
        .map_err(|e| read_error(&e))?;
    if let Some(remote) = remote_static {
        b = b
            .remote_public_key(&remote[..])
            .map_err(|e| read_error(&e))?;
    }
    if let Some(e) = ephemeral {
        b = b.fixed_ephemeral_key_for_testing_only(&e[..]);
    }
    Ok(b)
}

// ---------------------------------------------------------------------------
// Message helpers
// ---------------------------------------------------------------------------

fn write_message(state: &mut HandshakeState, payload: &[u8]) -> Result<Vec<u8>, HandshakeError> {
    let mut buf = vec![0u8; MAX_NOISE_MESSAGE];
    let len = state
        .write_message(payload, &mut buf)
        .map_err(|e| write_error(&e))?;
    buf.truncate(len);
    Ok(buf)
}

fn read_message(state: &mut HandshakeState, message: &[u8]) -> Result<Vec<u8>, HandshakeError> {
    if message.len() > MAX_NOISE_MESSAGE {
        return Err(HandshakeError::Malformed);
    }
    let mut buf = vec![0u8; message.len()];
    let len = state
        .read_message(message, &mut buf)
        .map_err(|e| read_error(&e))?;
    buf.truncate(len);
    Ok(buf)
}

/// Reads a message whose payload the spec requires to be empty.
fn read_empty_payload_message(
    state: &mut HandshakeState,
    message: &[u8],
) -> Result<(), HandshakeError> {
    if read_message(state, message)?.is_empty() {
        Ok(())
    } else {
        Err(HandshakeError::Malformed)
    }
}

fn remote_static_of(state: &HandshakeState) -> Option<[u8; 32]> {
    state
        .get_remote_static()
        .and_then(|key| <[u8; 32]>::try_from(key).ok())
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// The transport state after a completed handshake: one cipher per direction
/// (spec 02 §5). Records, fragmentation and the rekey schedule are built on
/// top of it by the record layer.
pub struct Transport {
    inner: TransportState,
    /// The handshake hash, kept only for the test vectors.
    #[cfg(any(test, feature = "test-vectors"))]
    handshake_hash: Vec<u8>,
}

impl Transport {
    #[cfg(test)]
    pub(crate) fn from_snow(inner: TransportState) -> Transport {
        Transport {
            inner,
            handshake_hash: Vec::new(),
        }
    }

    /// Finishes a handshake: the transport state, remembering the handshake
    /// hash for the test vectors.
    pub(crate) fn from_handshake(state: HandshakeState) -> Result<Transport, snow::Error> {
        #[cfg(any(test, feature = "test-vectors"))]
        let handshake_hash = state.get_handshake_hash().to_vec();
        let inner = state.into_transport_mode()?;
        Ok(Transport {
            inner,
            #[cfg(any(test, feature = "test-vectors"))]
            handshake_hash,
        })
    }

    /// The handshake hash (`h` of the Noise spec §5.2) of the handshake that
    /// produced this transport, for test vectors only.
    #[cfg(any(test, feature = "test-vectors"))]
    #[must_use]
    pub fn handshake_hash(&self) -> &[u8] {
        &self.handshake_hash
    }

    /// Encrypts one transport message. `plaintext` is at most
    /// [`MAX_PLAINTEXT`] bytes; the result is at most [`MAX_NOISE_MESSAGE`].
    ///
    /// # Errors
    ///
    /// [`HandshakeError::MessageTooLong`] above the limit,
    /// [`HandshakeError::Exhausted`] when the nonce space ends.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, HandshakeError> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(HandshakeError::MessageTooLong);
        }
        let mut buf = vec![0u8; plaintext.len() + TAG_LEN];
        let len = self
            .inner
            .write_message(plaintext, &mut buf)
            .map_err(|e| write_error(&e))?;
        buf.truncate(len);
        Ok(buf)
    }

    /// Decrypts one transport message.
    ///
    /// # Errors
    ///
    /// [`HandshakeError::Decrypt`] when authentication fails (the caller must
    /// close the session: there is no resend and no nonce skip, spec 02 §5),
    /// [`HandshakeError::Malformed`] above [`MAX_NOISE_MESSAGE`].
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, HandshakeError> {
        if ciphertext.len() > MAX_NOISE_MESSAGE {
            return Err(HandshakeError::Malformed);
        }
        if ciphertext.len() < TAG_LEN {
            return Err(HandshakeError::Decrypt);
        }
        let mut buf = vec![0u8; ciphertext.len()];
        let len = self
            .inner
            .read_message(ciphertext, &mut buf)
            .map_err(|e| read_error(&e))?;
        buf.truncate(len);
        Ok(buf)
    }

    /// `Rekey()` on the sending cipher (Noise spec §11.3).
    pub fn rekey_outgoing(&mut self) {
        self.inner.rekey_outgoing();
    }

    /// `Rekey()` on the receiving cipher.
    pub fn rekey_incoming(&mut self) {
        self.inner.rekey_incoming();
    }

    /// The static key of the other side, when the pattern revealed it.
    #[must_use]
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.inner
            .get_remote_static()
            .and_then(|key| <[u8; 32]>::try_from(key).ok())
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transport").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// What the client does after reading `msg2`.
#[derive(Debug)]
pub enum ClientStep {
    /// `IK` finished: the session is established.
    Established(Transport),
    /// `XX`: the host key matched the QR; call
    /// [`ClientHandshake::write_message3`] with the `pair-request`.
    NeedsPairRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientStage {
    AwaitMsg2,
    AwaitPairRequest,
    Ended,
}

/// The initiating side of both handshakes (the Porecatu Remote app).
pub struct ClientHandshake {
    kind: HandshakeKind,
    stage: ClientStage,
    state: Option<HandshakeState>,
    expected_host_key: [u8; 32],
}

impl ClientHandshake {
    /// Starts the `XX` pairing handshake and returns `msg1`.
    ///
    /// `expected_host_key` is the `key` of the QR code; it is checked against
    /// the key the host reveals in `msg2`.
    ///
    /// # Errors
    ///
    /// Setup failures only.
    pub fn pair(
        local: &StaticKeypair,
        expected_host_key: &[u8; 32],
        host_id: &str,
    ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError> {
        Self::start(HandshakeKind::Pair, local, expected_host_key, host_id, None)
    }

    /// Starts the `IK` handshake of a paired device and returns `msg1`.
    ///
    /// # Errors
    ///
    /// [`HandshakeError::DhZero`] when `host_key` is a low-order point.
    pub fn connect(
        local: &StaticKeypair,
        host_key: &[u8; 32],
        host_id: &str,
    ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError> {
        Self::start(HandshakeKind::Connect, local, host_key, host_id, None)
    }

    /// Like [`pair`](Self::pair) with a fixed ephemeral key, for test vectors.
    ///
    /// # Errors
    ///
    /// As [`pair`](Self::pair).
    #[cfg(any(test, feature = "test-vectors"))]
    pub fn pair_with_ephemeral(
        local: &StaticKeypair,
        expected_host_key: &[u8; 32],
        host_id: &str,
        ephemeral: &[u8; 32],
    ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError> {
        Self::start(
            HandshakeKind::Pair,
            local,
            expected_host_key,
            host_id,
            Some(ephemeral),
        )
    }

    /// Like [`connect`](Self::connect) with a fixed ephemeral key, for test
    /// vectors.
    ///
    /// # Errors
    ///
    /// As [`connect`](Self::connect).
    #[cfg(any(test, feature = "test-vectors"))]
    pub fn connect_with_ephemeral(
        local: &StaticKeypair,
        host_key: &[u8; 32],
        host_id: &str,
        ephemeral: &[u8; 32],
    ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError> {
        Self::start(
            HandshakeKind::Connect,
            local,
            host_key,
            host_id,
            Some(ephemeral),
        )
    }

    fn start(
        kind: HandshakeKind,
        local: &StaticKeypair,
        host_key: &[u8; 32],
        host_id: &str,
        ephemeral: Option<&[u8; 32]>,
    ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError> {
        let prologue = prologue(host_id);
        // The XX initiator does not know the host key to the Noise layer: the
        // QR check is ours, after msg2.
        let remote = match kind {
            HandshakeKind::Pair => None,
            HandshakeKind::Connect => Some(host_key),
        };
        let mut state = builder(kind.protocol(), local, remote, &prologue, ephemeral)?
            .build_initiator()
            .map_err(|e| read_error(&e))?;
        let msg1 = write_message(&mut state, &[])?;
        let client = ClientHandshake {
            kind,
            stage: ClientStage::AwaitMsg2,
            state: Some(state),
            expected_host_key: *host_key,
        };
        Ok((client, msg1))
    }

    /// Reads `msg2`.
    ///
    /// On `IK` the handshake ends here ([`ClientStep::Established`]). On `XX`
    /// the static key the host revealed is compared with the QR key in
    /// constant time: a difference is [`HandshakeError::HostKeyMismatch`] and
    /// no `msg3` can be written afterwards.
    ///
    /// Any error ends the handshake: the client sends nothing more.
    ///
    /// # Errors
    ///
    /// [`HandshakeError::Decrypt`] (including prologue mismatch),
    /// [`HandshakeError::DhZero`], [`HandshakeError::HostKeyMismatch`],
    /// [`HandshakeError::Malformed`] or [`HandshakeError::WrongState`].
    pub fn read_message2(&mut self, msg2: &[u8]) -> Result<ClientStep, HandshakeError> {
        if self.stage != ClientStage::AwaitMsg2 {
            return Err(HandshakeError::WrongState);
        }
        // Whatever happens next, this handshake does not go back.
        self.stage = ClientStage::Ended;
        let mut state = self.state.take().ok_or(HandshakeError::WrongState)?;
        read_empty_payload_message(&mut state, msg2)?;
        match self.kind {
            HandshakeKind::Connect => {
                let transport = Transport::from_handshake(state).map_err(|e| read_error(&e))?;
                Ok(ClientStep::Established(transport))
            }
            HandshakeKind::Pair => {
                let revealed = remote_static_of(&state).ok_or(HandshakeError::WrongState)?;
                if !ct_eq(&revealed, &self.expected_host_key) {
                    return Err(HandshakeError::HostKeyMismatch);
                }
                self.state = Some(state);
                self.stage = ClientStage::AwaitPairRequest;
                Ok(ClientStep::NeedsPairRequest)
            }
        }
    }

    /// Writes `msg3` of the `XX` handshake, carrying the opaque `pair-request`,
    /// and returns it with the established [`Transport`].
    ///
    /// # Errors
    ///
    /// [`HandshakeError::WrongState`] unless [`read_message2`](Self::read_message2)
    /// returned [`ClientStep::NeedsPairRequest`];
    /// [`HandshakeError::MessageTooLong`] for an oversized payload.
    pub fn write_message3(
        mut self,
        pair_request_payload: &[u8],
    ) -> Result<(Vec<u8>, Transport), HandshakeError> {
        if self.stage != ClientStage::AwaitPairRequest {
            return Err(HandshakeError::WrongState);
        }
        let mut state = self.state.take().ok_or(HandshakeError::WrongState)?;
        let msg3 = write_message(&mut state, pair_request_payload)?;
        let transport = Transport::from_handshake(state).map_err(|e| write_error(&e))?;
        Ok((msg3, transport))
    }
}

impl fmt::Debug for ClientHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientHandshake")
            .field("kind", &self.kind)
            .field("stage", &self.stage)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------

/// What the host does after reading `msg1`.
#[derive(Debug)]
pub enum HostStep {
    /// `IK`: the handshake is complete. Send `msg2`; the device key is
    /// `transport.remote_static()`. The caller decides afterwards whether the
    /// device is paired (spec 02 §4: the handshake always completes first).
    Connect {
        /// The message to send to the client.
        msg2: Vec<u8>,
        /// The established transport.
        transport: Transport,
    },
    /// `XX`: send `msg2`, then feed the client's `msg3` to
    /// [`HostHandshake::read_message3`].
    Pair {
        /// The message to send to the client.
        msg2: Vec<u8>,
        /// The handshake awaiting `msg3`.
        pending: HostHandshake,
    },
}

/// The responding side (the desktop). As a value it is the `XX` handshake
/// waiting for `msg3`; [`HostHandshake::respond`] creates it.
pub struct HostHandshake {
    state: Box<HandshakeState>,
}

impl HostHandshake {
    /// Reads `msg1` (the `payload` of the `handshake-start`, already split
    /// from the `kind`) and answers per `kind`.
    ///
    /// # Errors
    ///
    /// Any error is a handshake failure: the caller closes the channel with
    /// `host-rejected` (spec 02 §2).
    pub fn respond(
        kind: HandshakeKind,
        local: &StaticKeypair,
        host_id: &str,
        msg1: &[u8],
    ) -> Result<HostStep, HandshakeError> {
        Self::respond_inner(kind, local, host_id, msg1, None)
    }

    /// Like [`respond`](Self::respond) with a fixed ephemeral key, for test
    /// vectors.
    ///
    /// # Errors
    ///
    /// As [`respond`](Self::respond).
    #[cfg(any(test, feature = "test-vectors"))]
    pub fn respond_with_ephemeral(
        kind: HandshakeKind,
        local: &StaticKeypair,
        host_id: &str,
        msg1: &[u8],
        ephemeral: &[u8; 32],
    ) -> Result<HostStep, HandshakeError> {
        Self::respond_inner(kind, local, host_id, msg1, Some(ephemeral))
    }

    fn respond_inner(
        kind: HandshakeKind,
        local: &StaticKeypair,
        host_id: &str,
        msg1: &[u8],
        ephemeral: Option<&[u8; 32]>,
    ) -> Result<HostStep, HandshakeError> {
        let prologue = prologue(host_id);
        let mut state = builder(kind.protocol(), local, None, &prologue, ephemeral)?
            .build_responder()
            .map_err(|e| read_error(&e))?;
        read_empty_payload_message(&mut state, msg1)?;
        let msg2 = write_message(&mut state, &[])?;
        match kind {
            HandshakeKind::Connect => {
                let transport = Transport::from_handshake(state).map_err(|e| write_error(&e))?;
                Ok(HostStep::Connect { msg2, transport })
            }
            HandshakeKind::Pair => Ok(HostStep::Pair {
                msg2,
                pending: HostHandshake {
                    state: Box::new(state),
                },
            }),
        }
    }

    /// Reads `msg3` of the `XX` handshake. Returns the opaque `pair-request`
    /// payload, the device static key and the established [`Transport`].
    ///
    /// # Errors
    ///
    /// [`HandshakeError::Decrypt`], [`HandshakeError::DhZero`] or
    /// [`HandshakeError::Malformed`].
    pub fn read_message3(
        mut self,
        msg3: &[u8],
    ) -> Result<(Vec<u8>, [u8; 32], Transport), HandshakeError> {
        let payload = read_message(&mut self.state, msg3)?;
        let remote = remote_static_of(&self.state).ok_or(HandshakeError::WrongState)?;
        let transport = Transport::from_handshake(*self.state).map_err(|e| read_error(&e))?;
        Ok((payload, remote, transport))
    }
}

impl fmt::Debug for HostHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostHandshake").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use data_encoding::HEXLOWER;

    use super::*;

    const HOST_ID: &str = "k3x9testhostidentifier";

    fn pairs() -> (StaticKeypair, StaticKeypair) {
        (StaticKeypair::generate(), StaticKeypair::generate())
    }

    fn established_pair(
        client_keys: &StaticKeypair,
        host_keys: &StaticKeypair,
    ) -> (Transport, Transport) {
        let (client, msg1) = ClientHandshake::pair(client_keys, &host_keys.public(), HOST_ID)
            .expect("client starts");
        let HostStep::Pair { msg2, pending } =
            HostHandshake::respond(HandshakeKind::Pair, host_keys, HOST_ID, &msg1)
                .expect("host answers")
        else {
            panic!("pair must yield HostStep::Pair");
        };
        let mut client = client;
        assert!(matches!(
            client.read_message2(&msg2),
            Ok(ClientStep::NeedsPairRequest)
        ));
        let (msg3, client_transport) = client.write_message3(b"req").expect("msg3");
        let (_, _, host_transport) = pending.read_message3(&msg3).expect("host reads msg3");
        (client_transport, host_transport)
    }

    /// Three messages each way, then a rekey on both directions, then more.
    fn exercise_transport(client: &mut Transport, host: &mut Transport) {
        for i in 0u8..3 {
            let sent = client.encrypt(&[i; 40]).unwrap();
            assert_eq!(host.decrypt(&sent).unwrap(), vec![i; 40]);
            let sent = host.encrypt(&[i + 10; 7]).unwrap();
            assert_eq!(client.decrypt(&sent).unwrap(), vec![i + 10; 7]);
        }

        // Rekey: sender calls rekey_outgoing after a record, receiver
        // rekey_incoming after decrypting it.
        let sent = client.encrypt(b"before rekey").unwrap();
        client.rekey_outgoing();
        assert_eq!(host.decrypt(&sent).unwrap(), b"before rekey");
        host.rekey_incoming();

        let sent = host.encrypt(b"host before rekey").unwrap();
        host.rekey_outgoing();
        assert_eq!(client.decrypt(&sent).unwrap(), b"host before rekey");
        client.rekey_incoming();

        let sent = client.encrypt(b"after rekey").unwrap();
        assert_eq!(host.decrypt(&sent).unwrap(), b"after rekey");
        let sent = host.encrypt(b"host after rekey").unwrap();
        assert_eq!(client.decrypt(&sent).unwrap(), b"host after rekey");
    }

    #[test]
    fn prologue_is_the_documented_bytes() {
        assert_eq!(prologue("abc"), b"porecatu-remote/0 abc");
    }

    #[test]
    fn xx_pairing_round_trip() {
        let (client_keys, host_keys) = pairs();
        let (client, msg1) =
            ClientHandshake::pair(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let HostStep::Pair { msg2, pending } =
            HostHandshake::respond(HandshakeKind::Pair, &host_keys, HOST_ID, &msg1).unwrap()
        else {
            panic!("pair must yield HostStep::Pair");
        };
        let mut client = client;
        assert!(matches!(
            client.read_message2(&msg2).unwrap(),
            ClientStep::NeedsPairRequest
        ));
        let (msg3, mut client_transport) = client.write_message3(b"pair-request bytes").unwrap();
        let (payload, remote, mut host_transport) = pending.read_message3(&msg3).unwrap();

        assert_eq!(payload, b"pair-request bytes");
        assert_eq!(remote, client_keys.public());
        assert_eq!(host_transport.remote_static(), Some(client_keys.public()));
        assert_eq!(client_transport.remote_static(), Some(host_keys.public()));
        exercise_transport(&mut client_transport, &mut host_transport);
    }

    #[test]
    fn ik_connection_round_trip() {
        let (client_keys, host_keys) = pairs();
        let (mut client, msg1) =
            ClientHandshake::connect(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let HostStep::Connect {
            msg2,
            transport: mut host_transport,
        } = HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &msg1).unwrap()
        else {
            panic!("connect must yield HostStep::Connect");
        };
        assert_eq!(host_transport.remote_static(), Some(client_keys.public()));

        let ClientStep::Established(mut client_transport) = client.read_message2(&msg2).unwrap()
        else {
            panic!("IK must be established after msg2");
        };
        assert_eq!(client_transport.remote_static(), Some(host_keys.public()));
        exercise_transport(&mut client_transport, &mut host_transport);
    }

    #[test]
    fn calls_out_of_order_are_wrong_state() {
        let (client_keys, host_keys) = pairs();
        let (mut client, msg1) =
            ClientHandshake::connect(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let HostStep::Connect { msg2, .. } =
            HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &msg1).unwrap()
        else {
            panic!("connect must yield HostStep::Connect");
        };
        assert!(client.read_message2(&msg2).is_ok());
        assert_eq!(
            client.read_message2(&msg2).unwrap_err(),
            HandshakeError::WrongState
        );
        // msg3 before msg2 on a fresh XX handshake.
        let (client, _) =
            ClientHandshake::pair(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        assert_eq!(
            client.write_message3(b"x").unwrap_err(),
            HandshakeError::WrongState
        );
    }

    #[test]
    fn wrong_qr_key_is_host_key_mismatch_and_writes_no_msg3() {
        let (client_keys, host_keys) = pairs();
        let impostor = StaticKeypair::generate();
        // The client expects `impostor`'s key; the real host answers.
        let (mut client, msg1) =
            ClientHandshake::pair(&client_keys, &impostor.public(), HOST_ID).unwrap();
        let HostStep::Pair { msg2, .. } =
            HostHandshake::respond(HandshakeKind::Pair, &host_keys, HOST_ID, &msg1).unwrap()
        else {
            panic!("pair must yield HostStep::Pair");
        };
        assert_eq!(
            client.read_message2(&msg2).unwrap_err(),
            HandshakeError::HostKeyMismatch
        );
        // No way to produce msg3 from here.
        assert_eq!(
            client.write_message3(b"token").unwrap_err(),
            HandshakeError::WrongState
        );
    }

    #[test]
    fn different_host_id_fails_the_handshake() {
        let (client_keys, host_keys) = pairs();

        // IK: the host fails on msg1 (es already depends on the prologue).
        let (_, msg1) =
            ClientHandshake::connect(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let err = HostHandshake::respond(HandshakeKind::Connect, &host_keys, "otherhostid", &msg1)
            .unwrap_err();
        assert_eq!(err, HandshakeError::Decrypt);

        // XX: msg1 carries only `e`, so the host answers; the client then
        // fails to decrypt msg2.
        let (mut client, msg1) =
            ClientHandshake::pair(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let HostStep::Pair { msg2, .. } =
            HostHandshake::respond(HandshakeKind::Pair, &host_keys, "otherhostid", &msg1).unwrap()
        else {
            panic!("pair must yield HostStep::Pair");
        };
        assert_eq!(
            client.read_message2(&msg2).unwrap_err(),
            HandshakeError::Decrypt
        );
    }

    #[test]
    fn low_order_host_key_is_dh_zero() {
        let client_keys = StaticKeypair::generate();
        let err = ClientHandshake::connect(&client_keys, &[0u8; 32], HOST_ID).unwrap_err();
        assert_eq!(err, HandshakeError::DhZero);
    }

    #[test]
    fn low_order_ephemeral_in_msg2_is_dh_zero() {
        let (client_keys, host_keys) = pairs();
        let (mut client, _) =
            ClientHandshake::pair(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        // msg2 of XX = e (32) + encrypted s (48) + payload tag (16).
        let forged = [0u8; 96];
        assert_eq!(
            client.read_message2(&forged).unwrap_err(),
            HandshakeError::DhZero
        );
    }

    #[test]
    fn low_order_ephemeral_in_ik_msg1_is_dh_zero() {
        let host_keys = StaticKeypair::generate();
        // msg1 of IK = e (32) + encrypted s (48) + payload tag (16).
        let forged = [0u8; 96];
        let err = HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &forged)
            .unwrap_err();
        assert_eq!(err, HandshakeError::DhZero);
    }

    #[test]
    fn non_empty_payload_in_msg1_is_rejected() {
        let (client_keys, host_keys) = pairs();
        let prologue = prologue(HOST_ID);
        let host_public = host_keys.public();
        let mut raw = builder(NOISE_IK, &client_keys, Some(&host_public), &prologue, None)
            .unwrap()
            .build_initiator()
            .unwrap();
        let msg1 = write_message(&mut raw, b"not empty").unwrap();
        let err =
            HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &msg1).unwrap_err();
        assert_eq!(err, HandshakeError::Malformed);
    }

    #[test]
    fn truncated_messages_are_malformed() {
        let host_keys = StaticKeypair::generate();
        for msg1 in [&[][..], &[1u8; 5][..]] {
            let err =
                HostHandshake::respond(HandshakeKind::Pair, &host_keys, HOST_ID, msg1).unwrap_err();
            assert_eq!(err, HandshakeError::Malformed);
        }
        let too_big = vec![0u8; MAX_NOISE_MESSAGE + 1];
        let err = HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &too_big)
            .unwrap_err();
        assert_eq!(err, HandshakeError::Malformed);
    }

    #[test]
    fn transport_limits() {
        let (client_keys, host_keys) = pairs();
        let (mut client, mut host) = established_pair(&client_keys, &host_keys);

        let max = vec![0xAB; MAX_PLAINTEXT];
        let sealed = client.encrypt(&max).unwrap();
        assert_eq!(sealed.len(), MAX_NOISE_MESSAGE);
        assert_eq!(host.decrypt(&sealed).unwrap(), max);

        assert_eq!(
            client.encrypt(&vec![0u8; MAX_PLAINTEXT + 1]).unwrap_err(),
            HandshakeError::MessageTooLong
        );
        assert_eq!(client.encrypt(&[]).unwrap().len(), 16);

        assert_eq!(
            host.decrypt(&[0u8; 5]).unwrap_err(),
            HandshakeError::Decrypt
        );
        assert_eq!(
            host.decrypt(&vec![0u8; MAX_NOISE_MESSAGE + 1]).unwrap_err(),
            HandshakeError::Malformed
        );
    }

    #[test]
    fn tampered_transport_message_fails_to_decrypt() {
        let (client_keys, host_keys) = pairs();
        let (mut client, mut host) = established_pair(&client_keys, &host_keys);
        let mut sealed = client.encrypt(b"hello").unwrap();
        sealed[0] ^= 1;
        assert_eq!(host.decrypt(&sealed).unwrap_err(), HandshakeError::Decrypt);
    }

    #[test]
    fn missing_rekey_on_one_side_breaks_decryption() {
        let (client_keys, host_keys) = pairs();
        let (mut client, mut host) = established_pair(&client_keys, &host_keys);
        client.rekey_outgoing();
        let sealed = client.encrypt(b"x").unwrap();
        assert_eq!(host.decrypt(&sealed).unwrap_err(), HandshakeError::Decrypt);
    }

    // ----- cacophony vectors ------------------------------------------------
    //
    // Source: the `cacophony` test vectors shipped with `snow` 0.10.0
    // (`tests/vectors/cacophony.txt`, MIT OR Apache-2.0), cases
    // `Noise_XX_25519_ChaChaPoly_BLAKE2s` and `Noise_IK_25519_ChaChaPoly_BLAKE2s`.
    // They drive the strict resolver through `builder`, so a null-DH wrapper
    // that changed any output would show here. Their handshake payloads are
    // not empty, hence `snow` is driven directly rather than through the
    // wrappers above (which enforce the empty payloads of spec 02).

    const VECTOR_PROLOGUE: &str = "4a6f686e2047616c74";
    const VECTOR_INIT_STATIC: &str =
        "e61ef9919cde45dd5f82166404bd08e38bceb5dfdfded0a34c8df7ed542214d1";
    const VECTOR_INIT_EPHEMERAL: &str =
        "893e28b9dc6ca8d611ab664754b8ceb7bac5117349a4439a6b0569da977c464a";
    const VECTOR_RESP_STATIC: &str =
        "4a3acbfdb163dec651dfa3194dece676d437029c62a408b4c5ea9114246e4893";
    const VECTOR_RESP_EPHEMERAL: &str =
        "bbdb4cdbd309f1a1f2e1456967fe288cadd6f712d65dc7b7793d5e63da6b375b";
    const VECTOR_INIT_REMOTE_STATIC_IK: &str =
        "31e0303fd6418d2f8c0e78b91f22e8caed0fbe48656dcf4767e4834f701b8f62";

    const PAYLOADS: [&str; 6] = [
        "4c756477696720766f6e204d69736573",
        "4d757272617920526f746862617264",
        "462e20412e20486179656b",
        "4361726c204d656e676572",
        "4a65616e2d426170746973746520536179",
        "457567656e2042f6686d20766f6e2042617765726b",
    ];

    const XX_HASH: &str = "6c4c56cf71612f72d05ceb96c0155e6f4ea54a26b504c93de632a2db4a49d200";
    const XX_CIPHERTEXTS: [&str; 6] = [
        "ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c79444c756477696720766f6e204d69736573",
        "95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f1448088437c365eb362a1c991b0557fe8a7fb187d99346765d93ec63db6c1b01504ebeec55a2298d2dbff80eff034d20595153f63a196a6cead1e11b2bb13e336fa13616dd3e8b0a070c882ed3f1a78c7c06c93",
        "46c3307de83b014258717d97781c1f50936d8b7d50c0722a1739654d10392d415b670c114f79b9a4f80541570f77ce88802efa4220cff733e7b5668ba38059ec904b4b8eef9448085faf51",
        "d5e83adfaac5dc324a68f1862df54549e56d209fba707205f328b2",
        "d102c9029b1f55c788f561ba7737afbccef9c9f1bf2f238167fd40ba9c1c134867",
        "cb1ce80960382c6d5d5e740ffb724d1432f0310b200fb6f8424120f506092744baa415e155",
    ];

    const IK_HASH: &str = "48f3cb8bc9319da4ba1e9933991b1c4ed4034f1f126a76d3a1fbcfd7f94248d4";
    const IK_CIPHERTEXTS: [&str; 6] = [
        "ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c79440b03ddc7aac5123d06a1b23b71670e32e76c28239a7ca4ac8f784de7e44c1adbfc6e83fef7352a58d9d56157400c0a737b1d171ce368229c7b752ac25b8faf4eca690f6d896f543be02c996ab2b86b76",
        "95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f144808843d9b5a8927f0ac9655ef76833bc7e5561f42e691ac8404efd6fbd6308b6a27c",
        "2c256ed08fcd08c2980f954ee4beaccb61c9581340f5dd2fd1cf3b",
        "d6033f70eee20945c7c9dba304e397ee3b284ff5e00fd9efb095d3",
        "a9c068ca5d8babf72560652d8e851adbfac35c8a66e810d560863173e96adf4cfe",
        "2a09d8f459e5927e40fdd2eddc99bdafb04e13a26f145cb5cfe9e6ba34c94331ebc17d5156",
    ];

    fn hex32(text: &str) -> [u8; 32] {
        let bytes = HEXLOWER.decode(text.as_bytes()).unwrap();
        <[u8; 32]>::try_from(bytes.as_slice()).unwrap()
    }

    fn run_vector(
        protocol: &str,
        handshake_messages: usize,
        init_remote_static: Option<[u8; 32]>,
        expected_hash: &str,
        ciphertexts: &[&str; 6],
    ) {
        let prologue = HEXLOWER.decode(VECTOR_PROLOGUE.as_bytes()).unwrap();
        let init_static = StaticKeypair::from_private(hex32(VECTOR_INIT_STATIC));
        let resp_static = StaticKeypair::from_private(hex32(VECTOR_RESP_STATIC));
        let init_e = hex32(VECTOR_INIT_EPHEMERAL);
        let resp_e = hex32(VECTOR_RESP_EPHEMERAL);

        let mut init = builder(
            protocol,
            &init_static,
            init_remote_static.as_ref(),
            &prologue,
            Some(&init_e),
        )
        .unwrap()
        .build_initiator()
        .unwrap();
        let mut resp = builder(protocol, &resp_static, None, &prologue, Some(&resp_e))
            .unwrap()
            .build_responder()
            .unwrap();

        let mut buf = vec![0u8; MAX_NOISE_MESSAGE];
        let mut out = vec![0u8; MAX_NOISE_MESSAGE];
        for i in 0..handshake_messages {
            let payload = HEXLOWER.decode(PAYLOADS[i].as_bytes()).unwrap();
            let (send, recv) = if i % 2 == 0 {
                (&mut init, &mut resp)
            } else {
                (&mut resp, &mut init)
            };
            let len = send.write_message(&payload, &mut buf).unwrap();
            assert_eq!(HEXLOWER.encode(&buf[..len]), ciphertexts[i], "message {i}");
            let got = recv.read_message(&buf[..len], &mut out).unwrap();
            assert_eq!(&out[..got], &payload[..], "payload {i}");
        }
        assert!(init.is_handshake_finished() && resp.is_handshake_finished());
        assert_eq!(HEXLOWER.encode(init.get_handshake_hash()), expected_hash);
        assert_eq!(HEXLOWER.encode(resp.get_handshake_hash()), expected_hash);

        let mut init_t = Transport::from_snow(init.into_transport_mode().unwrap());
        let mut resp_t = Transport::from_snow(resp.into_transport_mode().unwrap());
        for i in handshake_messages..6 {
            let payload = HEXLOWER.decode(PAYLOADS[i].as_bytes()).unwrap();
            let (send, recv) = if i % 2 == 0 {
                (&mut init_t, &mut resp_t)
            } else {
                (&mut resp_t, &mut init_t)
            };
            let sealed = send.encrypt(&payload).unwrap();
            assert_eq!(HEXLOWER.encode(&sealed), ciphertexts[i], "message {i}");
            assert_eq!(recv.decrypt(&sealed).unwrap(), payload, "payload {i}");
        }
    }

    #[test]
    fn cacophony_noise_xx_25519_chachapoly_blake2s() {
        run_vector(NOISE_XX, 3, None, XX_HASH, &XX_CIPHERTEXTS);
    }

    #[test]
    fn cacophony_noise_ik_25519_chachapoly_blake2s() {
        run_vector(
            NOISE_IK,
            2,
            Some(hex32(VECTOR_INIT_REMOTE_STATIC_IK)),
            IK_HASH,
            &IK_CIPHERTEXTS,
        );
    }

    #[test]
    fn fixed_ephemeral_makes_msg1_deterministic() {
        let (client_keys, host_keys) = pairs();
        let eph = [9u8; 32];
        let (_, a) =
            ClientHandshake::pair_with_ephemeral(&client_keys, &host_keys.public(), HOST_ID, &eph)
                .unwrap();
        let (_, b) =
            ClientHandshake::pair_with_ephemeral(&client_keys, &host_keys.public(), HOST_ID, &eph)
                .unwrap();
        assert_eq!(a, b);
        let (_, c) = ClientHandshake::connect_with_ephemeral(
            &client_keys,
            &host_keys.public(),
            HOST_ID,
            &eph,
        )
        .unwrap();
        let HostStep::Connect { .. } = HostHandshake::respond_with_ephemeral(
            HandshakeKind::Connect,
            &host_keys,
            HOST_ID,
            &c,
            &[7u8; 32],
        )
        .unwrap() else {
            panic!("connect must yield HostStep::Connect");
        };
    }
}
