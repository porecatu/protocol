// SPDX-License-Identifier: GPL-3.0-or-later
//! Actions and their confirmation: `action`, `action-result`,
//! `confirm-request`, `confirm` (spec 03 §7).

use super::wire::{self, Body};
use super::{DecodeError, ErrorCode, GroupId, PaneId, ProfileId, RequestId, TabId, WindowId};
use crate::cbor::{MapReader, Value, expect_array};

/// `action` (client to host): the host runs it through the same path as the
/// local gesture and answers with `action-result`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// `id`: chosen by the client, unique among pending requests.
    pub id: RequestId,
    /// `do` (a Rust keyword): the action to run.
    pub action: ActionBody,
}

impl Body for Action {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            // `do` is a Rust keyword: the field is `action`.
            ("do", self.action.to_value()),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Action {
            id: wire::req_u64(r, "id")?,
            action: ActionBody::from_value(r.required("do")?)?,
        })
    }
}

/// The body of an action, `[code, map]`. The comment on each variant gives
/// its code on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionBody {
    /// 0, `new-tab`.
    NewTab {
        /// `window`.
        window: WindowId,
        /// `? "group"`: where to append the tab.
        group: Option<GroupId>,
        /// `? "profile"`: defaults to the host's default profile.
        profile: Option<ProfileId>,
    },
    /// 1, `close-tab`.
    CloseTab {
        /// `tab`.
        tab: TabId,
    },
    /// 2, `close-pane`.
    ClosePane {
        /// `pane`.
        pane: PaneId,
    },
    /// 3, `rename-tab`.
    RenameTab {
        /// `tab`.
        tab: TabId,
        /// `title`: `None` (null) clears the custom title.
        title: Option<String>,
    },
    /// 4, `move-tab-to-group`.
    MoveTabToGroup {
        /// `tab`.
        tab: TabId,
        /// `group`: `None` (null) takes the tab out of its group.
        group: Option<GroupId>,
    },
    /// 5, `move-tab-to-window`.
    MoveTabToWindow {
        /// `tab`.
        tab: TabId,
        /// `window`.
        window: WindowId,
    },
    /// 6, `new-group`.
    NewGroup {
        /// `tabs`: at least one, all in the same window, none pinned.
        tabs: Vec<TabId>,
        /// `name`.
        name: String,
        /// `? "color"`: index into `theme.group_colors`.
        color: Option<u64>,
    },
    /// 7, `rename-group`.
    RenameGroup {
        /// `group`.
        group: GroupId,
        /// `name`.
        name: String,
    },
    /// 8, `set-group-color`.
    SetGroupColor {
        /// `group`.
        group: GroupId,
        /// `color`.
        color: u64,
    },
    /// 9, `collapse-group`.
    CollapseGroup {
        /// `group`.
        group: GroupId,
        /// `collapsed`.
        collapsed: bool,
    },
    /// 10, `move-group-to-window`.
    MoveGroupToWindow {
        /// `group`.
        group: GroupId,
        /// `window`.
        window: WindowId,
    },
    /// 11, `ungroup`.
    Ungroup {
        /// `group`.
        group: GroupId,
    },
    /// 12, `start-pane`.
    StartPane {
        /// `pane`.
        pane: PaneId,
    },
    /// 13, `activate-tab`: makes the tab the active one in its own window.
    ActivateTab {
        /// `tab`.
        tab: TabId,
    },
}

impl ActionBody {
    /// The wire code of the action (the first element of `action-body`).
    pub fn code(&self) -> u64 {
        match self {
            ActionBody::NewTab { .. } => 0,
            ActionBody::CloseTab { .. } => 1,
            ActionBody::ClosePane { .. } => 2,
            ActionBody::RenameTab { .. } => 3,
            ActionBody::MoveTabToGroup { .. } => 4,
            ActionBody::MoveTabToWindow { .. } => 5,
            ActionBody::NewGroup { .. } => 6,
            ActionBody::RenameGroup { .. } => 7,
            ActionBody::SetGroupColor { .. } => 8,
            ActionBody::CollapseGroup { .. } => 9,
            ActionBody::MoveGroupToWindow { .. } => 10,
            ActionBody::Ungroup { .. } => 11,
            ActionBody::StartPane { .. } => 12,
            ActionBody::ActivateTab { .. } => 13,
        }
    }

