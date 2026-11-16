// SPDX-License-Identifier: GPL-3.0-or-later

//! The client side of a session as a sans-IO state machine (spec 01 §4 and
//! spec 02): the Porecatu Remote app talking to a host through the relay.
//!
//! [`ClientSession`] works with **WebSocket binary frames of the client
//! connection** (`/v0/client/<host_id>`): it takes the frames the relay sent
//! and gives back the frames to write, already wrapped in the `data` envelope.
//! Layer by layer, it covers the envelope ([`ClientEnvelope`]), the Noise
//! handshake (`XX` to pair, `IK` to connect) and the encrypted records, and
//! hands the application [`Message`]s.
//!
//! # Outputs
//!
//! Everything that comes out of [`receive`](ClientSession::receive) and
//! [`tick`](ClientSession::tick) is a [`ClientEvent`], in order. Frames to
//! write are [`ClientEvent::Send`] events, interleaved with the others, so the
//! order between "send this" and "this happened" is never lost. The
//! constructors and [`send`](ClientSession::send) return frames only.
//!
//! # Errors
//!
//! A failure caused by the peer or by the network (bad record, host key
//! mismatch, deadline) is an event, [`ClientEvent::Failed`], after which the
//! session is closed. `Err` is reserved for the caller's own misuse (a call on
//! a closed session, a message the client may not send) and for the failures
//! of [`send`](ClientSession::send), which has no event list to carry them.
//!
//! The session **does not send `hello` by itself**: when
//! [`ClientEvent::HandshakeComplete`] arrives, the application decides what
//! comes first (spec 03 §2).

use core::fmt;

use bytes::Bytes;
use zeroize::Zeroizing;

use super::HANDSHAKE_TIMEOUT_MS;
use super::handshake::{ClientHandshake, ClientStep, HandshakeError};
use super::keys::StaticKeypair;
use super::record::{RecordError, Session};
use crate::envelope::{
    self, ClientEnvelope, CloseReason, EnvelopeError, HandshakeStart, HandshakeStartKind, Payload,
};
use crate::host_id::HostId;
use crate::message::{
    self, DecodeError, Direction, ErrorCode, ErrorMessage, Message, PairPlatform, PairRequest,
    PairRequestError,
};
use crate::pairing_uri::{PROTOCOL_MAJOR, PairingUri};

/// Why a [`ClientSession`] failed or refused a call. Typed codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientError {
    /// The invitation is for a protocol major version this code does not
    /// speak.
    UnsupportedMajor,
    /// The device name is empty or longer than 64 bytes.
    PairRequest(PairRequestError),
    /// The handshake failed. [`HandshakeError::HostKeyMismatch`] is the QR
    /// identity error: show it to the user; nothing was sent.
    Handshake(HandshakeError),
    /// The relay sent a frame that is not a valid client envelope.
    Envelope(EnvelopeError),
    /// A record failed: the session is closed (there is no resend).
    Record(RecordError),
    /// The host sent a message that does not decode. The client answered
    /// `error { protocol }` and closed the session.
    Message(DecodeError),
    /// The handshake did not finish within
    /// [`HANDSHAKE_TIMEOUT_MS`](super::HANDSHAKE_TIMEOUT_MS).
    Timeout,
    /// The session already ended.
    Closed,
    /// The handshake has not finished: there is no channel to send on yet.
    NotOpen,
    /// The message is one the client must not send (spec 03 §10).
    NotSendable,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::UnsupportedMajor => f.write_str("unsupported protocol major version"),
            ClientError::PairRequest(e) => write!(f, "invalid pair request: {e}"),
            ClientError::Handshake(e) => write!(f, "handshake failed: {e}"),
            ClientError::Envelope(e) => write!(f, "invalid relay frame: {e}"),
            ClientError::Record(e) => write!(f, "record failed: {e}"),
            ClientError::Message(e) => write!(f, "invalid host message: {e}"),
            ClientError::Timeout => f.write_str("handshake timed out"),
            ClientError::Closed => f.write_str("session closed"),
            ClientError::NotOpen => f.write_str("session not open yet"),
            ClientError::NotSendable => f.write_str("message not sendable by the client"),
        }
    }
}

impl core::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            ClientError::PairRequest(e) => Some(e),
            ClientError::Handshake(e) => Some(e),
            ClientError::Envelope(e) => Some(e),
            ClientError::Record(e) => Some(e),
            ClientError::Message(e) => Some(e),
            _ => None,
        }
    }
}

