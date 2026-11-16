// SPDX-License-Identifier: GPL-3.0-or-later
//! Relay envelope: host and client frame families.
//!
//! Specification 01 §4 and 02 §2 (`handshake-start`). Every WebSocket binary
//! frame between a peer and the relay carries exactly one deterministic CBOR
//! array whose first element is the type. This module is always compiled: the
//! relay depends on it and on nothing else of the crate.
//!
//! Decoding is hand-written over the frame so that a `channel-data` payload
//! is returned as a zero-copy slice of the frame ([`Bytes::slice`]). It is as
//! strict as `cbor::decode` (specification 03 §1.1): shortest-form integers
//! and lengths, no indefinite lengths, no tags or floats, integers within
//! `0 ..= 2^53 - 1`, exact arity, nothing after the item.
//!
//! Payloads are Noise records and are opaque here. [`Payload`] has no
//! `Debug`, `Display` or `Serialize`, and the `Debug` of the envelope enums
//! shows only the payload length, so a payload cannot reach a log by accident.

use core::fmt;
use core::ops::Range;

use bytes::Bytes;

use crate::cbor::{CborError, MAX_SAFE_INT};
use crate::host_id::{HostId, HostIdError};

/// Version segment of the relay paths (`/v0/host`, `/v0/client/<host_id>`).
pub const PATH_VERSION: &str = "v0";
/// Largest WebSocket frame either side may send (01 §1, §6).
pub const MAX_FRAME: usize = 70_000;
/// Floor of simultaneous channels per host a conforming relay supports (01 §6).
pub const MIN_CHANNELS_PER_HOST: usize = 8;
/// Floor of sustained bandwidth per channel and direction, in bit/s (01 §6).
pub const MIN_CHANNEL_BANDWIDTH_BPS: u64 = 2_000_000;
/// Time the host has, since the upgrade, to complete registration (01 §3).
pub const REGISTER_TIMEOUT_MS: u64 = 10_000;
/// Interval of the relay's WebSocket `Ping` (01 §1).
pub const PING_INTERVAL_MS: u64 = 25_000;
/// Time without a `Pong` after which the relay drops the connection (01 §1).
pub const PONG_TIMEOUT_MS: u64 = 60_000;
/// Context the host signs together with the challenge nonce (01 §3).
pub const REGISTER_CONTEXT: &[u8] = b"porecatu-relay/v0/register";
/// Length of the registration challenge nonce.
pub const NONCE_LEN: usize = 32;
/// Length of a routing public key (Ed25519).
pub const ROUTING_KEY_LEN: usize = 32;
/// Length of a registration signature (Ed25519).
pub const SIGNATURE_LEN: usize = 64;
/// Length of [`register_message`]: context followed by the nonce.
pub const REGISTER_MESSAGE_LEN: usize = 26 + NONCE_LEN;

/// WebSocket close codes the relay uses (01 §4, §5).
pub mod close_code {
    /// Normal closure.
    pub const NORMAL: u16 = 1000;
    /// The relay is shutting down.
    pub const GOING_AWAY: u16 = 1001;
    /// A text frame was received; only binary frames are used.
    pub const UNSUPPORTED_DATA: u16 = 1003;
    /// A frame larger than the maximum.
    pub const MESSAGE_TOO_BIG: u16 = 1009;
    /// Host registration refused (invalid signature).
    pub const REGISTER_REJECTED: u16 = 4001;
    /// Replaced by a newer registration of the same `host_id`.
    pub const REPLACED: u16 = 4002;
    /// Relay limit exceeded.
    pub const LIMIT_EXCEEDED: u16 = 4003;
    /// Invalid envelope.
    pub const INVALID_ENVELOPE: u16 = 4004;
    /// Host registration not completed in time.
    pub const REGISTER_TIMEOUT: u16 = 4005;
}

/// The bytes the host signs to register: [`REGISTER_CONTEXT`] followed by the
/// challenge nonce.
pub fn register_message(nonce: &[u8; NONCE_LEN]) -> [u8; REGISTER_MESSAGE_LEN] {
    let mut message = [0u8; REGISTER_MESSAGE_LEN];
    let (context, tail) = message.split_at_mut(REGISTER_CONTEXT.len());
    context.copy_from_slice(REGISTER_CONTEXT);
    tail.copy_from_slice(nonce);
    message
}

/// Relay-assigned identifier of a channel, unique among the live channels of
/// one host. Decoders reject values above `2^53 - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(pub u64);

/// Why a channel closed (`close-reason`, 01 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseReason {
    /// The other side closed.
    Normal = 0,
    /// The host registered again.
    HostReplaced = 1,
    /// The host's connection dropped.
    HostGone = 2,
    /// The host refused the channel.
    HostRejected = 3,
    /// A relay limit (channels per host, bandwidth, size).
    LimitExceeded = 4,
    /// Invalid envelope.
    ProtocolError = 5,
}

impl CloseReason {
    /// The number on the wire.
    pub fn code(self) -> u64 {
        self as u64
    }

    /// The reason for a wire number, if it is one of `0 ..= 5`.
    pub fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            0 => Self::Normal,
            1 => Self::HostReplaced,
            2 => Self::HostGone,
            3 => Self::HostRejected,
            4 => Self::LimitExceeded,
            5 => Self::ProtocolError,
            _ => return None,
        })
    }

    /// The WebSocket close code the relay uses on the client connection after
    /// sending `closed { reason }` (01 §4): `4003` for `limit-exceeded`,
    /// `4004` for `protocol-error`, `1000` for the rest.
    pub fn client_close_code(self) -> u16 {
        match self {
            Self::LimitExceeded => close_code::LIMIT_EXCEEDED,
            Self::ProtocolError => close_code::INVALID_ENVELOPE,
            _ => close_code::NORMAL,
        }
    }
}

/// Opaque payload of a `data` or `channel-data` envelope: a Noise record.
///
/// Deliberately has no `Debug`, `Display` or `Serialize`.
#[derive(Clone, PartialEq, Eq)]
pub struct Payload(Bytes);

