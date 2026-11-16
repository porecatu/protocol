// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";

import { PROTOCOL_VERSION } from "../src/index.js";

describe("package entry point", () => {
  it("exposes the implemented protocol version", () => {
    expect(PROTOCOL_VERSION).toEqual([0, 1]);
  });
});
