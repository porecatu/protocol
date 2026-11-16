// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Replay of a session vector (`sessoes/` and `sessoes-ts/`) with the real
 * state machines: a {@link ClientSession} and a {@link HostChannel} with the
 * fixed keys and ephemerals of the vector, and the clock the vector gives.
 *
 * Every step names its sender. What the sender writes must match
 * `registros_hex` and `envelope_hex` byte for byte, and what the receiver
 * reports must be the message of the step. The host's answers also follow the
 * rules of specs 02 and 03 (what it owes after a `pair-request`, after an `IK`
 * of an unknown device, after a `hello`), exactly as the Rust replay checks.
 */

import { expect } from "vitest";
import { encodeCbor, type CborValue } from "../../src/cbor/index.js";
import { hexEncode } from "../../src/encoding.js";
import {
  CloseReason,
  decodeClientEnvelope,
  decodeHandshakeStart,
  decodeHostEnvelope,
  encodeClientEnvelope,
  encodeHostEnvelope,
} from "../../src/envelope.js";
import { ErrorCode, PairRejectReason } from "../../src/message/codes.js";
import { decodeMessage, encodeMessage } from "../../src/message/index.js";
import { decodePairRequest } from "../../src/message/pairing.js";
import type { Direction, Message } from "../../src/message/types.js";
import { ClientSession, type ClientEvent, type ClientStart } from "../../src/session/client.js";
import { HostChannel, type HostEvent } from "../../src/session/host.js";
import { ctEqual } from "../../src/ct.js";
import { publicKeyFromPrivate, type RandomSource, type StaticKeypair } from "../../src/session/keys.js";
import { PairingToken } from "../../src/session/pairingToken.js";
import { fromTyped, hexOf } from "./vectors.js";

/** The four fixed private keys of a conversation. */
export interface SessionKeys {
  readonly hostStatic: Uint8Array;
  readonly hostEphemeral: Uint8Array;
  readonly clientStatic: Uint8Array;
  readonly clientEphemeral: Uint8Array;
}

/** Reads the `chaves` object of a vector. */
export function readKeys(chaves: unknown): SessionKeys {
  const object = chaves as Record<string, unknown>;
  const key = (name: string): Uint8Array => {
    const bytes = fromTyped(object[name]);
    if (!(bytes instanceof Uint8Array) || bytes.length !== 32) {
      throw new Error(`${name} is not a 32-byte bstr`);
    }
    return bytes;
  };
  return {
    hostStatic: key("host_estatica"),
    hostEphemeral: key("host_efemera"),
    clientStatic: key("cliente_estatica"),
    clientEphemeral: key("cliente_efemera"),
  };
}

/** A key pair from a private key; the state machines wipe what they are given, so each user gets a copy. */
export function keypairOf(privateKey: Uint8Array): StaticKeypair {
  return { privateKey: privateKey.slice(), publicKey: publicKeyFromPrivate(privateKey) };
}

/** A random source that always returns the given ephemeral key (**only for vectors**). */
export function fixedRandom(ephemeral: Uint8Array): RandomSource {
  return (n: number): Uint8Array => {
    if (n !== ephemeral.length) {
      throw new Error("the vectors only draw the ephemeral key");
    }
    return ephemeral.slice();
  };
}

/** The relay of the vectors is any `wss` URL: it never appears on the wire. */
export const RELAY_URL = "wss://relay.example.org";

const HOST_NAME = "Workstation";

export function hexes(parts: readonly Uint8Array[]): string[] {
  return parts.map((part) => hexEncode(part));
}

/** The payload of a client `data` frame. */
export function clientPayload(frame: Uint8Array): Uint8Array {
  const envelope = decodeClientEnvelope(frame);
  if (envelope.type !== "data") {
    throw new Error("a client frame that is not `data`");
  }
  return envelope.payload;
}

/** The payload of a host `channel-data` frame on `channel`. */
export function hostPayload(frame: Uint8Array, channel: number): Uint8Array {
  const envelope = decodeHostEnvelope(frame);
  if (envelope.type !== "channel-data" || envelope.channel !== channel) {
    throw new Error("a host frame that is not `channel-data` of the channel");
  }
  return envelope.payload;
}

/** What the relay hands the client: a `data` envelope around a host payload. */
export function dataFrame(payload: Uint8Array): Uint8Array {
  return encodeClientEnvelope({ type: "data", payload });
}

/** What the host writes: a `channel-data` envelope. */
export function channelFrame(channel: number, payload: Uint8Array): Uint8Array {
  return encodeHostEnvelope({ type: "channel-data", channel, payload });
}

/** The frame with which the host closes a channel normally. */
export function closeFrame(channel: number): Uint8Array {
  return encodeHostEnvelope({ type: "close-channel", channel, reason: CloseReason.normal });
}

/** The `send` payloads of host events, in order. */
export function hostSends(events: readonly HostEvent[]): Uint8Array[] {
  return events.flatMap((event) => (event.type === "send" ? [event.payload] : []));
}

