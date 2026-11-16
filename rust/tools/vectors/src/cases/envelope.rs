// SPDX-License-Identifier: GPL-3.0-or-later
//! Envelope vectors (`envelope/`): the host and client frame families of the
//! relay (01 section 4) and the `handshake-start` payload (02 section 2).
//!
//! Valid vectors use the reference bytes shared by the Rust and TypeScript
//! tests (nonce `00..1f`, routing key `20..3f`, signature `40..7f`). Invalid
//! ones are written as raw hex because most of them are not what the
//! canonical encoder would ever produce.

use porecatu_protocol::cbor;
use porecatu_protocol::encoding::decode_hex;
use serde_json::{Value as Json, json};

use super::{Generated, bstr, bytes, rejected, seq, valid};
use crate::vector::Layer;

const HOST_ID: &str = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";
const MAX_SAFE: u64 = (1 << 53) - 1;

fn host(file: &str, descricao: &str, item: Json) -> Generated {
    Generated::value(
        format!("envelope/{file}.json"),
        valid(descricao, "host-envelope", &item),
    )
}

fn client(file: &str, descricao: &str, item: Json) -> Generated {
    Generated::value(
        format!("envelope/{file}.json"),
        valid(descricao, "client-envelope", &item),
    )
}

/// An invalid vector from raw hex (spaces allowed) and the typed JSON of the
/// item as written (`Json::Null` when it is not a CBOR item).
fn invalid(
    file: &str,
    descricao: &str,
    raiz: &str,
    hex: &str,
    valor: Json,
    code: &str,
) -> Generated {
    let hex: String = hex.split_whitespace().collect();
    let frame = decode_hex(&hex).unwrap_or_else(|_| panic!("{file}: bad hex"));
    let tree = (!valor.is_null()).then(|| super::to_value(&valor));
    Generated::value(
        format!("envelope/{file}.json"),
        rejected(
            descricao,
            raiz,
            tree.as_ref(),
            &frame,
            Layer::Envelope,
            code,
        ),
    )
}

