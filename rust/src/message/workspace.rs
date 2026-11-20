// SPDX-License-Identifier: GPL-3.0-or-later
//! Workspace state: `workspace`, `profiles`, `theme`, `view-tab`,
//! `list-sessions` and `sessions` (spec 03 §4).

use super::wire::{self, Body};
use super::{DecodeError, GroupId, PaneId, ProfileId, RequestId, TabId, WindowId};
use crate::cbor::{MapReader, Value};

/// Largest value of an `rgb` (`0xRRGGBB`).
const RGB_MAX: u64 = 0xFF_FFFF;

/// An `rgb` as a value.
fn rgb_value(color: u32) -> Value {
    wire::uint(u64::from(color))
}

/// An `rgb` from a value; above `0xFFFFFF` is out of the domain of `key`.
fn rgb_from(value: &Value, key: &'static str) -> Result<u32, DecodeError> {
    let color = value.as_u64()?;
    if color > RGB_MAX {
        return Err(DecodeError::InvalidValue(key));
    }
    u32::try_from(color).map_err(|_| DecodeError::InvalidValue(key))
}

/// A required array whose elements are read by `read`.
fn req_array<T>(
    r: &MapReader<'_>,
    key: &str,
    read: impl Fn(&Value) -> Result<T, DecodeError>,
) -> Result<Vec<T>, DecodeError> {
    r.required(key)?.as_array()?.iter().map(read).collect()
}

/// `workspace` (host to client): the whole tree, resent on every structural
/// or label change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// `rev`: grows with every send in the session.
    pub rev: u64,
    /// `windows`: in the host's focus order, most recent first.
    pub windows: Vec<Window>,
}

impl Body for Workspace {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("rev", wire::uint(self.rev)),
            (
                "windows",
                Value::Array(self.windows.iter().map(Window::to_value).collect()),
            ),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Workspace {
            rev: wire::req_u64(r, "rev")?,
            windows: req_array(r, "windows", Window::from_value)?,
        })
    }
}

/// A window of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// `id`.
    pub id: WindowId,
    /// `title`: title of the window's active tab.
    pub title: String,
    /// `pinned`: pinned tabs, in the order of the zone.
    pub pinned: Vec<Tab>,
    /// `items`: the track, loose tabs and groups in visual order.
    pub items: Vec<Item>,
    /// `active`: the active tab of the window on the desktop.
    pub active: Option<TabId>,
}

impl Window {
    /// The window as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            ("title", wire::text(&self.title)),
            (
                "pinned",
                Value::Array(self.pinned.iter().map(Tab::to_value).collect()),
            ),
            (
                "items",
                Value::Array(self.items.iter().map(Item::to_value).collect()),
            ),
            ("active", wire::nullable_uint(self.active)),
        ])
    }

    /// Reads a window from a CBOR map.
    pub fn from_value(value: &Value) -> Result<Window, DecodeError> {
        let r = MapReader::from_value(value)?;
        Ok(Window {
            id: wire::req_u64(&r, "id")?,
            title: wire::req_text(&r, "title")?,
            pinned: req_array(&r, "pinned", Tab::from_value)?,
            items: req_array(&r, "items", Item::from_value)?,
            active: wire::req_nullable_u64(&r, "active")?,
        })
    }
}

/// An entry of the track: a loose tab or a group. On the wire the presence of
/// the `group` key tells a group from a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A loose tab.
    Tab(Tab),
    /// A group of tabs.
    Group(Group),
}

impl Item {
    /// The item as a CBOR map.
    pub fn to_value(&self) -> Value {
        match self {
            Item::Tab(tab) => tab.to_value(),
            Item::Group(group) => group.to_value(),
        }
    }

    /// Reads an item: a group if the map has `"group"`, a tab if it has
    /// `"tab"`; both or neither is out of the domain.
    pub fn from_value(value: &Value) -> Result<Item, DecodeError> {
        let r = MapReader::from_value(value)?;
        match (r.optional("group").is_some(), r.optional("tab").is_some()) {
            (true, false) => Ok(Item::Group(Group::from_value(value)?)),
            (false, true) => Ok(Item::Tab(Tab::from_value(value)?)),
            _ => Err(DecodeError::InvalidValue("item")),
        }
    }
}

/// A group of tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// `group`: the id; also what marks the map as a group.
    pub id: GroupId,
    /// `name`.
    pub name: String,
    /// `color`: index into `theme.group_colors`.
    pub color: u64,
    /// `collapsed`.
    pub collapsed: bool,
    /// `tabs`: at least one.
    pub tabs: Vec<Tab>,
}

impl Group {
    /// The group as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("group", wire::uint(self.id)),
            ("name", wire::text(&self.name)),
            ("color", wire::uint(self.color)),
            ("collapsed", Value::Bool(self.collapsed)),
            (
                "tabs",
                Value::Array(self.tabs.iter().map(Tab::to_value).collect()),
            ),
        ])
    }

    /// Reads a group from a CBOR map.
    pub fn from_value(value: &Value) -> Result<Group, DecodeError> {
        let r = MapReader::from_value(value)?;
        let tabs = req_array(&r, "tabs", Tab::from_value)?;
        if tabs.is_empty() {
            return Err(DecodeError::InvalidValue("tabs"));
        }
        Ok(Group {
            id: wire::req_u64(&r, "group")?,
            name: wire::req_text(&r, "name")?,
            color: wire::req_u64(&r, "color")?,
            collapsed: wire::req_bool(&r, "collapsed")?,
            tabs,
        })
    }
}

