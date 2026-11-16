// SPDX-License-Identifier: GPL-3.0-or-later
//! Generator and validator of the protocol test vectors.
//!
//! Subcommands:
//!
//! - `check [<dir>]`: validates every vector under `<dir>` (default
//!   `docs/especificacao/vetores-de-teste`) against the CBOR layer of the Rust
//!   implementation and the CDDL schema; exits 1 on any failure;
//! - `schema`: parses `schema/porecatu-remote.cddl` and prints `ok` or the
//!   parse error;
//! - `generate [<dir>]`: writes the vectors (`identidade/`, `envelope/`,
//!   `mensagens/`, `noise/`, `registros/`, `sessoes/`) under `<dir>`,
//!   replacing the ones already there (except the vendored
//!   `noise/cacophony-*.json`); deterministic, and every vector is checked
//!   against the crate first (the Noise, record and session ones run the
//!   crate while they are built).

mod cases;
mod check;
mod check_formats;
mod generate;
mod schema;
mod typed_json;
mod vector;

use std::path::PathBuf;
use std::process::ExitCode;

use schema::Schema;

const DEFAULT_VECTORS: &str = "docs/especificacao/vetores-de-teste";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("generate") => run_generate(args.next().map(PathBuf::from)),
        Some("schema") => run_schema(),
        Some("check") => run_check(args.next().map(PathBuf::from)),
        _ => {
            eprintln!("usage: vectors <generate|check [dir]|schema>");
            ExitCode::from(2)
        }
    }
}

fn run_generate(dir: Option<PathBuf>) -> ExitCode {
    let dir = dir.unwrap_or_else(|| schema::repo_root().join(DEFAULT_VECTORS));
    match generate::run(&dir) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("generate: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_schema() -> ExitCode {
    match Schema::load() {
        Ok(_) => {
            println!("ok");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_check(dir: Option<PathBuf>) -> ExitCode {
    let schema = match Schema::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let dir = dir.unwrap_or_else(|| schema::repo_root().join(DEFAULT_VECTORS));
    let report = check::run(&dir, &schema);
    println!("{report}");
    if report.failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
