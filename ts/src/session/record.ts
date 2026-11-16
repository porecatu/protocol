// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Records of the transport (spec 02 §5): one flags byte plus a fragment of
 * a CBOR message per Noise transport message, fragmentation and reassembly
 * of messages up to 16 MiB, and the periodic rekey.
 *
 * Sans-IO: records in, records out, `nowMs` (monotonic milliseconds) given
 * by the caller. No CBOR here: a message is opaque bytes.
 */

import { HandshakeError } from "./errors.js";
import { MAX_PLAINTEXT, type Transport } from "./handshake.js";
import {
  DEFAULT_REKEY_POLICY,
  newRekeyState,
  shouldRekey,
  type RekeyPolicy,
  type RekeyState,
} from "./rekey.js";

export { DEFAULT_REKEY_POLICY, shouldRekey, type RekeyPolicy } from "./rekey.js";

/** Flag bit 0: the message continues in the next record. */
export const FLAG_CONTINUE = 0x01;
/** Flag bit 1: the sender calls `Rekey()` on its sending cipher after this record. */
export const FLAG_REKEY = 0x02;
const RESERVED_MASK = 0xff & ~(FLAG_CONTINUE | FLAG_REKEY);

/** Largest fragment of a record: the transport plaintext minus the flags byte. */
export const MAX_FRAGMENT = MAX_PLAINTEXT - 1;
/** Largest message, sent or reassembled: 16 MiB (spec 02 §5). */
export const MAX_MESSAGE = 16 * 1024 * 1024;

/** Error codes of the record layer. */
export type RecordErrorCode = "decrypt" | "reserved-flag" | "too-large" | "failed";

/**
 * Failure of the record layer. Every error is fatal: the session is unusable
 * afterwards and the caller closes it (spec 02 §5).
 *
 * - `decrypt`: authentication failure, or a record that is not a valid
 *   Noise transport message (too long, or without the flags byte).
 * - `reserved-flag`: a reserved flag bit is set.
 * - `too-large`: a message above {@link MAX_MESSAGE}, sent or reassembled.
 * - `failed`: the session had already failed (or the cipher is exhausted).
 *
 * The message is the code itself; no prose.
 */
export class RecordError extends Error {
  readonly code: RecordErrorCode;

  constructor(code: RecordErrorCode) {
    super(code);
    this.name = "RecordError";
    this.code = code;
  }
}

/** Options of the {@link Session} constructor. */
export interface SessionOptions {
  /** Rekey schedule; defaults to {@link DEFAULT_REKEY_POLICY}. */
  policy?: RekeyPolicy;
  /**
   * `nowMs` at which the sending key started (the handshake). When omitted,
   * the age of the key counts from the first `seal`.
   */
  nowMs?: number;
}

/** A transport plus the record layer over it. */
export class Session {
  private readonly _transport: Transport;
  private readonly _policy: RekeyPolicy;
  private readonly _rekey: RekeyState;
  private _pending: Uint8Array[] = [];
  private _pendingLen = 0;
  private _failed = false;

  /** @param transport The transport returned by a completed handshake. */
  constructor(transport: Transport, options: SessionOptions = {}) {
    this._transport = transport;
    this._policy = options.policy ?? DEFAULT_REKEY_POLICY;
    this._rekey = newRekeyState(options.nowMs ?? null);
  }

  /** The peer's static public key, authenticated by the handshake. */
  get remoteStatic(): Uint8Array {
    return this._transport.remoteStatic;
  }

  /** Whether an error made the session unusable. */
  get failed(): boolean {
    return this._failed;
  }

  /**
   * Encrypts one message into one or more records, in the order to send.
   *
   * `CONTINUE` is set on every record but the last; `REKEY` on the record
   * where the policy requires it, and the sending key is swapped right after
   * encrypting that record. An empty message is one record with an empty
   * fragment. A message above {@link MAX_MESSAGE} is refused with `too-large`.
   */
  seal(message: Uint8Array, nowMs: number): Uint8Array[] {
    this._ensureUsable();
    try {
      if (message.length > MAX_MESSAGE) {
        throw new RecordError("too-large");
      }
      this._rekey.keyStartedAtMs ??= nowMs;
      const records: Uint8Array[] = [];
      let offset = 0;
      do {
        const end = Math.min(offset + MAX_FRAGMENT, message.length);
        const last = end === message.length;
        const rekey = shouldRekey(this._policy, this._rekey, nowMs);
        const plain = new Uint8Array(1 + end - offset);
        plain[0] = (last ? 0 : FLAG_CONTINUE) | (rekey ? FLAG_REKEY : 0);
        plain.set(message.subarray(offset, end), 1);
        records.push(this._transport.encrypt(plain));
        this._rekey.sentSinceRekey += 1;
        if (rekey) {
          this._transport.rekeyOutgoing();
          this._rekey.sentSinceRekey = 0;
          this._rekey.keyStartedAtMs = nowMs;
        }
        offset = end;
      } while (offset < message.length);
      return records;
    } catch (error) {
      throw this._fail(error);
    }
  }

  /**
   * Decrypts one record. Returns the message when this record completes it,
   * `null` while more records are expected.
   *
   * A reserved flag bit, an authentication failure or a reassembly above
   * {@link MAX_MESSAGE} is fatal. After a record with `REKEY`, the receiving
   * key is swapped.
   */
  open(record: Uint8Array, nowMs: number): Uint8Array | null {
    // The receiving side has no deadline of its own: the peer decides when to
    // rekey. `nowMs` is accepted so both directions share one call shape.
    void nowMs;
    this._ensureUsable();
    try {
      const plain = this._transport.decrypt(record);
      const flags = plain[0];
      if (flags === undefined) {
        throw new RecordError("decrypt");
      }
      if ((flags & RESERVED_MASK) !== 0) {
        throw new RecordError("reserved-flag");
      }
      if ((flags & FLAG_REKEY) !== 0) {
        this._transport.rekeyIncoming();
      }
      const fragment = plain.subarray(1);
      if (this._pendingLen + fragment.length > MAX_MESSAGE) {
        throw new RecordError("too-large");
      }
      if ((flags & FLAG_CONTINUE) !== 0) {
        this._pending.push(fragment);
        this._pendingLen += fragment.length;
        return null;
      }
      return this._takeMessage(fragment);
    } catch (error) {
      throw this._fail(error);
    }
  }

  private _takeMessage(last: Uint8Array): Uint8Array {
    if (this._pending.length === 0) {
      return last;
    }
    const message = new Uint8Array(this._pendingLen + last.length);
    let offset = 0;
    for (const part of this._pending) {
      message.set(part, offset);
      offset += part.length;
    }
    message.set(last, offset);
    this._pending = [];
    this._pendingLen = 0;
    return message;
  }

  private _ensureUsable(): void {
    if (this._failed) {
      throw new RecordError("failed");
    }
  }

  /**
   * Zeroes the transport keys and marks the session closed: every later
   * `seal` or `open` throws `failed`. Idempotent. The owner of the session
   * calls it when the session ends for any reason (failure, close by the
   * relay, the app leaving the screen), so no key outlives the connection.
   */
  destroy(): void {
    this._failed = true;
    this._pending = [];
    this._pendingLen = 0;
    this._transport.destroy();
  }

  /** Marks the session unusable, wipes the keys and maps the error. */
  private _fail(error: unknown): RecordError {
    this.destroy();
    if (error instanceof RecordError) {
      return error;
    }
    if (error instanceof HandshakeError) {
      return new RecordError(error.code === "wrong-state" ? "failed" : "decrypt");
    }
    throw error;
  }
}
