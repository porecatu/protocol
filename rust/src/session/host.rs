// SPDX-License-Identifier: GPL-3.0-or-later

//! The host side of one channel as a sans-IO state machine (spec 02): the
//! Porecatu desktop talking to one client through the relay.
//!
//! The host multiplexes many channels on one WebSocket and builds the
//! `channel-*` envelopes itself, so [`HostChannel`] works with **payloads**:
//! the bytes inside a `channel-data` envelope, in both directions. One
//! `HostChannel` per channel, created at the `channel-opened`.
//!
//! # Outputs
//!
//! Everything that comes out of [`receive`](HostChannel::receive) and
//! [`tick`](HostChannel::tick) is a [`HostEvent`], in order. Payloads to send
//! are [`HostEvent::Send`] events, interleaved with the others, so the order
//! between "send this" and "this happened" is never lost.
//! [`send`](HostChannel::send) and
//! [`reject_not_paired`](HostChannel::reject_not_paired) return payloads only.
//!
//! # Errors
//!
//! A failure caused by the peer, the network or the clock is an event,
//! [`HostEvent::Failed`], after which the channel is closed and the caller
//! sends `close-channel` with [`HostError::close_reason`]. `Err` is reserved
//! for the caller's own misuse (a call on a closed channel, a message the host
//! may not send) and for the failures of the calls that return payloads only.
//!
//! # Who decides what
//!
//! This type knows nothing about the list of paired devices or the pairing
//! token. On `IK` it completes the handshake always and reports the device
//! key ([`HostEvent::DeviceConnecting`]); the caller looks the key up and, for
//! an unknown device, calls [`reject_not_paired`](HostChannel::reject_not_paired)
//! (spec 02 §4). On `XX` it reports the request
//! ([`HostEvent::PairRequested`]); the caller checks the token with
//! [`PairingToken`](super::PairingToken), asks the user, and answers with
//! `pair-accepted` or `pair-rejected` through [`send`](HostChannel::send).
//! Messages the client sends before the caller decided are delivered as they
//! come; deciding first is the caller's job.

use core::fmt;

use bytes::Bytes;

use super::HANDSHAKE_TIMEOUT_MS;
use super::handshake::{HandshakeError, HandshakeKind, HostHandshake, HostStep};
use super::keys::StaticKeypair;
use super::record::{RecordError, Session};
use crate::envelope::{self, CloseReason, EnvelopeError, HandshakeStartKind};
use crate::host_id::HostId;
use crate::message::{self, DecodeError, Direction, ErrorCode, ErrorMessage, Message, PairRequest};

/// Why a [`HostChannel`] failed or refused a call. Typed codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostError {
    /// The handshake failed: a Noise message that does not decrypt (this
    /// includes a prologue of another major), a null DH result, a non-empty
    /// payload in `msg1`.
    Handshake(HandshakeError),
    /// The first payload is not a valid `handshake-start`.
    HandshakeStart(EnvelopeError),
    /// A record failed: the channel is closed (there is no resend).
    Record(RecordError),
    /// The client sent a message that does not decode (or one only the host
    /// may send). The host answered `error { protocol }`.
    Message(DecodeError),
    /// The `pair-request` of `msg3` does not decode. The host answered
    /// `error { protocol }`.
    PairRequest(DecodeError),
    /// The handshake did not finish within
    /// [`HANDSHAKE_TIMEOUT_MS`](super::HANDSHAKE_TIMEOUT_MS).
    Timeout,
    /// The channel already ended.
    Closed,
    /// The handshake has not finished: there is no transport to send on yet.
    NotOpen,
    /// The message is one the host must not send (spec 03 §10).
    NotSendable,
}

