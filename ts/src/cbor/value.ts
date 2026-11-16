// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The generic tree every message codec reads from and writes to. Integers are
 * JavaScript numbers restricted to `Number.isSafeInteger` (spec 03 §1.1), text
 * is a string, bytes are a `Uint8Array`. Arrays are plain arrays; maps are
 * {@link CborMap}, so a map is never confused with an array and its pairs are
 * kept as given (the encoder orders them, the decoder checks the order).
 */
export type CborValue =
  | number
  | string
  | Uint8Array
  | boolean
  | null
  | CborValue[]
  | CborMap;

/** A CBOR map with text keys. Order and uniqueness are enforced on encode. */
export class CborMap {
  readonly entries: ReadonlyArray<readonly [string, CborValue]>;

  constructor(entries: ReadonlyArray<readonly [string, CborValue]> = []) {
    this.entries = entries;
  }

  /** Value of `key`, or `undefined` when absent. Linear: maps are small. */
  get(key: string): CborValue | undefined {
    for (const [k, v] of this.entries) {
      if (k === key) {
        return v;
      }
    }
    return undefined;
  }

  get size(): number {
    return this.entries.length;
  }
}
