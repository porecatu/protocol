// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Relay envelope (spec 01 §2-§6) and the `handshake-start` item (spec 02 §2).
 *
 * Both families are CBOR arrays whose first element is the type: the host
 * family (types 0-6, host <-> relay) and the client family (types 16-18,
 * client <-> relay). Every frame is exactly one item in deterministic form;
 * decoding goes through the strict CBOR decoder, so non-canonical frames,
 * trailing bytes and the like are refused before the shape is looked at.
 *
 * Both roles are implemented in both families so the two sides can be tested
 * against each other. No prose crosses the wire or the API: errors carry a
 * literal reason and never any payload bytes.
 */

import { CborError, type CborErrorReason, decodeCbor, encodeCbor } from "./cbor/index.js";
import { parseHostId } from "./hostId.js";

/** Version segment of the relay paths (`/v0/host`, `/v0/client/<host_id>`). */
export const PATH_VERSION = "v0";

/** Largest WebSocket frame either side may send, in bytes (spec 01 §1, §6). */
export const MAX_FRAME = 70_000;

/** Floor on simultaneous channels per host a conforming relay supports (spec 01 §6). */
export const MIN_CHANNELS_PER_HOST = 8;

/** Floor on sustained bandwidth per channel and direction, in bit/s (spec 01 §6). */
export const MIN_CHANNEL_BANDWIDTH_BPS = 2_000_000;

/** Time a host has, from the upgrade, to finish registering (spec 01 §3). */
export const REGISTER_TIMEOUT_MS = 10_000;

/** Interval between the relay's WebSocket pings (spec 01 §1). */
export const PING_INTERVAL_MS = 25_000;

/** Time without a pong after which the relay drops the connection (spec 01 §1). */
export const PONG_TIMEOUT_MS = 60_000;

/** Length of the registration challenge nonce, in bytes. */
export const NONCE_LEN = 32;

/** Length of the Ed25519 routing public key, in bytes. */
export const ROUTING_KEY_LEN = 32;

/** Length of the Ed25519 registration signature, in bytes. */
export const SIGNATURE_LEN = 64;

/** Domain-separation context of the registration signature (spec 01 §3). */
export const REGISTER_CONTEXT = "porecatu-relay/v0/register";

/** WebSocket close codes the protocol uses (spec 01 §4, §5). */
export const CLOSE_CODE = {
  /** Normal closure. */
  normal: 1000,
  /** The relay is shutting down. */
  goingAway: 1001,
  /** A text frame arrived; only binary frames are used. */
  unsupportedData: 1003,
  /** Frame above `MAX_FRAME`. */
  messageTooBig: 1009,
  /** Host registration refused (invalid signature). */
  registrationRejected: 4001,
  /** Replaced by a newer registration of the same `host_id`. */
  replaced: 4002,
  /** Relay limit exceeded. */
  limitExceeded: 4003,
  /** Invalid envelope. */
  invalidEnvelope: 4004,
  /** Host registration not completed in time. */
  registrationTimeout: 4005,
} as const;

/** Why a channel was closed (`close-reason`, spec 01 §4). */
export const CloseReason = {
  /** The other side closed. */
  normal: 0,
  /** The host registered again. */
  hostReplaced: 1,
  /** The host's connection dropped. */
  hostGone: 2,
  /** The host refused the channel. */
  hostRejected: 3,
  /** A relay limit (channels per host, bandwidth, size). */
  limitExceeded: 4,
  /** Invalid envelope. */
  protocolError: 5,
} as const;

export type CloseReason = (typeof CloseReason)[keyof typeof CloseReason];

/** Every {@link CloseReason} value, in wire order. */
export const CLOSE_REASONS: readonly CloseReason[] = [0, 1, 2, 3, 4, 5];

/** Which handshake follows `handshake-start` (spec 02 §2). */
export const HandshakeStartKind = {
  pair: 0,
  connect: 1,
} as const;

export type HandshakeStartKind = (typeof HandshakeStartKind)[keyof typeof HandshakeStartKind];

/** Every {@link HandshakeStartKind} value. */
export const HANDSHAKE_START_KINDS: readonly HandshakeStartKind[] = [0, 1];

