// SPDX-License-Identifier: GPL-3.0-or-later
//! Consistency checks of the Noise, record and session vectors for the
//! `check` subcommand: hex, lengths, hashes, and the frames against the
//! crate's envelope decoders. Replaying them against the handshake and record
//! layers is the job of `rust/tests/vectors.rs`.

use porecatu_protocol::cbor::Value;
use porecatu_protocol::encoding::decode_hex;
use porecatu_protocol::envelope::{
    ChannelId, ClientEnvelope, HostEnvelope, decode_client, decode_handshake_start, decode_host,
};
use porecatu_protocol::session::{MAX_NOISE_MESSAGE, MAX_PLAINTEXT, NOISE_IK, NOISE_XX, prologue};

use crate::cases::duo::{blake2s_hex, pattern};
use crate::typed_json;
use crate::vector::{CacophonyFile, NoiseVector, RecordVector, SessionKeys, SessionVector};

fn hex_bytes(text: &str, what: &str) -> Result<Vec<u8>, String> {
    decode_hex(text).map_err(|_| format!("{what} is not lowercase hex"))
}

/// A typed `{"bstr": ...}`; `len` is the required length, `None` for any.
fn typed_bytes(
    json: &serde_json::Value,
    len: Option<usize>,
    what: &str,
) -> Result<Vec<u8>, String> {
    let value = typed_json::to_value(json).map_err(|e| format!("{what}: {e}"))?;
    match value {
        Value::Bytes(b) if len.is_none_or(|n| b.len() == n) => Ok(b),
        _ => Err(format!("{what} is not a bstr of the right size")),
    }
}

fn check_keys(keys: &SessionKeys) -> Result<(), String> {
    for (json, what) in [
        (&keys.host_estatica, "host_estatica"),
        (&keys.host_efemera, "host_efemera"),
        (&keys.cliente_estatica, "cliente_estatica"),
        (&keys.cliente_efemera, "cliente_efemera"),
    ] {
        typed_bytes(json, Some(32), what)?;
    }
    Ok(())
}

fn side(name: &str) -> Result<(), String> {
    if name == "cliente" || name == "host" {
        Ok(())
    } else {
        Err(format!("unknown side {name:?}"))
    }
}

/// Checks one Noise vector with the Porecatu prologue.
pub fn check_noise(v: &NoiseVector) -> Result<(), String> {
    let expected_name = match v.padrao.as_str() {
        "XX" => NOISE_XX,
        "IK" => NOISE_IK,
        other => return Err(format!("unknown pattern {other:?}")),
    };
    if v.nome != expected_name {
        return Err(format!("name {:?} is not {expected_name}", v.nome));
    }
    if hex_bytes(&v.prologo_hex, "prologo_hex")? != prologue(&v.host_id) {
        return Err("prologo_hex is not the prologue of host_id".to_owned());
    }
    check_keys(&v.chaves)?;
    typed_bytes(&v.chave_do_host, Some(32), "chave_do_host")?;
    for message in &v.mensagens {
        side(&message.de)?;
        typed_bytes(&message.payload, None, "payload")?;
        let wire = hex_bytes(&message.hex, "hex")?;
        if wire.len() > MAX_NOISE_MESSAGE {
            return Err("a handshake message above 65 535 bytes".to_owned());
        }
    }
    let complete = if v.padrao == "XX" { 3 } else { 2 };
    if v.valido {
        if v.mensagens.len() != complete {
            return Err(format!("a valid {} has {complete} messages", v.padrao));
        }
        let hash = v
            .handshake_hash
            .as_deref()
            .ok_or("valid without handshake_hash")?;
        if hex_bytes(hash, "handshake_hash")?.len() != 32 {
            return Err("handshake_hash is not 32 bytes".to_owned());
        }
        if v.erro.is_some() || v.quem_falha.is_some() {
            return Err("valid vector with `erro` or `quem_falha`".to_owned());
        }
    } else {
        if v.mensagens.len() >= complete {
            return Err("an invalid vector with every message".to_owned());
        }
        if v.handshake_hash.is_some() {
            return Err("invalid vector with a handshake_hash".to_owned());
        }
        side(
            v.quem_falha
                .as_deref()
                .ok_or("invalid without `quem_falha`")?,
        )?;
        if v.erro.is_none() {
            return Err("invalid vector without an `erro`".to_owned());
        }
    }
    Ok(())
}

