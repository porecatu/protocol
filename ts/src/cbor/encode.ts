// SPDX-License-Identifier: GPL-3.0-or-later

import { CborError } from "./errors.js";
import { utf8Encode } from "./utf8.js";
import { CborMap, type CborValue } from "./value.js";

/** Nesting limit for arrays and maps (spec 03 §1.1). */
export const MAX_DEPTH = 32;

const TWO_32 = 4294967296;

class Writer {
  private buf = new Uint8Array(256);
  private len = 0;

  private reserve(extra: number): void {
    const need = this.len + extra;
    if (need <= this.buf.length) {
      return;
    }
    let cap = this.buf.length * 2;
    while (cap < need) {
      cap *= 2;
    }
    const next = new Uint8Array(cap);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }

  byte(b: number): void {
    this.reserve(1);
    this.buf[this.len++] = b;
  }

  bytes(data: Uint8Array): void {
    this.reserve(data.length);
    this.buf.set(data, this.len);
    this.len += data.length;
  }

  /** Major type and argument in the shortest form (RFC 8949 §4.2.1). */
  head(major: number, n: number): void {
    const m = major << 5;
    if (n < 24) {
      this.byte(m | n);
    } else if (n < 0x100) {
      this.byte(m | 24);
      this.byte(n);
    } else if (n < 0x10000) {
      this.byte(m | 25);
      this.byte(n >>> 8);
      this.byte(n & 0xff);
    } else if (n < TWO_32) {
      this.byte(m | 26);
      this.byte(n >>> 24);
      this.byte((n >>> 16) & 0xff);
      this.byte((n >>> 8) & 0xff);
      this.byte(n & 0xff);
    } else {
      const hi = Math.floor(n / TWO_32);
      const lo = n - hi * TWO_32;
      this.byte(m | 27);
      this.byte(hi >>> 24);
      this.byte((hi >>> 16) & 0xff);
      this.byte((hi >>> 8) & 0xff);
      this.byte(hi & 0xff);
      this.byte(lo >>> 24);
      this.byte((lo >>> 16) & 0xff);
      this.byte((lo >>> 8) & 0xff);
      this.byte(lo & 0xff);
    }
  }

  finish(): Uint8Array {
    return this.buf.slice(0, this.len);
  }
}

/** Lexicographic comparison of two byte strings. */
function compareBytes(a: Uint8Array, b: Uint8Array): number {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    const d = (a[i] ?? 0) - (b[i] ?? 0);
    if (d !== 0) {
      return d;
    }
  }
  return a.length - b.length;
}

function encodeInto(w: Writer, value: CborValue, depth: number): void {
  if (value === null) {
    w.byte(0xf6);
  } else if (value === true) {
    w.byte(0xf5);
  } else if (value === false) {
    w.byte(0xf4);
  } else if (typeof value === "number") {
    if (!Number.isInteger(value)) {
      throw new CborError("float");
    }
    if (!Number.isSafeInteger(value)) {
      throw new CborError("integer-out-of-range");
    }
    if (value >= 0) {
      w.head(0, value);
    } else {
      w.head(1, -1 - value);
    }
  } else if (typeof value === "string") {
    const data = utf8Encode(value);
    w.head(3, data.length);
    w.bytes(data);
  } else if (value instanceof Uint8Array) {
    w.head(2, value.length);
    w.bytes(value);
  } else if (Array.isArray(value)) {
    if (depth + 1 > MAX_DEPTH) {
      throw new CborError("too-deep");
    }
    w.head(4, value.length);
    for (const item of value) {
      encodeInto(w, item, depth + 1);
    }
  } else if (value instanceof CborMap) {
    if (depth + 1 > MAX_DEPTH) {
      throw new CborError("too-deep");
    }
    const keyed = value.entries.map(([key, item]) => {
      const kw = new Writer();
      encodeInto(kw, key, depth + 1);
      return { key: kw.finish(), item };
    });
    keyed.sort((a, b) => compareBytes(a.key, b.key));
    for (let i = 1; i < keyed.length; i++) {
      const prev = keyed[i - 1];
      const cur = keyed[i];
      if (prev !== undefined && cur !== undefined && compareBytes(prev.key, cur.key) === 0) {
        throw new CborError("duplicate-key");
      }
    }
    w.head(5, keyed.length);
    for (const { key, item } of keyed) {
      w.bytes(key);
      encodeInto(w, item, depth + 1);
    }
  } else {
    throw new CborError("wrong-type");
  }
}

/**
 * Deterministic CBOR encoding (RFC 8949 §4.2.1): shortest integers and
 * lengths, definite lengths, map keys sorted by their encoded bytes.
 */
export function encodeCbor(value: CborValue): Uint8Array {
  const w = new Writer();
  encodeInto(w, value, 0);
  return w.finish();
}