/// A tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// `tab`: the id.
    pub id: TabId,
    /// `title`: the title the desktop shows, precedence already applied.
    pub title: String,
    /// `custom`: the user set the title.
    pub custom: bool,
    /// `profile`.
    pub profile: Option<ProfileId>,
    /// `remote`: `user@host` on an SSH tab, `None` on local ones.
    pub remote: Option<String>,
    /// `activity`: background activity indicator.
    pub activity: bool,
    /// `bell`: bell indicator.
    pub bell: bool,
    /// `layout`: the pane tree.
    pub layout: PaneNode,
    /// `focused`: the focused pane of the tab.
    pub focused: PaneId,
}

impl Tab {
    /// The tab as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("tab", wire::uint(self.id)),
            ("title", wire::text(&self.title)),
            ("custom", Value::Bool(self.custom)),
            ("profile", wire::nullable_text(&self.profile)),
            ("remote", wire::nullable_text(&self.remote)),
            ("activity", Value::Bool(self.activity)),
            ("bell", Value::Bool(self.bell)),
            ("layout", self.layout.to_value()),
            ("focused", wire::uint(self.focused)),
        ])
    }

    /// Reads a tab from a CBOR map.
    pub fn from_value(value: &Value) -> Result<Tab, DecodeError> {
        let r = MapReader::from_value(value)?;
        Ok(Tab {
            id: wire::req_u64(&r, "tab")?,
            title: wire::req_text(&r, "title")?,
            custom: wire::req_bool(&r, "custom")?,
            profile: wire::req_nullable_text(&r, "profile")?,
            remote: wire::req_nullable_text(&r, "remote")?,
            activity: wire::req_bool(&r, "activity")?,
            bell: wire::req_bool(&r, "bell")?,
            layout: PaneNode::from_value(r.required("layout")?, "layout")?,
            focused: wire::req_u64(&r, "focused")?,
        })
    }
}

/// A node of the pane tree: a pane or a split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneNode {
    /// A leaf.
    Pane(Pane),
    /// A division in two.
    Split(Box<Split>),
}

impl PaneNode {
    /// The node as a CBOR map.
    pub fn to_value(&self) -> Value {
        match self {
            PaneNode::Pane(pane) => pane.to_value(),
            PaneNode::Split(split) => split.to_value(),
        }
    }

    /// Reads a node: a split if the map has `"dir"`, a pane if it has
    /// `"pane"`; both or neither is out of the domain of `key`. The nesting
    /// depth is bounded by the CBOR decoder (32 levels).
    pub fn from_value(value: &Value, key: &'static str) -> Result<PaneNode, DecodeError> {
        let r = MapReader::from_value(value)?;
        match (r.optional("dir").is_some(), r.optional("pane").is_some()) {
            (true, false) => Ok(PaneNode::Split(Box::new(Split::from_value(value)?))),
            (false, true) => Ok(PaneNode::Pane(Pane::from_value(value)?)),
            _ => Err(DecodeError::InvalidValue(key)),
        }
    }
}

code_enum! {
    /// Direction of a split (`split.dir`).
    SplitDir {
        /// The two children side by side.
        SideBySide = 0,
        /// One child above the other.
        Stacked = 1,
    }
}

/// A division of the pane tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    /// `dir`.
    pub dir: SplitDir,
    /// `ratio`: `1..=65535`; the first child gets `ratio / 65536`.
    pub ratio: u16,
    /// `first`.
    pub first: PaneNode,
    /// `second`.
    pub second: PaneNode,
}

impl Split {
    /// The split as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("dir", wire::code(self.dir)),
            ("ratio", wire::uint(u64::from(self.ratio))),
            ("first", self.first.to_value()),
            ("second", self.second.to_value()),
        ])
    }

    /// Reads a split from a CBOR map.
    pub fn from_value(value: &Value) -> Result<Split, DecodeError> {
        let r = MapReader::from_value(value)?;
        let ratio = wire::req_u64(&r, "ratio")?;
        let ratio = match u16::try_from(ratio) {
            Ok(ratio) if ratio >= 1 => ratio,
            _ => return Err(DecodeError::InvalidValue("ratio")),
        };
        Ok(Split {
            dir: wire::req_enum(&r, "dir")?,
            ratio,
            first: PaneNode::from_value(r.required("first")?, "first")?,
            second: PaneNode::from_value(r.required("second")?, "second")?,
        })
    }
}

/// A pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    /// `pane`: the id.
    pub id: PaneId,
    /// `title`.
    pub title: String,
    /// `cwd`: local directory; `None` if unknown or if the pane is SSH.
    pub cwd: Option<String>,
    /// `state`.
    pub state: PaneState,
    /// `exit`: exit code when `state` is `Exited`.
    pub exit: Option<i64>,
    /// `cols`.
    pub cols: u64,
    /// `rows`.
    pub rows: u64,
}

