// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { CborMap, encodeCbor, type CborValue } from "../src/cbor/index.js";
import {
  ACTION_TYPES,
  decodeMessage,
  encodeMessage,
  ErrorCode,
  messageSender,
  MOD,
  NamedKey,
  SessionsMode,
  type ActionBody,
  type DecodeErrorReason,
  type DecodeResult,
  type Direction,
  type Message,
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

function map(entries: ReadonlyArray<readonly [string, CborValue]>): CborMap {
  return new CborMap(entries);
}

function wire(type: number, entries: ReadonlyArray<readonly [string, CborValue]>): Uint8Array {
  return encodeCbor([type, map(entries)]);
}

// Every hex below was assembled by hand from RFC 8949 (not produced by the
// encoder). Type codes 30..32, 40..43 do not fit in 23, so they are `18 xx`.
// A text key's head byte is 0x60 + its length; map keys sort by their encoded
// bytes (shorter first, then lexicographically), which fixes the orders below:
// key: "key" < "mods" < "pane"; text-input/paste: "pane" < "text";
// action: "do" < "id"; rename-tab body: "tab" < "title"; new-group body:
// "name" < "tabs"; action-result: "id" < "error" < "created" (lengths 2, 5, 7);
// confirm-request: "id" < "kind" < "processes"; confirm: "id" < "accept".

const CASES: ReadonlyArray<readonly [string, Message, string, Direction]> = [
  [
    // [30, {key: 0, mods: 0, pane: 7}]
    "key (named key enter)",
    { type: "key", pane: 7, key: NamedKey.enter, mods: 0 },
    "82181ea3" + "636b6579" + "00" + "646d6f6473" + "00" + "6470616e65" + "07",
    "client",
  ],
  [
    // [30, {key: "c", mods: 1, pane: 7}]; "c" = 0x63, head 0x61
    "key (text with Ctrl)",
    { type: "key", pane: 7, key: "c", mods: MOD.ctrl },
    "82181ea3" + "636b6579" + "6163" + "646d6f6473" + "01" + "6470616e65" + "07",
    "client",
  ],
  [
    // [31, {pane: 7, text: "ls"}]; "ls" = 6c 73, head 0x62
    "text-input",
    { type: "text-input", pane: 7, text: "ls" },
    "82181fa2" + "6470616e65" + "07" + "6474657874" + "626c73",
    "client",
  ],
  [
    // [32, {pane: 7, text: "hi"}]; "hi" = 68 69
    "paste",
    { type: "paste", pane: 7, text: "hi" },
    "821820a2" + "6470616e65" + "07" + "6474657874" + "626869",
    "client",
  ],
  [
    // [40, {do: [0, {window: 1}], id: 1}]
    "action new-tab (minimal)",
    { type: "action", id: 1, do: { type: "new-tab", window: 1 } },
    "821828a2" + "62646f" + "8200a1" + "6677696e646f77" + "01" + "626964" + "01",
    "client",
  ],
  [
    // [40, {do: [3, {tab: 5, title: null}], id: 2}]
    "action rename-tab (title null)",
    { type: "action", id: 2, do: { type: "rename-tab", tab: 5, title: null } },
    "821828a2" +
      "62646f" +
      "8203a2" +
      "63746162" +
      "05" +
      "657469746c65" +
      "f6" +
      "626964" +
      "02",
    "client",
  ],
  [
    // [40, {do: [6, {name: "g", tabs: [1, 2]}], id: 4}]; "name" < "tabs"
    "action new-group (minimal)",
    { type: "action", id: 4, do: { type: "new-group", tabs: [1, 2], name: "g" } },
    "821828a2" +
      "62646f" +
      "8206a2" +
      "646e616d65" +
      "6167" +
      "6474616273" +
      "820102" +
      "626964" +
      "04",
    "client",
  ],
  [
    // [41, {id: 1, created: 9}]
    "action-result with created",
    { type: "action-result", id: 1, created: 9 },
    "821829a2" + "626964" + "01" + "67" + "63726561746564" + "09",
    "host",
  ],
  [
    // [41, {id: 2, error: 12}] (busy)
    "action-result with error",
    { type: "action-result", id: 2, error: ErrorCode.busy },
    "821829a2" + "626964" + "02" + "656572726f72" + "0c",
    "host",
  ],
  [
    // [42, {id: 3, kind: 0, processes: ["vim"]}]
    "confirm-request",
    { type: "confirm-request", id: 3, kind: 0, processes: ["vim"] },
    "82182aa3" +
      "626964" +
      "03" +
      "646b696e64" +
      "00" +
      "69" +
      "70726f636573736573" +
      "81" +
      "63" +
      "76696d",
    "host",
  ],
  [
    // [43, {id: 3, accept: true}]
    "confirm",
    { type: "confirm", id: 3, accept: true },
    "82182ba2" + "626964" + "03" + "66" + "616363657074" + "f5",
    "client",
  ],
];

describe("input and action: exact bytes", () => {
  it.each(CASES)("%s: encodes to the hand-made hex and decodes back", (_n, message, expected, from) => {
    expect(hex(encodeMessage(message))).toBe(expected);
    expect(success(decodeMessage(fromHex(expected), from))).toEqual(message);
  });
});

const ALL_ACTIONS: readonly ActionBody[] = [
  { type: "new-tab", window: 1 },
  { type: "new-tab", window: 1, group: 2, profile: "auto:pwsh" },
  { type: "new-tab", window: 1, group: null },
  { type: "new-tab", window: 1, group: null, profile: "auto:pwsh" },
  { type: "close-tab", tab: 5 },
  { type: "close-pane", pane: 6 },
  { type: "rename-tab", tab: 5, title: null },
  { type: "rename-tab", tab: 5, title: "build" },
  { type: "move-tab-to-group", tab: 5, group: null },
  { type: "move-tab-to-group", tab: 5, group: 3 },
  { type: "move-tab-to-window", tab: 5, window: 2 },
  { type: "new-group", tabs: [1, 2], name: "g" },
  { type: "new-group", tabs: [9], name: "servers", color: 3 },
  { type: "rename-group", group: 3, name: "x" },
  { type: "set-group-color", group: 3, color: 4 },
  { type: "collapse-group", group: 3, collapsed: true },
  { type: "collapse-group", group: 3, collapsed: false },
  { type: "move-group-to-window", group: 3, window: 2 },
  { type: "ungroup", group: 3 },
  { type: "start-pane", pane: 8 },
  { type: "restore-session", session: 7 },
  { type: "restore-session", session: 7, window: 3 },
];

describe("action round trips", () => {
  it("covers the 14 action types", () => {
    expect(new Set(ALL_ACTIONS.map((a) => a.type))).toEqual(new Set(ACTION_TYPES));
  });

  it.each(ALL_ACTIONS.map((a, i) => [i, a] as const))("action body #%i (%o)", (_i, body) => {
    const message: Message = { type: "action", id: 11, do: body };
    const bytes = encodeMessage(message);
    expect(success(decodeMessage(bytes, "client"))).toEqual(message);
  });

  it("round trips the complete forms of the other messages", () => {
    const messages: ReadonlyArray<readonly [Message, Direction]> = [
      [{ type: "key", pane: 1, key: NamedKey.f12, mods: MOD.ctrl | MOD.alt | MOD.shift | MOD.meta }, "client"],
      [{ type: "key", pane: 1, key: "\u{1f600}", mods: 0 }, "client"],
      [{ type: "text-input", pane: 1, text: "" }, "client"],
      [{ type: "paste", pane: 1, text: "line 1\nline 2\n\u{1f600}" }, "client"],
      [{ type: "action-result", id: 1 }, "host"],
      [{ type: "action-result", id: 1, error: ErrorCode.lastTab, created: 4 }, "host"],
      [{ type: "action-result", id: 1, error: ErrorCode.unknownSession }, "host"],
      [{ type: "action-result", id: 1, error: ErrorCode.sessionUnreadable }, "host"],
      [{ type: "confirm-request", id: 3, kind: 1, processes: [] }, "host"],
      [{ type: "confirm-request", id: 3, kind: 0, processes: ["vim", "node"] }, "host"],
      [{ type: "confirm", id: 3, accept: false }, "client"],
    ];
    for (const [message, from] of messages) {
      expect(success(decodeMessage(encodeMessage(message), from))).toEqual(message);
    }
  });

  it("keeps a new-tab without optional keys free of them", () => {
    const decoded = success(
      decodeMessage(encodeMessage({ type: "action", id: 1, do: { type: "new-tab", window: 1 } }), "client"),
    );
    if (decoded.type !== "action" || decoded.do.type !== "new-tab") {
      throw new Error("wrong message");
    }
    expect("group" in decoded.do).toBe(false);
    expect("profile" in decoded.do).toBe(false);
  });

  // spec 0.7: `"group": null` is a loose tab and goes on the wire (f6); it sorts before "window".
  it("new-tab with a null group (a loose tab): hand-made hex, and null survives the round trip", () => {
    const message: Message = { type: "action", id: 1, do: { type: "new-tab", window: 1, group: null } };
    expect(hex(encodeMessage(message))).toBe("821828a262646f8200a26567726f7570f66677696e646f770162696401");
    const decoded = success(decodeMessage(encodeMessage(message), "client"));
    if (decoded.type !== "action" || decoded.do.type !== "new-tab") {
      throw new Error("wrong message");
    }
    expect(decoded.do.group).toBeNull();
  });
});

// restore-session (spec 03 §7), action code 14. Hex by hand, as in the Rust crate: the optional
// "window" is left out, never null; with it, "window" (66 ..) sorts before "session" (67 ..).
describe("restore-session", () => {
  const WITHOUT_WINDOW_HEX = "821828a2" + "62646f" + "820ea1" + "6773657373696f6e" + "07" + "626964" + "01";
  const WITH_WINDOW_HEX =
    "821828a2" + "62646f" + "820ea2" + "6677696e646f77" + "03" + "6773657373696f6e" + "07" + "626964" + "01";

  it("encodes and decodes with and without window", () => {
    const cases: ReadonlyArray<readonly [Message, string]> = [
      [{ type: "action", id: 1, do: { type: "restore-session", session: 7 } }, WITHOUT_WINDOW_HEX],
      [{ type: "action", id: 1, do: { type: "restore-session", session: 7, window: 3 } }, WITH_WINDOW_HEX],
    ];
    for (const [message, bytesHex] of cases) {
      expect(hex(encodeMessage(message))).toBe(bytesHex);
      expect(success(decodeMessage(fromHex(bytesHex), "client"))).toEqual(message);
    }
  });

  it("leaves window out of the decoded action when absent", () => {
    const decoded = success(decodeMessage(fromHex(WITHOUT_WINDOW_HEX), "client"));
    if (decoded.type !== "action" || decoded.do.type !== "restore-session") {
      throw new Error("wrong message");
    }
    expect("window" in decoded.do).toBe(false);
  });

  it("refuses a missing session, and a window that is null or not a uint", () => {
    expect(failure(decodeMessage(wire(40, [["id", 1], ["do", [14, map([])]]]), "client"))).toBe("missing-key");
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [14, map([["session", 7], ["window", null]])]]]), "client")),
    ).toBe("wrong-type");
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [14, map([["session", 7], ["window", "3"]])]]]), "client")),
    ).toBe("wrong-type");
  });

  it("keeps 13 and 15 refused as action codes", () => {
    for (const code of [13, 15]) {
      expect(
        failure(decodeMessage(wire(40, [["id", 1], ["do", [code, map([["session", 7]])]]]), "client")),
      ).toBe("out-of-domain");
    }
  });
});

