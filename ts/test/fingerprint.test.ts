// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fingerprint, formatFingerprint } from "../src/fingerprint.js";

// Fixed cases shared with the Rust implementation. Key A = bytes 00 01 .. 1f,
// key B = 32 bytes of ff. Expected values computed with Python:
//   base64.b32encode(hashlib.blake2s(key).digest()[:10]).decode().rstrip("=")
const KEY_A = Uint8Array.from({ length: 32 }, (_, i) => i);
const KEY_B = new Uint8Array(32).fill(0xff);

describe("fingerprint", () => {
  it("matches the fixed cases", () => {
    expect(fingerprint(KEY_A)).toBe("AWBFMB6X7XZNQLXU");
    expect(fingerprint(KEY_B)).toBe("MHJ67ICRSCNSVBOF");
  });

  it("is 16 uppercase base32 characters", () => {
    expect(fingerprint(KEY_A)).toMatch(/^[A-Z2-7]{16}$/);
  });

  it("requires a 32-byte key", () => {
    expect(() => fingerprint(new Uint8Array(31))).toThrow(RangeError);
  });
});

describe("formatFingerprint", () => {
  it("writes four groups of four", () => {
    expect(formatFingerprint("AWBFMB6X7XZNQLXU")).toBe("AWBF MB6X 7XZN QLXU");
    expect(formatFingerprint(fingerprint(KEY_B))).toBe("MHJ6 7ICR SCNS VBOF");
  });

  it("requires 16 characters", () => {
    expect(() => formatFingerprint("ABCD")).toThrow(RangeError);
  });
});
