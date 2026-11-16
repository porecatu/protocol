// SPDX-License-Identifier: GPL-3.0-or-later

import { blake2s } from "@noble/hashes/blake2.js";
import { base32Encode } from "./encoding.js";

/** Length of a static public key (X25519), in bytes. */
const PUBLIC_KEY_LEN = 32;

/** Digest bytes kept in a fingerprint (80 bits, 16 base32 characters). */
const FINGERPRINT_DIGEST_LEN = 10;

/**
 * Human-readable digest of a public key (spec 02 §1):
 * `base32_uppercase_no_padding( BLAKE2s-256( pub )[0..10] )`, 16 characters.
 */
export function fingerprint(publicKey: Uint8Array): string {
  if (publicKey.length !== PUBLIC_KEY_LEN) {
    throw new RangeError(`public key must be ${PUBLIC_KEY_LEN} bytes`);
  }
  return base32Encode(blake2s(publicKey).subarray(0, FINGERPRINT_DIGEST_LEN), "upper");
}

/** Display form of a fingerprint: four groups of four, `"ABCD EFGH IJKL MNOP"`. */
export function formatFingerprint(fp: string): string {
  if (fp.length !== 16) {
    throw new RangeError("fingerprint must be 16 characters");
  }
  return [fp.slice(0, 4), fp.slice(4, 8), fp.slice(8, 12), fp.slice(12, 16)].join(" ");
}
