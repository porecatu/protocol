// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Strict text encodings used by identities and the pairing URI: base32
 * (RFC 4648, no padding), base64url (no padding), hex, UTF-8 and
 * percent-encoding (RFC 3986).
 *
 * Hand-written on purpose: the package runs on Hermes, where `atob`/`btoa`,
 * `Buffer` and a fatal `TextDecoder` cannot be relied on. Decoders are
 * canonical: they refuse padding, characters outside the alphabet, impossible
 * lengths and non-zero trailing bits, so every value has exactly one text form.
 */

const BASE32_LOWER = "abcdefghijklmnopqrstuvwxyz234567";
const BASE32_UPPER = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BASE64URL = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const HEX_LOWER = "0123456789abcdef";

/** Letter case of the base32 alphabet. */
export type Base32Case = "lower" | "upper";

function reverseTable(alphabet: string): Map<string, number> {
  const table = new Map<string, number>();
  for (let i = 0; i < alphabet.length; i++) {
    table.set(alphabet.charAt(i), i);
  }
  return table;
}

const BASE32_LOWER_REV = reverseTable(BASE32_LOWER);
const BASE32_UPPER_REV = reverseTable(BASE32_UPPER);
const BASE64URL_REV = reverseTable(BASE64URL);

/**
 * Packs `bytes` into groups of `bits` bits (5 for base32, 6 for base64url),
 * zero-padding the last group, and maps each group through `alphabet`.
 */
function encodeBits(bytes: Uint8Array, bits: number, alphabet: string): string {
  let out = "";
  let acc = 0;
  let accBits = 0;
  const mask = (1 << bits) - 1;
  for (const byte of bytes) {
    acc = ((acc << 8) | byte) & 0xffff;
    accBits += 8;
    while (accBits >= bits) {
      accBits -= bits;
      out += alphabet.charAt((acc >> accBits) & mask);
    }
  }
  if (accBits > 0) {
    out += alphabet.charAt((acc << (bits - accBits)) & mask);
  }
  return out;
}

/**
 * Inverse of {@link encodeBits}. Returns `null` for a character outside the
 * alphabet, a length no byte string can produce, or non-zero trailing bits.
 */
function decodeBits(text: string, bits: number, table: Map<string, number>): Uint8Array | null {
  const total = text.length * bits;
  const byteLen = Math.floor(total / 8);
  // Leftover bits must be fewer than one group, otherwise the length is
  // impossible (e.g. one base64 character on its own).
  if (total - byteLen * 8 >= bits) {
    return null;
  }
  const out = new Uint8Array(byteLen);
  let acc = 0;
  let accBits = 0;
  let pos = 0;
  for (let i = 0; i < text.length; i++) {
    const value = table.get(text.charAt(i));
    if (value === undefined) {
      return null;
    }
    acc = ((acc << bits) | value) & 0xffff;
    accBits += bits;
    if (accBits >= 8) {
      accBits -= 8;
      out[pos++] = (acc >> accBits) & 0xff;
    }
  }
  // Canonical form: the padding bits of the last group are zero.
  if ((acc & ((1 << accBits) - 1)) !== 0) {
    return null;
  }
  return out;
}

/** Base32 (RFC 4648 alphabet), no padding. Lowercase unless `letterCase` is `"upper"`. */
export function base32Encode(bytes: Uint8Array, letterCase: Base32Case = "lower"): string {
  return encodeBits(bytes, 5, letterCase === "upper" ? BASE32_UPPER : BASE32_LOWER);
}

/** Strict base32 decoder; `null` on padding, wrong case, foreign characters or non-canonical tail. */
export function base32Decode(text: string, letterCase: Base32Case = "lower"): Uint8Array | null {
  return decodeBits(text, 5, letterCase === "upper" ? BASE32_UPPER_REV : BASE32_LOWER_REV);
}

/** Base64url (RFC 4648 §5), no padding. */
export function base64UrlEncode(bytes: Uint8Array): string {
  return encodeBits(bytes, 6, BASE64URL);
}

/** Strict base64url decoder; `null` on padding, foreign characters or non-canonical tail. */
export function base64UrlDecode(text: string): Uint8Array | null {
  return decodeBits(text, 6, BASE64URL_REV);
}

/** Lowercase hexadecimal. */
export function hexEncode(bytes: Uint8Array): string {
  let out = "";
  for (const byte of bytes) {
    out += HEX_LOWER.charAt(byte >> 4) + HEX_LOWER.charAt(byte & 0x0f);
  }
  return out;
}

function hexValue(code: number): number {
  if (code >= 0x30 && code <= 0x39) return code - 0x30;
  if (code >= 0x61 && code <= 0x66) return code - 0x61 + 10;
  if (code >= 0x41 && code <= 0x46) return code - 0x41 + 10;
  return -1;
}

/** Strict hex decoder (either case); `null` on odd length or a non-hex character. */
export function hexDecode(text: string): Uint8Array | null {
  if (text.length % 2 !== 0) {
    return null;
  }
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) {
    const hi = hexValue(text.charCodeAt(2 * i));
    const lo = hexValue(text.charCodeAt(2 * i + 1));
    if (hi < 0 || lo < 0) {
      return null;
    }
    out[i] = (hi << 4) | lo;
  }
  return out;
}

