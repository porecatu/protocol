// SPDX-License-Identifier: GPL-3.0-or-later
//! Terminal grid: subscriptions, snapshots, diffs, flow control and
//! scrollback (spec 03 §5).
//!
//! Types that carry terminal text (`Span`, and through it every line and
//! grid message) have a `Debug` that shows sizes, never the text.

use std::fmt;

use super::wire;
use super::{DecodeError, PaneId, RequestId};
use crate::cbor::{CborError, MapReader, Value, expect_array};

/// Largest `count` of a `scrollback-request`.
pub const MAX_SCROLLBACK_COUNT: u64 = 500;

/// Style flags (`style.flags`).
pub mod style_flags {
    /// Bold.
    pub const BOLD: u64 = 0x01;
    /// Italic.
    pub const ITALIC: u64 = 0x02;
    /// Underline.
    pub const UNDERLINE: u64 = 0x04;
    /// Inverse.
    pub const INVERSE: u64 = 0x08;
    /// Dim.
    pub const DIM: u64 = 0x10;
    /// Strikethrough.
    pub const STRIKETHROUGH: u64 = 0x20;
    /// Hidden.
    pub const HIDDEN: u64 = 0x40;
}

/// `subscribe` (client to host).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscribe {
    /// `pane`.
    pub pane: PaneId,
}

impl wire::Body for Subscribe {
    fn to_value(&self) -> Value {
        wire::map(vec![("pane", wire::uint(self.pane))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Subscribe {
            pane: wire::req_u64(r, "pane")?,
        })
    }
}

/// `unsubscribe` (client to host).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsubscribe {
    /// `pane`.
    pub pane: PaneId,
}

impl wire::Body for Unsubscribe {
    fn to_value(&self) -> Value {
        wire::map(vec![("pane", wire::uint(self.pane))])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(Unsubscribe {
            pane: wire::req_u64(r, "pane")?,
        })
    }
}

code_enum! {
    /// Why a subscription ended (`subscription-ended.reason`).
    SubscriptionEndReason {
        /// The pane closed.
        PaneClosed = 0,
        /// The host limit was reached.
        HostLimit = 1,
    }
}

/// `subscription-ended` (host to client).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionEnded {
    /// `pane`.
    pub pane: PaneId,
    /// `reason`.
    pub reason: SubscriptionEndReason,
}

impl wire::Body for SubscriptionEnded {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("reason", wire::code(self.reason)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(SubscriptionEnded {
            pane: wire::req_u64(r, "pane")?,
            reason: wire::req_enum(r, "reason")?,
        })
    }
}

/// A color, not resolved: the client resolves it with the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// `null`: the default foreground or background of the theme.
    Default,
    /// `[0, index]`: indexed color, `0..=255`.
    Indexed(u8),
    /// `[1, rgb]`: direct color, `0xRRGGBB`.
    Rgb(u32),
}

impl Color {
    /// The wire form: `null`, `[0, index]` or `[1, rgb]`.
    pub(crate) fn to_value(self) -> Value {
        match self {
            Color::Default => Value::Null,
            Color::Indexed(index) => Value::Array(vec![wire::uint(0), wire::uint(index.into())]),
            Color::Rgb(rgb) => Value::Array(vec![wire::uint(1), wire::uint(rgb.into())]),
        }
    }

    /// Reads a color; an unknown form, an index above 255 or an rgb above
    /// `0xFFFFFF` is outside the domain.
    pub(crate) fn from_value(value: &Value) -> Result<Color, DecodeError> {
        if value.is_null() {
            return Ok(Color::Default);
        }
        let items = expect_array(value, 2)?;
        let form = items[0].as_u64()?;
        let number = items[1].as_u64()?;
        match form {
            0 => u8::try_from(number)
                .map(Color::Indexed)
                .map_err(|_| DecodeError::InvalidValue("color")),
            1 => {
                if number <= 0xFF_FFFF {
                    Ok(Color::Rgb(number as u32))
                } else {
                    Err(DecodeError::InvalidValue("color"))
                }
            }
            _ => Err(DecodeError::InvalidValue("color")),
        }
    }
}

/// A style, `[fg, bg, flags]`. Spans refer to styles by index into the
/// `styles` table of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Foreground.
    pub fg: Color,
    /// Background.
    pub bg: Color,
    /// Bit set of [`style_flags`].
    pub flags: u64,
}

impl Style {
    pub(crate) fn to_value(self) -> Value {
        Value::Array(vec![
            self.fg.to_value(),
            self.bg.to_value(),
            wire::uint(self.flags),
        ])
    }

    pub(crate) fn from_value(value: &Value) -> Result<Style, DecodeError> {
        let items = expect_array(value, 3)?;
        Ok(Style {
            fg: Color::from_value(&items[0])?,
            bg: Color::from_value(&items[1])?,
            flags: items[2].as_u64()?,
        })
    }
}