// view-tab (spec 03 §4.4), type 13, client to host. Hex by hand: [13, {"tab": 5}]
// is 82 0d a1 63 "tab" 05; with null the value is f6; a uint of 2^53-1 is 1b + 8 bytes.
describe("view-tab", () => {
  const VIEW_TAB_5_HEX = "820da16374616205";
  const VIEW_TAB_NULL_HEX = "820da163746162f6";
  const VIEW_TAB_MAX_HEX = "820da1637461621b001fffffffffffff";

  it("encodes and decodes a tab, null and the largest id", () => {
    const cases: ReadonlyArray<readonly [Message, string]> = [
      [{ type: "view-tab", tab: 5 }, VIEW_TAB_5_HEX],
      [{ type: "view-tab", tab: null }, VIEW_TAB_NULL_HEX],
      [{ type: "view-tab", tab: Number.MAX_SAFE_INTEGER }, VIEW_TAB_MAX_HEX],
    ];
    for (const [message, bytesHex] of cases) {
      expect(hex(encodeMessage(message))).toBe(bytesHex);
      expect(success(decodeMessage(fromHex(bytesHex), "client"))).toEqual(message);
    }
  });

  it("refuses a missing tab with missing-key", () => {
    expect(failure(decodeMessage(wire(13, []), "client"))).toBe("missing-key");
  });

  it("refuses a tab that is neither a uint nor null", () => {
    expect(failure(decodeMessage(wire(13, [["tab", "5"]]), "client"))).toBe("wrong-type");
    expect(failure(decodeMessage(wire(13, [["tab", -1]]), "client"))).toBe("wrong-type");
    expect(failure(decodeMessage(wire(13, [["tab", true]]), "client"))).toBe("wrong-type");
  });

  it("ignores an unknown key and drops it on re-encode", () => {
    const bytes = wire(13, [["zz", true], ["tab", null]]);
    expect(hex(bytes)).toBe("820da2627a7af563746162f6");
    const decoded = success(decodeMessage(bytes, "client"));
    expect(decoded).toEqual({ type: "view-tab", tab: null });
    expect(hex(encodeMessage(decoded))).toBe(VIEW_TAB_NULL_HEX);
  });

  it("refuses an out-of-order body as unordered-keys", () => {
    expect(failure(decodeMessage(fromHex("820da263746162f6627a7af5"), "client"))).toBe("unordered-keys");
  });
});