impl Payload {
    /// Wraps bytes without copying.
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self(bytes)
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the payload is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Takes the underlying buffer.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

/// An envelope on the host connection (`/v0/host`).
#[derive(Clone, PartialEq, Eq)]
pub enum HostEnvelope {
    /// Relay to host: registration challenge.
    Challenge {
        /// Random nonce to sign.
        nonce: [u8; NONCE_LEN],
    },
    /// Host to relay: routing key and signature over [`register_message`].
    Register {
        /// Ed25519 public routing key.
        routing_key: [u8; ROUTING_KEY_LEN],
        /// Ed25519 signature.
        signature: [u8; SIGNATURE_LEN],
    },
    /// Relay to host: registration accepted.
    Registered {
        /// The address derived from the routing key.
        host_id: HostId,
    },
    /// Relay to host: a client opened a channel.
    ChannelOpened {
        /// The new channel.
        channel: ChannelId,
    },
    /// Both directions: one Noise record for a channel.
    ChannelData {
        /// The channel.
        channel: ChannelId,
        /// The record.
        payload: Payload,
    },
    /// Relay to host: a channel closed.
    ChannelClosed {
        /// The channel.
        channel: ChannelId,
        /// Why.
        reason: CloseReason,
    },
    /// Host to relay: close a channel.
    CloseChannel {
        /// The channel.
        channel: ChannelId,
        /// Why.
        reason: CloseReason,
    },
}

/// An envelope on a client connection (`/v0/client/<host_id>`).
#[derive(Clone, PartialEq, Eq)]
pub enum ClientEnvelope {
    /// Both directions: one Noise record.
    Data {
        /// The record.
        payload: Payload,
    },
    /// Relay to client: no host registered under that `host_id`.
    HostUnavailable,
    /// Relay to client: the channel closed.
    Closed {
        /// Why.
        reason: CloseReason,
    },
}

impl fmt::Debug for HostEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Challenge { nonce } => f.debug_struct("Challenge").field("nonce", nonce).finish(),
            Self::Register {
                routing_key,
                signature,
            } => f
                .debug_struct("Register")
                .field("routing_key", routing_key)
                .field("signature", signature)
                .finish(),
            Self::Registered { host_id } => f
                .debug_struct("Registered")
                .field("host_id", host_id)
                .finish(),
            Self::ChannelOpened { channel } => f
                .debug_struct("ChannelOpened")
                .field("channel", channel)
                .finish(),
            Self::ChannelData { channel, payload } => f
                .debug_struct("ChannelData")
                .field("channel", channel)
                .field("payload_len", &payload.len())
                .finish(),
            Self::ChannelClosed { channel, reason } => f
                .debug_struct("ChannelClosed")
                .field("channel", channel)
                .field("reason", reason)
                .finish(),
            Self::CloseChannel { channel, reason } => f
                .debug_struct("CloseChannel")
                .field("channel", channel)
                .field("reason", reason)
                .finish(),
        }
    }
}

impl fmt::Debug for ClientEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data { payload } => f
                .debug_struct("Data")
                .field("payload_len", &payload.len())
                .finish(),
            Self::HostUnavailable => f.write_str("HostUnavailable"),
            Self::Closed { reason } => f.debug_struct("Closed").field("reason", reason).finish(),
        }
    }
}

/// Which handshake follows `handshake-start` (02 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandshakeStartKind {
    /// `XX`, a new pairing.
    Pair = 0,
    /// `IK`, a paired device connecting.
    Connect = 1,
}

impl HandshakeStartKind {
    /// The number on the wire.
    pub fn code(self) -> u64 {
        self as u64
    }

    /// The kind for a wire number, if it is `0` or `1`.
    pub fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::Pair),
            1 => Some(Self::Connect),
            _ => None,
        }
    }
}

/// The first payload of a channel: the kind of handshake and Noise message 1.
#[derive(Clone, PartialEq, Eq)]
pub struct HandshakeStart {
    /// Which handshake follows.
    pub kind: HandshakeStartKind,
    /// The first Noise handshake message.
    pub msg1: Bytes,
}

impl fmt::Debug for HandshakeStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandshakeStart")
            .field("kind", &self.kind)
            .field("msg1_len", &self.msg1.len())
            .finish()
    }
}

/// Why a frame was rejected. Never carries payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EnvelopeError {
    /// The frame is longer than [`MAX_FRAME`].
    FrameTooLarge,
    /// The frame is not canonical CBOR of the expected shape (strictness,
    /// arity, wrong element type, trailing bytes).
    Cbor(CborError),
    /// An envelope type this family does not have.
    UnknownType(u64),
    /// A `close-reason` outside `0 ..= 5`.
    UnknownCloseReason(u64),
    /// A `handshake-start` kind other than `0` or `1`.
    UnknownHandshakeKind(u64),
    /// A byte string of the wrong size (`bstr .size N`).
    WrongLength,
    /// The `host-id` text is not a valid host identifier.
    HostId(HostIdError),
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge => f.write_str("frame larger than the maximum"),
            Self::Cbor(e) => write!(f, "invalid envelope: {e}"),
            Self::UnknownType(t) => write!(f, "unknown envelope type {t}"),
            Self::UnknownCloseReason(r) => write!(f, "unknown close reason {r}"),
            Self::UnknownHandshakeKind(k) => write!(f, "unknown handshake kind {k}"),
            Self::WrongLength => f.write_str("byte string of the wrong size"),
            Self::HostId(e) => write!(f, "invalid host id: {e}"),
        }
    }
}