    /// The positional array `[code, map]`.
    pub fn to_value(&self) -> Value {
        let body = match self {
            ActionBody::NewTab {
                window,
                group,
                profile,
            } => {
                let mut entries = vec![("window", wire::uint(*window))];
                if let Some(group) = group {
                    entries.push(("group", wire::uint(*group)));
                }
                if let Some(profile) = profile {
                    entries.push(("profile", wire::text(profile)));
                }
                wire::map(entries)
            }
            ActionBody::CloseTab { tab } | ActionBody::ActivateTab { tab } => {
                wire::map(vec![("tab", wire::uint(*tab))])
            }
            ActionBody::ClosePane { pane } | ActionBody::StartPane { pane } => {
                wire::map(vec![("pane", wire::uint(*pane))])
            }
            ActionBody::RenameTab { tab, title } => wire::map(vec![
                ("tab", wire::uint(*tab)),
                ("title", wire::nullable_text(title)),
            ]),
            ActionBody::MoveTabToGroup { tab, group } => wire::map(vec![
                ("tab", wire::uint(*tab)),
                ("group", wire::nullable_uint(*group)),
            ]),
            ActionBody::MoveTabToWindow { tab, window } => wire::map(vec![
                ("tab", wire::uint(*tab)),
                ("window", wire::uint(*window)),
            ]),
            ActionBody::NewGroup { tabs, name, color } => {
                let mut entries = vec![
                    (
                        "tabs",
                        Value::Array(tabs.iter().map(|t| wire::uint(*t)).collect()),
                    ),
                    ("name", wire::text(name)),
                ];
                if let Some(color) = color {
                    entries.push(("color", wire::uint(*color)));
                }
                wire::map(entries)
            }
            ActionBody::RenameGroup { group, name } => wire::map(vec![
                ("group", wire::uint(*group)),
                ("name", wire::text(name)),
            ]),
            ActionBody::SetGroupColor { group, color } => wire::map(vec![
                ("group", wire::uint(*group)),
                ("color", wire::uint(*color)),
            ]),
            ActionBody::CollapseGroup { group, collapsed } => wire::map(vec![
                ("group", wire::uint(*group)),
                ("collapsed", Value::Bool(*collapsed)),
            ]),
            ActionBody::MoveGroupToWindow { group, window } => wire::map(vec![
                ("group", wire::uint(*group)),
                ("window", wire::uint(*window)),
            ]),
            ActionBody::Ungroup { group } => wire::map(vec![("group", wire::uint(*group))]),
        };
        Value::Array(vec![wire::uint(self.code()), body])
    }

    /// Reads `[code, map]`, requiring exactly two elements. An unknown code
    /// is a value outside the domain of `do`.
    pub fn from_value(value: &Value) -> Result<Self, DecodeError> {
        let items = expect_array(value, 2)?;
        let code = items[0].as_u64()?;
        let r = MapReader::from_value(&items[1])?;
        Ok(match code {
            0 => ActionBody::NewTab {
                window: wire::req_u64(&r, "window")?,
                group: wire::opt_u64(&r, "group")?,
                profile: r
                    .optional("profile")
                    .map(|v| v.as_text().map(str::to_owned))
                    .transpose()?,
            },
            1 => ActionBody::CloseTab {
                tab: wire::req_u64(&r, "tab")?,
            },
            2 => ActionBody::ClosePane {
                pane: wire::req_u64(&r, "pane")?,
            },
            3 => ActionBody::RenameTab {
                tab: wire::req_u64(&r, "tab")?,
                title: wire::req_nullable_text(&r, "title")?,
            },
            4 => ActionBody::MoveTabToGroup {
                tab: wire::req_u64(&r, "tab")?,
                group: wire::req_nullable_u64(&r, "group")?,
            },
            5 => ActionBody::MoveTabToWindow {
                tab: wire::req_u64(&r, "tab")?,
                window: wire::req_u64(&r, "window")?,
            },
            6 => {
                let tabs = r
                    .required("tabs")?
                    .as_array()?
                    .iter()
                    .map(|t| t.as_u64())
                    .collect::<Result<Vec<_>, _>>()?;
                if tabs.is_empty() {
                    return Err(DecodeError::InvalidValue("tabs"));
                }
                ActionBody::NewGroup {
                    tabs,
                    name: wire::req_text(&r, "name")?,
                    color: wire::opt_u64(&r, "color")?,
                }
            }
            7 => ActionBody::RenameGroup {
                group: wire::req_u64(&r, "group")?,
                name: wire::req_text(&r, "name")?,
            },
            8 => ActionBody::SetGroupColor {
                group: wire::req_u64(&r, "group")?,
                color: wire::req_u64(&r, "color")?,
            },
            9 => ActionBody::CollapseGroup {
                group: wire::req_u64(&r, "group")?,
                collapsed: wire::req_bool(&r, "collapsed")?,
            },
            10 => ActionBody::MoveGroupToWindow {
                group: wire::req_u64(&r, "group")?,
                window: wire::req_u64(&r, "window")?,
            },
            11 => ActionBody::Ungroup {
                group: wire::req_u64(&r, "group")?,
            },
            12 => ActionBody::StartPane {
                pane: wire::req_u64(&r, "pane")?,
            },
            13 => ActionBody::ActivateTab {
                tab: wire::req_u64(&r, "tab")?,
            },
            _ => return Err(DecodeError::InvalidValue("do")),
        })
    }
}

