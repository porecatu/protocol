// SPDX-License-Identifier: GPL-3.0-or-later
//! Serde shapes of the test-vector files.
//!
//! Formats are defined in `docs/especificacao/vetores-de-teste/README.md`.
//! Field names are the Portuguese ones of the files; every shape rejects
//! unknown fields so a typo in a vector is an error, not a silent skip.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// The CDDL root rules a value vector may name (field `raiz`).
pub const ROOTS: [&str; 5] = [
    "host-envelope",
    "client-envelope",
    "handshake-start",
    "pair-request",
    "message",
];

/// Layer of a rejection (`erro.camada`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// Relay envelope.
    Envelope,
    /// Noise record.
    Registro,
    /// Noise handshake.
    Handshake,
    /// Application message.
    Mensagem,
    /// Pairing URI.
    Uri,
}

/// Why a vector must be rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rejection {
    /// Layer that rejects.
    pub camada: Layer,
    /// Layer-specific code, as text.
    pub codigo: String,
}

/// A value vector (envelope, message, identity-free structures).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueVector {
    /// Human description.
    pub descricao: String,
    /// Protocol version `[major, minor]`.
    pub versao: [u64; 2],
    /// CDDL root rule used to validate the item.
    pub raiz: String,
    /// Typed JSON tree of the item; `null` when the item is not even CBOR.
    pub valor: Json,
    /// The exact CBOR bytes, lowercase hex.
    pub cbor_hex: String,
    /// Bytes the typed re-encoding must produce (unknown-key case only).
    #[serde(default)]
    pub reencode_hex: Option<String>,
    /// Whether decoders must accept `cbor_hex`.
    pub valido: bool,
    /// How decoders must reject it (when `valido` is false).
    #[serde(default)]
    pub erro: Option<Rejection>,
    /// Optional CBOR diagnostic notation, for humans only.
    #[serde(default)]
    pub diag: Option<String>,
}

/// An identity vector (`identidade/`): a function and its expected output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityVector {
    /// Human description.
    pub descricao: String,
    /// `host_id`, `fingerprint`, `pairing_uri_format` or `pairing_uri_parse`.
    pub funcao: String,
    /// Typed JSON input.
    pub entrada: Json,
    /// Expected output.
    pub saida: Json,
    /// Whether the function must succeed.
    pub valido: bool,
    /// Rejection, when `valido` is false.
    #[serde(default)]
    pub erro: Option<Rejection>,
}

/// Fixed keys of a session vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionKeys {
    /// Host static private key.
    pub host_estatica: Json,
    /// Host ephemeral private key.
    pub host_efemera: Json,
    /// Client static private key.
    pub cliente_estatica: Json,
    /// Client ephemeral private key.
    pub cliente_efemera: Json,
}

/// One step of a session vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStep {
    /// `cliente` or `host`.
    pub de: String,
    /// Monotonic milliseconds given to the side that acts.
    pub now_ms: u64,
    /// Application message in the clear, or `null` in a handshake step.
    pub mensagem: Json,
    /// Noise records, lowercase hex.
    pub registros_hex: Vec<String>,
    /// Exact WebSocket frames, lowercase hex.
    pub envelope_hex: Vec<String>,
}

/// Final state of a session vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionResult {
    /// For example `aberta` or `falhou`.
    pub estado: String,
    /// Rejection, when the session fails.
    #[serde(default)]
    pub erro: Option<Rejection>,
}

/// A whole conversation, handshake to goodbye (`sessoes/`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionVector {
    /// Human description.
    pub descricao: String,
    /// Protocol version `[major, minor]` the host speaks.
    pub versao: [u64; 2],
    /// The host id of the conversation.
    pub host_id: String,
    /// The channel number the relay gave this conversation (the `channel` of
    /// the host's `channel-data` and `close-channel` frames).
    pub canal: u64,
    /// `XX` (pairing) or `IK` (connection of a paired device).
    pub handshake: String,
    /// Every fixed key.
    pub chaves: SessionKeys,
    /// The pairing token the host holds, issued at time 0 (typed `bstr`);
    /// `null` when there is none.
    pub token_host: Json,
    /// Static public keys of the devices the host has paired (typed `bstr`).
    pub pareados: Vec<Json>,
    /// The conversation.
    pub passos: Vec<SessionStep>,
    /// Final state.
    pub resultado: SessionResult,
}

