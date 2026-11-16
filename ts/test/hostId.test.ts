// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { hostId, parseHostId } from "../src/hostId.js";

// Fixed cases shared with the Rust implementation. Key A = bytes 00 01 .. 1f,
// key B = 32 bytes of ff. Expected values computed with Python:
//   base64.b32encode(hashlib.blake2s(key).digest()[:20]).decode().lower().rstrip("=")
const KEY_A = Uint8Array.from({ length: 32 }, (_, i) => i);
const KEY_B = new Uint8Array(32).fill(0xff);
const HOST_ID_A = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";
const HOST_ID_B = "mhj67icrscnsvbofgekm46omai6nhln7";

describe("hostId", () => {
  it("matches the fixed cases", () => {
    expect(hostId(KEY_A)).toBe(HOST_ID_A);
    expect(hostId(KEY_B)).toBe(HOST_ID_B);
  });

  it("is 32 characters of a-z2-7", () => {
    expect(hostId(KEY_A)).toMatch(/^[a-z2-7]{32}$/);
  });

  it("requires a 32-byte key", () => {
    expect(() => hostId(new Uint8Array(31))).toThrow(RangeError);
    expect(() => hostId(new Uint8Array(33))).toThrow(RangeError);
  });
});

describe("parseHostId", () => {
  it("accepts a valid id unchanged", () => {
    expect(parseHostId(HOST_ID_A)).toBe(HOST_ID_A);
    expect(parseHostId(HOST_ID_B)).toBe(HOST_ID_B);
  });

  it("refuses wrong length, uppercase, padding and foreign characters", () => {
    expect(parseHostId("")).toBeNull();
    expect(parseHostId(HOST_ID_A.slice(1))).toBeNull();
    expect(parseHostId(`${HOST_ID_A}a`)).toBeNull();
    expect(parseHostId(HOST_ID_A.toUpperCase())).toBeNull();
    expect(parseHostId(`${HOST_ID_A.slice(0, 31)}1`)).toBeNull();
    expect(parseHostId(`${HOST_ID_A.slice(0, 31)}=`)).toBeNull();
    expect(parseHostId(`${HOST_ID_A.slice(0, 31)}/`)).toBeNull();
  });
});
