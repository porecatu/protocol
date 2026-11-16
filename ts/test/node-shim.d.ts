// SPDX-License-Identifier: GPL-3.0-or-later

// Minimal declarations of the Node APIs the tests use. The package has no
// `@types/node` on purpose (`types: []`): `src/` must not see Node at all, and
// the tests need only a file reader for the shared vectors.

declare module "node:fs" {
  export function readdirSync(path: string): string[];
  export function statSync(path: string): { isDirectory(): boolean };
  export function readFileSync(path: string, encoding: "utf8"): string;
  export function writeFileSync(path: string, data: string, encoding: "utf8"): void;
  export function mkdirSync(path: string, options: { recursive: true }): void;
}

declare module "node:path" {
  export function join(...parts: string[]): string;
  export function resolve(...parts: string[]): string;
  export function dirname(path: string): string;
}

/** Only the environment, read by `transcripts.test.ts` (`WRITE_TRANSCRIPTS`). */
declare const process: { readonly env: Readonly<Record<string, string | undefined>> };

interface ImportMeta {
  readonly dirname: string;
}
