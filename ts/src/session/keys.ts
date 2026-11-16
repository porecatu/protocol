// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Static X25519 key pairs (spec 02 §1) and the randomness source used for
 * static and ephemeral keys.
 */

import { x25519 } from "@noble/curves/ed25519.js";

/** X25519 key length in bytes (Noise `DHLEN`). */
export const DH_LEN = 32;

/** An X25519 key pair. The private key is the raw 32 bytes (clamped by X25519 on use). */
export interface StaticKeypair {
  publicKey: Uint8Array;
  privateKey: Uint8Array;
}

/**
 * Source of cryptographically secure random bytes: returns `n` fresh bytes.
 * Injectable so tests and platforms without `globalThis.crypto` can provide one.
 */
export type RandomSource = (n: number) => Uint8Array;

/** Minimal shape of the Web Crypto global; declared here to avoid `lib: DOM` and `@types/node`. */
interface MinimalCrypto {
  getRandomValues(array: Uint8Array): Uint8Array;
}

/** Web Crypto caps `getRandomValues` at 65 536 bytes per call. */
const MAX_RANDOM_CHUNK = 65_536;

/**
 * Default {@link RandomSource}: `globalThis.crypto.getRandomValues`, available
 * on Node, browsers and Hermes (with the Expo crypto polyfill). Throws if it
 * is missing, instead of falling back to an insecure source.
 */
export function defaultRandom(n: number): Uint8Array {
  const crypto = (globalThis as { crypto?: Partial<MinimalCrypto> }).crypto;
  if (crypto === undefined || typeof crypto.getRandomValues !== "function") {
    throw new Error(
      "globalThis.crypto.getRandomValues is not available; pass a RandomSource explicitly",
    );
  }
  const out = new Uint8Array(n);
  for (let offset = 0; offset < n; offset += MAX_RANDOM_CHUNK) {
    crypto.getRandomValues(out.subarray(offset, Math.min(n, offset + MAX_RANDOM_CHUNK)));
  }
  return out;
}

/** Calls `random(n)` and checks it returned exactly `n` bytes. */
export function randomBytes(random: RandomSource, n: number): Uint8Array {
  const bytes = random(n);
  if (!(bytes instanceof Uint8Array) || bytes.length !== n) {
    throw new Error("RandomSource returned the wrong number of bytes");
  }
  return bytes;
}

/** X25519 public key of a 32-byte private key. */
export function publicKeyFromPrivate(privateKey: Uint8Array): Uint8Array {
  if (privateKey.length !== DH_LEN) {
    throw new Error("X25519 private key must be 32 bytes");
  }
  return x25519.getPublicKey(privateKey);
}

/** Noise `GENERATE_KEYPAIR()` (Noise §4.1): a fresh X25519 key pair. */
export function generateStaticKeypair(random: RandomSource = defaultRandom): StaticKeypair {
  const privateKey = randomBytes(random, DH_LEN);
  return { publicKey: publicKeyFromPrivate(privateKey), privateKey };
}

/**
 * Overwrites `bytes` with zeros. Best effort: JavaScript engines may keep
 * copies, but this keeps secrets out of buffers this package still references.
 */
export function zeroize(bytes: Uint8Array): void {
  bytes.fill(0);
}
