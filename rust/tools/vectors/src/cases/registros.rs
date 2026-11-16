// SPDX-License-Identifier: GPL-3.0-or-later
//! Record vectors (`registros/`): fragmentation, flags, rekey and refusals.
//!
//! The transport is keyed by an `IK` handshake with the fixed keys. Each
//! message is the pattern `i mod 251` of a given length (the vector stores the
//! length and the BLAKE2s-256 of the plaintext, never the plaintext). The
//! records are stored in full (`registros_hex`) only for messages up to
//! 65 519 bytes; above that only their BLAKE2s-256 (`registros_blake2s`) is
//! stored, to keep the files small.
//!
//! While a case is built, three independent receivers read what the sender
//! wrote: a raw [`Transport`] that parses the flags itself (so `flags` is read
//! from the ciphertext, not predicted), and a [`Session`] that reassembles the
//! message.

use porecatu_protocol::PROTOCOL_VERSION;
use porecatu_protocol::encoding::encode_hex;
use porecatu_protocol::session::{
    FLAG_CONTINUE, FLAG_REKEY, MAX_MESSAGE, MAX_PLAINTEXT, RecordError, RekeyPolicy, Session,
    Transport,
};

use super::Generated;
use super::duo::{Keys, blake2s_hex, fixed_host_id, ik_transports, pattern};
use crate::vector::{Layer, Pattern, RecordStep, RecordVector, Rejection, RekeyPolicyJson};

/// Messages up to this length keep their records in full.
const FULL_RECORDS_UP_TO: usize = MAX_PLAINTEXT;

/// The default rekey policy of the spec (100 000 records, 60 minutes).
fn default_policy() -> RekeyPolicy {
    RekeyPolicy::default()
}

fn plain(len: usize) -> Pattern {
    Pattern {
        padrao: "i mod 251".to_owned(),
        tamanho: len as u64,
        blake2s: blake2s_hex(&pattern(len)),
    }
}

fn record_step(
    from: &str,
    now_ms: u64,
    len: usize,
    flags: Vec<u8>,
    records: &[Vec<u8>],
    forged: bool,
) -> RecordStep {
    RecordStep {
        de: from.to_owned(),
        now_ms,
        texto_claro: plain(len),
        flags,
        registros_hex: (len <= FULL_RECORDS_UP_TO)
            .then(|| records.iter().map(|r| encode_hex(r)).collect()),
        registros_blake2s: records.iter().map(|r| blake2s_hex(r)).collect(),
        forjado: forged,
    }
}

fn vector(
    descricao: &str,
    policy: RekeyPolicy,
    handshake_ms: u64,
    passos: Vec<RecordStep>,
    erro: Option<(&str, &str)>,
) -> RecordVector {
    let keys = Keys::standard();
    RecordVector {
        descricao: descricao.to_owned(),
        versao: PROTOCOL_VERSION,
        host_id: fixed_host_id().as_str().to_owned(),
        chaves: keys.json(),
        politica: RekeyPolicyJson {
            max_records: policy.max_records,
            max_age_ms: policy.max_age_ms,
        },
        handshake_ms,
        passos,
        valido: erro.is_none(),
        erro: erro.map(|(_, codigo)| Rejection {
            camada: Layer::Registro,
            codigo: codigo.to_owned(),
        }),
    }
}

/// The two senders and the two kinds of receiver of one case.
struct Rig {
    client: Session,
    host: Session,
    /// Raw receivers: `raw_host` reads what the client sends.
    raw_client: Transport,
    raw_host: Transport,
    /// Session receivers.
    read_client: Session,
    read_host: Session,
    steps: Vec<RecordStep>,
}

impl Rig {
    fn new(policy: RekeyPolicy, handshake_ms: u64) -> Rig {
        let keys = Keys::standard();
        let (client, host) = ik_transports(&keys);
        let (raw_client, raw_host) = ik_transports(&keys);
        let (read_client, read_host) = ik_transports(&keys);
        Rig {
            client: Session::with_policy(client, policy, Some(handshake_ms)),
            host: Session::with_policy(host, policy, Some(handshake_ms)),
            raw_client,
            raw_host,
            read_client: Session::new(read_client, Some(handshake_ms)),
            read_host: Session::new(read_host, Some(handshake_ms)),
            steps: Vec::new(),
        }
    }

