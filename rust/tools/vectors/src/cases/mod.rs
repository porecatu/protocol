// SPDX-License-Identifier: GPL-3.0-or-later
//! The value vectors the `generate` subcommand writes, one module per
//! category, plus the helpers they share.
//!
//! Every case is written by hand as typed JSON (see `typed_json`); the CBOR
//! bytes come from the crate's generic deterministic encoder, never from the
//! message codecs, so a vector is an independent statement of what the
//! specification says. `generate` then decodes every valid vector with the
//! codec under test and refuses to write a vector the codec disagrees with.

use porecatu_protocol::cbor::{self, Value};
use porecatu_protocol::encoding::encode_hex;
use serde_json::{Value as Json, json};

use crate::typed_json;
use crate::vector::{
    IdentityVector, Layer, NoiseVector, RecordVector, Rejection, SessionVector, ValueVector,
};

pub mod duo;
pub mod envelope;
pub mod identidade;
pub mod mensagens;
pub mod noise;
pub mod registros;
pub mod sessoes;

/// Protocol version `[major, minor]` the vectors are written for.
pub const PROTOCOL_VERSION: [u64; 2] = [0, 1];

/// A vector ready to be written.
#[derive(Debug, Clone)]
pub enum Vector {
    /// Envelope or message vector.
    Value(ValueVector),
    /// Identity vector.
    Identity(IdentityVector),
    /// Noise handshake vector with the Porecatu prologue.
    Noise(NoiseVector),
    /// Records, fragmentation and rekey.
    Records(RecordVector),
    /// A whole conversation.
    Session(SessionVector),
}

/// A vector and the file it goes to, relative to the vectors directory
/// (always with `/` separators).
#[derive(Debug, Clone)]
pub struct Generated {
    /// Relative path, for example `mensagens/hello/minimo.json`.
    pub path: String,
    /// The vector.
    pub vector: Vector,
}

impl Generated {
    /// A value vector at `path`.
    pub fn value(path: impl Into<String>, vector: ValueVector) -> Self {
        Generated {
            path: path.into(),
            vector: Vector::Value(vector),
        }
    }

    /// An identity vector at `path`.
    pub fn identity(path: impl Into<String>, vector: IdentityVector) -> Self {
        Generated {
            path: path.into(),
            vector: Vector::Identity(vector),
        }
    }

    /// A Noise handshake vector at `path`.
    pub fn noise(path: impl Into<String>, vector: NoiseVector) -> Self {
        Generated {
            path: path.into(),
            vector: Vector::Noise(vector),
        }
    }

    /// A record vector at `path`.
    pub fn records(path: impl Into<String>, vector: RecordVector) -> Self {
        Generated {
            path: path.into(),
            vector: Vector::Records(vector),
        }
    }

    /// A session vector at `path`.
    pub fn session(path: impl Into<String>, vector: SessionVector) -> Self {
        Generated {
            path: path.into(),
            vector: Vector::Session(vector),
        }
    }
}

// ---------------------------------------------------------------------------
// Typed JSON builders
// ---------------------------------------------------------------------------

/// `{"bstr": "<hex>"}`.
pub fn bstr(hex: &str) -> Json {
    json!({ "bstr": hex })
}

/// `{"bstr": ...}` of a byte slice.
pub fn bytes(data: &[u8]) -> Json {
    bstr(&encode_hex(data))
}

/// `{"map": [[key, value], ...]}`; the order given is the order written (the
/// canonical encoder reorders it when the bytes are produced).
pub fn map(pairs: Vec<(&str, Json)>) -> Json {
    let list: Vec<Json> = pairs.into_iter().map(|(k, v)| json!([k, v])).collect();
    json!({ "map": list })
}

/// `n` bytes counting up from `from`.
pub fn seq(from: u8, n: usize) -> Vec<u8> {
    (0..n).map(|i| from.wrapping_add(i as u8)).collect()
}

// ---------------------------------------------------------------------------
// Vector builders
// ---------------------------------------------------------------------------

/// The text form of hex bytes.
pub fn hex(bytes: &[u8]) -> String {
    encode_hex(bytes)
}

/// The typed JSON converted to a CBOR value; a panic here is a bug in a case.
pub fn to_value(json: &Json) -> Value {
    typed_json::to_value(json).unwrap_or_else(|e| panic!("bad typed JSON {json}: {e}"))
}

/// Canonical bytes of a typed JSON tree.
pub fn encode_json(json: &Json) -> Vec<u8> {
    cbor::encode(&to_value(json))
}

/// A valid value vector built from a typed JSON tree: the bytes are its
/// canonical encoding and `valor` is the tree as the decoder reads it back
/// (maps in canonical order).
pub fn valid(descricao: &str, raiz: &str, json: &Json) -> ValueVector {
    let bytes = encode_json(json);
    let tree = cbor::decode(&bytes).expect("the encoder output decodes");
    ValueVector {
        descricao: descricao.to_owned(),
        versao: PROTOCOL_VERSION,
        raiz: raiz.to_owned(),
        valor: typed_json::from_value(&tree),
        cbor_hex: hex(&bytes),
        reencode_hex: None,
        valido: true,
        erro: None,
        diag: Some(diag(&tree)),
    }
}

/// A value vector that decoders must refuse. `tree` is the item as written
/// (`None` when the bytes are not even a CBOR item).
pub fn rejected(
    descricao: &str,
    raiz: &str,
    tree: Option<&Value>,
    bytes: &[u8],
    camada: Layer,
    codigo: &str,
) -> ValueVector {
    ValueVector {
        descricao: descricao.to_owned(),
        versao: PROTOCOL_VERSION,
        raiz: raiz.to_owned(),
        valor: tree.map_or(Json::Null, typed_json::from_value),
        cbor_hex: hex(bytes),
        reencode_hex: None,
        valido: false,
        erro: Some(Rejection {
            camada,
            codigo: codigo.to_owned(),
        }),
        diag: tree.map(diag),
    }
}

/// CBOR diagnostic notation (RFC 8949 §8) of a value, for human readers.
pub fn diag(value: &Value) -> String {
    match value {
        Value::Int(i) => i.to_string(),
        Value::Bytes(b) => format!("h'{}'", encode_hex(b)),
        Value::Text(s) => Json::String(s.clone()).to_string(),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(diag).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Map(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}: {}", Json::String(k.clone()), diag(v)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_owned(),
    }
}

/// An identity vector.
pub fn identity(
    descricao: &str,
    funcao: &str,
    entrada: Json,
    saida: Json,
    erro: Option<(&str, &str)>,
) -> IdentityVector {
    IdentityVector {
        descricao: descricao.to_owned(),
        funcao: funcao.to_owned(),
        entrada,
        saida,
        valido: erro.is_none(),
        erro: erro.map(|(camada, codigo)| Rejection {
            camada: match camada {
                "uri" => Layer::Uri,
                other => panic!("unexpected layer {other}"),
            },
            codigo: codigo.to_owned(),
        }),
    }
}