/** Envelope of the host <-> relay family (types 0-6). */
export type HostEnvelope =
  /** Relay to host: registration challenge. */
  | { readonly type: "challenge"; readonly nonce: Uint8Array }
  /** Host to relay: routing key and signature over {@link registerMessage}. */
  | {
      readonly type: "register";
      readonly routingKey: Uint8Array;
      readonly signature: Uint8Array;
    }
  /** Relay to host: registration accepted. */
  | { readonly type: "registered"; readonly hostId: string }
  /** Relay to host: a client opened a channel. */
  | { readonly type: "channel-opened"; readonly channel: number }
  /** Both directions: one Noise record on a channel. */
  | {
      readonly type: "channel-data";
      readonly channel: number;
      readonly payload: Uint8Array;
    }
  /** Relay to host: a channel was closed. */
  | {
      readonly type: "channel-closed";
      readonly channel: number;
      readonly reason: CloseReason;
    }
  /** Host to relay: close a channel. */
  | {
      readonly type: "close-channel";
      readonly channel: number;
      readonly reason: CloseReason;
    };

/** Envelope of the client <-> relay family (types 16-18). */
export type ClientEnvelope =
  /** Both directions: one Noise record. */
  | { readonly type: "data"; readonly payload: Uint8Array }
  /** Relay to client: no host is registered under that `host_id`. */
  | { readonly type: "host-unavailable" }
  /** Relay to client: the channel is closed. */
  | { readonly type: "closed"; readonly reason: CloseReason };

/** First payload a client sends on a channel (spec 02 §2). */
export interface HandshakeStart {
  readonly kind: HandshakeStartKind;
  /** Noise message 1, in the clear. */
  readonly message1: Uint8Array;
}

export type EnvelopeErrorReason =
  /** Frame above `MAX_FRAME` (close with 1009). */
  | "frame-too-large"
  /** Not a strict, canonical CBOR item; see `cborReason`. */
  | "malformed-cbor"
  /** Item, field or type code has a different CBOR type than expected, or a fixed-size byte string has the wrong length. */
  | "wrong-type"
  /** Array with the wrong number of elements (an empty array included). */
  | "wrong-arity"
  /** Type code that is not in the family being decoded. */
  | "unknown-type"
  /** `close-reason` outside 0-5. */
  | "invalid-close-reason"
  /** `host-id` that is not 32 characters of canonical lowercase base32. */
  | "invalid-host-id"
  /** `handshake-start` kind that is neither pair (0) nor connect (1). */
  | "invalid-handshake-kind";

/** A frame that does not follow the envelope grammar. Carries no payload bytes. */
export class EnvelopeError extends Error {
  readonly reason: EnvelopeErrorReason;
  /** The CBOR-level cause when `reason` is `malformed-cbor`. */
  readonly cborReason: CborErrorReason | undefined;

  constructor(reason: EnvelopeErrorReason, cborReason?: CborErrorReason) {
    super(reason);
    this.name = "EnvelopeError";
    this.reason = reason;
    this.cborReason = cborReason;
  }
}

/** The WebSocket close code a client gets after `closed { reason }` (spec 01 §4). */
export function clientCloseCode(reason: CloseReason): number {
  switch (reason) {
    case CloseReason.limitExceeded:
      return CLOSE_CODE.limitExceeded;
    case CloseReason.protocolError:
      return CLOSE_CODE.invalidEnvelope;
    default:
      return CLOSE_CODE.normal;
  }
}

/** The bytes a host signs to register: `REGISTER_CONTEXT || nonce` (58 bytes). */
export function registerMessage(nonce: Uint8Array): Uint8Array {
  if (nonce.length !== NONCE_LEN) {
    throw new RangeError(`nonce must be ${NONCE_LEN} bytes`);
  }
  const out = new Uint8Array(REGISTER_CONTEXT.length + NONCE_LEN);
  for (let i = 0; i < REGISTER_CONTEXT.length; i++) {
    out[i] = REGISTER_CONTEXT.charCodeAt(i);
  }
  out.set(nonce, REGISTER_CONTEXT.length);
  return out;
}

/** Size of a CBOR head (initial byte plus argument) for the argument `n`. */
function headLength(n: number): number {
  if (n < 24) return 1;
  if (n < 0x100) return 2;
  if (n < 0x10000) return 3;
  if (n < 0x100000000) return 5;
  return 9;
}

/**
 * Size in bytes of the `channel-data` frame for `payloadLength` bytes of
 * payload on `channel`: what counts against `MAX_FRAME`.
 */
export function channelDataLength(channel: number, payloadLength: number): number {
  requireUint(channel, "channel");
  requireUint(payloadLength, "payload length");
  // array(3) head + type 4 + channel + bstr head + bytes
  return 1 + 1 + headLength(channel) + headLength(payloadLength) + payloadLength;
}