/// `action-result` (host to client).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionResult {
    /// `id`: of the action.
    pub id: RequestId,
    /// `? "error"`: absent means success.
    pub error: Option<ErrorCode>,
    /// `? "created"`: `new-tab` returns the tab id, `new-group` the group id.
    pub created: Option<u64>,
}

impl Body for ActionResult {
    fn to_value(&self) -> Value {
        let mut entries = vec![("id", wire::uint(self.id))];
        if let Some(error) = self.error {
            entries.push(("error", wire::code(error)));
        }
        if let Some(created) = self.created {
            entries.push(("created", wire::uint(created)));
        }
        wire::map(entries)
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(ActionResult {
            id: wire::req_u64(r, "id")?,
            error: r
                .optional("error")
                .map(|v| wire::enum_from(v, "error"))
                .transpose()?,
            // A number whose meaning depends on the action that asked for it:
            // read as a plain number.
            created: wire::opt_u64(r, "created")?,
        })
    }
}

code_enum! {
    /// What a closing would end (`confirm-request.kind`).
    ConfirmKind {
        /// A process in the foreground.
        RunningProcess = 0,
        /// An open SSH connection.
        SshConnection = 1,
    }
}

/// `confirm-request` (host to client): asked of the client that requested the
/// action, not of the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    /// `id`: of the action.
    pub id: RequestId,
    /// `kind`.
    pub kind: ConfirmKind,
    /// `processes`: names, for the client to list.
    pub processes: Vec<String>,
}