/// All envelope vectors.
// A long, flat catalogue of cases: one push per vector reads better than a giant `vec![]`.
#[allow(clippy::vec_init_then_push)]
pub fn all() -> Vec<Generated> {
    let mut out = Vec::new();

    // ---- host family, valid ---------------------------------------------
    out.push(host(
        "host-challenge",
        "challenge: o relay manda o nonce de 32 bytes ao host",
        json!([0, bytes(&seq(0, 32))]),
    ));
    out.push(host(
        "host-register",
        "register: chave de roteamento e assinatura de 64 bytes",
        json!([1, bytes(&seq(0x20, 32)), bytes(&seq(0x40, 64))]),
    ));
    out.push(host(
        "host-registered",
        "registered: o host_id derivado da chave de roteamento",
        json!([2, HOST_ID]),
    ));
    out.push(host(
        "host-channel-opened",
        "channel-opened: canal 5 (cabeça de um byte)",
        json!([3, 5]),
    ));
    out.push(host(
        "host-channel-opened-1000",
        "channel-opened: canal 1000 (inteiro de dois bytes)",
        json!([3, 1000]),
    ));
    out.push(host(
        "host-channel-data",
        "channel-data: canal 7 e um registro de três bytes",
        json!([4, 7, bytes(&[1, 2, 3])]),
    ));
    out.push(host(
        "host-channel-data-canal-maximo",
        "channel-data: canal 2^53-1 (o maior inteiro do fio) e registro vazio",
        json!([4, MAX_SAFE, bstr("")]),
    ));
    out.push(host(
        "host-channel-closed",
        "channel-closed: canal 7, o host-replaced",
        json!([5, 7, 1]),
    ));
    out.push(host(
        "host-close-channel",
        "close-channel: canal 7, host-rejected",
        json!([6, 7, 3]),
    ));

    // ---- client family, valid -------------------------------------------
    out.push(client(
        "cliente-data",
        "data: um registro de dois bytes",
        json!([16, bytes(&[0xaa, 0xbb])]),
    ));
    out.push(client(
        "cliente-data-vazio",
        "data: registro vazio",
        json!([16, bstr("")]),
    ));
    out.push(client(
        "cliente-host-unavailable",
        "host-unavailable: sem corpo",
        json!([17]),
    ));
    for (code, name) in [
        (0, "normal"),
        (1, "host-replaced"),
        (2, "host-gone"),
        (3, "host-rejected"),
        (4, "limit-exceeded"),
        (5, "protocol-error"),
    ] {
        out.push(client(
            &format!("cliente-closed-{name}"),
            &format!("closed: close-reason {name} ({code})"),
            json!([18, code]),
        ));
    }

    // ---- handshake-start ------------------------------------------------
    for (kind, name) in [(0, "pair"), (1, "connect")] {
        out.push(Generated::value(
            format!("envelope/handshake-start-{name}.json"),
            valid(
                &format!("handshake-start {name}: a mensagem 1 do Noise (32 bytes) depois do tipo"),
                "handshake-start",
                &json!([kind, bytes(&seq(0, 32))]),
            ),
        ));
    }

    // ---- invalid: wrong arity -------------------------------------------
    out.push(invalid(
        "host-aridade-channel-opened-com-tres",
        "channel-opened com um terceiro elemento",
        "host-envelope",
        "83 03 07 00",
        json!([3, 7, 0]),
        "4004",
    ));
    out.push(invalid(
        "host-aridade-channel-data-sem-registro",
        "channel-data sem o registro",
        "host-envelope",
        "82 04 07",
        json!([4, 7]),
        "4004",
    ));
    out.push(invalid(
        "cliente-aridade-data-com-tres",
        "data com um terceiro elemento",
        "client-envelope",
        "83 10 40 00",
        json!([16, bstr(""), 0]),
        "4004",
    ));
    out.push(invalid(
        "cliente-aridade-host-unavailable-com-dois",
        "host-unavailable com um segundo elemento",
        "client-envelope",
        "82 11 00",
        json!([17, 0]),
        "4004",
    ));

    // ---- invalid: unknown type ------------------------------------------
    out.push(invalid(
        "host-tipo-desconhecido",
        "tipo 7 não existe na família do host",
        "host-envelope",
        "82 07 00",
        json!([7, 0]),
        "4004",
    ));
    out.push(invalid(
        "host-tipo-do-cliente",
        "host-unavailable (17) é da família do cliente, não do host",
        "host-envelope",
        "81 11",
        json!([17]),
        "4004",
    ));
    out.push(invalid(
        "cliente-tipo-desconhecido",
        "tipo 19 não existe na família do cliente",
        "client-envelope",
        "82 13 00",
        json!([19, 0]),
        "4004",
    ));
    out.push(invalid(
        "cliente-tipo-do-host",
        "channel-opened (3) é da família do host, não do cliente",
        "client-envelope",
        "82 03 07",
        json!([3, 7]),
        "4004",
    ));

    // ---- invalid: close-reason outside the domain -----------------------
    out.push(invalid(
        "host-close-reason-fora-do-dominio",
        "channel-closed com close-reason 6 (o domínio é 0..5)",
        "host-envelope",
        "83 05 07 06",
        json!([5, 7, 6]),
        "4004",
    ));
    out.push(invalid(
        "cliente-closed-reason-fora-do-dominio",
        "closed com close-reason 6 (o domínio é 0..5)",
        "client-envelope",
        "82 12 06",
        json!([18, 6]),
        "4004",
    ));

    // ---- invalid: trailing bytes ----------------------------------------
    out.push(invalid(
        "host-bytes-sobrando",
        "channel-opened seguido de um byte a mais no quadro",
        "host-envelope",
        "82 03 07 00",
        json!([3, 7]),
        "4004",
    ));
    out.push(invalid(
        "cliente-bytes-sobrando",
        "host-unavailable seguido de um byte a mais no quadro",
        "client-envelope",
        "81 11 00",
        json!([17]),
        "4004",
    ));

    // ---- invalid: not canonical -----------------------------------------
    out.push(invalid(
        "host-inteiro-nao-minimo",
        "canal 7 escrito com um byte de argumento (18 07) em vez de 07",
        "host-envelope",
        "82 03 18 07",
        json!([3, 7]),
        "4004",
    ));
    out.push(invalid(
        "host-comprimento-nao-minimo",
        "bstr de 1 byte com o comprimento em forma longa (58 01)",
        "host-envelope",
        "83 04 07 58 01 aa",
        json!([4, 7, bstr("aa")]),
        "4004",
    ));
    out.push(invalid(
        "host-cabeca-de-array-nao-minima",
        "array de 2 elementos com a cabeça em forma longa (98 02)",
        "host-envelope",
        "98 02 03 07",
        json!([3, 7]),
        "4004",
    ));

    // ---- invalid: sizes and values --------------------------------------
    let short_nonce = format!("82 00 58 1f {}", super::hex(&seq(0, 31)));
    out.push(invalid(
        "host-challenge-nonce-curto",
        "challenge com nonce de 31 bytes em vez de 32",
        "host-envelope",
        &short_nonce,
        json!([0, bytes(&seq(0, 31))]),
        "4004",
    ));
    let upper_id = format!("82 02 78 20 {}", "41".repeat(32));
    out.push(invalid(
        "host-registered-host-id-invalido",
        "registered com 32 letras maiúsculas, fora do alfabeto do host_id",
        "host-envelope",
        &upper_id,
        json!([2, "A".repeat(32)]),
        "4004",
    ));

    // A channel-data of exactly 70 000 bytes (83 04 00 5a + 4-byte length +
    // 69 992 payload bytes) plus one byte: the size is checked before the
    // frame is parsed, so `valor` is null (the extra byte also makes it
    // more than one CBOR item).
    let mut big = vec![0x83, 0x04, 0x00, 0x5a, 0x00, 0x01, 0x11, 0x68];
    big.resize(70_000, 0);
    big.push(0);
    assert_eq!(big.len(), 70_001);
    assert!(cbor::decode(&big).is_err());
    out.push(Generated::value(
        "envelope/quadro-maior-que-o-maximo.json",
        rejected(
            "quadro de 70 001 bytes: um channel-data de 70 000 bytes (o máximo) mais um byte",
            "host-envelope",
            None,
            &big,
            Layer::Envelope,
            "1009",
        ),
    ));
    out
}
