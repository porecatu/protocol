// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Noise `CipherState` (Noise Protocol Framework, revision 34, §5.1) with the
 * `ChaChaPoly` cipher functions (§12.3).
 *
 * This module is internal to the session layer: callers use the handshake and
 * transport API in `handshake.ts`.
 */

import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { HandshakeError } from "./errors.js";
import { zeroize } from "./keys.js";

/** Cipher key length in bytes (Noise §5.1, `k`). */
export const KEY_LEN = 32;
/** Poly1305 authentication tag length in bytes. */
export const TAG_LEN = 16;

/** 2^64 - 1: reserved nonce value, used only by `REKEY()` (Noise §5.1, §11.3). */
const MAX_NONCE = 0xffff_ffff_ffff_ffffn;
const EMPTY = new Uint8Array(0);

/**
 * ChaChaPoly nonce (Noise §12.3): 32 bits of zeros followed by the 64-bit
 * little-endian encoding of `n`, 96 bits in total.
 */
function chachaNonce(n: bigint): Uint8Array {
  const nonce = new Uint8Array(12);
  let v = n;
  for (let i = 4; i < 12; i++) {
    nonce[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  return nonce;
}

/** `ENCRYPT(k, n, ad, plaintext)` (Noise §12.3): ciphertext followed by the 16-byte tag. */
function encrypt(k: Uint8Array, n: bigint, ad: Uint8Array, plaintext: Uint8Array): Uint8Array {
  return chacha20poly1305(k, chachaNonce(n), ad).encrypt(plaintext);
}

/**
 * `DECRYPT(k, n, ad, ciphertext)` (Noise §12.3). Returns `null` on
 * authentication failure; the library signals it by throwing.
 */
function decrypt(k: Uint8Array, n: bigint, ad: Uint8Array, ciphertext: Uint8Array): Uint8Array | null {
  if (ciphertext.length < TAG_LEN) {
    return null;
  }
  try {
    return chacha20poly1305(k, chachaNonce(n), ad).decrypt(ciphertext);
  } catch {
    return null;
  }
}

/**
 * `CipherState`: a key `k` (or none) and a nonce counter `n`.
 *
 * - With no key, `encryptWithAd`/`decryptWithAd` return the input unchanged,
 *   as the spec requires during the first handshake tokens.
 * - `n` advances only after a successful operation; a failed decryption leaves
 *   it untouched (Noise §5.1).
 * - `n = 2^64 - 1` is never used for data: once the counter reaches it, every
 *   further operation fails (Noise §5.1).
 */
export class CipherState {
  private _k: Uint8Array | null = null;
  private _n = 0n;

  /** `InitializeKey(key)`: sets `k = key` (copied) and `n = 0`; `null` means empty. */
  initializeKey(key: Uint8Array | null): void {
    this._wipeKey();
    if (key === null) {
      this._k = null;
    } else {
      if (key.length !== KEY_LEN) {
        throw new HandshakeError("malformed");
      }
      this._k = key.slice();
    }
    this._n = 0n;
  }

  /** `HasKey()`. */
  hasKey(): boolean {
    return this._k !== null;
  }

  /** `SetNonce(nonce)`; only used by tests. */
  setNonce(nonce: bigint): void {
    if (nonce < 0n || nonce > MAX_NONCE) {
      throw new HandshakeError("malformed");
    }
    this._n = nonce;
  }

  /** Current nonce value; only used by tests. */
  get nonce(): bigint {
    return this._n;
  }

  /** `EncryptWithAd(ad, plaintext)`. */
  encryptWithAd(ad: Uint8Array, plaintext: Uint8Array): Uint8Array {
    const k = this._k;
    if (k === null) {
      return plaintext.slice();
    }
    if (this._n >= MAX_NONCE) {
      throw new HandshakeError("wrong-state");
    }
    const ciphertext = encrypt(k, this._n, ad, plaintext);
    this._n += 1n;
    return ciphertext;
  }

  /** `DecryptWithAd(ad, ciphertext)`; throws `decrypt` on authentication failure. */
  decryptWithAd(ad: Uint8Array, ciphertext: Uint8Array): Uint8Array {
    const k = this._k;
    if (k === null) {
      return ciphertext.slice();
    }
    if (this._n >= MAX_NONCE) {
      throw new HandshakeError("wrong-state");
    }
    const plaintext = decrypt(k, this._n, ad, ciphertext);
    if (plaintext === null) {
      throw new HandshakeError("decrypt");
    }
    this._n += 1n;
    return plaintext;
  }

  /**
   * `Rekey()` (Noise §5.1, §11.3): `k = REKEY(k)`, where `REKEY(k)` is the
   * first 32 bytes of `ENCRYPT(k, 2^64 - 1, zerolen, zeros)` with 32 zero
   * bytes of plaintext. The nonce is not reset.
   */
  rekey(): void {
    const k = this._k;
    if (k === null) {
      throw new HandshakeError("wrong-state");
    }
    const out = encrypt(k, MAX_NONCE, EMPTY, new Uint8Array(KEY_LEN));
    const next = out.slice(0, KEY_LEN);
    zeroize(out);
    zeroize(k);
    this._k = next;
  }

  /** Overwrites the key and forgets it. */
  destroy(): void {
    this._wipeKey();
    this._k = null;
    this._n = 0n;
  }

  private _wipeKey(): void {
    if (this._k !== null) {
      zeroize(this._k);
    }
  }
}
