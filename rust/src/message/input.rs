// SPDX-License-Identifier: GPL-3.0-or-later
//! Semantic input: `key`, `text-input`, `paste` (spec 03 §6).
//!
//! `Debug` never shows what the user typed: it shows sizes only.

use std::fmt;

use super::wire::{self, Body};
use super::{DecodeError, PaneId};
use crate::cbor::{CborError, MapReader, Value};

/// Modifier bits of `key.mods`.
pub mod mods {
    /// Ctrl.
    pub const CTRL: u64 = 0x01;
    /// Alt.
    pub const ALT: u64 = 0x02;
    /// Shift.
    pub const SHIFT: u64 = 0x04;
    /// Meta.
    pub const META: u64 = 0x08;
}

code_enum! {
    /// A named key (`named-key`).
    NamedKey {
        /// Enter.
        Enter = 0,
        /// Tab.
        Tab = 1,
        /// Backspace.
        Backspace = 2,
        /// Escape.
        Escape = 3,
        /// Arrow up.
        Up = 4,
        /// Arrow down.
        Down = 5,
        /// Arrow left.
        Left = 6,
        /// Arrow right.
        Right = 7,
        /// Home.
        Home = 8,
        /// End.
        End = 9,
        /// Page up.
        PageUp = 10,
        /// Page down.
        PageDown = 11,
        /// Insert.
        Insert = 12,
        /// Delete.
        Delete = 13,
        /// F1.
        F1 = 21,
        /// F2.
        F2 = 22,
        /// F3.
        F3 = 23,
        /// F4.
        F4 = 24,
        /// F5.
        F5 = 25,
        /// F6.
        F6 = 26,
        /// F7.
        F7 = 27,
        /// F8.
        F8 = 28,
        /// F9.
        F9 = 29,
        /// F10.
        F10 = 30,
        /// F11.
        F11 = 31,
        /// F12.
        F12 = 32,
    }
}

/// The `key` field of a `key` message: a named key (integer on the wire) or
/// exactly one Unicode scalar value (text on the wire).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    /// A named key.
    Named(NamedKey),
    /// One Unicode scalar value, for example `'c'` with Ctrl.
    Char(char),
}

/// `key` (client to host).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// `pane`.
    pub pane: PaneId,
    /// `key`.
    pub key: KeyCode,
    /// `mods`: bit set of [`mods`].
    pub mods: u64,
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key")
            .field("pane", &self.pane)
            .field("mods", &self.mods)
            .finish_non_exhaustive()
    }
}

impl KeyCode {
    fn to_value(self) -> Value {
        match self {
            KeyCode::Named(named) => wire::code(named),
            KeyCode::Char(c) => Value::Text(c.to_string()),
        }
    }

    /// An integer is a `named-key`; a text must be exactly one Unicode
    /// scalar value; anything else is a wrong type.
    fn from_value(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::Int(_) => Ok(KeyCode::Named(wire::enum_from(value, "key")?)),
            Value::Text(text) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Ok(KeyCode::Char(c)),
                    _ => Err(DecodeError::InvalidValue("key")),
                }
            }
            _ => Err(DecodeError::Cbor(CborError::WrongType)),
        }
    }
}

impl Body for Key {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("key", self.key.to_value()),
            ("mods", wire::uint(self.mods)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Key {
            pane: wire::req_u64(r, "pane")?,
            key: KeyCode::from_value(r.required("key")?)?,
            mods: wire::req_u64(r, "mods")?,
        })
    }
}

/// `text-input` (client to host): typed text, including IME composition.
#[derive(Clone, PartialEq, Eq)]
pub struct TextInput {
    /// `pane`.
    pub pane: PaneId,
    /// `text`.
    pub text: String,
}

impl fmt::Debug for TextInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextInput")
            .field("pane", &self.pane)
            .field("text_len", &self.text.len())
            .finish()
    }
}

impl Body for TextInput {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("text", wire::text(&self.text)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(TextInput {
            pane: wire::req_u64(r, "pane")?,
            text: wire::req_text(r, "text")?,
        })
    }
}

/// `paste` (client to host): the host applies the rules of the local paste.
#[derive(Clone, PartialEq, Eq)]
pub struct Paste {
    /// `pane`.
    pub pane: PaneId,
    /// `text`.
    pub text: String,
}

