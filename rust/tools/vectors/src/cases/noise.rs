// SPDX-License-Identifier: GPL-3.0-or-later
//! Noise handshake vectors with the Porecatu prologue (`noise/`).
//!
//! `XX` and `IK` with every key and ephemeral fixed: each handshake message in
//! hex, and the handshake hash both sides end with. Three refusals: the host
//! key of the QR code differs from the one the host reveals
//! (`host-key-mismatch`), and a null DH result on each side (`dh-zero`).
//!
//! The cacophony vectors (`noise/cacophony-xx-ik.json`) are vendored as they
//! come; they are not generated here (see `generate`).

use porecatu_protocol::PROTOCOL_VERSION;
use porecatu_protocol::encoding::encode_hex;
use porecatu_protocol::session::{
    ClientHandshake, ClientStep, HandshakeError, HandshakeKind, HostHandshake, HostStep, NOISE_IK,
    NOISE_XX, StaticKeypair, prologue,
};
use serde_json::json;

use super::duo::{Keys, fixed_host_id};
use super::{Generated, bytes, encode_json, map, seq};
use crate::vector::{Layer, NoiseMessage, NoiseVector, Rejection};

/// The `pair-request` the `msg3` of `XX` carries.
fn pair_request() -> Vec<u8> {
    encode_json(&map(vec![
        ("token", bytes(&seq(0xc0, 16))),
        ("device", json!("Pixel 8")),
        ("platform", json!(0)),
    ]))
}

fn message(from: &str, payload: &[u8], wire: &[u8]) -> NoiseMessage {
    NoiseMessage {
        de: from.to_owned(),
        payload: bytes(payload),
        hex: encode_hex(wire),
    }
}

/// A vector with the fields every case shares filled in.
fn base(descricao: &str, padrao: &str, keys: &Keys, host_key: &[u8; 32]) -> NoiseVector {
    let id = fixed_host_id();
    NoiseVector {
        descricao: descricao.to_owned(),
        versao: PROTOCOL_VERSION,
        padrao: padrao.to_owned(),
        nome: match padrao {
            "XX" => NOISE_XX,
            _ => NOISE_IK,
        }
        .to_owned(),
        host_id: id.as_str().to_owned(),
        prologo_hex: encode_hex(&prologue(id.as_str())),
        chaves: keys.json(),
        chave_do_host: bytes(host_key),
        mensagens: Vec::new(),
        handshake_hash: None,
        quem_falha: None,
        valido: true,
        erro: None,
    }
}

fn refused(mut vector: NoiseVector, who: &str, code: &str) -> NoiseVector {
    vector.quem_falha = Some(who.to_owned());
    vector.valido = false;
    vector.erro = Some(Rejection {
        camada: Layer::Handshake,
        codigo: code.to_owned(),
    });
    vector
}

fn xx() -> NoiseVector {
    let keys = Keys::standard();
    let id = fixed_host_id();
    let (client_keys, host_keys) = (keys.client_pair(), keys.host_pair());
    let payload = pair_request();

    let (mut client, msg1) = ClientHandshake::pair_with_ephemeral(
        &client_keys,
        &host_keys.public(),
        id.as_str(),
        &keys.client_ephemeral,
    )
    .expect("the client starts");
    let HostStep::Pair { msg2, pending } = HostHandshake::respond_with_ephemeral(
        HandshakeKind::Pair,
        &host_keys,
        id.as_str(),
        &msg1,
        &keys.host_ephemeral,
    )
    .expect("the host answers") else {
        panic!("XX must give HostStep::Pair");
    };
    assert!(matches!(
        client.read_message2(&msg2),
        Ok(ClientStep::NeedsPairRequest)
    ));
    let (msg3, client_transport) = client.write_message3(&payload).expect("msg3");
    let (read, device_key, host_transport) = pending.read_message3(&msg3).expect("the host reads");
    assert_eq!(read, payload);
    assert_eq!(device_key, client_keys.public());
    assert_eq!(
        client_transport.handshake_hash(),
        host_transport.handshake_hash()
    );

    let mut vector = base(
        "XX com o prólogo do Porecatu, chaves e efêmeros fixos; a msg3 leva o pair-request",
        "XX",
        &keys,
        &host_keys.public(),
    );
    vector.mensagens = vec![
        message("cliente", &[], &msg1),
        message("host", &[], &msg2),
        message("cliente", &payload, &msg3),
    ];
    vector.handshake_hash = Some(encode_hex(client_transport.handshake_hash()));
    vector
}

