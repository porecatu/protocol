// SPDX-License-Identifier: GPL-3.0-or-later
//! Whole conversations (`sessoes/`): a handshake, the records of every
//! message and the exact WebSocket frames.
//!
//! Each step is one message (or one handshake message, or the host closing the
//! channel). The frames are the ones the **sender** writes: the client's are
//! `data` envelopes (`[16, payload]`), the host's are `channel-data` envelopes
//! (`[4, canal, payload]`) on its own connection to the relay; the relay
//! rewrites one into the other (`01` section 4), keeping the payload. The
//! first client frame carries a `handshake-start` (`[kind, msg1]`) as its
//! payload.
//!
//! Both sides run in this process, with the fixed keys and ephemerals: the
//! client seals, the host opens (and the other way round), so a vector is only
//! written when the two record layers agree with each other and with
//! `message::decode`.

use porecatu_protocol::PROTOCOL_VERSION;
use porecatu_protocol::cbor;
use porecatu_protocol::encoding::encode_hex;
use porecatu_protocol::envelope::{
    ChannelId, ClientEnvelope, CloseReason, HandshakeStart, HandshakeStartKind, HostEnvelope,
    Payload, encode_client, encode_handshake_start, encode_host,
};
use porecatu_protocol::message::{self, Direction};
use porecatu_protocol::session::{
    ClientHandshake, ClientStep, HandshakeKind, HostHandshake, HostStep, PairingToken, Session,
    StaticKeypair,
};
use serde_json::{Value as Json, json};

use super::duo::{Keys, fixed_host_id};
use super::{Generated, bytes, encode_json, map, seq};
use crate::typed_json;
use crate::vector::{Layer, Rejection, SessionResult, SessionStep, SessionVector};

const CHANNEL: u64 = 1;
const HOST_NAME: &str = "Workstation";

// ---------------------------------------------------------------------------
// The messages of the conversations
// ---------------------------------------------------------------------------

fn message_item(code: u64, body: Json) -> Json {
    json!([code, body])
}

fn pair_request(token: &[u8]) -> Json {
    map(vec![
        ("token", bytes(token)),
        ("device", json!("Pixel 8")),
        ("platform", json!(0)),
    ])
}

fn hello(major: u64, minor: u64) -> Json {
    message_item(
        0,
        map(vec![
            ("version", json!([major, minor])),
            ("app", json!("porecatu-remote")),
            ("app_version", json!("0.1.0")),
        ]),
    )
}

fn welcome() -> Json {
    message_item(
        1,
        map(vec![
            ("version", json!(PROTOCOL_VERSION)),
            ("host_name", json!(HOST_NAME)),
            ("porecatu_version", json!("0.9.11")),
            ("max_subscriptions", json!(4)),
            ("host_instance", bytes(&seq(0xa0, 16))),
        ]),
    )
}

fn incompatible() -> Json {
    message_item(2, map(vec![("version", json!(PROTOCOL_VERSION))]))
}

fn theme() -> Json {
    message_item(
        12,
        map(vec![
            ("fg", json!(0xd7_dce3_u64)),
            ("bg", json!(0x15_181d_u64)),
            ("cursor", json!(0x5e_d3bc_u64)),
            ("selection_bg", json!(0x2b_3a4a_u64)),
            ("palette", json!((0..16).collect::<Vec<u64>>())),
            ("group_colors", json!([0xff_0000_u64, 0x00_ff00_u64])),
        ]),
    )
}

fn profiles() -> Json {
    message_item(
        11,
        map(vec![
            (
                "list",
                json!([map(vec![
                    ("id", json!("auto:pwsh")),
                    ("name", json!("PowerShell")),
                    ("kind", json!(0)),
                ])]),
            ),
            ("default", json!("auto:pwsh")),
        ]),
    )
}

fn workspace() -> Json {
    let pane = map(vec![
        ("pane", json!(3)),
        ("title", json!("pwsh")),
        ("cwd", Json::Null),
        ("state", json!(0)),
        ("exit", Json::Null),
        ("cols", json!(12)),
        ("rows", json!(2)),
    ]);
    let tab = map(vec![
        ("tab", json!(2)),
        ("title", json!("pwsh")),
        ("custom", json!(false)),
        ("profile", Json::Null),
        ("remote", Json::Null),
        ("activity", json!(false)),
        ("bell", json!(false)),
        ("layout", pane),
        ("focused", json!(3)),
    ]);
    let window = map(vec![
        ("id", json!(1)),
        ("title", json!(HOST_NAME)),
        ("pinned", json!([])),
        ("items", json!([tab])),
        ("active", json!(2)),
    ]);
    message_item(
        10,
        map(vec![("rev", json!(1)), ("windows", json!([window]))]),
    )
}

fn subscribe() -> Json {
    message_item(20, map(vec![("pane", json!(3))]))
}