    /// `from` sends the pattern message of `len` bytes at `now_ms`.
    fn send(&mut self, from: &str, now_ms: u64, len: usize) {
        let message = pattern(len);
        let (sender, raw, reader) = match from {
            "cliente" => (&mut self.client, &mut self.raw_host, &mut self.read_host),
            _ => (&mut self.host, &mut self.raw_client, &mut self.read_client),
        };
        let records = sender.seal(&message, now_ms).expect("the message seals");

        // The flags, read from the ciphertext by a receiver that knows
        // nothing about the sender.
        let mut flags = Vec::new();
        let mut reassembled = Vec::new();
        for record in &records {
            let plaintext = raw.decrypt(record).expect("the record decrypts");
            let flag = plaintext[0];
            flags.push(flag);
            reassembled.extend_from_slice(&plaintext[1..]);
            if flag & FLAG_REKEY != 0 {
                raw.rekey_incoming();
            }
        }
        assert_eq!(reassembled, message, "the fragments rebuild the message");
        let last = records.len() - 1;
        for (index, flag) in flags.iter().enumerate() {
            assert_eq!(flag & FLAG_CONTINUE != 0, index != last, "CONTINUA");
            assert_eq!(flag & !(FLAG_CONTINUE | FLAG_REKEY), 0, "reserved bits");
        }

        // The record layer of the receiver.
        for (index, record) in records.iter().enumerate() {
            let out = reader.open(record, now_ms).expect("the record opens");
            if index == last {
                assert_eq!(out.as_deref(), Some(&message[..]), "the message");
            } else {
                assert!(out.is_none(), "a fragment completes nothing");
            }
        }
        self.steps
            .push(record_step(from, now_ms, len, flags, &records, false));
    }
}

fn single(descricao: &str, len: usize) -> RecordVector {
    let mut rig = Rig::new(default_policy(), 0);
    rig.send("cliente", 0, len);
    vector(descricao, default_policy(), 0, rig.steps, None)
}

fn rekey_by_count() -> RecordVector {
    let policy = RekeyPolicy {
        max_records: 3,
        max_age_ms: default_policy().max_age_ms,
    };
    let mut rig = Rig::new(policy, 0);
    // Ten messages in both directions: the third record each side sends and
    // every third after it carries REKEY.
    for (index, from) in [
        "cliente", "cliente", "host", "cliente", "host", "cliente", "host", "cliente", "cliente",
        "cliente",
    ]
    .into_iter()
    .enumerate()
    {
        rig.send(from, index as u64, 10);
    }
    let rekeys: Vec<_> = rig
        .steps
        .iter()
        .filter(|s| s.flags.iter().any(|f| f & FLAG_REKEY != 0))
        .map(|s| (s.de.clone(), s.now_ms))
        .collect();
    assert_eq!(
        rekeys,
        [
            ("cliente".to_owned(), 3),
            ("host".to_owned(), 6),
            ("cliente".to_owned(), 8),
        ]
    );
    vector(
        "REKEY forçado por contagem: política de 3 registros, dez mensagens nos dois sentidos",
        policy,
        0,
        rig.steps,
        None,
    )
}

fn rekey_by_age() -> RecordVector {
    let policy = RekeyPolicy {
        max_records: default_policy().max_records,
        max_age_ms: 1_000,
    };
    let mut rig = Rig::new(policy, 0);
    for (from, now) in [
        ("cliente", 0),
        ("cliente", 999),
        ("cliente", 1_000),
        ("cliente", 1_500),
        ("cliente", 2_000),
        ("host", 2_500),
        ("host", 2_600),
    ] {
        rig.send(from, now, 10);
    }
    let rekeys: Vec<_> = rig
        .steps
        .iter()
        .filter(|s| s.flags.iter().any(|f| f & FLAG_REKEY != 0))
        .map(|s| s.now_ms)
        .collect();
    assert_eq!(rekeys, [1_000, 2_000, 2_500]);
    vector(
        "REKEY forçado por idade: chave de 1000 ms, o handshake termina em 0 ms",
        policy,
        0,
        rig.steps,
        None,
    )
}