// list-sessions and sessions (spec 03 §4.5), types 14 (client to host) and 15 (host to client).
// Hex by hand, as in the Rust crate. Keys sort by length, then bytes: id, list, mode; in an
// entry: name, session, readable.
describe("list-sessions and sessions", () => {
  const LIST_SESSIONS_5_HEX = "820ea162696405";
  const SESSIONS_ONE_HEX =
    "820fa3" +
    "626964" + "01" +
    "646c697374" + "81a3" +
    "646e616d65" + "6161" +
    "6773657373696f6e" + "02" +
    "687265616461626c65" + "f5" +
    "646d6f6465" + "01";
  const SESSIONS_EMPTY_HEX = "820fa3" + "626964" + "00" + "646c697374" + "80" + "646d6f6465" + "00";

  it("encodes and decodes list-sessions, including the largest id", () => {
    const cases: ReadonlyArray<readonly [Message, string]> = [
      [{ type: "list-sessions", id: 5 }, LIST_SESSIONS_5_HEX],
      [{ type: "list-sessions", id: 0 }, "820ea162696400"],
      [{ type: "list-sessions", id: Number.MAX_SAFE_INTEGER }, "820ea1626964" + "1b001fffffffffffff"],
    ];
    for (const [message, bytesHex] of cases) {
      expect(hex(encodeMessage(message))).toBe(bytesHex);
      expect(success(decodeMessage(fromHex(bytesHex), "client"))).toEqual(message);
    }
  });

  it("encodes and decodes sessions: one entry, an empty list and an unreadable entry", () => {
    const cases: ReadonlyArray<readonly [Message, string]> = [
      [
        {
          type: "sessions",
          id: 1,
          mode: SessionsMode.append,
          list: [{ session: 2, name: "a", readable: true }],
        },
        SESSIONS_ONE_HEX,
      ],
      [{ type: "sessions", id: 0, mode: SessionsMode.newWindow, list: [] }, SESSIONS_EMPTY_HEX],
    ];
    for (const [message, bytesHex] of cases) {
      expect(hex(encodeMessage(message))).toBe(bytesHex);
      expect(success(decodeMessage(fromHex(bytesHex), "host"))).toEqual(message);
    }
    const complete: Message = {
      type: "sessions",
      id: Number.MAX_SAFE_INTEGER,
      mode: SessionsMode.append,
      list: [
        { session: Number.MAX_SAFE_INTEGER, name: "dev", readable: true },
        { session: 900, name: "", readable: false },
      ],
    };
    expect(success(decodeMessage(encodeMessage(complete), "host"))).toEqual(complete);
  });

  it("refuses the wrong direction", () => {
    expect(failure(decodeMessage(fromHex(LIST_SESSIONS_5_HEX), "host"))).toBe("wrong-direction");
    expect(failure(decodeMessage(fromHex(SESSIONS_EMPTY_HEX), "client"))).toBe("wrong-direction");
    expect(messageSender("list-sessions")).toBe("client");
    expect(messageSender("sessions")).toBe("host");
  });

  it("refuses missing keys and wrong types", () => {
    expect(failure(decodeMessage(wire(14, []), "client"))).toBe("missing-key");
    expect(failure(decodeMessage(wire(14, [["id", "5"]]), "client"))).toBe("wrong-type");
    const entry = map([["session", 2], ["name", "a"], ["readable", true]]);
    expect(failure(decodeMessage(wire(15, [["list", []], ["mode", 0]]), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(wire(15, [["id", 1], ["mode", 0]]), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(wire(15, [["id", 1], ["list", []]]), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(wire(15, [["id", 1], ["list", [map([["session", 2], ["name", "a"]])]], ["mode", 0]]), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(wire(15, [["id", 1], ["list", [map([["session", 2], ["name", "a"], ["readable", 1]])]], ["mode", 0]]), "host"))).toBe("wrong-type");
    expect(failure(decodeMessage(wire(15, [["id", 1], ["list", entry], ["mode", 0]]), "host"))).toBe("wrong-type");
  });

  it("refuses a mode outside the domain", () => {
    expect(failure(decodeMessage(wire(15, [["id", 1], ["list", []], ["mode", 2]]), "host"))).toBe("out-of-domain");
  });

  it("ignores an unknown key", () => {
    const bytes = wire(14, [["zz", true], ["id", 5]]);
    expect(success(decodeMessage(bytes, "client"))).toEqual({ type: "list-sessions", id: 5 });
  });
});

describe("missing required keys", () => {
  it("refuses each", () => {
    const cases: ReadonlyArray<readonly [Uint8Array, Direction]> = [
      [wire(30, [["pane", 7], ["key", 0]]), "client"], // no mods
      [wire(30, [["pane", 7], ["mods", 0]]), "client"], // no key
      [wire(30, [["key", 0], ["mods", 0]]), "client"], // no pane
      [wire(31, [["pane", 7]]), "client"],
      [wire(32, [["text", "x"]]), "client"],
      [wire(40, [["id", 1]]), "client"], // no do
      [wire(40, [["do", [11, map([["group", 3]])]]]), "client"], // no id
      [wire(40, [["id", 1], ["do", [0, map([])]]]), "client"], // new-tab without window
      [wire(40, [["id", 1], ["do", [3, map([["tab", 5]])]]]), "client"], // rename-tab without title
      [wire(40, [["id", 1], ["do", [4, map([["tab", 5]])]]]), "client"], // move-tab-to-group without group
      [wire(40, [["id", 1], ["do", [6, map([["tabs", [1]]])]]]), "client"], // new-group without name
      [wire(40, [["id", 1], ["do", [9, map([["group", 3]])]]]), "client"], // collapse-group without collapsed
      [wire(41, [["error", 12]]), "host"],
      [wire(42, [["id", 3], ["kind", 0]]), "host"], // no processes
      [wire(43, [["id", 3]]), "client"],
    ];
    for (const [bytes, from] of cases) {
      expect(failure(decodeMessage(bytes, from))).toBe("missing-key");
    }
  });
});

describe("values outside the domain", () => {
  it("refuses them with out-of-domain", () => {
    const cases: ReadonlyArray<readonly [string, Uint8Array, Direction]> = [
      ["unassigned named key 14", wire(30, [["pane", 7], ["key", 14], ["mods", 0]]), "client"],
      ["named key 20", wire(30, [["pane", 7], ["key", 20], ["mods", 0]]), "client"],
      ["named key 33", wire(30, [["pane", 7], ["key", 33], ["mods", 0]]), "client"],
      ["key text with two scalars", wire(30, [["pane", 7], ["key", "ab"], ["mods", 0]]), "client"],
      ["key text empty", wire(30, [["pane", 7], ["key", ""], ["mods", 0]]), "client"],
      [
        "key text with a base letter and a combining mark",
        wire(30, [["pane", 7], ["key", "é"], ["mods", 0]]),
        "client",
      ],
      ["unknown action code", wire(40, [["id", 1], ["do", [15, map([])]]]), "client"],
      // 13 was activate-tab in 0.4; removed in 0.5 (ADR-0011) and never reused.
      ["removed activate-tab code 13", wire(40, [["id", 17], ["do", [13, map([["tab", 5]])]]]), "client"],
      ["removed activate-tab code 13, empty body", wire(40, [["id", 1], ["do", [13, map([])]]]), "client"],
      [
        "new-group with no tabs",
        wire(40, [["id", 1], ["do", [6, map([["tabs", []], ["name", "g"]])]]]),
        "client",
      ],
      ["action-result with the reserved error 2", wire(41, [["id", 1], ["error", 2]]), "host"],
      ["action-result with error 19", wire(41, [["id", 1], ["error", 19]]), "host"],
      ["confirm-request kind 2", wire(42, [["id", 3], ["kind", 2], ["processes", []]]), "host"],
    ];
    for (const [name, bytes, from] of cases) {
      expect(failure(decodeMessage(bytes, from)), name).toBe("out-of-domain");
    }
  });

  it("accepts a key text of one astral scalar value", () => {
    const m = success(decodeMessage(wire(30, [["pane", 7], ["key", "\u{1f600}"], ["mods", 0]]), "client"));
    expect(m).toEqual({ type: "key", pane: 7, key: "\u{1f600}", mods: 0 });
  });
});

describe("wrong types and shapes", () => {
  it("refuses them", () => {
    // key as bytes, as bool, as null
    expect(failure(decodeMessage(wire(30, [["pane", 7], ["key", new Uint8Array([1])], ["mods", 0]]), "client"))).toBe(
      "wrong-type",
    );
    expect(failure(decodeMessage(wire(30, [["pane", 7], ["key", true], ["mods", 0]]), "client"))).toBe("wrong-type");
    expect(failure(decodeMessage(wire(30, [["pane", 7], ["key", null], ["mods", 0]]), "client"))).toBe("wrong-type");
    // negative named key
    expect(failure(decodeMessage(wire(30, [["pane", 7], ["key", -1], ["mods", 0]]), "client"))).toBe("wrong-type");
    // text-input with a number
    expect(failure(decodeMessage(wire(31, [["pane", 7], ["text", 5]]), "client"))).toBe("wrong-type");
    // action-body with three elements, and not as an array
    expect(failure(decodeMessage(wire(40, [["id", 1], ["do", [11, map([["group", 3]]), 0]]]), "client"))).toBe(
      "wrong-arity",
    );
    expect(failure(decodeMessage(wire(40, [["id", 1], ["do", 11]]), "client"))).toBe("wrong-type");
    // action-body whose second element is not a map
    expect(failure(decodeMessage(wire(40, [["id", 1], ["do", [11, [3]]]]), "client"))).toBe("wrong-type");
    // rename-tab title as a number; collapsed as a number
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [3, map([["tab", 5], ["title", 1]])]]]), "client")),
    ).toBe("wrong-type");
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [9, map([["group", 3], ["collapsed", 1]])]]]), "client")),
    ).toBe("wrong-type");
    // new-tab with a text group (null is a loose tab, a uint is a group; nothing else)
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [0, map([["window", 1], ["group", "g"]])]]]), "client")),
    ).toBe("wrong-type");
    // move-tab-to-group with a text group
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [4, map([["tab", 5], ["group", "x"]])]]]), "client")),
    ).toBe("wrong-type");
    // processes with a non-text entry
    expect(failure(decodeMessage(wire(42, [["id", 3], ["kind", 0], ["processes", [1]]]), "host"))).toBe("wrong-type");
    // action-result created as text
    expect(failure(decodeMessage(wire(41, [["id", 1], ["created", "9"]]), "host"))).toBe("wrong-type");
  });

  it("refuses the messages from the wrong side", () => {
    expect(failure(decodeMessage(encodeMessage({ type: "view-tab", tab: 5 }), "host"))).toBe("wrong-direction");
    expect(failure(decodeMessage(encodeMessage({ type: "confirm", id: 3, accept: true }), "host"))).toBe(
      "wrong-direction",
    );
    expect(failure(decodeMessage(encodeMessage({ type: "action-result", id: 1 }), "client"))).toBe(
      "wrong-direction",
    );
    expect(failure(decodeMessage(encodeMessage({ type: "text-input", pane: 1, text: "a" }), "host"))).toBe(
      "wrong-direction",
    );
  });
});