fn grid_snapshot() -> Json {
    message_item(
        22,
        map(vec![
            ("pane", json!(3)),
            ("seq", json!(0)),
            ("cols", json!(12)),
            ("rows", json!(2)),
            ("history", json!(0)),
            ("dropped", json!(0)),
            ("styles", json!([[Json::Null, Json::Null, 0]])),
            ("lines", json!([[false, [[0, 1, 0, "$ ls"]]], [false, []]])),
            (
                "cursor",
                map(vec![
                    ("row", json!(0)),
                    ("col", json!(4)),
                    ("shape", json!(0)),
                    ("visible", json!(true)),
                ]),
            ),
            ("alt", json!(false)),
        ]),
    )
}

fn grid_ack() -> Json {
    message_item(24, map(vec![("pane", json!(3)), ("seq", json!(0))]))
}

fn pair_accepted() -> Json {
    message_item(60, map(vec![("host_name", json!(HOST_NAME))]))
}

fn pair_rejected(reason: u64) -> Json {
    message_item(61, map(vec![("reason", json!(reason))]))
}

fn error_not_paired() -> Json {
    message_item(70, map(vec![("code", json!(1))]))
}

// ---------------------------------------------------------------------------
// The conversation builder
// ---------------------------------------------------------------------------

fn client_frame(payload: Vec<u8>) -> String {
    encode_hex(&encode_client(&ClientEnvelope::Data {
        payload: Payload::from_bytes(payload.into()),
    }))
}

fn host_frame(payload: Vec<u8>) -> String {
    encode_hex(&encode_host(&HostEnvelope::ChannelData {
        channel: ChannelId(CHANNEL),
        payload: Payload::from_bytes(payload.into()),
    }))
}

struct Convo {
    keys: Keys,
    handshake: &'static str,
    token_host: Option<[u8; 16]>,
    paired: Vec<[u8; 32]>,
    client: Option<Session>,
    host: Option<Session>,
    steps: Vec<SessionStep>,
}

impl Convo {
    fn new(handshake: &'static str, token_host: Option<[u8; 16]>, paired: Vec<[u8; 32]>) -> Self {
        Convo {
            keys: Keys::standard(),
            handshake,
            token_host,
            paired,
            client: None,
            host: None,
            steps: Vec::new(),
        }
    }

    fn push(
        &mut self,
        de: &str,
        now_ms: u64,
        mensagem: Json,
        records: &[Vec<u8>],
        frames: Vec<String>,
    ) {
        self.steps.push(SessionStep {
            de: de.to_owned(),
            now_ms,
            mensagem,
            registros_hex: records.iter().map(|r| encode_hex(r)).collect(),
            envelope_hex: frames,
        });
    }

    /// `XX`: `msg1` at `t[0]`, `msg2` at `t[1]`, `msg3` (the `pair-request`)
    /// at `t[2]`.
    fn start_xx(&mut self, t: [u64; 3], request: &Json) {
        let id = fixed_host_id();
        let (client_keys, host_keys) = (self.keys.client_pair(), self.keys.host_pair());
        let request_bytes = encode_json(request);

        let (mut client, msg1) = ClientHandshake::pair_with_ephemeral(
            &client_keys,
            &host_keys.public(),
            id.as_str(),
            &self.keys.client_ephemeral,
        )
        .expect("the client starts");
        let start = encode_handshake_start(&HandshakeStart {
            kind: HandshakeStartKind::Pair,
            msg1: msg1.clone().into(),
        });
        self.push(
            "cliente",
            t[0],
            Json::Null,
            std::slice::from_ref(&msg1),
            vec![client_frame(start)],
        );

        let HostStep::Pair { msg2, pending } = HostHandshake::respond_with_ephemeral(
            HandshakeKind::Pair,
            &host_keys,
            id.as_str(),
            &msg1,
            &self.keys.host_ephemeral,
        )
        .expect("the host answers") else {
            panic!("XX must give HostStep::Pair");
        };
        self.push(
            "host",
            t[1],
            Json::Null,
            std::slice::from_ref(&msg2),
            vec![host_frame(msg2.clone())],
        );

        assert!(matches!(
            client.read_message2(&msg2),
            Ok(ClientStep::NeedsPairRequest)
        ));
        let (msg3, client_transport) = client.write_message3(&request_bytes).expect("msg3");
        self.push(
            "cliente",
            t[2],
            typed(&request_bytes),
            std::slice::from_ref(&msg3),
            vec![client_frame(msg3.clone())],
        );
        let (read, device_key, host_transport) =
            pending.read_message3(&msg3).expect("the host reads");
        assert_eq!(read, request_bytes);
        assert_eq!(device_key, client_keys.public());
        self.client = Some(Session::new(client_transport, Some(t[2])));
        self.host = Some(Session::new(host_transport, Some(t[2])));
    }

