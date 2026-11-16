// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The two handshakes of the remote access protocol (spec 02 §2, §3.2, §4) and
 * the transport state that follows them:
 *
 * - pairing: `Noise_XX_25519_ChaChaPoly_BLAKE2s`, the client checks the host
 *   static key against the QR code before sending `msg3`;
 * - connection of a paired device: `Noise_IK_25519_ChaChaPoly_BLAKE2s`, one
 *   round trip, and the host always completes it.
 *
 * Sans-IO: bytes in, bytes out. No CBOR here: `msg1` goes inside the
 * `handshake-start` item built by the caller, and the `msg3` payload
 * (`pair-request`) is opaque bytes for this layer.
 */

import { ctEqual } from "../ct.js";
import type { CipherState } from "./cipherState.js";
import { HandshakeError } from "./errors.js";
import {
  HandshakeState,
  MAX_NOISE_MESSAGE as NOISE_MESSAGE_LIMIT,
  PATTERN_IK,
  PATTERN_XX,
  type HandshakeResult,
} from "./handshakeState.js";
import { DH_LEN, defaultRandom, type RandomSource, type StaticKeypair } from "./keys.js";

export { HandshakeError, type HandshakeErrorCode } from "./errors.js";

/** Noise protocol of the pairing handshake (spec 02 §2). */
export const NOISE_XX = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/** Noise protocol of the connection of a paired device (spec 02 §2). */
export const NOISE_IK = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

/** Prologue prefix: protocol name and major version, followed by a space (spec 02 §2). */
export const PROLOGUE_PREFIX = "porecatu-remote/0 ";

/**
 * How long a handshake may take: 15 seconds (spec 02 §2). The host counts from
 * the `channel-opened`, the client from the creation of its session.
 */
export const HANDSHAKE_TIMEOUT_MS = 15_000;

/** Largest Noise message: 65 535 bytes (spec 02 §5). */
export const MAX_NOISE_MESSAGE = NOISE_MESSAGE_LIMIT;
/** Largest transport plaintext: 65 535 minus the 16-byte tag (spec 02 §5). */
export const MAX_PLAINTEXT = 65_519;

/** Which handshake follows, as carried by `handshake-start` (spec 02 §2). */
export const HandshakeKind = { pair: 0, connect: 1 } as const;
export type HandshakeKind = (typeof HandshakeKind)[keyof typeof HandshakeKind];
/** All {@link HandshakeKind} values. */
export const HANDSHAKE_KIND_VALUES: readonly HandshakeKind[] = [
  HandshakeKind.pair,
  HandshakeKind.connect,
];

/**
 * Prologue of both handshakes: the ASCII bytes of `"porecatu-remote/0 "`
 * followed by the `host_id` (spec 02 §2). A non-ASCII `hostId` is refused
 * with `malformed`.
 */
