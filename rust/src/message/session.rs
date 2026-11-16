// SPDX-License-Identifier: GPL-3.0-or-later
//! Start and end of a session: `hello`, `welcome`, `incompatible`, `goodbye`
//! (spec 03 §2).

use super::DecodeError;
use super::wire::{self, Body};
use crate::cbor::{MapReader, Value, expect_array};

/// A protocol version, `[major, minor]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    /// Major version.
    pub major: u64,
    /// Minor version.
    pub minor: u64,
}

impl Version {
    /// The positional array `[major, minor]`.
    pub fn to_value(&self) -> Value {
        Value::Array(vec![wire::uint(self.major), wire::uint(self.minor)])
    }

    /// Reads `[major, minor]`, requiring exactly two elements.
    pub fn from_value(value: &Value) -> Result<Self, DecodeError> {
        let items = expect_array(value, 2)?;
        Ok(Version {
            major: items[0].as_u64()?,
            minor: items[1].as_u64()?,
        })
    }
}

fn req_version(r: &MapReader<'_>, key: &str) -> Result<Version, DecodeError> {
    Version::from_value(r.required(key)?)
}

/// `hello` (client to host): the first message of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// `version`: what the client speaks.
    pub version: Version,
    /// `app`: for example `"porecatu-remote"`.
    pub app: String,
    /// `app_version`: semver of the app.
    pub app_version: String,
}

impl Body for Hello {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("version", self.version.to_value()),
            ("app", wire::text(&self.app)),
            ("app_version", wire::text(&self.app_version)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Hello {
            version: req_version(r, "version")?,
            app: wire::req_text(r, "app")?,
            app_version: wire::req_text(r, "app_version")?,
        })
    }
}

/// `welcome` (host to client): the host accepted the version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    /// `version`: major equal, minor the smaller of the two.
    pub version: Version,
    /// `host_name`: name of the host for display.
    pub host_name: String,
    /// `porecatu_version`: semver of the Porecatu.
    pub porecatu_version: String,
    /// `max_subscriptions`: at least 1 (decoding refuses 0); the current host
    /// uses 4.
    pub max_subscriptions: u64,
    /// `host_instance`: random, new for every host process; ids are only
    /// comparable between sessions with the same value.
    pub host_instance: [u8; 16],
}

impl Body for Welcome {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("version", self.version.to_value()),
            ("host_name", wire::text(&self.host_name)),
            ("porecatu_version", wire::text(&self.porecatu_version)),
            ("max_subscriptions", wire::uint(self.max_subscriptions)),
            ("host_instance", Value::Bytes(self.host_instance.to_vec())),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        let max_subscriptions = wire::req_u64(r, "max_subscriptions")?;
        if max_subscriptions == 0 {
            return Err(DecodeError::InvalidValue("max_subscriptions"));
        }
        Ok(Welcome {
            version: req_version(r, "version")?,
            host_name: wire::req_text(r, "host_name")?,
            porecatu_version: wire::req_text(r, "porecatu_version")?,
            max_subscriptions,
            host_instance: wire::req_bytes::<16>(r, "host_instance")?,
        })
    }
}

/// `incompatible` (host to client): the versions do not match; the host
/// closes the channel afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Incompatible {
    /// `version`: what the host speaks.
    pub version: Version,
}

impl Body for Incompatible {
    fn to_value(&self) -> Value {
        wire::map(vec![("version", self.version.to_value())])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Incompatible {
            version: req_version(r, "version")?,
        })
    }
}

code_enum! {
    /// Why a side ends the session (`goodbye-reason`).
    GoodbyeReason {
        /// Ordinary end.
        Normal = 0,
        /// The user turned remote access off on the host.
        RemoteDisabled = 1,
        /// This device was removed from the list of paired devices.
        Revoked = 2,
        /// The Porecatu is closing.
        HostQuitting = 3,
    }
}

/// `goodbye` (both directions): sent before closing the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goodbye {
    /// `reason`.
    pub reason: GoodbyeReason,
}

impl Body for Goodbye {
    fn to_value(&self) -> Value {
        wire::map(vec![("reason", wire::code(self.reason))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Goodbye {
            reason: wire::req_enum(r, "reason")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Direction, Message, decode, encode};
    use super::*;

    fn welcome(max_subscriptions: u64) -> Welcome {
        Welcome {
            version: Version { major: 0, minor: 2 },
            host_name: "desk".to_owned(),
            porecatu_version: "0.9.11".to_owned(),
            max_subscriptions,
            host_instance: [0xab; 16],
        }
    }

    #[test]
    fn welcome_refuses_zero_max_subscriptions() {
        let bytes = encode(&Message::Welcome(welcome(0)));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::InvalidValue("max_subscriptions"))
        );
        let bytes = encode(&Message::Welcome(welcome(1)));
        assert!(decode(&bytes, Direction::HostToClient).is_ok());
    }
}
