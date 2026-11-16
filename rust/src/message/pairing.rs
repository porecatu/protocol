// SPDX-License-Identifier: GPL-3.0-or-later
//! Pairing: `pair-accepted`, `pair-rejected` (spec 03 §8) and the
//! `pair-request` payload of handshake message 3 (spec 02 §3).

use std::fmt;

use super::DecodeError;
use super::wire::{self, Body};
use crate::cbor::{self, MapReader, Value};
use crate::pairing_uri::DEVICE_NAME_MAX_BYTES;

/// `pair-accepted` (host to client): first application message of a pairing
/// session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairAccepted {
    /// `host_name`.
    pub host_name: String,
}

impl Body for PairAccepted {
    fn to_value(&self) -> Value {
        wire::map(vec![("host_name", wire::text(&self.host_name))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(PairAccepted {
            host_name: wire::req_text(r, "host_name")?,
        })
    }
}

code_enum! {
    /// Why a pairing was refused (`pair-rejected.reason`).
    PairRejectReason {
        /// The token does not exist, expired or was already used.
        TokenInvalid = 0,
        /// The user declined on the host.
        UserDeclined = 1,
        /// The confirmation timed out.
        Timeout = 2,
    }
}

/// `pair-rejected` (host to client): the host closes the channel afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairRejected {
    /// `reason`.
    pub reason: PairRejectReason,
}

impl Body for PairRejected {
    fn to_value(&self) -> Value {
        wire::map(vec![("reason", wire::code(self.reason))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(PairRejected {
            reason: wire::req_enum(r, "reason")?,
        })
    }
}

code_enum! {
    /// Platform of the paired device (`pair-request.platform`).
    PairPlatform {
        /// Android.
        Android = 0,
        /// iOS.
        Ios = 1,
        /// Anything else.
        Other = 2,
    }
}

/// Why a [`PairRequest`] cannot be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PairRequestError {
    /// The device name is empty or longer than 64 bytes.
    DeviceNameLength,
}

impl fmt::Display for PairRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairRequestError::DeviceNameLength => {
                f.write_str("device name must have 1 to 64 bytes")
            }
        }
    }
}

impl std::error::Error for PairRequestError {}

/// The `pair-request` carried, encrypted, in handshake message 3. It is not a
/// message: it has no type number and no envelope, just the CBOR map.
#[derive(Clone, PartialEq, Eq)]
pub struct PairRequest {
    /// `token`: the pairing token, 16 bytes. A secret.
    pub token: [u8; 16],
    /// `device`: name of the device, 1 to 64 bytes.
    pub device: String,
    /// `platform`.
    pub platform: PairPlatform,
}

impl fmt::Debug for PairRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairRequest")
            .field("device", &self.device)
            .field("platform", &self.platform)
            .finish_non_exhaustive()
    }
}

impl PairRequest {
    /// Encodes the request as a deterministic CBOR map.
    pub fn encode(&self) -> Result<Vec<u8>, PairRequestError> {
        if !(1..=DEVICE_NAME_MAX_BYTES).contains(&self.device.len()) {
            return Err(PairRequestError::DeviceNameLength);
        }
        Ok(cbor::encode(&wire::map(vec![
            ("token", Value::Bytes(self.token.to_vec())),
            ("device", wire::text(&self.device)),
            ("platform", wire::code(self.platform)),
        ])))
    }

