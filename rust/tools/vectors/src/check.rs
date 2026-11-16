// SPDX-License-Identifier: GPL-3.0-or-later
//! The `check` subcommand: validates the vector files against the Rust
//! implementation's CBOR layer and against the CDDL schema.
//!
//! Checks per value vector (`envelope/`, `mensagens/`):
//!
//! 1. `cbor_hex` x `valor` through the typed-JSON conversion and the crate's
//!    strict `cbor::decode`/`cbor::encode`;
//! 2. validity of `cbor_hex` against the CDDL schema at `raiz` (valid vectors
//!    must pass; invalid ones are not required to fail at the CDDL level,
//!    because most are rejected by the strict decoder or by typed decoding);
//! 3. `reencode_hex`, when present, is canonical CBOR and conforms to the
//!    schema too.
//!
//! Identity vectors are only checked for shape here. Noise, record and session
//! vectors are checked for internal consistency (hex, lengths, hashes, and the
//! frames against the crate's envelope decoders); replaying them against the
//! crate's handshake and record layers is the job of `rust/tests/vectors.rs`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use porecatu_protocol::cbor::{self, Value};
use porecatu_protocol::encoding::decode_hex;

use crate::check_formats::{check_cacophony, check_noise, check_records, check_session};
use crate::schema::Schema;
use crate::typed_json::{self, TypedJsonError};
use crate::vector::{
    CacophonyFile, IdentityVector, NoiseVector, ROOTS, RecordVector, SessionVector, ValueVector,
};

/// Outcome of a whole run.
#[derive(Debug, Default)]
pub struct Report {
    /// Value vectors fully checked.
    pub value_vectors: usize,
    /// Identity vectors checked for shape.
    pub identity_vectors: usize,
    /// Session vectors checked.
    pub session_vectors: usize,
    /// Noise vectors checked (the cacophony file counts its vectors).
    pub noise_vectors: usize,
    /// Record vectors checked.
    pub record_vectors: usize,
    /// Files of a format this tool does not check.
    pub skipped: Vec<PathBuf>,
    /// One entry per problem found: `(file, what)`.
    pub failures: Vec<(PathBuf, String)>,
}

impl Report {
    /// Total of vectors checked.
    pub fn total(&self) -> usize {
        self.value_vectors
            + self.identity_vectors
            + self.session_vectors
            + self.noise_vectors
            + self.record_vectors
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (file, what) in &self.failures {
            writeln!(f, "FAIL {}: {what}", file.display())?;
        }
        for file in &self.skipped {
            writeln!(f, "skip {} (format not checked here)", file.display())?;
        }
        write!(
            f,
            "{} vetores ({} de valor, {} de identidade, {} de Noise, {} de registros, {} de sessão), {} falha(s)",
            self.total(),
            self.value_vectors,
            self.identity_vectors,
            self.noise_vectors,
            self.record_vectors,
            self.session_vectors,
            self.failures.len()
        )
    }
}

/// Which format a file is expected to have, decided by its directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Value,
    Identity,
    Session,
    /// `noise/`: the cacophony files and the Porecatu ones.
    Noise,
    /// `registros/`.
    Records,
    /// Anything else: skipped.
    Other,
}

fn kind_of(relative: &Path) -> Kind {
    let top = relative
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("");
    match top {
        "envelope" | "mensagens" => Kind::Value,
        "identidade" => Kind::Identity,
        // `sessoes-ts/` has the format of `sessoes/`: the same conversations, written by the TypeScript implementation.
        "sessoes" | "sessoes-ts" => Kind::Session,
        "noise" => Kind::Noise,
        "registros" => Kind::Records,
        _ => Kind::Other,
    }
}

/// Checks every vector under `dir`.
pub fn run(dir: &Path, schema: &Schema) -> Report {
    let mut report = Report::default();
    let mut files = Vec::new();
    collect_json(dir, &mut files, &mut report);
    files.sort();
    for file in files {
        let relative = file.strip_prefix(dir).unwrap_or(&file).to_path_buf();
        let text = match fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                report
                    .failures
                    .push((relative, format!("cannot read: {e}")));
                continue;
            }
        };
        match kind_of(&relative) {
            Kind::Value => match serde_json::from_str::<ValueVector>(&text) {
                Ok(v) => match check_value(&v, schema) {
                    Ok(()) => report.value_vectors += 1,
                    Err(e) => report.failures.push((relative, e)),
                },
                Err(e) => report.failures.push((relative, format!("bad format: {e}"))),
            },
            Kind::Identity => match serde_json::from_str::<IdentityVector>(&text) {
                Ok(_) => report.identity_vectors += 1,
                Err(e) => report.failures.push((relative, format!("bad format: {e}"))),
            },
            Kind::Session => match serde_json::from_str::<SessionVector>(&text) {
                Ok(v) => match check_session(&v) {
                    Ok(()) => report.session_vectors += 1,
                    Err(e) => report.failures.push((relative, e)),
                },
                Err(e) => report.failures.push((relative, format!("bad format: {e}"))),
            },
            Kind::Noise => {
                let cacophony = relative
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("cacophony-"));
                if cacophony {
                    match serde_json::from_str::<CacophonyFile>(&text) {
                        Ok(f) => match check_cacophony(&f) {
                            Ok(()) => report.noise_vectors += f.vectors.len(),
                            Err(e) => report.failures.push((relative, e)),
                        },
                        Err(e) => report.failures.push((relative, format!("bad format: {e}"))),
                    }
                } else {
                    match serde_json::from_str::<NoiseVector>(&text) {
                        Ok(v) => match check_noise(&v) {
                            Ok(()) => report.noise_vectors += 1,
                            Err(e) => report.failures.push((relative, e)),
                        },
                        Err(e) => report.failures.push((relative, format!("bad format: {e}"))),
                    }
                }
            }
            Kind::Records => match serde_json::from_str::<RecordVector>(&text) {
                Ok(v) => match check_records(&v) {
                    Ok(()) => report.record_vectors += 1,
                    Err(e) => report.failures.push((relative, e)),
                },
                Err(e) => report.failures.push((relative, format!("bad format: {e}"))),
            },
            Kind::Other => report.skipped.push(relative),
        }
    }
    report
}