/**
 * UTF-8 encoding of a string. A lone surrogate becomes U+FFFD, the same
 * replacement `TextEncoder` makes, so the output is always valid UTF-8.
 */
export function utf8Encode(text: string): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < text.length; i++) {
    let cp = text.charCodeAt(i);
    if (cp >= 0xd800 && cp <= 0xdbff) {
      const next = i + 1 < text.length ? text.charCodeAt(i + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        cp = 0x10000 + ((cp - 0xd800) << 10) + (next - 0xdc00);
        i++;
      } else {
        cp = 0xfffd;
      }
    } else if (cp >= 0xdc00 && cp <= 0xdfff) {
      cp = 0xfffd;
    }
    pushUtf8(out, cp);
  }
  return Uint8Array.from(out);
}

function pushUtf8(out: number[], cp: number): void {
  if (cp < 0x80) {
    out.push(cp);
  } else if (cp < 0x800) {
    out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
  } else if (cp < 0x10000) {
    out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
  } else {
    out.push(
      0xf0 | (cp >> 18),
      0x80 | ((cp >> 12) & 0x3f),
      0x80 | ((cp >> 6) & 0x3f),
      0x80 | (cp & 0x3f),
    );
  }
}

/** Number of bytes `codePoint` takes in UTF-8. */
export function utf8CodePointLength(codePoint: number): number {
  if (codePoint < 0x80) return 1;
  if (codePoint < 0x800) return 2;
  if (codePoint < 0x10000) return 3;
  return 4;
}

/**
 * Strict UTF-8 decoder: `null` for truncated or stray bytes, overlong forms,
 * surrogates and values above U+10FFFF.
 */
export function utf8Decode(bytes: Uint8Array): string | null {
  let out = "";
  let i = 0;
  while (i < bytes.length) {
    const b0 = bytes[i] ?? 0;
    let cp: number;
    let extra: number;
    let min: number;
    if (b0 < 0x80) {
      out += String.fromCharCode(b0);
      i++;
      continue;
    } else if (b0 >= 0xc2 && b0 <= 0xdf) {
      cp = b0 & 0x1f;
      extra = 1;
      min = 0x80;
    } else if (b0 >= 0xe0 && b0 <= 0xef) {
      cp = b0 & 0x0f;
      extra = 2;
      min = 0x800;
    } else if (b0 >= 0xf0 && b0 <= 0xf4) {
      cp = b0 & 0x07;
      extra = 3;
      min = 0x10000;
    } else {
      return null;
    }
    if (i + extra >= bytes.length) {
      return null;
    }
    for (let k = 1; k <= extra; k++) {
      const b = bytes[i + k] ?? 0;
      if ((b & 0xc0) !== 0x80) {
        return null;
      }
      cp = (cp << 6) | (b & 0x3f);
    }
    if (cp < min || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) {
      return null;
    }
    out += String.fromCodePoint(cp);
    i += extra + 1;
  }
  return out;
}

function isUnreserved(byte: number): boolean {
  return (
    (byte >= 0x41 && byte <= 0x5a) ||
    (byte >= 0x61 && byte <= 0x7a) ||
    (byte >= 0x30 && byte <= 0x39) ||
    byte === 0x2d ||
    byte === 0x2e ||
    byte === 0x5f ||
    byte === 0x7e
  );
}

const HEX_UPPER = "0123456789ABCDEF";

/**
 * Percent-encodes every byte outside the RFC 3986 unreserved set
 * (`A-Z a-z 0-9 - . _ ~`) as `%HH` with uppercase hex.
 */
export function percentEncode(bytes: Uint8Array): string {
  let out = "";
  for (const byte of bytes) {
    out += isUnreserved(byte)
      ? String.fromCharCode(byte)
      : `%${HEX_UPPER.charAt(byte >> 4)}${HEX_UPPER.charAt(byte & 0x0f)}`;
  }
  return out;
}

/**
 * Percent-decodes `text` into bytes. Characters other than `%` pass through
 * (as UTF-8); `+` is a plain plus. `null` on a `%` not followed by two hex
 * digits.
 */
export function percentDecode(text: string): Uint8Array | null {
  const out: number[] = [];
  const chars = Array.from(text);
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i] ?? "";
    if (ch === "%") {
      const hi = hexValue((chars[i + 1] ?? "").charCodeAt(0));
      const lo = hexValue((chars[i + 2] ?? "").charCodeAt(0));
      if (hi < 0 || lo < 0) {
        return null;
      }
      out.push((hi << 4) | lo);
      i += 2;
    } else {
      const cp = ch.codePointAt(0) ?? 0xfffd;
      // A lone surrogate cannot be UTF-8; same replacement as utf8Encode.
      pushUtf8(out, cp >= 0xd800 && cp <= 0xdfff ? 0xfffd : cp);
    }
  }
  return Uint8Array.from(out);
}
