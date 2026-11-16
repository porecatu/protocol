// SPDX-License-Identifier: GPL-3.0-or-later
//! Identity vectors (`identidade/`): `host_id`, `fingerprint` and the pairing
//! URI, with fixed keys.
//!
//! Key A is the bytes `00..1f`, key B is 32 times `ff`. The expected strings
//! below were computed independently with Python (`hashlib.blake2s` and
//! `base64.b32encode`); `generate` checks them against the crate before
//! writing anything.

use porecatu_protocol::encoding::encode_base64url;
use porecatu_protocol::fingerprint::fingerprint;
use porecatu_protocol::host_id::host_id;
use porecatu_protocol::pairing_uri::PairingUri;
use serde_json::{Value as Json, json};

use super::{Generated, bytes, identity, map, seq};

const HOST_ID_A: &str = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";
const HOST_ID_B: &str = "mhj67icrscnsvbofgekm46omai6nhln7";
const FINGERPRINT_A: &str = "AWBFMB6X7XZNQLXU";
const FINGERPRINT_B: &str = "MHJ67ICRSCNSVBOF";

const KEY_B_B64: &str = "__________________________________________8";
const TOKEN_B64: &str = "AAECAwQFBgcICQoLDA0ODw";

/// The URI of the fixed case, exactly as the specification writes it (02
/// section 3.1): parameter order, uppercase hex, normalized relay.
const FULL_URI: &str = concat!(
    "porecatu://pair?v=0",
    "&relay=wss%3A%2F%2Frelay.exemplo.com.br%3A8443%2Fporecatu",
    "&host=awbfmb6x7xznqlxuypemflvjmgwzrvqo",
    "&key=__________________________________________8",
    "&token=AAECAwQFBgcICQoLDA0ODw",
    "&name=Mesa%20do%20L%C3%A9o%20%28casa%29",
);

fn key_a() -> [u8; 32] {
    core::array::from_fn(|i| i as u8)
}

fn key_b() -> [u8; 32] {
    [0xff; 32]
}

/// Typed JSON of the URI contents, in the order of the URI parameters.
fn uri_fields(relay: &str, host: &str, name: &str) -> Json {
    map(vec![
        ("v", json!(0)),
        ("relay", json!(relay)),
        ("host", json!(host)),
        ("key", bytes(&key_b())),
        ("token", bytes(&seq(0, 16))),
        ("name", json!(name)),
    ])
}

/// A valid URI with one parameter replaced (`Some`) or removed (`None`).
fn uri_with(field: &str, value: Option<&str>) -> String {
    let defaults = [
        ("v", "0".to_owned()),
        ("relay", "wss%3A%2F%2Frelay.example.com".to_owned()),
        ("host", HOST_ID_A.to_owned()),
        ("key", KEY_B_B64.to_owned()),
        ("token", TOKEN_B64.to_owned()),
        ("name", "Desk".to_owned()),
    ];
    let params: Vec<String> = defaults
        .iter()
        .filter_map(|(name, default)| match (*name == field, value) {
            (true, None) => None,
            (true, Some(v)) => Some(format!("{name}={v}")),
            (false, _) => Some(format!("{name}={default}")),
        })
        .collect();
    format!("porecatu://pair?{}", params.join("&"))
}