describe("unknown keys are ignored", () => {
  it("in every message and in every action body", () => {
    const extra: [string, CborValue] = ["zz_extra", [1, map([["a", 2]])]];
    const cases: ReadonlyArray<readonly [Uint8Array, Message, Direction]> = [
      [
        wire(30, [["pane", 7], ["key", 0], ["mods", 0], extra]),
        { type: "key", pane: 7, key: 0, mods: 0 },
        "client",
      ],
      [
        wire(31, [["pane", 7], ["text", "ls"], extra]),
        { type: "text-input", pane: 7, text: "ls" },
        "client",
      ],
      [
        wire(32, [["pane", 7], ["text", "hi"], extra]),
        { type: "paste", pane: 7, text: "hi" },
        "client",
      ],
      [
        wire(40, [["id", 1], ["do", [0, map([["window", 1], extra])]], extra]),
        { type: "action", id: 1, do: { type: "new-tab", window: 1 } },
        "client",
      ],
      [
        wire(40, [["id", 2], ["do", [12, map([["pane", 4], extra])]]]),
        { type: "action", id: 2, do: { type: "start-pane", pane: 4 } },
        "client",
      ],
      [
        wire(41, [["id", 1], ["created", 9], extra]),
        { type: "action-result", id: 1, created: 9 },
        "host",
      ],
      [
        wire(42, [["id", 3], ["kind", 0], ["processes", ["vim"]], extra]),
        { type: "confirm-request", id: 3, kind: 0, processes: ["vim"] },
        "host",
      ],
      [
        wire(43, [["id", 3], ["accept", true], extra]),
        { type: "confirm", id: 3, accept: true },
        "client",
      ],
    ];
    for (const [bytes, message, from] of cases) {
      expect(success(decodeMessage(bytes, from))).toEqual(message);
    }
  });

  it("still requires canonical form", () => {
    // [43, {id: 3, accept: true}] with the keys swapped ("accept" before "id")
    const unordered = fromHex("82182ba2" + "66616363657074" + "f5" + "626964" + "03");
    expect(failure(decodeMessage(unordered, "client"))).toBe("unordered-keys");
  });
});
