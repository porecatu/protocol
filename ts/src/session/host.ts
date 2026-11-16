// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The host side of one channel as a sans-IO state machine (spec 02): the
 * Porecatu desktop talking to one client through the relay.
 *
 * The host multiplexes many channels on one WebSocket and builds the
 * `channel-*` envelopes itself, so {@link HostChannel} works with
 * **payloads**: the bytes inside a `channel-data` envelope, in both
 * directions. One `HostChannel` per channel, created at the `channel-opened`.
 *
 * # Outputs
 *
 * Everything that comes out of `receive` and `tick` is a {@link HostEvent}, in
 * order. Payloads to send are `send` events, interleaved with the others, so
 * the order between "send this" and "this happened" is never lost. `send` and
 * `rejectNotPaired` return payloads only.
 *
 * # Errors
 *
 * A failure caused by the peer, the network or the clock is an event,
 * `failed`, after which the channel is closed and the caller sends
 * `close-channel` with {@link HostError.closeReason}. A thrown
 * {@link HostError} is reserved for the caller's own misuse (a call on a closed
 * channel, a message the host may not send) and for the failures of the calls
 * that return payloads only.
 *
 * # Who decides what
 *
 * This class knows nothing about the list of paired devices or the pairing
 * token. On `IK` it completes the handshake always and reports the device key
 * (`device-connecting`); the caller looks the key up and, for an unknown
 * device, calls `rejectNotPaired` (spec 02 §4). On `XX` it reports the request
 * (`pair-requested`); the caller checks the token with `PairingToken`, asks the
 * user, and answers with `pair-accepted` or `pair-rejected` through `send`.
 * Messages the client sends before the caller decided are delivered as they
 * come; deciding first is the caller's job.
 */

import { CloseReason, decodeHandshakeStart, EnvelopeError } from "../envelope.js";
import { decodeMessage, encodeMessage, messageSender } from "../message/index.js";
import { ErrorCode } from "../message/codes.js";
import { DecodeError } from "../message/codec.js";
import { decodePairRequest } from "../message/pairing.js";
import type { Message, PairRequest } from "../message/types.js";
import { HandshakeError } from "./errors.js";
import { HANDSHAKE_TIMEOUT_MS, HandshakeKind, HostHandshake } from "./handshake.js";
import { zeroize, type RandomSource, type StaticKeypair } from "./keys.js";
import { RecordError, Session } from "./record.js";

/** Why a {@link HostError} was raised. */
export type HostErrorCode =
  /**
   * The handshake failed: a Noise message that does not decrypt (this includes
   * a prologue of another major), a null DH result, a non-empty payload in
   * `msg1`; `detail` is the `HandshakeErrorCode`.
   */
  | "handshake"
  /** The first payload is not a valid `handshake-start`; `detail` is the `EnvelopeErrorReason`. */
  | "handshake-start"
  /** A record failed: the channel is closed (there is no resend); `detail` is the `RecordErrorCode`. */
  | "record"
  /** The client sent a message that does not decode (or one only the host may send). The host answered `error { protocol }`; `detail` is the `DecodeErrorReason`. */
  | "message"
  /** The `pair-request` of `msg3` does not decode. The host answered `error { protocol }`; `detail` is the `DecodeErrorReason`. */
  | "pair-request"
  /** The handshake did not finish within {@link HANDSHAKE_TIMEOUT_MS}. */
  | "timeout"
  /** The channel already ended. */
  | "closed"
  /** The handshake has not finished: there is no transport to send on yet. */
  | "not-open"
  /** The message is one the host must not send (spec 03 §10). */
  | "not-sendable";

/**
 * A failure of a {@link HostChannel}, as an event payload or as a thrown
 * value. Typed codes only: the message is the code, never prose or bytes.
 */
export class HostError extends Error {
  readonly code: HostErrorCode;
  /** The code of the lower layer that failed, when there is one. */
  readonly detail: string | undefined;

  constructor(code: HostErrorCode, detail?: string) {
    super(detail === undefined ? code : `${code}: ${detail}`);
    this.name = "HostError";
    this.code = code;
    this.detail = detail;
  }

  /**
   * The `close-reason` for the `close-channel` the caller sends after a
   * `failed` event: `host-rejected` for every failure of the handshake (spec 02
   * §2: no application message), `normal` for failures of an already open
   * session (the `error { protocol }` went first, when there was one).
   */
  get closeReason(): CloseReason {
    switch (this.code) {
      case "handshake":
      case "handshake-start":
      case "timeout":
        return CloseReason.hostRejected;
      default:
        return CloseReason.normal;
    }
  }
}

