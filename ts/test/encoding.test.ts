// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  base32Decode,
  base32Encode,
  base64UrlDecode,
  base64UrlEncode,
  hexDecode,
  hexEncode,
  percentDecode,
  percentEncode,
  utf8Decode,
  utf8Encode,
} from "../src/encoding.js";

const ascii = (s: string): Uint8Array => Uint8Array.from(Array.from(s, (c) => c.charCodeAt(0)));

describe("base32", () => {
  // RFC 4648 §10 vectors, padding stripped (checked with Python base64.b32encode).
  const vectors: [string, string][] = [
    ["", ""],
    ["f", "MY"],
    ["fo", "MZXQ"],
    ["foo", "MZXW6"],
    ["foob", "MZXW6YQ"],
    ["fooba", "MZXW6YTB"],
    ["foobar", "MZXW6YTBOI"],
  ];

  it("encodes and decodes the RFC 4648 vectors in both cases", () => {
    for (const [plain, upper] of vectors) {
      expect(base32Encode(ascii(plain), "upper")).toBe(upper);
      expect(base32Encode(ascii(plain))).toBe(upper.toLowerCase());
      expect(base32Decode(upper, "upper")).toEqual(ascii(plain));
      expect(base32Decode(upper.toLowerCase())).toEqual(ascii(plain));
    }
  });

  it("refuses padding, wrong case, foreign characters and impossible lengths", () => {
    expect(base32Decode("mzxw6===")).toBeNull();
    expect(base32Decode("MZXW6", "lower")).toBeNull();
    expect(base32Decode("mzxw6", "upper")).toBeNull();
    expect(base32Decode("mzxw1")).toBeNull(); // '1' is not in the alphabet
    expect(base32Decode("m")).toBeNull(); // 5 bits cannot make a byte
    expect(base32Decode("mzx")).toBeNull(); // 15 bits: leftover is a whole group
    expect(base32Decode("mzxw6y")).toBeNull(); // 30 bits: leftover is a whole group
  });

  it("refuses non-zero trailing bits (non-canonical)", () => {
    expect(base32Decode("my")).toEqual(ascii("f"));
    expect(base32Decode("mz")).toBeNull(); // 'z' sets bits below the byte
  });
});

describe("base64url", () => {
  it("encodes and decodes without padding", () => {
    expect(base64UrlEncode(ascii(""))).toBe("");
    expect(base64UrlEncode(ascii("f"))).toBe("Zg");
    expect(base64UrlEncode(ascii("fo"))).toBe("Zm8");
    expect(base64UrlEncode(ascii("foo"))).toBe("Zm9v");
    expect(base64UrlEncode(Uint8Array.from([0xfb, 0xff, 0xbf]))).toBe("-_-_");
    for (const s of ["", "Zg", "Zm8", "Zm9v", "-_-_"]) {
      expect(base64UrlEncode(base64UrlDecode(s) ?? new Uint8Array([1]))).toBe(s);
    }
  });

  it("refuses padding, the standard alphabet, impossible lengths and non-zero tails", () => {
    expect(base64UrlDecode("Zg==")).toBeNull();
    expect(base64UrlDecode("+/+/")).toBeNull();
    expect(base64UrlDecode("Z")).toBeNull();
    expect(base64UrlDecode("Zh")).toBeNull(); // trailing bits set
    expect(base64UrlDecode("Zm9 v")).toBeNull();
  });

  it("matches the 43/22 character sizes of the pairing QR", () => {
    expect(base64UrlEncode(new Uint8Array(32)).length).toBe(43);
    expect(base64UrlEncode(new Uint8Array(16)).length).toBe(22);
  });
});

describe("hex", () => {
  it("round-trips and refuses bad input", () => {
    expect(hexEncode(Uint8Array.from([0, 1, 0xab, 0xff]))).toBe("0001abff");
    expect(hexDecode("0001abff")).toEqual(Uint8Array.from([0, 1, 0xab, 0xff]));
    expect(hexDecode("0001ABFF")).toEqual(Uint8Array.from([0, 1, 0xab, 0xff]));
    expect(hexDecode("abc")).toBeNull();
    expect(hexDecode("zz")).toBeNull();
  });
});

describe("utf8", () => {
  it("encodes 1 to 4 byte sequences", () => {
    expect(utf8Encode("A")).toEqual(Uint8Array.from([0x41]));
    expect(utf8Encode("é")).toEqual(Uint8Array.from([0xc3, 0xa9]));
    expect(utf8Encode("€")).toEqual(Uint8Array.from([0xe2, 0x82, 0xac]));
    expect(utf8Encode("😀")).toEqual(Uint8Array.from([0xf0, 0x9f, 0x98, 0x80]));
  });

  it("replaces a lone surrogate with U+FFFD", () => {
    expect(utf8Encode("\ud800")).toEqual(Uint8Array.from([0xef, 0xbf, 0xbd]));
    expect(utf8Encode("\udc00x")).toEqual(Uint8Array.from([0xef, 0xbf, 0xbd, 0x78]));
  });

  it("decodes what it encodes", () => {
    for (const s of ["", "abc", "Léo (casa)", "€", "😀 ok"]) {
      expect(utf8Decode(utf8Encode(s))).toBe(s);
    }
  });

  it("refuses malformed sequences", () => {
    expect(utf8Decode(Uint8Array.from([0x80]))).toBeNull(); // stray continuation
    expect(utf8Decode(Uint8Array.from([0xc0, 0x80]))).toBeNull(); // overlong
    expect(utf8Decode(Uint8Array.from([0xe0, 0x80, 0x80]))).toBeNull(); // overlong
    expect(utf8Decode(Uint8Array.from([0xed, 0xa0, 0x80]))).toBeNull(); // surrogate
    expect(utf8Decode(Uint8Array.from([0xf4, 0x90, 0x80, 0x80]))).toBeNull(); // > U+10FFFF
    expect(utf8Decode(Uint8Array.from([0xff]))).toBeNull();
    expect(utf8Decode(Uint8Array.from([0xe2, 0x82]))).toBeNull(); // truncated
    expect(utf8Decode(Uint8Array.from([0xc3]))).toBeNull(); // truncated
  });
});

describe("percent-encoding", () => {
  it("leaves only RFC 3986 unreserved characters in clear, with uppercase hex", () => {
    expect(percentEncode(utf8Encode("AZaz09-._~"))).toBe("AZaz09-._~");
    expect(percentEncode(utf8Encode("wss://a:1/b"))).toBe("wss%3A%2F%2Fa%3A1%2Fb");
    expect(percentEncode(utf8Encode("Léo (casa)"))).toBe("L%C3%A9o%20%28casa%29");
    expect(percentEncode(utf8Encode("a+b"))).toBe("a%2Bb");
  });

  it("decodes %HH in either case and keeps '+' as a plus", () => {
    expect(utf8Decode(percentDecode("L%c3%A9o+x") ?? new Uint8Array())).toBe("Léo+x");
    expect(percentDecode("%")).toBeNull();
    expect(percentDecode("%4")).toBeNull();
    expect(percentDecode("%zz")).toBeNull();
  });
});
