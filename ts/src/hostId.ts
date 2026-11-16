// SPDX-License-Identifier: GPL-3.0-or-later

import { blake2s } from "@noble/hashes/blake2.js";
import { base32Decode, base32Encode } from "./encoding.js";

/** Length of the routing public key (Ed25519), in bytes. */
export const ROUTING_KEY_LEN = 32;

/** Number of digest bytes kept in a host id (160 bits). */
const HOST_ID_DIGEST_LEN = 20;

/** Length of a host id in characters (20 bytes in base32). */
export const HOST_ID_LEN = 32;

/**
 * Address of a host at the relay (spec 01 §3):
 * `base32_lowercase_no_padding( BLAKE2s-256( routing_public_key )[0..20] )`.
 * Always 32 characters of `a-z2-7`.
 */
export function hostId(routingKey: Uint8Array): string {
  if (routingKey.length !== ROUTING_KEY_LEN) {
    throw new RangeError(`routing key must be ${ROUTING_KEY_LEN} bytes`);
  }
  return base32Encode(blake2s(routingKey).subarray(0, HOST_ID_DIGEST_LEN));
}

/**
 * Validates a host id taken from untrusted text: exactly 32 characters of
 * lowercase base32, canonical. Returns it unchanged, or `null` when malformed.
 */
export function parseHostId(text: string): string | null {
  if (text.length !== HOST_ID_LEN) {
    return null;
  }
  const bytes = base32Decode(text);
  return bytes !== null && bytes.length === HOST_ID_DIGEST_LEN ? text : null;
}