fn collect_json(dir: &Path, out: &mut Vec<PathBuf>, report: &mut Report) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            report
                .failures
                .push((dir.to_path_buf(), format!("cannot read directory: {e}")));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json(&path, out, report);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// Checks one value vector; the error is a human-readable reason.
pub fn check_value(v: &ValueVector, schema: &Schema) -> Result<(), String> {
    if !ROOTS.contains(&v.raiz.as_str()) {
        return Err(format!("unknown root rule {:?}", v.raiz));
    }
    if v.valido && v.erro.is_some() {
        return Err("valid vector with an `erro`".to_owned());
    }
    if !v.valido && v.erro.is_none() {
        return Err("invalid vector without an `erro`".to_owned());
    }
    if !v.valido && v.reencode_hex.is_some() {
        return Err("`reencode_hex` on an invalid vector".to_owned());
    }
    let bytes = decode_hex(&v.cbor_hex).map_err(|_| "cbor_hex is not lowercase hex".to_owned())?;

    // (a) cbor_hex x valor.
    let tree = if v.valor.is_null() {
        None
    } else {
        Some(typed_json::to_value(&v.valor).map_err(|e: TypedJsonError| format!("valor: {e}"))?)
    };
    let decoded = cbor::decode(&bytes);
    match (v.valido, &tree) {
        (true, None) => return Err("valid vector with `valor: null`".to_owned()),
        (true, Some(tree)) => {
            let decoded = decoded.map_err(|e| format!("cbor_hex does not decode: {e}"))?;
            if &decoded != tree {
                return Err("decode(cbor_hex) differs from `valor`".to_owned());
            }
            if cbor::encode(tree) != bytes {
                return Err("encode(`valor`) differs from cbor_hex".to_owned());
            }
        }
        (false, None) => {
            if decoded.is_ok() {
                return Err("`valor: null` but cbor_hex decodes".to_owned());
            }
        }
        (false, Some(tree)) => {
            // A non-canonical or message-level-invalid item: the strict
            // decoder may refuse it; when it accepts, it must agree.
            if let Ok(decoded) = decoded
                && &decoded != tree
            {
                return Err("decode(cbor_hex) differs from `valor`".to_owned());
            }
        }
    }

    // (b) CDDL, for valid vectors.
    if v.valido {
        schema
            .validate(&v.raiz, &bytes)
            .map_err(|e| format!("cbor_hex {e}"))?;
    }

    // (c) reencode_hex.
    if let Some(hex) = &v.reencode_hex {
        let re = decode_hex(hex).map_err(|_| "reencode_hex is not lowercase hex".to_owned())?;
        let tree: Value = cbor::decode(&re).map_err(|e| format!("reencode_hex: {e}"))?;
        if cbor::encode(&tree) != re {
            return Err("reencode_hex is not canonical".to_owned());
        }
        schema
            .validate(&v.raiz, &re)
            .map_err(|e| format!("reencode_hex {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::{Layer, Rejection};
    use porecatu_protocol::encoding::encode_hex;
    use serde_json::json;

    fn schema() -> Schema {
        Schema::load().expect("schema loads")
    }

    fn vector(value: serde_json::Value, cbor_hex: &str) -> ValueVector {
        ValueVector {
            descricao: "t".to_owned(),
            versao: [0, 1],
            raiz: "message".to_owned(),
            valor: value,
            cbor_hex: cbor_hex.to_owned(),
            reencode_hex: None,
            valido: true,
            erro: None,
            diag: None,
        }
    }

    fn goodbye() -> (serde_json::Value, String) {
        let value = json!([3, {"map": [["reason", 0]]}]);
        let bytes = cbor::encode(&typed_json::to_value(&value).unwrap());
        (value, encode_hex(&bytes))
    }

    fn rejected(mut v: ValueVector) -> ValueVector {
        v.valido = false;
        v.erro = Some(Rejection {
            camada: Layer::Mensagem,
            codigo: "protocol".to_owned(),
        });
        v
    }

    #[test]
    fn a_good_vector_passes() {
        let (value, hex) = goodbye();
        check_value(&vector(value, &hex), &schema()).unwrap();
    }

    #[test]
    fn value_and_bytes_must_agree() {
        let (_, hex) = goodbye();
        let other = json!([3, {"map": [["reason", 1]]}]);
        let err = check_value(&vector(other, &hex), &schema()).unwrap_err();
        assert!(err.contains("differs"), "{err}");
    }

    #[test]
    fn schema_violation_fails_a_valid_vector() {
        // `reason` must be an integer, here it is text.
        let value = json!([3, {"map": [["reason", "normal"]]}]);
        let bytes = cbor::encode(&typed_json::to_value(&value).unwrap());
        let err = check_value(&vector(value, &encode_hex(&bytes)), &schema()).unwrap_err();
        assert!(err.contains("schema"), "{err}");
    }

    #[test]
    fn unknown_key_with_reencode_passes() {
        // Canonical order: shorter encoded key first.
        let with_extra = json!([3, {"map": [["zzz", 1], ["reason", 0]]}]);
        let bytes = cbor::encode(&typed_json::to_value(&with_extra).unwrap());
        let (_, reencoded) = goodbye();
        let mut v = vector(with_extra, &encode_hex(&bytes));
        v.reencode_hex = Some(reencoded);
        check_value(&v, &schema()).unwrap();
    }

    #[test]
    fn non_canonical_item_is_a_valid_invalid_vector() {
        // A map with keys out of order: valor given, strict decoder refuses.
        let value = json!([3, {"map": [["b", 1], ["a", 2]]}]);
        // 82 03 | a2 "b":1 "a":2
        let v = rejected(vector(value, "8203a2616201616102"));
        check_value(&v, &schema()).unwrap();
    }

    #[test]
    fn not_cbor_at_all_needs_null_valor() {
        let v = rejected(vector(serde_json::Value::Null, "ff"));
        check_value(&v, &schema()).unwrap();
        // A well-formed item cannot claim to be non-CBOR.
        let (_, hex) = goodbye();
        let v = rejected(vector(serde_json::Value::Null, &hex));
        assert!(check_value(&v, &schema()).is_err());
    }

    #[test]
    fn consistency_of_valido_and_erro() {
        let (value, hex) = goodbye();
        let mut v = vector(value.clone(), &hex);
        v.erro = Some(Rejection {
            camada: Layer::Mensagem,
            codigo: "protocol".to_owned(),
        });
        assert!(check_value(&v, &schema()).is_err());
        let mut v = vector(value, &hex);
        v.valido = false;
        assert!(check_value(&v, &schema()).is_err());
    }

    #[test]
    fn unknown_root_and_bad_hex_are_errors() {
        let (value, hex) = goodbye();
        let mut v = vector(value.clone(), &hex);
        v.raiz = "nope".to_owned();
        assert!(check_value(&v, &schema()).is_err());
        let v = vector(value, "ABC");
        assert!(check_value(&v, &schema()).is_err());
    }

    #[test]
    fn the_typescript_transcripts_have_the_session_format() {
        assert_eq!(kind_of(Path::new("sessoes/x.json")), Kind::Session);
        assert_eq!(kind_of(Path::new("sessoes-ts/x.json")), Kind::Session);
    }

    #[test]
    fn empty_directory_reports_zero_vectors() {
        let dir = std::env::temp_dir().join(format!("vectors-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let report = run(&dir, &schema());
        assert_eq!(report.total(), 0);
        assert!(report.failures.is_empty());
        assert!(report.to_string().contains("0 vetores"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn directory_run_classifies_by_folder() {
        let dir = std::env::temp_dir().join(format!("vectors-run-{}", std::process::id()));
        let messages = dir.join("mensagens").join("goodbye");
        fs::create_dir_all(&messages).unwrap();
        fs::create_dir_all(dir.join("noise")).unwrap();
        let (value, hex) = goodbye();
        let good = serde_json::to_string(&vector(value.clone(), &hex)).unwrap();
        fs::write(messages.join("normal.json"), good).unwrap();
        let bad =
            serde_json::to_string(&vector(json!([3, {"map": [["reason", 1]]}]), &hex)).unwrap();
        fs::write(messages.join("errado.json"), bad).unwrap();
        // A cacophony file with no vectors is a failure of its own.
        fs::write(
            dir.join("noise").join("cacophony-xx.json"),
            "{\"formato\": \"noise-cacophony\", \"source\": \"s\", \"license\": \"l\", \"format\": \"f\", \"vectors\": []}",
        )
        .unwrap();
        fs::create_dir_all(dir.join("outra")).unwrap();
        fs::write(dir.join("outra").join("x.json"), "{}").unwrap();
        fs::write(dir.join("README.md"), "ignored").unwrap();

        let report = run(&dir, &schema());
        assert_eq!(report.value_vectors, 1);
        assert_eq!(report.failures.len(), 2);
        assert_eq!(report.skipped.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