fn ik() -> NoiseVector {
    let keys = Keys::standard();
    let id = fixed_host_id();
    let (client_keys, host_keys) = (keys.client_pair(), keys.host_pair());

    let (mut client, msg1) = ClientHandshake::connect_with_ephemeral(
        &client_keys,
        &host_keys.public(),
        id.as_str(),
        &keys.client_ephemeral,
    )
    .expect("the client starts");
    let HostStep::Connect {
        msg2,
        transport: host_transport,
    } = HostHandshake::respond_with_ephemeral(
        HandshakeKind::Connect,
        &host_keys,
        id.as_str(),
        &msg1,
        &keys.host_ephemeral,
    )
    .expect("the host answers")
    else {
        panic!("IK must give HostStep::Connect");
    };
    let Ok(ClientStep::Established(client_transport)) = client.read_message2(&msg2) else {
        panic!("IK is established after msg2");
    };
    assert_eq!(host_transport.remote_static(), Some(client_keys.public()));
    assert_eq!(
        client_transport.handshake_hash(),
        host_transport.handshake_hash()
    );

    let mut vector = base(
        "IK com o prólogo do Porecatu, chaves e efêmeros fixos; payloads vazios",
        "IK",
        &keys,
        &host_keys.public(),
    );
    vector.mensagens = vec![message("cliente", &[], &msg1), message("host", &[], &msg2)];
    vector.handshake_hash = Some(encode_hex(client_transport.handshake_hash()));
    vector
}

/// The QR code carries another host key: the client aborts after `msg2`.
fn host_key_mismatch() -> NoiseVector {
    let keys = Keys::standard();
    let id = fixed_host_id();
    let host_keys = keys.host_pair();
    let qr_key = StaticKeypair::from_private(seq(0xb0, 32).try_into().expect("32 bytes")).public();
    assert_ne!(qr_key, host_keys.public());

    let (mut client, msg1) = ClientHandshake::pair_with_ephemeral(
        &keys.client_pair(),
        &qr_key,
        id.as_str(),
        &keys.client_ephemeral,
    )
    .expect("the client starts");
    let HostStep::Pair { msg2, .. } = HostHandshake::respond_with_ephemeral(
        HandshakeKind::Pair,
        &host_keys,
        id.as_str(),
        &msg1,
        &keys.host_ephemeral,
    )
    .expect("the host answers") else {
        panic!("XX must give HostStep::Pair");
    };
    assert_eq!(
        client.read_message2(&msg2).err(),
        Some(HandshakeError::HostKeyMismatch)
    );

    let mut vector = base(
        "XX em que a chave do QR difere da que o host revela na msg2: o cliente aborta sem msg3",
        "XX",
        &keys,
        &qr_key,
    );
    vector.mensagens = vec![message("cliente", &[], &msg1), message("host", &[], &msg2)];
    refused(vector, "cliente", "host-key-mismatch")
}

/// An `XX` `msg1` whose ephemeral is the zero point: the host's `ee` is a null
/// DH result.
fn dh_zero_host() -> NoiseVector {
    let keys = Keys::standard();
    let id = fixed_host_id();
    let host_keys = keys.host_pair();
    let msg1 = [0u8; 32];
    assert_eq!(
        HostHandshake::respond_with_ephemeral(
            HandshakeKind::Pair,
            &host_keys,
            id.as_str(),
            &msg1,
            &keys.host_ephemeral,
        )
        .err(),
        Some(HandshakeError::DhZero)
    );

    let mut vector = base(
        "XX cuja msg1 traz a efêmera nula (ponto de ordem baixa): o ee do host dá DH nulo e ele recusa",
        "XX",
        &keys,
        &host_keys.public(),
    );
    vector.mensagens = vec![message("cliente", &[], &msg1)];
    refused(vector, "host", "dh-zero")
}

/// An `IK` client given a zero host key: its `es` is a null DH result.
fn dh_zero_client() -> NoiseVector {
    let keys = Keys::standard();
    let id = fixed_host_id();
    let zero = [0u8; 32];
    assert_eq!(
        ClientHandshake::connect_with_ephemeral(
            &keys.client_pair(),
            &zero,
            id.as_str(),
            &keys.client_ephemeral,
        )
        .err(),
        Some(HandshakeError::DhZero)
    );

    let vector = base(
        "IK em que a chave do host é o ponto nulo: o es do cliente dá DH nulo e ele recusa antes de escrever a msg1",
        "IK",
        &keys,
        &zero,
    );
    refused(vector, "cliente", "dh-zero")
}

/// All Noise vectors this tool generates.
pub fn all() -> Vec<Generated> {
    vec![
        Generated::noise("noise/xx-porecatu.json", xx()),
        Generated::noise("noise/ik-porecatu.json", ik()),
        Generated::noise("noise/xx-chave-do-qr-diferente.json", host_key_mismatch()),
        Generated::noise("noise/xx-dh-nulo-no-host.json", dh_zero_host()),
        Generated::noise("noise/ik-dh-nulo-no-cliente.json", dh_zero_client()),
    ]
}