export function prologue(hostId: string): Uint8Array {
  const text = PROLOGUE_PREFIX + hostId;
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

/**
 * Transport state after a handshake (Noise §5.1 `CipherState` pair, already
 * oriented): one cipher for sending, one for receiving. Records, flags and
 * the rekey schedule (spec 02 §5) belong to the layer above.
 *
 * A failed `decrypt` leaves the receive nonce unchanged (Noise §5.1); the
 * spec requires the caller to close the session (spec 02 §5).
 */
export class Transport {
  private readonly _send: CipherState;
  private readonly _receive: CipherState;
  private readonly _remoteStatic: Uint8Array;
  private readonly _handshakeHash: Uint8Array;

  /** @internal Built by the handshakes; not part of the public API. */
  constructor(result: HandshakeResult) {
    this._send = result.send;
    this._receive = result.receive;
    this._remoteStatic = result.remoteStatic.slice();
    this._handshakeHash = result.handshakeHash.slice();
  }

  /** The peer's static public key, authenticated by the handshake. */
  get remoteStatic(): Uint8Array {
    return this._remoteStatic.slice();
  }

  /** The final handshake hash `h` (Noise §11.2, channel binding). */
  get handshakeHash(): Uint8Array {
    return this._handshakeHash.slice();
  }

  /** Encrypts one transport message; plaintext of at most {@link MAX_PLAINTEXT} bytes. */
  encrypt(plaintext: Uint8Array): Uint8Array {
    if (plaintext.length > MAX_PLAINTEXT) {
      throw new HandshakeError("malformed");
    }
    return this._send.encryptWithAd(new Uint8Array(0), plaintext);
  }

  /** Decrypts one transport message; throws `decrypt` on authentication failure. */
  decrypt(ciphertext: Uint8Array): Uint8Array {
    if (ciphertext.length > MAX_NOISE_MESSAGE) {
      throw new HandshakeError("malformed");
    }
    return this._receive.decryptWithAd(new Uint8Array(0), ciphertext);
  }

  /** `Rekey()` on the sending cipher (Noise §11.3). */
  rekeyOutgoing(): void {
    this._send.rekey();
  }

  /** `Rekey()` on the receiving cipher (Noise §11.3). */
  rekeyIncoming(): void {
    this._receive.rekey();
  }

  /** Overwrites both keys; the transport is unusable afterwards. */
  destroy(): void {
    this._send.destroy();
    this._receive.destroy();
  }
}

/** Options shared by the client constructors. */
interface ClientOptionsBase {
  /** The device static key pair. */
  local: StaticKeypair;
  /** `host_id` the client reached the host by; goes into the prologue. */
  hostId: string;
  /** Source of the ephemeral key; defaults to {@link defaultRandom}. */
  random?: RandomSource;
  /**
   * Fixed ephemeral private key. **Only for tests and test vectors**: a
   * reused ephemeral key destroys forward secrecy.
   */
  ephemeral?: Uint8Array;
}

/** Options of {@link ClientHandshake.pair}. */
export interface ClientPairOptions extends ClientOptionsBase {
  /** Host static public key read from the QR code (spec 02 §3.1). */
  expectedHostKey: Uint8Array;
}

/** Options of {@link ClientHandshake.connect}. */
export interface ClientConnectOptions extends ClientOptionsBase {
  /** Host static public key stored at pairing time. */
  hostKey: Uint8Array;
}

/** Result of {@link ClientHandshake.readMessage2}. */
export type ClientStep =
  | { kind: "established"; transport: Transport }
  | { kind: "needs-pair-request" };

const EMPTY = new Uint8Array(0);

function checkKey(key: Uint8Array): void {
  if (key.length !== DH_LEN) {
    throw new HandshakeError("malformed");
  }
}

/** Handshake payloads that the spec fixes as empty (spec 02 §3.2, §4). */
function requireEmpty(payload: Uint8Array): void {
  if (payload.length !== 0) {
    throw new HandshakeError("malformed");
  }
}

function stateOptions(
  base: ClientOptionsBase | HostRespondOptions,
): { random: RandomSource; ephemeral?: Uint8Array } {
  const random = base.random ?? defaultRandom;
  return base.ephemeral === undefined ? { random } : { random, ephemeral: base.ephemeral };
}

/** Client (initiator) side of both handshakes. */
export class ClientHandshake {
  private readonly _state: HandshakeState;
  private readonly _kind: HandshakeKind;
  private readonly _expectedHostKey: Uint8Array | null;
  private _stage: "await-msg2" | "await-msg3" | "done" | "failed" = "await-msg2";

  private constructor(state: HandshakeState, kind: HandshakeKind, expectedHostKey: Uint8Array | null) {
    this._state = state;
    this._kind = kind;
    this._expectedHostKey = expectedHostKey;
  }

  /** Which handshake this is. */
  get kind(): HandshakeKind {
    return this._kind;
  }

  /** Pairing (`XX`): returns the handshake and `msg1` (`-> e`, empty payload). */
  static pair(options: ClientPairOptions): { handshake: ClientHandshake; msg1: Uint8Array } {
    checkKey(options.expectedHostKey);
    const state = new HandshakeState({
      protocolName: NOISE_XX,
      pattern: PATTERN_XX,
      initiator: true,
      prologue: prologue(options.hostId),
      s: options.local,
      ...stateOptions(options),
    });
    const msg1 = state.writeMessage(EMPTY);
    const handshake = new ClientHandshake(state, HandshakeKind.pair, options.expectedHostKey.slice());
    return { handshake, msg1 };
  }

  /** Connection (`IK`): returns the handshake and `msg1` (`-> e, es, s, ss`, empty payload). */
  static connect(options: ClientConnectOptions): { handshake: ClientHandshake; msg1: Uint8Array } {
    checkKey(options.hostKey);
    const state = new HandshakeState({
      protocolName: NOISE_IK,
      pattern: PATTERN_IK,
      initiator: true,
      prologue: prologue(options.hostId),
      s: options.local,
      rs: options.hostKey,
      ...stateOptions(options),
    });
    const msg1 = state.writeMessage(EMPTY);
    return { handshake: new ClientHandshake(state, HandshakeKind.connect, null), msg1 };
  }

  /**
   * Reads the host's `msg2`. In `IK` the handshake completes. In `XX` the
   * revealed host static key is compared, in constant time, with the key
   * from the QR code; a difference fails with `host-key-mismatch` and the
   * handshake is abandoned without `msg3` (spec 02 §3.2).
   */
  readMessage2(msg2: Uint8Array): ClientStep {
    if (this._stage !== "await-msg2") {
      throw new HandshakeError("wrong-state");
    }
    try {
      requireEmpty(this._state.readMessage(msg2));
      if (this._kind === HandshakeKind.connect) {
        this._stage = "done";
        return { kind: "established", transport: new Transport(this._state.result()) };
      }
      const revealed = this._state.remoteStatic;
      const expected = this._expectedHostKey;
      if (revealed === null || expected === null || !ctEqual(revealed, expected)) {
        throw new HandshakeError("host-key-mismatch");
      }
      this._stage = "await-msg3";
      return { kind: "needs-pair-request" };
    } catch (error) {
      this._abort();
      throw error;
    }
  }

  /** Pairing only: writes `msg3` (`-> s, se`) carrying the `pair-request` and completes. */
  writeMessage3(pairRequestPayload: Uint8Array): { msg3: Uint8Array; transport: Transport } {
    if (this._stage !== "await-msg3") {
      throw new HandshakeError("wrong-state");
    }
    try {
      const msg3 = this._state.writeMessage(pairRequestPayload);
      this._stage = "done";
      return { msg3, transport: new Transport(this._state.result()) };
    } catch (error) {
      this._abort();
      throw error;
    }
  }

  private _abort(): void {
    this._stage = "failed";
    this._state.destroy();
  }
}

/** Options of {@link HostHandshake.respond}. */
export interface HostRespondOptions {
  /** Handshake kind from `handshake-start`; anything else than 0/1 is `malformed`. */
  kind: number;
  /** The host static key pair. */
  local: StaticKeypair;
  /** This host's `host_id`; goes into the prologue. */
  hostId: string;
  /** `noise-message-1` from `handshake-start`. */
  msg1: Uint8Array;
  /** Source of the ephemeral key; defaults to {@link defaultRandom}. */
  random?: RandomSource;
  /** Fixed ephemeral private key. **Only for tests and test vectors.** */
  ephemeral?: Uint8Array;
}

/** `IK`: the handshake is complete; the caller decides whether the device is paired. */
export interface HostConnectResult {
  kind: typeof HandshakeKind.connect;
  msg2: Uint8Array;
  transport: Transport;
  remoteStatic: Uint8Array;
}

/** `XX`: `msg2` is ready and the handshake waits for `msg3`. */
export interface HostPairResult {
  kind: typeof HandshakeKind.pair;
  msg2: Uint8Array;
  handshake: HostHandshake;
}

/** Host (responder) side of both handshakes. */
export class HostHandshake {
  private readonly _state: HandshakeState;
  private _stage: "await-msg3" | "done" | "failed" = "await-msg3";

  private constructor(state: HandshakeState) {
    this._state = state;
  }

  /**
   * Reads `msg1` and writes `msg2` (both payloads empty).
   *
   * - `connect` (`IK`): always completes (spec 02 §4) and returns the
   *   transport with the device static key.
   * - `pair` (`XX`): returns the handshake that reads `msg3`.
   */
  static respond(options: HostRespondOptions & { kind: typeof HandshakeKind.connect }): HostConnectResult;
  static respond(options: HostRespondOptions & { kind: typeof HandshakeKind.pair }): HostPairResult;
  static respond(options: HostRespondOptions): HostConnectResult | HostPairResult;
  static respond(options: HostRespondOptions): HostConnectResult | HostPairResult {
    if (options.kind === HandshakeKind.connect) {
      const state = new HandshakeState({
        protocolName: NOISE_IK,
        pattern: PATTERN_IK,
        initiator: false,
        prologue: prologue(options.hostId),
        s: options.local,
        ...stateOptions(options),
      });
      try {
        requireEmpty(state.readMessage(options.msg1));
        const msg2 = state.writeMessage(EMPTY);
        const transport = new Transport(state.result());
        return {
          kind: HandshakeKind.connect,
          msg2,
          transport,
          remoteStatic: transport.remoteStatic,
        };
      } catch (error) {
        state.destroy();
        throw error;
      }
    }
    if (options.kind === HandshakeKind.pair) {
      const state = new HandshakeState({
        protocolName: NOISE_XX,
        pattern: PATTERN_XX,
        initiator: false,
        prologue: prologue(options.hostId),
        s: options.local,
        ...stateOptions(options),
      });
      try {
        requireEmpty(state.readMessage(options.msg1));
        const msg2 = state.writeMessage(EMPTY);
        return { kind: HandshakeKind.pair, msg2, handshake: new HostHandshake(state) };
      } catch (error) {
        state.destroy();
        throw error;
      }
    }
    throw new HandshakeError("malformed");
  }

  /** Pairing only: reads `msg3` and returns its payload (`pair-request`), the device key and the transport. */
  readMessage3(msg3: Uint8Array): { payload: Uint8Array; remoteStatic: Uint8Array; transport: Transport } {
    if (this._stage !== "await-msg3") {
      throw new HandshakeError("wrong-state");
    }
    try {
      const payload = this._state.readMessage(msg3);
      const transport = new Transport(this._state.result());
      this._stage = "done";
      return { payload, remoteStatic: transport.remoteStatic, transport };
    } catch (error) {
      this._stage = "failed";
      this._state.destroy();
      throw error;
    }
  }
}