/// What happened on a [`ClientSession`], in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    /// A frame to write on the WebSocket (a `data` envelope).
    Send(Bytes),
    /// The transport is open: after `IK` when `msg2` is read, after `XX` when
    /// `msg3` is written. Messages can be sent from now on.
    HandshakeComplete,
    /// A message from the host.
    Message(Message),
    /// The relay has no host registered under that `host_id`. The session is
    /// closed.
    HostUnavailable,
    /// The relay closed the channel. The session is closed.
    Closed(CloseReason),
    /// The session failed and is closed. After a message that does not
    /// decode, an `error { protocol }` [`Send`](ClientEvent::Send) precedes
    /// this event.
    Failed(ClientError),
}

enum Stage {
    Handshaking {
        handshake: Box<ClientHandshake>,
        /// The encoded `pair-request`, for `XX`; a secret (it carries the
        /// token), zeroed when dropped.
        pair_request: Option<Zeroizing<Vec<u8>>>,
    },
    Open(Box<Session>),
    Closed,
}

/// The client side of one channel to a host.
pub struct ClientSession {
    stage: Stage,
    deadline_ms: u64,
}

impl ClientSession {
    /// Starts a pairing (`XX`) from the invitation of a QR code.
    ///
    /// Returns the session and the frames to write: one `data` carrying
    /// `handshake-start [0, msg1]`.
    ///
    /// # Errors
    ///
    /// [`ClientError::UnsupportedMajor`] for an invitation of another major
    /// version, [`ClientError::PairRequest`] for a device name of 0 or more
    /// than 64 bytes (see
    /// [`clamp_device_name`](crate::pairing_uri::clamp_device_name)), and
    /// [`ClientError::Handshake`] for setup failures.
    pub fn pair(
        invite: &PairingUri,
        device: &StaticKeypair,
        device_name: &str,
        platform: PairPlatform,
        now_ms: u64,
    ) -> Result<(ClientSession, Vec<Bytes>), ClientError> {
        Self::pair_with(
            invite,
            device,
            device_name,
            platform,
            now_ms,
            ClientHandshake::pair,
        )
    }

    /// Like [`pair`](Self::pair) with a fixed ephemeral key, for test vectors.
    ///
    /// # Errors
    ///
    /// As [`pair`](Self::pair).
    #[cfg(any(test, feature = "test-vectors"))]
    pub fn pair_with_ephemeral(
        invite: &PairingUri,
        device: &StaticKeypair,
        device_name: &str,
        platform: PairPlatform,
        now_ms: u64,
        ephemeral: &[u8; 32],
    ) -> Result<(ClientSession, Vec<Bytes>), ClientError> {
        Self::pair_with(
            invite,
            device,
            device_name,
            platform,
            now_ms,
            |local, host_key, host_id| {
                ClientHandshake::pair_with_ephemeral(local, host_key, host_id, ephemeral)
            },
        )
    }