/// The text of a span (spec 03 section 5.2): the wire form says whether it
/// is one code point per cell or a single grapheme in one cell.
#[derive(Clone, PartialEq, Eq)]
pub enum SpanText {
    /// `tstr`: a plain span, one code point per cell. The host does not put a
    /// grapheme of more than one code point here.
    Plain(String),
    /// `[tstr]`: one single grapheme of several code points in one cell. The
    /// string is never empty.
    Grapheme(String),
}

impl SpanText {
    /// The text, whichever the form.
    pub fn as_str(&self) -> &str {
        match self {
            SpanText::Plain(text) | SpanText::Grapheme(text) => text,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            SpanText::Plain(text) => wire::text(text),
            SpanText::Grapheme(text) => Value::Array(vec![wire::text(text)]),
        }
    }

    /// A text is a plain span; an array of exactly one non-empty text is a
    /// grapheme span; an empty array, one with 2 or more elements or an
    /// empty text inside is outside the domain.
    fn from_value(value: &Value) -> Result<SpanText, DecodeError> {
        match value {
            Value::Text(text) => Ok(SpanText::Plain(text.clone())),
            Value::Array(items) => match items.as_slice() {
                [only] => {
                    let text = only.as_text()?;
                    if text.is_empty() {
                        Err(DecodeError::InvalidValue("text"))
                    } else {
                        Ok(SpanText::Grapheme(text.to_owned()))
                    }
                }
                _ => Err(DecodeError::InvalidValue("text")),
            },
            _ => Err(DecodeError::Cbor(CborError::WrongType)),
        }
    }
}

impl fmt::Debug for SpanText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            SpanText::Plain(_) => "Plain",
            SpanText::Grapheme(_) => "Grapheme",
        };
        f.debug_struct(kind)
            .field("text_len", &self.as_str().len())
            .finish()
    }
}

/// A run of cells with one style, `[col, width, style, text]`.
#[derive(Clone, PartialEq, Eq)]
pub struct Span {
    /// First column.
    pub col: u64,
    /// Width in cells of each grapheme: 1, or 2 for wide ones.
    pub width: u8,
    /// Index into the `styles` table of the message.
    pub style: u64,
    /// The text.
    pub text: SpanText,
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Span")
            .field("col", &self.col)
            .field("width", &self.width)
            .field("style", &self.style)
            .field("text_len", &self.text.as_str().len())
            .finish()
    }
}

impl Span {
    pub(crate) fn to_value(&self) -> Value {
        Value::Array(vec![
            wire::uint(self.col),
            wire::uint(self.width.into()),
            wire::uint(self.style),
            self.text.to_value(),
        ])
    }

    pub(crate) fn from_value(value: &Value) -> Result<Span, DecodeError> {
        let items = expect_array(value, 4)?;
        let width = match items[1].as_u64()? {
            1 => 1,
            2 => 2,
            _ => return Err(DecodeError::InvalidValue("width")),
        };
        Ok(Span {
            col: items[0].as_u64()?,
            width,
            style: items[2].as_u64()?,
            text: SpanText::from_value(&items[3])?,
        })
    }
}

/// A line, `[wrapped, spans]`. A cell with no span is a space with the
/// default style; an empty line is `[false, []]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The line continues on the next one (soft wrap).
    pub wrapped: bool,
    /// Non-overlapping spans in column order.
    pub spans: Vec<Span>,
}

impl Line {
    pub(crate) fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Bool(self.wrapped),
            Value::Array(self.spans.iter().map(Span::to_value).collect()),
        ])
    }

    pub(crate) fn from_value(value: &Value) -> Result<Line, DecodeError> {
        let items = expect_array(value, 2)?;
        let spans = items[1]
            .as_array()?
            .iter()
            .map(Span::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Line {
            wrapped: items[0].as_bool()?,
            spans,
        })
    }
}

code_enum! {
    /// Cursor shape (`cursor.shape`).
    CursorShape {
        /// Block.
        Block = 0,
        /// Underline.
        Underline = 1,
        /// Vertical bar.
        Beam = 2,
    }
}

/// The cursor of a grid message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// `row`.
    pub row: u64,
    /// `col`.
    pub col: u64,
    /// `shape`.
    pub shape: CursorShape,
    /// `visible`.
    pub visible: bool,
}

impl Cursor {
    pub(crate) fn to_value(self) -> Value {
        wire::map(vec![
            ("row", wire::uint(self.row)),
            ("col", wire::uint(self.col)),
            ("shape", wire::code(self.shape)),
            ("visible", Value::Bool(self.visible)),
        ])
    }

    pub(crate) fn from_value(value: &Value) -> Result<Cursor, DecodeError> {
        let r = MapReader::from_value(value)?;
        Ok(Cursor {
            row: wire::req_u64(&r, "row")?,
            col: wire::req_u64(&r, "col")?,
            shape: wire::req_enum(&r, "shape")?,
            visible: wire::req_bool(&r, "visible")?,
        })
    }
}