/// Checks the vendored cacophony file.
pub fn check_cacophony(file: &CacophonyFile) -> Result<(), String> {
    if file.formato != "noise-cacophony" {
        return Err(format!("formato {:?}", file.formato));
    }
    if file.vectors.is_empty() {
        return Err("no vectors".to_owned());
    }
    for (index, v) in file.vectors.iter().enumerate() {
        let at = |e: String| format!("vector {index}: {e}");
        if v.protocol_name != NOISE_XX && v.protocol_name != NOISE_IK {
            return Err(at(format!("protocol {:?}", v.protocol_name)));
        }
        for (hex, what) in [
            (&v.init_static, "init_static"),
            (&v.init_ephemeral, "init_ephemeral"),
            (&v.resp_static, "resp_static"),
            (&v.resp_ephemeral, "resp_ephemeral"),
        ] {
            if hex_bytes(hex, what).map_err(at)?.len() != 32 {
                return Err(at(format!("{what} is not 32 bytes")));
            }
        }
        hex_bytes(&v.init_prologue, "init_prologue").map_err(at)?;
        hex_bytes(&v.resp_prologue, "resp_prologue").map_err(at)?;
        if (v.protocol_name == NOISE_IK) != v.init_remote_static.is_some() {
            return Err(at(
                "init_remote_static is for IK only, and IK needs it".to_owned()
            ));
        }
        if let Some(hash) = &v.handshake_hash
            && hex_bytes(hash, "handshake_hash").map_err(at)?.len() != 32
        {
            return Err(at("handshake_hash is not 32 bytes".to_owned()));
        }
        let handshake = if v.protocol_name == NOISE_XX { 3 } else { 2 };
        if v.messages.len() <= handshake {
            return Err(at("no transport messages".to_owned()));
        }
        for message in &v.messages {
            hex_bytes(&message.payload, "payload").map_err(at)?;
            hex_bytes(&message.ciphertext, "ciphertext").map_err(at)?;
        }
    }
    Ok(())
}

/// Checks one record vector.
pub fn check_records(v: &RecordVector) -> Result<(), String> {
    check_keys(&v.chaves)?;
    if v.passos.is_empty() {
        return Err("no steps".to_owned());
    }
    if v.valido && v.erro.is_some() {
        return Err("valid vector with an `erro`".to_owned());
    }
    if !v.valido && v.erro.is_none() {
        return Err("invalid vector without an `erro`".to_owned());
    }
    if v.politica.max_records == 0 || v.politica.max_age_ms == 0 {
        return Err("a rekey policy with a zero limit".to_owned());
    }
    for (index, step) in v.passos.iter().enumerate() {
        let at = |e: String| format!("step {index}: {e}");
        side(&step.de).map_err(at)?;
        if step.texto_claro.padrao != "i mod 251" {
            return Err(at("unknown plaintext pattern".to_owned()));
        }
        let len = usize::try_from(step.texto_claro.tamanho).map_err(|e| at(e.to_string()))?;
        if blake2s_hex(&pattern(len)) != step.texto_claro.blake2s {
            return Err(at(
                "texto_claro.blake2s does not match the pattern".to_owned()
            ));
        }
        if step.registros_blake2s.len() != step.flags.len() {
            return Err(at("flags and registros_blake2s differ in length".to_owned()));
        }
        match &step.registros_hex {
            Some(records) => {
                if len > MAX_PLAINTEXT {
                    return Err(at(
                        "records in full above 65 519 bytes of message".to_owned()
                    ));
                }
                if records.len() != step.flags.len() {
                    return Err(at("registros_hex and flags differ in length".to_owned()));
                }
                for (record, hash) in records.iter().zip(&step.registros_blake2s) {
                    let bytes = hex_bytes(record, "registros_hex").map_err(at)?;
                    if bytes.len() > MAX_NOISE_MESSAGE {
                        return Err(at("a record above 65 535 bytes".to_owned()));
                    }
                    if &blake2s_hex(&bytes) != hash {
                        return Err(at("a record and its BLAKE2s differ".to_owned()));
                    }
                }
            }
            None => {
                if len <= MAX_PLAINTEXT {
                    return Err(at("a message that fits must keep its records".to_owned()));
                }
            }
        }
        if step.forjado && (v.passos.len() != 1 || v.valido) {
            return Err(at(
                "forjado is for the single step of an invalid vector".to_owned()
            ));
        }
    }
    Ok(())
}

