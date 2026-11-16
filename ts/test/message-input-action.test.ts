// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { CborMap, encodeCbor, type CborValue } from "../src/cbor/index.js";
import {
  ACTION_TYPES,
  decodeMessage,
  encodeMessage,
  ErrorCode,
  MOD,
  NamedKey,
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
];

describe("action round trips", () => {
  it("covers the 13 action types", () => {
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
      ["unknown action code", wire(40, [["id", 1], ["do", [13, map([])]]]), "client"],
      [
        "new-group with no tabs",
        wire(40, [["id", 1], ["do", [6, map([["tabs", []], ["name", "g"]])]]]),
        "client",
      ],
      ["action-result with the reserved error 2", wire(41, [["id", 1], ["error", 2]]), "host"],
      ["action-result with error 17", wire(41, [["id", 1], ["error", 17]]), "host"],
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
    // new-tab group present but null (optional keys are omitted, never null)
    expect(
      failure(decodeMessage(wire(40, [["id", 1], ["do", [0, map([["window", 1], ["group", null]])]]]), "client")),
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