fn styles_to_value(styles: &[Style]) -> Value {
    Value::Array(styles.iter().map(|s| s.to_value()).collect())
}

fn lines_to_value(lines: &[Line]) -> Value {
    Value::Array(lines.iter().map(Line::to_value).collect())
}

fn req_styles(r: &MapReader<'_>) -> Result<Vec<Style>, DecodeError> {
    r.required("styles")?
        .as_array()?
        .iter()
        .map(Style::from_value)
        .collect()
}

fn req_lines(r: &MapReader<'_>) -> Result<Vec<Line>, DecodeError> {
    r.required("lines")?
        .as_array()?
        .iter()
        .map(Line::from_value)
        .collect()
}

/// `grid-snapshot` (host to client): the whole screen of a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSnapshot {
    /// `pane`.
    pub pane: PaneId,
    /// `seq`: monotonic per subscription, starting at 0.
    pub seq: u64,
    /// `cols`.
    pub cols: u64,
    /// `rows`.
    pub rows: u64,
    /// `history`: scrollback lines available above the screen.
    pub history: u64,
    /// `dropped`: lines already dropped from the top of the pane's history
    /// (spec 03 section 5.5).
    pub dropped: u64,
    /// `styles`.
    pub styles: Vec<Style>,
    /// `lines`: exactly `rows` lines, top to bottom.
    pub lines: Vec<Line>,
    /// `cursor`.
    pub cursor: Cursor,
    /// `alt`: alternate screen active.
    pub alt: bool,
}

impl wire::Body for GridSnapshot {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("seq", wire::uint(self.seq)),
            ("cols", wire::uint(self.cols)),
            ("rows", wire::uint(self.rows)),
            ("history", wire::uint(self.history)),
            ("dropped", wire::uint(self.dropped)),
            ("styles", styles_to_value(&self.styles)),
            ("lines", lines_to_value(&self.lines)),
            ("cursor", self.cursor.to_value()),
            ("alt", Value::Bool(self.alt)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(GridSnapshot {
            pane: wire::req_u64(r, "pane")?,
            seq: wire::req_u64(r, "seq")?,
            cols: wire::req_u64(r, "cols")?,
            rows: wire::req_u64(r, "rows")?,
            history: wire::req_u64(r, "history")?,
            dropped: wire::req_u64(r, "dropped")?,
            styles: req_styles(r)?,
            lines: req_lines(r)?,
            cursor: Cursor::from_value(r.required("cursor")?)?,
            alt: wire::req_bool(r, "alt")?,
        })
    }
}

/// `grid-diff` (host to client): only the lines that changed since the
/// previous grid message of the same subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridDiff {
    /// `pane`.
    pub pane: PaneId,
    /// `seq`: previous message's `seq` plus one.
    pub seq: u64,
    /// `history`.
    pub history: u64,
    /// `dropped`.
    pub dropped: u64,
    /// `styles`.
    pub styles: Vec<Style>,
    /// `lines`: `[row, line]` pairs.
    pub lines: Vec<(u64, Line)>,
    /// `cursor`.
    pub cursor: Cursor,
    /// `alt`.
    pub alt: bool,
}

impl wire::Body for GridDiff {
    fn to_value(&self) -> Value {
        let lines = self
            .lines
            .iter()
            .map(|(row, line)| Value::Array(vec![wire::uint(*row), line.to_value()]))
            .collect();
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("seq", wire::uint(self.seq)),
            ("history", wire::uint(self.history)),
            ("dropped", wire::uint(self.dropped)),
            ("styles", styles_to_value(&self.styles)),
            ("lines", Value::Array(lines)),
            ("cursor", self.cursor.to_value()),
            ("alt", Value::Bool(self.alt)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        let lines = r
            .required("lines")?
            .as_array()?
            .iter()
            .map(|entry| {
                let pair = expect_array(entry, 2)?;
                Ok((pair[0].as_u64()?, Line::from_value(&pair[1])?))
            })
            .collect::<Result<Vec<_>, DecodeError>>()?;
        Ok(GridDiff {
            pane: wire::req_u64(r, "pane")?,
            seq: wire::req_u64(r, "seq")?,
            history: wire::req_u64(r, "history")?,
            dropped: wire::req_u64(r, "dropped")?,
            styles: req_styles(r)?,
            lines,
            cursor: Cursor::from_value(r.required("cursor")?)?,
            alt: wire::req_bool(r, "alt")?,
        })
    }
}

/// `grid-ack` (client to host).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridAck {
    /// `pane`.
    pub pane: PaneId,
    /// `seq`: the message processed.
    pub seq: u64,
}

impl wire::Body for GridAck {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("pane", wire::uint(self.pane)),
            ("seq", wire::uint(self.seq)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(GridAck {
            pane: wire::req_u64(r, "pane")?,
            seq: wire::req_u64(r, "seq")?,
        })
    }
}

