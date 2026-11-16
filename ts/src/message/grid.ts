// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Grid subscription, snapshots, diffs, flow control and scrollback (spec 03
 * §5): `subscribe`, `unsubscribe`, `grid-snapshot`, `grid-diff`, `grid-ack`,
 * `scrollback-request`, `scrollback-page`, `subscription-ended`.
 */

import {
  asArray,
  asBool,
  asMap,
  asText,
  asUint,
  expectArray,
  isNull,
  mapReader,
  type CborValue,
} from "../cbor/index.js";
import { body, DecodeError, enumFrom, type BodyCodec } from "./codec.js";
import { CURSOR_SHAPES, SUBSCRIPTION_END_REASONS } from "./codes.js";
import type {
  Color,
  Cursor,
  GridAck,
  GridDiff,
  GridSnapshot,
  Line,
  RowLine,
  ScrollbackPage,
  ScrollbackRequest,
  Span,
  Style,
  Subscribe,
  SubscriptionEnded,
  Unsubscribe,
} from "./types.js";

/** Largest `count` of a `scrollback-request` (spec 03 §5.5). */
export const MAX_SCROLLBACK_COUNT = 500;

/** Largest `rgb` value (spec 03 §3). */
const MAX_RGB = 0xffffff;

/** Largest indexed color (spec 03 §5.2). */
const MAX_INDEXED_COLOR = 255;

function outOfDomain(): never {
  throw new DecodeError("out-of-domain");
}

// --- color, style, span, line ---

function colorToValue(color: Color): CborValue {
  return color === null ? null : [color[0], color[1]];
}

function colorFromValue(value: CborValue | undefined): Color {
  if (isNull(value)) {
    return null;
  }
  const items = expectArray(value, 2);
  const kind = asUint(items[0]);
  const n = asUint(items[1]);
  if (kind === 0 && n <= MAX_INDEXED_COLOR) {
    return [0, n];
  }
  if (kind === 1 && n <= MAX_RGB) {
    return [1, n];
  }
  return outOfDomain();
}

function styleToValue(style: Style): CborValue {
  return [colorToValue(style[0]), colorToValue(style[1]), style[2]];
}

function styleFromValue(value: CborValue | undefined): Style {
  const items = expectArray(value, 3);
  return [colorFromValue(items[0]), colorFromValue(items[1]), asUint(items[2])];
}

function spanToValue(span: Span): CborValue {
  const text = span[3];
  return [span[0], span[1], span[2], typeof text === "string" ? text : [text[0]]];
}

/**
 * The `text` of a span: a text is an ordinary span; an array of exactly one
 * non-empty text is a single multi-code-point grapheme (spec 03 §5.2). An
 * empty array, one with 2+ elements or with an empty text is out of domain.
 */
function spanTextFromValue(value: CborValue | undefined): string | readonly [string] {
  if (!Array.isArray(value)) {
    return asText(value);
  }
  if (value.length !== 1) {
    return outOfDomain();
  }
  const grapheme = asText(value[0]);
  if (grapheme.length === 0) {
    return outOfDomain();
  }
  return [grapheme];
}

function spanFromValue(value: CborValue | undefined): Span {
  const items = expectArray(value, 4);
  const col = asUint(items[0]);
  const width = asUint(items[1]);
  const style = asUint(items[2]);
  const text = spanTextFromValue(items[3]);
  if (width !== 1 && width !== 2) {
    return outOfDomain();
  }
  return [col, width, style, text];
}

function lineToValue(line: Line): CborValue {
  return [line[0], line[1].map(spanToValue)];
}

function lineFromValue(value: CborValue | undefined): Line {
  const items = expectArray(value, 2);
  const wrapped = asBool(items[0]);
  const spans = asArray(items[1]).map(spanFromValue);
  return [wrapped, spans];
}

function rowLineToValue(entry: RowLine): CborValue {
  return [entry[0], lineToValue(entry[1])];
}

function rowLineFromValue(value: CborValue | undefined): RowLine {
  const items = expectArray(value, 2);
  return [asUint(items[0]), lineFromValue(items[1])];
}

function stylesToValue(styles: readonly Style[]): CborValue {
  return styles.map(styleToValue);
}

function stylesFromValue(value: CborValue | undefined): Style[] {
  return asArray(value).map(styleFromValue);
}

// --- cursor ---

function cursorToValue(cursor: Cursor): CborValue {
  return body([
    ["row", cursor.row],
    ["col", cursor.col],
    ["shape", cursor.shape],
    ["visible", cursor.visible],
  ]);
}