/** The frames of client `send` events, in order. */
export function clientSends(events: readonly ClientEvent[]): Uint8Array[] {
  return events.flatMap((event) => (event.type === "send" ? [event.frame] : []));
}

/** What the host must answer next (specs 02 and 03). */
export type Decision =
  | { readonly kind: "pairing"; readonly tokenOk: boolean }
  | { readonly kind: "not-paired" }
  | { readonly kind: "hello"; readonly sameVersion: boolean };

/** Checks the first host message after a decision against the rules. */
export function checkDecision(
  decision: Decision,
  message: Message,
  hostVersion: readonly [number, number],
): void {
  switch (decision.kind) {
    case "pairing":
      if (decision.tokenOk) {
        expect(message.type).toBe("pair-accepted");
      } else {
        expect(message).toEqual({ type: "pair-rejected", reason: PairRejectReason.tokenInvalid });
      }
      return;
    case "not-paired":
      expect(message).toEqual({ type: "error", code: ErrorCode.notPaired });
      return;
    case "hello":
      expect(message.type).toBe(decision.sameVersion ? "welcome" : "incompatible");
      expect((message as { version: readonly [number, number] }).version).toEqual([...hostVersion]);
      return;
  }
}

/** Decodes the wire bytes of an item sent by `from`; the message must re-encode to the same bytes. */
export function decodeItem(wire: Uint8Array, from: Direction): Message {
  const decoded = decodeMessage(wire, from);
  if (!decoded.ok) {
    throw new Error(`the item does not decode: ${decoded.error.reason}`);
  }
  expect(hexEncode(encodeMessage(decoded.message))).toBe(hexEncode(wire));
  return decoded.message;
}

interface Step {
  readonly fromClient: boolean;
  readonly now: number;
  readonly item: CborValue | null;
  readonly records: Uint8Array[];
  readonly frames: Uint8Array[];
}

function stepOf(raw: Record<string, unknown>): Step {
  return {
    fromClient: raw["de"] === "cliente",
    now: raw["now_ms"] as number,
    item: raw["mensagem"] === null ? null : fromTyped(raw["mensagem"]),
    records: (raw["registros_hex"] as string[]).map((h) => hexOf(h, "registros_hex")),
    frames: (raw["envelope_hex"] as string[]).map((h) => hexOf(h, "envelope_hex")),
  };
}

/** The code of the closed conversation, from the last message the host sent. */
function closingCode(last: Message): string {
  switch (last.type) {
    case "pair-rejected":
      return "token-invalid";
    case "error":
      return "not-paired";
    case "incompatible":
      return "incompatible";
    default:
      throw new Error(`closed after a message of type ${last.type}`);
  }
}