impl core::error::Error for EnvelopeError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Cbor(e) => Some(e),
            Self::HostId(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CborError> for EnvelopeError {
    fn from(e: CborError) -> Self {
        Self::Cbor(e)
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decodes a frame received on the host connection. The payload of
/// `channel-data` is a slice of `frame`, not a copy.
pub fn decode_host(frame: Bytes) -> Result<HostEnvelope, EnvelopeError> {
    check_frame_size(&frame)?;
    let mut s = Scanner::new(&frame);
    let (kind, count) = s.envelope_head()?;
    let envelope = match kind {
        0 => {
            arity(count, 2)?;
            let nonce = fixed(&frame[s.bstr()?])?;
            HostEnvelope::Challenge { nonce }
        }
        1 => {
            arity(count, 3)?;
            let routing_key = fixed(&frame[s.bstr()?])?;
            let signature = fixed(&frame[s.bstr()?])?;
            HostEnvelope::Register {
                routing_key,
                signature,
            }
        }
        2 => {
            arity(count, 2)?;
            let text = s.tstr()?;
            let host_id = HostId::parse(text).map_err(EnvelopeError::HostId)?;
            HostEnvelope::Registered { host_id }
        }
        3 => {
            arity(count, 2)?;
            HostEnvelope::ChannelOpened {
                channel: ChannelId(s.uint()?),
            }
        }
        4 => {
            arity(count, 3)?;
            let channel = ChannelId(s.uint()?);
            let payload = Payload(frame.slice(s.bstr()?));
            HostEnvelope::ChannelData { channel, payload }
        }
        5 => {
            arity(count, 3)?;
            let channel = ChannelId(s.uint()?);
            let reason = s.close_reason()?;
            HostEnvelope::ChannelClosed { channel, reason }
        }
        6 => {
            arity(count, 3)?;
            let channel = ChannelId(s.uint()?);
            let reason = s.close_reason()?;
            HostEnvelope::CloseChannel { channel, reason }
        }
        other => return Err(EnvelopeError::UnknownType(other)),
    };
    s.finish()?;
    Ok(envelope)
}

/// Decodes a frame received on a client connection. The payload of `data` is
/// a slice of `frame`, not a copy.
pub fn decode_client(frame: Bytes) -> Result<ClientEnvelope, EnvelopeError> {
    check_frame_size(&frame)?;
    let mut s = Scanner::new(&frame);
    let (kind, count) = s.envelope_head()?;
    let envelope = match kind {
        16 => {
            arity(count, 2)?;
            ClientEnvelope::Data {
                payload: Payload(frame.slice(s.bstr()?)),
            }
        }
        17 => {
            arity(count, 1)?;
            ClientEnvelope::HostUnavailable
        }
        18 => {
            arity(count, 2)?;
            ClientEnvelope::Closed {
                reason: s.close_reason()?,
            }
        }
        other => return Err(EnvelopeError::UnknownType(other)),
    };
    s.finish()?;
    Ok(envelope)
}

/// Decodes the first payload of a channel (`handshake-start`, 02 §2). Message
/// 1 is a slice of `payload`, not a copy.
pub fn decode_handshake_start(payload: Bytes) -> Result<HandshakeStart, EnvelopeError> {
    check_frame_size(&payload)?;
    let mut s = Scanner::new(&payload);
    let count = s.array_len()?;
    arity(count, 2)?;
    let code = s.uint()?;
    let kind =
        HandshakeStartKind::from_code(code).ok_or(EnvelopeError::UnknownHandshakeKind(code))?;
    let msg1 = payload.slice(s.bstr()?);
    s.finish()?;
    Ok(HandshakeStart { kind, msg1 })
}

fn check_frame_size(frame: &[u8]) -> Result<(), EnvelopeError> {
    if frame.len() > MAX_FRAME {
        Err(EnvelopeError::FrameTooLarge)
    } else {
        Ok(())
    }
}

fn arity(count: u64, expected: u64) -> Result<(), EnvelopeError> {
    if count == expected {
        Ok(())
    } else {
        Err(EnvelopeError::Cbor(CborError::WrongArity))
    }
}

fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], EnvelopeError> {
    bytes.try_into().map_err(|_| EnvelopeError::WrongLength)
}

/// Strict reader over one CBOR item made of an array of integers, byte
/// strings and one text string.
struct Scanner<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(input: &'a [u8]) -> Self {
        Scanner { input, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CborError> {
        let end = self.pos.checked_add(n).ok_or(CborError::Truncated)?;
        let slice = self.input.get(self.pos..end).ok_or(CborError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    /// Reads a head and returns the major type and its argument, enforcing the
    /// shortest form.
    fn head(&mut self) -> Result<(u8, u64), CborError> {
        let initial = self.take(1)?[0];
        let major = initial >> 5;
        let info = initial & 0x1f;
        match major {
            6 => return Err(CborError::Tag),
            7 => {
                return Err(match info {
                    25..=27 => CborError::Float,
                    20..=22 => CborError::WrongType,
                    _ => CborError::UnsupportedSimple,
                });
            }
            _ => {}
        }
        let arg = match info {
            0..=23 => u64::from(info),
            24 => {
                let v = u64::from(self.take(1)?[0]);
                if v < 24 {
                    return Err(CborError::NonMinimal);
                }
                v
            }
            25 => {
                let b = self.take(2)?;
                let v = u64::from(u16::from_be_bytes([b[0], b[1]]));
                if v <= u64::from(u8::MAX) {
                    return Err(CborError::NonMinimal);
                }
                v
            }
            26 => {
                let b = self.take(4)?;
                let v = u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
                if v <= u64::from(u16::MAX) {
                    return Err(CborError::NonMinimal);
                }
                v
            }
            27 => {
                let b = self.take(8)?;
                let v = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
                if v <= u64::from(u32::MAX) {
                    return Err(CborError::NonMinimal);
                }
                v
            }
            31 if (2..=5).contains(&major) => return Err(CborError::IndefiniteLength),
            _ => return Err(CborError::Reserved),
        };
        Ok((major, arg))
    }

    /// Reads the array head and returns its element count.
    fn array_len(&mut self) -> Result<u64, CborError> {
        match self.head()? {
            (4, count) => Ok(count),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads the array head and the type element of an envelope.
    fn envelope_head(&mut self) -> Result<(u64, u64), CborError> {
        let count = self.array_len()?;
        if count == 0 {
            return Err(CborError::WrongArity);
        }
        Ok((self.uint()?, count))
    }

    /// Reads an unsigned integer within `0 ..= 2^53 - 1`.
    fn uint(&mut self) -> Result<u64, CborError> {
        match self.head()? {
            (0, v) if v <= MAX_SAFE_INT as u64 => Ok(v),
            (0 | 1, _) => Err(CborError::IntOutOfRange),
            _ => Err(CborError::WrongType),
        }
    }

    /// Reads a byte string and returns its range in the input.
    fn bstr(&mut self) -> Result<Range<usize>, CborError> {
        let (major, len) = self.head()?;
        if major != 2 {
            return Err(CborError::WrongType);
        }
        let len = usize::try_from(len).map_err(|_| CborError::Truncated)?;
        let start = self.pos;
        self.take(len)?;
        Ok(start..self.pos)
    }

    /// Reads a text string.
    fn tstr(&mut self) -> Result<&'a str, EnvelopeError> {
        let (major, len) = self.head()?;
        if major != 3 {
            return Err(CborError::WrongType.into());
        }
        let len = usize::try_from(len).map_err(|_| CborError::Truncated)?;
        let raw = self.take(len)?;
        core::str::from_utf8(raw).map_err(|_| CborError::InvalidUtf8.into())
    }

    fn close_reason(&mut self) -> Result<CloseReason, EnvelopeError> {
        let code = self.uint()?;
        CloseReason::from_code(code).ok_or(EnvelopeError::UnknownCloseReason(code))
    }

    fn finish(&self) -> Result<(), CborError> {
        if self.pos == self.input.len() {
            Ok(())
        } else {
            Err(CborError::TrailingBytes)
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encodes a host-connection envelope deterministically.
pub fn encode_host(envelope: &HostEnvelope) -> Vec<u8> {
    match envelope {
        HostEnvelope::Challenge { nonce } => {
            let mut out = Vec::with_capacity(2 + 2 + NONCE_LEN);
            push_head(&mut out, 4, 2);
            push_head(&mut out, 0, 0);
            push_bstr(&mut out, nonce);
            out
        }
        HostEnvelope::Register {
            routing_key,
            signature,
        } => {
            let mut out = Vec::with_capacity(2 + 2 + ROUTING_KEY_LEN + 2 + SIGNATURE_LEN);
            push_head(&mut out, 4, 3);
            push_head(&mut out, 0, 1);
            push_bstr(&mut out, routing_key);
            push_bstr(&mut out, signature);
            out
        }
        HostEnvelope::Registered { host_id } => {
            let text = host_id.as_str().as_bytes();
            let mut out = Vec::with_capacity(2 + 2 + text.len());
            push_head(&mut out, 4, 2);
            push_head(&mut out, 0, 2);
            push_head(&mut out, 3, text.len() as u64);
            out.extend_from_slice(text);
            out
        }
        HostEnvelope::ChannelOpened { channel } => {
            let mut out = Vec::with_capacity(12);
            push_head(&mut out, 4, 2);
            push_head(&mut out, 0, 3);
            push_head(&mut out, 0, channel.0);
            out
        }
        HostEnvelope::ChannelData { channel, payload } => {
            let mut out = Vec::with_capacity(channel_data_len(*channel, payload.len()));
            push_head(&mut out, 4, 3);
            push_head(&mut out, 0, 4);
            push_head(&mut out, 0, channel.0);
            push_bstr(&mut out, payload.as_bytes());
            out
        }
        HostEnvelope::ChannelClosed { channel, reason } => {
            encode_channel_reason(5, *channel, *reason)
        }
        HostEnvelope::CloseChannel { channel, reason } => {
            encode_channel_reason(6, *channel, *reason)
        }
    }
}

fn encode_channel_reason(kind: u64, channel: ChannelId, reason: CloseReason) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    push_head(&mut out, 4, 3);
    push_head(&mut out, 0, kind);
    push_head(&mut out, 0, channel.0);
    push_head(&mut out, 0, reason.code());
    out
}

/// Encodes a client-connection envelope deterministically.
pub fn encode_client(envelope: &ClientEnvelope) -> Vec<u8> {
    match envelope {
        ClientEnvelope::Data { payload } => {
            let mut out = Vec::with_capacity(client_data_len(payload.len()));
            push_head(&mut out, 4, 2);
            push_head(&mut out, 0, 16);
            push_bstr(&mut out, payload.as_bytes());
            out
        }
        ClientEnvelope::HostUnavailable => {
            let mut out = Vec::with_capacity(2);
            push_head(&mut out, 4, 1);
            push_head(&mut out, 0, 17);
            out
        }
        ClientEnvelope::Closed { reason } => {
            let mut out = Vec::with_capacity(3);
            push_head(&mut out, 4, 2);
            push_head(&mut out, 0, 18);
            push_head(&mut out, 0, reason.code());
            out
        }
    }
}

/// Encodes the first payload of a channel (`handshake-start`).
pub fn encode_handshake_start(start: &HandshakeStart) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 1 + 9 + start.msg1.len());
    push_head(&mut out, 4, 2);
    push_head(&mut out, 0, start.kind.code());
    push_bstr(&mut out, &start.msg1);
    out
}

/// Size of the frame [`encode_host`] produces for a `channel-data` with the
/// given channel and payload length. Lets a sender check [`MAX_FRAME`] before
/// building the frame.
pub fn channel_data_len(channel: ChannelId, payload_len: usize) -> usize {
    // array(3), type 4, channel, bstr head, payload.
    1 + 1 + head_len(channel.0) + head_len(payload_len as u64) + payload_len
}

/// Size of the frame [`encode_client`] produces for a `data` with the given
/// payload length.
pub fn client_data_len(payload_len: usize) -> usize {
    // array(2), type 16, bstr head, payload.
    1 + 1 + head_len(payload_len as u64) + payload_len
}

/// Number of bytes of a head carrying `arg` in the shortest form.
fn head_len(arg: u64) -> usize {
    if arg < 24 {
        1
    } else if arg <= u64::from(u8::MAX) {
        2
    } else if arg <= u64::from(u16::MAX) {
        3
    } else if arg <= u64::from(u32::MAX) {
        5
    } else {
        9
    }
}

fn push_head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    if arg < 24 {
        out.push(m | arg as u8);
    } else if arg <= u64::from(u8::MAX) {
        out.push(m | 24);
        out.push(arg as u8);
    } else if arg <= u64::from(u16::MAX) {
        out.push(m | 25);
        out.extend_from_slice(&(arg as u16).to_be_bytes());
    } else if arg <= u64::from(u32::MAX) {
        out.push(m | 26);
        out.extend_from_slice(&(arg as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

fn push_bstr(out: &mut Vec<u8>, bytes: &[u8]) {
    push_head(out, 2, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn frame(s: &str) -> Bytes {
        Bytes::from(unhex(s))
    }

    fn repeat(byte: &str, n: usize) -> String {
        byte.repeat(n)
    }

    fn payload(bytes: &[u8]) -> Payload {
        Payload::from_bytes(Bytes::copy_from_slice(bytes))
    }

    /// 32 characters: `abcdefghijklmnopqrstuvwxyz234567`, as ASCII hex.
    const HOST_ID_TEXT: &str = "abcdefghijklmnopqrstuvwxyz234567";
    const HOST_ID_HEX: &str = "6162636465666768696a6b6c6d6e6f707172737475767778797a323334353637";

    // Hex below is computed by hand from RFC 8949: head byte = major << 5 |
    // argument (1-byte argument if < 24, else 0x18/0x19/0x1a/0x1b prefix).
    // Array heads: 0x81 / 0x82 / 0x83. bstr: 0x40 + len, or 0x58 len, 0x59 len
    // (2 bytes). tstr of 32: 0x78 0x20.

    #[test]
    fn host_envelopes_encode_to_hand_computed_hex() {
        let cases: Vec<(HostEnvelope, String)> = vec![
            (
                // [0, h'aa' x32]: 82, 00, 58 20, 32 bytes.
                HostEnvelope::Challenge { nonce: [0xaa; 32] },
                format!("82 00 5820 {}", repeat("aa", 32)),
            ),
            (
                // [1, h'11' x32, h'22' x64]: 83, 01, 58 20, ..., 58 40, ...
                HostEnvelope::Register {
                    routing_key: [0x11; 32],
                    signature: [0x22; 64],
                },
                format!("83 01 5820 {} 5840 {}", repeat("11", 32), repeat("22", 64)),
            ),
            (
                // [2, "abc...567"]: 82, 02, 78 20, 32 ASCII bytes.
                HostEnvelope::Registered {
                    host_id: HostId::parse(HOST_ID_TEXT).unwrap(),
                },
                format!("82 02 7820 {HOST_ID_HEX}"),
            ),
            (
                // [3, 7]
                HostEnvelope::ChannelOpened {
                    channel: ChannelId(7),
                },
                "82 03 07".to_owned(),
            ),
            (
                // [4, 300, h'010203']: 300 = 0x012c -> 19 012c; bstr 43.
                HostEnvelope::ChannelData {
                    channel: ChannelId(300),
                    payload: payload(&[1, 2, 3]),
                },
                "83 04 19012c 43 010203".to_owned(),
            ),
            (
                // [5, 7, 2]
                HostEnvelope::ChannelClosed {
                    channel: ChannelId(7),
                    reason: CloseReason::HostGone,
                },
                "83 05 07 02".to_owned(),
            ),
            (
                // [6, 0, 3]
                HostEnvelope::CloseChannel {
                    channel: ChannelId(0),
                    reason: CloseReason::HostRejected,
                },
                "83 06 00 03".to_owned(),
            ),
        ];
        for (envelope, expected) in cases {
            let expected = unhex(&expected);
            assert_eq!(hex(&encode_host(&envelope)), hex(&expected), "{envelope:?}");
            assert_eq!(
                decode_host(Bytes::from(expected)).as_ref(),
                Ok(&envelope),
                "{envelope:?}"
            );
        }
    }

    #[test]
    fn client_envelopes_encode_to_hand_computed_hex() {
        let cases: Vec<(ClientEnvelope, &str)> = vec![
            (
                // [16, h'010203']
                ClientEnvelope::Data {
                    payload: payload(&[1, 2, 3]),
                },
                "82 10 43 010203",
            ),
            (
                // [16, h'']
                ClientEnvelope::Data {
                    payload: payload(&[]),
                },
                "82 10 40",
            ),
            // [17]
            (ClientEnvelope::HostUnavailable, "81 11"),
            (
                // [18, 1]
                ClientEnvelope::Closed {
                    reason: CloseReason::HostReplaced,
                },
                "82 12 01",
            ),
        ];
        for (envelope, expected) in cases {
            let expected = unhex(expected);
            assert_eq!(hex(&encode_client(&envelope)), hex(&expected));
            assert_eq!(decode_client(Bytes::from(expected)).as_ref(), Ok(&envelope));
        }
    }

    #[test]
    fn handshake_start_hex_and_round_trip() {
        // [0, h'dead'] and [1, h'dead']
        for (kind, expected) in [
            (HandshakeStartKind::Pair, "82 00 42 dead"),
            (HandshakeStartKind::Connect, "82 01 42 dead"),
        ] {
            let start = HandshakeStart {
                kind,
                msg1: Bytes::from_static(&[0xde, 0xad]),
            };
            let expected = unhex(expected);
            assert_eq!(hex(&encode_handshake_start(&start)), hex(&expected));
            assert_eq!(decode_handshake_start(Bytes::from(expected)), Ok(start));
        }
        // Kind 2, wrong arity, trailing, wrong type.
        assert_eq!(
            decode_handshake_start(frame("82 02 42 dead")),
            Err(EnvelopeError::UnknownHandshakeKind(2))
        );
        assert_eq!(
            decode_handshake_start(frame("81 00")),
            Err(EnvelopeError::Cbor(CborError::WrongArity))
        );
        assert_eq!(
            decode_handshake_start(frame("83 00 42 dead 00")),
            Err(EnvelopeError::Cbor(CborError::WrongArity))
        );
        assert_eq!(
            decode_handshake_start(frame("82 00 42 dead 00")),
            Err(EnvelopeError::Cbor(CborError::TrailingBytes))
        );
        assert_eq!(
            decode_handshake_start(frame("82 00 6161")),
            Err(EnvelopeError::Cbor(CborError::WrongType))
        );
        assert_eq!(
            decode_handshake_start(frame("82 00 42 de")),
            Err(EnvelopeError::Cbor(CborError::Truncated))
        );
    }

    #[test]
    fn handshake_start_message_is_zero_copy() {
        let bytes = frame("82 00 44 01020304");
        let start = decode_handshake_start(bytes.clone()).unwrap();
        assert_eq!(start.msg1.as_ptr(), bytes[3..].as_ptr());
        assert_eq!(start.msg1.as_ref(), &[1, 2, 3, 4]);
    }

    #[test]
    fn wrong_arity_is_rejected() {
        // The array count is checked against the type before any element.
        for input in [
            "80",                 // empty array
            "81 00",              // challenge without nonce
            "83 00 4101 00",      // challenge with an extra element
            "83 03 07 00",        // channel-opened with three elements
            "82 04 07",           // channel-data without payload
            "84 04 07 40 00",     // channel-data with four elements
            "82 05 07",           // channel-closed without reason
            "82 06 07",           // close-channel without reason
            "82 01 4100",         // register with two elements
            "84 01 4100 4100 00", // register with four elements
            "83 02 6161 00",      // registered with three elements
        ] {
            assert_eq!(
                decode_host(frame(input)),
                Err(EnvelopeError::Cbor(CborError::WrongArity)),
                "{input}"
            );
        }
        for input in [
            "80",
            "83 10 40 00",
            "81 10",
            "83 12 01 00",
            "82 11 00",
            "81 12",
        ] {
            assert_eq!(
                decode_client(frame(input)),
                Err(EnvelopeError::Cbor(CborError::WrongArity)),
                "{input}"
            );
        }
    }

    #[test]
    fn unknown_and_foreign_types_are_rejected() {
        assert_eq!(
            decode_host(frame("82 07 00")),
            Err(EnvelopeError::UnknownType(7))
        );
        // A client type on the host connection and vice versa.
        assert_eq!(
            decode_host(frame("81 11")),
            Err(EnvelopeError::UnknownType(17))
        );
        assert_eq!(
            decode_client(frame("82 03 07")),
            Err(EnvelopeError::UnknownType(3))
        );
        assert_eq!(
            decode_client(frame("82 13 00")),
            Err(EnvelopeError::UnknownType(19))
        );
        // Type 24 needs the 1-byte form: 18 18.
        assert_eq!(
            decode_host(frame("82 1818 00")),
            Err(EnvelopeError::UnknownType(24))
        );
    }

    #[test]
    fn close_reason_6_is_rejected() {
        assert_eq!(
            decode_host(frame("83 05 07 06")),
            Err(EnvelopeError::UnknownCloseReason(6))
        );
        assert_eq!(
            decode_host(frame("83 06 07 06")),
            Err(EnvelopeError::UnknownCloseReason(6))
        );
        assert_eq!(
            decode_client(frame("82 12 06")),
            Err(EnvelopeError::UnknownCloseReason(6))
        );
        for code in 0..=5 {
            assert_eq!(CloseReason::from_code(code).unwrap().code(), code);
        }
        assert_eq!(CloseReason::from_code(6), None);
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        for input in ["82 03 07 00", "82 03 07 f6"] {
            assert_eq!(
                decode_host(frame(input)),
                Err(EnvelopeError::Cbor(CborError::TrailingBytes)),
                "{input}"
            );
        }
        assert_eq!(
            decode_client(frame("81 11 00")),
            Err(EnvelopeError::Cbor(CborError::TrailingBytes))
        );
        assert_eq!(
            decode_client(frame("82 10 40 40")),
            Err(EnvelopeError::Cbor(CborError::TrailingBytes))
        );
    }

    #[test]
    fn non_canonical_forms_are_rejected() {
        let non_minimal = [
            "82 1803 07",         // type 3 in the 1-byte form
            "82 03 1807",         // channel 7 in the 1-byte form
            "82 03 190007",       // channel 7 in the 2-byte form
            "82 03 1a00000007",   // 4-byte form
            "83 04 19 00ff 40",   // 255 in the 2-byte form
            "9802 03 07",         // array head in the 1-byte form
            "83 04 07 5800",      // bstr length in the 1-byte form
            "83 04 07 590001 aa", // bstr length 1 in the 2-byte form
        ];
        for input in non_minimal {
            assert_eq!(
                decode_host(frame(input)),
                Err(EnvelopeError::Cbor(CborError::NonMinimal)),
                "{input}"
            );
        }
        assert_eq!(
            decode_client(frame("82 1810 40")),
            Err(EnvelopeError::Cbor(CborError::NonMinimal))
        );
        // Indefinite lengths.
        assert_eq!(
            decode_host(frame("9f 03 07 ff")),
            Err(EnvelopeError::Cbor(CborError::IndefiniteLength))
        );
        assert_eq!(
            decode_host(frame("83 04 07 5f 4101 ff")),
            Err(EnvelopeError::Cbor(CborError::IndefiniteLength))
        );
        // Tag, float, simple value where an integer belongs.
        assert_eq!(
            decode_host(frame("82 03 c1 07")),
            Err(EnvelopeError::Cbor(CborError::Tag))
        );
        assert_eq!(
            decode_host(frame("82 03 f93c00")),
            Err(EnvelopeError::Cbor(CborError::Float))
        );
        assert_eq!(
            decode_host(frame("82 03 f6")),
            Err(EnvelopeError::Cbor(CborError::WrongType))
        );
        // Reserved additional information.
        assert_eq!(
            decode_host(frame("82 03 1c")),
            Err(EnvelopeError::Cbor(CborError::Reserved))
        );
    }

    #[test]
    fn wrong_element_types_and_ranges_are_rejected() {
        // Not an array.
        for input in ["00", "a0", "40", "f6"] {
            assert_eq!(
                decode_host(frame(input)),
                Err(EnvelopeError::Cbor(CborError::WrongType)),
                "{input}"
            );
        }
        // Channel as text; payload as text; type as bstr.
        assert_eq!(
            decode_host(frame("82 03 6161")),
            Err(EnvelopeError::Cbor(CborError::WrongType))
        );
        assert_eq!(
            decode_client(frame("82 10 6161")),
            Err(EnvelopeError::Cbor(CborError::WrongType))
        );
        assert_eq!(
            decode_host(frame("82 4100 07")),
            Err(EnvelopeError::Cbor(CborError::WrongType))
        );
        // Negative channel; channel = 2^53; channel = 2^53 - 1 is accepted.
        assert_eq!(
            decode_host(frame("82 03 20")),
            Err(EnvelopeError::Cbor(CborError::IntOutOfRange))
        );
        assert_eq!(
            decode_host(frame("82 03 1b 0020000000000000")),
            Err(EnvelopeError::Cbor(CborError::IntOutOfRange))
        );
        assert_eq!(
            decode_host(frame("82 03 1b 001fffffffffffff")),
            Ok(HostEnvelope::ChannelOpened {
                channel: ChannelId(MAX_SAFE_INT as u64)
            })
        );
    }

    #[test]
    fn truncated_frames_are_rejected() {
        for input in [
            "",
            "82",
            "82 03",
            "83 04 07 43 0102",
            "83 04 07 58",
            "82 02 7820 6162",
        ] {
            assert_eq!(
                decode_host(frame(input)),
                Err(EnvelopeError::Cbor(CborError::Truncated)),
                "{input}"
            );
        }
        // A declared length far beyond the frame does not allocate or panic.
        assert_eq!(
            decode_host(frame("83 04 07 5b 7fffffffffffffff")),
            Err(EnvelopeError::Cbor(CborError::Truncated))
        );
        assert_eq!(
            decode_host(frame("83 04 07 5b ffffffffffffffff")),
            Err(EnvelopeError::Cbor(CborError::Truncated))
        );
    }

    #[test]
    fn byte_strings_have_exact_sizes() {
        assert_eq!(
            decode_host(frame("82 00 4101")),
            Err(EnvelopeError::WrongLength)
        );
        assert_eq!(
            decode_host(frame(&format!("82 00 5821 {}", repeat("aa", 33)))),
            Err(EnvelopeError::WrongLength)
        );
        assert_eq!(
            decode_host(frame(&format!(
                "83 01 5820 {} 5820 {}",
                repeat("11", 32),
                repeat("22", 32)
            ))),
            Err(EnvelopeError::WrongLength)
        );
    }

    #[test]
    fn registered_validates_the_host_id() {
        // Wrong length.
        assert_eq!(
            decode_host(frame("82 02 61 61")),
            Err(EnvelopeError::HostId(HostIdError::Length))
        );
        // Right length, uppercase letters.
        assert_eq!(
            decode_host(frame(&format!("82 02 7820 {}", repeat("41", 32)))),
            Err(EnvelopeError::HostId(HostIdError::Alphabet))
        );
        // Invalid UTF-8.
        assert_eq!(
            decode_host(frame("82 02 61 ff")),
            Err(EnvelopeError::Cbor(CborError::InvalidUtf8))
        );
        // Not text.
        assert_eq!(
            decode_host(frame("82 02 4161")),
            Err(EnvelopeError::Cbor(CborError::WrongType))
        );
    }

    #[test]
    fn payload_is_a_zero_copy_slice_of_the_frame() {
        let bytes = frame("83 04 19012c 43 010203");
        let start = bytes.as_ptr() as usize;
        let HostEnvelope::ChannelData { channel, payload } = decode_host(bytes.clone()).unwrap()
        else {
            panic!("not channel-data");
        };
        assert_eq!(channel, ChannelId(300));
        assert_eq!(payload.as_bytes(), &[1, 2, 3]);
        // Offset: 83 | 04 | 19 01 2c | 43 -> payload starts at byte 6.
        assert_eq!(payload.as_bytes().as_ptr() as usize, start + 6);

        let bytes = frame("82 10 43 010203");
        let start = bytes.as_ptr() as usize;
        let ClientEnvelope::Data { payload } = decode_client(bytes.clone()).unwrap() else {
            panic!("not data");
        };
        assert_eq!(payload.as_bytes().as_ptr() as usize, start + 3);
        // The payload keeps the buffer alive on its own.
        drop(bytes);
        assert_eq!(payload.into_bytes().as_ref(), &[1, 2, 3]);
    }

    #[test]
    fn frame_size_limit() {
        // 83 04 00 5a <4 bytes>: 3 + 5 head bytes, so a 69_992-byte payload
        // makes a frame of exactly 70_000 bytes.
        let at_limit = HostEnvelope::ChannelData {
            channel: ChannelId(0),
            payload: payload(&vec![0x5a; 69_992]),
        };
        let encoded = encode_host(&at_limit);
        assert_eq!(encoded.len(), MAX_FRAME);
        assert_eq!(&encoded[..8], &unhex("83 04 00 5a 00011168")[..]);
        assert_eq!(decode_host(Bytes::from(encoded)).as_ref(), Ok(&at_limit));

        let over = vec![0u8; MAX_FRAME + 1];
        assert_eq!(
            decode_host(Bytes::from(over.clone())),
            Err(EnvelopeError::FrameTooLarge)
        );
        assert_eq!(
            decode_client(Bytes::from(over.clone())),
            Err(EnvelopeError::FrameTooLarge)
        );
        assert_eq!(
            decode_handshake_start(Bytes::from(over)),
            Err(EnvelopeError::FrameTooLarge)
        );
    }

    #[test]
    fn channel_data_len_matches_the_encoder() {
        let channels = [
            0u64,
            23,
            24,
            255,
            256,
            65_535,
            65_536,
            u64::from(u32::MAX),
            1 << 32,
        ];
        let lengths = [0usize, 1, 23, 24, 255, 256, 65_535, 65_536];
        for channel in channels {
            for len in lengths {
                let channel = ChannelId(channel);
                let envelope = HostEnvelope::ChannelData {
                    channel,
                    payload: payload(&vec![0; len]),
                };
                assert_eq!(
                    encode_host(&envelope).len(),
                    channel_data_len(channel, len),
                    "channel {channel:?}, len {len}"
                );
            }
        }
        for len in lengths {
            let envelope = ClientEnvelope::Data {
                payload: payload(&vec![0; len]),
            };
            assert_eq!(encode_client(&envelope).len(), client_data_len(len));
        }
        // 3-byte channel head, 1-byte length head: 2 + 3 + 1 + 10.
        assert_eq!(channel_data_len(ChannelId(300), 10), 16);
    }

    #[test]
    fn large_channels_round_trip() {
        for channel in [
            0,
            23,
            24,
            255,
            256,
            65_535,
            65_536,
            1 << 32,
            MAX_SAFE_INT as u64,
        ] {
            let envelope = HostEnvelope::ChannelOpened {
                channel: ChannelId(channel),
            };
            assert_eq!(
                decode_host(Bytes::from(encode_host(&envelope))).as_ref(),
                Ok(&envelope)
            );
        }
    }

    /// Reference bytes agreed with the TypeScript implementation (nonce
    /// 00..1f, routing key 20..3f, signature 40..7f).
    #[test]
    fn matches_the_typescript_reference_bytes() {
        let seq = |from: u8, n: usize| -> Vec<u8> { (0..n as u8).map(|i| from + i).collect() };
        let arr = |from: u8, n: usize| -> [u8; 32] {
            let mut a = [0u8; 32];
            a.copy_from_slice(&seq(from, n));
            a
        };
        let host_id = HostId::parse("awbfmb6x7xznqlxuypemflvjmgwzrvqo").unwrap();
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&seq(0x40, 64));
        let cases: Vec<(HostEnvelope, String)> = vec![
            (
                HostEnvelope::Challenge { nonce: arr(0, 32) },
                format!("82 00 5820 {}", hex(&seq(0, 32))),
            ),
            (
                HostEnvelope::Register {
                    routing_key: arr(0x20, 32),
                    signature,
                },
                format!(
                    "83 01 5820 {} 5840 {}",
                    hex(&seq(0x20, 32)),
                    hex(&seq(0x40, 64))
                ),
            ),
            (
                HostEnvelope::Registered {
                    host_id: host_id.clone(),
                },
                format!("82 02 7820 {}", hex(host_id.as_str().as_bytes())),
            ),
            (
                HostEnvelope::ChannelOpened {
                    channel: ChannelId(5),
                },
                "820305".to_owned(),
            ),
            (
                HostEnvelope::ChannelOpened {
                    channel: ChannelId(1000),
                },
                "82031903e8".to_owned(),
            ),
            (
                HostEnvelope::ChannelData {
                    channel: ChannelId(7),
                    payload: payload(&[1, 2, 3]),
                },
                "83040743010203".to_owned(),
            ),
            (
                HostEnvelope::ChannelClosed {
                    channel: ChannelId(7),
                    reason: CloseReason::HostReplaced,
                },
                "83050701".to_owned(),
            ),
            (
                HostEnvelope::CloseChannel {
                    channel: ChannelId(7),
                    reason: CloseReason::HostRejected,
                },
                "83060703".to_owned(),
            ),
        ];
        for (envelope, expected) in cases {
            let expected = unhex(&expected);
            assert_eq!(hex(&encode_host(&envelope)), hex(&expected), "{envelope:?}");
            assert_eq!(
                decode_host(Bytes::from(expected)).as_ref(),
                Ok(&envelope),
                "{envelope:?}"
            );
        }
        let client: Vec<(ClientEnvelope, &str)> = vec![
            (
                ClientEnvelope::Data {
                    payload: payload(&[0xaa, 0xbb]),
                },
                "821042aabb",
            ),
            (ClientEnvelope::HostUnavailable, "8111"),
            (
                ClientEnvelope::Closed {
                    reason: CloseReason::LimitExceeded,
                },
                "821204",
            ),
        ];
        for (envelope, expected) in client {
            let expected = unhex(expected);
            assert_eq!(hex(&encode_client(&envelope)), hex(&expected));
            assert_eq!(decode_client(Bytes::from(expected)).as_ref(), Ok(&envelope));
        }
        let start = HandshakeStart {
            kind: HandshakeStartKind::Pair,
            msg1: Bytes::from_static(&[1, 2]),
        };
        assert_eq!(hex(&encode_handshake_start(&start)), "8200420102");
    }

    #[test]
    fn client_close_codes() {
        assert_eq!(CloseReason::Normal.client_close_code(), 1000);
        assert_eq!(CloseReason::HostReplaced.client_close_code(), 1000);
        assert_eq!(CloseReason::HostGone.client_close_code(), 1000);
        assert_eq!(CloseReason::HostRejected.client_close_code(), 1000);
        assert_eq!(CloseReason::LimitExceeded.client_close_code(), 4003);
        assert_eq!(CloseReason::ProtocolError.client_close_code(), 4004);
    }

    #[test]
    fn register_message_is_context_then_nonce() {
        let nonce = [0x42; 32];
        let message = register_message(&nonce);
        assert_eq!(message.len(), 58);
        assert_eq!(REGISTER_CONTEXT.len(), 26);
        assert_eq!(&message[..26], b"porecatu-relay/v0/register");
        assert_eq!(&message[26..], &nonce);
    }

    #[test]
    fn constants_match_the_specification() {
        assert_eq!(PATH_VERSION, "v0");
        assert_eq!(MAX_FRAME, 70_000);
        assert_eq!(MIN_CHANNELS_PER_HOST, 8);
        assert_eq!(MIN_CHANNEL_BANDWIDTH_BPS, 2_000_000);
        assert_eq!(REGISTER_TIMEOUT_MS, 10_000);
        assert_eq!(PING_INTERVAL_MS, 25_000);
        assert_eq!(PONG_TIMEOUT_MS, 60_000);
        assert_eq!(NONCE_LEN, 32);
        assert_eq!(close_code::NORMAL, 1000);
        assert_eq!(close_code::GOING_AWAY, 1001);
        assert_eq!(close_code::UNSUPPORTED_DATA, 1003);
        assert_eq!(close_code::MESSAGE_TOO_BIG, 1009);
        assert_eq!(close_code::REGISTER_REJECTED, 4001);
        assert_eq!(close_code::REPLACED, 4002);
        assert_eq!(close_code::LIMIT_EXCEEDED, 4003);
        assert_eq!(close_code::INVALID_ENVELOPE, 4004);
        assert_eq!(close_code::REGISTER_TIMEOUT, 4005);
    }

    #[test]
    fn debug_never_shows_payload_bytes() {
        let envelope = HostEnvelope::ChannelData {
            channel: ChannelId(1),
            payload: payload(&[0xde, 0xad, 0xbe, 0xef]),
        };
        let shown = format!("{envelope:?}");
        assert!(shown.contains("payload_len: 4"), "{shown}");
        assert!(!shown.contains('['), "{shown}");
        let shown = format!(
            "{:?}",
            ClientEnvelope::Data {
                payload: payload(&[0xca, 0xfe])
            }
        );
        assert!(shown.contains("payload_len: 2"), "{shown}");
        let shown = format!(
            "{:?}",
            HandshakeStart {
                kind: HandshakeStartKind::Pair,
                msg1: Bytes::from_static(&[0xca, 0xfe])
            }
        );
        assert!(shown.contains("msg1_len: 2"), "{shown}");
    }
}