/** What happened on a {@link HostChannel}, in the order it happened. */
export type HostEvent =
  /** A payload to put in a `channel-data` envelope for this channel. */
  | { readonly type: "send"; readonly payload: Uint8Array }
  /**
   * `IK` completed: a device with this static key connects. The caller checks
   * the list of paired devices and, for an unknown key, calls `rejectNotPaired`.
   */
  | { readonly type: "device-connecting"; readonly deviceKey: Uint8Array }
  /**
   * `XX` completed: a device asks to pair. The caller checks the token, asks
   * the user and answers with `pair-accepted` or `pair-rejected`.
   */
  | {
      readonly type: "pair-requested";
      readonly deviceKey: Uint8Array;
      readonly request: PairRequest;
    }
  /** A message from the client. */
  | { readonly type: "message"; readonly message: Message }
  /**
   * The channel failed and is closed. After a message that does not decode, an
   * `error { protocol }` `send` event precedes this one. The caller closes the
   * channel with {@link HostError.closeReason}.
   */
  | { readonly type: "failed"; readonly error: HostError };

type Stage =
  /** Waiting for the `handshake-start`. Holds a copy of the host key until it comes. */
  | { readonly kind: "await-start"; readonly key: StaticKeypair }
  /** `XX` answered; waiting for `msg3`. */
  | { readonly kind: "await-msg3"; readonly handshake: HostHandshake }
  | { readonly kind: "open"; readonly session: Session }
  | { readonly kind: "closed" };

const CLOSED: Stage = { kind: "closed" };

/** Options of the {@link HostChannel} constructor. */
export interface HostChannelOptions {
  /** Source of the ephemeral key; defaults to `globalThis.crypto`. */
  readonly random?: RandomSource;
}

/** The host side of one channel. */
export class HostChannel {
  private _stage: Stage;
  /** The `host_id`, for the Noise prologue. */
  private readonly _hostId: string;
  private readonly _deadlineMs: number;
  private readonly _random: RandomSource | undefined;

  /**
   * Starts the state machine of a channel that was just opened
   * (`channel-opened`). The 15 s handshake deadline counts from `nowMs`.
   *
   * The channel keeps a copy of the host key until the first payload arrives,
   * and wipes it right after.
   */
  constructor(
    hostStatic: StaticKeypair,
    hostId: string,
    nowMs: number,
    options: HostChannelOptions = {},
  ) {
    this._stage = {
      kind: "await-start",
      key: { publicKey: hostStatic.publicKey.slice(), privateKey: hostStatic.privateKey.slice() },
    };
    this._hostId = hostId;
    this._deadlineMs = nowMs + HANDSHAKE_TIMEOUT_MS;
    this._random = options.random;
  }

  /** Whether the transport is open and messages can be sent. */
  get isOpen(): boolean {
    return this._stage.kind === "open";
  }

  /** Whether the handshake is still running. */
  get isHandshaking(): boolean {
    return this._stage.kind === "await-start" || this._stage.kind === "await-msg3";
  }

  /** Whether the channel ended. */
  get isClosed(): boolean {
    return this._stage.kind === "closed";
  }

  /**
   * Takes one payload the client sent on the channel.
   *
   * Throws {@link HostError} (`closed`) when the channel already ended. Every
   * other failure comes as a `failed` event in the returned list.
   */
  receive(payload: Uint8Array, nowMs: number): HostEvent[] {
    if (this.isClosed) {
      throw new HostError("closed");
    }
    const events: HostEvent[] = [];
    if (this._handshakeOverdue(nowMs)) {
      events.push(this._fail(new HostError("timeout")));
      return events;
    }
    const stage = this._stage;
    // Every path below that does not restore the stage leaves it closed.
    this._stage = CLOSED;
    switch (stage.kind) {
      case "closed":
        break;
      case "await-start":
        this._onStart(stage.key, payload, nowMs, events);
        break;
      case "await-msg3":
        this._onMsg3(stage.handshake, payload, nowMs, events);
        break;
      case "open":
        this._onRecord(stage.session, payload, nowMs, events);
        break;
    }
    return events;
  }

  /**
   * Seals `message` into the payloads to send, in order.
   *
   * Throws {@link HostError}: `not-open` before the handshake ends, `closed`
   * after the channel ended, `not-sendable` for a message only the client
   * sends, and `record` when sealing fails (the channel is closed then).
   */
  send(message: Message, nowMs: number): Uint8Array[] {
    const stage = this._stage;
    if (stage.kind === "closed") {
      throw new HostError("closed");
    }
    if (stage.kind !== "open") {
      throw new HostError("not-open");
    }
    const sender = messageSender(message.type);
    if (sender !== "both" && sender !== "host") {
      throw new HostError("not-sendable");
    }
    try {
      return stage.session.seal(encodeMessage(message), nowMs);
    } catch (error) {
      if (error instanceof RecordError) {
        this.close();
        throw new HostError("record", error.code);
      }
      throw error;
    }
  }

  /**
   * Answers an `IK` of a device that is not paired (spec 02 §4): the first
   * application message is `error { not-paired }`. Returns its payloads and
   * closes the channel; the caller then sends `close-channel`.
   *
   * Throws {@link HostError}: `not-open` before the handshake ends, `closed`
   * after the channel ended, and `record` when sealing fails.
   */
  rejectNotPaired(nowMs: number): Uint8Array[] {
    const payloads = this.send({ type: "error", code: ErrorCode.notPaired }, nowMs);
    this.close();
    return payloads;
  }

