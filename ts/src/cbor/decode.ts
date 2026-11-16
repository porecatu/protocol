// SPDX-License-Identifier: GPL-3.0-or-later

import { MAX_DEPTH } from "./encode.js";
import { CborError } from "./errors.js";
import { utf8Decode } from "./utf8.js";
import { CborMap, type CborValue } from "./value.js";

const TWO_32 = 4294967296;
/** Marks an argument above 2^53 - 1: out of range for ints, truncated for lengths. */
const HUGE = Number.POSITIVE_INFINITY;

interface Head {
  readonly major: number;
  readonly arg: number;
}

// Hand-written strict decoder. The ADR-0008 route (decode with cbor-x,
// re-encode, compare) lets BigInt, Node Buffers, reordered numeric keys and
// converted tags through before the comparison can see them; reading the
// bytes directly refuses each of them at the source and checks the length
// against the rest of the input before anything is allocated.
class Reader {
  private pos = 0;

  constructor(private readonly data: Uint8Array) {}

  get remaining(): number {
    return this.data.length - this.pos;
  }

  get offset(): number {
    return this.pos;
  }

  slice(from: number, to: number): Uint8Array {
    return this.data.subarray(from, to);
  }

  private u8(): number {
    if (this.pos >= this.data.length) {
      throw new CborError("truncated");
    }
    return this.data[this.pos++] ?? 0;
  }

  private u32(): number {
    const a = this.u8();
    const b = this.u8();
    const c = this.u8();
    const d = this.u8();
    return a * 0x1000000 + b * 0x10000 + c * 0x100 + d;
  }

  /** Reads an initial byte plus argument; majors 0-6 only (7 is handled by the caller). */
  head(): Head {
    const initial = this.u8();
    const major = initial >> 5;
    const ai = initial & 0x1f;
    if (ai < 24) {
      return { major, arg: ai };
    }
    if (ai === 24) {
      const arg = this.u8();
      if (arg < 24) {
        throw new CborError("non-canonical");
      }
      return { major, arg };
    }
    if (ai === 25) {
      const arg = this.u8() * 0x100 + this.u8();
      if (arg < 0x100) {
        throw new CborError("non-canonical");
      }
      return { major, arg };
    }
    if (ai === 26) {
      const arg = this.u32();
      if (arg < 0x10000) {
        throw new CborError("non-canonical");
      }
      return { major, arg };
    }
    if (ai === 27) {
      const hi = this.u32();
      const lo = this.u32();
      if (hi === 0) {
        throw new CborError("non-canonical");
      }
      if (hi > 0x1fffff) {
        return { major, arg: HUGE };
      }
      return { major, arg: hi * TWO_32 + lo };
    }
    if (ai === 31) {
      if (major >= 2 && major <= 5) {
        throw new CborError("indefinite-length");
      }
      throw new CborError("malformed");
    }
    throw new CborError("malformed");
  }

  peekMajor(): number {
    if (this.pos >= this.data.length) {
      throw new CborError("truncated");
    }
    return (this.data[this.pos] ?? 0) >> 5;
  }

  /** Raw bytes of a string body; the length is checked before anything is copied. */
  body(length: number): Uint8Array {
    if (length > this.remaining) {
      throw new CborError("truncated");
    }
    const out = this.data.subarray(this.pos, this.pos + length);
    this.pos += length;
    return out;
  }
}

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

function readText(r: Reader): string {
  const { arg } = r.head();
  return utf8Decode(r.body(arg));
}

function readItem(r: Reader, depth: number): CborValue {
  const major = r.peekMajor();
  if (major === 7) {
    const initial = r.body(1)[0] ?? 0;
    switch (initial) {
      case 0xf4:
        return false;
      case 0xf5:
        return true;
      case 0xf6:
        return null;
      case 0xf9:
      case 0xfa:
      case 0xfb:
        throw new CborError("float");
      case 0xff:
      case 0xfc:
      case 0xfd:
      case 0xfe:
        throw new CborError("malformed");
      default:
        // 0xe0-0xf3, undefined (0xf7), 0xf8 + one byte.
        throw new CborError("simple-value");
    }
  }
  if (major === 6) {
    throw new CborError("tag");
  }
  const { arg } = r.head();
  switch (major) {
    case 0:
      if (arg > Number.MAX_SAFE_INTEGER) {
        throw new CborError("integer-out-of-range");
      }
      return arg;
    case 1:
      // Value is -1 - arg; the lowest allowed is -(2^53 - 1).
      if (arg > Number.MAX_SAFE_INTEGER - 1) {
        throw new CborError("integer-out-of-range");
      }
      return -1 - arg;
    case 2:
      // Copy: the result must not alias the caller's input buffer.
      return r.body(arg).slice();
    case 3:
      return utf8Decode(r.body(arg));
    case 4: {
      if (depth + 1 > MAX_DEPTH) {
        throw new CborError("too-deep");
      }
      // Each element takes at least one byte, so a count above the rest is truncated.
      if (arg > r.remaining) {
        throw new CborError("truncated");
      }
      const items: CborValue[] = [];
      for (let i = 0; i < arg; i++) {
        items.push(readItem(r, depth + 1));
      }
      return items;
    }
    default: {
      // major 5
      if (depth + 1 > MAX_DEPTH) {
        throw new CborError("too-deep");
      }
      if (arg > r.remaining) {
        throw new CborError("truncated");
      }
      const entries: Array<readonly [string, CborValue]> = [];
      let prev: Uint8Array | undefined;
      for (let i = 0; i < arg; i++) {
        if (r.peekMajor() !== 3) {
          throw new CborError("non-text-key");
        }
        const start = r.offset;
        const key = readText(r);
        const raw = r.slice(start, r.offset);
        if (prev !== undefined) {
          const order = compareBytes(prev, raw);
          if (order === 0) {
            throw new CborError("duplicate-key");
          }
          if (order > 0) {
            throw new CborError("unordered-keys");
          }
        }
        prev = raw;
        entries.push([key, readItem(r, depth + 1)]);
      }
      return new CborMap(entries);
    }
  }
}

/**
 * Strict decoding of exactly one CBOR item (spec 03 §1.1): refuses anything
 * that is not in deterministic form, tags, floats, simple values other than
 * false/true/null, integers beyond ±(2^53 - 1), non-text or unordered or
 * duplicate map keys, invalid UTF-8, nesting beyond 32 levels, truncated
 * input and trailing bytes.
 */
export function decodeCbor(bytes: Uint8Array): CborValue {
  const r = new Reader(bytes);
  const value = readItem(r, 0);
  if (r.remaining > 0) {
    throw new CborError("trailing-bytes");
  }
  return value;
}
