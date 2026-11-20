// SPDX-License-Identifier: GPL-3.0-or-later
//! Events and errors: `bell` and `error` (spec 03 §9).

use super::wire::{self, Body};
use super::{DecodeError, PaneId, RequestId};
use crate::cbor::{MapReader, Value};

code_enum! {
    /// Why a request failed (`error-code`). Number 2 is reserved: the
    /// revocation of a live session is `goodbye { revoked }`.
    ErrorCode {
        /// Invalid CBOR, missing field, unknown type, value out of domain.
        Protocol = 0,
        /// The peer is not a paired device.
        NotPaired = 1,
        /// No such window.
        UnknownWindow = 3,
        /// No such group.
        UnknownGroup = 4,
        /// No such tab.
        UnknownTab = 5,
        /// No such pane.
        UnknownPane = 6,
        /// No such profile.
        UnknownProfile = 7,
        /// The pane is not in the `running` state.
        PaneNotRunning = 8,
        /// The pane is not subscribed.
        NotSubscribed = 9,
        /// The client is at `max_subscriptions`.
        TooManySubscriptions = 10,
        /// An argument is invalid.
        InvalidArgument = 11,
        /// The desktop is in a state that prevents the action.
        Busy = 12,
        /// The confirmation was not answered in time.
        ConfirmTimeout = 13,
        /// The user refused the confirmation.
        Cancelled = 14,
        /// The host does not support the request.
        Unsupported = 15,
        /// The action would close the last tab of the last window.
        LastTab = 16,
        /// The saved session does not exist (any more) on the host.
        UnknownSession = 17,
        /// The host cannot read the saved session to restore it.
        SessionUnreadable = 18,
    }
}

/// `bell` (host to client): a pane rang the bell. Transient; the indicator
/// stays in the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bell {
    /// `pane`.
    pub pane: PaneId,
}

impl Body for Bell {
    fn to_value(&self) -> Value {
        wire::map(vec![("pane", wire::uint(self.pane))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Bell {
            pane: wire::req_u64(r, "pane")?,
        })
    }
}

/// `error` (both directions). Named `ErrorMessage` to stay clear of
/// `std::error::Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorMessage {
    /// `code`.
    pub code: ErrorCode,
    /// `? "id"`: when answering a request that has an id
    /// (`scrollback-request`).
    pub id: Option<RequestId>,
    /// `? "pane"`: when answering `subscribe`, `unsubscribe`, `grid-ack`,
    /// `key`, `text-input` or `paste`.
    pub pane: Option<PaneId>,
}

impl Body for ErrorMessage {
    fn to_value(&self) -> Value {
        let mut entries = vec![("code", wire::code(self.code))];
        if let Some(id) = self.id {
            entries.push(("id", wire::uint(id)));
        }
        if let Some(pane) = self.pane {
            entries.push(("pane", wire::uint(pane)));
        }
        wire::map(entries)
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(ErrorMessage {
            code: wire::req_enum(r, "code")?,
            id: wire::opt_u64(r, "id")?,
            pane: wire::opt_u64(r, "pane")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Direction, Message, decode, encode};
    use super::*;
    use crate::cbor::{self, CborError};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    // Hand-computed: 82 = array of 2; 18 32 = 50 (above 23: one extra byte);
    // a1 = map of 1; 64 "pane"; 07.
    #[test]
    fn bell_round_trip() {
        let message = Message::Bell(Bell { pane: 7 });
        let bytes = encode(&message);
        assert_eq!(hex(&bytes), concat!("821832a1", "6470616e65", "07"));
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
        // The largest id.
        let big = Message::Bell(Bell {
            pane: 9_007_199_254_740_991,
        });
        assert_eq!(decode(&encode(&big), Direction::HostToClient).unwrap(), big);
    }

    #[test]
    fn bell_is_a_host_message() {
        let bytes = encode(&Message::Bell(Bell { pane: 7 }));
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(50))
        );
    }

    #[test]
    fn bell_errors() {
        // Missing "pane".
        let bytes = cbor::encode(&Value::Array(vec![wire::uint(50), wire::map(vec![])]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // "pane" with the wrong type.
        let bytes = cbor::encode(&Value::Array(vec![
            wire::uint(50),
            wire::map(vec![("pane", wire::text("7"))]),
        ]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // A negative id is out of the domain.
        let bytes = cbor::encode(&Value::Array(vec![
            wire::uint(50),
            wire::map(vec![("pane", wire::int(-1))]),
        ]));
        assert!(decode(&bytes, Direction::HostToClient).is_err());
        // Unknown key ignored.
        let bytes = cbor::encode(&Value::Array(vec![
            wire::uint(50),
            wire::map(vec![("pane", wire::uint(7)), ("zzz", Value::Null)]),
        ]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).unwrap(),
            Message::Bell(Bell { pane: 7 })
        );
    }
}
