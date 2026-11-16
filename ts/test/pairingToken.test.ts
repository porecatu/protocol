// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { PairingToken, TOKEN_LEN, TOKEN_TTL_MS } from "../src/session/pairingToken.js";

const BYTES = new Uint8Array(TOKEN_LEN).fill(9);

describe("PairingToken", () => {
  it("accepts the matching token once", () => {
    const token = new PairingToken(BYTES, 1_000);
    expect(token.isLive(1_000)).toBe(true);
    expect(token.matchesAndConsume(BYTES, 2_000)).toBe(true);
    expect(token.isConsumed).toBe(true);
    expect(token.matchesAndConsume(BYTES, 2_001)).toBe(false);
  });

  it("burns the right token when a wrong one is presented", () => {
    const token = new PairingToken(BYTES, 0);
    expect(token.matchesAndConsume(new Uint8Array(TOKEN_LEN).fill(8), 10)).toBe(false);
    expect(token.isConsumed).toBe(true);
    expect(token.matchesAndConsume(BYTES, 11)).toBe(false);
  });

  it("never matches a wrong length and still consumes", () => {
    const token = new PairingToken(BYTES, 0);
    expect(token.matchesAndConsume(BYTES.subarray(0, 15), 10)).toBe(false);
    expect(token.isConsumed).toBe(true);
  });

  it("expires after five minutes", () => {
    const token = new PairingToken(BYTES, 1_000);
    expect(TOKEN_TTL_MS).toBe(300_000);
    expect(token.expiresAtMs).toBe(1_000 + TOKEN_TTL_MS);
    expect(token.isLive(1_000 + TOKEN_TTL_MS - 1)).toBe(true);
    expect(token.isLive(1_000 + TOKEN_TTL_MS)).toBe(false);
    expect(token.matchesAndConsume(BYTES, 1_000 + TOKEN_TTL_MS)).toBe(false);
    expect(token.isConsumed).toBe(true);
  });

  it("still matches in the last millisecond", () => {
    const token = new PairingToken(BYTES, 0);
    expect(token.matchesAndConsume(BYTES, TOKEN_TTL_MS - 1)).toBe(true);
  });

  it("does not overflow the deadline", () => {
    const token = new PairingToken(BYTES, Number.MAX_SAFE_INTEGER);
    expect(token.expiresAtMs).toBe(Number.MAX_SAFE_INTEGER);
  });

  it("generates distinct tokens from the injected source", () => {
    let counter = 0;
    const random = (n: number): Uint8Array => new Uint8Array(n).fill(++counter);
    const a = PairingToken.generate(0, random);
    const b = PairingToken.generate(0, random);
    expect(a.bytes).not.toEqual(b.bytes);
    expect(a.bytes).toHaveLength(TOKEN_LEN);
  });

  it("generates from the platform source by default", () => {
    const a = PairingToken.generate(0);
    const b = PairingToken.generate(0);
    expect(a.bytes).not.toEqual(b.bytes);
    expect(a.bytes).not.toEqual(new Uint8Array(TOKEN_LEN));
  });

  it("refuses bytes that are not 16", () => {
    expect(() => new PairingToken(new Uint8Array(15), 0)).toThrow(RangeError);
    expect(() => PairingToken.generate(0, () => new Uint8Array(4))).toThrow();
  });

  it("copies what it is given and what it hands out", () => {
    const source = new Uint8Array(TOKEN_LEN).fill(5);
    const token = new PairingToken(source, 0);
    source.fill(0);
    const out = token.bytes;
    out.fill(0);
    expect(token.bytes).toEqual(new Uint8Array(TOKEN_LEN).fill(5));
  });

  it("hides the bytes from every textual view", () => {
    const token = new PairingToken(new Uint8Array(TOKEN_LEN).fill(7), 0);
    for (const shown of [String(token), JSON.stringify(token), JSON.stringify({ token })]) {
      expect(shown).toContain("redacted");
      expect(shown).not.toContain("7,7");
    }
    expect(Object.keys(token)).toEqual([]);
  });

  it("never matches after destroy", () => {
    const token = new PairingToken(BYTES, 0);
    token.destroy();
    expect(token.isLive(1)).toBe(false);
    expect(token.matchesAndConsume(new Uint8Array(TOKEN_LEN), 1)).toBe(false);
  });
});
