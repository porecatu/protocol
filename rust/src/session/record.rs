// SPDX-License-Identifier: GPL-3.0-or-later

//! Encrypted records: fragmentation, reassembly and rekey (spec 02 §5).
//!
//! Every `payload` of the channel after the handshake is one Noise transport
//! message whose plaintext is a flags byte followed by a fragment of a CBOR
//! message. [`Session`] wraps the [`Transport`] of a finished handshake and
//! turns messages into records and records into messages.
//!
//! Sans-IO: records go in and out as byte vectors, `now_ms` is a monotonic
//! millisecond number received from outside. After **any** error the session
//! is unusable: there is no resend and no nonce skip.

use core::fmt;

use super::handshake::{HandshakeError, MAX_PLAINTEXT, Transport};
use super::rekey::{RekeyPolicy, RekeyState};

/// Flag bit 0: the message continues in the next record.
pub const FLAG_CONTINUE: u8 = 0x01;

/// Flag bit 1: the sender calls `Rekey()` on its sending cipher after this
/// record.
pub const FLAG_REKEY: u8 = 0x02;

/// Bits 2..7: reserved, must be zero.
const RESERVED_FLAGS: u8 = !(FLAG_CONTINUE | FLAG_REKEY);

/// Largest fragment of a message in one record: the Noise plaintext limit
/// minus the flags byte.
pub const MAX_FRAGMENT: usize = MAX_PLAINTEXT - 1;

/// Largest message, sealed or reassembled: 16 MiB.
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// Why a record operation failed. Typed codes only; no record bytes inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordError {
    /// A record with a reserved flag bit set.
    ReservedFlags,
    /// A message above [`MAX_MESSAGE`], on the way out or reassembled.
    MessageTooLong,
    /// The transport refused the record: authentication failure, a record
    /// above 65 535 bytes, or a plaintext without the flags byte (the three
    /// are `Transport(HandshakeError::Decrypt)`).
    Transport(HandshakeError),
    /// The session cannot be used: an earlier error, or the nonce space of
    /// the sending direction is exhausted.
    Failed,
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            RecordError::ReservedFlags => "record with a reserved flag bit set",
            RecordError::MessageTooLong => "message above 16 MiB",
            RecordError::Transport(_) => "transport refused the record",
            RecordError::Failed => "session already failed",
        };
        f.write_str(text)
    }
}

impl core::error::Error for RecordError {}

impl From<HandshakeError> for RecordError {
    fn from(error: HandshakeError) -> Self {
        match error {
            // The nonce space ended: the session cannot go on.
            HandshakeError::Exhausted => RecordError::Failed,
            // An oversized record is reported like any record that does not
            // decrypt.
            HandshakeError::Malformed => RecordError::Transport(HandshakeError::Decrypt),
            other => RecordError::Transport(other),
        }
    }
}

/// An established session: the transport plus fragmentation, reassembly and
/// the rekey schedule.
pub struct Session {
    transport: Transport,
    policy: RekeyPolicy,
    rekey: RekeyState,
    /// Fragments of the message being reassembled.
    partial: Vec<u8>,
    failed: bool,
}

impl Session {
    /// Wraps the transport of a finished handshake, with the default rekey
    /// policy (100 000 records, 60 minutes). `handshake_ms` is the monotonic
    /// time at which the handshake ended: the first key's age counts from it.
    /// Without it, the age counts from the first [`Session::seal`].
    #[must_use]
    pub fn new(transport: Transport, handshake_ms: Option<u64>) -> Session {
        Session::with_policy(transport, RekeyPolicy::default(), handshake_ms)
    }

    /// Like [`Session::new`] with an explicit rekey policy.
    #[must_use]
    pub fn with_policy(
        transport: Transport,
        policy: RekeyPolicy,
        handshake_ms: Option<u64>,
    ) -> Session {
        Session {
            transport,
            policy,
            rekey: handshake_ms.map_or_else(RekeyState::unstarted, RekeyState::new),
            partial: Vec::new(),
            failed: false,
        }
    }