    /// `start` begins the `XX` handshake: with a random or a fixed ephemeral key.
    fn pair_with(
        invite: &PairingUri,
        device: &StaticKeypair,
        device_name: &str,
        platform: PairPlatform,
        now_ms: u64,
        start: impl FnOnce(
            &StaticKeypair,
            &[u8; 32],
            &str,
        ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError>,
    ) -> Result<(ClientSession, Vec<Bytes>), ClientError> {
        if invite.major != PROTOCOL_MAJOR {
            return Err(ClientError::UnsupportedMajor);
        }
        let request = PairRequest {
            token: invite.token,
            device: device_name.to_owned(),
            platform,
        };
        let encoded = Zeroizing::new(request.encode().map_err(ClientError::PairRequest)?);
        let (handshake, msg1) = start(device, &invite.host_key, invite.host_id.as_str())
            .map_err(ClientError::Handshake)?;
        let session = ClientSession {
            stage: Stage::Handshaking {
                handshake: Box::new(handshake),
                pair_request: Some(encoded),
            },
            deadline_ms: now_ms.saturating_add(HANDSHAKE_TIMEOUT_MS),
        };
        let frame = start_frame(HandshakeStartKind::Pair, msg1);
        Ok((session, vec![frame]))
    }

    /// Starts the connection (`IK`) of an already paired device.
    ///
    /// Returns the session and the frames to write: one `data` carrying
    /// `handshake-start [1, msg1]`.
    ///
    /// # Errors
    ///
    /// [`ClientError::Handshake`] when `host_key` is not a usable key.
    pub fn connect(
        host_id: &HostId,
        host_key: &[u8; 32],
        device: &StaticKeypair,
        now_ms: u64,
    ) -> Result<(ClientSession, Vec<Bytes>), ClientError> {
        Self::connect_with(host_id, host_key, device, now_ms, ClientHandshake::connect)
    }

    /// Like [`connect`](Self::connect) with a fixed ephemeral key, for test
    /// vectors.
    ///
    /// # Errors
    ///
    /// As [`connect`](Self::connect).
    #[cfg(any(test, feature = "test-vectors"))]
    pub fn connect_with_ephemeral(
        host_id: &HostId,
        host_key: &[u8; 32],
        device: &StaticKeypair,
        now_ms: u64,
        ephemeral: &[u8; 32],
    ) -> Result<(ClientSession, Vec<Bytes>), ClientError> {
        Self::connect_with(
            host_id,
            host_key,
            device,
            now_ms,
            |local, host_key, host_id| {
                ClientHandshake::connect_with_ephemeral(local, host_key, host_id, ephemeral)
            },
        )
    }

    /// `start` begins the `IK` handshake: with a random or a fixed ephemeral key.
    fn connect_with(
        host_id: &HostId,
        host_key: &[u8; 32],
        device: &StaticKeypair,
        now_ms: u64,
        start: impl FnOnce(
            &StaticKeypair,
            &[u8; 32],
            &str,
        ) -> Result<(ClientHandshake, Vec<u8>), HandshakeError>,
    ) -> Result<(ClientSession, Vec<Bytes>), ClientError> {
        let (handshake, msg1) =
            start(device, host_key, host_id.as_str()).map_err(ClientError::Handshake)?;
        let session = ClientSession {
            stage: Stage::Handshaking {
                handshake: Box::new(handshake),
                pair_request: None,
            },
            deadline_ms: now_ms.saturating_add(HANDSHAKE_TIMEOUT_MS),
        };
        let frame = start_frame(HandshakeStartKind::Connect, msg1);
        Ok((session, vec![frame]))
    }

    /// Whether the transport is open and messages can be sent.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self.stage, Stage::Open(_))
    }

    /// Whether the handshake is still running.
    #[must_use]
    pub fn is_handshaking(&self) -> bool {
        matches!(self.stage, Stage::Handshaking { .. })
    }