function cursorFromValue(value: CborValue | undefined): Cursor {
  const r = mapReader(asMap(value));
  return {
    row: asUint(r.required("row")),
    col: asUint(r.required("col")),
    shape: enumFrom(CURSOR_SHAPES, asUint(r.required("shape"))),
    visible: asBool(r.required("visible")),
  };
}

// --- subscription ---

export const subscribeCodec: BodyCodec<Subscribe> = {
  toBody: (m) => body([["pane", m.pane]]),
  fromBody: (r) => ({ type: "subscribe", pane: asUint(r.required("pane")) }),
};

export const unsubscribeCodec: BodyCodec<Unsubscribe> = {
  toBody: (m) => body([["pane", m.pane]]),
  fromBody: (r) => ({ type: "unsubscribe", pane: asUint(r.required("pane")) }),
};

export const subscriptionEndedCodec: BodyCodec<SubscriptionEnded> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["reason", m.reason],
    ]),
  fromBody: (r) => ({
    type: "subscription-ended",
    pane: asUint(r.required("pane")),
    reason: enumFrom(SUBSCRIPTION_END_REASONS, asUint(r.required("reason"))),
  }),
};

// --- snapshot, diff, ack ---

export const gridSnapshotCodec: BodyCodec<GridSnapshot> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["seq", m.seq],
      ["cols", m.cols],
      ["rows", m.rows],
      ["history", m.history],
      ["dropped", m.dropped],
      ["styles", stylesToValue(m.styles)],
      ["lines", m.lines.map(lineToValue)],
      ["cursor", cursorToValue(m.cursor)],
      ["alt", m.alt],
    ]),
  fromBody: (r) => ({
    type: "grid-snapshot",
    pane: asUint(r.required("pane")),
    seq: asUint(r.required("seq")),
    cols: asUint(r.required("cols")),
    rows: asUint(r.required("rows")),
    history: asUint(r.required("history")),
    dropped: asUint(r.required("dropped")),
    styles: stylesFromValue(r.required("styles")),
    lines: asArray(r.required("lines")).map(lineFromValue),
    cursor: cursorFromValue(r.required("cursor")),
    alt: asBool(r.required("alt")),
  }),
};

export const gridDiffCodec: BodyCodec<GridDiff> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["seq", m.seq],
      ["history", m.history],
      ["dropped", m.dropped],
      ["styles", stylesToValue(m.styles)],
      ["lines", m.lines.map(rowLineToValue)],
      ["cursor", cursorToValue(m.cursor)],
      ["alt", m.alt],
    ]),
  fromBody: (r) => ({
    type: "grid-diff",
    pane: asUint(r.required("pane")),
    seq: asUint(r.required("seq")),
    history: asUint(r.required("history")),
    dropped: asUint(r.required("dropped")),
    styles: stylesFromValue(r.required("styles")),
    lines: asArray(r.required("lines")).map(rowLineFromValue),
    cursor: cursorFromValue(r.required("cursor")),
    alt: asBool(r.required("alt")),
  }),
};

export const gridAckCodec: BodyCodec<GridAck> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["seq", m.seq],
    ]),
  fromBody: (r) => ({
    type: "grid-ack",
    pane: asUint(r.required("pane")),
    seq: asUint(r.required("seq")),
  }),
};

// --- scrollback ---

export const scrollbackRequestCodec: BodyCodec<ScrollbackRequest> = {
  toBody: (m) =>
    body([
      ["id", m.id],
      ["pane", m.pane],
      ["offset", m.offset],
      ["count", m.count],
    ]),
  fromBody: (r) => {
    const id = asUint(r.required("id"));
    const pane = asUint(r.required("pane"));
    const offset = asUint(r.required("offset"));
    const count = asUint(r.required("count"));
    if (count > MAX_SCROLLBACK_COUNT) {
      return outOfDomain();
    }
    return { type: "scrollback-request", id, pane, offset, count };
  },
};

export const scrollbackPageCodec: BodyCodec<ScrollbackPage> = {
  toBody: (m) =>
    body([
      ["id", m.id],
      ["pane", m.pane],
      ["history", m.history],
      ["dropped", m.dropped],
      ["offset", m.offset],
      ["styles", stylesToValue(m.styles)],
      ["lines", m.lines.map(lineToValue)],
    ]),
  fromBody: (r) => ({
    type: "scrollback-page",
    id: asUint(r.required("id")),
    pane: asUint(r.required("pane")),
    history: asUint(r.required("history")),
    dropped: asUint(r.required("dropped")),
    offset: asUint(r.required("offset")),
    styles: stylesFromValue(r.required("styles")),
    lines: asArray(r.required("lines")).map(lineFromValue),
  }),
};