/** Replays one session vector; every mismatch fails an `expect`. */
export function replaySession(v: Record<string, unknown>): void {
  const keys = readKeys(v["chaves"]);
  const hostId = v["host_id"] as string;
  const channel = v["canal"] as number;
  const hostVersion = v["versao"] as [number, number];
  const kind = v["handshake"] as "XX" | "IK";
  const tokenHost = v["token_host"] === null ? null : (fromTyped(v["token_host"]) as Uint8Array);
  const paired = (v["pareados"] as unknown[]).map((key) => fromTyped(key) as Uint8Array);
  const steps = (v["passos"] as Array<Record<string, unknown>>).map(stepOf);
  const handshakeLen = kind === "XX" ? 3 : 2;
  expect(steps.length).toBeGreaterThan(handshakeLen);

  // The public keys are kept apart: the state machines wipe the pairs they are given.
  const hostPublic = publicKeyFromPrivate(keys.hostStatic);
  const devicePublic = publicKeyFromPrivate(keys.clientStatic);
  const at = (index: number): Step => {
    const step = steps[index];
    if (step === undefined) {
      throw new Error(`no step ${index}`);
    }
    return step;
  };

  // Step 0: the client writes msg1, inside a handshake-start.
  const first = at(0);
  expect(first.fromClient).toBe(true);
  expect(first.item).toBeNull();
  const clientRandom = fixedRandom(keys.clientEphemeral);
  let start: ClientStart;
  if (kind === "XX") {
    // The pair-request is the payload of msg3: the invitation gets its token.
    const request = decodePairRequest(encodeCbor(at(2).item));
    start = ClientSession.pair(
      {
        major: hostVersion[0],
        relayUrl: RELAY_URL,
        hostId,
        hostKey: hostPublic.slice(),
        token: request.token,
        hostName: HOST_NAME,
      },
      keypairOf(keys.clientStatic),
      request.device,
      request.platform,
      first.now,
      { random: clientRandom },
    );
  } else {
    start = ClientSession.connect(hostId, hostPublic.slice(), keypairOf(keys.clientStatic), first.now, {
      random: clientRandom,
    });
  }
  const { session: client } = start;
  expect(hexes(start.frames), "frame 0").toEqual(hexes(first.frames));
  const startPayload = clientPayload(start.frames[0] as Uint8Array);
  expect(hexes([decodeHandshakeStart(startPayload).message1]), "msg1").toEqual(hexes(first.records));

  const host = new HostChannel(keypairOf(keys.hostStatic), hostId, first.now, {
    random: fixedRandom(keys.hostEphemeral),
  });
  const hostOut = host.receive(startPayload, at(1).now);

  // Step 1: the host answers msg2.
  const second = at(1);
  expect(second.fromClient).toBe(false);
  expect(second.item).toBeNull();
  const msg2 = hostSends(hostOut);
  expect(hexes(msg2), "msg2").toEqual(hexes(second.records));
  expect(hexes(msg2.map((p) => channelFrame(channel, p))), "frame 1").toEqual(hexes(second.frames));
  expect(msg2).toHaveLength(1);

  let owed: Decision | null = null;
  if (kind === "IK") {
    const connecting = hostOut.find((event) => event.type === "device-connecting");
    expect(connecting?.type).toBe("device-connecting");
    if (connecting?.type === "device-connecting") {
      expect(hexEncode(connecting.deviceKey)).toBe(hexEncode(devicePublic));
      if (!paired.some((known) => ctEqual(known, connecting.deviceKey))) {
        owed = { kind: "not-paired" };
      }
    }
    const events = client.receive(dataFrame(msg2[0] as Uint8Array), second.now);
    expect(events).toEqual([{ type: "handshake-complete" }]);
  } else {
    // The client reads msg2 at the instant of its msg3.
    const third = at(2);
    const events = client.receive(dataFrame(msg2[0] as Uint8Array), third.now);
    expect(events.map((event) => event.type)).toEqual(["send", "handshake-complete"]);
    expect(third.fromClient).toBe(true);
    const sent = clientSends(events);
    expect(hexes(sent), "frame 2").toEqual(hexes(third.frames));
    expect(hexes(sent.map(clientPayload)), "msg3").toEqual(hexes(third.records));
    const reading = host.receive(clientPayload(sent[0] as Uint8Array), third.now);
    const request = decodePairRequest(encodeCbor(third.item));
    expect(reading).toEqual([
      { type: "pair-requested", deviceKey: devicePublic, request },
    ]);
    const tokenOk =
      tokenHost !== null && new PairingToken(tokenHost, 0).matchesAndConsume(request.token, third.now);
    owed = { kind: "pairing", tokenOk };
  }

  // The steps after the handshake.
  let decision: Decision | null = owed;
  let closed = false;
  let lastHost: Message | null = null;
  for (let index = handshakeLen; index < steps.length; index++) {
    const step = at(index);
    const where = `step ${index}`;
    expect(closed, `${where}: a step after the close`).toBe(false);
    if (step.records.length === 0) {
      // The host closes the channel.
      expect(step.fromClient, where).toBe(false);
      expect(hexes(step.frames), where).toEqual(hexes([closeFrame(channel)]));
      closed = true;
      continue;
    }
    expect(step.item, `${where}: a message step needs \`mensagem\``).not.toBeNull();
    const wire = encodeCbor(step.item);
    const message = decodeItem(wire, step.fromClient ? "client" : "host");
    if (step.fromClient) {
      const frames = client.send(message, step.now);
      expect(hexes(frames), `${where}: frames`).toEqual(hexes(step.frames));
      expect(hexes(frames.map(clientPayload)), `${where}: records`).toEqual(hexes(step.records));
      const events = frames.flatMap((frame) => host.receive(clientPayload(frame), step.now));
      expect(events, `${where}: host events`).toEqual([{ type: "message", message }]);
      if (message.type === "hello") {
        decision = {
          kind: "hello",
          sameVersion: message.version[0] === hostVersion[0] && message.version[1] === hostVersion[1],
        };
      }
    } else {
      const reject = decision?.kind === "not-paired" && message.type === "error";
      const payloads = reject ? host.rejectNotPaired(step.now) : host.send(message, step.now);
      const frames = payloads.map((payload) => channelFrame(channel, payload));
      expect(hexes(frames), `${where}: frames`).toEqual(hexes(step.frames));
      expect(hexes(payloads), `${where}: records`).toEqual(hexes(step.records));
      const events = frames.flatMap((frame) =>
        client.receive(dataFrame(hostPayload(frame, channel)), step.now),
      );
      expect(events, `${where}: client events`).toEqual([{ type: "message", message }]);
      if (decision !== null) {
        checkDecision(decision, message, hostVersion);
        decision = null;
      }
      lastHost = message;
    }
  }

  // The final state follows from the host's last message.
  const result = v["resultado"] as { estado: string; erro: { camada: string; codigo: string } | null };
  expect(closed).toBe(result.estado === "fechada");
  if (closed) {
    expect(lastHost, "closed without a host message").not.toBeNull();
    expect(result.erro).toEqual({ camada: "mensagem", codigo: closingCode(lastHost as Message) });
  } else {
    expect(result.erro).toBeNull();
  }
}