/// Checks one session vector: the frames against the envelope decoders.
pub fn check_session(v: &SessionVector) -> Result<(), String> {
    check_keys(&v.chaves)?;
    let handshake = match v.handshake.as_str() {
        "XX" => 3,
        "IK" => 2,
        other => return Err(format!("unknown handshake {other:?}")),
    };
    if v.passos.len() <= handshake {
        return Err("a conversation shorter than its handshake".to_owned());
    }
    if !v.token_host.is_null() {
        typed_bytes(&v.token_host, Some(16), "token_host")?;
    }
    for key in &v.pareados {
        typed_bytes(key, Some(32), "pareados")?;
    }
    let mut closed = false;
    for (index, step) in v.passos.iter().enumerate() {
        let at = |e: String| format!("step {index}: {e}");
        side(&step.de).map_err(at)?;
        if closed {
            return Err(at("a step after the channel closed".to_owned()));
        }
        let records: Vec<Vec<u8>> = step
            .registros_hex
            .iter()
            .map(|h| hex_bytes(h, "registros_hex"))
            .collect::<Result<_, _>>()
            .map_err(at)?;
        let frames: Vec<Vec<u8>> = step
            .envelope_hex
            .iter()
            .map(|h| hex_bytes(h, "envelope_hex"))
            .collect::<Result<_, _>>()
            .map_err(at)?;
        if !step.mensagem.is_null() {
            typed_json::to_value(&step.mensagem).map_err(|e| at(format!("mensagem: {e}")))?;
        }
        if records.is_empty() {
            // Only the host closing the channel has no record.
            let [frame] = frames.as_slice() else {
                return Err(at(
                    "a step without records needs one close-channel frame".to_owned()
                ));
            };
            match decode_host(frame.clone().into()) {
                Ok(HostEnvelope::CloseChannel { channel, .. })
                    if step.de == "host" && channel == ChannelId(v.canal) =>
                {
                    closed = true;
                }
                _ => return Err(at("not a close-channel for the channel".to_owned())),
            }
            continue;
        }
        if records.len() != frames.len() {
            return Err(at("records and frames differ in length".to_owned()));
        }
        for (position, (record, frame)) in records.iter().zip(frames).enumerate() {
            let payload = if step.de == "cliente" {
                match decode_client(frame.into()) {
                    Ok(ClientEnvelope::Data { payload }) => payload.into_bytes(),
                    _ => return Err(at("a client frame that is not `data`".to_owned())),
                }
            } else {
                match decode_host(frame.into()) {
                    Ok(HostEnvelope::ChannelData { channel, payload })
                        if channel == ChannelId(v.canal) =>
                    {
                        payload.into_bytes()
                    }
                    _ => return Err(at("a host frame that is not `channel-data`".to_owned())),
                }
            };
            if index == 0 && position == 0 {
                let start = decode_handshake_start(payload)
                    .map_err(|e| at(format!("first payload: {e}")))?;
                if start.msg1[..] != record[..] {
                    return Err(at("handshake-start does not carry msg1".to_owned()));
                }
            } else if payload[..] != record[..] {
                return Err(at("a frame does not carry its record".to_owned()));
            }
        }
        if index < handshake {
            if records.len() != 1 {
                return Err(at("a handshake step is one record".to_owned()));
            }
        } else if step.mensagem.is_null() {
            return Err(at("a message step without `mensagem`".to_owned()));
        }
    }
    match (v.resultado.estado.as_str(), closed) {
        ("aberta", false) | ("fechada", true) => {}
        (estado, _) => return Err(format!("estado {estado:?} does not match the last step")),
    }
    if (v.resultado.estado == "fechada") != v.resultado.erro.is_some() {
        return Err("`erro` is for a closed conversation only".to_owned());
    }
    Ok(())
}
