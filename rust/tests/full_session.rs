// SPDX-License-Identifier: GPL-3.0-or-later

//! Whole conversations between a [`ClientSession`] and a [`HostChannel`], in
//! memory, through a test "relay" that only turns `data` frames into
//! payloads and back.

use std::collections::VecDeque;

use bytes::Bytes;
use porecatu_protocol::envelope::{
    self, ClientEnvelope, CloseReason, HandshakeStart, HandshakeStartKind, Payload,
};
use porecatu_protocol::host_id::{self, HostId};
use porecatu_protocol::message::{
    self, Color, Cursor, CursorShape, Direction, ErrorCode, ErrorMessage, Goodbye, GoodbyeReason,
    GridAck, GridSnapshot, Hello, Line, Message, PairAccepted, PairPlatform, Paste, Span, SpanText,
    Style, Subscribe, Version, ViewTab, Welcome,
};
use porecatu_protocol::pairing_uri::{PROTOCOL_MAJOR, PairingUri};
use porecatu_protocol::session::{
    ClientError, ClientEvent, ClientHandshake, ClientSession, ClientStep, HANDSHAKE_TIMEOUT_MS,
    HandshakeError, HandshakeKind, HostChannel, HostError, HostEvent, HostHandshake, HostStep,
    PairingToken, Session, StaticKeypair,
};

const RELAY: &str = "wss://relay.example.org";
const HOST_NAME: &str = "Workstation";

/// Everything about a host that the tests need.
struct Host {
    keys: StaticKeypair,
    id: HostId,
}

impl Host {
    fn new() -> Host {
        Host {
            keys: StaticKeypair::generate(),
            id: host_id::host_id(&[0x42; 32]),
        }
    }

    fn invite(&self, token: &PairingToken) -> PairingUri {
        PairingUri {
            major: PROTOCOL_MAJOR,
            relay: RELAY.to_owned(),
            host_id: self.id.clone(),
            host_key: self.keys.public(),
            token: token.bytes(),
            name: HOST_NAME.to_owned(),
        }
    }
}

/// A client and a host channel with a relay between them.
struct Rig {
    client: ClientSession,
    host: HostChannel,
    /// Frames the client wrote, waiting for the relay.
    to_host: VecDeque<Bytes>,
    /// Payloads the host wrote, waiting for the relay.
    to_client: VecDeque<Bytes>,
    client_log: Vec<ClientEvent>,
    host_log: Vec<HostEvent>,
    now: u64,
}

impl Rig {
    fn new(client: ClientSession, first: Vec<Bytes>, host: HostChannel) -> Rig {
        Rig {
            client,
            host,
            to_host: first.into(),
            to_client: VecDeque::new(),
            client_log: Vec::new(),
            host_log: Vec::new(),
            now: 0,
        }
    }