function requireUint(value: number, what: string): void {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${what} must be a non-negative safe integer`);
  }
}

function requireLength(value: Uint8Array, length: number, what: string): void {
  if (value.length !== length) {
    throw new RangeError(`${what} must be ${length} bytes`);
  }
}

function requireCloseReason(reason: number): void {
  if (!CLOSE_REASONS.includes(reason as CloseReason)) {
    throw new RangeError("close reason must be 0-5");
  }
}

// ---- encoding ---------------------------------------------------------------

const TYPE_CHALLENGE = 0;
const TYPE_REGISTER = 1;
const TYPE_REGISTERED = 2;
const TYPE_CHANNEL_OPENED = 3;
const TYPE_CHANNEL_DATA = 4;
const TYPE_CHANNEL_CLOSED = 5;
const TYPE_CLOSE_CHANNEL = 6;
const TYPE_DATA = 16;
const TYPE_HOST_UNAVAILABLE = 17;
const TYPE_CLOSED = 18;

/**
 * Deterministic encoding of a host-family envelope. Throws `RangeError` on a
 * value that cannot be encoded (wrong-size key, negative channel, ...).
 */
export function encodeHostEnvelope(envelope: HostEnvelope): Uint8Array {
  switch (envelope.type) {
    case "challenge":
      requireLength(envelope.nonce, NONCE_LEN, "nonce");
      return encodeCbor([TYPE_CHALLENGE, envelope.nonce]);
    case "register":
      requireLength(envelope.routingKey, ROUTING_KEY_LEN, "routing key");
      requireLength(envelope.signature, SIGNATURE_LEN, "signature");
      return encodeCbor([TYPE_REGISTER, envelope.routingKey, envelope.signature]);
    case "registered":
      if (parseHostId(envelope.hostId) === null) {
        throw new RangeError("host id is malformed");
      }
      return encodeCbor([TYPE_REGISTERED, envelope.hostId]);
    case "channel-opened":
      requireUint(envelope.channel, "channel");
      return encodeCbor([TYPE_CHANNEL_OPENED, envelope.channel]);
    case "channel-data":
      requireUint(envelope.channel, "channel");
      return encodeCbor([TYPE_CHANNEL_DATA, envelope.channel, envelope.payload]);
    case "channel-closed":
      requireUint(envelope.channel, "channel");
      requireCloseReason(envelope.reason);
      return encodeCbor([TYPE_CHANNEL_CLOSED, envelope.channel, envelope.reason]);
    case "close-channel":
      requireUint(envelope.channel, "channel");
      requireCloseReason(envelope.reason);
      return encodeCbor([TYPE_CLOSE_CHANNEL, envelope.channel, envelope.reason]);
  }
}

/** Deterministic encoding of a client-family envelope. */
export function encodeClientEnvelope(envelope: ClientEnvelope): Uint8Array {
  switch (envelope.type) {
    case "data":
      return encodeCbor([TYPE_DATA, envelope.payload]);
    case "host-unavailable":
      return encodeCbor([TYPE_HOST_UNAVAILABLE]);
    case "closed":
      requireCloseReason(envelope.reason);
      return encodeCbor([TYPE_CLOSED, envelope.reason]);
  }
}

/** Deterministic encoding of the `handshake-start` item. */
export function encodeHandshakeStart(start: HandshakeStart): Uint8Array {
  if (!HANDSHAKE_START_KINDS.includes(start.kind)) {
    throw new RangeError("handshake kind must be 0 or 1");
  }
  return encodeCbor([start.kind, start.message1]);
}

// ---- decoding ---------------------------------------------------------------

function wrongType(): EnvelopeError {
  return new EnvelopeError("wrong-type");
}

/** Strictly decodes one item; any CBOR refusal becomes `malformed-cbor`. */
function decodeStrict(frame: Uint8Array): unknown[] {
  if (frame.length > MAX_FRAME) {
    throw new EnvelopeError("frame-too-large");
  }
  let value;
  try {
    value = decodeCbor(frame);
  } catch (error) {
    if (error instanceof CborError) {
      throw new EnvelopeError("malformed-cbor", error.reason);
    }
    throw error;
  }
  if (!Array.isArray(value)) {
    throw wrongType();
  }
  if (value.length === 0) {
    throw new EnvelopeError("wrong-arity");
  }
  return value;
}

function arity(items: readonly unknown[], expected: number): void {
  if (items.length !== expected) {
    throw new EnvelopeError("wrong-arity");
  }
}

function field(items: readonly unknown[], index: number): unknown {
  return items[index];
}

function uint(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw wrongType();
  }
  return value;
}

function bytes(value: unknown, length?: number): Uint8Array {
  if (!(value instanceof Uint8Array) || (length !== undefined && value.length !== length)) {
    throw wrongType();
  }
  return value;
}

function closeReason(value: unknown): CloseReason {
  const n = uint(value);
  if (!CLOSE_REASONS.includes(n as CloseReason)) {
    throw new EnvelopeError("invalid-close-reason");
  }
  return n as CloseReason;
}

/**
 * The trailing byte string of a frame, as a view into `frame` (no copy). The
 * decoder already proved the frame holds exactly one item and that the byte
 * string is its last element, so it occupies the final `length` bytes.
 */
function trailingBytes(frame: Uint8Array, length: number): Uint8Array {
  return frame.subarray(frame.length - length);
}

/**
 * Decodes one host-family frame. `channel-data` payloads are views into
 * `frame`, not copies. Throws {@link EnvelopeError}.
 */
export function decodeHostEnvelope(frame: Uint8Array): HostEnvelope {
  const items = decodeStrict(frame);
  switch (uint(field(items, 0))) {
    case TYPE_CHALLENGE:
      arity(items, 2);
      return { type: "challenge", nonce: bytes(field(items, 1), NONCE_LEN) };
    case TYPE_REGISTER:
      arity(items, 3);
      return {
        type: "register",
        routingKey: bytes(field(items, 1), ROUTING_KEY_LEN),
        signature: bytes(field(items, 2), SIGNATURE_LEN),
      };
    case TYPE_REGISTERED: {
      arity(items, 2);
      const text = field(items, 1);
      if (typeof text !== "string") {
        throw wrongType();
      }
      const hostId = parseHostId(text);
      if (hostId === null) {
        throw new EnvelopeError("invalid-host-id");
      }
      return { type: "registered", hostId };
    }
    case TYPE_CHANNEL_OPENED:
      arity(items, 2);
      return { type: "channel-opened", channel: uint(field(items, 1)) };
    case TYPE_CHANNEL_DATA: {
      arity(items, 3);
      const channel = uint(field(items, 1));
      const payload = bytes(field(items, 2));
      return { type: "channel-data", channel, payload: trailingBytes(frame, payload.length) };
    }
    case TYPE_CHANNEL_CLOSED:
      arity(items, 3);
      return {
        type: "channel-closed",
        channel: uint(field(items, 1)),
        reason: closeReason(field(items, 2)),
      };
    case TYPE_CLOSE_CHANNEL:
      arity(items, 3);
      return {
        type: "close-channel",
        channel: uint(field(items, 1)),
        reason: closeReason(field(items, 2)),
      };
    default:
      throw new EnvelopeError("unknown-type");
  }
}

/**
 * Decodes one client-family frame. `data` payloads are views into `frame`,
 * not copies. Throws {@link EnvelopeError}.
 */
export function decodeClientEnvelope(frame: Uint8Array): ClientEnvelope {
  const items = decodeStrict(frame);
  switch (uint(field(items, 0))) {
    case TYPE_DATA: {
      arity(items, 2);
      const payload = bytes(field(items, 1));
      return { type: "data", payload: trailingBytes(frame, payload.length) };
    }
    case TYPE_HOST_UNAVAILABLE:
      arity(items, 1);
      return { type: "host-unavailable" };
    case TYPE_CLOSED:
      arity(items, 2);
      return { type: "closed", reason: closeReason(field(items, 1)) };
    default:
      throw new EnvelopeError("unknown-type");
  }
}

/**
 * Decodes the first payload of a channel. `message1` is a view into `bytes`,
 * not a copy. Throws {@link EnvelopeError}.
 */
export function decodeHandshakeStart(frame: Uint8Array): HandshakeStart {
  const items = decodeStrict(frame);
  arity(items, 2);
  const kind = uint(field(items, 0));
  if (!HANDSHAKE_START_KINDS.includes(kind as HandshakeStartKind)) {
    throw new EnvelopeError("invalid-handshake-kind");
  }
  const message1 = bytes(field(items, 1));
  return {
    kind: kind as HandshakeStartKind,
    message1: trailingBytes(frame, message1.length),
  };
}