impl HostError {
    /// The `close-reason` for the `close-channel` the caller sends after a
    /// [`HostEvent::Failed`]: `host-rejected` for every failure of the
    /// handshake (spec 02 §2: no application message), `normal` for failures
    /// of an already open session (the `error { protocol }` went first, when
    /// there was one).
    #[must_use]
    pub fn close_reason(&self) -> CloseReason {
        match self {
            HostError::Handshake(_) | HostError::HandshakeStart(_) | HostError::Timeout => {
                CloseReason::HostRejected
            }
            _ => CloseReason::Normal,
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Handshake(e) => write!(f, "handshake failed: {e}"),
            HostError::HandshakeStart(e) => write!(f, "invalid handshake-start: {e}"),
            HostError::Record(e) => write!(f, "record failed: {e}"),
            HostError::Message(e) => write!(f, "invalid client message: {e}"),
            HostError::PairRequest(e) => write!(f, "invalid pair request: {e}"),
            HostError::Timeout => f.write_str("handshake timed out"),
            HostError::Closed => f.write_str("channel closed"),
            HostError::NotOpen => f.write_str("channel not open yet"),
            HostError::NotSendable => f.write_str("message not sendable by the host"),
        }
    }
}

impl core::error::Error for HostError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            HostError::Handshake(e) => Some(e),
            HostError::HandshakeStart(e) => Some(e),
            HostError::Record(e) => Some(e),
            HostError::Message(e) | HostError::PairRequest(e) => Some(e),
            _ => None,
        }
    }
}

/// What happened on a [`HostChannel`], in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A payload to put in a `channel-data` envelope for this channel.
    Send(Bytes),
    /// `IK` completed: a device with this static key connects. The caller
    /// checks the list of paired devices and, for an unknown key, calls
    /// [`HostChannel::reject_not_paired`].
    DeviceConnecting {
        /// The static key of the device.
        device_key: [u8; 32],
    },
    /// `XX` completed: a device asks to pair. The caller checks the token,
    /// asks the user and answers with `pair-accepted` or `pair-rejected`.
    PairRequested {
        /// The static key of the device.
        device_key: [u8; 32],
        /// The decrypted `pair-request`.
        request: PairRequest,
    },
    /// A message from the client.
    Message(Message),
    /// The channel failed and is closed. After a message that does not
    /// decode, an `error { protocol }` [`Send`](HostEvent::Send) precedes
    /// this event. The caller closes the channel with
    /// [`HostError::close_reason`].
    Failed(HostError),
}

enum Stage {
    /// Waiting for the `handshake-start`. Holds the host key until it comes.
    AwaitStart(StaticKeypair),
    /// `XX` answered; waiting for `msg3`.
    AwaitMsg3(Box<HostHandshake>),
    Open(Box<Session>),
    Closed,
}

/// The host side of one channel.
pub struct HostChannel {
    stage: Stage,
    /// The `host_id`, for the Noise prologue.
    host_id: String,
    deadline_ms: u64,
    /// The fixed ephemeral key of the handshake, for test vectors.
    #[cfg(any(test, feature = "test-vectors"))]
    ephemeral: Option<[u8; 32]>,
}

impl HostChannel {
    /// Starts the state machine of a channel that was just opened
    /// (`channel-opened`). The 15 s handshake deadline counts from `now_ms`.
    ///
    /// The channel keeps a copy of the host key until the first payload
    /// arrives, and drops it right after.
    #[must_use]
    pub fn new(host_static: &StaticKeypair, host_id: &HostId, now_ms: u64) -> HostChannel {
        HostChannel {
            stage: Stage::AwaitStart(host_static.duplicate()),
            host_id: host_id.as_str().to_owned(),
            deadline_ms: now_ms.saturating_add(HANDSHAKE_TIMEOUT_MS),
            #[cfg(any(test, feature = "test-vectors"))]
            ephemeral: None,
        }
    }

    /// Like [`new`](Self::new) with a fixed ephemeral key for the handshake,
    /// for test vectors.
    #[cfg(any(test, feature = "test-vectors"))]
    #[must_use]
    pub fn with_ephemeral(
        host_static: &StaticKeypair,
        host_id: &HostId,
        now_ms: u64,
        ephemeral: [u8; 32],
    ) -> HostChannel {
        HostChannel {
            ephemeral: Some(ephemeral),
            ..HostChannel::new(host_static, host_id, now_ms)
        }
    }

    /// Answers the first payload: with a random or, in test vectors, a fixed
    /// ephemeral key.
    fn respond(
        &self,
        kind: HandshakeKind,
        host_key: &StaticKeypair,
        msg1: &[u8],
    ) -> Result<HostStep, HandshakeError> {
        #[cfg(any(test, feature = "test-vectors"))]
        if let Some(ephemeral) = &self.ephemeral {
            return HostHandshake::respond_with_ephemeral(
                kind,
                host_key,
                &self.host_id,
                msg1,
                ephemeral,
            );
        }
        HostHandshake::respond(kind, host_key, &self.host_id, msg1)
    }

