// SPDX-License-Identifier: GPL-3.0-or-later

import { CborError } from "./errors.js";

// Own UTF-8 codec: TextEncoder/TextDecoder are not guaranteed on every
// runtime this package targets, and the decoder must be strict (no
// replacement characters, no overlongs, no surrogates).

/** Encodes a string as UTF-8. Lone surrogates are refused. */
export function utf8Encode(text: string): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < text.length; i++) {
    let cp = text.charCodeAt(i);
    if (cp >= 0xd800 && cp <= 0xdbff) {
      const next = i + 1 < text.length ? text.charCodeAt(i + 1) : 0;
      if (next < 0xdc00 || next > 0xdfff) {
        throw new CborError("invalid-utf8");
      }
      cp = 0x10000 + ((cp - 0xd800) << 10) + (next - 0xdc00);
      i++;
    } else if (cp >= 0xdc00 && cp <= 0xdfff) {
      throw new CborError("invalid-utf8");
    }
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
  return Uint8Array.from(out);
}

/** Strictly decodes UTF-8; anything malformed throws `invalid-utf8`. */
export function utf8Decode(bytes: Uint8Array): string {
  const units: number[] = [];
  let result = "";
  const flush = (): void => {
    result += String.fromCharCode(...units);
    units.length = 0;
  };
  let i = 0;
  while (i < bytes.length) {
    const b0 = bytes[i] ?? 0;
    let cp: number;
    let extra: number;
    let min: number;
    if (b0 < 0x80) {
      cp = b0;
      extra = 0;
      min = 0;
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
      throw new CborError("invalid-utf8");
    }
    if (extra > 0 && i + extra >= bytes.length) {
      throw new CborError("invalid-utf8");
    }
    for (let k = 1; k <= extra; k++) {
      const bk = bytes[i + k] ?? 0;
      if ((bk & 0xc0) !== 0x80) {
        throw new CborError("invalid-utf8");
      }
      cp = (cp << 6) | (bk & 0x3f);
    }
    if (cp < min || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) {
      throw new CborError("invalid-utf8");
    }
    if (cp >= 0x10000) {
      const v = cp - 0x10000;
      units.push(0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
    } else {
      units.push(cp);
    }
    i += extra + 1;
    if (units.length >= 4096) {
      flush();
    }
  }
  flush();
  return result;
}
