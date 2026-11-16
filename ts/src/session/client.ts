// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The client side of a session as a sans-IO state machine (spec 01 §4 and
 * spec 02): the Porecatu Remote app talking to a host through the relay.
 *
 * {@link ClientSession} works with **WebSocket binary frames of the client
 * connection** (`/v0/client/<host_id>`): it takes the frames the relay sent and
 * gives back the frames to write, already wrapped in the `data` envelope. Layer
 * by layer, it covers the envelope, the Noise handshake (`XX` to pair, `IK` to
 * connect) and the encrypted records, and hands the application messages.
 *
 * # Outputs
 *
 * Everything that comes out of `receive` and `tick` is a {@link ClientEvent},
 * in order. Frames to write are `send` events, interleaved with the others, so
 * the order between "send this" and "this happened" is never lost. The
 * factories and `send` return frames only.
 *
 * # Errors
 *
 * A failure caused by the peer or by the network (bad record, host key
 * mismatch, deadline) is an event, `failed`, after which the session is
 * closed. A thrown {@link ClientError} is reserved for the caller's own misuse
 * (a call on a closed session, a message the client may not send) and for the
 * failures of `send`, which has no event list to carry them.
 *
 * The session **does not send `hello` by itself**: when `handshake-complete`
 * arrives, the application decides what comes first (spec 03 §2).
 */

import { CborError } from "../cbor/index.js";
import {
  decodeClientEnvelope,
  encodeClientEnvelope,
  encodeHandshakeStart,
  EnvelopeError,
  HandshakeStartKind,
  type CloseReason,
} from "../envelope.js";
import { decodeMessage, encodeMessage, messageSender } from "../message/index.js";
import { ErrorCode, type Platform } from "../message/codes.js";
import { encodePairRequest } from "../message/pairing.js";
import type { Message } from "../message/types.js";
import { PROTOCOL_MAJOR, type PairingInvite } from "../pairingUri.js";
import { HandshakeError } from "./errors.js";
import {
  ClientHandshake,
  HANDSHAKE_TIMEOUT_MS,
  type ClientStep,
} from "./handshake.js";
import { zeroize, type RandomSource, type StaticKeypair } from "./keys.js";
import { RecordError, Session } from "./record.js";

/** Why a {@link ClientError} was raised. */
export type ClientErrorCode =
  /** The invitation is for a protocol major version this code does not speak. */
  | "unsupported-major"
  /** The device name is empty or longer than 64 bytes. */
  | "pair-request"
  /** The handshake failed; `detail` is the `HandshakeErrorCode`. `host-key-mismatch` is the QR identity error: show it to the user; nothing was sent. */
  | "handshake"
  /** The relay sent a frame that is not a valid client envelope; `detail` is the `EnvelopeErrorReason`. */
  | "envelope"
  /** A record failed: the session is closed (there is no resend); `detail` is the `RecordErrorCode`. */
  | "record"
  /** The host sent a message that does not decode. The client answered `error { protocol }` and closed; `detail` is the `DecodeErrorReason`. */
  | "message"
  /** The handshake did not finish within {@link HANDSHAKE_TIMEOUT_MS}. */
  | "timeout"
  /** The session already ended. */
  | "closed"
  /** The handshake has not finished: there is no channel to send on yet. */
  | "not-open"
  /** The message is one the client must not send (spec 03 §10). */
  | "not-sendable";

/**
 * A failure of a {@link ClientSession}, as an event payload or as a thrown
 * value. Typed codes only: the message is the code, never prose or bytes.
 */
export class ClientError extends Error {
  readonly code: ClientErrorCode;
  /** The code of the lower layer that failed, when there is one. */
  readonly detail: string | undefined;

  constructor(code: ClientErrorCode, detail?: string) {
    super(detail === undefined ? code : `${code}: ${detail}`);
    this.name = "ClientError";
    this.code = code;
    this.detail = detail;
  }
}