    /// `IK`: `msg1` at `t[0]`, `msg2` at `t[1]`.
    fn start_ik(&mut self, t: [u64; 2]) {
        let id = fixed_host_id();
        let (client_keys, host_keys) = (self.keys.client_pair(), self.keys.host_pair());

        let (mut client, msg1) = ClientHandshake::connect_with_ephemeral(
            &client_keys,
            &host_keys.public(),
            id.as_str(),
            &self.keys.client_ephemeral,
        )
        .expect("the client starts");
        let start = encode_handshake_start(&HandshakeStart {
            kind: HandshakeStartKind::Connect,
            msg1: msg1.clone().into(),
        });
        self.push(
            "cliente",
            t[0],
            Json::Null,
            std::slice::from_ref(&msg1),
            vec![client_frame(start)],
        );

        let HostStep::Connect {
            msg2,
            transport: host_transport,
        } = HostHandshake::respond_with_ephemeral(
            HandshakeKind::Connect,
            &host_keys,
            id.as_str(),
            &msg1,
            &self.keys.host_ephemeral,
        )
        .expect("the host answers")
        else {
            panic!("IK must give HostStep::Connect");
        };
        assert_eq!(host_transport.remote_static(), Some(client_keys.public()));
        self.push(
            "host",
            t[1],
            Json::Null,
            std::slice::from_ref(&msg2),
            vec![host_frame(msg2.clone())],
        );
        let Ok(ClientStep::Established(client_transport)) = client.read_message2(&msg2) else {
            panic!("IK is established after msg2");
        };
        self.client = Some(Session::new(client_transport, Some(t[1])));
        self.host = Some(Session::new(host_transport, Some(t[1])));
    }

    /// The client sends `item`; the host's record layer reads it.
    fn client_says(&mut self, now_ms: u64, item: &Json) {
        let wire = encode_json(item);
        let records = self
            .client
            .as_mut()
            .expect("the handshake ran")
            .seal(&wire, now_ms)
            .expect("seals");
        read_back(self.host.as_mut().expect("open"), &records, &wire, now_ms);
        message::decode(&wire, Direction::ClientToHost).expect("the host decodes it");
        let frames = records.iter().map(|r| client_frame(r.clone())).collect();
        self.push("cliente", now_ms, typed(&wire), &records, frames);
    }

    /// The host sends `item`; the client's record layer reads it.
    fn host_says(&mut self, now_ms: u64, item: &Json) {
        let wire = encode_json(item);
        let records = self
            .host
            .as_mut()
            .expect("the handshake ran")
            .seal(&wire, now_ms)
            .expect("seals");
        read_back(self.client.as_mut().expect("open"), &records, &wire, now_ms);
        message::decode(&wire, Direction::HostToClient).expect("the client decodes it");
        let frames = records.iter().map(|r| host_frame(r.clone())).collect();
        self.push("host", now_ms, typed(&wire), &records, frames);
    }

    /// The host closes the channel: `close-channel { normal }`.
    fn host_closes(&mut self, now_ms: u64) {
        let frame = encode_hex(&encode_host(&HostEnvelope::CloseChannel {
            channel: ChannelId(CHANNEL),
            reason: CloseReason::Normal,
        }));
        self.push("host", now_ms, Json::Null, &[], vec![frame]);
    }

    fn finish(self, descricao: &str, closing: Option<&str>) -> SessionVector {
        let id = fixed_host_id();
        SessionVector {
            descricao: descricao.to_owned(),
            versao: PROTOCOL_VERSION,
            host_id: id.as_str().to_owned(),
            canal: CHANNEL,
            handshake: self.handshake.to_owned(),
            chaves: self.keys.json(),
            token_host: self.token_host.map_or(Json::Null, |t| bytes(&t)),
            pareados: self.paired.iter().map(|k| bytes(k)).collect(),
            passos: self.steps,
            resultado: match closing {
                None => SessionResult {
                    estado: "aberta".to_owned(),
                    erro: None,
                },
                Some(code) => SessionResult {
                    estado: "fechada".to_owned(),
                    erro: Some(Rejection {
                        camada: Layer::Mensagem,
                        codigo: code.to_owned(),
                    }),
                },
            },
        }
    }
}

/// The typed JSON tree of the CBOR item `wire`.
fn typed(wire: &[u8]) -> Json {
    typed_json::from_value(&cbor::decode(wire).expect("the encoder output decodes"))
}

/// The receiver reads every record and rebuilds `wire`.
fn read_back(receiver: &mut Session, records: &[Vec<u8>], wire: &[u8], now_ms: u64) {
    let last = records.len() - 1;
    for (index, record) in records.iter().enumerate() {
        let out = receiver.open(record, now_ms).expect("the record opens");
        if index == last {
            assert_eq!(
                out.as_deref(),
                Some(wire),
                "the receiver rebuilds the message"
            );
        } else {
            assert!(out.is_none());
        }
    }
}

