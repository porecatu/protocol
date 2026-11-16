// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { ctEqual } from "../src/ct.js";

describe("ctEqual", () => {
  it("compares contents and lengths", () => {
    expect(ctEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 2, 3]))).toBe(true);
    expect(ctEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 2, 4]))).toBe(false);
    expect(ctEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 2]))).toBe(false);
    expect(ctEqual(new Uint8Array([]), new Uint8Array([]))).toBe(true);
  });
});
