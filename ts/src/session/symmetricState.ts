// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Noise `SymmetricState` (Noise Protocol Framework, revision 34, §5.2) with
 * the `BLAKE2s` hash functions (§12.8) and the Noise `HKDF` (§4.3).
 *
 * Internal to the session layer.
 */

import { blake2s } from "@noble/hashes/blake2.js";
import { hmac } from "@noble/hashes/hmac.js";
import { CipherState } from "./cipherState.js";
import { zeroize } from "./keys.js";

/** BLAKE2s output length (Noise `HASHLEN`). */
export const HASH_LEN = 32;

/** `HASH(data)`: BLAKE2s with a 32-byte digest. */
export function hash(data: Uint8Array): Uint8Array {
  return blake2s(data);
}

/** `HMAC-HASH(key, data)`: HMAC (RFC 2104) over BLAKE2s, `BLOCKLEN` 64. */
function hmacHash(key: Uint8Array, data: Uint8Array): Uint8Array {
  return hmac(blake2s, key, data);
}

function concat(a: Uint8Array, b: Uint8Array): Uint8Array {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

/**
 * Noise `HKDF(chaining_key, input_key_material, num_outputs)` (Noise §4.3).
 * This is the Noise construction, not RFC 5869 with salt/info:
 *
 * ```
 * temp_key = HMAC-HASH(chaining_key, input_key_material)
 * output1  = HMAC-HASH(temp_key, byte(0x01))
 * output2  = HMAC-HASH(temp_key, output1 || byte(0x02))
 * output3  = HMAC-HASH(temp_key, output2 || byte(0x03))   (if num_outputs == 3)
 * ```
 */
export function hkdf(
  chainingKey: Uint8Array,
  inputKeyMaterial: Uint8Array,
  numOutputs: 2 | 3,
): Uint8Array[] {
  const tempKey = hmacHash(chainingKey, inputKeyMaterial);
  const output1 = hmacHash(tempKey, Uint8Array.of(0x01));
  const in2 = concat(output1, Uint8Array.of(0x02));
  const output2 = hmacHash(tempKey, in2);
  zeroize(in2);
  const outputs = [output1, output2];
  if (numOutputs === 3) {
    const in3 = concat(output2, Uint8Array.of(0x03));
    outputs.push(hmacHash(tempKey, in3));
    zeroize(in3);
  }
  zeroize(tempKey);
  return outputs;
}

/** The two `CipherState`s returned by `Split()`: `c1` for initiator→responder, `c2` for the reverse. */
export interface SplitResult {
  c1: CipherState;
  c2: CipherState;
}

/** `SymmetricState`: chaining key `ck`, handshake hash `h` and a `CipherState`. */
export class SymmetricState {
  private readonly _cipher = new CipherState();
  private _ck: Uint8Array;
  private _h: Uint8Array;

  /**
   * `InitializeSymmetric(protocol_name)`: if the name fits in `HASHLEN` bytes,
   * `h` is the name padded with zeros; otherwise `h = HASH(name)`. `ck = h`,
   * and the cipher starts with no key.
   */
  constructor(protocolName: Uint8Array) {
    if (protocolName.length <= HASH_LEN) {
      this._h = new Uint8Array(HASH_LEN);
      this._h.set(protocolName, 0);
    } else {
      this._h = hash(protocolName);
    }
    this._ck = this._h.slice();
    this._cipher.initializeKey(null);
  }

  /** `MixKey(input_key_material)`: `ck, temp_k = HKDF(ck, ikm, 2)`; `InitializeKey(temp_k)`. */
  mixKey(inputKeyMaterial: Uint8Array): void {
    const [ck, tempK] = hkdf(this._ck, inputKeyMaterial, 2) as [Uint8Array, Uint8Array];
    zeroize(this._ck);
    this._ck = ck;
    // HASHLEN is 32 for BLAKE2s, so temp_k needs no truncation.
    this._cipher.initializeKey(tempK);
    zeroize(tempK);
  }

  /** `MixHash(data)`: `h = HASH(h || data)`. */
  mixHash(data: Uint8Array): void {
    this._h = hash(concat(this._h, data));
  }

  /** `GetHandshakeHash()`: a copy of `h`. */
  handshakeHash(): Uint8Array {
    return this._h.slice();
  }

  /** `HasKey()` of the inner cipher; decides whether a static key arrives encrypted. */
  hasKey(): boolean {
    return this._cipher.hasKey();
  }

  /** `EncryptAndHash(plaintext)`: `ciphertext = EncryptWithAd(h, plaintext)`; `MixHash(ciphertext)`. */
  encryptAndHash(plaintext: Uint8Array): Uint8Array {
    const ciphertext = this._cipher.encryptWithAd(this._h, plaintext);
    this.mixHash(ciphertext);
    return ciphertext;
  }

  /** `DecryptAndHash(ciphertext)`: `plaintext = DecryptWithAd(h, ciphertext)`; `MixHash(ciphertext)`. */
  decryptAndHash(ciphertext: Uint8Array): Uint8Array {
    const plaintext = this._cipher.decryptWithAd(this._h, ciphertext);
    this.mixHash(ciphertext);
    return plaintext;
  }

  /**
   * `Split()`: `temp_k1, temp_k2 = HKDF(ck, zerolen, 2)`; two new
   * `CipherState`s keyed with them. Wipes `ck` and the handshake cipher,
   * which are not used after the split; `h` stays readable.
   */
  split(): SplitResult {
    const [k1, k2] = hkdf(this._ck, new Uint8Array(0), 2) as [Uint8Array, Uint8Array];
    const c1 = new CipherState();
    const c2 = new CipherState();
    c1.initializeKey(k1);
    c2.initializeKey(k2);
    zeroize(k1);
    zeroize(k2);
    this.destroy();
    return { c1, c2 };
  }

  /** Overwrites `ck` and the cipher key. */
  destroy(): void {
    zeroize(this._ck);
    this._cipher.destroy();
  }
}