/// `max_records = 1`: every record carries `REKEY`, including a `CONTINUA` one.
fn rekey_in_a_fragment() -> RecordVector {
    let policy = RekeyPolicy {
        max_records: 1,
        max_age_ms: default_policy().max_age_ms,
    };
    let mut rig = Rig::new(policy, 0);
    rig.send("cliente", 0, MAX_PLAINTEXT);
    rig.send("host", 0, 10);
    assert_eq!(rig.steps[0].flags, [FLAG_CONTINUE | FLAG_REKEY, FLAG_REKEY]);
    vector(
        "REKEY num registro com CONTINUA: política de 1 registro, mensagem de 65 519 bytes em dois registros",
        policy,
        0,
        rig.steps,
        None,
    )
}

/// A record whose flags byte has a reserved bit set.
fn reserved_flag() -> RecordVector {
    let keys = Keys::standard();
    let (mut sender, receiver) = ik_transports(&keys);
    let mut receiver = Session::new(receiver, Some(0));
    let mut plaintext = vec![0x04];
    plaintext.extend_from_slice(&pattern(10));
    let record = sender.encrypt(&plaintext).expect("the record encrypts");
    assert_eq!(
        receiver.open(&record, 0).err(),
        Some(RecordError::ReservedFlags)
    );
    let step = record_step("cliente", 0, 10, vec![0x04], &[record], true);
    vector(
        "registro com o bit reservado 0x04 ligado: a sessão fecha",
        default_policy(),
        0,
        vec![step],
        Some(("registro", "reserved-flag")),
    )
}

/// A record with one bit flipped: it does not authenticate.
fn tampered() -> RecordVector {
    let keys = Keys::standard();
    let (sender, receiver) = ik_transports(&keys);
    let mut sender = Session::new(sender, Some(0));
    let mut receiver = Session::new(receiver, Some(0));
    let mut records = sender.seal(&pattern(10), 0).expect("seals");
    let record = &mut records[0];
    let last = record.len() - 1;
    record[last] ^= 0x01;
    assert!(matches!(
        receiver.open(record, 0),
        Err(RecordError::Transport(_))
    ));
    let step = record_step("cliente", 0, 10, vec![0], &records, true);
    vector(
        "registro com o último bit do texto cifrado trocado: a decifração falha e a sessão fecha",
        default_policy(),
        0,
        vec![step],
        Some(("registro", "decrypt")),
    )
}

/// A message one byte above 16 MiB: the sender refuses to seal it.
fn too_large() -> RecordVector {
    let keys = Keys::standard();
    let (sender, _) = ik_transports(&keys);
    let mut sender = Session::new(sender, Some(0));
    let len = MAX_MESSAGE + 1;
    assert_eq!(
        sender.seal(&pattern(len), 0).err(),
        Some(RecordError::MessageTooLong)
    );
    let step = record_step("cliente", 0, len, vec![], &[], false);
    vector(
        "mensagem de 16 MiB + 1 byte: quem envia recusa e nenhum registro sai",
        default_policy(),
        0,
        vec![step],
        Some(("registro", "too-large")),
    )
}

/// All record vectors.
pub fn all() -> Vec<Generated> {
    vec![
        Generated::records(
            "registros/mensagem-vazia.json",
            single("mensagem de 0 bytes: um registro só com o byte de flags", 0),
        ),
        Generated::records(
            "registros/um-registro-65518.json",
            single(
                "mensagem de 65 518 bytes: o que cabe num registro (flags + 65 518 = 65 519 de texto claro)",
                65_518,
            ),
        ),
        Generated::records(
            "registros/dois-registros-65519.json",
            single(
                "mensagem de 65 519 bytes: um byte a mais que cabe, dois registros (CONTINUA no primeiro)",
                65_519,
            ),
        ),
        Generated::records(
            "registros/tres-registros-131037.json",
            single(
                "mensagem de 131 037 bytes (2 x 65 518 + 1): três registros; só os BLAKE2s-256 dos registros",
                131_037,
            ),
        ),
        Generated::records(
            "registros/grande-200000.json",
            single(
                "mensagem de 200 000 bytes: quatro registros; só os BLAKE2s-256 dos registros",
                200_000,
            ),
        ),
        Generated::records("registros/rekey-por-contagem.json", rekey_by_count()),
        Generated::records("registros/rekey-por-idade.json", rekey_by_age()),
        Generated::records("registros/rekey-em-fragmento.json", rekey_in_a_fragment()),
        Generated::records("registros/bit-reservado.json", reserved_flag()),
        Generated::records("registros/registro-adulterado.json", tampered()),
        Generated::records("registros/mensagem-acima-de-16-mib.json", too_large()),
    ]
}