    /// Delivers everything in flight until both queues are empty.
    fn pump(&mut self) {
        loop {
            if let Some(frame) = self.to_host.pop_front() {
                // The relay: `data` becomes a channel payload.
                let ClientEnvelope::Data { payload } = envelope::decode_client(frame).unwrap()
                else {
                    panic!("the client only writes data frames");
                };
                let Ok(events) = self.host.receive(payload.as_bytes(), self.now) else {
                    continue;
                };
                self.absorb_host(events);
            } else if let Some(payload) = self.to_client.pop_front() {
                // The relay: a channel payload becomes `data`.
                let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::Data {
                    payload: Payload::from_bytes(payload),
                }));
                let Ok(events) = self.client.receive(frame, self.now) else {
                    continue;
                };
                self.absorb_client(events);
            } else {
                return;
            }
        }
    }

    fn absorb_host(&mut self, events: Vec<HostEvent>) {
        for event in events {
            match event {
                HostEvent::Send(payload) => self.to_client.push_back(payload),
                other => self.host_log.push(other),
            }
        }
    }

    fn absorb_client(&mut self, events: Vec<ClientEvent>) {
        for event in events {
            match event {
                ClientEvent::Send(frame) => self.to_host.push_back(frame),
                other => self.client_log.push(other),
            }
        }
    }

    fn client_sends(&mut self, message: &Message) {
        let frames = self.client.send(message, self.now).unwrap();
        self.to_host.extend(frames);
        self.pump();
    }

    fn host_sends(&mut self, message: &Message) {
        let payloads = self.host.send(message, self.now).unwrap();
        self.to_client.extend(payloads);
        self.pump();
    }

    /// The messages the client received so far.
    fn client_messages(&self) -> Vec<&Message> {
        self.client_log
            .iter()
            .filter_map(|e| match e {
                ClientEvent::Message(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    /// The messages the host received so far.
    fn host_messages(&self) -> Vec<&Message> {
        self.host_log
            .iter()
            .filter_map(|e| match e {
                HostEvent::Message(m) => Some(m),
                _ => None,
            })
            .collect()
    }
}

fn pairing_rig(host: &Host, device: &StaticKeypair, token: &PairingToken) -> Rig {
    let (client, first) = ClientSession::pair(
        &host.invite(token),
        device,
        "Pixel 9",
        PairPlatform::Android,
        0,
    )
    .unwrap();
    Rig::new(client, first, HostChannel::new(&host.keys, &host.id, 0))
}

fn connect_rig(host: &Host, device: &StaticKeypair) -> Rig {
    let (client, first) = ClientSession::connect(&host.id, &host.keys.public(), device, 0).unwrap();
    Rig::new(client, first, HostChannel::new(&host.keys, &host.id, 0))
}

fn hello() -> Message {
    Message::Hello(Hello {
        version: Version { major: 0, minor: 2 },
        app: "porecatu-remote".to_owned(),
        app_version: "0.1.0".to_owned(),
    })
}

fn welcome() -> Message {
    Message::Welcome(Welcome {
        version: Version { major: 0, minor: 2 },
        host_name: HOST_NAME.to_owned(),
        porecatu_version: "0.9.11".to_owned(),
        max_subscriptions: 4,
        host_instance: [3; 16],
    })
}

fn snapshot(seq: u64, text: &str) -> GridSnapshot {
    GridSnapshot {
        pane: 7,
        seq,
        cols: 80,
        rows: 24,
        history: 0,
        dropped: 0,
        styles: vec![Style {
            fg: Color::Default,
            bg: Color::Default,
            flags: 0,
        }],
        lines: vec![Line {
            wrapped: false,
            spans: vec![Span {
                col: 0,
                width: 1,
                style: 0,
                text: SpanText::Plain(text.to_owned()),
            }],
        }],
        cursor: Cursor {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: true,
        },
        alt: false,
    }
}

/// Runs the whole conversation after the transport is open: `hello`,
/// `welcome`, `subscribe`, `grid-snapshot`, `grid-ack`.
fn converse(rig: &mut Rig) {
    rig.client_sends(&hello());
    assert!(rig.host_messages().contains(&&hello()));
    rig.host_sends(&welcome());
    assert!(rig.client_messages().contains(&&welcome()));

    let subscribe = Message::Subscribe(Subscribe { pane: 7 });
    rig.client_sends(&subscribe);
    assert!(rig.host_messages().contains(&&subscribe));

    let grid = Message::GridSnapshot(snapshot(1, "$ "));
    rig.host_sends(&grid);
    assert!(rig.client_messages().contains(&&grid));

    let ack = Message::GridAck(GridAck { pane: 7, seq: 1 });
    rig.client_sends(&ack);
    assert!(rig.host_messages().contains(&&ack));
}

/// Pairs `device` with `host`; returns the rig with the pairing already
/// accepted.
fn pair_device(host: &Host, device: &StaticKeypair) -> Rig {
    let mut token = PairingToken::generate(0);
    let mut rig = pairing_rig(host, device, &token);
    rig.pump();

    // The desktop's side of the pairing: check the token, then answer.
    let Some(HostEvent::PairRequested {
        device_key,
        request,
    }) = rig.host_log.last().cloned()
    else {
        panic!("the host must report the pair request: {:?}", rig.host_log);
    };
    assert_eq!(device_key, device.public());
    assert_eq!(request.device, "Pixel 9");
    assert_eq!(request.platform, PairPlatform::Android);
    assert!(token.matches_and_consume(&request.token, 1_000));
    assert!(
        rig.client_log.contains(&ClientEvent::HandshakeComplete),
        "XX completes on the client when msg3 is written"
    );

    rig.host_sends(&Message::PairAccepted(PairAccepted {
        host_name: HOST_NAME.to_owned(),
    }));
    assert_eq!(
        rig.client_messages().last(),
        Some(&&Message::PairAccepted(PairAccepted {
            host_name: HOST_NAME.to_owned()
        }))
    );
    rig
}

#[test]
fn pairing_then_a_full_conversation() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let mut rig = pair_device(&host, &device);
    // The session continues as a normal connection, with no reconnect.
    converse(&mut rig);
    assert!(rig.client.is_open());
    assert!(rig.host.is_open());
}

#[test]
fn client_reports_the_tab_it_shows_with_view_tab() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let mut rig = pair_device(&host, &device);
    converse(&mut rig);

    let shown = Message::ViewTab(ViewTab { tab: Some(5) });
    rig.client_sends(&shown);
    let none = Message::ViewTab(ViewTab { tab: None });
    rig.client_sends(&none);

    let received: Vec<&Message> = rig
        .host_messages()
        .into_iter()
        .filter(|m| matches!(m, Message::ViewTab(_)))
        .collect();
    assert_eq!(received, [&shown, &none]);
    assert!(rig.client.is_open());
    assert!(rig.host.is_open());
}

#[test]
fn paired_device_connects_with_ik() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let paired = [device.public()];
    drop(pair_device(&host, &device));

    let mut rig = connect_rig(&host, &device);
    rig.pump();
    assert!(rig.client_log.contains(&ClientEvent::HandshakeComplete));
    let Some(HostEvent::DeviceConnecting { device_key }) = rig.host_log.last().cloned() else {
        panic!(
            "the host must report the connecting device: {:?}",
            rig.host_log
        );
    };
    assert!(
        paired.contains(&device_key),
        "the desktop finds the device paired"
    );
    converse(&mut rig);
}

