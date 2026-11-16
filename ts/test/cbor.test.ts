// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  asArray,
  asBool,
  asBytes,
  asInt,
  asMap,
  asText,
  asUint,
  CborError,
  CborMap,
  decodeCbor,
  encodeCbor,
  expectArray,
  isNull,
  mapReader,
  type CborErrorReason,
  type CborValue,
} from "../src/cbor/index.js";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function fromHex(text: string): Uint8Array {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(text.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function reasonOf(fn: () => unknown): CborErrorReason {
  try {
    fn();
  } catch (e) {
    if (e instanceof CborError) {
      return e.reason;
    }
    throw e;
  }
  throw new Error("expected a CborError");
}

function refuses(input: string, reason: CborErrorReason): void {
  expect(reasonOf(() => decodeCbor(fromHex(input)))).toBe(reason);
}

describe("encodeCbor", () => {
  const cases: Array<[string, CborValue, string]> = [
    ["0", 0, "00"],
    ["23", 23, "17"],
    ["24", 24, "1818"],
    ["255", 255, "18ff"],
    ["256", 256, "190100"],
    ["65535", 65535, "19ffff"],
    ["65536", 65536, "1a00010000"],
    ["4294967295", 4294967295, "1affffffff"],
    ["4294967296", 4294967296, "1b0000000100000000"],
    ["-1", -1, "20"],
    ["-24", -24, "37"],
    ["-25", -25, "3818"],
    ["2^53-1", Number.MAX_SAFE_INTEGER, "1b001fffffffffffff"],
    ["-(2^53-1)", -Number.MAX_SAFE_INTEGER, "3b001ffffffffffffe"],
    ["empty text", "", "60"],
    ["text a", "a", "6161"],
    ["text non-ascii", "é€\u{1f600}", "69c3a9e282acf09f9880"],
    ["bytes", new Uint8Array([1, 2, 3]), "43010203"],
    ["empty bytes", new Uint8Array(), "40"],
    ["false", false, "f4"],
    ["true", true, "f5"],
    ["null", null, "f6"],
    ["array", [1, [2, 3]], "8201820203"],
    ["empty array", [], "80"],
    ["empty map", new CborMap(), "a0"],
  ];

  for (const [name, value, expected] of cases) {
    it(`encodes ${name}`, () => {
      expect(hex(encodeCbor(value))).toBe(expected);
    });
  }

  it("orders map keys by encoded bytes (b, a, aa -> a, b, aa)", () => {
    const map = new CborMap([
      ["b", 2],
      ["a", 1],
      ["aa", 3],
    ]);
    // a1 6161 01 | 6162 02 | 626161 03 (shorter keys first)
    expect(hex(encodeCbor(map))).toBe("a3" + "616101" + "616202" + "62616103");
  });

  it("orders keys of a nested map too", () => {
    const map = new CborMap([["z", new CborMap([["y", 1], ["x", 2]])], ["a", []]]);
    expect(hex(encodeCbor(map))).toBe("a2" + "6161" + "80" + "617a" + "a2" + "617802" + "617901");
  });

  it("refuses non-integers and out of range numbers", () => {
    expect(reasonOf(() => encodeCbor(1.5))).toBe("float");
    expect(reasonOf(() => encodeCbor(Number.NaN))).toBe("float");
    expect(reasonOf(() => encodeCbor(Number.POSITIVE_INFINITY))).toBe("float");
    expect(reasonOf(() => encodeCbor(2 ** 53))).toBe("integer-out-of-range");
    expect(reasonOf(() => encodeCbor(-(2 ** 53)))).toBe("integer-out-of-range");
  });

  it("refuses duplicate keys and lone surrogates", () => {
    expect(
      reasonOf(() =>
        encodeCbor(
          new CborMap([
            ["a", 1],
            ["a", 2],
          ]),
        ),
      ),
    ).toBe("duplicate-key");
    expect(reasonOf(() => encodeCbor("\ud800"))).toBe("invalid-utf8");
  });

  it("refuses nesting beyond 32 levels and accepts exactly 32", () => {
    let ok: CborValue = 0;
    for (let i = 0; i < 32; i++) {
      ok = [ok];
    }
    expect(encodeCbor(ok).length).toBe(33);
    expect(reasonOf(() => encodeCbor([ok]))).toBe("too-deep");
  });

  it("encodes a long byte string with a one-pass length head", () => {
    const out = encodeCbor(new Uint8Array(70_000));
    expect(hex(out.subarray(0, 5))).toBe("5a00011170");
    expect(out.length).toBe(70_005);
  });
});

describe("decodeCbor", () => {
  it("refuses a non-minimal integer", () => {
    refuses("1817", "non-canonical");
    refuses("190017", "non-canonical");
    refuses("1a000000ff", "non-canonical");
    refuses("1b00000000000000ff", "non-canonical");
  });

  it("refuses a non-minimal length", () => {
    refuses("5801" + "00", "non-canonical");
    refuses("7800", "non-canonical");
    refuses("9800", "non-canonical");
    refuses("b800", "non-canonical");
  });

  it("refuses indefinite lengths", () => {
    refuses("9f01ff", "indefinite-length");
    refuses("bf6161" + "01ff", "indefinite-length");
    refuses("5f4101ff", "indefinite-length");
    refuses("7f6161ff", "indefinite-length");
  });

  it("refuses tags", () => {
    refuses("c0" + "74" + "323031332d30332d32315432303a30343a30305a", "tag");
    refuses("c11a514b67b0", "tag");
  });

  it("refuses floats", () => {
    refuses("f93c00", "float");
    refuses("fa47c35000", "float");
    refuses("fb3ff0000000000000", "float");
  });

  it("refuses simple values other than false, true and null", () => {
    refuses("f7", "simple-value");
    refuses("f0", "simple-value");
    refuses("f820", "simple-value");
  });

  it("refuses reserved bytes and a stray break", () => {
    refuses("ff", "malformed");
    refuses("1c", "malformed");
    refuses("1f", "malformed");
  });

  it("refuses integers beyond 2^53 - 1", () => {
    refuses("1b0020000000000000", "integer-out-of-range");
    refuses("3b0020000000000000", "integer-out-of-range");
    refuses("3b001fffffffffffff", "integer-out-of-range");
    refuses("1bffffffffffffffff", "integer-out-of-range");
    expect(decodeCbor(fromHex("1b001fffffffffffff"))).toBe(Number.MAX_SAFE_INTEGER);
    expect(decodeCbor(fromHex("3b001ffffffffffffe"))).toBe(-Number.MAX_SAFE_INTEGER);
  });

  it("refuses unordered, duplicate and non-text map keys", () => {
    refuses("a2" + "6162" + "00" + "6161" + "00", "unordered-keys");
    refuses("a2" + "626161" + "00" + "6162" + "00", "unordered-keys");
    refuses("a2" + "6161" + "00" + "6161" + "01", "duplicate-key");
    refuses("a1" + "01" + "00", "non-text-key");
    refuses("a1" + "4161" + "00", "non-text-key");
  });

  it("accepts canonical map order, shorter keys first", () => {
    const value = decodeCbor(fromHex("a3" + "616101" + "616202" + "62616103"));
    expect(value).toBeInstanceOf(CborMap);
    expect(asMap(value).entries.map(([k]) => k)).toEqual(["a", "b", "aa"]);
  });

  it("refuses invalid UTF-8", () => {
    refuses("61ff", "invalid-utf8");
    refuses("62c0af", "invalid-utf8"); // overlong
    refuses("63eda080", "invalid-utf8"); // surrogate
    refuses("64f4908080", "invalid-utf8"); // above U+10FFFF
    refuses("61c3", "invalid-utf8"); // cut sequence
    refuses("a1" + "61ff" + "00", "invalid-utf8"); // key too
  });

  it("refuses nesting beyond 32 levels and accepts exactly 32", () => {
    refuses("81".repeat(33) + "00", "too-deep");
    refuses("a1" + "6161" + "a1" + "6161", "truncated");
    const ok = decodeCbor(fromHex("81".repeat(32) + "00"));
    let depth = 0;
    let cur: CborValue | undefined = ok;
    while (Array.isArray(cur)) {
      depth++;
      cur = cur[0];
    }
    expect(depth).toBe(32);
    refuses("a1" + "6161".repeat(1) + "81".repeat(32) + "00", "too-deep");
  });

  it("refuses trailing bytes", () => {
    refuses("0000", "trailing-bytes");
    refuses("8100" + "00", "trailing-bytes");
  });

  it("refuses truncated input and lengths larger than the rest, before allocating", () => {
    refuses("", "truncated");
    refuses("18", "truncated");
    refuses("5affffffff", "truncated");
    refuses("5b001fffffffffffff", "truncated");
    refuses("5bffffffffffffffff", "truncated");
    refuses("7affffffff", "truncated");
    refuses("9affffffff", "truncated");
    refuses("baffffffff", "truncated");
    refuses("820a", "truncated");
    refuses("4301", "truncated");
  });

  it("returns bytes that do not alias the input", () => {
    const input = fromHex("4301" + "0203");
    const out = asBytes(decodeCbor(input));
    input[1] = 0xff;
    expect(Array.from(out)).toEqual([1, 2, 3]);
  });
});

describe("round trip", () => {
  it("returns the same tree for every type", () => {
    const tree: CborValue = new CborMap([
      ["a", 0],
      ["b", -1],
      ["big", Number.MAX_SAFE_INTEGER],
      ["small", -Number.MAX_SAFE_INTEGER],
      ["text", "héllo \u{1f600}"],
      ["bytes", new Uint8Array([0, 255, 128])],
      ["list", [1, "x", null, true, false, [], new CborMap()]],
      ["map", new CborMap([["k", new CborMap([["n", null]])]])],
      ["nil", null],
      ["yes", true],
      ["no", false],
    ]);
    const bytes = encodeCbor(tree);
    const back = decodeCbor(bytes);
    // Canonical: re-encoding gives the same bytes.
    expect(hex(encodeCbor(back))).toBe(hex(bytes));
    const m = asMap(back);
    expect(asUint(m.get("a"))).toBe(0);
    expect(asInt(m.get("b"))).toBe(-1);
    expect(asInt(m.get("small"))).toBe(-Number.MAX_SAFE_INTEGER);
    expect(asText(m.get("text"))).toBe("héllo \u{1f600}");
    expect(Array.from(asBytes(m.get("bytes")))).toEqual([0, 255, 128]);
    expect(asArray(m.get("list")).length).toBe(7);
    expect(isNull(m.get("nil"))).toBe(true);
    expect(asBool(m.get("yes"))).toBe(true);
  });

  it("round-trips integers around every width boundary", () => {
    for (const n of [
      0, 1, 23, 24, 255, 256, 65535, 65536, 4294967295, 4294967296, 2 ** 40, Number.MAX_SAFE_INTEGER,
    ]) {
      expect(decodeCbor(encodeCbor(n))).toBe(n);
      expect(decodeCbor(encodeCbor(-n))).toBe(n === 0 ? 0 : -n);
    }
  });

  it("round-trips strings and byte strings across length boundaries", () => {
    for (const n of [0, 1, 23, 24, 255, 256, 65535, 65536]) {
      const text = "x".repeat(n);
      expect(decodeCbor(encodeCbor(text))).toBe(text);
      const bytes = new Uint8Array(n).fill(7);
      expect(Array.from(asBytes(decodeCbor(encodeCbor(bytes))))).toEqual(Array.from(bytes));
    }
  });
});

describe("reader helpers", () => {
  it("throws wrong-type on a mismatch", () => {
    expect(reasonOf(() => asUint(-1))).toBe("wrong-type");
    expect(reasonOf(() => asUint("1"))).toBe("wrong-type");
    expect(reasonOf(() => asInt(null))).toBe("wrong-type");
    expect(reasonOf(() => asText(1))).toBe("wrong-type");
    expect(reasonOf(() => asBytes([1]))).toBe("wrong-type");
    expect(reasonOf(() => asArray(new CborMap()))).toBe("wrong-type");
    expect(reasonOf(() => asMap([]))).toBe("wrong-type");
    expect(reasonOf(() => asBool(0))).toBe("wrong-type");
    expect(reasonOf(() => asUint(undefined))).toBe("wrong-type");
    expect(isNull(0)).toBe(false);
  });

  it("expectArray checks the arity", () => {
    expect(expectArray([1, 2], 2)).toEqual([1, 2]);
    expect(reasonOf(() => expectArray([1], 2))).toBe("wrong-arity");
    expect(reasonOf(() => expectArray([1, 2, 3], 2))).toBe("wrong-arity");
    expect(reasonOf(() => expectArray(1, 2))).toBe("wrong-type");
  });

  it("mapReader reads required and optional keys and ignores unknown ones", () => {
    const map = asMap(decodeCbor(encodeCbor(new CborMap([["a", 1], ["extra", "ignored"]]))));
    const r = mapReader(map);
    expect(r.required("a")).toBe(1);
    expect(r.optional("a")).toBe(1);
    expect(r.optional("missing")).toBeUndefined();
    expect(reasonOf(() => r.required("missing"))).toBe("missing-key");
  });
});
