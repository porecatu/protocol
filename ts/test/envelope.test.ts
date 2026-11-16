// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  CLOSE_CODE,
  CLOSE_REASONS,
  CloseReason,
  EnvelopeError,
  HandshakeStartKind,
  MAX_FRAME,
  NONCE_LEN,
  REGISTER_CONTEXT,
  channelDataLength,
  clientCloseCode,
  decodeClientEnvelope,
  decodeHandshakeStart,
  decodeHostEnvelope,
  encodeClientEnvelope,
  encodeHandshakeStart,
  encodeHostEnvelope,
  registerMessage,
  type ClientEnvelope,
  type EnvelopeErrorReason,
  type HostEnvelope,
} from "../src/envelope.js";

// Every expected frame below was written by hand from the CDDL in spec 01 §4
// and RFC 8949, not produced by the encoder under test. Reading aid:
//   82 / 83 / 81      array of 2 / 3 / 1 items
//   00 .. 06, 10 .. 12  the envelope type code (0-6 host family, 16-18 client)
//   58 20 / 58 40     byte string of 32 / 64 bytes
//   78 20             text string of 32 bytes
//   43 / 42 / 40      byte string of 3 / 2 / 0 bytes
//   19 03 e8          unsigned integer 1000 (two-byte argument)
// The Rust implementation (rust/src/envelope.rs) repeats these same frames.

