// SPDX-License-Identifier: GPL-3.0-or-later

/** The `pair-request` codec (spec 02 §3): the body of handshake message 3. */

import { describe, expect, it } from "vitest";
import { CborMap, encodeCbor, type CborValue } from "../src/cbor/index.js";
import { DecodeError } from "../src/message/index.js";
import { decodePairRequest, encodePairRequest } from "../src/message/pairing.js";
import { Platform, type PairRequest } from "../src/message/index.js";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function map(entries: Record<string, CborValue>): Uint8Array {
  return encodeCbor(new CborMap(Object.entries(entries)));
}

function reasonOf(bytes: Uint8Array): string {
  try {
    decodePairRequest(bytes);
  } catch (error) {
    if (error instanceof DecodeError) {
      return error.reason;
    }
    throw error;
  }
  throw new Error("expected a decode failure");
}

const request: PairRequest = {
  token: new Uint8Array(16).fill(7),
  device: "Pixel",
  platform: Platform.android,
};

describe("pair-request", () => {
  // Same bytes as the Rust test `pair_request_round_trip`: a3 map(3); keys by
  // encoded bytes: "token" (65..), "device" (66..), "platform" (68..).
  it("encodes to the bytes of the Rust implementation and decodes back", () => {
    const bytes = encodePairRequest(request);
    expect(hex(bytes)).toBe(
      [
        "a3",
        "65746f6b656e", // "token"
        "5007070707070707070707070707070707", // bstr(16) of 0x07
        "66646576696365", // "device"
        "65506978656c", // "Pixel"
        "68706c6174666f726d", // "platform"
        "00", // android
      ].join(""),
    );
    expect(decodePairRequest(bytes)).toEqual(request);
  });

  it("round-trips every platform", () => {
    for (const platform of [Platform.android, Platform.ios, Platform.other]) {
      const value = { ...request, platform };
      expect(decodePairRequest(encodePairRequest(value))).toEqual(value);
    }
  });

  it("checks the device name length in bytes, not characters", () => {
    expect(() => encodePairRequest({ ...request, device: "" })).toThrow(RangeError);
    expect(() => encodePairRequest({ ...request, device: "x".repeat(65) })).toThrow(RangeError);
    const edge = { ...request, device: "x".repeat(64) };
    expect(decodePairRequest(encodePairRequest(edge))).toEqual(edge);
    // 32 two-byte characters are 64 bytes; 33 are 66.
    const accents = { ...request, device: "é".repeat(32) };
    expect(decodePairRequest(encodePairRequest(accents))).toEqual(accents);
    expect(() => encodePairRequest({ ...request, device: "é".repeat(33) })).toThrow(RangeError);
  });

  it("refuses a token that is not 16 bytes when encoding", () => {
    expect(() => encodePairRequest({ ...request, token: new Uint8Array(15) })).toThrow(RangeError);
  });

  it("refuses bad values when decoding", () => {
    const token = new Uint8Array(16).fill(1);
    // Token with 15 bytes.
    expect(reasonOf(map({ token: new Uint8Array(15), device: "a", platform: 0 }))).toBe(
      "out-of-domain",
    );
    // Platform 3.
    expect(reasonOf(map({ token, device: "a", platform: 3 }))).toBe("out-of-domain");
    // Device out of range.
    expect(reasonOf(map({ token, device: "", platform: 0 }))).toBe("out-of-domain");
    expect(reasonOf(map({ token, device: "x".repeat(65), platform: 0 }))).toBe("out-of-domain");
    // Missing keys.
    expect(reasonOf(map({ token, platform: 0 }))).toBe("missing-key");
    expect(reasonOf(map({ device: "a", platform: 0 }))).toBe("missing-key");
    expect(reasonOf(map({ token, device: "a" }))).toBe("missing-key");
    // Wrong types.
    expect(reasonOf(map({ token, device: 1, platform: 0 }))).toBe("wrong-type");
    expect(reasonOf(map({ token: "text", device: "a", platform: 0 }))).toBe("wrong-type");
    expect(reasonOf(map({ token, device: "a", platform: "0" }))).toBe("wrong-type");
  });

  it("refuses what is not a map, and bytes that are not canonical CBOR", () => {
    expect(reasonOf(encodeCbor([1, 2]))).toBe("wrong-type");
    expect(reasonOf(new Uint8Array([0xf6]))).toBe("wrong-type");
    expect(reasonOf(new Uint8Array(0))).toBe("truncated");
    // A valid map followed by one more byte.
    const valid = encodePairRequest(request);
    const trailing = new Uint8Array(valid.length + 1);
    trailing.set(valid);
    expect(reasonOf(trailing)).toBe("trailing-bytes");
  });

  it("ignores unknown keys", () => {
    const token = new Uint8Array(16).fill(1);
    const decoded = decodePairRequest(map({ token, device: "a", platform: 1, zzz: true }));
    expect(decoded.platform).toBe(Platform.ios);
  });

  it("does not alias the input: wiping the payload leaves the token intact", () => {
    const bytes = encodePairRequest(request);
    const decoded = decodePairRequest(bytes);
    bytes.fill(0);
    expect(decoded.token).toEqual(new Uint8Array(16).fill(7));
  });
});