/** What happened on a {@link ClientSession}, in the order it happened. */
export type ClientEvent =
  /** A frame to write on the WebSocket (a `data` envelope). */
  | { readonly type: "send"; readonly frame: Uint8Array }
  /**
   * The transport is open: after `IK` when `msg2` is read, after `XX` when
   * `msg3` is written. Messages can be sent from now on.
   */
  | { readonly type: "handshake-complete" }
  /** A message from the host. */
  | { readonly type: "message"; readonly message: Message }
  /** The relay has no host registered under that `host_id`. The session is closed. */
  | { readonly type: "host-unavailable" }
  /** The relay closed the channel. The session is closed. */
  | { readonly type: "closed"; readonly reason: CloseReason }
  /**
   * The session failed and is closed. After a message that does not decode, an
   * `error { protocol }` `send` event precedes this one.
   */
  | { readonly type: "failed"; readonly error: ClientError };

/** What the factories return: the session and the frames to write first. */
export interface ClientStart {
  readonly session: ClientSession;
  readonly frames: Uint8Array[];
}

/** Options of the {@link ClientSession} factories. */
export interface ClientSessionOptions {
  /** Source of the ephemeral key; defaults to `globalThis.crypto`. */
  readonly random?: RandomSource;
}

type Stage =
  | {
      readonly kind: "handshaking";
      readonly handshake: ClientHandshake;
      /** The encoded `pair-request`, for `XX`; a secret (it carries the token), wiped when used. */
      readonly pairRequest: Uint8Array | null;
    }
  | { readonly kind: "open"; readonly session: Session }
  | { readonly kind: "closed" };

const CLOSED: Stage = { kind: "closed" };

/** The client side of one channel to a host. */
export class ClientSession {
  private _stage: Stage;
  private readonly _deadlineMs: number;

  private constructor(stage: Stage, nowMs: number) {
    this._stage = stage;
    this._deadlineMs = nowMs + HANDSHAKE_TIMEOUT_MS;
  }

  /**
   * Starts a pairing (`XX`) from the invitation of a QR code.
   *
   * Returns the session and the frames to write: one `data` carrying
   * `handshake-start [0, msg1]`.
   *
   * Throws {@link ClientError}: `unsupported-major` for an invitation of
   * another major version, `pair-request` for a device name of 0 or more than
   * 64 bytes (see `clampDeviceName`) and `handshake` for setup failures.
   */
  static pair(
    invite: PairingInvite,
    device: StaticKeypair,
    deviceName: string,
    platform: Platform,
    nowMs: number,
    options: ClientSessionOptions = {},
  ): ClientStart {
    if (invite.major !== PROTOCOL_MAJOR) {
      throw new ClientError("unsupported-major");
    }
    let request: Uint8Array;
    try {
      request = encodePairRequest({ token: invite.token, device: deviceName, platform });
    } catch (error) {
      if (error instanceof RangeError || error instanceof CborError) {
        throw new ClientError("pair-request");
      }
      throw error;
    }
    try {
      const { handshake, msg1 } = ClientHandshake.pair({
        local: device,
        hostId: invite.hostId,
        expectedHostKey: invite.hostKey,
        ...(options.random === undefined ? {} : { random: options.random }),
      });
      const session = new ClientSession(
        { kind: "handshaking", handshake, pairRequest: request },
        nowMs,
      );
      return { session, frames: [startFrame(HandshakeStartKind.pair, msg1)] };
    } catch (error) {
      zeroize(request);
      throw asClientError(error);
    }
  }

  /**
   * Starts the connection (`IK`) of an already paired device.
   *
   * Returns the session and the frames to write: one `data` carrying
   * `handshake-start [1, msg1]`. Throws {@link ClientError} (`handshake`) when
   * `hostKey` is not a usable key.
   */
  static connect(
    hostId: string,
    hostKey: Uint8Array,
    device: StaticKeypair,
    nowMs: number,
    options: ClientSessionOptions = {},
  ): ClientStart {
    try {
      const { handshake, msg1 } = ClientHandshake.connect({
        local: device,
        hostId,
        hostKey,
        ...(options.random === undefined ? {} : { random: options.random }),
      });
      const session = new ClientSession(
        { kind: "handshaking", handshake, pairRequest: null },
        nowMs,
      );
      return { session, frames: [startFrame(HandshakeStartKind.connect, msg1)] };
    } catch (error) {
      throw asClientError(error);
    }
  }