    /// Decodes the request strictly; unknown keys are ignored.
    pub fn decode(bytes: &[u8]) -> Result<PairRequest, DecodeError> {
        let value = cbor::decode(bytes)?;
        let reader = MapReader::from_value(&value)?;
        let device = wire::req_text(&reader, "device")?;
        if !(1..=DEVICE_NAME_MAX_BYTES).contains(&device.len()) {
            return Err(DecodeError::InvalidValue("device"));
        }
        Ok(PairRequest {
            token: wire::req_bytes::<16>(&reader, "token")?,
            device,
            platform: wire::req_enum(&reader, "platform")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PairRequest {
        PairRequest {
            token: [7; 16],
            device: "Pixel".to_owned(),
            platform: PairPlatform::Android,
        }
    }

    // a3 map(3); keys by encoded bytes: "token" (65..), "device" (66..),
    // "platform" (68..).
    #[test]
    fn pair_request_round_trip() {
        let bytes = request().encode().unwrap();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            concat!(
                "a3",
                "65746f6b656e",                       // "token"
                "5007070707070707070707070707070707", // bstr(16) of 0x07
                "66646576696365",                     // "device"
                "65506978656c",                       // "Pixel"
                "68706c6174666f726d",                 // "platform"
                "00",                                 // android
            )
        );
        assert_eq!(PairRequest::decode(&bytes).unwrap(), request());
    }

    #[test]
    fn device_name_length_is_checked() {
        let mut r = request();
        r.device = String::new();
        assert_eq!(r.encode(), Err(PairRequestError::DeviceNameLength));
        r.device = "x".repeat(65);
        assert_eq!(r.encode(), Err(PairRequestError::DeviceNameLength));
        r.device = "x".repeat(64);
        let bytes = r.encode().unwrap();
        assert_eq!(PairRequest::decode(&bytes).unwrap(), r);
    }

    #[test]
    fn decode_refuses_bad_values() {
        // Token with 15 bytes.
        let bytes = cbor::encode(&wire::map(vec![
            ("token", Value::Bytes(vec![1; 15])),
            ("device", wire::text("a")),
            ("platform", wire::uint(0)),
        ]));
        assert_eq!(
            PairRequest::decode(&bytes),
            Err(DecodeError::InvalidValue("token"))
        );
        // Platform 3.
        let bytes = cbor::encode(&wire::map(vec![
            ("token", Value::Bytes(vec![1; 16])),
            ("device", wire::text("a")),
            ("platform", wire::uint(3)),
        ]));
        assert_eq!(
            PairRequest::decode(&bytes),
            Err(DecodeError::InvalidValue("platform"))
        );
        // Missing device.
        let bytes = cbor::encode(&wire::map(vec![
            ("token", Value::Bytes(vec![1; 16])),
            ("platform", wire::uint(0)),
        ]));
        assert!(PairRequest::decode(&bytes).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    // Hand-computed: 82 = array of 2; 18 3c = 60; a1 = map of 1; 69 =
    // text(9) "host_name"; 62 = text(2) "pc".
    #[test]
    fn pair_accepted_round_trip() {
        use super::super::{Direction, Message, decode, encode};
        let message = Message::PairAccepted(PairAccepted {
            host_name: "pc".to_owned(),
        });
        let bytes = encode(&message);
        assert_eq!(
            hex(&bytes),
            concat!("82183ca1", "69", "686f73745f6e616d65", "62", "7063")
        );
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
        // An empty name is still a valid text.
        let empty = Message::PairAccepted(PairAccepted {
            host_name: String::new(),
        });
        assert_eq!(
            decode(&encode(&empty), Direction::HostToClient).unwrap(),
            empty
        );
        // Host message only.
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(60))
        );
    }

    #[test]
    fn pair_accepted_errors() {
        use super::super::{Direction, decode};
        let message = |entries: Vec<(&str, Value)>| {
            cbor::encode(&Value::Array(vec![wire::uint(60), wire::map(entries)]))
        };
        assert_eq!(
            decode(&message(vec![]), Direction::HostToClient),
            Err(DecodeError::Cbor(cbor::CborError::MissingKey))
        );
        assert_eq!(
            decode(
                &message(vec![("host_name", wire::uint(1))]),
                Direction::HostToClient
            ),
            Err(DecodeError::Cbor(cbor::CborError::WrongType))
        );
        // Unknown key ignored.
        assert!(
            decode(
                &message(vec![
                    ("host_name", wire::text("pc")),
                    ("zzz", Value::Bool(true))
                ]),
                Direction::HostToClient
            )
            .is_ok()
        );
    }

    // Hand-computed: 18 3d = 61; 66 = text(6) "reason"; 01 = user-declined.
    #[test]
    fn pair_rejected_round_trip() {
        use super::super::{Direction, Message, decode, encode};
        let message = Message::PairRejected(PairRejected {
            reason: PairRejectReason::UserDeclined,
        });
        let bytes = encode(&message);
        assert_eq!(hex(&bytes), concat!("82183da1", "66726561736f6e", "01"));
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
        for reason in PairRejectReason::ALL {
            let message = Message::PairRejected(PairRejected { reason: *reason });
            assert_eq!(
                decode(&encode(&message), Direction::HostToClient).unwrap(),
                message
            );
        }
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(61))
        );
    }

    #[test]
    fn pair_rejected_errors() {
        use super::super::{Direction, decode};
        let message = |entries: Vec<(&str, Value)>| {
            cbor::encode(&Value::Array(vec![wire::uint(61), wire::map(entries)]))
        };
        assert_eq!(
            decode(&message(vec![]), Direction::HostToClient),
            Err(DecodeError::Cbor(cbor::CborError::MissingKey))
        );
        // Reason 3 is outside the domain.
        assert_eq!(
            decode(
                &message(vec![("reason", wire::uint(3))]),
                Direction::HostToClient
            ),
            Err(DecodeError::InvalidValue("reason"))
        );
        // Unknown key ignored.
        assert!(
            decode(
                &message(vec![("reason", wire::uint(2)), ("zzz", Value::Null)]),
                Direction::HostToClient
            )
            .is_ok()
        );
    }

    #[test]
    fn debug_hides_the_token() {
        let text = format!("{:?}", request());
        assert!(!text.contains("token"));
        assert!(!text.contains("[7"));
    }
}