impl Pane {
    /// The pane as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.id)),
            ("title", wire::text(&self.title)),
            ("cwd", wire::nullable_text(&self.cwd)),
            ("state", wire::code(self.state)),
            (
                "exit",
                match self.exit {
                    Some(code) => wire::int(code),
                    None => Value::Null,
                },
            ),
            ("cols", wire::uint(self.cols)),
            ("rows", wire::uint(self.rows)),
        ])
    }

    /// Reads a pane from a CBOR map.
    pub fn from_value(value: &Value) -> Result<Pane, DecodeError> {
        let r = MapReader::from_value(value)?;
        let exit = r.required("exit")?;
        Ok(Pane {
            id: wire::req_u64(&r, "pane")?,
            title: wire::req_text(&r, "title")?,
            cwd: wire::req_nullable_text(&r, "cwd")?,
            state: wire::req_enum(&r, "state")?,
            exit: if exit.is_null() {
                None
            } else {
                Some(exit.as_i64()?)
            },
            cols: wire::req_u64(&r, "cols")?,
            rows: wire::req_u64(&r, "rows")?,
        })
    }
}

code_enum! {
    /// State of a pane (`pane-state`).
    PaneState {
        /// Running.
        Running = 0,
        /// The process ended.
        Exited = 1,
        /// Restored tab not started yet, or dormant SSH.
        NotStarted = 2,
        /// Connecting.
        Connecting = 3,
        /// Reconnecting after a drop.
        Reconnecting = 4,
        /// Disconnected.
        Disconnected = 5,
        /// Stopped at a question answered only on the desktop.
        AwaitingDesktop = 6,
    }
}

/// `profiles` (host to client).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profiles {
    /// `list`.
    pub list: Vec<Profile>,
    /// `default`: the id of the default profile.
    pub default: ProfileId,
}

impl Body for Profiles {
    fn to_value(&self) -> Value {
        wire::map(vec![
            (
                "list",
                Value::Array(self.list.iter().map(Profile::to_value).collect()),
            ),
            ("default", wire::text(&self.default)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Profiles {
            list: req_array(r, "list", Profile::from_value)?,
            default: wire::req_text(r, "default")?,
        })
    }
}

code_enum! {
    /// Kind of a profile (`profile.kind`).
    ProfileKind {
        /// Local shell.
        Local = 0,
        /// SSH connection.
        Ssh = 1,
        /// WSL.
        Wsl = 2,
        /// Anything else.
        Other = 3,
    }
}

/// A tab profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// `id`.
    pub id: ProfileId,
    /// `name`.
    pub name: String,
    /// `kind`.
    pub kind: ProfileKind,
}

impl Profile {
    /// The profile as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::text(&self.id)),
            ("name", wire::text(&self.name)),
            ("kind", wire::code(self.kind)),
        ])
    }

    /// Reads a profile from a CBOR map.
    pub fn from_value(value: &Value) -> Result<Profile, DecodeError> {
        let r = MapReader::from_value(value)?;
        Ok(Profile {
            id: wire::req_text(&r, "id")?,
            name: wire::req_text(&r, "name")?,
            kind: wire::req_enum(&r, "kind")?,
        })
    }
}

/// `theme` (host to client): the colors the client paints the grid with.
/// Colors are `0xRRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// `fg`.
    pub fg: u32,
    /// `bg`.
    pub bg: u32,
    /// `cursor`.
    pub cursor: u32,
    /// `selection_bg`.
    pub selection_bg: u32,
    /// `palette`: colors 0 to 15.
    pub palette: [u32; 16],
    /// `group_colors`: the group color palette, at least one color.
    pub group_colors: Vec<u32>,
}

impl Body for Theme {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("fg", rgb_value(self.fg)),
            ("bg", rgb_value(self.bg)),
            ("cursor", rgb_value(self.cursor)),
            ("selection_bg", rgb_value(self.selection_bg)),
            (
                "palette",
                Value::Array(self.palette.iter().map(|c| rgb_value(*c)).collect()),
            ),
            (
                "group_colors",
                Value::Array(self.group_colors.iter().map(|c| rgb_value(*c)).collect()),
            ),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        let palette = req_array(r, "palette", |v| rgb_from(v, "palette"))?;
        let palette: [u32; 16] = palette
            .try_into()
            .map_err(|_| DecodeError::InvalidValue("palette"))?;
        let group_colors = req_array(r, "group_colors", |v| rgb_from(v, "group_colors"))?;
        if group_colors.is_empty() {
            return Err(DecodeError::InvalidValue("group_colors"));
        }
        Ok(Theme {
            fg: rgb_from(r.required("fg")?, "fg")?,
            bg: rgb_from(r.required("bg")?, "bg")?,
            cursor: rgb_from(r.required("cursor")?, "cursor")?,
            selection_bg: rgb_from(r.required("selection_bg")?, "selection_bg")?,
            palette,
            group_colors,
        })
    }
}

/// `view-tab` (client to host): the tab the client is showing now, `None` when
/// it shows none. Information for the desktop; it never changes the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewTab {
    /// `tab`.
    pub tab: Option<TabId>,
}

