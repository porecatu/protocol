// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Pairing (spec 03 §8 and spec 02 §3): `pair-accepted`, `pair-rejected` and the
 * `pair-request` body of the handshake's third message (not a message, so it
 * has a codec but no entry in the message table).
 */

import {
  asBytes,
  asMap,
  asText,
  asUint,
  decodeCbor,
  encodeCbor,
  mapReader,
} from "../cbor/index.js";
import { utf8Encode } from "../cbor/utf8.js";
import { body, DecodeError, enumFrom, toDecodeError, type BodyCodec } from "./codec.js";
import { PAIR_REJECT_REASONS, PLATFORMS } from "./codes.js";
import type { PairAccepted, PairRejected, PairRequest } from "./types.js";

/** Length of the pairing token carried by `pair-request`, in bytes. */
const PAIR_TOKEN_LEN = 16;
/** Largest device name, in UTF-8 bytes (spec 02 §3.2). */
const DEVICE_NAME_MAX_BYTES = 64;

export const pairAcceptedCodec: BodyCodec<PairAccepted> = {
  toBody: (m) => body([["host_name", m.hostName]]),
  fromBody: (r) => ({
    type: "pair-accepted",
    hostName: asText(r.required("host_name")),
  }),
};

export const pairRejectedCodec: BodyCodec<PairRejected> = {
  toBody: (m) => body([["reason", m.reason]]),
  fromBody: (r) => ({
    type: "pair-rejected",
    reason: enumFrom(PAIR_REJECT_REASONS, asUint(r.required("reason"))),
  }),
};

/** Whether `device` has 1 to 64 UTF-8 bytes. */
function deviceNameInRange(device: string): boolean {
  const length = utf8Encode(device).length;
  return length >= 1 && length <= DEVICE_NAME_MAX_BYTES;
}

/**
 * Codec of the `pair-request` map: `token` (bstr 16), `device` (tstr of 1 to
 * 64 UTF-8 bytes) and `platform`. `toBody` throws `RangeError` for a value
 * that cannot be encoded; `fromBody` throws `DecodeError("out-of-domain")`.
 */
export const pairRequestCodec: BodyCodec<PairRequest> = {
  toBody: (m) => {
    if (m.token.length !== PAIR_TOKEN_LEN) {
      throw new RangeError(`pair-request token must be ${PAIR_TOKEN_LEN} bytes`);
    }
    if (!deviceNameInRange(m.device)) {
      throw new RangeError(`device name must have 1 to ${DEVICE_NAME_MAX_BYTES} bytes`);
    }
    return body([
      ["token", m.token],
      ["device", m.device],
      ["platform", m.platform],
    ]);
  },
  fromBody: (r) => {
    const device = asText(r.required("device"));
    if (!deviceNameInRange(device)) {
      throw new DecodeError("out-of-domain");
    }
    const token = asBytes(r.required("token"));
    if (token.length !== PAIR_TOKEN_LEN) {
      throw new DecodeError("out-of-domain");
    }
    return {
      // A copy: the decoder hands out views into the input, which the caller
      // may wipe (the payload of `msg3` is a secret).
      token: token.slice(),
      device,
      platform: enumFrom(PLATFORMS, asUint(r.required("platform"))),
    };
  },
};

/**
 * Encodes the `pair-request` as a deterministic CBOR map (the plaintext of
 * `msg3`). Throws `RangeError` for a token that is not 16 bytes or a device
 * name outside 1 to 64 bytes.
 */
export function encodePairRequest(request: PairRequest): Uint8Array {
  return encodeCbor(pairRequestCodec.toBody(request));
}

/**
 * Decodes the `pair-request` strictly; unknown keys are ignored. Throws
 * {@link DecodeError}: it never returns a partial value.
 */
export function decodePairRequest(bytes: Uint8Array): PairRequest {
  try {
    return pairRequestCodec.fromBody(mapReader(asMap(decodeCbor(bytes))));
  } catch (e) {
    throw toDecodeError(e);
  }
}