#[test]
fn unknown_device_gets_not_paired() {
    let host = Host::new();
    let stranger = StaticKeypair::generate();
    let paired: [[u8; 32]; 0] = [];

    let mut rig = connect_rig(&host, &stranger);
    rig.pump();
    // The handshake completed on both sides before any refusal (spec 02 §4).
    assert!(rig.client_log.contains(&ClientEvent::HandshakeComplete));
    let Some(HostEvent::DeviceConnecting { device_key }) = rig.host_log.last().cloned() else {
        panic!(
            "the host must report the connecting device: {:?}",
            rig.host_log
        );
    };
    assert_eq!(device_key, stranger.public());
    assert!(!paired.contains(&device_key));

    let payloads = rig.host.reject_not_paired(0).unwrap();
    rig.to_client.extend(payloads);
    rig.pump();
    assert_eq!(
        rig.client_messages(),
        vec![&Message::Error(ErrorMessage {
            code: ErrorCode::NotPaired,
            id: None,
            pane: None,
        })]
    );
    assert!(rig.host.is_closed());
    assert_eq!(rig.host.reject_not_paired(0), Err(HostError::Closed));
}

#[test]
fn wrong_qr_key_stops_the_client_without_sending() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let impostor = StaticKeypair::generate();
    let token = PairingToken::generate(0);
    let mut invite = host.invite(&token);
    invite.host_key = impostor.public();

    let (client, first) =
        ClientSession::pair(&invite, &device, "Pixel 9", PairPlatform::Ios, 0).unwrap();
    let mut rig = Rig::new(client, first, HostChannel::new(&host.keys, &host.id, 0));
    rig.pump();

    assert_eq!(
        rig.client_log,
        vec![ClientEvent::Failed(ClientError::Handshake(
            HandshakeError::HostKeyMismatch
        ))]
    );
    assert!(rig.client.is_closed());
    // Nothing was sent after msg1: no msg3 reached the host.
    assert!(rig.to_host.is_empty());
    assert!(rig.host.is_handshaking());
    assert!(rig.host_log.is_empty());
    assert_eq!(
        rig.client.send(&hello(), 0),
        Err(ClientError::Closed),
        "a closed session sends nothing"
    );
}

#[test]
fn handshake_deadline_on_both_sides() {
    let host = Host::new();
    let device = StaticKeypair::generate();

    let (mut client, _first) =
        ClientSession::connect(&host.id, &host.keys.public(), &device, 1_000).unwrap();
    let mut channel = HostChannel::new(&host.keys, &host.id, 1_000);
    let deadline = 1_000 + HANDSHAKE_TIMEOUT_MS;
    assert_eq!(client.next_deadline(), Some(deadline));
    assert_eq!(channel.next_deadline(), Some(deadline));

    assert!(client.tick(deadline - 1).is_empty());
    assert!(channel.tick(deadline - 1).is_empty());
    assert!(!client.is_closed());

    assert_eq!(
        client.tick(deadline),
        vec![ClientEvent::Failed(ClientError::Timeout)]
    );
    assert!(client.is_closed());
    assert_eq!(client.next_deadline(), None);
    assert!(client.tick(deadline + 1).is_empty());

    let events = channel.tick(deadline);
    assert_eq!(events, vec![HostEvent::Failed(HostError::Timeout)]);
    assert_eq!(HostError::Timeout.close_reason(), CloseReason::HostRejected);
    assert!(channel.is_closed());
    assert_eq!(channel.receive(&[0], deadline), Err(HostError::Closed));
}

