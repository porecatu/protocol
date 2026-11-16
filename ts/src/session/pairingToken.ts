// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The pairing token of the QR code (spec 02 §3.1 and §3.2, item 3).
 *
 * A token is 16 random bytes, single use, valid for five minutes. The host
 * keeps at most one live token: the application owns that rule (generating
 * another token means dropping this one). This class enforces what is local to
 * one token: the deadline, the single use and the constant-time comparison.
 *
 * Sans-IO: the clock is a monotonic millisecond number received from outside.
 */

import { ctEqual } from "../ct.js";
import { defaultRandom, randomBytes, zeroize, type RandomSource } from "./keys.js";

/** How long a token stays valid: five minutes (spec 02 §3.1). */
export const TOKEN_TTL_MS = 300_000;

/** Size of a token in bytes. */
export const TOKEN_LEN = 16;

/**
 * A pairing token with its deadline and its single-use state.
 *
 * The bytes live in a true private field: they do not show up in
 * `console.log`, `JSON.stringify` or `String(token)`.
 */
export class PairingToken {
  readonly #bytes: Uint8Array;
  readonly #expiresAtMs: number;
  #consumed = false;

  /**
   * Wraps given bytes (a token read back from somewhere, or injected in a
   * test). It is valid for {@link TOKEN_TTL_MS} after `nowMs`. The bytes are
   * copied. Throws `RangeError` when they are not {@link TOKEN_LEN} bytes.
   */
  constructor(bytes: Uint8Array, nowMs: number) {
    if (bytes.length !== TOKEN_LEN) {
      throw new RangeError(`token must be ${TOKEN_LEN} bytes`);
    }
    this.#bytes = bytes.slice();
    this.#expiresAtMs = Math.min(nowMs + TOKEN_TTL_MS, Number.MAX_SAFE_INTEGER);
  }

  /**
   * Generates a token from `random` (default: `globalThis.crypto`). Throws if
   * no random source is available: there is no safe fallback for a secret.
   */
  static generate(nowMs: number, random: RandomSource = defaultRandom): PairingToken {
    return new PairingToken(randomBytes(random, TOKEN_LEN), nowMs);
  }

  /** A copy of the token bytes, for the QR code. A secret: never log it. */
  get bytes(): Uint8Array {
    return this.#bytes.slice();
  }

  /** The monotonic time at which the token stops being valid. */
  get expiresAtMs(): number {
    return this.#expiresAtMs;
  }

  /** Whether the token was already presented, matching or not. */
  get isConsumed(): boolean {
    return this.#consumed;
  }

  /** Whether the token can still be presented: not expired and not used. */
  isLive(nowMs: number): boolean {
    return !this.#consumed && nowMs < this.#expiresAtMs;
  }

  /**
   * Checks a token presented by a client (spec 02 §3.2, item 3).
   *
   * The comparison is constant-time. The token is **consumed even when the
   * check fails**: whoever presents a wrong token burns the right one, so a
   * guess costs a new QR code. An expired or already consumed token never
   * matches.
   */
  matchesAndConsume(presented: Uint8Array, nowMs: number): boolean {
    // The secret comparison always runs, so the time does not tell which of
    // the (public) conditions below decided.
    const equal = ctEqual(this.#bytes, presented);
    const live = this.isLive(nowMs);
    this.#consumed = true;
    return equal && live;
  }

  /** Overwrites the token bytes; it never matches afterwards. */
  destroy(): void {
    zeroize(this.#bytes);
    this.#consumed = true;
  }

  /** Never shows the bytes. */
  toString(): string {
    return `PairingToken(expiresAtMs=${this.#expiresAtMs}, consumed=${this.#consumed}, bytes=<redacted>)`;
  }

  /** Never shows the bytes. */
  toJSON(): string {
    return this.toString();
  }
}