  /** Whether the transport is open and messages can be sent. */
  get isOpen(): boolean {
    return this._stage.kind === "open";
  }

  /** Whether the handshake is still running. */
  get isHandshaking(): boolean {
    return this._stage.kind === "handshaking";
  }

  /** Whether the session ended (failure, close by the relay, or `host-unavailable`). */
  get isClosed(): boolean {
    return this._stage.kind === "closed";
  }

  /**
   * Takes one frame the relay sent on the client connection.
   *
   * Throws {@link ClientError} (`closed`) when the session already ended.
   * Every other failure comes as a `failed` event in the returned list.
   */
  receive(frame: Uint8Array, nowMs: number): ClientEvent[] {
    if (this.isClosed) {
      throw new ClientError("closed");
    }
    const events: ClientEvent[] = [];
    if (this._handshakeOverdue(nowMs)) {
      events.push(this._fail(new ClientError("timeout")));
      return events;
    }

    let envelope;
    try {
      envelope = decodeClientEnvelope(frame);
    } catch (error) {
      if (error instanceof EnvelopeError) {
        events.push(this._fail(new ClientError("envelope", error.reason)));
        return events;
      }
      throw error;
    }
    switch (envelope.type) {
      case "host-unavailable":
        this._close();
        events.push({ type: "host-unavailable" });
        break;
      case "closed":
        this._close();
        events.push({ type: "closed", reason: envelope.reason });
        break;
      case "data":
        this._onData(envelope.payload, nowMs, events);
        break;
    }
    return events;
  }

  /**
   * Seals `message` into the frames to write, in order.
   *
   * Throws {@link ClientError}: `not-open` before the handshake ends,
   * `closed` after the session ended, `not-sendable` for a message only the
   * host sends, and `record` when sealing fails (the session is closed then).
   */
  send(message: Message, nowMs: number): Uint8Array[] {
    const stage = this._stage;
    if (stage.kind === "closed") {
      throw new ClientError("closed");
    }
    if (stage.kind === "handshaking") {
      throw new ClientError("not-open");
    }
    const sender = messageSender(message.type);
    if (sender !== "both" && sender !== "client") {
      throw new ClientError("not-sendable");
    }
    try {
      return stage.session.seal(encodeMessage(message), nowMs).map(dataFrame);
    } catch (error) {
      if (error instanceof RecordError) {
        this._close();
        throw new ClientError("record", error.code);
      }
      throw error;
    }
  }

  /**
   * Advances the clock. The only deadline is the one of the handshake: when it
   * passes, the session fails with `timeout`.
   */
  tick(nowMs: number): ClientEvent[] {
    return this._handshakeOverdue(nowMs) ? [this._fail(new ClientError("timeout"))] : [];
  }

  /**
   * The next time `tick` has something to do: the handshake deadline while the
   * handshake runs, `null` afterwards.
   */
  nextDeadline(): number | null {
    return this.isHandshaking ? this._deadlineMs : null;
  }

  private _handshakeOverdue(nowMs: number): boolean {
    return this.isHandshaking && nowMs >= this._deadlineMs;
  }

  /** Ends the session, wiping the secret the handshake still held. */
  private _close(): void {
    const stage = this._stage;
    if (stage.kind === "handshaking" && stage.pairRequest !== null) {
      zeroize(stage.pairRequest);
    }
    if (stage.kind === "open") {
      stage.session.destroy();
    }
    this._stage = CLOSED;
  }

  /**
   * Ends the session from the application side (the user leaves the screen):
   * zeroes the transport keys and the pending pairing secret. Idempotent; the
   * session is closed afterwards and produces no event.
   */
  close(): void {
    this._close();
  }

  private _fail(error: ClientError): ClientEvent {
    this._close();
    return { type: "failed", error };
  }