/// All identity vectors.
pub fn all() -> Vec<Generated> {
    let mut out = Vec::new();

    // host_id and fingerprint.
    for (file, label, key, id, fp) in [
        ("a", "00..1f", key_a(), HOST_ID_A, FINGERPRINT_A),
        ("b", "32 bytes ff", key_b(), HOST_ID_B, FINGERPRINT_B),
    ] {
        assert_eq!(host_id(&key).as_str(), id, "host_id of key {file}");
        assert_eq!(fingerprint(&key).as_str(), fp, "fingerprint of key {file}");
        out.push(Generated::identity(
            format!("identidade/host-id-chave-{file}.json"),
            identity(
                &format!("host_id da chave de roteamento {label}"),
                "host_id",
                bytes(&key),
                json!(id),
                None,
            ),
        ));
        let grouped: Vec<&str> = (0..4).map(|i| &fp[i * 4..i * 4 + 4]).collect();
        out.push(Generated::identity(
            format!("identidade/fingerprint-chave-{file}.json"),
            identity(
                &format!(
                    "impressão digital da chave estática {label} (16 caracteres; exibição \"{}\")",
                    grouped.join(" ")
                ),
                "fingerprint",
                bytes(&key),
                json!(fp),
                None,
            ),
        ));
    }

    // pairing_uri_format: relay not yet normalized, name outside the
    // unreserved set (accent, space, parentheses).
    let uri = PairingUri {
        major: 0,
        relay: "wss://Relay.Exemplo.com.br:8443/porecatu/".to_owned(),
        host_id: host_id(&key_a()),
        host_key: key_b(),
        token: core::array::from_fn(|i| i as u8),
        name: "Mesa do Léo (casa)".to_owned(),
    };
    assert_eq!(uri.to_uri(), FULL_URI);
    out.push(Generated::identity(
        "identidade/uri-formato-completo.json",
        identity(
            "URI de pareamento: relay normalizado na escrita, nome com acento, espaço e parênteses em %HH maiúsculo",
            "pairing_uri_format",
            uri_fields(
                "wss://Relay.Exemplo.com.br:8443/porecatu/",
                HOST_ID_A,
                "Mesa do Léo (casa)",
            ),
            json!(FULL_URI),
            None,
        ),
    ));

    // pairing_uri_parse, valid.
    let normalized = "wss://relay.exemplo.com.br:8443/porecatu";
    out.push(parse_ok(
        "uri-leitura-completa",
        "leitura da URI completa: o relay volta normalizado, a chave e o token como bytes",
        FULL_URI,
        uri_fields(normalized, HOST_ID_A, "Mesa do Léo (casa)"),
    ));
    let unknown = format!("{FULL_URI}&future=%ZZ&other").replacen("?v=0", "?extra=1&v=0", 1);
    out.push(parse_ok(
        "uri-parametro-desconhecido",
        "parâmetros desconhecidos, mesmo mal formados, são ignorados (versão menor futura)",
        &unknown,
        uri_fields(normalized, HOST_ID_A, "Mesa do Léo (casa)"),
    ));
    let local = format!(
        "porecatu://pair?v=0&relay=ws%3A%2F%2F%5B%3A%3A1%5D%3A8080&host={HOST_ID_A}&key={KEY_B_B64}&token={TOKEN_B64}&name=Desk"
    );
    out.push(parse_ok(
        "uri-relay-local",
        "ws:// é aceito para um endereço local (aqui [::1])",
        &local,
        map(vec![
            ("v", json!(0)),
            ("relay", json!("ws://[::1]:8080")),
            ("host", json!(HOST_ID_A)),
            ("key", bytes(&key_b())),
            ("token", bytes(&seq(0, 16))),
            ("name", json!("Desk")),
        ]),
    ));

    // pairing_uri_parse, invalid.
    let key31 = encode_base64url(&[1u8; 31]);
    let invalid: Vec<(&str, &str, String, &str)> = vec![
        (
            "uri-nao-porecatu",
            "outro caminho do esquema porecatu",
            "porecatu://other?v=0".to_owned(),
            "not-porecatu",
        ),
        (
            "uri-versao-diferente",
            "v diferente da versão maior do leitor",
            uri_with("v", Some("1")),
            "version-mismatch",
        ),
        (
            "uri-campo-ausente",
            "sem o parâmetro token",
            uri_with("token", None),
            "missing-field",
        ),
        (
            "uri-campo-repetido",
            "o parâmetro name aparece duas vezes",
            format!("{}&name=Other", uri_with("name", Some("Desk"))),
            "duplicate-field",
        ),
        (
            "uri-campo-mal-formado",
            "key com 31 bytes decodificados em vez de 32",
            uri_with("key", Some(&key31)),
            "malformed-field",
        ),
        (
            "uri-relay-inseguro",
            "ws:// para um host que não é local",
            uri_with("relay", Some("ws%3A%2F%2Frelay.example.com")),
            "insecure-relay",
        ),
    ];
    for (file, descricao, input, code) in invalid {
        let err = PairingUri::parse(&input).expect_err(file);
        assert_eq!(err.code(), code, "{file}");
        out.push(Generated::identity(
            format!("identidade/{file}.json"),
            identity(
                descricao,
                "pairing_uri_parse",
                json!(input),
                Json::Null,
                Some(("uri", code)),
            ),
        ));
    }
    out
}

fn parse_ok(file: &str, descricao: &str, input: &str, saida: Json) -> Generated {
    let parsed = PairingUri::parse(input).unwrap_or_else(|e| panic!("{file}: {e}"));
    let got = map(vec![
        ("v", json!(parsed.major)),
        ("relay", json!(parsed.relay)),
        ("host", json!(parsed.host_id.as_str())),
        ("key", bytes(&parsed.host_key)),
        ("token", bytes(&parsed.token)),
        ("name", json!(parsed.name)),
    ]);
    assert_eq!(got, saida, "{file}");
    Generated::identity(
        format!("identidade/{file}.json"),
        identity(descricao, "pairing_uri_parse", json!(input), saida, None),
    )
}