#[test]
fn late_data_counts_as_a_timeout() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let mut rig = connect_rig(&host, &device);
    rig.now = HANDSHAKE_TIMEOUT_MS;
    rig.pump();
    assert_eq!(rig.host_log, vec![HostEvent::Failed(HostError::Timeout)]);
}

#[test]
fn the_deadline_ends_once_the_transport_opens() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let mut rig = connect_rig(&host, &device);
    assert!(rig.client.next_deadline().is_some());
    rig.pump();
    assert_eq!(rig.client.next_deadline(), None);
    assert_eq!(rig.host.next_deadline(), None);
    assert!(rig.client.tick(10 * HANDSHAKE_TIMEOUT_MS).is_empty());
    assert!(rig.host.tick(10 * HANDSHAKE_TIMEOUT_MS).is_empty());
}

#[test]
fn a_message_of_200_000_bytes_crosses_in_fragments() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let mut rig = connect_rig(&host, &device);
    rig.pump();

    // Client to host.
    let paste = Message::Paste(Paste {
        pane: 7,
        text: "abcdefghij".repeat(20_000),
    });
    let frames = rig.client.send(&paste, 0).unwrap();
    assert!(frames.len() >= 4, "200 000 bytes need 4 or more records");
    for frame in &frames {
        assert!(frame.len() <= envelope::MAX_FRAME);
    }
    rig.to_host.extend(frames);
    rig.pump();
    assert_eq!(rig.host_messages(), vec![&paste]);

    // Host to client.
    let big = Message::GridSnapshot(snapshot(2, &"x".repeat(200_000)));
    let payloads = rig.host.send(&big, 0).unwrap();
    assert!(payloads.len() >= 4);
    rig.to_client.extend(payloads);
    rig.pump();
    assert_eq!(rig.client_messages(), vec![&big]);
}

#[test]
fn relay_events_close_the_client() {
    let host = Host::new();
    let device = StaticKeypair::generate();

    let (mut client, _) =
        ClientSession::connect(&host.id, &host.keys.public(), &device, 0).unwrap();
    let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::HostUnavailable));
    assert_eq!(
        client.receive(frame, 0).unwrap(),
        vec![ClientEvent::HostUnavailable]
    );
    assert!(client.is_closed());
    assert_eq!(client.receive(Bytes::new(), 0), Err(ClientError::Closed));

    let (mut client, _) =
        ClientSession::connect(&host.id, &host.keys.public(), &device, 0).unwrap();
    let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::Closed {
        reason: CloseReason::HostRejected,
    }));
    assert_eq!(
        client.receive(frame, 0).unwrap(),
        vec![ClientEvent::Closed(CloseReason::HostRejected)]
    );
    assert!(client.is_closed());
}

#[test]
fn garbage_from_the_relay_fails_the_client() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let (mut client, _) =
        ClientSession::connect(&host.id, &host.keys.public(), &device, 0).unwrap();
    let events = client
        .receive(Bytes::from_static(&[0xff, 0x00]), 0)
        .unwrap();
    assert!(matches!(
        events.as_slice(),
        [ClientEvent::Failed(ClientError::Envelope(_))]
    ));
    assert!(client.is_closed());
}

#[test]
fn a_noise_message_that_does_not_decrypt_fails_the_client_silently() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let (mut client, _) =
        ClientSession::connect(&host.id, &host.keys.public(), &device, 0).unwrap();
    let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::Data {
        payload: Payload::from_bytes(Bytes::from(vec![7u8; 96])),
    }));
    let events = client.receive(frame, 0).unwrap();
    // No `Send`: the client sends nothing after a failed msg2 (spec 02 §2).
    assert!(matches!(
        events.as_slice(),
        [ClientEvent::Failed(ClientError::Handshake(_))]
    ));
}

#[test]
fn host_refuses_a_bad_handshake_start() {
    let host = Host::new();
    let mut channel = HostChannel::new(&host.keys, &host.id, 0);
    let events = channel.receive(&[0xf6], 0).unwrap();
    assert!(matches!(
        events.as_slice(),
        [HostEvent::Failed(HostError::HandshakeStart(_))]
    ));
    assert!(channel.is_closed());

    // An unknown handshake kind.
    let mut channel = HostChannel::new(&host.keys, &host.id, 0);
    let bad = [0x82, 0x02, 0x40]; // [2, h'']
    let events = channel.receive(&bad, 0).unwrap();
    assert!(matches!(
        events.as_slice(),
        [HostEvent::Failed(HostError::HandshakeStart(_))]
    ));
}