  private _onData(record: Uint8Array, nowMs: number, events: ClientEvent[]): void {
    const stage = this._stage;
    // Every path below that does not restore the stage leaves it closed.
    this._stage = CLOSED;
    switch (stage.kind) {
      case "closed":
        return;
      case "handshaking":
        this._onHandshakeData(stage, record, nowMs, events);
        return;
      case "open":
        this._onRecord(stage.session, record, nowMs, events);
        return;
    }
  }

  private _onRecord(
    session: Session,
    record: Uint8Array,
    nowMs: number,
    events: ClientEvent[],
  ): void {
    let bytes: Uint8Array | null;
    try {
      bytes = session.open(record, nowMs);
    } catch (error) {
      if (error instanceof RecordError) {
        events.push({ type: "failed", error: new ClientError("record", error.code) });
        return;
      }
      throw error;
    }
    if (bytes === null) {
      this._stage = { kind: "open", session };
      return;
    }
    const decoded = decodeMessage(bytes, "host");
    if (decoded.ok) {
      events.push({ type: "message", message: decoded.message });
      this._stage = { kind: "open", session };
      return;
    }
    // Spec 03 §1: who detects the error answers `error { protocol }` and
    // closes the session.
    sealProtocolError(session, nowMs, (frame) => events.push({ type: "send", frame: dataFrame(frame) }));
    session.destroy();
    events.push({ type: "failed", error: new ClientError("message", decoded.error.reason) });
  }

  private _onHandshakeData(
    stage: Extract<Stage, { kind: "handshaking" }>,
    msg2: Uint8Array,
    nowMs: number,
    events: ClientEvent[],
  ): void {
    const { handshake, pairRequest } = stage;
    // Any error below leaves the stage closed: the client sends nothing more
    // (spec 02 §2).
    try {
      let step: ClientStep;
      try {
        step = handshake.readMessage2(msg2);
      } catch (error) {
        events.push({ type: "failed", error: asClientError(error) });
        return;
      }
      if (step.kind === "established") {
        this._stage = { kind: "open", session: new Session(step.transport, { nowMs }) };
        events.push({ type: "handshake-complete" });
        return;
      }
      if (pairRequest === null) {
        events.push({ type: "failed", error: new ClientError("handshake", "wrong-state") });
        return;
      }
      try {
        const { msg3, transport } = handshake.writeMessage3(pairRequest);
        events.push({ type: "send", frame: dataFrame(msg3) });
        this._stage = { kind: "open", session: new Session(transport, { nowMs }) };
        events.push({ type: "handshake-complete" });
      } catch (error) {
        events.push({ type: "failed", error: asClientError(error) });
      }
    } finally {
      if (pairRequest !== null) {
        zeroize(pairRequest);
      }
    }
  }
}

/** Maps a handshake failure to a {@link ClientError}; anything else is a bug and propagates. */
function asClientError(error: unknown): ClientError {
  if (error instanceof ClientError) {
    return error;
  }
  if (error instanceof HandshakeError) {
    return new ClientError("handshake", error.code);
  }
  throw error;
}

/** Seals `error { protocol }`; a sealing failure is ignored, the session is closing anyway. */
function sealProtocolError(
  session: Session,
  nowMs: number,
  emit: (record: Uint8Array) => void,
): void {
  const reply: Message = { type: "error", code: ErrorCode.protocol };
  try {
    for (const record of session.seal(encodeMessage(reply), nowMs)) {
      emit(record);
    }
  } catch (error) {
    if (!(error instanceof RecordError)) {
      throw error;
    }
  }
}

/** The `data` frame carrying a Noise message or record. */
function dataFrame(payload: Uint8Array): Uint8Array {
  return encodeClientEnvelope({ type: "data", payload });
}

/** The first frame of a channel: `data` carrying `handshake-start`. */
function startFrame(kind: HandshakeStartKind, msg1: Uint8Array): Uint8Array {
  return dataFrame(encodeHandshakeStart({ kind, message1: msg1 }));
}