impl fmt::Debug for Paste {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Paste")
            .field("pane", &self.pane)
            .field("text_len", &self.text.len())
            .finish()
    }
}

impl Body for Paste {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("text", wire::text(&self.text)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Paste {
            pane: wire::req_u64(r, "pane")?,
            text: wire::req_text(r, "text")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Direction, Message, decode, encode};
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

    fn roundtrip(message: Message, expected_hex: &str) {
        let bytes = encode(&message);
        assert_eq!(hex(&bytes), expected_hex);
        assert_eq!(decode(&bytes, Direction::ClientToHost).unwrap(), message);
    }

    // Hand-computed. 82 = array of 2, 18 1e = type 30, a3 = map of 3. The
    // keys go in the order of their encoded bytes: "key" (63..), "mods"
    // (64 6d..), "pane" (64 70..).
    #[test]
    fn key_named_round_trip() {
        roundtrip(
            Message::Key(Key {
                pane: 7,
                key: KeyCode::Named(NamedKey::Enter),
                mods: 0,
            }),
            concat!(
                "82181ea3",
                "636b6579",   // "key"
                "00",         // enter
                "646d6f6473", // "mods"
                "00",
                "6470616e65", // "pane"
                "07",
            ),
        );
    }

    #[test]
    fn key_char_round_trip() {
        roundtrip(
            Message::Key(Key {
                pane: 7,
                key: KeyCode::Char('c'),
                mods: mods::CTRL,
            }),
            concat!(
                "82181ea3",
                "636b6579",
                "6163", // "c"
                "646d6f6473",
                "01",
                "6470616e65",
                "07",
            ),
        );
        // The complete case: a scalar value outside the BMP, every modifier.
        let message = Message::Key(Key {
            pane: 1,
            key: KeyCode::Char('\u{1F600}'),
            mods: mods::CTRL | mods::ALT | mods::SHIFT | mods::META,
        });
        assert_eq!(
            decode(&encode(&message), Direction::ClientToHost).unwrap(),
            message
        );
    }

    #[test]
    fn key_every_named_key_round_trips() {
        for named in NamedKey::ALL {
            let message = Message::Key(Key {
                pane: 2,
                key: KeyCode::Named(*named),
                mods: 0,
            });
            assert_eq!(
                decode(&encode(&message), Direction::ClientToHost).unwrap(),
                message
            );
        }
    }

    #[test]
    fn key_missing_key_is_an_error() {
        // key { pane: 7, mods: 0 }
        let bytes = unhex("82181ea2646d6f6473006470616e6507");
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
    }

    #[test]
    fn key_outside_the_domain_is_an_error() {
        // key { key: 14 } (a number not in named-key), pane 7, mods 0
        assert_eq!(
            decode(
                &unhex("82181ea3636b65790e646d6f6473006470616e6507"),
                Direction::ClientToHost
            ),
            Err(DecodeError::InvalidValue("key"))
        );
        // key "ab": two scalar values
        assert_eq!(
            decode(
                &unhex("82181ea3636b6579626162646d6f6473006470616e6507"),
                Direction::ClientToHost
            ),
            Err(DecodeError::InvalidValue("key"))
        );
        // key "": no scalar value
        assert_eq!(
            decode(
                &unhex("82181ea3636b657960646d6f6473006470616e6507"),
                Direction::ClientToHost
            ),
            Err(DecodeError::InvalidValue("key"))
        );
        // "e" followed by a combining accent: two scalar values
        assert_eq!(
            decode(
                &unhex(concat!(
                    "82181ea3636b657963",
                    "65cc81646d6f6473006470616e6507"
                )),
                Direction::ClientToHost
            ),
            Err(DecodeError::InvalidValue("key"))
        );
        // key of another CBOR type
        assert_eq!(
            decode(
                &unhex("82181ea3636b6579f5646d6f6473006470616e6507"),
                Direction::ClientToHost
            ),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
    }

    #[test]
    fn key_negative_number_is_refused() {
        // key = -1 (0x20): the CBOR layer reports a negative as out of range
        // (a protocol error all the same).
        assert_eq!(
            decode(
                &unhex("82181ea3636b657920646d6f6473006470616e6507"),
                Direction::ClientToHost
            ),
            Err(DecodeError::Cbor(CborError::IntOutOfRange))
        );
    }

    #[test]
    fn key_ignores_unknown_keys() {
        // key with an extra "zzz" (63 7a7a7a) = false: it sorts after "key"
        // (63 6b6579) and before "mods" (64..).
        let bytes = unhex(concat!(
            "82181ea4636b6579",
            "00637a7a7af4646d6f6473006470616e6507"
        ));
        assert_eq!(
            decode(&bytes, Direction::ClientToHost).unwrap(),
            Message::Key(Key {
                pane: 7,
                key: KeyCode::Named(NamedKey::Enter),
                mods: 0,
            })
        );
    }

    #[test]
    fn key_debug_hides_the_key() {
        let key = Key {
            pane: 7,
            key: KeyCode::Char('s'),
            mods: 0,
        };
        let shown = format!("{key:?}");
        assert!(!shown.contains("Char") && !shown.contains("'s'"));
    }

    // "pane" (64 70..) sorts before "text" (64 74..).
    #[test]
    fn text_input_round_trip() {
        roundtrip(
            Message::TextInput(TextInput {
                pane: 7,
                text: "ls".to_owned(),
            }),
            concat!(
                "82181fa2",
                "6470616e65", // "pane"
                "07",
                "6474657874", // "text"
                "626c73",     // "ls"
            ),
        );
        // The complete case: IME composition, multi-byte text, empty text.
        for text in ["日本語", "e\u{301}", ""] {
            let message = Message::TextInput(TextInput {
                pane: 9,
                text: text.to_owned(),
            });
            assert_eq!(
                decode(&encode(&message), Direction::ClientToHost).unwrap(),
                message
            );
        }
    }

    #[test]
    fn text_input_errors() {
        // text-input { pane: 7 }
        assert_eq!(
            decode(&unhex("82181fa16470616e6507"), Direction::ClientToHost),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // text-input { pane: 7, text: 5 }: text of the wrong type
        assert_eq!(
            decode(
                &unhex("82181fa26470616e6507647465787405"),
                Direction::ClientToHost
            ),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // Unknown key "zzz" (sorts first) is ignored.
        assert_eq!(
            decode(
                &unhex("82181fa3637a7a7af56470616e65076474657874626c73"),
                Direction::ClientToHost
            )
            .unwrap(),
            Message::TextInput(TextInput {
                pane: 7,
                text: "ls".to_owned(),
            })
        );
    }

    #[test]
    fn paste_round_trip() {
        roundtrip(
            Message::Paste(Paste {
                pane: 7,
                text: "a\nb".to_owned(),
            }),
            concat!(
                "821820a2",
                "6470616e65", // "pane"
                "07",
                "6474657874", // "text"
                "63610a62",   // "a\nb"
            ),
        );
        let message = Message::Paste(Paste {
            pane: 1,
            text: "x".repeat(10_000),
        });
        assert_eq!(
            decode(&encode(&message), Direction::ClientToHost).unwrap(),
            message
        );
    }

    #[test]
    fn paste_errors() {
        // paste { text: "a" }
        assert_eq!(
            decode(&unhex("821820a164746578746161"), Direction::ClientToHost),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // paste { pane: 7, text: "a", zzz: true } with "zzz" first
        assert_eq!(
            decode(
                &unhex(concat!("821820a3637a7a7af56470616e65076474657874", "6161")),
                Direction::ClientToHost
            )
            .unwrap(),
            Message::Paste(Paste {
                pane: 7,
                text: "a".to_owned(),
            })
        );
    }

    #[test]
    fn input_is_a_client_message() {
        let bytes = encode(&Message::TextInput(TextInput {
            pane: 7,
            text: "ls".to_owned(),
        }));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::WrongDirection(31))
        );
    }

    #[test]
    fn debug_never_shows_the_text() {
        let text = TextInput {
            pane: 7,
            text: "secret".to_owned(),
        };
        assert!(!format!("{text:?}").contains("secret"));
        let paste = Paste {
            pane: 7,
            text: "secret".to_owned(),
        };
        assert!(!format!("{paste:?}").contains("secret"));
    }
}