#[test]
fn host_refuses_a_prologue_of_another_host_id() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let other = host_id::host_id(&[0x43; 32]);
    // The client dials `other`, the channel belongs to `host`.
    let (_client, first) = ClientSession::connect(&other, &host.keys.public(), &device, 0).unwrap();
    let mut channel = HostChannel::new(&host.keys, &host.id, 0);
    let ClientEnvelope::Data { payload } = envelope::decode_client(first[0].clone()).unwrap()
    else {
        panic!("data expected");
    };
    let events = channel.receive(payload.as_bytes(), 0).unwrap();
    assert!(matches!(
        events.as_slice(),
        [HostEvent::Failed(HostError::Handshake(
            HandshakeError::Decrypt
        ))]
    ));
    assert_eq!(
        HostError::Handshake(HandshakeError::Decrypt).close_reason(),
        CloseReason::HostRejected
    );
    assert!(channel.is_closed());
}

/// A client written by hand, on top of the public handshake and record
/// layers, to send what [`ClientSession`] never would.
fn manual_client(host: &Host, channel: &mut HostChannel) -> Session {
    let device = StaticKeypair::generate();
    let (mut handshake, msg1) =
        ClientHandshake::connect(&device, &host.keys.public(), host.id.as_str()).unwrap();
    let start = envelope::encode_handshake_start(&HandshakeStart {
        kind: HandshakeStartKind::Connect,
        msg1: Bytes::from(msg1),
    });
    let events = channel.receive(&start, 0).unwrap();
    let HostEvent::Send(msg2) = &events[0] else {
        panic!("msg2 expected first: {events:?}");
    };
    let ClientStep::Established(transport) = handshake.read_message2(msg2).unwrap() else {
        panic!("IK must be established");
    };
    Session::new(transport, Some(0))
}

#[test]
fn host_answers_protocol_error_to_a_message_that_does_not_decode() {
    let host = Host::new();
    let mut channel = HostChannel::new(&host.keys, &host.id, 0);
    let mut client = manual_client(&host, &mut channel);

    let record = client.seal(&[0xf6], 0).unwrap().remove(0);
    let events = channel.receive(&record, 0).unwrap();
    let [
        HostEvent::Send(reply),
        HostEvent::Failed(HostError::Message(_)),
    ] = events.as_slice()
    else {
        panic!("error then failure expected: {events:?}");
    };
    let bytes = client.open(reply, 0).unwrap().unwrap();
    assert_eq!(
        message::decode(&bytes, Direction::HostToClient).unwrap(),
        Message::Error(ErrorMessage {
            code: ErrorCode::Protocol,
            id: None,
            pane: None,
        })
    );
    assert!(channel.is_closed());
    assert_eq!(
        HostError::Message(message::DecodeError::UnknownType(0)).close_reason(),
        CloseReason::Normal
    );
}

#[test]
fn host_refuses_a_message_only_the_host_sends() {
    let host = Host::new();
    let mut channel = HostChannel::new(&host.keys, &host.id, 0);
    let mut client = manual_client(&host, &mut channel);

    let record = client
        .seal(&message::encode(&welcome()), 0)
        .unwrap()
        .remove(0);
    let events = channel.receive(&record, 0).unwrap();
    assert!(matches!(
        events.as_slice(),
        [
            HostEvent::Send(_),
            HostEvent::Failed(HostError::Message(message::DecodeError::WrongDirection(1)))
        ]
    ));
}

#[test]
fn host_fails_on_a_corrupted_record_without_answering() {
    let host = Host::new();
    let mut channel = HostChannel::new(&host.keys, &host.id, 0);
    let mut client = manual_client(&host, &mut channel);

    let mut record = client
        .seal(&message::encode(&hello()), 0)
        .unwrap()
        .remove(0);
    let last = record.len() - 1;
    record[last] ^= 1;
    let events = channel.receive(&record, 0).unwrap();
    assert!(matches!(
        events.as_slice(),
        [HostEvent::Failed(HostError::Record(_))]
    ));
    assert!(channel.is_closed());
}

