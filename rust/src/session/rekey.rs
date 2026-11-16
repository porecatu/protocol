// SPDX-License-Identifier: GPL-3.0-or-later

//! The rekey schedule of the sending direction (spec 02 §5).
//!
//! Each side **must** mark `REKEY` on a record and change its sending key at
//! least every 100 000 records sent or every 60 minutes, whichever comes
//! first. This module is pure: the clock is a monotonic millisecond number
//! received from outside.

/// Default `max_records` (spec 02 §5).
pub const DEFAULT_MAX_RECORDS: u64 = 100_000;

/// Default `max_age_ms`: 60 minutes (spec 02 §5).
pub const DEFAULT_MAX_AGE_MS: u64 = 3_600_000;

/// When the sending key must be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RekeyPolicy {
    /// A key encrypts at most this many records.
    pub max_records: u64,
    /// A key lives at most this many milliseconds.
    pub max_age_ms: u64,
}

impl Default for RekeyPolicy {
    fn default() -> Self {
        RekeyPolicy {
            max_records: DEFAULT_MAX_RECORDS,
            max_age_ms: DEFAULT_MAX_AGE_MS,
        }
    }
}

/// What the sending side remembers about its current key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RekeyState {
    /// Records already sent under the current key.
    pub sent_since: u64,
    /// Monotonic time (ms) when the current key started to be used: the end
    /// of the handshake, or the last rekey. `None` until the first record is
    /// sent when the session was created without a handshake time.
    pub since_ms: Option<u64>,
}

impl RekeyState {
    /// The state of a fresh key, in use from `now_ms`.
    #[must_use]
    pub fn new(now_ms: u64) -> RekeyState {
        RekeyState {
            sent_since: 0,
            since_ms: Some(now_ms),
        }
    }

    /// The state of a fresh key whose start is not known yet: it is taken
    /// from the first record sent (see [`RekeyState::start_if_unknown`]).
    #[must_use]
    pub fn unstarted() -> RekeyState {
        RekeyState {
            sent_since: 0,
            since_ms: None,
        }
    }

    /// Sets the start of the key to `now_ms` when it is still unknown.
    pub fn start_if_unknown(&mut self, now_ms: u64) {
        self.since_ms.get_or_insert(now_ms);
    }

    /// Accounts for one record sent under the current key.
    pub fn record_sent(&mut self) {
        self.sent_since = self.sent_since.saturating_add(1);
    }

    /// Accounts for a rekey that happened at `now_ms`.
    pub fn rekeyed(&mut self, now_ms: u64) {
        *self = RekeyState::new(now_ms);
    }
}

impl RekeyPolicy {
    /// Whether the record **about to be sent** must carry `REKEY`.
    ///
    /// `state.sent_since` counts the records already sent under the current
    /// key, so the record about to go is number `sent_since + 1`: it carries
    /// `REKEY` when it is the last one the key may encrypt, or when the key
    /// has reached its age. A clock that goes backwards counts as age zero.
    #[must_use]
    pub fn should_rekey(&self, state: &RekeyState, now_ms: u64) -> bool {
        let records = state.sent_since.saturating_add(1);
        let age = state
            .since_ms
            .map_or(0, |since| now_ms.saturating_sub(since));
        records >= self.max_records || age >= self.max_age_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_spec_values() {
        let policy = RekeyPolicy::default();
        assert_eq!(policy.max_records, 100_000);
        assert_eq!(policy.max_age_ms, 3_600_000);
    }

    #[test]
    fn the_record_number_max_records_carries_rekey() {
        let policy = RekeyPolicy {
            max_records: 3,
            max_age_ms: u64::MAX,
        };
        let mut state = RekeyState::new(0);
        assert!(!policy.should_rekey(&state, 0)); // record 1
        state.record_sent();
        assert!(!policy.should_rekey(&state, 0)); // record 2
        state.record_sent();
        assert!(policy.should_rekey(&state, 0)); // record 3
        state.rekeyed(0);
        assert!(!policy.should_rekey(&state, 0));
    }

    #[test]
    fn age_counts_from_the_start_of_the_key() {
        let policy = RekeyPolicy {
            max_records: u64::MAX,
            max_age_ms: 1_000,
        };
        let state = RekeyState::new(500);
        assert!(!policy.should_rekey(&state, 1_499));
        assert!(policy.should_rekey(&state, 1_500));
    }

    #[test]
    fn a_clock_going_backwards_is_not_age() {
        let policy = RekeyPolicy {
            max_records: u64::MAX,
            max_age_ms: 1,
        };
        let state = RekeyState::new(10_000);
        assert!(!policy.should_rekey(&state, 5));
    }
}