impl Body for ConfirmRequest {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            ("kind", wire::code(self.kind)),
            (
                "processes",
                Value::Array(self.processes.iter().map(|p| wire::text(p)).collect()),
            ),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(ConfirmRequest {
            id: wire::req_u64(r, "id")?,
            kind: wire::req_enum(r, "kind")?,
            processes: r
                .required("processes")?
                .as_array()?
                .iter()
                .map(|p| p.as_text().map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

/// `confirm` (client to host).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confirm {
    /// `id`: of the action.
    pub id: RequestId,
    /// `accept`.
    pub accept: bool,
}

impl Body for Confirm {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            ("accept", Value::Bool(self.accept)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Confirm {
            id: wire::req_u64(r, "id")?,
            accept: wire::req_bool(r, "accept")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Direction, Message, decode, encode};
    use super::*;
    use crate::cbor::CborError;

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

    fn round_trips(message: Message, from: Direction) {
        assert_eq!(decode(&encode(&message), from).unwrap(), message);
    }

    fn act(id: RequestId, action: ActionBody) -> Message {
        Message::Action(Action { id, action })
    }

    // Hand-computed. 82 = array of 2, 18 28 = type 40, a2 = map of 2; "do"
    // (62 646f) sorts before "id" (62 6964); the action body is [0, {window}]
    // with "window" = 66 77696e646f77.
    #[test]
    fn action_new_tab_minimal_round_trip() {
        roundtrip(
            act(
                1,
                ActionBody::NewTab {
                    window: 1,
                    group: None,
                    profile: None,
                },
            ),
            Direction::ClientToHost,
            concat!(
                "821828a2",
                "62646f", // "do"
                "8200a1", // [0, {1 entry}]
                "66",     // "window"
                "77696e646f77",
                "01",
                "626964", // "id"
                "01",
            ),
        );
    }

    // "tab" (63..) sorts before "title" (65..); a null title is present.
    #[test]
    fn action_rename_tab_round_trip() {
        roundtrip(
            act(
                2,
                ActionBody::RenameTab {
                    tab: 5,
                    title: None,
                },
            ),
            Direction::ClientToHost,
            concat!(
                "821828a2",
                "62646f",
                "8203a2",
                "63746162", // "tab"
                "05",
                "65", // "title"
                "7469746c65",
                "f6", // null
                "626964",
                "02",
            ),
        );
        round_trips(
            act(
                2,
                ActionBody::RenameTab {
                    tab: 5,
                    title: Some("build".to_owned()),
                },
            ),
            Direction::ClientToHost,
        );
    }

    // "name" (64 6e..) < "tabs" (64 74..) < "color" (65..).
    #[test]
    fn action_new_group_round_trip() {
        roundtrip(
            act(
                4,
                ActionBody::NewGroup {
                    tabs: vec![1, 2],
                    name: "g".to_owned(),
                    color: Some(2),
                },
            ),
            Direction::ClientToHost,
            concat!(
                "821828a2",
                "62646f",
                "8206a3",
                "646e616d65", // "name"
                "6167",       // "g"
                "6474616273", // "tabs"
                "820102",
                "65636f6c6f72", // "color"
                "02",
                "626964",
                "04",
            ),
        );
        // Without the optional color the key is omitted.
        let message = act(
            4,
            ActionBody::NewGroup {
                tabs: vec![9],
                name: "g".to_owned(),
                color: None,
            },
        );
        assert!(!hex(&encode(&message)).contains("65636f6c6f72"));
        round_trips(message, Direction::ClientToHost);
    }

    #[test]
    fn every_action_body_round_trips_minimum_and_complete() {
        let bodies = vec![
            ActionBody::NewTab {
                window: 1,
                group: Some(2),
                profile: Some("auto:pwsh".to_owned()),
            },
            ActionBody::CloseTab { tab: 3 },
            ActionBody::ClosePane { pane: 4 },
            ActionBody::RenameTab {
                tab: 5,
                title: None,
            },
            ActionBody::RenameTab {
                tab: 5,
                title: Some("x".to_owned()),
            },
            ActionBody::MoveTabToGroup {
                tab: 6,
                group: None,
            },
            ActionBody::MoveTabToGroup {
                tab: 6,
                group: Some(7),
            },
            ActionBody::MoveTabToWindow { tab: 8, window: 9 },
            ActionBody::NewGroup {
                tabs: vec![1, 2, 3],
                name: "g".to_owned(),
                color: Some(0),
            },
            ActionBody::RenameGroup {
                group: 10,
                name: "n".to_owned(),
            },
            ActionBody::SetGroupColor {
                group: 11,
                color: 3,
            },
            ActionBody::CollapseGroup {
                group: 12,
                collapsed: true,
            },
            ActionBody::CollapseGroup {
                group: 12,
                collapsed: false,
            },
            ActionBody::MoveGroupToWindow {
                group: 13,
                window: 14,
            },
            ActionBody::Ungroup { group: 15 },
            ActionBody::StartPane { pane: 16 },
            ActionBody::ActivateTab { tab: 17 },
        ];
        let mut codes = Vec::new();
        for body in bodies {
            codes.push(body.code());
            round_trips(act(1, body), Direction::ClientToHost);
        }
        codes.dedup();
        assert_eq!(codes, (0..=13).collect::<Vec<u64>>());
    }

    macro_rules! client {
        ($($s:literal)+) => {
            decode(&unhex(concat!($($s),+)), Direction::ClientToHost)
        };
        ($e:expr) => {
            decode(&unhex($e), Direction::ClientToHost)
        };
    }

    macro_rules! host {
        ($($s:literal)+) => {
            decode(&unhex(concat!($($s),+)), Direction::HostToClient)
        };
        ($e:expr) => {
            decode(&unhex($e), Direction::HostToClient)
        };
    }

    #[test]
    fn action_errors() {
        // action { id: 1 }: missing "do"
        assert_eq!(
            client!("821828a1626964" "01"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // action { do: [11, {group: 3}] }: missing "id"
        assert_eq!(
            client!("821828a162646f820ba165" "67726f7570" "03"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // do = [14, {}]: unknown action
        assert_eq!(
            client!("821828a262646f820ea0" "62696401"),
            Err(DecodeError::InvalidValue("do"))
        );
        // do = [13, {}]: activate-tab without "tab"
        assert_eq!(
            client!("821828a262646f820da0" "62696401"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // do = [1, {}]: close-tab without "tab"
        assert_eq!(
            client!("821828a262646f8201a0" "62696401"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // do = [0, {}]: new-tab without "window"
        assert_eq!(
            client!("821828a262646f8200a0" "62696401"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // do = [3, {tab: 5}]: rename-tab without "title"
        assert_eq!(
            client!("821828a262646f8203a1" "6374616205" "62696401"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // do with the wrong arity: [1]
        assert_eq!(
            client!("821828a262646f8101" "62696401"),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
        // do that is not an array
        assert_eq!(
            client!("821828a262646f0b" "62696401"),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // new-group with no tabs
        assert_eq!(
            client!(concat!(
                "821828a262646f8206a2",
                "646e616d65",
                "6167", // name "g"
                "6474616273",
                "80", // tabs []
                "62696401"
            )),
            Err(DecodeError::InvalidValue("tabs"))
        );
        // new-tab whose optional "group" is null: an error, not an absence
        // ("group" = 65 67726f7570 sorts before "window").
        assert_eq!(
            client!(concat!(
                "821828a262646f8200a2",
                "6567726f7570f6",
                "6677696e646f7701",
                "62696401"
            )),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // rename-tab with a numeric title
        assert_eq!(
            client!("821828a262646f8203a2" "6374616205" "657469746c6501" "62696401"),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
    }

    #[test]
    fn action_ignores_unknown_keys() {
        // "zzz" (63 7a7a7a) sorts after "tab" (63 746162) in the body and
        // after "do" and "id" (62..) in the message.
        let bytes = concat!(
            "821828a3", "62646f", "8201a2", "63746162", "03", "637a7a7a", "f5", "626964", "01",
            "637a7a7a", "f5"
        );
        assert_eq!(
            client!(bytes).unwrap(),
            act(1, ActionBody::CloseTab { tab: 3 })
        );
    }

    #[test]
    fn action_result_round_trips() {
        // [41, {id: 1, created: 9}]; "created" = 67 63726561746564
        roundtrip(
            Message::ActionResult(ActionResult {
                id: 1,
                error: None,
                created: Some(9),
            }),
            Direction::HostToClient,
            concat!("821829a2", "626964", "01", "67", "63726561746564", "09"),
        );
        // [41, {id: 2, error: 12}] (busy); "error" = 65 6572726f72
        roundtrip(
            Message::ActionResult(ActionResult {
                id: 2,
                error: Some(ErrorCode::Busy),
                created: None,
            }),
            Direction::HostToClient,
            concat!("821829a2", "626964", "02", "65", "6572726f72", "0c"),
        );
        // minimal and complete
        round_trips(
            Message::ActionResult(ActionResult {
                id: 1,
                error: None,
                created: None,
            }),
            Direction::HostToClient,
        );
        round_trips(
            Message::ActionResult(ActionResult {
                id: 1,
                error: Some(ErrorCode::LastTab),
                created: Some(4),
            }),
            Direction::HostToClient,
        );
    }

    #[test]
    fn action_result_errors() {
        // missing "id"
        assert_eq!(
            host!("821829a165" "6572726f72" "0c"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // error 2 is reserved, 17 is unassigned
        assert_eq!(
            host!("821829a2" "62696401" "656572726f72" "02"),
            Err(DecodeError::InvalidValue("error"))
        );
        assert_eq!(
            host!("821829a2" "62696401" "656572726f72" "11"),
            Err(DecodeError::InvalidValue("error"))
        );
        // created as text
        assert_eq!(
            host!("821829a2" "62696401" "6763726561746564" "6139"),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // unknown key "zzz" (63 7a7a7a) goes last
        assert_eq!(
            host!("821829a3" "62696401" "637a7a7af5" "656572726f72" "0c").unwrap(),
            Message::ActionResult(ActionResult {
                id: 1,
                error: Some(ErrorCode::Busy),
                created: None,
            })
        );
    }

    #[test]
    fn confirm_request_round_trips() {
        // [42, {id: 3, kind: 0, processes: ["vim"]}]
        roundtrip(
            Message::ConfirmRequest(ConfirmRequest {
                id: 3,
                kind: ConfirmKind::RunningProcess,
                processes: vec!["vim".to_owned()],
            }),
            Direction::HostToClient,
            concat!(
                "82182aa3",
                "626964",
                "03",
                "646b696e64",
                "00",
                "69",
                "70726f636573736573", // "processes"
                "81",
                "63",
                "76696d", // ["vim"]
            ),
        );
        round_trips(
            Message::ConfirmRequest(ConfirmRequest {
                id: 3,
                kind: ConfirmKind::SshConnection,
                processes: vec![],
            }),
            Direction::HostToClient,
        );
        round_trips(
            Message::ConfirmRequest(ConfirmRequest {
                id: 3,
                kind: ConfirmKind::RunningProcess,
                processes: vec!["vim".to_owned(), "node".to_owned()],
            }),
            Direction::HostToClient,
        );
    }

    #[test]
    fn confirm_request_errors() {
        // missing "processes"
        assert_eq!(
            host!("82182aa2" "62696403" "646b696e64" "00"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // kind 2
        assert_eq!(
            host!("82182aa3" "62696403" "646b696e64" "02" "6970726f636573736573" "80"),
            Err(DecodeError::InvalidValue("kind"))
        );
        // processes with a non-text entry
        assert_eq!(
            host!("82182aa3" "62696403" "646b696e64" "00" "6970726f636573736573" "8101"),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // unknown key "zzz" goes before "processes" (63 < 69)... after "id".
        assert_eq!(
            host!(concat!(
                "82182aa4",
                "62696403",
                "637a7a7af5",
                "646b696e6400",
                "6970726f63657373657380"
            ))
            .unwrap(),
            Message::ConfirmRequest(ConfirmRequest {
                id: 3,
                kind: ConfirmKind::RunningProcess,
                processes: vec![],
            })
        );
    }

    #[test]
    fn confirm_round_trips() {
        // [43, {id: 3, accept: true}]; "accept" = 66 616363657074
        roundtrip(
            Message::Confirm(Confirm {
                id: 3,
                accept: true,
            }),
            Direction::ClientToHost,
            concat!("82182ba2", "626964", "03", "66", "616363657074", "f5"),
        );
        round_trips(
            Message::Confirm(Confirm {
                id: 3,
                accept: false,
            }),
            Direction::ClientToHost,
        );
    }

    #[test]
    fn confirm_errors() {
        // missing "accept"
        assert_eq!(
            client!("82182ba1" "62696403"),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // accept as a number
        assert_eq!(
            client!("82182ba2" "62696403" "66616363657074" "01"),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // unknown key "zzz" goes before "accept" (63 < 66)... after "id"
        assert_eq!(
            client!("82182ba3" "62696403" "637a7a7af5" "66616363657074f5").unwrap(),
            Message::Confirm(Confirm {
                id: 3,
                accept: true,
            })
        );
        // The keys out of order are refused.
        assert_eq!(
            client!("82182ba2" "66616363657074f5" "62696403"),
            Err(DecodeError::Cbor(CborError::UnorderedKeys))
        );
    }

    #[test]
    fn action_messages_are_one_way() {
        let action = encode(&act(1, ActionBody::CloseTab { tab: 3 }));
        assert_eq!(
            decode(&action, Direction::HostToClient),
            Err(DecodeError::WrongDirection(40))
        );
        let result = encode(&Message::ActionResult(ActionResult {
            id: 1,
            error: None,
            created: None,
        }));
        assert_eq!(
            decode(&result, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(41))
        );
    }
}
