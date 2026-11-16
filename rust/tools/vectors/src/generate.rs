// SPDX-License-Identifier: GPL-3.0-or-later
//! The `generate` subcommand: writes the vectors of `identidade/`,
//! `envelope/`, `mensagens/`, `noise/`, `registros/` and `sessoes/`.
//!
//! The vendored cacophony files (`noise/cacophony-*.json`) are not generated:
//! they are kept as they are.
//!
//! The output is deterministic: the same files, byte for byte, on every run
//! (vectors in a fixed order, JSON indented by 2 spaces, a newline at the
//! end). Before writing, every vector is checked against the crate's own
//! decoders, so the tool never commits a vector the implementation under
//! test disagrees with; a mismatch is a bug in a case or in the crate, and
//! the run stops.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use porecatu_protocol::encoding::decode_hex;
use porecatu_protocol::envelope::{
    EnvelopeError, decode_client, decode_handshake_start, decode_host, encode_client,
    encode_handshake_start, encode_host,
};
use porecatu_protocol::message::{self, Direction, PairRequest};

use crate::cases::{self, Generated, Vector};
use crate::vector::ValueVector;

/// Directories under the vectors directory this subcommand owns: their
/// `.json` files are replaced on every run.
const OWNED: [&str; 6] = [
    "identidade",
    "envelope",
    "mensagens",
    "noise",
    "registros",
    "sessoes",
];

/// Files that live in an owned directory but are not generated.
fn is_vendored(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("cacophony-"))
}

/// What a run wrote.
#[derive(Debug, Default)]
pub struct Summary {
    /// Vectors written per top-level directory, in directory order.
    pub per_category: Vec<(String, usize)>,
    /// Vectors written per `mensagens/<type>/` folder.
    pub per_message: Vec<(String, usize)>,
}

impl Summary {
    /// Total of vectors written.
    pub fn total(&self) -> usize {
        self.per_category.iter().map(|(_, n)| n).sum()
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (category, count) in &self.per_category {
            writeln!(f, "{category}: {count}")?;
        }
        write!(f, "total: {}", self.total())
    }
}

/// Generates every vector under `dir`.
pub fn run(dir: &Path) -> Result<Summary, String> {
    let mut all: Vec<Generated> = Vec::new();
    all.extend(cases::identidade::all());
    all.extend(cases::envelope::all());
    all.extend(cases::mensagens::all());
    all.extend(cases::noise::all());
    all.extend(cases::registros::all());
    all.extend(cases::sessoes::all());

    let mut seen = BTreeSet::new();
    for generated in &all {
        if !seen.insert(generated.path.clone()) {
            return Err(format!("two vectors for the path {}", generated.path));
        }
        verify(generated).map_err(|e| format!("{}: {e}", generated.path))?;
    }

    for owned in OWNED {
        clear_json(&dir.join(owned))?;
    }

    let mut summary = Summary::default();
    for owned in OWNED {
        let count = all
            .iter()
            .filter(|g| g.path.split('/').next() == Some(owned))
            .count();
        summary.per_category.push((owned.to_owned(), count));
    }
    for generated in &all {
        write(dir, generated)?;
        let mut parts = generated.path.split('/');
        if let (Some("mensagens"), Some(folder)) = (parts.next(), parts.next()) {
            match summary.per_message.last_mut() {
                Some((name, count)) if name == folder => *count += 1,
                _ => summary.per_message.push((folder.to_owned(), 1)),
            }
        }
    }
    Ok(summary)
}

