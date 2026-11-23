// SPDX-License-Identifier: GPL-3.0-or-later
//! Application messages encoded as CBOR (`message` feature).
//!
//! Specification 03 in full: the 29 messages, their components and the enums
//! of the domain, plus [`encode`] and [`decode`].
//!
//! # Wire shape
//!
//! Every message is the CBOR array `[type, body]` where `body` is a map with
//! text keys. [`encode`] builds a [`Value`] tree and lets the deterministic
//! encoder of [`crate::cbor`] sort the keys. [`decode`] goes through the
//! strict decoder of [`crate::cbor`], which already rejects every
//! non-canonical form (spec 03 §1.1), so a body parser only deals with
//! well-formed trees.
//!
//! # The pattern each message body follows
//!
//! Each message struct implements the crate-private `wire::Body` trait:
//!
//! ```ignore
//! impl Body for Welcome {
//!     fn to_value(&self) -> Value { /* the body map, keys exactly as in the spec */ }
//!     fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> { /* ... */ }
//! }
//! ```
//!
//! Rules for the implementations:
//!
//! - **Key names are the ones of the specification, verbatim** (`host_name`,
//!   `max_subscriptions`, ...). The Rust field is the same name; a spec key
//!   that is a Rust keyword (`"do"`) gets a descriptive field name and the
//!   mapping is commented at the key.
//! - **Optional keys (`? "id"`) are `Option<T>`**: `None` is encoded by
//!   *omitting* the key, never as `null`; a present `null` is an error. A key
//!   that the spec types `T / null` is also `Option<T>`, but there `None` is
//!   encoded as `null` and the key is mandatory.
//! - **Unknown keys are ignored** (spec 03 §1): `from_map` reads the keys it
//!   knows through [`MapReader`](crate::cbor::MapReader) and never iterates
//!   the map. A required key that is absent is
//!   [`CborError::MissingKey`], a value of the wrong CBOR type is
//!   [`CborError::WrongType`]; both surface as [`DecodeError::Cbor`].
//! - **A value outside its domain** (unknown enum code, byte string of the
//!   wrong size, `ratio` of 0) is [`DecodeError::InvalidValue`] with the name
//!   of the key.
//! - Positional arrays (`version`, `span`, `style`, `line`, `color`) are
//!   checked with [`expect_array`], which enforces the exact arity.
//! - Integers given to `wire::uint` must be within `0 ..= 2^53 - 1` (spec 03
//!   §1.1); anything larger is a caller bug (debug assertion; saturates in
//!   release builds).
//! - Use the helpers of the private `wire` module; do not repeat their logic.
//! - Nested non-message types (`Window`, `Tab`, `Span`, ...) get
//!   `to_value`/`from_value` inherent methods next to their definition.
//! - Test each body with a hand-computed hex, a round trip, a missing
//!   required key and an unknown extra key.
//!
//! Every body of the 29 messages (and `PairRequest`) is implemented; [`decode`]
//! never panics on untrusted bytes.
//!
//! # Errors
//!
//! [`DecodeError`] always means a protocol error: the receiver answers
//! `error { code: protocol }` and closes the session
//! ([`DecodeError::error_code`]).

use crate::cbor::{self, CborError, Value, expect_array};

/// Declares an enum of the domain with wire numbers.
///
/// Generates a `#[non_exhaustive]` enum, `ALL`, `code()` and `from_code()`.
macro_rules! code_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $code:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Every value of the enum, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// The number this value has on the wire.
            pub const fn code(self) -> u64 {
                match self {
                    $( $name::$variant => $code ),+
                }
            }

            /// The value for a wire number, or `None` outside the domain.
            pub const fn from_code(code: u64) -> Option<$name> {
                match code {
                    $( $code => Some($name::$variant), )+
                    _ => None,
                }
            }
        }

        impl $crate::message::wire::CodeEnum for $name {
            fn code(self) -> u64 {
                $name::code(self)
            }

            fn from_code(code: u64) -> Option<$name> {
                $name::from_code(code)
            }
        }
    };
}

pub mod action;
pub mod event;
pub mod grid;
pub mod input;
pub mod pairing;
pub mod session;
pub mod workspace;

