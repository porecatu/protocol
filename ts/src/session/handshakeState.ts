// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Noise `HandshakeState` (Noise Protocol Framework, revision 34, §5.3): a
 * token interpreter for the `XX` and `IK` patterns (§7) over X25519 (§12.1).
 *
 * Internal to the session layer; `handshake.ts` builds the protocol API on
 * top of it. Payloads here are arbitrary, so the cacophony vectors can drive
 * it directly.
 */

import { x25519 } from "@noble/curves/ed25519.js";
import type { CipherState } from "./cipherState.js";
import { TAG_LEN } from "./cipherState.js";
import { HandshakeError } from "./errors.js";
import {
  DH_LEN,
  publicKeyFromPrivate,
  randomBytes,
  zeroize,
  type RandomSource,
  type StaticKeypair,
} from "./keys.js";
import { SymmetricState } from "./symmetricState.js";

/** Largest Noise message, handshake or transport (Noise §3). */
export const MAX_NOISE_MESSAGE = 65_535;

/** Message tokens of the patterns used here (no PSK). */
export type Token = "e" | "s" | "ee" | "es" | "se" | "ss";

/** A handshake pattern (Noise §7): pre-messages and message token lists. */
export interface HandshakePattern {
  /** Pattern name as it appears in the protocol name (`XX`, `IK`). */
  readonly name: string;
  /** Pre-message tokens of the initiator (`-> ...` before `...`). */
  readonly initiatorPreMessage: readonly ("e" | "s")[];
  /** Pre-message tokens of the responder (`<- ...` before `...`). */
  readonly responderPreMessage: readonly ("e" | "s")[];
  /** Message patterns; even indices are sent by the initiator, odd by the responder. */
  readonly messages: readonly (readonly Token[])[];
}

/**
 * ```
 * XX:
 *   -> e
 *   <- e, ee, s, es
 *   -> s, se
 * ```
 */
export const PATTERN_XX: HandshakePattern = {
  name: "XX",
  initiatorPreMessage: [],
  responderPreMessage: [],
  messages: [["e"], ["e", "ee", "s", "es"], ["s", "se"]],
};

/**
 * ```
 * IK:
 *   <- s
 *   ...
 *   -> e, es, s, ss
 *   <- e, ee, se
 * ```
 */
export const PATTERN_IK: HandshakePattern = {
  name: "IK",
  initiatorPreMessage: [],
  responderPreMessage: ["s"],
  messages: [
    ["e", "es", "s", "ss"],
    ["e", "ee", "se"],
  ],
};

/** Options of {@link HandshakeState}. */
export interface HandshakeStateOptions {
  /** Full Noise protocol name, e.g. `Noise_XX_25519_ChaChaPoly_BLAKE2s`. */
  protocolName: string;
  pattern: HandshakePattern;
  initiator: boolean;
  prologue: Uint8Array;
  /** Local static key pair (`s`). Copied; the caller keeps ownership of its own buffers. */
  s?: StaticKeypair;
  /** Remote static public key known in advance (`rs`), for pre-messages. */
  rs?: Uint8Array;
  /** Source of ephemeral keys. */
  random: RandomSource;
  /**
   * Fixed ephemeral private key, used instead of `random` when the `e` token is
   * written. **Only for tests and test vectors**: reusing an ephemeral key
   * destroys forward secrecy and can expose the static keys.
   */
  ephemeral?: Uint8Array;
}

/** Result of a completed handshake: the transport ciphers already oriented for this side. */
export interface HandshakeResult {
  send: CipherState;
  receive: CipherState;
  handshakeHash: Uint8Array;
  remoteStatic: Uint8Array;
}

function asciiBytes(text: string): Uint8Array {
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c > 0x7f) {
      throw new HandshakeError("malformed");
    }
    out[i] = c;
  }
  return out;
}

function isAllZero(bytes: Uint8Array): boolean {
  let acc = 0;
  for (let i = 0; i < bytes.length; i++) {
    acc |= bytes[i] ?? 0;
  }
  return acc === 0;
}

/**
 * `DH(key_pair, public_key)` (Noise §12.1): X25519. A result of 32 zero bytes
 * is refused with `dh-zero` (spec 02 §2). `@noble/curves` already refuses
 * low-order public keys before the ladder by throwing; that is normalized to
 * the same error, and the zero check stays as the protocol's own rule.
 */
function dh(privateKey: Uint8Array, publicKey: Uint8Array): Uint8Array {
  let shared: Uint8Array;
  try {
    shared = x25519.getSharedSecret(privateKey, publicKey);
  } catch {
    throw new HandshakeError("dh-zero");
  }
  if (shared.length !== DH_LEN || isAllZero(shared)) {
    zeroize(shared);
    throw new HandshakeError("dh-zero");
  }
  return shared;
}