/// One message of a Noise handshake (`noise/`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseMessage {
    /// `cliente` or `host`.
    pub de: String,
    /// The payload the message carries, in the clear (typed `bstr`; empty
    /// except for the `msg3` of `XX`, the `pair-request`).
    pub payload: Json,
    /// The Noise message, lowercase hex.
    pub hex: String,
}

/// A Noise handshake with the Porecatu prologue (`noise/`), fixed keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseVector {
    /// Human description.
    pub descricao: String,
    /// Protocol version `[major, minor]`.
    pub versao: [u64; 2],
    /// `XX` or `IK`.
    pub padrao: String,
    /// The Noise protocol name.
    pub nome: String,
    /// The host id of the conversation.
    pub host_id: String,
    /// The Noise prologue, lowercase hex.
    pub prologo_hex: String,
    /// Every fixed key.
    pub chaves: SessionKeys,
    /// The host static public key the client was given (the `key` of the QR
    /// code for `XX`, the stored key for `IK`), typed `bstr`.
    pub chave_do_host: Json,
    /// The handshake messages exchanged, in order.
    pub mensagens: Vec<NoiseMessage>,
    /// The handshake hash both sides end with; `null` when it fails.
    pub handshake_hash: Option<String>,
    /// The side that refuses (`cliente` or `host`); `null` when it succeeds.
    pub quem_falha: Option<String>,
    /// Whether the handshake completes.
    pub valido: bool,
    /// Why it fails.
    pub erro: Option<Rejection>,
}

/// The plaintext of a record test: bytes `i mod 251`, `tamanho` of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// Always `"i mod 251"`.
    pub padrao: String,
    /// Length in bytes.
    pub tamanho: u64,
    /// BLAKE2s-256 of the plaintext, lowercase hex.
    pub blake2s: String,
}

/// The rekey policy of a record vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RekeyPolicyJson {
    /// A key encrypts at most this many records.
    pub max_records: u64,
    /// A key lives at most this many milliseconds.
    pub max_age_ms: u64,
}

/// One message sent in a record vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordStep {
    /// The sender: `cliente` or `host`.
    pub de: String,
    /// Monotonic milliseconds given to the sender.
    pub now_ms: u64,
    /// The message the records carry.
    pub texto_claro: Pattern,
    /// The flags byte of each record (`0x01` CONTINUA, `0x02` REKEY).
    pub flags: Vec<u8>,
    /// The records, lowercase hex; `null` when the message is above 65 519
    /// bytes (see `registros_blake2s`).
    pub registros_hex: Option<Vec<String>>,
    /// BLAKE2s-256 of each record, lowercase hex.
    pub registros_blake2s: Vec<String>,
    /// The records were built below the record layer (a forged flag, a
    /// flipped bit): only the receiver is replayed.
    pub forjado: bool,
}

/// Records, fragmentation and rekey (`registros/`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordVector {
    /// Human description.
    pub descricao: String,
    /// Protocol version `[major, minor]`.
    pub versao: [u64; 2],
    /// The host id (the prologue of the `IK` handshake that keys the transport).
    pub host_id: String,
    /// Every fixed key: an `IK` handshake with them keys the transport.
    pub chaves: SessionKeys,
    /// The rekey policy of both sessions.
    pub politica: RekeyPolicyJson,
    /// The monotonic time at which the handshake ended, given to both
    /// sessions.
    pub handshake_ms: u64,
    /// The messages, in order.
    pub passos: Vec<RecordStep>,
    /// Whether every step succeeds.
    pub valido: bool,
    /// What fails, in the last step.
    pub erro: Option<Rejection>,
}