pub use action::{
    Action, ActionBody, ActionResult, Confirm, ConfirmKind, ConfirmRequest, NewTabGroup,
};
pub use event::{Bell, ErrorCode, ErrorMessage};
pub use grid::{
    Color, Cursor, CursorShape, GridAck, GridDiff, GridSnapshot, Line, ScrollbackPage,
    ScrollbackRequest, Span, SpanText, Style, Subscribe, SubscriptionEndReason, SubscriptionEnded,
    Unsubscribe,
};
pub use input::{Key, KeyCode, NamedKey, Paste, TextInput};
pub use pairing::{
    PairAccepted, PairPlatform, PairRejectReason, PairRejected, PairRequest, PairRequestError,
};
pub use session::{Goodbye, GoodbyeReason, Hello, Incompatible, Version, Welcome};
pub use workspace::{
    Group, Item, ListSessions, NamedSession, Pane, PaneNode, PaneState, Profile, ProfileKind,
    Profiles, SessionId, Sessions, SessionsMode, Split, SplitDir, Tab, Theme, ViewTab, Window,
    Workspace,
};

/// Identifier of a window (spec 03 §3).
pub type WindowId = u64;
/// Identifier of a group.
pub type GroupId = u64;
/// Identifier of a tab.
pub type TabId = u64;
/// Identifier of a pane.
pub type PaneId = u64;
/// Identifier of a profile (the text identifier of the Porecatu profile).
pub type ProfileId = String;
/// Identifier of a request, chosen by the client.
pub type RequestId = u64;

/// Who sent a message: the host (the Porecatu desktop) or the client (the
/// phone app).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Host to client.
    HostToClient,
    /// Client to host.
    ClientToHost,
}

/// Which directions a message type may travel in (spec 03 §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Senders {
    Host,
    Client,
    Both,
}

impl Senders {
    fn allows(self, from: Direction) -> bool {
        matches!(
            (self, from),
            (Senders::Both, _)
                | (Senders::Host, Direction::HostToClient)
                | (Senders::Client, Direction::ClientToHost)
        )
    }
}

/// Why a message was refused. Always a protocol error (spec 03 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// The CBOR is invalid or not canonical, a value has the wrong CBOR
    /// type, a required key is missing, or an array has the wrong arity.
    Cbor(CborError),
    /// The message type is not in the table of spec 03 §10.
    UnknownType(u64),
    /// A known message type arrived from the side that must not send it. The
    /// payload is the message type.
    WrongDirection(u64),
    /// A value is outside its domain. The payload is the name of the key.
    InvalidValue(&'static str),
}

impl DecodeError {
    /// The error code to answer with: always [`ErrorCode::Protocol`].
    pub fn error_code(&self) -> ErrorCode {
        ErrorCode::Protocol
    }
}

impl From<CborError> for DecodeError {
    fn from(error: CborError) -> Self {
        DecodeError::Cbor(error)
    }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Cbor(e) => write!(f, "invalid message encoding: {e}"),
            DecodeError::UnknownType(code) => write!(f, "unknown message type {code}"),
            DecodeError::WrongDirection(code) => {
                write!(f, "message type {code} sent in the wrong direction")
            }
            DecodeError::InvalidValue(key) => write!(f, "value out of domain in `{key}`"),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeError::Cbor(e) => Some(e),
            _ => None,
        }
    }
}

/// Helpers shared by every message body (see the module docs).
pub(crate) mod wire {
    // The helpers are used as the bodies of the following steps land.
    #![allow(dead_code)]

    use super::DecodeError;
    use crate::cbor::{MAX_SAFE_INT, MapReader, Value};

    /// The codec of one message body.
    pub(crate) trait Body: Sized {
        /// The body as a CBOR map.
        fn to_value(&self) -> Value;
        /// Reads the body from its map, ignoring unknown keys.
        fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError>;
    }

    /// An enum of the domain with wire numbers (see `code_enum!`).
    pub(crate) trait CodeEnum: Sized + Copy {
        fn code(self) -> u64;
        fn from_code(code: u64) -> Option<Self>;
    }

    /// An unsigned integer value.
    pub(crate) fn uint(value: u64) -> Value {
        debug_assert!(value <= MAX_SAFE_INT as u64, "integer above 2^53 - 1");
        Value::Int(value.min(MAX_SAFE_INT as u64) as i64)
    }