/// After the handshake of a connection that is accepted: `hello`, the host's
/// `welcome`, `theme`, `profiles` and `workspace`, then `subscribe`, a
/// `grid-snapshot` and the `grid-ack`.
fn session_proper(c: &mut Convo, t0: u64) {
    c.client_says(t0, &hello(0, 1));
    c.host_says(t0 + 10, &welcome());
    c.host_says(t0 + 11, &theme());
    c.host_says(t0 + 12, &profiles());
    c.host_says(t0 + 13, &workspace());
    c.client_says(t0 + 100, &subscribe());
    c.host_says(t0 + 110, &grid_snapshot());
    c.client_says(t0 + 200, &grid_ack());
}

fn token(from: u8) -> [u8; 16] {
    seq(from, 16).try_into().expect("16 bytes")
}

fn other_device() -> [u8; 32] {
    StaticKeypair::from_private(seq(0xb0, 32).try_into().expect("32 bytes")).public()
}

// ---------------------------------------------------------------------------
// The conversations
// ---------------------------------------------------------------------------

fn pairing_accepted() -> SessionVector {
    let presented = token(0xc0);
    // The host holds the same token, issued at time 0.
    let mut host_token = PairingToken::new(presented, 0);
    assert!(host_token.matches_and_consume(&presented, 25));

    let mut c = Convo::new("XX", Some(presented), vec![]);
    c.start_xx([0, 12, 25], &pair_request(&presented));
    // The user confirms the fingerprint on the desktop three seconds later.
    c.host_says(3_100, &pair_accepted());
    session_proper(&mut c, 3_200);
    c.finish(
        "pareamento XX aceito, hello/welcome, theme/profiles/workspace, subscribe, grid-snapshot e grid-ack",
        None,
    )
}

fn pairing_refused_by_token() -> SessionVector {
    let presented = token(0xc0);
    let held = token(0xd0);
    let mut host_token = PairingToken::new(held, 0);
    assert!(!host_token.matches_and_consume(&presented, 25));

    let mut c = Convo::new("XX", Some(held), vec![]);
    c.start_xx([0, 12, 25], &pair_request(&presented));
    c.host_says(30, &pair_rejected(0));
    c.host_closes(31);
    c.finish(
        "pareamento XX recusado: o token do pair-request não é o do host; pair-rejected token-invalid e o canal fecha",
        Some("token-invalid"),
    )
}

fn connect_paired() -> SessionVector {
    let paired = Keys::standard().client_pair().public();
    let mut c = Convo::new("IK", None, vec![other_device(), paired]);
    c.start_ik([0, 10]);
    session_proper(&mut c, 100);
    c.finish(
        "conexão IK de um aparelho pareado: hello/welcome, theme/profiles/workspace, subscribe, grid-snapshot e grid-ack",
        None,
    )
}

fn connect_unknown() -> SessionVector {
    let mut c = Convo::new("IK", None, vec![other_device()]);
    c.start_ik([0, 10]);
    // The host completes the handshake first, then its first message is the
    // refusal (spec 02 section 4).
    c.host_says(10, &error_not_paired());
    c.host_closes(11);
    c.finish(
        "conexão IK de um aparelho que não está na lista: o handshake completa e a primeira mensagem do host é error not-paired",
        Some("not-paired"),
    )
}

fn incompatible_with(major: u64, minor: u64, descricao: &str) -> SessionVector {
    let paired = Keys::standard().client_pair().public();
    let mut c = Convo::new("IK", None, vec![paired]);
    c.start_ik([0, 10]);
    c.client_says(100, &hello(major, minor));
    c.host_says(110, &incompatible());
    c.host_closes(111);
    c.finish(descricao, Some("incompatible"))
}

/// All session vectors.
pub fn all() -> Vec<Generated> {
    vec![
        Generated::session("sessoes/pareamento-aceito.json", pairing_accepted()),
        Generated::session(
            "sessoes/pareamento-recusado-token.json",
            pairing_refused_by_token(),
        ),
        Generated::session("sessoes/conexao-ik-pareado.json", connect_paired()),
        Generated::session("sessoes/conexao-ik-desconhecido.json", connect_unknown()),
        Generated::session(
            "sessoes/incompatible-major.json",
            incompatible_with(
                1,
                0,
                "hello com major 1 para um host 0.1: incompatible com a versão do host e o canal fecha",
            ),
        ),
        Generated::session(
            "sessoes/incompatible-minor.json",
            incompatible_with(
                0,
                2,
                "hello 0.2 para um host 0.1: com major 0 o minor também tem de ser igual, incompatible",
            ),
        ),
    ]
}
