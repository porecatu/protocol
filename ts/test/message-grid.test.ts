// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { CborMap, encodeCbor, type CborValue } from "../src/cbor/index.js";
import {
  decodeMessage,
  encodeMessage,
  type Cursor,
  type DecodeResult,
  type Direction,
  type DecodeErrorReason,
  type GridDiff,
  type GridSnapshot,
  type Message,
  type ScrollbackPage,
} from "../src/message/index.js";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function fromHex(text: string): Uint8Array {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(text.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function failure(result: DecodeResult): DecodeErrorReason {
  if (result.ok) {
    throw new Error("expected a decode failure");
  }
  return result.error.reason;
}

function success(result: DecodeResult): Message {
  if (!result.ok) {
    throw new Error(`expected success, got ${result.error.reason}`);
  }
  return result.message;
}

// Every hex below was assembled by hand from RFC 8949 (not produced by the
// encoder): text head byte = 0x60 + length; map keys sort by encoded bytes,
// i.e. by length first, then bytewise ("alt" < "seq" < "cols" < "pane" < ...).
// The same hex strings are used by the Rust tests of the same group.

const CURSOR: Cursor = { row: 0, col: 2, shape: 0, visible: true };

/** {col: 2, row: 0, shape: 0, visible: true} */
const CURSOR_HEX =
  "a4" +
  "63636f6c" + "02" +
  "63726f77" + "00" +
  "6573686170" + "65" + "00" +
  "67" + "76697369626c65" + "f5";

// --- subscribe / unsubscribe / grid-ack ---

/** [20, {pane: 7}] */
const SUBSCRIBE_HEX = "8214" + "a1" + "6470616e65" + "07";
/** [21, {pane: 7}] */
const UNSUBSCRIBE_HEX = "8215" + "a1" + "6470616e65" + "07";
/** [24, {seq: 3, pane: 7}]; 24 = 0x18 0x18; "seq" (0x63) sorts before "pane" (0x64). */
const GRID_ACK_HEX = "8218" + "18" + "a2" + "63736571" + "03" + "6470616e65" + "07";

// --- grid-snapshot ---

/**
 * [22, {alt, seq, cols, pane, rows, lines, cursor, styles, dropped, history}]
 * lines: [[false, [[0, 1, 0, "ab"]]]]; styles: [[null, null, 0]]
 */
const SNAPSHOT_HEX =
  "8216" + "aa" +
  "63616c74" + "f4" +
  "63736571" + "00" +
  "64636f6c73" + "02" +
  "6470616e65" + "07" +
  "64726f7773" + "01" +
  "656c696e6573" + "81" + "82" + "f4" + "81" + "84" + "00" + "01" + "00" + "62" + "6162" +
  "66637572736f72" + CURSOR_HEX +
  "667374796c6573" + "81" + "83" + "f6" + "f6" + "00" +
  "67" + "64726f70706564" + "00" +
  "67686973746f7279" + "00";

const SNAPSHOT: GridSnapshot = {
  type: "grid-snapshot",
  pane: 7,
  seq: 0,
  cols: 2,
  rows: 1,
  history: 0,
  dropped: 0,
  styles: [[null, null, 0]],
  lines: [[false, [[0, 1, 0, "ab"]]]],
  cursor: CURSOR,
  alt: false,
};

// --- grid-diff ---

/**
 * [23, {alt, seq, pane, lines, cursor, styles, dropped, history}]
 * styles: [[[0, 196], null, 1]]; lines: [[0, [true, [[0, 2, 0, "界"]]]]]
 * (196 = 0x18 0xc4; "界" = U+754C = e7 95 8c)
 */
const DIFF_HEX =
  "8217" + "a8" +
  "63616c74" + "f4" +
  "63736571" + "01" +
  "6470616e65" + "07" +
  "656c696e6573" + "81" + "82" + "00" + "82" + "f5" + "81" + "84" + "00" + "02" + "00" +
  "63" + "e7958c" +
  "66637572736f72" + CURSOR_HEX +
  "667374796c6573" + "81" + "83" + "82" + "00" + "18c4" + "f6" + "01" +
  "67" + "64726f70706564" + "00" +
  "67686973746f7279" + "00";

const DIFF: GridDiff = {
  type: "grid-diff",
  pane: 7,
  seq: 1,
  history: 0,
  dropped: 0,
  styles: [[[0, 196], null, 1]],
  lines: [[0, [true, [[0, 2, 0, "界"]]]]],
  cursor: CURSOR,
  alt: false,
};

// --- scrollback ---

/**
 * [25, {id: 1, pane: 7, count: 500, offset: 0}]; 25 = 0x18 0x19; 500 = 0x19 0x01 0xf4.
 */
const SCROLLBACK_REQUEST_HEX =
  "8218" + "19" + "a4" +
  "626964" + "01" +
  "6470616e65" + "07" +
  "65636f756e74" + "1901f4" +
  "666f6666736574" + "00";

/**
 * [26, {id: 1, pane: 7, lines: [[false, []]], offset: 0, styles: [], dropped: 3, history: 5}]
 * 26 = 0x18 0x1a; at length 6, "offset" (0x6f) sorts before "styles" (0x73);
 * at length 7, "dropped" (0x64) sorts before "history" (0x68).
 */
const SCROLLBACK_PAGE_HEX =
  "8218" + "1a" + "a7" +
  "626964" + "01" +
  "6470616e65" + "07" +
  "656c696e6573" + "81" + "82" + "f4" + "80" +
  "666f6666736574" + "00" +
  "667374796c6573" + "80" +
  "67" + "64726f70706564" + "03" +
  "67686973746f7279" + "05";

const SCROLLBACK_PAGE: ScrollbackPage = {
  type: "scrollback-page",
  id: 1,
  pane: 7,
  history: 5,
  dropped: 3,
  offset: 0,
  styles: [],
  lines: [[false, []]],
};

/** [27, {pane: 7, reason: 1}]; 27 = 0x18 0x1b (24 and above take the one-byte form). */
const ENDED_HEX = "8218" + "1b" + "a2" + "6470616e65" + "07" + "66726561736f6e" + "01";

const CASES: ReadonlyArray<readonly [string, Message, string, Direction]> = [
  ["subscribe", { type: "subscribe", pane: 7 }, SUBSCRIBE_HEX, "client"],
  ["unsubscribe", { type: "unsubscribe", pane: 7 }, UNSUBSCRIBE_HEX, "client"],
  ["grid-ack", { type: "grid-ack", pane: 7, seq: 3 }, GRID_ACK_HEX, "client"],
  ["grid-snapshot", SNAPSHOT, SNAPSHOT_HEX, "host"],
  ["grid-diff", DIFF, DIFF_HEX, "host"],
  [
    "scrollback-request",
    { type: "scrollback-request", id: 1, pane: 7, offset: 0, count: 500 },
    SCROLLBACK_REQUEST_HEX,
    "client",
  ],
  ["scrollback-page", SCROLLBACK_PAGE, SCROLLBACK_PAGE_HEX, "host"],
  [
    "subscription-ended",
    { type: "subscription-ended", pane: 7, reason: 1 },
    ENDED_HEX,
    "host",
  ],
];

describe("grid messages: bytes and round trip", () => {
  it.each(CASES)("%s: exact bytes and round trip", (_name, message, expected, from) => {
    expect(hex(encodeMessage(message))).toBe(expected);
    expect(success(decodeMessage(fromHex(expected), from))).toEqual(message);
  });

  it("round trips a complete snapshot: wide, grapheme cluster, empty line, all colors", () => {
    // A ZWJ sequence (man + ZWJ + woman): one grapheme in one cell, so it goes
    // as a one-element array (spec 03 §5.2).
    const family = "\u{1F468}‍\u{1F469}";
    const full: GridSnapshot = {
      type: "grid-snapshot",
      pane: 9,
      seq: 41,
      cols: 80,
      rows: 3,
      history: 1234,
      dropped: 77,
      styles: [
        [null, null, 0],
        [[0, 196], [1, 0x112233], 0x01 | 0x08],
        [[1, 0xffffff], [0, 0], 0x7f],
        [[0, 255], null, 0x20],
      ],
      lines: [
        [false, [[0, 1, 0, "ls "], [3, 2, 1, "界界"], [7, 2, 2, [family]]]],
        [true, [[0, 1, 3, "é"]]],
        [false, []],
      ],
      cursor: { row: 2, col: 79, shape: 2, visible: false },
      alt: true,
    };
    expect(success(decodeMessage(encodeMessage(full), "host"))).toEqual(full);
    const diff: GridDiff = {
      type: "grid-diff",
      pane: 9,
      seq: 42,
      history: 1235,
      dropped: 78,
      styles: full.styles,
      lines: [
        [0, full.lines[0] as GridSnapshot["lines"][number]],
        [2, [false, []]],
      ],
      cursor: { row: 0, col: 0, shape: 1, visible: true },
      alt: false,
    };
    expect(success(decodeMessage(encodeMessage(diff), "host"))).toEqual(diff);
    const page: ScrollbackPage = { ...SCROLLBACK_PAGE, styles: full.styles, lines: full.lines };
    expect(success(decodeMessage(encodeMessage(page), "host"))).toEqual(page);
  });

  it("round trips the minimal diff and the largest allowed count", () => {
    const minimal: GridDiff = {
      type: "grid-diff",
      pane: 0,
      seq: 0,
      history: 0,
      dropped: 0,
      styles: [],
      lines: [],
      cursor: { row: 0, col: 0, shape: 0, visible: false },
      alt: false,
    };
    expect(success(decodeMessage(encodeMessage(minimal), "host"))).toEqual(minimal);
    const zero: Message = { type: "scrollback-request", id: 0, pane: 0, offset: 0, count: 0 };
    expect(success(decodeMessage(encodeMessage(zero), "client"))).toEqual(zero);
  });
});

/** Body of a valid `grid-snapshot`, as ordered pairs, with the given overrides. */
function snapshotBody(overrides: Record<string, CborValue> = {}): CborMap {
  const base: Record<string, CborValue> = {
    pane: 7,
    seq: 0,
    cols: 2,
    rows: 1,
    history: 0,
    dropped: 0,
    styles: [[null, null, 0]],
    lines: [[false, [[0, 1, 0, "ab"]]]],
    cursor: new CborMap([
      ["row", 0],
      ["col", 2],
      ["shape", 0],
      ["visible", true],
    ]),
    alt: false,
    ...overrides,
  };
  return new CborMap(Object.entries(base));
}

describe("grid messages: refusals", () => {
  it("ignores unknown keys but needs canonical form", () => {
    const bytes = encodeCbor([
      22,
      snapshotBody({ extra: [1, 2, 3] }),
    ]);
    expect(success(decodeMessage(bytes, "host"))).toEqual(SNAPSHOT);
    const cursorWithExtra = encodeCbor([
      22,
      snapshotBody({
        cursor: new CborMap([
          ["row", 0],
          ["col", 2],
          ["shape", 0],
          ["visible", true],
          ["blink", true],
        ]),
      }),
    ]);
    expect(success(decodeMessage(cursorWithExtra, "host"))).toEqual(SNAPSHOT);
    // grid-ack with "pane" before "seq": out of canonical order.
    const unordered = fromHex(
      "8218" + "18" + "a2" + "6470616e65" + "07" + "63736571" + "03",
    );
    expect(failure(decodeMessage(unordered, "client"))).toBe("unordered-keys");
  });

  it("refuses a missing required key", () => {
    const keys = [
      "pane", "seq", "cols", "rows", "history", "dropped", "styles", "lines", "cursor", "alt",
    ];
    for (const key of keys) {
      const entries = snapshotBody().entries.filter(([k]) => k !== key);
      const bytes = encodeCbor([22, new CborMap(entries)]);
      expect(failure(decodeMessage(bytes, "host"))).toBe("missing-key");
    }
    expect(
      failure(decodeMessage(encodeCbor([24, new CborMap([["pane", 7]])]), "client")),
    ).toBe("missing-key");
    expect(
      failure(
        decodeMessage(
          encodeCbor([25, new CborMap([["id", 1], ["pane", 7], ["offset", 0]])]),
          "client",
        ),
      ),
    ).toBe("missing-key");
    // cursor without `visible`
    const bytes = encodeCbor([
      22,
      snapshotBody({
        cursor: new CborMap([
          ["row", 0],
          ["col", 2],
          ["shape", 0],
        ]),
      }),
    ]);
    expect(failure(decodeMessage(bytes, "host"))).toBe("missing-key");
  });

  it("refuses values outside their domain", () => {
    const cases: ReadonlyArray<readonly [string, Uint8Array, Direction]> = [
      [
        "count above 500",
        encodeCbor([
          25,
          new CborMap([["id", 1], ["pane", 7], ["offset", 0], ["count", 501]]),
        ]),
        "client",
      ],
      [
        "span width 3",
        encodeCbor([22, snapshotBody({ lines: [[false, [[0, 3, 0, "a"]]]] })]),
        "host",
      ],
      [
        "span width 0",
        encodeCbor([22, snapshotBody({ lines: [[false, [[0, 0, 0, "a"]]]] })]),
        "host",
      ],
      [
        "grapheme span with an empty array",
        encodeCbor([22, snapshotBody({ lines: [[false, [[0, 1, 0, []]]]] })]),
        "host",
      ],
      [
        "grapheme span with two texts",
        encodeCbor([22, snapshotBody({ lines: [[false, [[0, 1, 0, ["a", "b"]]]]] })]),
        "host",
      ],
      [
        "grapheme span with an empty text",
        encodeCbor([22, snapshotBody({ lines: [[false, [[0, 1, 0, [""]]]]] })]),
        "host",
      ],
      [
        "indexed color 256",
        encodeCbor([22, snapshotBody({ styles: [[[0, 256], null, 0]] })]),
        "host",
      ],
      [
        "color kind 2",
        encodeCbor([22, snapshotBody({ styles: [[[2, 1], null, 0]] })]),
        "host",
      ],
      [
        "direct color above 0xFFFFFF",
        encodeCbor([22, snapshotBody({ styles: [[null, [1, 0x1000000], 0]] })]),
        "host",
      ],
      [
        "unknown cursor shape",
        encodeCbor([
          22,
          snapshotBody({
            cursor: new CborMap([
              ["row", 0],
              ["col", 2],
              ["shape", 3],
              ["visible", true],
            ]),
          }),
        ]),
        "host",
      ],
      [
        "unknown end reason",
        encodeCbor([27, new CborMap([["pane", 7], ["reason", 2]])]),
        "host",
      ],
    ];
    for (const [, bytes, from] of cases) {
      expect(failure(decodeMessage(bytes, from))).toBe("out-of-domain");
    }
  });

  it("refuses positional arrays of the wrong arity", () => {
    const cases: ReadonlyArray<readonly [Uint8Array, DecodeErrorReason]> = [
      // span with 3 elements
      [
        encodeCbor([22, snapshotBody({ lines: [[false, [[0, 1, 0]]]] })]),
        "wrong-arity",
      ],
      // line with 3 elements
      [encodeCbor([22, snapshotBody({ lines: [[false, [], 0]] })]), "wrong-arity"],
      // style with 2 elements
      [encodeCbor([22, snapshotBody({ styles: [[null, null]] })]), "wrong-arity"],
      // color with 3 elements
      [encodeCbor([22, snapshotBody({ styles: [[[0, 1, 2], null, 0]] })]), "wrong-arity"],
      // diff entry [row, line] with 3 elements
      [
        encodeCbor([
          23,
          new CborMap([
            ["pane", 7],
            ["seq", 1],
            ["history", 0],
            ["dropped", 0],
            ["styles", []],
            ["lines", [[0, [false, []], 9]]],
            [
              "cursor",
              new CborMap([
                ["row", 0],
                ["col", 0],
                ["shape", 0],
                ["visible", true],
              ]),
            ],
            ["alt", false],
          ]),
        ]),
        "wrong-arity",
      ],
    ];
    for (const [bytes, reason] of cases) {
      expect(failure(decodeMessage(bytes, "host"))).toBe(reason);
    }
  });

  it("refuses values of the wrong type", () => {
    // pane as text
    expect(
      failure(decodeMessage(encodeCbor([20, new CborMap([["pane", "7"]])]), "client")),
    ).toBe("wrong-type");
    // wrapped as an integer
    expect(
      failure(decodeMessage(encodeCbor([22, snapshotBody({ lines: [[0, []]] })]), "host")),
    ).toBe("wrong-type");
    // span text as bytes
    expect(
      failure(
        decodeMessage(
          encodeCbor([
            22,
            snapshotBody({ lines: [[false, [[0, 1, 0, new Uint8Array([97])]]]] }),
          ]),
          "host",
        ),
      ),
    ).toBe("wrong-type");
    // grapheme span whose element is not text
    expect(
      failure(
        decodeMessage(
          encodeCbor([22, snapshotBody({ lines: [[false, [[0, 1, 0, [1]]]]] })]),
          "host",
        ),
      ),
    ).toBe("wrong-type");
    // dropped as text
    expect(
      failure(decodeMessage(encodeCbor([22, snapshotBody({ dropped: "0" })]), "host")),
    ).toBe("wrong-type");
    // cursor as an array
    expect(
      failure(decodeMessage(encodeCbor([22, snapshotBody({ cursor: [0, 2, 0, true] })]), "host")),
    ).toBe("wrong-type");
  });

  it("refuses each message from the wrong side", () => {
    expect(failure(decodeMessage(fromHex(SUBSCRIBE_HEX), "host"))).toBe("wrong-direction");
    expect(failure(decodeMessage(fromHex(GRID_ACK_HEX), "host"))).toBe("wrong-direction");
    expect(failure(decodeMessage(fromHex(SNAPSHOT_HEX), "client"))).toBe("wrong-direction");
    expect(failure(decodeMessage(fromHex(DIFF_HEX), "client"))).toBe("wrong-direction");
    expect(failure(decodeMessage(fromHex(ENDED_HEX), "client"))).toBe("wrong-direction");
  });
});
