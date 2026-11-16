// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Why a CBOR item was refused (spec 03 §1.1) or a decoded tree did not have
 * the shape a codec expected. One literal per reason; no prose crosses the
 * wire, and the `message` of {@link CborError} is just the literal.
 */
export type CborErrorReason =
  /** Integer or length not in its shortest form. */
  | "non-canonical"
  /** Indefinite-length item or break byte. */
  | "indefinite-length"
  /** Any CBOR tag (major type 6). */
  | "tag"
  /** Floating-point number (decoder), or a non-integer number (encoder). */
  | "float"
  /** Simple value other than false, true and null (includes `undefined`). */
  | "simple-value"
  /** Reserved additional-information byte or stray break. */
  | "malformed"
  /** Integer outside -(2^53 - 1) .. 2^53 - 1. */
  | "integer-out-of-range"
  /** Map key that is not a text string. */
  | "non-text-key"
  /** Map keys not sorted by their encoded bytes. */
  | "unordered-keys"
  /** Same key twice in one map. */
  | "duplicate-key"
  /** Text string that is not valid UTF-8 (or a JS string with lone surrogates). */
  | "invalid-utf8"
  /** Arrays and maps nested deeper than 32 levels. */
  | "too-deep"
  /** Bytes left after the item. */
  | "trailing-bytes"
  /** Input ends before the item does, or a length exceeds what is left. */
  | "truncated"
  /** Decoded value has a different type than the codec expects. */
  | "wrong-type"
  /** Positional array with the wrong number of elements. */
  | "wrong-arity"
  /** Required map key absent. */
  | "missing-key";

export class CborError extends Error {
  readonly reason: CborErrorReason;

  constructor(reason: CborErrorReason) {
    super(reason);
    this.name = "CborError";
    this.reason = reason;
  }
}