fn write(dir: &Path, generated: &Generated) -> Result<(), String> {
    let path: PathBuf = generated
        .path
        .split('/')
        .fold(dir.to_path_buf(), |p, s| p.join(s));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = match &generated.vector {
        Vector::Value(v) => serde_json::to_string_pretty(v),
        Vector::Identity(v) => serde_json::to_string_pretty(v),
        Vector::Noise(v) => serde_json::to_string_pretty(v),
        Vector::Records(v) => serde_json::to_string_pretty(v),
        Vector::Session(v) => serde_json::to_string_pretty(v),
    }
    .map_err(|e| format!("{}: {e}", generated.path))?;
    text.push('\n');
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Removes every `.json` file under `dir` (and the directories left empty).
fn clear_json(dir: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            clear_json(&path)?;
            // Only succeeds when empty.
            let _ = fs::remove_dir(&path);
        } else if path.extension().is_some_and(|e| e == "json") && !is_vendored(&path) {
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Verification against the crate
// ---------------------------------------------------------------------------

fn verify(generated: &Generated) -> Result<(), String> {
    match &generated.vector {
        // Identity, Noise, record and session cases run the crate while they
        // are built and assert what it did; nothing is left to compare.
        Vector::Identity(_) | Vector::Noise(_) | Vector::Records(_) | Vector::Session(_) => Ok(()),
        Vector::Value(v) => verify_value(v),
    }
}

/// The code a relay would give for an envelope error.
fn envelope_code(error: &EnvelopeError) -> &'static str {
    match error {
        EnvelopeError::FrameTooLarge => "1009",
        _ => "4004",
    }
}

fn verify_value(v: &ValueVector) -> Result<(), String> {
    let bytes = decode_hex(&v.cbor_hex).map_err(|_| "cbor_hex is not hex".to_owned())?;
    let frame = bytes.clone();
    let expected = match &v.reencode_hex {
        Some(hex) => decode_hex(hex).map_err(|_| "reencode_hex is not hex".to_owned())?,
        None => bytes.clone(),
    };
    let wanted_code = v.erro.as_ref().map(|e| e.codigo.as_str());

    match v.raiz.as_str() {
        "host-envelope" => match decode_host(frame.into()) {
            Ok(envelope) if v.valido => same(&encode_host(&envelope), &expected),
            Err(e) if !v.valido => same_code(envelope_code(&e), wanted_code),
            other => Err(format!("unexpected result {other:?}")),
        },
        "client-envelope" => match decode_client(frame.into()) {
            Ok(envelope) if v.valido => same(&encode_client(&envelope), &expected),
            Err(e) if !v.valido => same_code(envelope_code(&e), wanted_code),
            other => Err(format!("unexpected result {other:?}")),
        },
        "handshake-start" => match decode_handshake_start(frame.into()) {
            Ok(start) if v.valido => same(&encode_handshake_start(&start), &expected),
            Err(e) if !v.valido => same_code(envelope_code(&e), wanted_code),
            other => Err(format!("unexpected result {other:?}")),
        },
        "pair-request" => match PairRequest::decode(&bytes) {
            Ok(request) if v.valido => {
                let again = request.encode().map_err(|e| e.to_string())?;
                same(&again, &expected)
            }
            Err(_) if !v.valido => same_code("protocol", wanted_code),
            other => Err(format!("unexpected result {other:?}")),
        },
        "message" => verify_message(v, &bytes, &expected),
        other => Err(format!("unknown root {other}")),
    }
}

/// Messages are decoded from both directions; what matters is that a valid
/// vector decodes from at least one and a refused one from none.
fn verify_message(v: &ValueVector, bytes: &[u8], expected: &[u8]) -> Result<(), String> {
    let decoded: Vec<_> = [Direction::HostToClient, Direction::ClientToHost]
        .into_iter()
        .filter_map(|from| message::decode(bytes, from).ok())
        .collect();
    if v.valido {
        let Some(first) = decoded.first() else {
            return Err("decodes from neither direction".to_owned());
        };
        // `goodbye` and `error` decode from both sides to the same message.
        if decoded.iter().any(|m| m != first) {
            return Err("the two directions decode to different messages".to_owned());
        }
        same(&message::encode(first), expected)
    } else if decoded.is_empty() {
        Ok(())
    } else {
        Err("the decoder accepts a vector marked invalid".to_owned())
    }
}

fn same(got: &[u8], want: &[u8]) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err("re-encoding differs from the expected bytes".to_owned())
    }
}

fn same_code(got: &str, want: Option<&str>) -> Result<(), String> {
    if Some(got) == want {
        Ok(())
    } else {
        Err(format!("error code {got} differs from {want:?}"))
    }
}