/// One Noise test vector of the cacophony format (`noise/cacophony-*.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacophonyVector {
    /// File of the snow repository it came from.
    pub source_file: String,
    /// Noise protocol name.
    pub protocol_name: String,
    /// Initiator prologue, hex.
    pub init_prologue: String,
    /// Initiator static private key, hex.
    pub init_static: String,
    /// Initiator ephemeral private key, hex.
    pub init_ephemeral: String,
    /// Initiator's copy of the responder static public key (`IK`), hex.
    #[serde(default)]
    pub init_remote_static: Option<String>,
    /// Responder prologue, hex.
    pub resp_prologue: String,
    /// Responder static private key, hex.
    pub resp_static: String,
    /// Responder ephemeral private key, hex.
    pub resp_ephemeral: String,
    /// Handshake hash, hex (some vectors do not give it).
    #[serde(default)]
    pub handshake_hash: Option<String>,
    /// Handshake messages then transport messages, initiator first.
    pub messages: Vec<CacophonyMessage>,
}

/// A message of a cacophony vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacophonyMessage {
    /// Plaintext, hex.
    pub payload: String,
    /// Ciphertext, hex.
    pub ciphertext: String,
}

/// The file of cacophony vectors (`noise/cacophony-*.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacophonyFile {
    /// Always `noise-cacophony`.
    pub formato: String,
    /// Where the vectors come from.
    pub source: String,
    /// Their licence.
    pub license: String,
    /// How `messages` are laid out.
    pub format: String,
    /// The vectors.
    pub vectors: Vec<CacophonyVector>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_vector_from_the_readme() {
        let text = r#"{
          "descricao": "grid-diff minimo",
          "versao": [0, 1],
          "raiz": "message",
          "valor": [23, {"map": [["pane", 7]]}],
          "cbor_hex": "82",
          "reencode_hex": null,
          "valido": true,
          "erro": null,
          "diag": "[23, {...}]"
        }"#;
        let v: ValueVector = serde_json::from_str(text).unwrap();
        assert_eq!(v.raiz, "message");
        assert_eq!(v.versao, [0, 1]);
        assert!(v.valido && v.erro.is_none() && v.reencode_hex.is_none());
    }

    #[test]
    fn rejected_vector_carries_layer_and_code() {
        let text = r#"{
          "descricao": "x", "versao": [0, 1], "raiz": "message", "valor": null,
          "cbor_hex": "ff", "valido": false,
          "erro": {"camada": "mensagem", "codigo": "protocol"}
        }"#;
        let v: ValueVector = serde_json::from_str(text).unwrap();
        assert_eq!(
            v.erro,
            Some(Rejection {
                camada: Layer::Mensagem,
                codigo: "protocol".to_owned()
            })
        );
    }

    #[test]
    fn unknown_fields_are_errors() {
        let text = r#"{
          "descricao": "x", "versao": [0, 1], "raiz": "message", "valor": null,
          "cbor_hex": "ff", "valido": false, "erro": null, "extra": 1
        }"#;
        assert!(serde_json::from_str::<ValueVector>(text).is_err());
    }

    #[test]
    fn identity_and_session_shapes_parse() {
        let id = r#"{ "descricao": "host_id", "funcao": "host_id",
          "entrada": {"bstr": "00"}, "saida": "k3x9", "valido": true, "erro": null }"#;
        let id: IdentityVector = serde_json::from_str(id).unwrap();
        assert_eq!(id.funcao, "host_id");

        let session = r#"{
          "descricao": "pareamento", "versao": [0, 1], "host_id": "abc",
          "canal": 1, "handshake": "XX", "token_host": null, "pareados": [],
          "chaves": {"host_estatica": {"bstr": "00"}, "host_efemera": {"bstr": "00"},
                     "cliente_estatica": {"bstr": "00"}, "cliente_efemera": {"bstr": "00"}},
          "passos": [{"de": "cliente", "now_ms": 0, "mensagem": null,
                      "registros_hex": [], "envelope_hex": ["8210"]}],
          "resultado": {"estado": "aberta", "erro": null}
        }"#;
        let s: SessionVector = serde_json::from_str(session).unwrap();
        assert_eq!(s.passos.len(), 1);
        assert_eq!(s.resultado.estado, "aberta");
    }
}
