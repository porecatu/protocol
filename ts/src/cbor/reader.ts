// SPDX-License-Identifier: GPL-3.0-or-later

import { CborError } from "./errors.js";
import { CborMap, type CborValue } from "./value.js";

export function asUint(value: CborValue | undefined): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new CborError("wrong-type");
  }
  return value;
}

export function asInt(value: CborValue | undefined): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new CborError("wrong-type");
  }
  return value;
}

export function asText(value: CborValue | undefined): string {
  if (typeof value !== "string") {
    throw new CborError("wrong-type");
  }
  return value;
}

export function asBytes(value: CborValue | undefined): Uint8Array {
  if (!(value instanceof Uint8Array)) {
    throw new CborError("wrong-type");
  }
  return value;
}

export function asArray(value: CborValue | undefined): CborValue[] {
  if (!Array.isArray(value)) {
    throw new CborError("wrong-type");
  }
  return value;
}

export function asMap(value: CborValue | undefined): CborMap {
  if (!(value instanceof CborMap)) {
    throw new CborError("wrong-type");
  }
  return value;
}

export function asBool(value: CborValue | undefined): boolean {
  if (typeof value !== "boolean") {
    throw new CborError("wrong-type");
  }
  return value;
}

export function isNull(value: CborValue | undefined): value is null {
  return value === null;
}

/** Positional array with exactly `arity` elements (spec 03 §1.1). */
export function expectArray(value: CborValue | undefined, arity: number): CborValue[] {
  const items = asArray(value);
  if (items.length !== arity) {
    throw new CborError("wrong-arity");
  }
  return items;
}

export interface MapReader {
  /** The value of `key`; throws `missing-key` when absent. */
  required(key: string): CborValue;
  /** The value of `key`, or `undefined` when absent. */
  optional(key: string): CborValue | undefined;
}

/**
 * Field access over a decoded map. Unknown keys are simply never asked for:
 * spec 03 §1.1 item 3 ignores them (canonical form was already checked).
 */
export function mapReader(map: CborMap): MapReader {
  return {
    required(key: string): CborValue {
      const value = map.get(key);
      if (value === undefined) {
        throw new CborError("missing-key");
      }
      return value;
    },
    optional(key: string): CborValue | undefined {
      return map.get(key);
    },
  };
}