/// `scrollback-request` (client to host).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbackRequest {
    /// `id`.
    pub id: RequestId,
    /// `pane`.
    pub pane: PaneId,
    /// `offset`: 0 is the line right above the top of the screen.
    pub offset: u64,
    /// `count`: at most [`MAX_SCROLLBACK_COUNT`].
    pub count: u64,
}

impl wire::Body for ScrollbackRequest {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            ("pane", wire::uint(self.pane)),
            ("offset", wire::uint(self.offset)),
            ("count", wire::uint(self.count)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        let count = wire::req_u64(r, "count")?;
        if count > MAX_SCROLLBACK_COUNT {
            return Err(DecodeError::InvalidValue("count"));
        }
        Ok(ScrollbackRequest {
            id: wire::req_u64(r, "id")?,
            pane: wire::req_u64(r, "pane")?,
            offset: wire::req_u64(r, "offset")?,
            count,
        })
    }
}

/// `scrollback-page` (host to client).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackPage {
    /// `id`: of the request.
    pub id: RequestId,
    /// `pane`.
    pub pane: PaneId,
    /// `history`: lines available when answering.
    pub history: u64,
    /// `dropped`: the same counter as in the grid messages, when answering.
    pub dropped: u64,
    /// `offset`.
    pub offset: u64,
    /// `styles`.
    pub styles: Vec<Style>,
    /// `lines`: top to bottom; the last one is the line at `offset`.
    pub lines: Vec<Line>,
}

impl wire::Body for ScrollbackPage {
    fn to_value(&self) -> Value {
        wire::map(vec![
            ("id", wire::uint(self.id)),
            ("pane", wire::uint(self.pane)),
            ("history", wire::uint(self.history)),
            ("dropped", wire::uint(self.dropped)),
            ("offset", wire::uint(self.offset)),
            ("styles", styles_to_value(&self.styles)),
            ("lines", lines_to_value(&self.lines)),
        ])
    }

