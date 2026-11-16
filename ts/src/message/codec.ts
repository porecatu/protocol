// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Shared machinery of the message codecs. Every message module follows the
 * pattern documented in `index.ts`: a `BodyCodec<M>` per message, built from
 * the helpers here, and registered in the table of `index.ts`.
 */

import { CborError, CborMap, type CborErrorReason, type CborValue, type MapReader } from "../cbor/index.js";

/**
 * Why a message was refused. The CBOR reasons come from the strict decoder and
 * the typed readers; the others are decided by the message layer. Every reason
 * maps to `error { code: protocol }` (spec 03 §1); no prose crosses the wire.
 */
export type DecodeErrorReason =
  | CborErrorReason
  /** Message type code not in the table of spec 03 §10. */
  | "unknown-type"
  /** Known type, but sent by the side that does not send it. */
  | "wrong-direction"
  /** Well-typed value outside its domain (unknown enum number, bad length). */
  | "out-of-domain";

export class DecodeError extends Error {
  readonly reason: DecodeErrorReason;

  constructor(reason: DecodeErrorReason) {
    super(reason);
    this.name = "DecodeError";
    this.reason = reason;
  }
}

/** Converts anything the readers throw into a {@link DecodeError}; rethrows bugs. */
export function toDecodeError(e: unknown): DecodeError {
  if (e instanceof DecodeError) {
    return e;
  }
  if (e instanceof CborError) {
    return new DecodeError(e.reason);
  }
  throw e;
}

/** Body codec of one message: its map on the wire <-> its object in memory. */
export interface BodyCodec<M> {
  toBody(message: M): CborMap;
  fromBody(reader: MapReader): M;
}

/** The value of an enum, or `out-of-domain` when `n` is not one of `values`. */
export function enumFrom<T extends number>(values: readonly T[], n: number): T {
  for (const v of values) {
    if (v === n) {
      return v;
    }
  }
  throw new DecodeError("out-of-domain");
}

/** Builds a body map; entries whose value is `undefined` are omitted (optional keys). */
export function body(
  entries: ReadonlyArray<readonly [string, CborValue | undefined]>,
): CborMap {
  const present: Array<readonly [string, CborValue]> = [];
  for (const [key, value] of entries) {
    if (value !== undefined) {
      present.push([key, value]);
    }
  }
  return new CborMap(present);
}

/** Placeholder for message bodies that the next steps implement, file by file. */
export function notImplemented<M>(name: string): BodyCodec<M> {
  return {
    toBody(): CborMap {
      throw new Error(`not implemented: ${name}`);
    },
    fromBody(): M {
      throw new Error(`not implemented: ${name}`);
    },
  };
}