impl Body for ViewTab {
    fn to_value(&self) -> Value {
        wire::map(vec![("tab", wire::nullable_uint(self.tab))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(ViewTab {
            tab: wire::req_nullable_u64(r, "tab")?,
        })
    }
}

/// Identifier of a saved named session (spec 03 §3, §4.5).
pub type SessionId = u64;

/// `list-sessions` (client to host): asks for the saved named sessions. The
/// host answers with `sessions` of the same `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListSessions {
    /// `id`: chosen by the client.
    pub id: RequestId,
}

impl Body for ListSessions {
    fn to_value(&self) -> Value {
        wire::map(vec![("id", wire::uint(self.id))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(ListSessions {
            id: wire::req_u64(r, "id")?,
        })
    }
}

code_enum! {
    /// Where the desktop restores a session to (`sessions.mode`).
    SessionsMode {
        /// Open a new window.
        NewWindow = 0,
        /// Append to a window that already exists.
        Append = 1,
    }
}

/// `sessions` (host to client): the saved named sessions, most recently saved
/// first. Sent only in answer to `list-sessions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sessions {
    /// `id`: the one of the `list-sessions`.
    pub id: RequestId,
    /// `mode`: the desktop preference on the destination of a restoration.
    pub mode: SessionsMode,
    /// `list`: empty when nothing is saved.
    pub list: Vec<NamedSession>,
}

impl Body for Sessions {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            ("mode", wire::code(self.mode)),
            (
                "list",
                Value::Array(self.list.iter().map(NamedSession::to_value).collect()),
            ),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Sessions {
            id: wire::req_u64(r, "id")?,
            mode: wire::req_enum(r, "mode")?,
            list: req_array(r, "list", NamedSession::from_value)?,
        })
    }
}

/// A saved named session in `sessions.list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSession {
    /// `session`.
    pub session: SessionId,
    /// `name`: as given on the desktop (the desktop label when unreadable).
    pub name: String,
    /// `readable`: `false` when the host cannot restore it.
    pub readable: bool,
}

impl NamedSession {
    /// The session as a CBOR map.
    pub fn to_value(&self) -> Value {
        wire::map(vec![
            ("session", wire::uint(self.session)),
            ("name", wire::text(&self.name)),
            ("readable", Value::Bool(self.readable)),
        ])
    }

