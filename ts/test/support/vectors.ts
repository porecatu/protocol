// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Helpers shared by the tests that read the shared vectors
 * (`docs/especificacao/vetores-de-teste/`): listing the JSON files, the typed
 * value of the README ("O valor tipado"), and hex fields.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { CborMap, type CborValue } from "../../src/cbor/index.js";
import { hexDecode, hexEncode } from "../../src/encoding.js";

// --- Vector files ---

/** The directory of the shared vectors. */
export const ROOT = resolve(import.meta.dirname, "../../../docs/especificacao/vetores-de-teste");

/** One vector file, parsed. */
export interface VectorFile {
  /** Path relative to the vectors root, with `/` separators. */
  readonly name: string;
  readonly json: Record<string, unknown>;
}

function listJson(dir: string, prefix: string): VectorFile[] {
  const out: VectorFile[] = [];
  for (const entry of readdirSync(dir).sort()) {
    const path = join(dir, entry);
    const name = `${prefix}${entry}`;
    if (statSync(path).isDirectory()) {
      out.push(...listJson(path, `${name}/`));
    } else if (entry.endsWith(".json")) {
      out.push({ name, json: JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown> });
    }
  }
  return out;
}

/** Every vector of one category (a folder of the vectors root). */
export function vectorsIn(category: string): VectorFile[] {
  return listJson(join(ROOT, category), `${category}/`);
}

// --- Typed values (README, "O valor tipado") ---

/** Converts the typed JSON tree of a vector into a {@link CborValue}. */
export function fromTyped(typed: unknown): CborValue {
  if (typed === null || typeof typed === "boolean" || typeof typed === "number" || typeof typed === "string") {
    return typed;
  }
  if (Array.isArray(typed)) {
    return typed.map(fromTyped);
  }
  if (typeof typed === "object") {
    const object = typed as Record<string, unknown>;
    if ("bstr" in object) {
      const bytes = hexDecode(object["bstr"] as string);
      if (bytes === null) {
        throw new Error("bad hex in typed value");
      }
      return bytes;
    }
    if ("map" in object) {
      const pairs = object["map"] as Array<[string, unknown]>;
      return new CborMap(pairs.map(([key, value]) => [key, fromTyped(value)] as const));
    }
  }
  throw new Error("unknown typed value");
}

/** The inverse of {@link fromTyped}. */
export function toTyped(value: CborValue): unknown {
  if (value === null || typeof value === "boolean" || typeof value === "number" || typeof value === "string") {
    return value;
  }
  if (value instanceof Uint8Array) {
    return { bstr: hexEncode(value) };
  }
  if (Array.isArray(value)) {
    return value.map(toTyped);
  }
  return { map: value.entries.map(([key, item]) => [key, toTyped(item)]) };
}

/** The bytes of the hex field `key` of a vector. */
export function hex(vector: Record<string, unknown>, key: string): Uint8Array {
  return hexOf(vector[key] as string, key);
}

/** The bytes of a lowercase hex string; `what` names it in the error. */
export function hexOf(text: string, what: string): Uint8Array {
  const bytes = hexDecode(text);
  if (bytes === null) {
    throw new Error(`bad hex in ${what}`);
  }
  return bytes;
}

/** The `erro` of a vector that must be refused. */
export interface VectorError {
  readonly camada: string;
  readonly codigo: string;
}