    fn from_map(r: &MapReader<'_>) -> Result<Self, DecodeError> {
        Ok(ScrollbackPage {
            id: wire::req_u64(r, "id")?,
            pane: wire::req_u64(r, "pane")?,
            history: wire::req_u64(r, "history")?,
            dropped: wire::req_u64(r, "dropped")?,
            offset: wire::req_u64(r, "offset")?,
            styles: req_styles(r)?,
            lines: req_lines(r)?,
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

    /// Encodes `message`, checks the hex written by hand and decodes it back.
    fn roundtrip(message: Message, from: Direction, expected_hex: &str) {
        let bytes = encode(&message);
        assert_eq!(hex(&bytes), expected_hex);
        assert_eq!(decode(&bytes, from).unwrap(), message);
    }

    fn span(col: u64, width: u8, style: u64, text: &str) -> Span {
        Span {
            col,
            width,
            style,
            text: SpanText::Plain(text.to_owned()),
        }
    }

    fn grapheme(col: u64, width: u8, style: u64, text: &str) -> Span {
        Span {
            col,
            width,
            style,
            text: SpanText::Grapheme(text.to_owned()),
        }
    }

    // "dropped" is 7 bytes, so its key (67 + text) sorts before "history".
    const DROPPED_KEY: &str = "6764726f70706564";

    fn plain_style() -> Style {
        Style {
            fg: Color::Default,
            bg: Color::Default,
            flags: 0,
        }
    }

    fn cursor() -> Cursor {
        Cursor {
            row: 0,
            col: 2,
            shape: CursorShape::Block,
            visible: true,
        }
    }

    // a4 = map of 4; keys by encoded bytes: "col", "row" (3 bytes, c < r),
    // "shape" (5), "visible" (7).
    const CURSOR_HEX: &str = concat!(
        "a4",
        "63636f6c",
        "02", // "col": 2
        "63726f77",
        "00", // "row": 0
        "657368617065",
        "00", // "shape": block
        "6776697369626c65",
        "f5", // "visible": true
    );

    // Keys are ordered by their encoded bytes: shorter key first, then
    // bytewise. a1 = map of 1; 64 70616e65 = "pane".
    #[test]
    fn subscribe_and_unsubscribe_round_trip() {
        roundtrip(
            Message::Subscribe(Subscribe { pane: 7 }),
            Direction::ClientToHost,
            "8214a16470616e6507",
        );
        roundtrip(
            Message::Unsubscribe(Unsubscribe { pane: 7 }),
            Direction::ClientToHost,
            "8215a16470616e6507",
        );
    }

    // 82 18 18 = [24, ...]; "seq" (63..) sorts before "pane" (64..).
    #[test]
    fn grid_ack_round_trip() {
        roundtrip(
            Message::GridAck(GridAck { pane: 7, seq: 3 }),
            Direction::ClientToHost,
            "821818a263736571036470616e6507",
        );
    }

    // 82 18 1b = [27, ...]; "pane" (64..) before "reason" (66..).
    #[test]
    fn subscription_ended_round_trip() {
        roundtrip(
            Message::SubscriptionEnded(SubscriptionEnded {
                pane: 7,
                reason: SubscriptionEndReason::HostLimit,
            }),
            Direction::HostToClient,
            concat!(
                "82181ba2",
                "6470616e65",
                "07", // "pane": 7
                "66726561736f6e",
                "01", // "reason": host-limit
            ),
        );
        assert_eq!(
            decode(
                &unhex("82181ba26470616e650766726561736f6e02"),
                Direction::HostToClient
            ),
            Err(DecodeError::InvalidValue("reason"))
        );
    }

    // 82 18 19 = [25, ...]; keys: "id" (62), "pane" (64), "count" (65),
    // "offset" (66). 19 01f4 = 500.
    #[test]
    fn scrollback_request_round_trip() {
        roundtrip(
            Message::ScrollbackRequest(ScrollbackRequest {
                id: 1,
                pane: 7,
                offset: 0,
                count: 500,
            }),
            Direction::ClientToHost,
            concat!(
                "821819a4",
                "626964",
                "01", // "id": 1
                "6470616e65",
                "07", // "pane": 7
                "65636f756e74",
                "1901f4", // "count": 500
                "666f6666736574",
                "00", // "offset": 0
            ),
        );
    }

    // 0.3: a7 = map of 7, with "dropped" (0) before "history" (5).
    #[test]
    fn scrollback_page_minimal_round_trip() {
        roundtrip(
            Message::ScrollbackPage(ScrollbackPage {
                id: 1,
                pane: 7,
                history: 5,
                dropped: 0,
                offset: 0,
                styles: vec![],
                lines: vec![Line {
                    wrapped: false,
                    spans: vec![],
                }],
            }),
            Direction::HostToClient,
            concat!(
                "82181aa7",
                "626964",
                "01",
                "6470616e65",
                "07",
                "656c696e6573",
                "81",
                "82f480", // lines: [[false, []]]
                "666f6666736574",
                "00",
                "667374796c6573",
                "80", // styles: []
                "6764726f70706564",
                "00", // "dropped": 0
                "67686973746f7279",
                "05",
            ),
        );
    }

    #[test]
    fn scrollback_request_count_above_500_is_refused() {
        let bytes = encode(&Message::ScrollbackRequest(ScrollbackRequest {
            id: 1,
            pane: 7,
            offset: 0,
            count: 501,
        }));
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::InvalidValue("count"))
        );
    }

    fn snapshot() -> GridSnapshot {
        GridSnapshot {
            pane: 7,
            seq: 0,
            cols: 2,
            rows: 1,
            history: 0,
            dropped: 0,
            styles: vec![plain_style()],
            lines: vec![Line {
                wrapped: false,
                spans: vec![span(0, 1, 0, "ab")],
            }],
            cursor: cursor(),
            alt: false,
        }
    }

    // 82 16 = [22, ...], aa = map of 10. Order: "alt", "seq", "cols",
    // "pane", "rows", "lines", "cursor", "styles", "dropped", "history".
    fn snapshot_hex() -> String {
        [
            "8216aa",
            "63616c74",
            "f4", // "alt": false
            "63736571",
            "00", // "seq": 0
            "64636f6c73",
            "02", // "cols": 2
            "6470616e65",
            "07", // "pane": 7
            "64726f7773",
            "01", // "rows": 1
            "656c696e6573",
            "81",
            "82f4", // lines: [ [false,
            "81",
            "84000100",
            "626162", // [[0, 1, 0, "ab"]] ]
            "66637572736f72",
            CURSOR_HEX,
            "667374796c6573",
            "81",
            "83f6f600", // styles: [[null, null, 0]]
            DROPPED_KEY,
            "00", // "dropped": 0
            "67686973746f7279",
            "00", // "history": 0
        ]
        .concat()
    }

    #[test]
    fn grid_snapshot_round_trip() {
        roundtrip(
            Message::GridSnapshot(snapshot()),
            Direction::HostToClient,
            &snapshot_hex(),
        );
    }

    fn diff() -> GridDiff {
        GridDiff {
            pane: 7,
            seq: 1,
            history: 0,
            dropped: 2,
            styles: vec![Style {
                fg: Color::Indexed(196),
                bg: Color::Default,
                flags: style_flags::BOLD,
            }],
            lines: vec![(
                0,
                Line {
                    wrapped: true,
                    spans: vec![span(0, 2, 0, "界")],
                },
            )],
            cursor: cursor(),
            alt: false,
        }
    }

    // 82 17 = [23, ...], a8 = map of 8. Order: "alt", "seq", "pane",
    // "lines", "cursor", "styles", "dropped", "history". "界" is U+754C =
    // e7 95 8c.
    #[test]
    fn grid_diff_round_trip() {
        let expected = [
            "8217a8",
            "63616c74",
            "f4", // "alt": false
            "63736571",
            "01", // "seq": 1
            "6470616e65",
            "07", // "pane": 7
            "656c696e6573",
            "81",
            "82",
            "00", // lines: [ [0,
            "82f5",
            "81",
            "84000200",
            "63e7958c", // [true, [[0, 2, 0, "界"]]] ]
            "66637572736f72",
            CURSOR_HEX,
            "667374796c6573",
            "81",
            "83",
            "8200",
            "18c4", // [0, 196]
            "f6",
            "01", // styles: [[[0,196], null, 1]]
            DROPPED_KEY,
            "02", // "dropped": 2
            "67686973746f7279",
            "00", // "history": 0
        ]
        .concat();
        roundtrip(
            Message::GridDiff(diff()),
            Direction::HostToClient,
            &expected,
        );
    }

    #[test]
    fn grid_diff_without_changed_lines() {
        let mut empty = diff();
        empty.lines.clear();
        empty.styles.clear();
        let bytes = encode(&Message::GridDiff(empty.clone()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).unwrap(),
            Message::GridDiff(empty)
        );
    }

    fn page() -> ScrollbackPage {
        ScrollbackPage {
            id: 1,
            pane: 7,
            history: 100,
            dropped: 3,
            offset: 10,
            styles: vec![Style {
                fg: Color::Rgb(0x11_2233),
                bg: Color::Indexed(196),
                flags: style_flags::BOLD | style_flags::UNDERLINE,
            }],
            lines: vec![
                Line {
                    wrapped: true,
                    spans: vec![span(0, 1, 0, "x")],
                },
                Line {
                    wrapped: false,
                    spans: vec![],
                },
            ],
        }
    }

    // 82 18 1a = [26, ...], a7 = map of 7. Order: "id", "pane", "lines",
    // "offset", "styles", "dropped", "history". 1a 00112233 is the rgb in a
    // 4-byte uint; the empty line is 82 f4 80.
    #[test]
    fn scrollback_page_round_trip() {
        roundtrip(
            Message::ScrollbackPage(page()),
            Direction::HostToClient,
            concat!(
                "82181aa7",
                "626964",
                "01", // "id": 1
                "6470616e65",
                "07", // "pane": 7
                "656c696e6573",
                "82",
                "82f5",
                "81",
                "84000100",
                "6178",   // [true, [[0, 1, 0, "x"]]]
                "82f480", // [false, []]
                "666f6666736574",
                "0a", // "offset": 10
                "667374796c6573",
                "81",
                "83",
                "82011a00112233", // fg [1, 0x112233]
                "820018c4",       // bg [0, 196]
                "05",             // flags bold | underline
                "6764726f70706564",
                "03", // "dropped": 3
                "67686973746f7279",
                "1864", // "history": 100
            ),
        );
    }

    #[test]
    fn minimal_messages_round_trip() {
        // Empty tables and lines: the smallest snapshot of a 0x0 pane.
        let mut s = snapshot();
        s.cols = 0;
        s.rows = 0;
        s.styles.clear();
        s.lines.clear();
        let bytes = encode(&Message::GridSnapshot(s.clone()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).unwrap(),
            Message::GridSnapshot(s)
        );
    }

    #[test]
    fn graphemes_widths_and_colors_round_trip() {
        let mut s = snapshot();
        s.cols = 8;
        s.styles = vec![
            plain_style(),
            Style {
                fg: Color::Indexed(0),
                bg: Color::Rgb(0xFF_FFFF),
                flags: 0x7f,
            },
            Style {
                fg: Color::Indexed(255),
                bg: Color::Rgb(0),
                flags: 0,
            },
        ];
        s.lines = vec![Line {
            wrapped: true,
            spans: vec![
                span(0, 2, 1, "界"),
                // A grapheme of several code points goes as `[tstr]`.
                grapheme(2, 2, 0, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
                grapheme(4, 1, 2, "e\u{301}"),
                span(5, 1, 0, "abc"),
            ],
        }];
        let bytes = encode(&Message::GridSnapshot(s.clone()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).unwrap(),
            Message::GridSnapshot(s)
        );
    }

    // [0, 1, 0, ["e\u{301}"]]: 84 00 01 00, then 81 (array of 1) and
    // 63 65 cc 81 (3-byte text: "e" + U+0301). A plain span has the text
    // without the array head.
    #[test]
    fn grapheme_span_is_an_array_of_one_text() {
        let grapheme_span = grapheme(0, 1, 0, "e\u{301}");
        assert_eq!(
            hex(&cbor::encode(&grapheme_span.to_value())),
            "84000100816365cc81"
        );
        let plain_span = span(0, 1, 0, "e\u{301}");
        assert_eq!(
            hex(&cbor::encode(&plain_span.to_value())),
            "840001006365cc81"
        );
        assert_eq!(
            Span::from_value(&grapheme_span.to_value()).unwrap(),
            grapheme_span
        );
        assert_eq!(
            Span::from_value(&plain_span.to_value()).unwrap(),
            plain_span
        );
    }

    #[test]
    fn span_text_outside_the_domain_is_refused() {
        let span_with = |text: Value| arr(vec![wire::uint(0), wire::uint(1), wire::uint(0), text]);
        // `[]`, `["a", "b"]` and `[""]`.
        for text in [
            arr(vec![]),
            arr(vec![wire::text("a"), wire::text("b")]),
            arr(vec![wire::text("")]),
        ] {
            assert_eq!(
                Span::from_value(&span_with(text)),
                Err(DecodeError::InvalidValue("text"))
            );
        }
        // Not a text and not an array of text: a CBOR type error.
        assert_eq!(
            Span::from_value(&span_with(wire::uint(1))),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        assert_eq!(
            Span::from_value(&span_with(arr(vec![wire::uint(1)]))),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // Through a whole message the error is the same.
        let line = arr(vec![Value::Bool(false), arr(vec![span_with(arr(vec![]))])]);
        let bytes = page_bytes(arr(vec![]), arr(vec![line]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::InvalidValue("text"))
        );
    }

    #[test]
    fn dropped_is_required_and_a_uint() {
        let mut body = page().to_body_entries();
        for (key, value) in &mut body {
            if key == "dropped" {
                *value = wire::text("3");
            }
        }
        let bytes = cbor::encode(&Value::Array(vec![wire::uint(26), Value::Map(body)]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
    }

    #[test]
    fn debug_hides_the_text() {
        let text = format!("{:?}", snapshot());
        assert!(!text.contains("ab\""));
        assert!(text.contains("text_len"));
    }

    // A message with every required key but one is refused.
    #[test]
    fn missing_required_key_is_an_error() {
        for message in [
            Message::Subscribe(Subscribe { pane: 7 }),
            Message::Unsubscribe(Unsubscribe { pane: 7 }),
            Message::GridAck(GridAck { pane: 7, seq: 3 }),
            Message::SubscriptionEnded(SubscriptionEnded {
                pane: 7,
                reason: SubscriptionEndReason::PaneClosed,
            }),
            Message::ScrollbackRequest(ScrollbackRequest {
                id: 1,
                pane: 7,
                offset: 0,
                count: 1,
            }),
            Message::GridSnapshot(snapshot()),
            Message::GridDiff(diff()),
            Message::ScrollbackPage(page()),
        ] {
            let from = if message.can_be_sent_by(Direction::ClientToHost) {
                Direction::ClientToHost
            } else {
                Direction::HostToClient
            };
            let Value::Map(entries) = message.body() else {
                panic!("body is a map");
            };
            for skipped in 0..entries.len() {
                let mut partial = entries.clone();
                partial.remove(skipped);
                let bytes = cbor::encode(&Value::Array(vec![
                    wire::uint(message.type_code()),
                    Value::Map(partial),
                ]));
                assert_eq!(
                    decode(&bytes, from),
                    Err(DecodeError::Cbor(CborError::MissingKey)),
                    "type {} without key #{skipped}",
                    message.type_code()
                );
            }
        }
    }

    #[test]
    fn unknown_keys_are_ignored() {
        for message in [
            Message::Subscribe(Subscribe { pane: 7 }),
            Message::GridAck(GridAck { pane: 7, seq: 3 }),
            Message::SubscriptionEnded(SubscriptionEnded {
                pane: 7,
                reason: SubscriptionEndReason::HostLimit,
            }),
            Message::ScrollbackRequest(ScrollbackRequest {
                id: 1,
                pane: 7,
                offset: 10,
                count: 500,
            }),
            Message::GridSnapshot(snapshot()),
            Message::GridDiff(diff()),
            Message::ScrollbackPage(page()),
        ] {
            let from = if message.can_be_sent_by(Direction::ClientToHost) {
                Direction::ClientToHost
            } else {
                Direction::HostToClient
            };
            let Value::Map(mut entries) = message.body() else {
                panic!("body is a map");
            };
            entries.push(("zzz".to_owned(), Value::Bool(true)));
            let bytes = cbor::encode(&Value::Array(vec![
                wire::uint(message.type_code()),
                Value::Map(entries),
            ]));
            assert_eq!(decode(&bytes, from).unwrap(), message);
        }
    }

    /// A `scrollback-page` whose `styles` and `lines` are given as values.
    fn page_bytes(styles: Value, lines: Value) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            wire::uint(26),
            wire::map(vec![
                ("id", wire::uint(1)),
                ("pane", wire::uint(7)),
                ("history", wire::uint(0)),
                ("dropped", wire::uint(0)),
                ("offset", wire::uint(0)),
                ("styles", styles),
                ("lines", lines),
            ]),
        ]))
    }

    fn arr(items: Vec<Value>) -> Value {
        Value::Array(items)
    }

    fn style_of(fg: Value, bg: Value) -> Value {
        arr(vec![arr(vec![fg, bg, wire::uint(0)])])
    }

    #[test]
    fn colors_outside_the_domain_are_refused() {
        let bad_colors = [
            arr(vec![wire::uint(0), wire::uint(256)]), // index above 255
            arr(vec![wire::uint(1), wire::uint(0x100_0000)]), // rgb above 0xFFFFFF
            arr(vec![wire::uint(2), wire::uint(0)]),   // unknown form
        ];
        for color in bad_colors {
            let bytes = page_bytes(style_of(color, Value::Null), arr(vec![]));
            assert_eq!(
                decode(&bytes, Direction::HostToClient),
                Err(DecodeError::InvalidValue("color"))
            );
        }
        // Wrong arity and wrong type are CBOR errors.
        let bytes = page_bytes(style_of(arr(vec![wire::uint(0)]), Value::Null), arr(vec![]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
        let bytes = page_bytes(style_of(wire::uint(5), Value::Null), arr(vec![]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongType))
        );
        // The three forms at their limits are accepted.
        let ok = page_bytes(
            style_of(arr(vec![wire::uint(0), wire::uint(255)]), Value::Null),
            arr(vec![]),
        );
        assert!(decode(&ok, Direction::HostToClient).is_ok());
        let ok = page_bytes(
            style_of(arr(vec![wire::uint(1), wire::uint(0xFF_FFFF)]), Value::Null),
            arr(vec![]),
        );
        assert!(decode(&ok, Direction::HostToClient).is_ok());
    }

    #[test]
    fn style_and_span_arity_is_exact() {
        // Style with 2 elements.
        let bytes = page_bytes(arr(vec![arr(vec![Value::Null, Value::Null])]), arr(vec![]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
        // Span with 5 elements.
        let line = arr(vec![
            Value::Bool(false),
            arr(vec![arr(vec![
                wire::uint(0),
                wire::uint(1),
                wire::uint(0),
                wire::text("a"),
                wire::uint(0),
            ])]),
        ]);
        let bytes = page_bytes(arr(vec![]), arr(vec![line]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
        // Line with 3 elements.
        let line = arr(vec![Value::Bool(false), arr(vec![]), Value::Null]);
        let bytes = page_bytes(arr(vec![]), arr(vec![line]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
    }

    #[test]
    fn span_width_must_be_1_or_2() {
        for width in [0, 3] {
            let line = arr(vec![
                Value::Bool(false),
                arr(vec![arr(vec![
                    wire::uint(0),
                    wire::uint(width),
                    wire::uint(0),
                    wire::text("a"),
                ])]),
            ]);
            let bytes = page_bytes(arr(vec![]), arr(vec![line]));
            assert_eq!(
                decode(&bytes, Direction::HostToClient),
                Err(DecodeError::InvalidValue("width"))
            );
        }
    }

    #[test]
    fn diff_entries_must_be_row_line_pairs() {
        let mut body = diff().to_body_entries();
        for (key, value) in &mut body {
            if key == "lines" {
                // An entry with 3 elements.
                *value = arr(vec![arr(vec![
                    wire::uint(0),
                    arr(vec![Value::Bool(false), arr(vec![])]),
                    Value::Null,
                ])]);
            }
        }
        let bytes = cbor::encode(&Value::Array(vec![wire::uint(23), Value::Map(body)]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::Cbor(CborError::WrongArity))
        );
    }

    #[test]
    fn cursor_shape_outside_the_domain_is_refused() {
        let mut body = snapshot().to_body_entries();
        for (key, value) in &mut body {
            if key == "cursor" {
                *value = wire::map(vec![
                    ("row", wire::uint(0)),
                    ("col", wire::uint(0)),
                    ("shape", wire::uint(3)),
                    ("visible", Value::Bool(true)),
                ]);
            }
        }
        let bytes = cbor::encode(&Value::Array(vec![wire::uint(22), Value::Map(body)]));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::InvalidValue("shape"))
        );
    }

    #[test]
    fn wrong_direction_is_refused() {
        let bytes = encode(&Message::GridSnapshot(snapshot()));
        assert_eq!(
            decode(&bytes, Direction::ClientToHost),
            Err(DecodeError::WrongDirection(22))
        );
        let bytes = encode(&Message::GridAck(GridAck { pane: 7, seq: 3 }));
        assert_eq!(
            decode(&bytes, Direction::HostToClient),
            Err(DecodeError::WrongDirection(24))
        );
    }

    trait ToBodyEntries {
        fn to_body_entries(&self) -> Vec<(String, Value)>;
    }

    impl<T: wire::Body> ToBodyEntries for T {
        fn to_body_entries(&self) -> Vec<(String, Value)> {
            match wire::Body::to_value(self) {
                Value::Map(entries) => entries,
                _ => panic!("body is a map"),
            }
        }
    }
}