function copyKeypair(pair: StaticKeypair): StaticKeypair {
  if (pair.privateKey.length !== DH_LEN || pair.publicKey.length !== DH_LEN) {
    throw new HandshakeError("malformed");
  }
  return { publicKey: pair.publicKey.slice(), privateKey: pair.privateKey.slice() };
}

/** Byte reader over a received handshake message. */
class Reader {
  private _offset = 0;
  private readonly _bytes: Uint8Array;

  constructor(bytes: Uint8Array) {
    this._bytes = bytes;
  }

  take(n: number): Uint8Array {
    if (this._offset + n > this._bytes.length) {
      throw new HandshakeError("malformed");
    }
    const out = this._bytes.slice(this._offset, this._offset + n);
    this._offset += n;
    return out;
  }

  rest(): Uint8Array {
    const out = this._bytes.slice(this._offset);
    this._offset = this._bytes.length;
    return out;
  }
}

/**
 * Noise `HandshakeState`. Each instance runs one handshake; after the last
 * message it holds the split ciphers ({@link HandshakeState.result}). Any
 * failure wipes the state and every later call fails with `wrong-state`
 * (Noise §5.3: a failed handshake is abandoned).
 */
export class HandshakeState {
  private readonly _symmetric: SymmetricState;
  private readonly _pattern: HandshakePattern;
  private readonly _initiator: boolean;
  private readonly _random: RandomSource;
  private _s: StaticKeypair | null;
  private _e: StaticKeypair | null = null;
  private _rs: Uint8Array | null;
  private _re: Uint8Array | null = null;
  private _fixedEphemeral: Uint8Array | null;
  private _index = 0;
  private _failed = false;
  private _result: HandshakeResult | null = null;

  /**
   * `Initialize(handshake_pattern, initiator, prologue, s, e, rs, re)`:
   * `InitializeSymmetric(protocol_name)`, `MixHash(prologue)`, then `MixHash`
   * of each public key in the pre-messages, initiator's first.
   */
  constructor(options: HandshakeStateOptions) {
    this._pattern = options.pattern;
    this._initiator = options.initiator;
    this._random = options.random;
    this._s = options.s === undefined ? null : copyKeypair(options.s);
    if (options.rs !== undefined && options.rs.length !== DH_LEN) {
      throw new HandshakeError("malformed");
    }
    this._rs = options.rs === undefined ? null : options.rs.slice();
    if (options.ephemeral !== undefined && options.ephemeral.length !== DH_LEN) {
      throw new HandshakeError("malformed");
    }
    this._fixedEphemeral = options.ephemeral === undefined ? null : options.ephemeral.slice();

    this._symmetric = new SymmetricState(asciiBytes(options.protocolName));
    this._symmetric.mixHash(options.prologue);
    this._mixPreMessage(this._pattern.initiatorPreMessage, true);
    this._mixPreMessage(this._pattern.responderPreMessage, false);
  }

  private _mixPreMessage(tokens: readonly ("e" | "s")[], ofInitiator: boolean): void {
    const local = ofInitiator === this._initiator;
    for (const token of tokens) {
      if (token === "s") {
        const key = local ? this._s?.publicKey : this._rs;
        if (key === undefined || key === null) {
          throw new HandshakeError("wrong-state");
        }
        this._symmetric.mixHash(key);
      } else {
        // Ephemeral pre-messages do not occur in XX or IK.
        throw new HandshakeError("wrong-state");
      }
    }
  }

  /** True once the last message pattern has been processed. */
  get isComplete(): boolean {
    return this._result !== null;
  }

  /** True when the next message is ours to write. */
  get isMyTurn(): boolean {
    return (this._index % 2 === 0) === this._initiator;
  }

  /** The remote static key, once known (pre-message or `s` token). */
  get remoteStatic(): Uint8Array | null {
    return this._rs === null ? null : this._rs.slice();
  }

  /** `GetHandshakeHash()`. */
  handshakeHash(): Uint8Array {
    return this._symmetric.handshakeHash();
  }

  /** The split ciphers, oriented for this side; `wrong-state` before completion. */
  result(): HandshakeResult {
    if (this._result === null) {
      throw new HandshakeError("wrong-state");
    }
    return this._result;
  }

  /** `WriteMessage(payload, message_buffer)`: returns the message. */
  writeMessage(payload: Uint8Array): Uint8Array {
    this._checkUsable();
    if (!this.isMyTurn) {
      throw new HandshakeError("wrong-state");
    }
    return this._guard(() => {
      const tokens = this._currentTokens();
      const parts: Uint8Array[] = [];
      for (const token of tokens) {
        switch (token) {
          case "e": {
            if (this._e !== null) {
              throw new HandshakeError("wrong-state");
            }
            this._e = this._generateEphemeral();
            parts.push(this._e.publicKey.slice());
            this._symmetric.mixHash(this._e.publicKey);
            break;
          }
          case "s": {
            const s = this._requireS();
            parts.push(this._symmetric.encryptAndHash(s.publicKey));
            break;
          }
          default:
            this._dhToken(token);
        }
      }
      parts.push(this._symmetric.encryptAndHash(payload));
      const message = concatAll(parts);
      if (message.length > MAX_NOISE_MESSAGE) {
        throw new HandshakeError("malformed");
      }
      this._advance();
      return message;
    });
  }

