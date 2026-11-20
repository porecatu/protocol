// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { CborMap, encodeCbor } from "../src/cbor/index.js";
import {
  CONFIRM_KINDS,
  CURSOR_SHAPES,
  decodeMessage,
  ERROR_CODES,
  encodeMessage,
  GOODBYE_REASONS,
  messageSender,
  messageTypeCode,
  MessageType,
  MESSAGE_TYPE_CODES,
  MOD,
  NAMED_KEYS,
  NamedKey,
  PAIR_REJECT_REASONS,
  PANE_STATES,
  PROFILE_KINDS,
  SPLIT_DIRS,
  STYLE,
  SUBSCRIPTION_END_REASONS,
  type DecodeResult,
  type Direction,
  type DecodeErrorReason,
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

// Every hex below was assembled by hand from RFC 8949 (not produced by the
// encoder): the head byte of a text key is 0x60 + its length, and map keys sort
// by their encoded bytes, which here means by that head byte first.

/** [0, {app: "porecatu-remote", version: [0, 1], app_version: "0.1.0"}] */
const HELLO_HEX =
  "8200a3" +
  "63617070" + // "app"
  "6f706f726563617475" + // 0x6f = text(15), "porecatu
  "2d72656d6f7465" + // -remote"
  "67" +
  "76657273696f6e" + // "version"
  "820001" + // [0, 1]
  "6b" +
  "6170705f76657273696f6e" + // "app_version"
  "65" +
  "302e312e30"; // "0.1.0"

const HELLO: Message = {
  type: "hello",
  version: [0, 1],
  app: "porecatu-remote",
  appVersion: "0.1.0",
};

/**
 * [1, {version: [0, 1], host_name: "pc", host_instance: h'07 x16',
 *      porecatu_version: "0.9", max_subscriptions: 4}]
 * Key head bytes: 67 < 69 < 6d < 70 < 71.
 */
const WELCOME_HEX =
  "8201a5" +
  "6776657273696f6e" +
  "820001" +
  "69686f73745f6e616d65" +
  "627063" +
  "6d686f73745f696e7374616e6365" +
  "50" +
  "07".repeat(16) +
  "70706f726563617475" +
  "5f76657273696f6e" +
  "63302e39" +
  "716d61785f737562736372697074696f6e73" +
  "04";

const WELCOME: Message = {
  type: "welcome",
  version: [0, 1],
  hostName: "pc",
  porecatuVersion: "0.9",
  maxSubscriptions: 4,
  hostInstance: new Uint8Array(16).fill(7),
};

/** [2, {version: [1, 0]}] */
const INCOMPATIBLE_HEX = "8202a1" + "6776657273696f6e" + "820100";

const INCOMPATIBLE: Message = { type: "incompatible", version: [1, 0] };

/** [3, {reason: 2}] (revoked) */
const GOODBYE_HEX = "8203a1" + "66726561736f6e" + "02";

const GOODBYE: Message = { type: "goodbye", reason: 2 };

/** [70, {id: 5, code: 6, pane: 9}]; 70 = 0x18 0x46. Key head bytes: 62 < 64 ("code") < 64 ("pane"). */
const ERROR_FULL_HEX =
  "821846a3" + "626964" + "05" + "64636f6465" + "06" + "6470616e65" + "09";

const ERROR_FULL: Message = { type: "error", code: 6, id: 5, pane: 9 };

/** [70, {code: 0}] */
const ERROR_MIN_HEX = "821846a1" + "64636f6465" + "00";

const ERROR_MIN: Message = { type: "error", code: 0 };

const IMPLEMENTED: ReadonlyArray<readonly [string, Message, string, Direction]> = [
  ["hello", HELLO, HELLO_HEX, "client"],
  ["welcome", WELCOME, WELCOME_HEX, "host"],
  ["incompatible", INCOMPATIBLE, INCOMPATIBLE_HEX, "host"],
  ["goodbye (from host)", GOODBYE, GOODBYE_HEX, "host"],
  ["goodbye (from client)", GOODBYE, GOODBYE_HEX, "client"],
  ["error with id and pane", ERROR_FULL, ERROR_FULL_HEX, "host"],
  ["error without optional keys", ERROR_MIN, ERROR_MIN_HEX, "client"],
];

describe("implemented bodies", () => {
  it.each(IMPLEMENTED)("%s: exact bytes and round trip", (_name, message, expected, from) => {
    expect(hex(encodeMessage(message))).toBe(expected);
    expect(success(decodeMessage(fromHex(expected), from))).toEqual(message);
  });

  it("ignores unknown keys but still requires canonical form", () => {
    // "extra" (head 0x65) sorts between "app" (0x63) and "version" (0x67).
    const bytes = encodeCbor([
      0,
      new CborMap([
        ["app", "porecatu-remote"],
        ["version", [0, 1]],
        ["app_version", "0.1.0"],
        ["extra", [1, 2, 3]],
      ]),
    ]);
    expect(success(decodeMessage(bytes, "client"))).toEqual(HELLO);
    // Same bytes with the keys out of order must be refused.
    const unordered = fromHex(
      "8200a2" + "6776657273696f6e" + "820001" + "63617070" + "6161",
    );
    expect(failure(decodeMessage(unordered, "client"))).toBe("unordered-keys");
  });

  it("refuses a missing required key", () => {
    const bytes = encodeCbor([
      0,
      new CborMap([
        ["app", "porecatu-remote"],
        ["version", [0, 1]],
      ]),
    ]);
    expect(failure(decodeMessage(bytes, "client"))).toBe("missing-key");
  });

  it("refuses values outside their domain", () => {
    const cases: ReadonlyArray<readonly [Uint8Array, Direction]> = [
      // goodbye reason 4 (only 0..3 exist)
      [encodeCbor([3, new CborMap([["reason", 4]])]), "host"],
      // error code 2 is reserved
      [encodeCbor([70, new CborMap([["code", 2]])]), "host"],
      // error code 17 does not exist yet
      [encodeCbor([70, new CborMap([["code", 17]])]), "host"],
      // max_subscriptions 0
      [
        encodeCbor([
          1,
          new CborMap([
            ["version", [0, 1]],
            ["host_name", "pc"],
            ["porecatu_version", "0.9"],
            ["max_subscriptions", 0],
            ["host_instance", new Uint8Array(16)],
          ]),
        ]),
        "host",
      ],
      // host_instance of 15 bytes
      [
        encodeCbor([
          1,
          new CborMap([
            ["version", [0, 1]],
            ["host_name", "pc"],
            ["porecatu_version", "0.9"],
            ["max_subscriptions", 4],
            ["host_instance", new Uint8Array(15)],
          ]),
        ]),
        "host",
      ],
    ];
    for (const [bytes, from] of cases) {
      expect(failure(decodeMessage(bytes, from))).toBe("out-of-domain");
    }
  });

  it("refuses values of the wrong type or shape", () => {
    // version with three elements
    expect(
      failure(
        decodeMessage(
          encodeCbor([
            2,
            new CborMap([["version", [1, 0, 0]]]),
          ]),
          "host",
        ),
      ),
    ).toBe("wrong-arity");
    // reason as text
    expect(
      failure(decodeMessage(encodeCbor([3, new CborMap([["reason", "normal"]])]), "host")),
    ).toBe("wrong-type");
    // optional key present but null
    expect(
      failure(
        decodeMessage(
          encodeCbor([70, new CborMap([["code", 0], ["id", null]])]),
          "host",
        ),
      ),
    ).toBe("wrong-type");
  });
});

describe("envelope and dispatch", () => {
  it("refuses an unknown type", () => {
    // [99, {}]: 99 = 0x18 0x63
    expect(failure(decodeMessage(fromHex("8218" + "63" + "a0"), "host"))).toBe(
      "unknown-type",
    );
    // 4 sits in a gap of the table
    expect(failure(decodeMessage(fromHex("8204a0"), "host"))).toBe("unknown-type");
  });

  it("refuses a known type from the wrong side", () => {
    expect(failure(decodeMessage(fromHex(HELLO_HEX), "host"))).toBe("wrong-direction");
    expect(failure(decodeMessage(fromHex(WELCOME_HEX), "client"))).toBe(
      "wrong-direction",
    );
    expect(failure(decodeMessage(fromHex(INCOMPATIBLE_HEX), "client"))).toBe(
      "wrong-direction",
    );
  });

  it("refuses a wrong envelope", () => {
    expect(failure(decodeMessage(fromHex("83" + "00" + "a0" + "00"), "client"))).toBe(
      "wrong-arity",
    );
    expect(failure(decodeMessage(fromHex("80"), "client"))).toBe("wrong-arity");
    expect(failure(decodeMessage(fromHex("a0"), "client"))).toBe("wrong-type");
    // body is not a map
    expect(failure(decodeMessage(fromHex("8200" + "80"), "client"))).toBe("wrong-type");
    // type is not an integer
    expect(failure(decodeMessage(fromHex("8261" + "30" + "a0"), "client"))).toBe(
      "wrong-type",
    );
    // negative type
    expect(failure(decodeMessage(fromHex("8220" + "a0"), "client"))).toBe("wrong-type");
  });

  it("refuses non-canonical, indefinite and trailing CBOR, and empty input", () => {
    // type 0 in the two-byte form
    expect(failure(decodeMessage(fromHex("82" + "1800" + "a0"), "client"))).toBe(
      "non-canonical",
    );
    // indefinite-length array
    expect(failure(decodeMessage(fromHex("9f" + "00" + "a0" + "ff"), "client"))).toBe(
      "indefinite-length",
    );
    expect(failure(decodeMessage(fromHex(GOODBYE_HEX + "00"), "host"))).toBe(
      "trailing-bytes",
    );
    expect(failure(decodeMessage(new Uint8Array(0), "host"))).toBe("truncated");
  });

  it("never throws on a damaged valid message", () => {
    for (const hexText of [HELLO_HEX, WELCOME_HEX, ERROR_FULL_HEX]) {
      const valid = fromHex(hexText);
      const from: Direction = hexText === HELLO_HEX ? "client" : "host";
      for (let cut = 0; cut < valid.length; cut++) {
        expect(decodeMessage(valid.subarray(0, cut), from).ok).toBe(false);
      }
      for (let i = 0; i < valid.length; i++) {
        const damaged = valid.slice();
        damaged[i] = (damaged[i] ?? 0) ^ 0xff;
        decodeMessage(damaged, from); // any outcome is fine, a throw is not
      }
    }
  });

  it("has a consistent table of 27 messages", () => {
    const names = Object.keys(MessageType) as Array<Message["type"]>;
    expect(names).toHaveLength(27);
    expect(new Set(MESSAGE_TYPE_CODES).size).toBe(27);
    const bySender = { host: 0, client: 0, both: 0 };
    for (const name of names) {
      expect(messageTypeCode(name)).toBe(MessageType[name]);
      bySender[messageSender(name)]++;
    }
    expect(bySender).toEqual({ host: 14, client: 11, both: 2 });
    expect(messageSender("goodbye")).toBe("both");
    expect(messageSender("error")).toBe("both");
    expect(messageSender("hello")).toBe("client");
    expect(messageSender("welcome")).toBe("host");
  });

  it("keeps the spec's numbers", () => {
    expect(MESSAGE_TYPE_CODES).toEqual([
      0, 1, 2, 3, 10, 11, 12, 13, 20, 21, 22, 23, 24, 25, 26, 27, 30, 31, 32, 40, 41, 42, 43,
      50, 60, 61, 70,
    ]);
    expect(GOODBYE_REASONS).toEqual([0, 1, 2, 3]);
    expect(PANE_STATES).toEqual([0, 1, 2, 3, 4, 5, 6]);
    expect(SPLIT_DIRS).toEqual([0, 1]);
    expect(PROFILE_KINDS).toEqual([0, 1, 2, 3]);
    expect(CURSOR_SHAPES).toEqual([0, 1, 2]);
    expect(SUBSCRIPTION_END_REASONS).toEqual([0, 1]);
    expect(CONFIRM_KINDS).toEqual([0, 1]);
    expect(PAIR_REJECT_REASONS).toEqual([0, 1, 2]);
    expect(ERROR_CODES).toEqual([0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    expect(NAMED_KEYS).toHaveLength(26);
    expect(NamedKey.pageUp).toBe(10);
    expect(NamedKey.delete).toBe(13);
    expect(NamedKey.f1).toBe(21);
    expect(NamedKey.f12).toBe(32);
    expect(STYLE).toEqual({
      bold: 1,
      italic: 2,
      underline: 4,
      inverse: 8,
      dim: 16,
      strike: 32,
      hidden: 64,
    });
    expect(MOD).toEqual({ ctrl: 1, alt: 2, shift: 4, meta: 8 });
  });
});

describe("bell", () => {
  // The placeholder tests that lived here were replaced when `bell` was written.
  it("encodes and decodes [50, {pane: 1}]", () => {
    // 50 = 0x18 0x32
    const bytes = "821832" + "a1" + "6470616e65" + "01";
    expect(hex(encodeMessage({ type: "bell", pane: 1 }))).toBe(bytes);
    expect(success(decodeMessage(fromHex(bytes), "host"))).toEqual({ type: "bell", pane: 1 });
  });
});