function fromHex(text: string): Uint8Array {
  const clean = text.replace(/\s+/g, "");
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** 32 bytes 00 01 .. 1f. */
const NONCE_HEX = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
/** 32 bytes 20 21 .. 3f. */
const ROUTING_KEY_HEX = "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f";
/** 64 bytes 40 41 .. 7f. */
const SIGNATURE_HEX =
  "404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f" +
  "606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f";
/** Host id of the routing key 00 01 .. 1f (same value as hostId.test.ts). */
const HOST_ID = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";
/** ASCII of HOST_ID, one byte per character. */
const HOST_ID_HEX = "61776266 6d623678 37787a6e 716c7875 7970656d 666c766a 6d67777a 7276716f";

const NONCE = fromHex(NONCE_HEX);
const ROUTING_KEY = fromHex(ROUTING_KEY_HEX);
const SIGNATURE = fromHex(SIGNATURE_HEX);

const HOST_CASES: ReadonlyArray<readonly [string, HostEnvelope, string]> = [
  ["challenge", { type: "challenge", nonce: NONCE }, `82 00 5820 ${NONCE_HEX}`],
  [
    "register",
    { type: "register", routingKey: ROUTING_KEY, signature: SIGNATURE },
    `83 01 5820 ${ROUTING_KEY_HEX} 5840 ${SIGNATURE_HEX}`,
  ],
  ["registered", { type: "registered", hostId: HOST_ID }, `82 02 7820 ${HOST_ID_HEX}`],
  ["channel-opened (one-byte channel)", { type: "channel-opened", channel: 5 }, "82 03 05"],
  ["channel-opened (channel 1000)", { type: "channel-opened", channel: 1000 }, "82 03 1903e8"],
  [
    "channel-data",
    { type: "channel-data", channel: 7, payload: fromHex("010203") },
    "83 04 07 43 010203",
  ],
  [
    "channel-data (empty payload)",
    { type: "channel-data", channel: 7, payload: new Uint8Array(0) },
    "83 04 07 40",
  ],
  [
    "channel-closed",
    { type: "channel-closed", channel: 7, reason: CloseReason.hostReplaced },
    "83 05 07 01",
  ],
  [
    "close-channel",
    { type: "close-channel", channel: 7, reason: CloseReason.hostRejected },
    "83 06 07 03",
  ],
];

const CLIENT_CASES: ReadonlyArray<readonly [string, ClientEnvelope, string]> = [
  ["data", { type: "data", payload: fromHex("aabb") }, "82 10 42 aabb"],
  ["data (empty payload)", { type: "data", payload: new Uint8Array(0) }, "82 10 40"],
  ["host-unavailable", { type: "host-unavailable" }, "81 11"],
  ["closed", { type: "closed", reason: CloseReason.limitExceeded }, "82 12 04"],
];

function hostErrorReason(frame: string | Uint8Array): EnvelopeErrorReason {
  return errorReason(() => decodeHostEnvelope(typeof frame === "string" ? fromHex(frame) : frame));
}

function clientErrorReason(frame: string): EnvelopeErrorReason {
  return errorReason(() => decodeClientEnvelope(fromHex(frame)));
}

function errorReason(run: () => unknown): EnvelopeErrorReason {
  try {
    run();
  } catch (error) {
    if (error instanceof EnvelopeError) {
      return error.reason;
    }
    throw error;
  }
  throw new Error("expected an EnvelopeError");
}

describe("constants", () => {
  it("has the close codes of spec 01 §4-§5", () => {
    expect(CLOSE_CODE).toEqual({
      normal: 1000,
      goingAway: 1001,
      unsupportedData: 1003,
      messageTooBig: 1009,
      registrationRejected: 4001,
      replaced: 4002,
      limitExceeded: 4003,
      invalidEnvelope: 4004,
      registrationTimeout: 4005,
    });
  });

  it("lists every close reason in wire order", () => {
    expect(CLOSE_REASONS).toEqual(Object.values(CloseReason));
    expect(CLOSE_REASONS).toEqual([0, 1, 2, 3, 4, 5]);
  });
});

describe("host envelope", () => {
  it.each(HOST_CASES)("encodes %s to the hand-written frame", (_name, envelope, frame) => {
    expect(hex(encodeHostEnvelope(envelope))).toBe(frame.replace(/\s+/g, ""));
  });

  it.each(HOST_CASES)("decodes %s from the hand-written frame", (_name, envelope, frame) => {
    expect(decodeHostEnvelope(fromHex(frame))).toEqual(envelope);
  });

  it("round-trips a payload of every CBOR length form", () => {
    for (const length of [23, 24, 255, 256, 65_535, 65_536]) {
      const payload = new Uint8Array(length).map((_, i) => i & 0xff);
      const frame = encodeHostEnvelope({ type: "channel-data", channel: 1, payload });
      expect(frame.length).toBe(channelDataLength(1, length));
      expect(decodeHostEnvelope(frame)).toEqual({ type: "channel-data", channel: 1, payload });
    }
  });

  it("round-trips the largest channel number a safe integer allows", () => {
    const channel = Number.MAX_SAFE_INTEGER;
    const frame = encodeHostEnvelope({ type: "channel-opened", channel });
    // 82 03 1b 001fffffffffffff
    expect(hex(frame)).toBe("82031b001fffffffffffff");
    expect(decodeHostEnvelope(frame)).toEqual({ type: "channel-opened", channel });
  });

  it("returns the payload as a view into the frame, without copying", () => {
    const outer = new Uint8Array(100);
    const frame = fromHex("83 04 07 43 010203");
    outer.set(frame, 40);
    const view = outer.subarray(40, 40 + frame.length);
    const decoded = decodeHostEnvelope(view);
    if (decoded.type !== "channel-data") {
      throw new Error("expected channel-data");
    }
    expect(decoded.payload.buffer).toBe(outer.buffer);
    expect(decoded.payload.byteOffset).toBe(40 + frame.length - 3);
    expect(Array.from(decoded.payload)).toEqual([1, 2, 3]);
    // A view, not a copy: writing through the frame shows in the payload.
    outer[40 + frame.length - 1] = 0xff;
    expect(decoded.payload[2]).toBe(0xff);
  });

  it("refuses a type that belongs to the other family or is unassigned", () => {
    expect(hostErrorReason("82 10 40")).toBe("unknown-type");
    expect(hostErrorReason("81 11")).toBe("unknown-type");
    expect(hostErrorReason("82 07 00")).toBe("unknown-type");
    expect(hostErrorReason("82 0f 00")).toBe("unknown-type");
    expect(hostErrorReason("82 13 00")).toBe("unknown-type");
  });

  it("refuses the wrong arity", () => {
    expect(hostErrorReason("80")).toBe("wrong-arity");
    expect(hostErrorReason("81 00")).toBe("wrong-arity");
    expect(hostErrorReason(`83 00 5820 ${NONCE_HEX} 00`)).toBe("wrong-arity");
    expect(hostErrorReason("81 03")).toBe("wrong-arity");
    expect(hostErrorReason("83 03 05 05")).toBe("wrong-arity");
    expect(hostErrorReason("82 04 07")).toBe("wrong-arity");
    expect(hostErrorReason("82 05 07")).toBe("wrong-arity");
    expect(hostErrorReason("82 06 07")).toBe("wrong-arity");
    expect(hostErrorReason(`82 01 5820 ${ROUTING_KEY_HEX}`)).toBe("wrong-arity");
  });

  it("refuses a close reason above 5", () => {
    expect(hostErrorReason("83 05 07 06")).toBe("invalid-close-reason");
    expect(hostErrorReason("83 06 07 18 ff")).toBe("invalid-close-reason");
  });

  it("refuses fields of the wrong CBOR type or fixed size", () => {
    expect(hostErrorReason("a0")).toBe("wrong-type");
    expect(hostErrorReason("00")).toBe("wrong-type");
    expect(hostErrorReason("82 61 30 00")).toBe("wrong-type");
    expect(hostErrorReason("82 03 80")).toBe("wrong-type");
    expect(hostErrorReason("82 03 20")).toBe("wrong-type");
    expect(hostErrorReason("83 04 61 31 40")).toBe("wrong-type");
    expect(hostErrorReason("83 04 07 00")).toBe("wrong-type");
    expect(hostErrorReason("83 05 07 61 30")).toBe("wrong-type");
    // 31-byte nonce, 33-byte nonce
    expect(hostErrorReason(`82 00 581f ${NONCE_HEX.slice(0, 62)}`)).toBe("wrong-type");
    expect(hostErrorReason(`82 00 5821 ${NONCE_HEX}00`)).toBe("wrong-type");
    // 32-byte signature slot with 63 bytes
    expect(hostErrorReason(`83 01 5820 ${ROUTING_KEY_HEX} 583f ${SIGNATURE_HEX.slice(0, 126)}`)).toBe(
      "wrong-type",
    );
    expect(hostErrorReason("82 02 00")).toBe("wrong-type");
  });

  it("refuses a malformed host id", () => {
    expect(hostErrorReason("82 02 61 61")).toBe("invalid-host-id");
    expect(hostErrorReason(`82 02 7820 ${HOST_ID_HEX.replace(/\s+/g, "").replace("61", "41")}`)).toBe(
      "invalid-host-id",
    );
  });

  it("refuses trailing bytes and non-canonical forms, naming the CBOR cause", () => {
    const cause = (frame: string): string | undefined => {
      try {
        decodeHostEnvelope(fromHex(frame));
      } catch (error) {
        if (error instanceof EnvelopeError) {
          expect(error.reason).toBe("malformed-cbor");
          return error.cborReason;
        }
      }
      return undefined;
    };
    expect(cause("82 03 05 00")).toBe("trailing-bytes");
    expect(cause("82 03 18 05")).toBe("non-canonical");
    expect(cause("98 02 03 05")).toBe("non-canonical");
    expect(cause("9f 03 05 ff")).toBe("indefinite-length");
    expect(cause("c0 82 03 05")).toBe("tag");
    expect(cause("82 03")).toBe("truncated");
    expect(cause("")).toBe("truncated");
    expect(cause("82 03 f9 0000")).toBe("float");
  });

  it("refuses a frame above MAX_FRAME before parsing it", () => {
    expect(hostErrorReason(new Uint8Array(MAX_FRAME + 1))).toBe("frame-too-large");
  });

  it("accepts a frame of exactly MAX_FRAME bytes", () => {
    // 83 04 <channel 1 byte> 5a <4-byte length> payload: 1 + 1 + 1 + 5 + n
    const payload = new Uint8Array(MAX_FRAME - 8);
    const frame = encodeHostEnvelope({ type: "channel-data", channel: 1, payload });
    expect(frame.length).toBe(MAX_FRAME);
    expect(frame.length).toBe(channelDataLength(1, payload.length));
    expect(decodeHostEnvelope(frame).type).toBe("channel-data");
  });

  it("never puts payload bytes in an error", () => {
    const secret = "deadbeef".repeat(8);
    try {
      decodeHostEnvelope(fromHex(`83 05 5820 ${secret} 02`));
    } catch (error) {
      expect(error).toBeInstanceOf(EnvelopeError);
      const text = JSON.stringify(error) + String((error as Error).message);
      expect(text).not.toContain("deadbeef");
      expect((error as Error).message).toBe("wrong-type");
      return;
    }
    throw new Error("expected an EnvelopeError");
  });

  it("refuses to encode what the wire cannot carry", () => {
    expect(() => encodeHostEnvelope({ type: "challenge", nonce: new Uint8Array(31) })).toThrow(
      RangeError,
    );
    expect(() =>
      encodeHostEnvelope({
        type: "register",
        routingKey: new Uint8Array(32),
        signature: new Uint8Array(63),
      }),
    ).toThrow(RangeError);
    expect(() => encodeHostEnvelope({ type: "registered", hostId: "short" })).toThrow(RangeError);
    expect(() => encodeHostEnvelope({ type: "channel-opened", channel: -1 })).toThrow(RangeError);
    expect(() => encodeHostEnvelope({ type: "channel-opened", channel: 1.5 })).toThrow(RangeError);
    expect(() =>
      encodeHostEnvelope({ type: "close-channel", channel: 1, reason: 6 as CloseReason }),
    ).toThrow(RangeError);
  });
});

describe("client envelope", () => {
  it.each(CLIENT_CASES)("encodes %s to the hand-written frame", (_name, envelope, frame) => {
    expect(hex(encodeClientEnvelope(envelope))).toBe(frame.replace(/\s+/g, ""));
  });

  it.each(CLIENT_CASES)("decodes %s from the hand-written frame", (_name, envelope, frame) => {
    expect(decodeClientEnvelope(fromHex(frame))).toEqual(envelope);
  });

  it("encodes every close reason in `closed`", () => {
    for (const reason of CLOSE_REASONS) {
      const frame = encodeClientEnvelope({ type: "closed", reason });
      expect(hex(frame)).toBe(`8212${reason.toString(16).padStart(2, "0")}`);
      expect(decodeClientEnvelope(frame)).toEqual({ type: "closed", reason });
    }
  });

  it("returns the payload as a view into the frame, without copying", () => {
    const frame = fromHex("82 10 42 aabb");
    const decoded = decodeClientEnvelope(frame);
    if (decoded.type !== "data") {
      throw new Error("expected data");
    }
    expect(decoded.payload.buffer).toBe(frame.buffer);
    expect(decoded.payload.byteOffset).toBe(frame.byteOffset + 3);
    expect(Array.from(decoded.payload)).toEqual([0xaa, 0xbb]);
  });

  it("refuses a type that belongs to the other family or is unassigned", () => {
    expect(clientErrorReason(`82 00 5820 ${NONCE_HEX}`)).toBe("unknown-type");
    expect(clientErrorReason("83 04 07 40")).toBe("unknown-type");
    expect(clientErrorReason("81 13")).toBe("unknown-type");
    expect(clientErrorReason("82 0f 00")).toBe("unknown-type");
  });

  it("refuses the wrong arity", () => {
    expect(clientErrorReason("80")).toBe("wrong-arity");
    expect(clientErrorReason("81 10")).toBe("wrong-arity");
    expect(clientErrorReason("83 10 40 40")).toBe("wrong-arity");
    expect(clientErrorReason("82 11 00")).toBe("wrong-arity");
    expect(clientErrorReason("81 12")).toBe("wrong-arity");
  });

  it("refuses a close reason above 5 and a wrong field type", () => {
    expect(clientErrorReason("82 12 06")).toBe("invalid-close-reason");
    expect(clientErrorReason("82 12 61 30")).toBe("wrong-type");
    expect(clientErrorReason("82 10 00")).toBe("wrong-type");
  });

  it("refuses trailing bytes and non-canonical forms", () => {
    expect(clientErrorReason("81 11 00")).toBe("malformed-cbor");
    expect(clientErrorReason("82 18 10 40")).toBe("malformed-cbor");
  });

  it("encodes `data` up to MAX_FRAME", () => {
    // 82 10 5a <4-byte length> payload: 1 + 1 + 5 + n
    const payload = new Uint8Array(MAX_FRAME - 7);
    const frame = encodeClientEnvelope({ type: "data", payload });
    expect(frame.length).toBe(MAX_FRAME);
    expect(decodeClientEnvelope(frame)).toEqual({ type: "data", payload });
  });
});

describe("handshake-start", () => {
  const MSG1 = fromHex("0102");

  it("encodes and decodes `pair` to the hand-written frame", () => {
    const start = { kind: HandshakeStartKind.pair, message1: MSG1 };
    // 82 array(2), 00 pair, 42 bstr(2), 0102
    expect(hex(encodeHandshakeStart(start))).toBe("820042" + "0102");
    expect(decodeHandshakeStart(fromHex("82 00 42 0102"))).toEqual(start);
  });

  it("encodes and decodes `connect` with a 32-byte message", () => {
    const start = { kind: HandshakeStartKind.connect, message1: NONCE };
    const frame = `82 01 5820 ${NONCE_HEX}`;
    expect(hex(encodeHandshakeStart(start))).toBe(frame.replace(/\s+/g, ""));
    expect(decodeHandshakeStart(fromHex(frame))).toEqual(start);
  });

  it("returns message1 as a view into the frame", () => {
    const frame = fromHex("82 00 42 0102");
    const decoded = decodeHandshakeStart(frame);
    expect(decoded.message1.buffer).toBe(frame.buffer);
    expect(decoded.message1.byteOffset).toBe(frame.byteOffset + 3);
  });

  it("refuses an unknown kind, the wrong arity and the wrong types", () => {
    expect(errorReason(() => decodeHandshakeStart(fromHex("82 02 40")))).toBe(
      "invalid-handshake-kind",
    );
    expect(errorReason(() => decodeHandshakeStart(fromHex("81 00")))).toBe("wrong-arity");
    expect(errorReason(() => decodeHandshakeStart(fromHex("83 00 40 40")))).toBe("wrong-arity");
    expect(errorReason(() => decodeHandshakeStart(fromHex("82 61 30 40")))).toBe("wrong-type");
    expect(errorReason(() => decodeHandshakeStart(fromHex("82 00 00")))).toBe("wrong-type");
    expect(errorReason(() => decodeHandshakeStart(fromHex("82 00 40 00")))).toBe(
      "malformed-cbor",
    );
  });

  it("refuses to encode an unknown kind", () => {
    expect(() =>
      encodeHandshakeStart({ kind: 2 as HandshakeStartKind, message1: MSG1 }),
    ).toThrow(RangeError);
  });
});

describe("registerMessage", () => {
  it("is the context followed by the nonce, 58 bytes", () => {
    const message = registerMessage(NONCE);
    expect(message.length).toBe(26 + NONCE_LEN);
    // "porecatu-relay/v0/register" in ASCII, written out by hand
    expect(hex(message.subarray(0, 26))).toBe("706f726563617475" + "2d72656c61792f76" + "302f726567697374" + "6572");
    expect(hex(message.subarray(26))).toBe(NONCE_HEX);
    expect(REGISTER_CONTEXT).toBe("porecatu-relay/v0/register");
  });

  it("requires a 32-byte nonce", () => {
    expect(() => registerMessage(new Uint8Array(31))).toThrow(RangeError);
    expect(() => registerMessage(new Uint8Array(33))).toThrow(RangeError);
  });
});

describe("channelDataLength", () => {
  it("counts the array head, type, channel, byte-string head and payload", () => {
    // 83 04 07 43 010203 is 7 bytes
    expect(channelDataLength(7, 3)).toBe(7);
    // channel 24 takes 2 bytes; payload 24 takes a 2-byte head: 1+1+2+2+24
    expect(channelDataLength(24, 24)).toBe(30);
    // channel 300 takes 3 bytes; payload 300 takes a 3-byte head: 1+1+3+3+300
    expect(channelDataLength(300, 300)).toBe(308);
    // channel 70000 takes 5 bytes; payload 65535: 1+1+5+3+65535
    expect(channelDataLength(70_000, 65_535)).toBe(65_545);
    // payload 65536 takes a 5-byte head
    expect(channelDataLength(0, 65_536)).toBe(1 + 1 + 1 + 5 + 65_536);
  });

  it("matches the encoder for a spread of sizes", () => {
    for (const channel of [0, 23, 24, 255, 256, 65_535, 65_536, 2 ** 32]) {
      for (const length of [0, 23, 24, 255, 256, 1000]) {
        const frame = encodeHostEnvelope({
          type: "channel-data",
          channel,
          payload: new Uint8Array(length),
        });
        expect(channelDataLength(channel, length)).toBe(frame.length);
      }
    }
  });

  it("refuses negative or fractional inputs", () => {
    expect(() => channelDataLength(-1, 0)).toThrow(RangeError);
    expect(() => channelDataLength(0, 1.5)).toThrow(RangeError);
  });
});

describe("clientCloseCode", () => {
  it("maps limit-exceeded to 4003, protocol-error to 4004 and the rest to 1000", () => {
    expect(clientCloseCode(CloseReason.limitExceeded)).toBe(4003);
    expect(clientCloseCode(CloseReason.protocolError)).toBe(4004);
    expect(clientCloseCode(CloseReason.normal)).toBe(1000);
    expect(clientCloseCode(CloseReason.hostReplaced)).toBe(1000);
    expect(clientCloseCode(CloseReason.hostGone)).toBe(1000);
    expect(clientCloseCode(CloseReason.hostRejected)).toBe(1000);
  });
});
