// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The transcripts of `sessoes-ts/`: whole conversations recorded from this
 * package's `ClientSession` and `HostChannel` (see `support/transcript.ts`).
 *
 * Normally the test checks that the committed files are exactly what the
 * implementation writes today, so a change to the wire cannot slip past the
 * Rust side unnoticed. To rewrite the files:
 *
 *     WRITE_TRANSCRIPTS=1 pnpm exec vitest run test/transcripts.test.ts
 *
 * and then run `cargo test --workspace` in `rust/`, which replays them.
 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";
import { render, transcripts } from "./support/transcript.js";
import { ROOT } from "./support/vectors.js";

const WRITE = (process.env["WRITE_TRANSCRIPTS"] ?? "") !== "";

describe.skipIf(!WRITE)("write the transcripts", () => {
  it("writes sessoes-ts/", () => {
    for (const { path, vector } of transcripts()) {
      const file = join(ROOT, path);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, render(vector), "utf8");
    }
  });
});

describe.skipIf(WRITE)("transcripts", () => {
  it("are deterministic", () => {
    expect(transcripts().map(({ vector }) => render(vector))).toEqual(
      transcripts().map(({ vector }) => render(vector)),
    );
  });
  for (const { path, vector } of transcripts()) {
    it(`${path} is what the implementation writes`, () => {
      // Git may check the files out with CRLF on Windows.
      const committed = readFileSync(join(ROOT, path), "utf8").replace(/\r\n/g, "\n");
      expect(committed).toBe(render(vector));
    });
  }
});