    /// The static key of the other side.
    #[must_use]
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.transport.remote_static()
    }

    /// The rekey state of the sending direction.
    #[must_use]
    pub fn rekey_state(&self) -> RekeyState {
        self.rekey
    }

    /// Whether an error already made the session unusable.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Turns one message into records, in the order they must be sent.
    ///
    /// An empty message is one record with no fragment. All records but the
    /// last carry `CONTINUA`. The record in which the policy says so carries
    /// `REKEY`, and the sending key changes right **after** it is encrypted.
    ///
    /// # Errors
    ///
    /// [`RecordError::MessageTooLong`] above [`MAX_MESSAGE`],
    /// [`RecordError::Transport`] when encryption fails, and
    /// [`RecordError::Failed`] on a session that already failed. Any error
    /// leaves the session unusable.
    pub fn seal(&mut self, message: &[u8], now_ms: u64) -> Result<Vec<Vec<u8>>, RecordError> {
        self.ensure_usable()?;
        if message.len() > MAX_MESSAGE {
            return Err(self.fail(RecordError::MessageTooLong));
        }
        self.rekey.start_if_unknown(now_ms);

        let mut fragments: Vec<&[u8]> = message.chunks(MAX_FRAGMENT).collect();
        if fragments.is_empty() {
            fragments.push(&[]);
        }
        let last = fragments.len() - 1;

        let mut records = Vec::with_capacity(fragments.len());
        for (index, fragment) in fragments.into_iter().enumerate() {
            let rekey_now = self.policy.should_rekey(&self.rekey, now_ms);
            let mut flags = 0u8;
            if index != last {
                flags |= FLAG_CONTINUE;
            }
            if rekey_now {
                flags |= FLAG_REKEY;
            }

            let mut plaintext = Vec::with_capacity(1 + fragment.len());
            plaintext.push(flags);
            plaintext.extend_from_slice(fragment);
            let record = match self.transport.encrypt(&plaintext) {
                Ok(record) => record,
                Err(error) => return Err(self.fail(error.into())),
            };

            self.rekey.record_sent();
            if rekey_now {
                self.transport.rekey_outgoing();
                self.rekey.rekeyed(now_ms);
            }
            records.push(record);
        }
        Ok(records)
    }

    /// Takes one record from the channel.
    ///
    /// Returns `Some(message)` when the record completes a message and `None`
    /// while more fragments are expected. A `REKEY` record changes the
    /// receiving key right **after** it is decrypted. `now_ms` is reserved:
    /// the receiving side enforces no deadline (spec 02 §5 binds the sender).
    ///
    /// # Errors
    ///
    /// [`RecordError::Transport`] (`Decrypt`) when decryption fails, the
    /// record is above 65 535 bytes or its plaintext has no flags byte,
    /// [`RecordError::ReservedFlags`],
    /// [`RecordError::MessageTooLong`] when the reassembly passes
    /// [`MAX_MESSAGE`], and [`RecordError::Failed`] on a session that already
    /// failed. Any error leaves the session unusable.
    pub fn open(&mut self, record: &[u8], _now_ms: u64) -> Result<Option<Vec<u8>>, RecordError> {
        self.ensure_usable()?;

        let plaintext = match self.transport.decrypt(record) {
            Ok(plaintext) => plaintext,
            Err(error) => return Err(self.fail(error.into())),
        };
        let Some((&flags, fragment)) = plaintext.split_first() else {
            return Err(self.fail(RecordError::Transport(HandshakeError::Decrypt)));
        };
        if flags & RESERVED_FLAGS != 0 {
            return Err(self.fail(RecordError::ReservedFlags));
        }
        if flags & FLAG_REKEY != 0 {
            self.transport.rekey_incoming();
        }
        if self.partial.len() + fragment.len() > MAX_MESSAGE {
            return Err(self.fail(RecordError::MessageTooLong));
        }

        if flags & FLAG_CONTINUE != 0 {
            self.partial.extend_from_slice(fragment);
            return Ok(None);
        }
        if self.partial.is_empty() {
            return Ok(Some(fragment.to_vec()));
        }
        let mut message = core::mem::take(&mut self.partial);
        message.extend_from_slice(fragment);
        Ok(Some(message))
    }

    fn ensure_usable(&self) -> Result<(), RecordError> {
        if self.failed {
            Err(RecordError::Failed)
        } else {
            Ok(())
        }
    }

    fn fail(&mut self, error: RecordError) -> RecordError {
        self.failed = true;
        self.partial = Vec::new();
        error
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::super::handshake::{
        ClientHandshake, ClientStep, HandshakeKind, HostHandshake, HostStep,
    };
    use super::super::keys::StaticKeypair;
    use super::*;

    const HOST_ID: &str = "k3x9testhostidentifier";

    /// Two transports linked by a real `IK` handshake: (client, host).
    fn transports() -> (Transport, Transport) {
        let client_keys = StaticKeypair::generate();
        let host_keys = StaticKeypair::generate();
        let (mut client, msg1) =
            ClientHandshake::connect(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let HostStep::Connect { msg2, transport } =
            HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &msg1).unwrap()
        else {
            panic!("connect must yield HostStep::Connect");
        };
        let ClientStep::Established(client_transport) = client.read_message2(&msg2).unwrap() else {
            panic!("IK must be established after msg2");
        };
        (client_transport, transport)
    }

    fn sessions() -> (Session, Session) {
        let (client, host) = transports();
        (Session::new(client, Some(0)), Session::new(host, Some(0)))
    }

    fn sessions_with(policy: RekeyPolicy) -> (Session, Session) {
        let (client, host) = transports();
        (
            Session::with_policy(client, policy, Some(0)),
            Session::with_policy(host, policy, Some(0)),
        )
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// Sends `message` from `from` and delivers every record to `to`.
    fn deliver(from: &mut Session, to: &mut Session, message: &[u8], now_ms: u64) -> Vec<Vec<u8>> {
        let records = from.seal(message, now_ms).unwrap();
        let mut done = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let opened = to.open(record, now_ms).unwrap();
            if index + 1 == records.len() {
                done.push(opened.expect("last record completes the message"));
            } else {
                assert!(opened.is_none(), "a fragment must not complete a message");
            }
        }
        done
    }

    #[test]
    fn empty_message_is_one_record() {
        let (mut client, mut host) = sessions();
        let records = client.seal(&[], 0).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(host.open(&records[0], 0).unwrap(), Some(Vec::new()));
    }

    #[test]
    fn one_full_record_holds_65_518_bytes() {
        let (mut client, mut host) = sessions();
        let message = pattern(65_518);
        let records = client.seal(&message, 0).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].len(), 65_535);
        assert_eq!(host.open(&records[0], 0).unwrap(), Some(message));
    }

    #[test]
    fn one_byte_more_makes_two_records() {
        let (mut client, mut host) = sessions();
        let message = pattern(65_519);
        let records = client.seal(&message, 0).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(host.open(&records[0], 0).unwrap(), None);
        assert_eq!(host.open(&records[1], 0).unwrap(), Some(message));
    }

    #[test]
    fn two_hundred_thousand_bytes_make_four_records_in_both_directions() {
        let (mut client, mut host) = sessions();
        let message = pattern(200_000);
        assert_eq!(
            deliver(&mut client, &mut host, &message, 0),
            vec![message.clone()]
        );
        assert_eq!(
            deliver(&mut host, &mut client, &message, 0),
            vec![message.clone()]
        );
        assert_eq!(client.seal(&message, 0).unwrap().len(), 4);
    }

    #[test]
    fn flags_follow_the_spec() {
        let (client, mut peer) = transports();
        let mut sender = Session::new(client, Some(0));
        let records = sender.seal(&pattern(200_000), 0).unwrap();
        let flags: Vec<u8> = records
            .iter()
            .map(|record| peer.decrypt(record).unwrap()[0])
            .collect();
        assert_eq!(flags, vec![FLAG_CONTINUE, FLAG_CONTINUE, FLAG_CONTINUE, 0]);
    }

    #[test]
    fn messages_follow_each_other() {
        let (mut client, mut host) = sessions();
        for len in [0usize, 1, 300, 65_518, 65_519, 140_000] {
            let message = pattern(len);
            assert_eq!(deliver(&mut client, &mut host, &message, 0), vec![message]);
        }
    }

    #[test]
    fn rekey_by_count_keeps_both_sides_in_step() {
        let policy = RekeyPolicy {
            max_records: 3,
            max_age_ms: u64::MAX,
        };
        let (mut client, mut host) = sessions_with(policy);
        for i in 0u8..20 {
            let message = vec![i; 10 + usize::from(i)];
            assert_eq!(
                deliver(&mut client, &mut host, &message, 0),
                vec![message.clone()]
            );
            let back = vec![i; 5];
            assert_eq!(deliver(&mut host, &mut client, &back, 0), vec![back]);
        }
        // 20 records sent with a rekey every third one: 2 since the last.
        assert_eq!(client.rekey_state().sent_since, 2);
    }

    #[test]
    fn the_third_record_carries_rekey_and_the_key_changes_after_it() {
        let policy = RekeyPolicy {
            max_records: 3,
            max_age_ms: u64::MAX,
        };
        let (client, mut peer) = transports();
        let mut sender = Session::with_policy(client, policy, Some(0));

        let mut flags = Vec::new();
        for i in 0u8..7 {
            let records = sender.seal(&[i], 0).unwrap();
            let plaintext = peer.decrypt(&records[0]).unwrap();
            flags.push(plaintext[0]);
            if plaintext[0] & FLAG_REKEY != 0 {
                peer.rekey_incoming();
            }
        }
        assert_eq!(flags, vec![0, 0, FLAG_REKEY, 0, 0, FLAG_REKEY, 0]);
    }

    #[test]
    fn a_peer_that_ignores_rekey_loses_sync() {
        let policy = RekeyPolicy {
            max_records: 2,
            max_age_ms: u64::MAX,
        };
        let (client, mut peer) = transports();
        let mut sender = Session::with_policy(client, policy, Some(0));
        let first = sender.seal(&[1], 0).unwrap();
        let second = sender.seal(&[2], 0).unwrap(); // carries REKEY
        let third = sender.seal(&[3], 0).unwrap(); // already under the new key
        peer.decrypt(&first[0]).unwrap();
        peer.decrypt(&second[0]).unwrap();
        // The peer did not call rekey_incoming: the third record fails.
        assert_eq!(peer.decrypt(&third[0]), Err(HandshakeError::Decrypt));
    }

    #[test]
    fn rekey_by_age() {
        let policy = RekeyPolicy {
            max_records: u64::MAX,
            max_age_ms: 1_000,
        };
        let (mut client, mut host) = sessions_with(policy);
        assert_eq!(
            deliver(&mut client, &mut host, b"a", 0),
            vec![b"a".to_vec()]
        );
        assert_eq!(
            deliver(&mut client, &mut host, b"b", 999),
            vec![b"b".to_vec()]
        );
        assert_eq!(client.rekey_state().sent_since, 2);
        // Age reached: this record carries REKEY and the age restarts at 1000.
        assert_eq!(
            deliver(&mut client, &mut host, b"c", 1_000),
            vec![b"c".to_vec()]
        );
        assert_eq!(client.rekey_state().sent_since, 0);
        assert_eq!(client.rekey_state().since_ms, Some(1_000));
        assert_eq!(
            deliver(&mut client, &mut host, b"d", 1_500),
            vec![b"d".to_vec()]
        );
        assert_eq!(client.rekey_state().sent_since, 1);
    }

    #[test]
    fn a_reserved_bit_closes_the_session() {
        for flags in [0x04u8, 0x08, 0x80, 0xFC] {
            let (client, host) = transports();
            let mut raw_sender = client;
            let mut receiver = Session::new(host, Some(0));
            let record = raw_sender.encrypt(&[flags, 1, 2, 3]).unwrap();
            assert_eq!(receiver.open(&record, 0), Err(RecordError::ReservedFlags));
            assert!(receiver.is_failed());
        }
    }

    #[test]
    fn a_record_without_flags_byte_is_an_error() {
        let (mut raw_sender, host) = transports();
        let mut receiver = Session::new(host, Some(0));
        let record = raw_sender.encrypt(&[]).unwrap();
        assert_eq!(
            receiver.open(&record, 0),
            Err(RecordError::Transport(HandshakeError::Decrypt))
        );
        assert!(receiver.is_failed());
    }

    #[test]
    fn a_record_above_65_535_bytes_is_a_decrypt_error() {
        let (_, host) = transports();
        let mut receiver = Session::new(host, Some(0));
        assert_eq!(
            receiver.open(&vec![0u8; 65_536], 0),
            Err(RecordError::Transport(HandshakeError::Decrypt))
        );
        assert!(receiver.is_failed());
    }

    #[test]
    fn an_empty_message_is_a_one_byte_plaintext() {
        let (client, mut peer) = transports();
        let mut sender = Session::new(client, Some(0));
        let records = sender.seal(&[], 0).unwrap();
        assert_eq!(peer.decrypt(&records[0]).unwrap(), vec![0u8]);
    }

    #[test]
    fn without_a_handshake_time_the_age_counts_from_the_first_seal() {
        let policy = RekeyPolicy {
            max_records: u64::MAX,
            max_age_ms: 1_000,
        };
        let (client, host) = transports();
        let mut client = Session::with_policy(client, policy, None);
        let mut host = Session::with_policy(host, policy, None);
        assert_eq!(client.rekey_state().since_ms, None);
        // First seal far from zero: the age starts here, no rekey yet.
        assert_eq!(
            deliver(&mut client, &mut host, b"a", 5_000),
            vec![b"a".to_vec()]
        );
        assert_eq!(client.rekey_state().since_ms, Some(5_000));
        assert_eq!(client.rekey_state().sent_since, 1);
        assert_eq!(
            deliver(&mut client, &mut host, b"b", 5_999),
            vec![b"b".to_vec()]
        );
        assert_eq!(client.rekey_state().sent_since, 2);
        assert_eq!(
            deliver(&mut client, &mut host, b"c", 6_000),
            vec![b"c".to_vec()]
        );
        assert_eq!(client.rekey_state().sent_since, 0);
        assert_eq!(client.rekey_state().since_ms, Some(6_000));
    }

    #[test]
    fn reassembly_above_16_mib_is_an_error() {
        let (mut raw_sender, host) = transports();
        let mut receiver = Session::new(host, Some(0));
        let mut plaintext = vec![0u8; 1 + MAX_FRAGMENT];
        plaintext[0] = FLAG_CONTINUE;

        let mut total = 0usize;
        let outcome = loop {
            let record = raw_sender.encrypt(&plaintext).unwrap();
            total += MAX_FRAGMENT;
            match receiver.open(&record, 0) {
                Ok(None) => assert!(total <= MAX_MESSAGE),
                other => break other,
            }
        };
        assert!(total > MAX_MESSAGE);
        assert_eq!(outcome, Err(RecordError::MessageTooLong));
        assert!(receiver.is_failed());
    }

    #[test]
    fn a_message_of_exactly_16_mib_goes_through_and_one_more_byte_does_not() {
        let (mut client, mut host) = sessions();
        let message = vec![0x5A; MAX_MESSAGE];
        let delivered = deliver(&mut client, &mut host, &message, 0);
        assert_eq!(delivered.len(), 1);
        assert!(delivered[0] == message);

        let too_long = vec![0u8; MAX_MESSAGE + 1];
        assert_eq!(client.seal(&too_long, 0), Err(RecordError::MessageTooLong));
        assert!(client.is_failed());
    }

    #[test]
    fn decryption_failure_makes_the_session_unusable() {
        let (mut client, mut host) = sessions();
        let mut record = client.seal(b"hello", 0).unwrap().remove(0);
        record[0] ^= 0x01;
        assert!(matches!(
            host.open(&record, 0),
            Err(RecordError::Transport(HandshakeError::Decrypt))
        ));
        assert!(host.is_failed());

        // Neither opening nor sealing works any more.
        let good = client.seal(b"world", 0).unwrap().remove(0);
        assert_eq!(host.open(&good, 0), Err(RecordError::Failed));
        assert_eq!(host.seal(b"x", 0), Err(RecordError::Failed));
    }

    #[test]
    fn an_error_discards_the_partial_message() {
        let (mut client, mut host) = sessions();
        let records = client.seal(&pattern(70_000), 0).unwrap();
        assert_eq!(host.open(&records[0], 0).unwrap(), None);
        let mut broken = records[1].clone();
        broken[0] ^= 0x01;
        assert!(host.open(&broken, 0).is_err());
        assert_eq!(host.open(&records[1], 0), Err(RecordError::Failed));
    }

    #[test]
    fn remote_static_is_the_peer_key() {
        let client_keys = StaticKeypair::generate();
        let host_keys = StaticKeypair::generate();
        let (mut client, msg1) =
            ClientHandshake::connect(&client_keys, &host_keys.public(), HOST_ID).unwrap();
        let HostStep::Connect { msg2, transport } =
            HostHandshake::respond(HandshakeKind::Connect, &host_keys, HOST_ID, &msg1).unwrap()
        else {
            panic!("connect must yield HostStep::Connect");
        };
        let ClientStep::Established(client_transport) = client.read_message2(&msg2).unwrap() else {
            panic!("IK must be established after msg2");
        };
        let host_session = Session::new(transport, Some(0));
        let client_session = Session::new(client_transport, Some(0));
        assert_eq!(host_session.remote_static(), Some(client_keys.public()));
        assert_eq!(client_session.remote_static(), Some(host_keys.public()));
    }

    #[test]
    fn debug_shows_no_state() {
        let (client, _) = sessions();
        assert_eq!(format!("{client:?}"), "Session { failed: false, .. }");
    }
}