    /// A signed integer value.
    pub(crate) fn int(value: i64) -> Value {
        debug_assert!((-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&value));
        Value::Int(value.clamp(-MAX_SAFE_INT, MAX_SAFE_INT))
    }

    /// A text value.
    pub(crate) fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    /// A map value from `(key, value)` pairs (the encoder sorts the keys).
    pub(crate) fn map(entries: Vec<(&str, Value)>) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        )
    }

    /// An enum as its wire number.
    pub(crate) fn code<E: CodeEnum>(value: E) -> Value {
        uint(value.code())
    }

    /// `text` or `null`.
    pub(crate) fn nullable_text(value: &Option<String>) -> Value {
        match value {
            Some(s) => text(s),
            None => Value::Null,
        }
    }

    /// `uint` or `null`.
    pub(crate) fn nullable_uint(value: Option<u64>) -> Value {
        match value {
            Some(v) => uint(v),
            None => Value::Null,
        }
    }

    /// Required unsigned integer.
    pub(crate) fn req_u64(r: &MapReader<'_>, key: &str) -> Result<u64, DecodeError> {
        Ok(r.required(key)?.as_u64()?)
    }

    /// Required signed integer.
    pub(crate) fn req_i64(r: &MapReader<'_>, key: &str) -> Result<i64, DecodeError> {
        Ok(r.required(key)?.as_i64()?)
    }

    /// Required text.
    pub(crate) fn req_text(r: &MapReader<'_>, key: &str) -> Result<String, DecodeError> {
        Ok(r.required(key)?.as_text()?.to_owned())
    }

    /// Required boolean.
    pub(crate) fn req_bool(r: &MapReader<'_>, key: &str) -> Result<bool, DecodeError> {
        Ok(r.required(key)?.as_bool()?)
    }

    /// Required byte string of exactly `N` bytes.
    pub(crate) fn req_bytes<const N: usize>(
        r: &MapReader<'_>,
        key: &'static str,
    ) -> Result<[u8; N], DecodeError> {
        r.required(key)?
            .as_bytes()?
            .try_into()
            .map_err(|_| DecodeError::InvalidValue(key))
    }

    /// Required enum, by its wire number.
    pub(crate) fn req_enum<E: CodeEnum>(
        r: &MapReader<'_>,
        key: &'static str,
    ) -> Result<E, DecodeError> {
        enum_from(r.required(key)?, key)
    }

    /// An enum from an already located value.
    pub(crate) fn enum_from<E: CodeEnum>(
        value: &Value,
        key: &'static str,
    ) -> Result<E, DecodeError> {
        E::from_code(value.as_u64()?).ok_or(DecodeError::InvalidValue(key))
    }

    /// Optional (`? "key"`) unsigned integer: absent is `None`, `null` is an
    /// error.
    pub(crate) fn opt_u64(r: &MapReader<'_>, key: &str) -> Result<Option<u64>, DecodeError> {
        r.optional(key).map(|v| Ok(v.as_u64()?)).transpose()
    }

    /// `text / null`, key mandatory.
    pub(crate) fn req_nullable_text(
        r: &MapReader<'_>,
        key: &str,
    ) -> Result<Option<String>, DecodeError> {
        let value = r.required(key)?;
        if value.is_null() {
            Ok(None)
        } else {
            Ok(Some(value.as_text()?.to_owned()))
        }
    }

    /// `uint / null`, key mandatory.
    pub(crate) fn req_nullable_u64(
        r: &MapReader<'_>,
        key: &str,
    ) -> Result<Option<u64>, DecodeError> {
        let value = r.required(key)?;
        if value.is_null() {
            Ok(None)
        } else {
            Ok(Some(value.as_u64()?))
        }
    }
}