    /// Reads a session from a CBOR map.
    pub fn from_value(value: &Value) -> Result<NamedSession, DecodeError> {
        let r = MapReader::from_value(value)?;
        Ok(NamedSession {
            session: wire::req_u64(&r, "session")?,
            name: wire::req_text(&r, "name")?,
            readable: wire::req_bool(&r, "readable")?,
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

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    fn roundtrip(message: Message, expected_hex: &str) {
        let bytes = encode(&message);
        assert_eq!(hex(&bytes), expected_hex);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
    }

    fn pane(id: u64) -> Pane {
        Pane {
            id,
            title: "a".to_owned(),
            cwd: None,
            state: PaneState::Running,
            exit: None,
            cols: 80,
            rows: 24,
        }
    }

    fn tab(id: u64, layout: PaneNode, focused: u64) -> Tab {
        Tab {
            id,
            title: "a".to_owned(),
            custom: false,
            profile: None,
            remote: None,
            activity: false,
            bell: false,
            layout,
            focused,
        }
    }

    fn canonical_workspace() -> Workspace {
        Workspace {
            rev: 1,
            windows: vec![Window {
                id: 1,
                title: "a".to_owned(),
                pinned: vec![],
                items: vec![Item::Tab(tab(2, PaneNode::Pane(pane(3)), 3))],
                active: Some(2),
            }],
        }
    }

    // The pane map, keys by encoded bytes (shorter first, then bytewise):
    // cwd, cols, exit, pane, rows, state, title.
    const PANE_HEX: &str = concat!(
        "a7",
        "63637764",     // "cwd"
        "f6",           // null
        "64636f6c73",   // "cols"
        "1850",         // 80
        "6465786974",   // "exit"
        "f6",           // null
        "6470616e65",   // "pane"
        "03",           // 3
        "64726f7773",   // "rows"
        "1818",         // 24
        "657374617465", // "state"
        "00",           // running
        "657469746c65", // "title"
        "6161",         // "a"
    );

    // Hand-computed. Tab keys: tab, bell, title, custom, layout, remote,
    // focused, profile, activity. Window keys: id, items, title, active,
    // pinned. Message keys: rev, windows.
    #[test]
    fn workspace_round_trip() {
        let tab_hex = format!(
            "{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
            "a9",
            "63746162",           // "tab"
            "02",                 // 2
            "6462656c6c",         // "bell"
            "f4",                 // false
            "657469746c65",       // "title"
            "6161",               // "a"
            "66637573746f6d",     // "custom"
            "f4",                 // false
            "666c61796f7574",     // "layout"
            PANE_HEX,             // the pane
            "6672656d6f7465",     // "remote"
            "f6",                 // null
            "67666f6375736564",   // "focused"
            "03",                 // 3
            "6770726f66696c65",   // "profile"
            "f6",                 // null
            "686163746976697479", // "activity"
            "f4",                 // false
        );
        let window_hex = format!(
            "{}{}{}{}{}{}{}{}{}{}{}{}",
            "a5",
            "626964",         // "id"
            "01",             // 1
            "656974656d73",   // "items"
            "81",             // array of 1
            tab_hex,          // the tab
            "657469746c65",   // "title"
            "6161",           // "a"
            "66616374697665", // "active"
            "02",             // 2
            "6670696e6e6564", // "pinned"
            "80",             // []
        );
        let expected = format!(
            "{}{}{}{}{}{}",
            "820aa2",
            "63726576",         // "rev"
            "01",               // 1
            "6777696e646f7773", // "windows"
            "81",               // array of 1
            window_hex,
        );
        roundtrip(Message::Workspace(canonical_workspace()), &expected);
    }

    #[test]
    fn workspace_complete_round_trip() {
        let split = PaneNode::Split(Box::new(Split {
            dir: SplitDir::Stacked,
            ratio: 65535,
            first: PaneNode::Pane(pane(3)),
            second: PaneNode::Split(Box::new(Split {
                dir: SplitDir::SideBySide,
                ratio: 1,
                first: PaneNode::Pane(Pane {
                    id: 4,
                    title: "ssh".to_owned(),
                    cwd: Some("/home/a".to_owned()),
                    state: PaneState::Exited,
                    exit: Some(-1),
                    cols: 120,
                    rows: 30,
                }),
                second: PaneNode::Pane(Pane {
                    state: PaneState::AwaitingDesktop,
                    ..pane(5)
                }),
            })),
        }));
        let mut full_tab = tab(2, split, 4);
        full_tab.custom = true;
        full_tab.profile = Some("auto:pwsh".to_owned());
        full_tab.remote = Some("me@host".to_owned());
        full_tab.activity = true;
        full_tab.bell = true;
        let message = Message::Workspace(Workspace {
            rev: 9_007_199_254_740_991,
            windows: vec![
                Window {
                    id: 1,
                    title: "a".to_owned(),
                    pinned: vec![tab(10, PaneNode::Pane(pane(11)), 11)],
                    items: vec![
                        Item::Tab(full_tab),
                        Item::Group(Group {
                            id: 6,
                            name: "work".to_owned(),
                            color: 2,
                            collapsed: true,
                            tabs: vec![
                                tab(7, PaneNode::Pane(pane(8)), 8),
                                tab(9, PaneNode::Pane(pane(12)), 12),
                            ],
                        }),
                    ],
                    active: None,
                },
                Window {
                    id: 2,
                    title: String::new(),
                    pinned: vec![],
                    items: vec![],
                    active: None,
                },
            ],
        });
        let bytes = encode(&message);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
    }

    fn tab_value() -> Value {
        tab(2, PaneNode::Pane(pane(3)), 3).to_value()
    }

    fn without(value: Value, key: &str) -> Value {
        match value {
            Value::Map(entries) => {
                Value::Map(entries.into_iter().filter(|(k, _)| k != key).collect())
            }
            other => other,
        }
    }

    fn with(value: Value, key: &str, new: Value) -> Value {
        match value {
            Value::Map(mut entries) => {
                entries.retain(|(k, _)| k != key);
                entries.push((key.to_owned(), new));
                Value::Map(entries)
            }
            other => other,
        }
    }

    fn workspace_with_tab(tab: Value) -> Vec<u8> {
        let window = wire::map(vec![
            ("id", wire::uint(1)),
            ("title", wire::text("a")),
            ("pinned", Value::Array(vec![])),
            ("items", Value::Array(vec![tab])),
            ("active", Value::Null),
        ]);
        let body = wire::map(vec![
            ("rev", wire::uint(1)),
            ("windows", Value::Array(vec![window])),
        ]);
        cbor::encode(&Value::Array(vec![wire::uint(10), body]))
    }

    fn decode_workspace(bytes: &[u8]) -> Result<Message, DecodeError> {
        decode(bytes, Direction::HostToClient)
    }

    #[test]
    fn workspace_missing_required_keys() {
        // Missing "rev".
        let bytes = unhex("820aa1" /* map of 1 */);
        assert_eq!(
            decode_workspace(&[bytes, unhex("6777696e646f777380")].concat()),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // A tab without "layout" or a pane without "exit".
        assert_eq!(
            decode_workspace(&workspace_with_tab(without(tab_value(), "layout"))),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        assert_eq!(
            decode_workspace(&workspace_with_tab(with(
                tab_value(),
                "layout",
                without(pane(3).to_value(), "exit")
            ))),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // `profile` is `T / null`: the key must be present.
        assert_eq!(
            decode_workspace(&workspace_with_tab(without(tab_value(), "profile"))),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
    }

    #[test]
    fn workspace_values_outside_the_domain() {
        let split = |ratio: u64| {
            Value::Map(
                match wire::map(vec![
                    ("dir", wire::uint(0)),
                    ("ratio", wire::uint(ratio)),
                    ("first", pane(3).to_value()),
                    ("second", pane(4).to_value()),
                ]) {
                    Value::Map(m) => m,
                    _ => unreachable!(),
                },
            )
        };
        for ratio in [0, 65536] {
            assert_eq!(
                decode_workspace(&workspace_with_tab(with(
                    tab_value(),
                    "layout",
                    split(ratio)
                ))),
                Err(DecodeError::InvalidValue("ratio")),
                "ratio {ratio}"
            );
        }
        assert!(
            decode_workspace(&workspace_with_tab(with(
                tab_value(),
                "layout",
                split(65535)
            )))
            .is_ok()
        );
        // pane-state 7.
        assert_eq!(
            decode_workspace(&workspace_with_tab(with(
                tab_value(),
                "layout",
                with(pane(3).to_value(), "state", wire::uint(7))
            ))),
            Err(DecodeError::InvalidValue("state"))
        );
        // A group with no tab.
        let group = |tabs: Vec<Value>| {
            wire::map(vec![
                ("group", wire::uint(6)),
                ("name", wire::text("g")),
                ("color", wire::uint(0)),
                ("collapsed", Value::Bool(false)),
                ("tabs", Value::Array(tabs)),
            ])
        };
        assert_eq!(
            decode_workspace(&workspace_with_tab(group(vec![]))),
            Err(DecodeError::InvalidValue("tabs"))
        );
        assert!(decode_workspace(&workspace_with_tab(group(vec![tab_value()]))).is_ok());
        // An item that is both a tab and a group, or neither.
        assert_eq!(
            decode_workspace(&workspace_with_tab(with(
                tab_value(),
                "group",
                wire::uint(1)
            ))),
            Err(DecodeError::InvalidValue("item"))
        );
        assert_eq!(
            decode_workspace(&workspace_with_tab(without(tab_value(), "tab"))),
            Err(DecodeError::InvalidValue("item"))
        );
        // A pane-node that is both a split and a pane, or neither.
        assert_eq!(
            decode_workspace(&workspace_with_tab(with(
                tab_value(),
                "layout",
                with(pane(3).to_value(), "dir", wire::uint(0))
            ))),
            Err(DecodeError::InvalidValue("layout"))
        );
        assert_eq!(
            decode_workspace(&workspace_with_tab(with(
                tab_value(),
                "layout",
                wire::map(vec![])
            ))),
            Err(DecodeError::InvalidValue("layout"))
        );
    }

    #[test]
    fn workspace_ignores_unknown_keys() {
        let value = with(tab_value(), "zzz", Value::Bool(true));
        let value = with(
            value,
            "layout",
            with(pane(3).to_value(), "zzz", wire::uint(1)),
        );
        let message = decode_workspace(&workspace_with_tab(value)).unwrap();
        let mut expected = canonical_workspace();
        expected.windows[0].active = None; // `workspace_with_tab` writes null
        assert_eq!(message, Message::Workspace(expected));
    }

    fn profiles() -> Profiles {
        Profiles {
            list: vec![Profile {
                id: "auto:pwsh".to_owned(),
                name: "PowerShell".to_owned(),
                kind: ProfileKind::Local,
            }],
            default: "auto:pwsh".to_owned(),
        }
    }

    // Hand-computed. Message keys: list, default. Profile keys: id, kind,
    // name ("kind" 64 6b.. sorts before "name" 64 6e..).
    #[test]
    fn profiles_round_trip() {
        roundtrip(
            Message::Profiles(profiles()),
            concat!(
                "820ba2",
                "646c697374",             // "list"
                "81a3",                   // array of 1, map of 3
                "626964",                 // "id"
                "696175746f3a70777368",   // "auto:pwsh"
                "646b696e64",             // "kind"
                "00",                     // local
                "646e616d65",             // "name"
                "6a506f7765725368656c6c", // "PowerShell"
                "6764656661756c74",       // "default"
                "696175746f3a70777368",   // "auto:pwsh"
            ),
        );
        let empty = Message::Profiles(Profiles {
            list: vec![],
            default: String::new(),
        });
        let bytes = encode(&empty);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), empty);
        let all_kinds = Message::Profiles(Profiles {
            list: ProfileKind::ALL
                .iter()
                .map(|kind| Profile {
                    id: format!("p{}", kind.code()),
                    name: "n".to_owned(),
                    kind: *kind,
                })
                .collect(),
            default: "p0".to_owned(),
        });
        let bytes = encode(&all_kinds);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), all_kinds);
    }

    #[test]
    fn profiles_errors() {
        let body = |entries: Vec<(&str, Value)>| {
            cbor::encode(&Value::Array(vec![wire::uint(11), wire::map(entries)]))
        };
        let profile = |kind: u64| {
            Value::Array(vec![wire::map(vec![
                ("id", wire::text("x")),
                ("name", wire::text("y")),
                ("kind", wire::uint(kind)),
            ])])
        };
        // Missing "default".
        assert_eq!(
            decode(&body(vec![("list", profile(0))]), Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // Kind 4.
        assert_eq!(
            decode(
                &body(vec![("list", profile(4)), ("default", wire::text("x"))]),
                Direction::HostToClient
            ),
            Err(DecodeError::InvalidValue("kind"))
        );
        // Unknown key ignored.
        assert!(
            decode(
                &body(vec![
                    ("list", profile(3)),
                    ("default", wire::text("x")),
                    ("zzz", Value::Null),
                ]),
                Direction::HostToClient
            )
            .is_ok()
        );
    }

    fn theme() -> Theme {
        Theme {
            fg: 0xd7dce3,
            bg: 0x15181d,
            cursor: 0x5ed3bc,
            selection_bg: 0x2b3a4a,
            palette: std::array::from_fn(|i| i as u32),
            group_colors: vec![0xff0000],
        }
    }

    // 82 0d = [13, ...], a1 = map of 1, 63 746162 = "tab".
    #[test]
    fn view_tab_round_trip() {
        for (tab, tail) in [
            (Some(5), "05"),
            (None, "f6"),
            (Some((1u64 << 53) - 1), "1b001fffffffffffff"),
        ] {
            let message = Message::ViewTab(ViewTab { tab });
            let bytes = encode(&message);
            assert_eq!(hex(&bytes), format!("820da163746162{tail}"));
            assert_eq!(decode(&bytes, Direction::ClientToHost).unwrap(), message);
        }
    }

    #[test]
    fn view_tab_is_only_for_the_client_to_send() {
        let bytes = unhex("820da16374616205");
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::WrongDirection(13))
        );
        assert!(Message::ViewTab(ViewTab { tab: None }).can_be_sent_by(Direction::ClientToHost));
        assert!(!Message::ViewTab(ViewTab { tab: None }).can_be_sent_by(Direction::HostToClient));
    }

    #[test]
    fn view_tab_errors_and_unknown_keys() {
        // No "tab": the key is required even though its value may be null.
        assert_eq!(
            decode(&unhex("820da0"), Direction::ClientToHost),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // "tab" of the wrong type.
        assert!(decode(&unhex("820da1637461626161"), Direction::ClientToHost).is_err());
        // An unknown key is ignored ("zz" sorts before "tab": shorter first).
        assert_eq!(
            decode(
                &unhex(concat!("820da262", "7a7a", "f5", "63746162", "05")),
                Direction::ClientToHost
            ),
            Ok(Message::ViewTab(ViewTab { tab: Some(5) }))
        );
    }

    // 82 0e = [14, ...], a1 = map of 1, 62 6964 = "id".
    #[test]
    fn list_sessions_round_trip() {
        for (id, tail) in [
            (5, "05"),
            (0, "00"),
            ((1u64 << 53) - 1, "1b001fffffffffffff"),
        ] {
            let message = Message::ListSessions(ListSessions { id });
            let bytes = encode(&message);
            assert_eq!(hex(&bytes), format!("820ea1626964{tail}"));
            assert_eq!(decode(&bytes, Direction::ClientToHost).unwrap(), message);
        }
    }

    #[test]
    fn list_sessions_direction_and_errors() {
        let bytes = unhex("820ea162696405");
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::WrongDirection(14))
        );
        let message = Message::ListSessions(ListSessions { id: 1 });
        assert!(message.can_be_sent_by(Direction::ClientToHost));
        assert!(!message.can_be_sent_by(Direction::HostToClient));
        // No "id".
        assert_eq!(
            decode(&unhex("820ea0"), Direction::ClientToHost),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // "id" of the wrong type.
        assert!(decode(&unhex("820ea16269646161"), Direction::ClientToHost).is_err());
        // An unknown key is ignored ("zz" sorts after "id": same length,
        // bytewise).
        assert_eq!(
            decode(
                &unhex(concat!("820ea2", "626964", "05", "627a7a", "f5")),
                Direction::ClientToHost
            ),
            Ok(Message::ListSessions(ListSessions { id: 5 }))
        );
    }

    fn named(session: u64, name: &str, readable: bool) -> NamedSession {
        NamedSession {
            session,
            name: name.to_owned(),
            readable,
        }
    }

    // Keys sort by length, then bytes: id, list, mode; and in an entry:
    // name, session, readable.
    #[test]
    fn sessions_round_trip() {
        roundtrip(
            Message::Sessions(Sessions {
                id: 1,
                mode: SessionsMode::Append,
                list: vec![named(2, "a", true)],
            }),
            concat!(
                "820fa3",
                "626964",
                "01",
                "646c697374", // "list"
                "81a3",
                "646e616d65",       // "name"
                "6161",             // "a"
                "6773657373696f6e", // "session"
                "02",
                "687265616461626c65", // "readable"
                "f5",
                "646d6f6465", // "mode"
                "01",
            ),
        );
        // Minimal: an empty list is not an error.
        roundtrip(
            Message::Sessions(Sessions {
                id: 0,
                mode: SessionsMode::NewWindow,
                list: Vec::new(),
            }),
            concat!(
                "820fa3",
                "626964",
                "00",
                "646c697374",
                "80",
                "646d6f6465",
                "00"
            ),
        );
        // Complete, with an unreadable entry.
        let message = Message::Sessions(Sessions {
            id: (1 << 53) - 1,
            mode: SessionsMode::Append,
            list: vec![named((1 << 53) - 1, "dev", true), named(900, "", false)],
        });
        let bytes = encode(&message);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
        assert!(hex(&bytes).contains("f4"));
    }

    #[test]
    fn sessions_direction_and_errors() {
        let message = Message::Sessions(Sessions {
            id: 1,
            mode: SessionsMode::NewWindow,
            list: Vec::new(),
        });
        let bytes = encode(&message);
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(15))
        );
        assert!(message.can_be_sent_by(Direction::HostToClient));
        assert!(!message.can_be_sent_by(Direction::ClientToHost));
        // Missing "mode", "list" and "id".
        for text in [
            concat!("820fa2", "626964", "01", "646c697374", "80"),
            concat!("820fa2", "626964", "01", "646d6f6465", "00"),
            concat!("820fa2", "646c697374", "80", "646d6f6465", "00"),
        ] {
            assert_eq!(
                decode(&unhex(text), Direction::HostToClient),
                Err(DecodeError::Cbor(CborError::MissingKey))
            );
        }
        // "mode" outside the domain.
        assert_eq!(
            decode(
                &unhex(concat!(
                    "820fa3",
                    "626964",
                    "01",
                    "646c697374",
                    "80",
                    "646d6f6465",
                    "02"
                )),
                Direction::HostToClient
            ),
            Err(DecodeError::InvalidValue("mode"))
        );
        // An entry without "readable": the key is required in the nested map.
        assert_eq!(
            decode(
                &unhex(concat!(
                    "820fa3",
                    "626964",
                    "01",
                    "646c697374",
                    "81a2",
                    "646e616d65",
                    "6161",
                    "6773657373696f6e",
                    "02",
                    "646d6f6465",
                    "00"
                )),
                Direction::HostToClient
            ),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // "readable" of the wrong type.
        assert_eq!(
            decode(
                &unhex(concat!(
                    "820fa3",
                    "626964",
                    "01",
                    "646c697374",
                    "81a3",
                    "646e616d65",
                    "6161",
                    "6773657373696f6e",
                    "02",
                    "687265616461626c65",
                    "01",
                    "646d6f6465",
                    "00"
                )),
                Direction::HostToClient
            ),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // "list" that is not an array.
        assert!(
            decode(
                &unhex(concat!(
                    "820fa3",
                    "626964",
                    "01",
                    "646c697374",
                    "01",
                    "646d6f6465",
                    "00"
                )),
                Direction::HostToClient
            )
            .is_err()
        );
    }

    #[test]
    fn sessions_ignore_unknown_keys_in_an_entry() {
        // "zz" (62 7a7a) sorts before "name" (64 ...): shorter first.
        let bytes = unhex(concat!(
            "820fa3",
            "626964",
            "01",
            "646c697374",
            "81a4",
            "627a7a",
            "f5",
            "646e616d65",
            "6161",
            "6773657373696f6e",
            "02",
            "687265616461626c65",
            "f5",
            "646d6f6465",
            "00"
        ));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Ok(Message::Sessions(Sessions {
                id: 1,
                mode: SessionsMode::NewWindow,
                list: vec![named(2, "a", true)],
            }))
        );
    }

    // Hand-computed. Keys: bg, fg, cursor, palette, group_colors,
    // selection_bg. 0x15181d needs 3 bytes: 1a 0015181d.
    #[test]
    fn theme_round_trip() {
        roundtrip(
            Message::Theme(theme()),
            concat!(
                "820ca6",
                "626267",                           // "bg"
                "1a0015181d",                       // 0x15181d
                "626667",                           // "fg"
                "1a00d7dce3",                       // 0xd7dce3
                "66637572736f72",                   // "cursor"
                "1a005ed3bc",                       // 0x5ed3bc
                "6770616c65747465",                 // "palette"
                "90",                               // array of 16
                "000102030405060708090a0b0c0d0e0f", // 0 .. 15
                "6c67726f75705f636f6c6f7273",       // "group_colors"
                "81",                               // array of 1
                "1a00ff0000",                       // 0xff0000
                "6c73656c656374696f6e5f6267",       // "selection_bg"
                "1a002b3a4a",                       // 0x2b3a4a
            ),
        );
        let mut full = theme();
        full.fg = 0xFF_FFFF;
        full.bg = 0;
        full.palette = [0xFF_FFFF; 16];
        full.group_colors = vec![1, 2, 3, 0xab_cdef];
        let message = Message::Theme(full);
        let bytes = encode(&message);
        assert_eq!(decode(&bytes, Direction::HostToClient).unwrap(), message);
    }

    #[test]
    fn theme_errors() {
        let entries = |palette: Vec<Value>, colors: Vec<Value>, fg: u64| {
            cbor::encode(&Value::Array(vec![
                wire::uint(12),
                wire::map(vec![
                    ("fg", wire::uint(fg)),
                    ("bg", wire::uint(0)),
                    ("cursor", wire::uint(0)),
                    ("selection_bg", wire::uint(0)),
                    ("palette", Value::Array(palette)),
                    ("group_colors", Value::Array(colors)),
                ]),
            ]))
        };
        let sixteen = || (0..16).map(wire::uint).collect::<Vec<_>>();
        let one = || vec![wire::uint(1)];
        assert!(
            decode(
                &entries(sixteen(), one(), 0xFF_FFFF),
                Direction::HostToClient
            )
            .is_ok()
        );
        // rgb above 0xFFFFFF in a scalar and in a list.
        assert_eq!(
            decode(
                &entries(sixteen(), one(), 0x100_0000),
                Direction::HostToClient
            ),
            Err(DecodeError::InvalidValue("fg"))
        );
        let mut palette = sixteen();
        palette[5] = wire::uint(0x100_0000);
        assert_eq!(
            decode(&entries(palette, one(), 0), Direction::HostToClient),
            Err(DecodeError::InvalidValue("palette"))
        );
        // palette with 15 or 17 colors.
        for size in [15, 17] {
            let palette = (0..size).map(wire::uint).collect();
            assert_eq!(
                decode(&entries(palette, one(), 0), Direction::HostToClient),
                Err(DecodeError::InvalidValue("palette")),
                "size {size}"
            );
        }
        // Empty group_colors.
        assert_eq!(
            decode(&entries(sixteen(), vec![], 0), Direction::HostToClient),
            Err(DecodeError::InvalidValue("group_colors"))
        );
        // Missing key: no "fg".
        let bytes = cbor::encode(&Value::Array(vec![
            wire::uint(12),
            wire::map(vec![
                ("bg", wire::uint(0)),
                ("cursor", wire::uint(0)),
                ("selection_bg", wire::uint(0)),
                ("palette", Value::Array(sixteen())),
                ("group_colors", Value::Array(one())),
            ]),
        ]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::MissingKey))
        );
        // Unknown key ignored.
        let bytes = cbor::encode(&Value::Array(vec![
            wire::uint(12),
            wire::map(vec![
                ("fg", wire::uint(1)),
                ("bg", wire::uint(2)),
                ("cursor", wire::uint(3)),
                ("selection_bg", wire::uint(4)),
                ("palette", Value::Array(sixteen())),
                ("group_colors", Value::Array(one())),
                ("zzz", Value::Bool(true)),
            ]),
        ]));
        assert!(decode(&bytes, Direction::HostToClient).is_ok());
    }
}