  /** `ReadMessage(message, payload_buffer)`: returns the payload. */
  readMessage(message: Uint8Array): Uint8Array {
    this._checkUsable();
    if (this.isMyTurn) {
      throw new HandshakeError("wrong-state");
    }
    return this._guard(() => {
      if (message.length > MAX_NOISE_MESSAGE) {
        throw new HandshakeError("malformed");
      }
      const reader = new Reader(message);
      for (const token of this._currentTokens()) {
        switch (token) {
          case "e": {
            if (this._re !== null) {
              throw new HandshakeError("wrong-state");
            }
            this._re = reader.take(DH_LEN);
            this._symmetric.mixHash(this._re);
            break;
          }
          case "s": {
            if (this._rs !== null) {
              throw new HandshakeError("wrong-state");
            }
            const len = this._symmetric.hasKey() ? DH_LEN + TAG_LEN : DH_LEN;
            this._rs = this._symmetric.decryptAndHash(reader.take(len));
            break;
          }
          default:
            this._dhToken(token);
        }
      }
      const payload = this._symmetric.decryptAndHash(reader.rest());
      this._advance();
      return payload;
    });
  }

  /** Wipes every secret held by this handshake. */
  destroy(): void {
    this._symmetric.destroy();
    for (const pair of [this._s, this._e]) {
      if (pair !== null) {
        zeroize(pair.privateKey);
      }
    }
    if (this._fixedEphemeral !== null) {
      zeroize(this._fixedEphemeral);
    }
    this._s = null;
    this._e = null;
    this._fixedEphemeral = null;
  }

  private _checkUsable(): void {
    if (this._failed || this._result !== null) {
      throw new HandshakeError("wrong-state");
    }
  }

  private _guard<T>(step: () => T): T {
    try {
      return step();
    } catch (error) {
      this._fail();
      throw error;
    }
  }

  private _fail(): void {
    this._failed = true;
    this.destroy();
  }

  private _currentTokens(): readonly Token[] {
    const tokens = this._pattern.messages[this._index];
    if (tokens === undefined) {
      throw new HandshakeError("wrong-state");
    }
    return tokens;
  }

  private _requireS(): StaticKeypair {
    if (this._s === null) {
      throw new HandshakeError("wrong-state");
    }
    return this._s;
  }

  private _generateEphemeral(): StaticKeypair {
    let privateKey: Uint8Array;
    if (this._fixedEphemeral !== null) {
      privateKey = this._fixedEphemeral;
      this._fixedEphemeral = null;
    } else {
      privateKey = randomBytes(this._random, DH_LEN);
    }
    return { publicKey: publicKeyFromPrivate(privateKey), privateKey };
  }

  /**
   * DH tokens (Noise §5.3). The letters name the initiator's key first and
   * the responder's second: `es` is DH(e, rs) for the initiator and
   * DH(s, re) for the responder; `se` the reverse.
   */
  private _dhToken(token: "ee" | "es" | "se" | "ss"): void {
    const initiatorKey = token[0] === "e" ? "e" : "s";
    const responderKey = token[1] === "e" ? "e" : "s";
    const localKey = this._initiator ? initiatorKey : responderKey;
    const remoteKey = this._initiator ? responderKey : initiatorKey;
    const local = localKey === "e" ? this._e : this._s;
    const remote = remoteKey === "e" ? this._re : this._rs;
    if (local === null || remote === null) {
      throw new HandshakeError("wrong-state");
    }
    const shared = dh(local.privateKey, remote);
    this._symmetric.mixKey(shared);
    zeroize(shared);
  }

  private _advance(): void {
    this._index += 1;
    if (this._index < this._pattern.messages.length) {
      return;
    }
    const remoteStatic = this._rs;
    if (remoteStatic === null) {
      throw new HandshakeError("wrong-state");
    }
    const handshakeHash = this._symmetric.handshakeHash();
    const { c1, c2 } = this._symmetric.split();
    this._result = this._initiator
      ? { send: c1, receive: c2, handshakeHash, remoteStatic: remoteStatic.slice() }
      : { send: c2, receive: c1, handshakeHash, remoteStatic: remoteStatic.slice() };
    // Ephemeral and static private copies are no longer needed.
    for (const pair of [this._s, this._e]) {
      if (pair !== null) {
        zeroize(pair.privateKey);
      }
    }
    this._s = null;
    this._e = null;
  }
}

function concatAll(parts: readonly Uint8Array[]): Uint8Array {
  let length = 0;
  for (const part of parts) {
    length += part.length;
  }
  const out = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}