/// Declares [`Message`] and its dispatch from one table (spec 03 §10).
macro_rules! message_table {
    ( $( $code:literal $variant:ident($ty:ty) $senders:ident ),+ $(,)? ) => {
        /// An application message: one variant per row of the table of spec
        /// 03 §10.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum Message {
            $(
                #[doc = concat!("Message type ", stringify!($code), ".")]
                $variant($ty)
            ),+
        }

        impl Message {
            /// The message type number on the wire.
            pub fn type_code(&self) -> u64 {
                match self {
                    $( Message::$variant(_) => $code ),+
                }
            }

            fn senders(&self) -> Senders {
                match self {
                    $( Message::$variant(_) => Senders::$senders ),+
                }
            }

            fn body(&self) -> Value {
                match self {
                    $( Message::$variant(body) => wire::Body::to_value(body) ),+
                }
            }
        }

        fn decode_body(
            code: u64,
            from: Direction,
            reader: &cbor::MapReader<'_>,
        ) -> Result<Message, DecodeError> {
            match code {
                $(
                    $code => {
                        if !Senders::$senders.allows(from) {
                            return Err(DecodeError::WrongDirection(code));
                        }
                        Ok(Message::$variant(<$ty as wire::Body>::from_map(reader)?))
                    }
                )+
                _ => Err(DecodeError::UnknownType(code)),
            }
        }

        /// Every message type number, in table order.
        pub const TYPE_CODES: &[u64] = &[ $( $code ),+ ];
    };
}

message_table! {
    0  Hello(Hello) Client,
    1  Welcome(Welcome) Host,
    2  Incompatible(Incompatible) Host,
    3  Goodbye(Goodbye) Both,
    10 Workspace(Workspace) Host,
    11 Profiles(Profiles) Host,
    12 Theme(Theme) Host,
    13 ViewTab(ViewTab) Client,
    14 ListSessions(ListSessions) Client,
    15 Sessions(Sessions) Host,
    20 Subscribe(Subscribe) Client,
    21 Unsubscribe(Unsubscribe) Client,
    22 GridSnapshot(GridSnapshot) Host,
    23 GridDiff(GridDiff) Host,
    24 GridAck(GridAck) Client,
    25 ScrollbackRequest(ScrollbackRequest) Client,
    26 ScrollbackPage(ScrollbackPage) Host,
    27 SubscriptionEnded(SubscriptionEnded) Host,
    30 Key(Key) Client,
    31 TextInput(TextInput) Client,
    32 Paste(Paste) Client,
    40 Action(Action) Client,
    41 ActionResult(ActionResult) Host,
    42 ConfirmRequest(ConfirmRequest) Host,
    43 Confirm(Confirm) Client,
    50 Bell(Bell) Host,
    60 PairAccepted(PairAccepted) Host,
    61 PairRejected(PairRejected) Host,
    70 Error(ErrorMessage) Both,
}

impl Message {
    /// Who sends this message: `Some(direction)` for a message that travels
    /// one way only, `None` for `goodbye` and `error`, which both sides send.
    pub fn sender(&self) -> Option<Direction> {
        match self.senders() {
            Senders::Host => Some(Direction::HostToClient),
            Senders::Client => Some(Direction::ClientToHost),
            Senders::Both => None,
        }
    }

    /// Whether the side `from` is allowed to send this message.
    pub fn can_be_sent_by(&self, from: Direction) -> bool {
        self.senders().allows(from)
    }
}

/// Encodes a message deterministically (spec 03 §1.1): `[type, body]`.
pub fn encode(message: &Message) -> Vec<u8> {
    let value = Value::Array(vec![wire::uint(message.type_code()), message.body()]);
    cbor::encode(&value)
}

/// Decodes a message that arrived from `from`.
///
/// An unknown message type, or a known one sent from the wrong side, is an
/// error, as is anything that spec 03 §1.1 says to refuse.
pub fn decode(bytes: &[u8], from: Direction) -> Result<Message, DecodeError> {
    let value = cbor::decode(bytes)?;
    let items = expect_array(&value, 2)?;
    let code = items[0].as_u64()?;
    let body = cbor::MapReader::from_value(&items[1])?;
    decode_body(code, from, &body)
}

#[cfg(test)]
mod tests {
    use super::wire::CodeEnum;
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    fn roundtrip(message: Message, from: Direction, expected_hex: &str) {
        let bytes = encode(&message);
        assert_eq!(hex(&bytes), expected_hex);
        assert_eq!(decode(&bytes, from).unwrap(), message);
    }