#[test]
fn client_answers_protocol_error_to_a_host_message_that_does_not_decode() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let (mut client, first) =
        ClientSession::connect(&host.id, &host.keys.public(), &device, 0).unwrap();
    let ClientEnvelope::Data { payload } = envelope::decode_client(first[0].clone()).unwrap()
    else {
        panic!("data expected");
    };
    let start = envelope::decode_handshake_start(payload.into_bytes()).unwrap();
    let HostStep::Connect { msg2, transport } = HostHandshake::respond(
        HandshakeKind::Connect,
        &host.keys,
        host.id.as_str(),
        &start.msg1,
    )
    .unwrap() else {
        panic!("IK expected");
    };
    let mut host_session = Session::new(transport, Some(0));

    let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::Data {
        payload: Payload::from_bytes(Bytes::from(msg2)),
    }));
    assert_eq!(
        client.receive(frame, 0).unwrap(),
        vec![ClientEvent::HandshakeComplete]
    );

    // A host that sends a message only the client may send.
    let record = host_session
        .seal(&message::encode(&hello()), 0)
        .unwrap()
        .remove(0);
    let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::Data {
        payload: Payload::from_bytes(Bytes::from(record)),
    }));
    let events = client.receive(frame, 0).unwrap();
    let [
        ClientEvent::Send(reply),
        ClientEvent::Failed(ClientError::Message(_)),
    ] = events.as_slice()
    else {
        panic!("error then failure expected: {events:?}");
    };
    let ClientEnvelope::Data { payload } = envelope::decode_client(reply.clone()).unwrap() else {
        panic!("data expected");
    };
    let bytes = host_session.open(payload.as_bytes(), 0).unwrap().unwrap();
    assert_eq!(
        message::decode(&bytes, Direction::ClientToHost).unwrap(),
        Message::Error(ErrorMessage {
            code: ErrorCode::Protocol,
            id: None,
            pane: None,
        })
    );
    assert!(client.is_closed());
}

#[test]
fn directions_are_enforced_when_sending() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let mut rig = connect_rig(&host, &device);

    // Before the handshake ends there is no channel to send on.
    assert_eq!(rig.client.send(&hello(), 0), Err(ClientError::NotOpen));
    assert_eq!(rig.host.send(&welcome(), 0), Err(HostError::NotOpen));

    rig.pump();
    assert_eq!(
        rig.client.send(&welcome(), 0),
        Err(ClientError::NotSendable)
    );
    assert_eq!(rig.host.send(&hello(), 0), Err(HostError::NotSendable));
    // Rejected sends do not close the session.
    assert!(rig.client.is_open() && rig.host.is_open());

    // `goodbye` goes both ways.
    let goodbye = Message::Goodbye(Goodbye {
        reason: GoodbyeReason::Normal,
    });
    rig.client_sends(&goodbye);
    rig.host_sends(&goodbye);
    assert_eq!(rig.host_messages(), vec![&goodbye]);
    assert_eq!(rig.client_messages(), vec![&goodbye]);
}

#[test]
fn pair_refuses_a_device_name_out_of_range_and_another_major() {
    let host = Host::new();
    let device = StaticKeypair::generate();
    let token = PairingToken::generate(0);

    for name in ["", &"x".repeat(65)] {
        let result =
            ClientSession::pair(&host.invite(&token), &device, name, PairPlatform::Other, 0);
        assert!(matches!(result, Err(ClientError::PairRequest(_))));
    }

    let mut invite = host.invite(&token);
    invite.major = 1;
    assert!(matches!(
        ClientSession::pair(&invite, &device, "ok", PairPlatform::Other, 0),
        Err(ClientError::UnsupportedMajor)
    ));
}

#[test]
fn a_used_token_cannot_pair_a_second_device() {
    let host = Host::new();
    let first = StaticKeypair::generate();
    let second = StaticKeypair::generate();
    let mut token = PairingToken::generate(0);

    // First device: the token matches and is consumed.
    let mut rig = pairing_rig(&host, &first, &token);
    rig.pump();
    let Some(HostEvent::PairRequested { request, .. }) = rig.host_log.last().cloned() else {
        panic!("pair request expected");
    };
    assert!(token.matches_and_consume(&request.token, 10));

    // Second device with the same QR: the token no longer matches.
    let mut rig = pairing_rig(&host, &second, &token);
    rig.pump();
    let Some(HostEvent::PairRequested { request, .. }) = rig.host_log.last().cloned() else {
        panic!("pair request expected");
    };
    assert!(!token.matches_and_consume(&request.token, 20));
}