    /// Whether the session ended (failure, close by the relay, or
    /// `host-unavailable`).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        matches!(self.stage, Stage::Closed)
    }

    /// Takes one frame the relay sent on the client connection.
    ///
    /// # Errors
    ///
    /// [`ClientError::Closed`] when the session already ended. Every other
    /// failure comes as a [`ClientEvent::Failed`] in the returned list.
    pub fn receive(&mut self, frame: Bytes, now_ms: u64) -> Result<Vec<ClientEvent>, ClientError> {
        if self.is_closed() {
            return Err(ClientError::Closed);
        }
        let mut events = Vec::new();
        if self.handshake_overdue(now_ms) {
            events.push(self.fail(ClientError::Timeout));
            return Ok(events);
        }

        let envelope = match envelope::decode_client(frame) {
            Ok(envelope) => envelope,
            Err(error) => {
                events.push(self.fail(ClientError::Envelope(error)));
                return Ok(events);
            }
        };
        match envelope {
            ClientEnvelope::HostUnavailable => {
                self.stage = Stage::Closed;
                events.push(ClientEvent::HostUnavailable);
            }
            ClientEnvelope::Closed { reason } => {
                self.stage = Stage::Closed;
                events.push(ClientEvent::Closed(reason));
            }
            ClientEnvelope::Data { payload } => {
                self.on_data(payload.as_bytes(), now_ms, &mut events);
            }
        }
        Ok(events)
    }

    /// Seals `message` into the frames to write, in order.
    ///
    /// # Errors
    ///
    /// [`ClientError::NotOpen`] before the handshake ends,
    /// [`ClientError::Closed`] after the session ended,
    /// [`ClientError::NotSendable`] for a message only the host sends, and
    /// [`ClientError::Record`] when sealing fails (the session is closed
    /// then).
    pub fn send(&mut self, message: &Message, now_ms: u64) -> Result<Vec<Bytes>, ClientError> {
        let session = match &mut self.stage {
            Stage::Open(session) => session,
            Stage::Handshaking { .. } => return Err(ClientError::NotOpen),
            Stage::Closed => return Err(ClientError::Closed),
        };
        if !message.can_be_sent_by(Direction::ClientToHost) {
            return Err(ClientError::NotSendable);
        }
        match session.seal(&message::encode(message), now_ms) {
            Ok(records) => Ok(records.into_iter().map(data_frame).collect()),
            Err(error) => {
                self.stage = Stage::Closed;
                Err(ClientError::Record(error))
            }
        }
    }

    /// Advances the clock. The only deadline is the one of the handshake:
    /// when it passes, the session fails with [`ClientError::Timeout`].
    pub fn tick(&mut self, now_ms: u64) -> Vec<ClientEvent> {
        if self.handshake_overdue(now_ms) {
            vec![self.fail(ClientError::Timeout)]
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

    fn fail(&mut self, error: ClientError) -> ClientEvent {
        self.stage = Stage::Closed;
        ClientEvent::Failed(error)
    }

    fn on_data(&mut self, record: &[u8], now_ms: u64, events: &mut Vec<ClientEvent>) {
        match core::mem::replace(&mut self.stage, Stage::Closed) {
            Stage::Closed => {}
            Stage::Handshaking {
                handshake,
                pair_request,
            } => self.on_handshake_data(*handshake, pair_request, record, now_ms, events),
            Stage::Open(mut session) => {
                match session.open(record, now_ms) {
                    Err(error) => events.push(ClientEvent::Failed(ClientError::Record(error))),
                    Ok(None) => self.stage = Stage::Open(session),
                    Ok(Some(bytes)) => match message::decode(&bytes, Direction::HostToClient) {
                        Ok(message) => {
                            events.push(ClientEvent::Message(message));
                            self.stage = Stage::Open(session);
                        }
                        Err(error) => {
                            // Spec 03 §1: who detects the error answers
                            // `error { protocol }` and closes the session.
                            let reply = Message::Error(ErrorMessage {
                                code: ErrorCode::Protocol,
                                id: None,
                                pane: None,
                            });
                            if let Ok(records) = session.seal(&message::encode(&reply), now_ms) {
                                events.extend(
                                    records
                                        .into_iter()
                                        .map(|r| ClientEvent::Send(data_frame(r))),
                                );
                            }
                            events.push(ClientEvent::Failed(ClientError::Message(error)));
                        }
                    },
                }
            }
        }
    }

    fn on_handshake_data(
        &mut self,
        mut handshake: ClientHandshake,
        pair_request: Option<Zeroizing<Vec<u8>>>,
        msg2: &[u8],
        now_ms: u64,
        events: &mut Vec<ClientEvent>,
    ) {
        // Any error below leaves the stage `Closed`: the client sends nothing
        // more (spec 02 §2).
        match handshake.read_message2(msg2) {
            Err(error) => events.push(ClientEvent::Failed(ClientError::Handshake(error))),
            Ok(ClientStep::Established(transport)) => {
                self.stage = Stage::Open(Box::new(Session::new(transport, Some(now_ms))));
                events.push(ClientEvent::HandshakeComplete);
            }
            Ok(ClientStep::NeedsPairRequest) => {
                let Some(request) = pair_request else {
                    events.push(ClientEvent::Failed(ClientError::Handshake(
                        HandshakeError::WrongState,
                    )));
                    return;
                };
                match handshake.write_message3(&request) {
                    Err(error) => events.push(ClientEvent::Failed(ClientError::Handshake(error))),
                    Ok((msg3, transport)) => {
                        events.push(ClientEvent::Send(data_frame(msg3)));
                        self.stage = Stage::Open(Box::new(Session::new(transport, Some(now_ms))));
                        events.push(ClientEvent::HandshakeComplete);
                    }
                }
            }
        }
    }
}

impl fmt::Debug for ClientSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self.stage {
            Stage::Handshaking { .. } => "handshaking",
            Stage::Open(_) => "open",
            Stage::Closed => "closed",
        };
        f.debug_struct("ClientSession")
            .field("stage", &stage)
            .finish_non_exhaustive()
    }
}

/// The `data` frame carrying a Noise message or record.
fn data_frame(payload: Vec<u8>) -> Bytes {
    Bytes::from(envelope::encode_client(&ClientEnvelope::Data {
        payload: Payload::from_bytes(Bytes::from(payload)),
    }))
}

/// The first frame of a channel: `data` carrying `handshake-start`.
fn start_frame(kind: HandshakeStartKind, msg1: Vec<u8>) -> Bytes {
    let start = HandshakeStart {
        kind,
        msg1: Bytes::from(msg1),
    };
    data_frame(envelope::encode_handshake_start(&start))
}