    fn hello() -> Hello {
        Hello {
            version: Version { major: 0, minor: 2 },
            app: "porecatu-remote".to_owned(),
            app_version: "0.1.0".to_owned(),
        }
    }

    fn welcome() -> Welcome {
        Welcome {
            version: Version { major: 0, minor: 2 },
            host_name: "desk".to_owned(),
            porecatu_version: "0.9.11".to_owned(),
            max_subscriptions: 4,
            host_instance: [0xab; 16],
        }
    }

    // Hand-computed. 82 = array of 2, 00 = type 0, a3 = map of 3, then the
    // keys ordered by their encoded bytes: "app" (63..), "version" (67..),
    // "app_version" (6b..).
    #[test]
    fn hello_round_trip() {
        roundtrip(
            Message::Hello(hello()),
            Direction::ClientToHost,
            concat!(
                "8200a3",
                "63617070",                         // "app"
                "6f706f7265636174752d72656d6f7465", // "porecatu-remote"
                "6776657273696f6e",                 // "version"
                "820002",                           // [0, 2]
                "6b6170705f76657273696f6e",         // "app_version"
                "65302e312e30",                     // "0.1.0"
            ),
        );
    }

    #[test]
    fn welcome_round_trip() {
        let message = Message::Welcome(welcome());
        let bytes = encode(&message);
        // [1, map of 5 ...]
        assert_eq!(&bytes[..3], &[0x82, 0x01, 0xa5]);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
    }

    #[test]
    fn incompatible_round_trip() {
        roundtrip(
            Message::Incompatible(Incompatible {
                version: Version { major: 0, minor: 1 },
            }),
            Direction::HostToClient,
            "8202a16776657273696f6e820001",
        );
    }

    #[test]
    fn goodbye_round_trip_both_directions() {
        let message = Message::Goodbye(Goodbye {
            reason: GoodbyeReason::Revoked,
        });
        let hex_text = "8203a166726561736f6e02";
        roundtrip(message.clone(), Direction::HostToClient, hex_text);
        assert_eq!(
            decode(&unhex(hex_text), Direction::ClientToHost).unwrap(),
            message
        );
    }

    // "id" (62..) sorts before "code" (64..) and "code" before "pane": a
    // shorter encoded key first, then bytewise.
    #[test]
    fn error_round_trip() {
        roundtrip(
            Message::Error(ErrorMessage {
                code: ErrorCode::UnknownTab,
                id: Some(7),
                pane: None,
            }),
            Direction::HostToClient,
            concat!("821846a2", "626964", "07", "64636f6465", "05"),
        );
        roundtrip(
            Message::Error(ErrorMessage {
                code: ErrorCode::Protocol,
                id: None,
                pane: Some(3),
            }),
            Direction::ClientToHost,
            concat!("821846a2", "64636f6465", "00", "6470616e65", "03"),
        );
    }