    /// Whether the transport is open and messages can be sent.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self.stage, Stage::Open(_))
    }

    /// Whether the handshake is still running.
    #[must_use]
    pub fn is_handshaking(&self) -> bool {
        matches!(self.stage, Stage::AwaitStart(_) | Stage::AwaitMsg3(_))
    }

    /// Whether the channel ended.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        matches!(self.stage, Stage::Closed)
    }

    /// Takes one payload the client sent on the channel.
    ///
    /// # Errors
    ///
    /// [`HostError::Closed`] when the channel already ended. Every other
    /// failure comes as a [`HostEvent::Failed`] in the returned list.
    pub fn receive(&mut self, payload: &[u8], now_ms: u64) -> Result<Vec<HostEvent>, HostError> {
        if self.is_closed() {
            return Err(HostError::Closed);
        }
        let mut events = Vec::new();
        if self.handshake_overdue(now_ms) {
            events.push(self.fail(HostError::Timeout));
            return Ok(events);
        }
        match core::mem::replace(&mut self.stage, Stage::Closed) {
            Stage::Closed => {}
            Stage::AwaitStart(key) => self.on_start(&key, payload, now_ms, &mut events),
            Stage::AwaitMsg3(pending) => self.on_msg3(*pending, payload, now_ms, &mut events),
            Stage::Open(session) => self.on_record(session, payload, now_ms, &mut events),
        }
        Ok(events)
    }

    /// Seals `message` into the payloads to send, in order.
    ///
    /// # Errors
    ///
    /// [`HostError::NotOpen`] before the handshake ends,
    /// [`HostError::Closed`] after the channel ended,
    /// [`HostError::NotSendable`] for a message only the client sends, and
    /// [`HostError::Record`] when sealing fails (the channel is closed then).
    pub fn send(&mut self, message: &Message, now_ms: u64) -> Result<Vec<Bytes>, HostError> {
        let session = match &mut self.stage {
            Stage::Open(session) => session,
            Stage::AwaitStart(_) | Stage::AwaitMsg3(_) => return Err(HostError::NotOpen),
            Stage::Closed => return Err(HostError::Closed),
        };
        if !message.can_be_sent_by(Direction::HostToClient) {
            return Err(HostError::NotSendable);
        }
        match session.seal(&message::encode(message), now_ms) {
            Ok(records) => Ok(records.into_iter().map(Bytes::from).collect()),
            Err(error) => {
                self.stage = Stage::Closed;
                Err(HostError::Record(error))
            }
        }
    }

    /// Answers an `IK` of a device that is not paired (spec 02 §4): the first
    /// application message is `error { not-paired }`. Returns its payloads
    /// and closes the channel; the caller then sends `close-channel`.
    ///
    /// # Errors
    ///
    /// [`HostError::NotOpen`] before the handshake ends,
    /// [`HostError::Closed`] after the channel ended, and
    /// [`HostError::Record`] when sealing fails.
    pub fn reject_not_paired(&mut self, now_ms: u64) -> Result<Vec<Bytes>, HostError> {
        let reply = Message::Error(ErrorMessage {
            code: ErrorCode::NotPaired,
            id: None,
            pane: None,
        });
        let payloads = self.send(&reply, now_ms)?;
        self.stage = Stage::Closed;
        Ok(payloads)
    }

    /// Advances the clock. The only deadline is the one of the handshake:
    /// when it passes, the channel fails with [`HostError::Timeout`] and the
    /// caller closes it with `host-rejected`.
    pub fn tick(&mut self, now_ms: u64) -> Vec<HostEvent> {
        if self.handshake_overdue(now_ms) {
            vec![self.fail(HostError::Timeout)]
        } else {
            Vec::new()
        }
    }

    /// The next time [`tick`](Self::tick) has something to do: the handshake
    /// deadline while the handshake runs, `None` afterwards.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        self.is_handshaking().then_some(self.deadline_ms)
    }

    fn handshake_overdue(&self, now_ms: u64) -> bool {
        self.is_handshaking() && now_ms >= self.deadline_ms
    }

    fn fail(&mut self, error: HostError) -> HostEvent {
        self.stage = Stage::Closed;
        HostEvent::Failed(error)
    }

    fn on_start(
        &mut self,
        host_key: &StaticKeypair,
        payload: &[u8],
        now_ms: u64,
        events: &mut Vec<HostEvent>,
    ) {
        let start = match envelope::decode_handshake_start(Bytes::copy_from_slice(payload)) {
            Ok(start) => start,
            Err(error) => {
                events.push(HostEvent::Failed(HostError::HandshakeStart(error)));
                return;
            }
        };
        let kind = match start.kind {
            HandshakeStartKind::Pair => HandshakeKind::Pair,
            HandshakeStartKind::Connect => HandshakeKind::Connect,
        };
        match self.respond(kind, host_key, &start.msg1) {
            Err(error) => events.push(HostEvent::Failed(HostError::Handshake(error))),
            Ok(HostStep::Pair { msg2, pending }) => {
                events.push(HostEvent::Send(Bytes::from(msg2)));
                self.stage = Stage::AwaitMsg3(Box::new(pending));
            }
            Ok(HostStep::Connect { msg2, transport }) => {
                let session = Session::new(transport, Some(now_ms));
                let Some(device_key) = session.remote_static() else {
                    events.push(HostEvent::Failed(HostError::Handshake(
                        HandshakeError::WrongState,
                    )));
                    return;
                };
                events.push(HostEvent::Send(Bytes::from(msg2)));
                events.push(HostEvent::DeviceConnecting { device_key });
                self.stage = Stage::Open(Box::new(session));
            }
        }
    }

    fn on_msg3(
        &mut self,
        pending: HostHandshake,
        payload: &[u8],
        now_ms: u64,
        events: &mut Vec<HostEvent>,
    ) {
        let (request_bytes, device_key, transport) = match pending.read_message3(payload) {
            Ok(parts) => parts,
            Err(error) => {
                events.push(HostEvent::Failed(HostError::Handshake(error)));
                return;
            }
        };
        let mut session = Session::new(transport, Some(now_ms));
        match PairRequest::decode(&request_bytes) {
            Ok(request) => {
                events.push(HostEvent::PairRequested {
                    device_key,
                    request,
                });
                self.stage = Stage::Open(Box::new(session));
            }
            Err(error) => {
                send_protocol_error(&mut session, now_ms, events);
                events.push(HostEvent::Failed(HostError::PairRequest(error)));
            }
        }
    }

    fn on_record(
        &mut self,
        mut session: Box<Session>,
        record: &[u8],
        now_ms: u64,
        events: &mut Vec<HostEvent>,
    ) {
        match session.open(record, now_ms) {
            Err(error) => events.push(HostEvent::Failed(HostError::Record(error))),
            Ok(None) => self.stage = Stage::Open(session),
            Ok(Some(bytes)) => match message::decode(&bytes, Direction::ClientToHost) {
                Ok(message) => {
                    events.push(HostEvent::Message(message));
                    self.stage = Stage::Open(session);
                }
                Err(error) => {
                    send_protocol_error(&mut session, now_ms, events);
                    events.push(HostEvent::Failed(HostError::Message(error)));
                }
            },
        }
    }
}

impl fmt::Debug for HostChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self.stage {
            Stage::AwaitStart(_) => "await-start",
            Stage::AwaitMsg3(_) => "await-msg3",
            Stage::Open(_) => "open",
            Stage::Closed => "closed",
        };
        f.debug_struct("HostChannel")
            .field("stage", &stage)
            .finish_non_exhaustive()
    }
}

/// Spec 03 §1: who detects a protocol error answers `error { protocol }`
/// before closing.
fn send_protocol_error(session: &mut Session, now_ms: u64, events: &mut Vec<HostEvent>) {
    let reply = Message::Error(ErrorMessage {
        code: ErrorCode::Protocol,
        id: None,
        pane: None,
    });
    if let Ok(records) = session.seal(&message::encode(&reply), now_ms) {
        events.extend(records.into_iter().map(|r| HostEvent::Send(Bytes::from(r))));
    }
}