  /**
   * Ends the channel from the caller's side (`channel-closed` from the relay,
   * or the host shutting down): zeroes the transport keys and the host key copy
   * when still held. Idempotent; the channel is closed afterwards and produces
   * no event.
   */
  close(): void {
    const stage = this._stage;
    if (stage.kind === "await-start") {
      wipe(stage.key);
    } else if (stage.kind === "open") {
      stage.session.destroy();
    }
    this._stage = CLOSED;
  }

  /**
   * Advances the clock. The only deadline is the one of the handshake: when it
   * passes, the channel fails with `timeout` and the caller closes it with
   * `host-rejected`.
   */
  tick(nowMs: number): HostEvent[] {
    return this._handshakeOverdue(nowMs) ? [this._fail(new HostError("timeout"))] : [];
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

  /** Ends the channel, wiping the host key copy when it is still held. */
  private _fail(error: HostError): HostEvent {
    const stage = this._stage;
    if (stage.kind === "await-start") {
      wipe(stage.key);
    }
    this._stage = CLOSED;
    return { type: "failed", error };
  }

  private _onStart(
    hostKey: StaticKeypair,
    payload: Uint8Array,
    nowMs: number,
    events: HostEvent[],
  ): void {
    try {
      let start;
      try {
        start = decodeHandshakeStart(payload);
      } catch (error) {
        if (error instanceof EnvelopeError) {
          events.push({ type: "failed", error: new HostError("handshake-start", error.reason) });
          return;
        }
        throw error;
      }
      let result;
      try {
        result = HostHandshake.respond({
          kind: start.kind,
          local: hostKey,
          hostId: this._hostId,
          msg1: start.message1,
          ...(this._random === undefined ? {} : { random: this._random }),
        });
      } catch (error) {
        if (error instanceof HandshakeError) {
          events.push({ type: "failed", error: new HostError("handshake", error.code) });
          return;
        }
        throw error;
      }
      events.push({ type: "send", payload: result.msg2 });
      if (result.kind === HandshakeKind.pair) {
        this._stage = { kind: "await-msg3", handshake: result.handshake };
        return;
      }
      events.push({ type: "device-connecting", deviceKey: result.remoteStatic });
      this._stage = { kind: "open", session: new Session(result.transport, { nowMs }) };
    } finally {
      wipe(hostKey);
    }
  }

  private _onMsg3(
    pending: HostHandshake,
    payload: Uint8Array,
    nowMs: number,
    events: HostEvent[],
  ): void {
    let parts;
    try {
      parts = pending.readMessage3(payload);
    } catch (error) {
      if (error instanceof HandshakeError) {
        events.push({ type: "failed", error: new HostError("handshake", error.code) });
        return;
      }
      throw error;
    }
    const session = new Session(parts.transport, { nowMs });
    try {
      const request = decodePairRequest(parts.payload);
      events.push({ type: "pair-requested", deviceKey: parts.remoteStatic, request });
      this._stage = { kind: "open", session };
    } catch (error) {
      if (!(error instanceof DecodeError)) {
        throw error;
      }
      sealProtocolError(session, nowMs, events);
      session.destroy();
      events.push({ type: "failed", error: new HostError("pair-request", error.reason) });
    } finally {
      // The payload carried the pairing token.
      zeroize(parts.payload);
    }
  }

  private _onRecord(
    session: Session,
    record: Uint8Array,
    nowMs: number,
    events: HostEvent[],
  ): void {
    let bytes: Uint8Array | null;
    try {
      bytes = session.open(record, nowMs);
    } catch (error) {
      if (error instanceof RecordError) {
        events.push({ type: "failed", error: new HostError("record", error.code) });
        return;
      }
      throw error;
    }
    if (bytes === null) {
      this._stage = { kind: "open", session };
      return;
    }
    const decoded = decodeMessage(bytes, "client");
    if (decoded.ok) {
      events.push({ type: "message", message: decoded.message });
      this._stage = { kind: "open", session };
      return;
    }
    sealProtocolError(session, nowMs, events);
    session.destroy();
    events.push({ type: "failed", error: new HostError("message", decoded.error.reason) });
  }
}

function wipe(key: StaticKeypair): void {
  zeroize(key.privateKey);
  zeroize(key.publicKey);
}

/**
 * Spec 03 §1: who detects a protocol error answers `error { protocol }` before
 * closing. A sealing failure is ignored: the channel is closing anyway.
 */
function sealProtocolError(session: Session, nowMs: number, events: HostEvent[]): void {
  const reply: Message = { type: "error", code: ErrorCode.protocol };
  try {
    for (const payload of session.seal(encodeMessage(reply), nowMs)) {
      events.push({ type: "send", payload });
    }
  } catch (error) {
    if (!(error instanceof RecordError)) {
      throw error;
    }
  }
}