    #[test]
    fn unknown_type_is_an_error() {
        // [99, {}]
        let bytes = unhex("821863a0");
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::UnknownType(99))
        );
    }

    #[test]
    fn wrong_direction_is_an_error() {
        let bytes = encode(&Message::Welcome(welcome()));
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(1))
        );
        let bytes = encode(&Message::Hello(hello()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::WrongDirection(0))
        );
        // The direction is checked before the body is read.
        let bytes = unhex("8214a0"); // [20, {}]: subscribe, a client message
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::WrongDirection(20))
        );
    }

    #[test]
    fn missing_required_key_is_an_error() {
        // goodbye with an empty body
        assert_eq!(
            decode(&unhex("8203a0"), Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
    }

    #[test]
    fn unknown_keys_are_ignored_but_must_be_canonical() {
        // goodbye { "zzz": true, "reason": 0 }: "zzz" (63..) sorts first.
        let bytes = unhex("8203a2637a7a7af566726561736f6e00");
        assert_eq!(
            decode(&bytes, Direction::HostToClient).unwrap(),
            Message::Goodbye(Goodbye {
                reason: GoodbyeReason::Normal
            })
        );
        // The same map with the keys out of order is refused.
        let bytes = unhex("8203a266726561736f6e00637a7a7af5");
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::UnorderedKeys))
        );
    }

    #[test]
    fn value_outside_the_domain_is_an_error() {
        // goodbye { reason: 9 }
        assert_eq!(
            decode(&unhex("8203a166726561736f6e09"), Direction::HostToClient),
            Err(DecodeError::InvalidValue("reason"))
        );
        // error { code: 2 }: code 2 is reserved
        assert_eq!(
            decode(&unhex("821846a164636f646502"), Direction::HostToClient),
            Err(DecodeError::InvalidValue("code"))
        );
    }

    #[test]
    fn host_instance_must_have_16_bytes() {
        let mut bytes = encode(&Message::Welcome(welcome()));
        // Turn the 16-byte string (0x50 + 16 bytes) into a 15-byte one.
        let at = bytes.iter().position(|b| *b == 0x50).unwrap();
        bytes[at] = 0x4f;
        bytes.remove(at + 1);
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::InvalidValue("host_instance"))
        );
    }

    #[test]
    fn malformed_envelope_is_an_error() {
        // Not an array of two elements.
        assert_eq!(
            decode(&unhex("8100"), Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
        // Trailing bytes.
        let mut bytes = encode(&Message::Goodbye(Goodbye {
            reason: GoodbyeReason::Normal,
        }));
        bytes.push(0);
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::TrailingBytes))
        );
        // Body that is not a map.
        assert_eq!(
            decode(&unhex("820300"), Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // Negative type.
        assert!(decode(&unhex("822000"), Direction::HostToClient).is_err());
        assert_eq!(
            DecodeError::UnknownType(1).error_code(),
            ErrorCode::Protocol
        );
    }

    #[test]
    fn type_codes_match_the_table_of_the_spec() {
        assert_eq!(
            TYPE_CODES,
            [
                0, 1, 2, 3, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 25, 26, 27, 30, 31, 32, 40,
                41, 42, 43, 50, 60, 61, 70
            ]
        );
        assert_eq!(TYPE_CODES.len(), 29);
        assert_eq!(Message::Hello(hello()).type_code(), 0);
        assert_eq!(
            Message::Hello(hello()).sender(),
            Some(Direction::ClientToHost)
        );
        assert_eq!(
            Message::Welcome(welcome()).sender(),
            Some(Direction::HostToClient)
        );
        let goodbye = Message::Goodbye(Goodbye {
            reason: GoodbyeReason::Normal,
        });
        assert_eq!(goodbye.sender(), None);
        assert!(goodbye.can_be_sent_by(Direction::ClientToHost));
        assert!(goodbye.can_be_sent_by(Direction::HostToClient));
        assert!(!Message::Hello(hello()).can_be_sent_by(Direction::HostToClient));
    }

    fn check_enum<E: CodeEnum + PartialEq + std::fmt::Debug>(all: &[E], expected: &[u64]) {
        let codes: Vec<u64> = all.iter().map(|e| e.code()).collect();
        assert_eq!(codes, expected);
        for (variant, code) in all.iter().zip(expected) {
            assert_eq!(E::from_code(*code), Some(*variant));
        }
        for code in 0..=100u64 {
            if !expected.contains(&code) {
                assert_eq!(E::from_code(code), None, "code {code}");
            }
        }
        assert_eq!(E::from_code(u64::MAX), None);
    }

    #[test]
    fn enum_codes_match_the_spec() {
        check_enum(
            ErrorCode::ALL,
            &[
                0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
            ],
        );
        check_enum(GoodbyeReason::ALL, &[0, 1, 2, 3]);
        check_enum(PaneState::ALL, &[0, 1, 2, 3, 4, 5, 6]);
        check_enum(
            NamedKey::ALL,
            &[
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 21, 22, 23, 24, 25, 26, 27, 28, 29,
                30, 31, 32,
            ],
        );
        check_enum(ProfileKind::ALL, &[0, 1, 2, 3]);
        check_enum(CursorShape::ALL, &[0, 1, 2]);
        check_enum(ConfirmKind::ALL, &[0, 1]);
        check_enum(SubscriptionEndReason::ALL, &[0, 1]);
        check_enum(PairRejectReason::ALL, &[0, 1, 2]);
        check_enum(PairPlatform::ALL, &[0, 1, 2]);
        check_enum(SplitDir::ALL, &[0, 1]);
    }
}
