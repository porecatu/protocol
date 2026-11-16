// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Whole conversations between a `ClientSession` and a `HostChannel`, in
 * memory, through a test "relay" that only turns `data` frames into payloads
 * and back. The same scenarios as `rust/tests/full_session.rs`.
 */

import { describe, expect, it } from "vitest";
import {
  ClientError,
  ClientHandshake,
  ClientSession,
  CloseReason,
  CursorShape,
  decodeClientEnvelope,
  decodeHandshakeStart,
  decodeMessage,
  encodeClientEnvelope,
  encodeHandshakeStart,
  encodeMessage,
  ErrorCode,
  generateStaticKeypair,
  GoodbyeReason,
  HANDSHAKE_TIMEOUT_MS,
  HandshakeKind,
  HandshakeStartKind,
  hostId as computeHostId,
  HostChannel,
  HostError,
  HostHandshake,
  MAX_FRAME,
  PairingToken,
  Platform,
  PROTOCOL_MAJOR,
  Session,
  type ClientEvent,
  type GridSnapshot,
  type HostEvent,
  type Message,
  type PairingInvite,
  type StaticKeypair,
} from "../src/index.js";

const RELAY = "wss://relay.example.org";
const HOST_NAME = "Workstation";

/** Everything about a host that the tests need. */
class Host {
  readonly keys: StaticKeypair = generateStaticKeypair();
  readonly id: string = computeHostId(new Uint8Array(32).fill(0x42));

  invite(token: PairingToken): PairingInvite {
    return {
      major: PROTOCOL_MAJOR,
      relayUrl: RELAY,
      hostId: this.id,
      hostKey: this.keys.publicKey,
      token: token.bytes,
      hostName: HOST_NAME,
    };
  }
}

/** A client and a host channel with a relay between them. */
class Rig {
  toHost: Uint8Array[];
  /** Payloads the host wrote, waiting for the relay. */
  toClient: Uint8Array[] = [];
  clientLog: ClientEvent[] = [];
  hostLog: HostEvent[] = [];
  now = 0;

  constructor(
    readonly client: ClientSession,
    first: Uint8Array[],
    readonly host: HostChannel,
  ) {
    this.toHost = [...first];
  }

  /** Delivers everything in flight until both queues are empty. */
  pump(): void {
    for (;;) {
      const frame = this.toHost.shift();
      if (frame !== undefined) {
        // The relay: `data` becomes a channel payload.
        const envelope = decodeClientEnvelope(frame);
        if (envelope.type !== "data") {
          throw new Error("the client only writes data frames");
        }
        let events: HostEvent[];
        try {
          events = this.host.receive(envelope.payload, this.now);
        } catch (error) {
          if (error instanceof HostError) continue;
          throw error;
        }
        this.absorbHost(events);
        continue;
      }
      const payload = this.toClient.shift();
      if (payload !== undefined) {
        // The relay: a channel payload becomes `data`.
        let events: ClientEvent[];
        try {
          events = this.client.receive(encodeClientEnvelope({ type: "data", payload }), this.now);
        } catch (error) {
          if (error instanceof ClientError) continue;
          throw error;
        }
        this.absorbClient(events);
        continue;
      }
      return;
    }
  }

  absorbHost(events: HostEvent[]): void {
    for (const event of events) {
      if (event.type === "send") this.toClient.push(event.payload);
      else this.hostLog.push(event);
    }
  }

  absorbClient(events: ClientEvent[]): void {
    for (const event of events) {
      if (event.type === "send") this.toHost.push(event.frame);
      else this.clientLog.push(event);
    }
  }

  clientSends(message: Message): void {
    this.toHost.push(...this.client.send(message, this.now));
    this.pump();
  }

  hostSends(message: Message): void {
    this.toClient.push(...this.host.send(message, this.now));
    this.pump();
  }

  /** The messages the client received so far. */
  clientMessages(): Message[] {
    return this.clientLog.flatMap((e) => (e.type === "message" ? [e.message] : []));
  }

  /** The messages the host received so far. */
  hostMessages(): Message[] {
    return this.hostLog.flatMap((e) => (e.type === "message" ? [e.message] : []));
  }

  lastHostEvent(): HostEvent | undefined {
    return this.hostLog.at(-1);
  }
}

function pairingRig(host: Host, device: StaticKeypair, token: PairingToken): Rig {
  const { session, frames } = ClientSession.pair(
    host.invite(token),
    device,
    "Pixel 9",
    Platform.android,
    0,
  );
  return new Rig(session, frames, new HostChannel(host.keys, host.id, 0));
}

function connectRig(host: Host, device: StaticKeypair): Rig {
  const { session, frames } = ClientSession.connect(host.id, host.keys.publicKey, device, 0);
  return new Rig(session, frames, new HostChannel(host.keys, host.id, 0));
}

function hello(): Message {
  return { type: "hello", version: [0, 2], app: "porecatu-remote", appVersion: "0.1.0" };
}

function welcome(): Message {
  return {
    type: "welcome",
    version: [0, 2],
    hostName: HOST_NAME,
    porecatuVersion: "0.9.11",
    maxSubscriptions: 4,
    hostInstance: new Uint8Array(16).fill(3),
  };
}

function snapshot(seq: number, text: string): GridSnapshot {
  return {
    type: "grid-snapshot",
    pane: 7,
    seq,
    cols: 80,
    rows: 24,
    history: 0,
    dropped: 0,
    styles: [[null, null, 0]],
    lines: [[false, [[0, 1, 0, text]]]],
    cursor: { row: 0, col: 0, shape: CursorShape.block, visible: true },
    alt: false,
  };
}

/** Runs the whole conversation after the transport is open: `hello`, `welcome`, `subscribe`, `grid-snapshot`, `grid-ack`. */
function converse(rig: Rig): void {
  rig.clientSends(hello());
  expect(rig.hostMessages()).toContainEqual(hello());
  rig.hostSends(welcome());
  expect(rig.clientMessages()).toContainEqual(welcome());

  const subscribe: Message = { type: "subscribe", pane: 7 };
  rig.clientSends(subscribe);
  expect(rig.hostMessages()).toContainEqual(subscribe);

  const grid = snapshot(1, "$ ");
  rig.hostSends(grid);
  expect(rig.clientMessages()).toContainEqual(grid);

  const ack: Message = { type: "grid-ack", pane: 7, seq: 1 };
  rig.clientSends(ack);
  expect(rig.hostMessages()).toContainEqual(ack);
}

/** Pairs `device` with `host`; returns the rig with the pairing already accepted. */
function pairDevice(host: Host, device: StaticKeypair): Rig {
  const token = PairingToken.generate(0);
  const rig = pairingRig(host, device, token);
  rig.pump();

  // The desktop's side of the pairing: check the token, then answer.
  const last = rig.lastHostEvent();
  if (last?.type !== "pair-requested") {
    throw new Error(`the host must report the pair request: ${JSON.stringify(rig.hostLog.map((e) => e.type))}`);
  }
  expect(last.deviceKey).toEqual(device.publicKey);
  expect(last.request.device).toBe("Pixel 9");
  expect(last.request.platform).toBe(Platform.android);
  expect(token.matchesAndConsume(last.request.token, 1_000)).toBe(true);
  expect(
    rig.clientLog.some((e) => e.type === "handshake-complete"),
    "XX completes on the client when msg3 is written",
  ).toBe(true);

  const accepted: Message = { type: "pair-accepted", hostName: HOST_NAME };
  rig.hostSends(accepted);
  expect(rig.clientMessages().at(-1)).toEqual(accepted);
  return rig;
}

function clientCode(call: () => unknown): string {
  try {
    call();
  } catch (error) {
    if (error instanceof ClientError) return error.code;
    throw error;
  }
  throw new Error("expected a ClientError");
}

function hostCode(call: () => unknown): string {
  try {
    call();
  } catch (error) {
    if (error instanceof HostError) return error.code;
    throw error;
  }
  throw new Error("expected a HostError");
}

/** The event types, to assert the order of what happened. */
function types(events: ReadonlyArray<{ readonly type: string }>): string[] {
  return events.map((e) => e.type);
}

/** The data frame a client wrote, as the payload the host receives. */
function payloadOf(frame: Uint8Array | undefined): Uint8Array {
  if (frame === undefined) throw new Error("a frame was expected");
  const envelope = decodeClientEnvelope(frame);
  if (envelope.type !== "data") throw new Error("data expected");
  return envelope.payload;
}

describe("full session", () => {
  it("pairs, then holds a full conversation", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const rig = pairDevice(host, device);
    // The session continues as a normal connection, with no reconnect.
    converse(rig);
    expect(rig.client.isOpen).toBe(true);
    expect(rig.host.isOpen).toBe(true);
  });

  it("connects a paired device with IK", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const paired = [device.publicKey];
    pairDevice(host, device);

    const rig = connectRig(host, device);
    rig.pump();
    expect(types(rig.clientLog)).toEqual(["handshake-complete"]);
    const last = rig.lastHostEvent();
    if (last?.type !== "device-connecting") {
      throw new Error("the host must report the connecting device");
    }
    expect(paired).toContainEqual(last.deviceKey);
    converse(rig);
  });

  it("hands the XX events over in order: msg3 is sent before the handshake completes", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const rig = pairingRig(host, device, PairingToken.generate(0));
    // msg1 to the host, msg2 back.
    const [msg1] = rig.toHost.splice(0);
    const hostEvents = rig.host.receive(payloadOf(msg1), 0);
    expect(types(hostEvents)).toEqual(["send"]);
    const [reply] = hostEvents;
    if (reply?.type !== "send") throw new Error("msg2 expected");
    const events = rig.client.receive(
      encodeClientEnvelope({ type: "data", payload: reply.payload }),
      0,
    );
    expect(types(events)).toEqual(["send", "handshake-complete"]);
    expect(rig.client.isOpen).toBe(true);
    expect(rig.client.nextDeadline()).toBeNull();
  });

  it("answers not-paired to an unknown device after the handshake completed", () => {
    const host = new Host();
    const stranger = generateStaticKeypair();
    const paired: Uint8Array[] = [];

    const rig = connectRig(host, stranger);
    rig.pump();
    // The handshake completed on both sides before any refusal (spec 02 §4).
    expect(types(rig.clientLog)).toEqual(["handshake-complete"]);
    const last = rig.lastHostEvent();
    if (last?.type !== "device-connecting") {
      throw new Error("the host must report the connecting device");
    }
    expect(last.deviceKey).toEqual(stranger.publicKey);
    expect(paired).not.toContainEqual(last.deviceKey);

    rig.toClient.push(...rig.host.rejectNotPaired(0));
    rig.pump();
    expect(rig.clientMessages()).toEqual([{ type: "error", code: ErrorCode.notPaired }]);
    expect(rig.host.isClosed).toBe(true);
    expect(hostCode(() => rig.host.rejectNotPaired(0))).toBe("closed");
  });

  it("stops the client without sending when the QR key is wrong", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const impostor = generateStaticKeypair();
    const invite = { ...host.invite(PairingToken.generate(0)), hostKey: impostor.publicKey };

    const { session, frames } = ClientSession.pair(invite, device, "Pixel 9", Platform.ios, 0);
    const rig = new Rig(session, frames, new HostChannel(host.keys, host.id, 0));
    rig.pump();

    expect(types(rig.clientLog)).toEqual(["failed"]);
    const [failed] = rig.clientLog;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("handshake");
    expect(failed.error.detail).toBe("host-key-mismatch");
    expect(rig.client.isClosed).toBe(true);
    // Nothing was sent after msg1: no msg3 reached the host.
    expect(rig.toHost).toEqual([]);
    expect(rig.host.isHandshaking).toBe(true);
    expect(rig.hostLog).toEqual([]);
    expect(clientCode(() => rig.client.send(hello(), 0)), "a closed session sends nothing").toBe(
      "closed",
    );
  });

  it("runs the handshake deadline on both sides", () => {
    const host = new Host();
    const device = generateStaticKeypair();

    const { session: client } = ClientSession.connect(host.id, host.keys.publicKey, device, 1_000);
    const channel = new HostChannel(host.keys, host.id, 1_000);
    const deadline = 1_000 + HANDSHAKE_TIMEOUT_MS;
    expect(client.nextDeadline()).toBe(deadline);
    expect(channel.nextDeadline()).toBe(deadline);

    expect(client.tick(deadline - 1)).toEqual([]);
    expect(channel.tick(deadline - 1)).toEqual([]);
    expect(client.isClosed).toBe(false);

    const clientEvents = client.tick(deadline);
    expect(types(clientEvents)).toEqual(["failed"]);
    const [clientFailed] = clientEvents;
    if (clientFailed?.type !== "failed") throw new Error("failure expected");
    expect(clientFailed.error.code).toBe("timeout");
    expect(client.isClosed).toBe(true);
    expect(client.nextDeadline()).toBeNull();
    expect(client.tick(deadline + 1)).toEqual([]);

    const hostEvents = channel.tick(deadline);
    expect(types(hostEvents)).toEqual(["failed"]);
    const [hostFailed] = hostEvents;
    if (hostFailed?.type !== "failed") throw new Error("failure expected");
    expect(hostFailed.error.code).toBe("timeout");
    expect(hostFailed.error.closeReason).toBe(CloseReason.hostRejected);
    expect(channel.isClosed).toBe(true);
    expect(hostCode(() => channel.receive(new Uint8Array([0]), deadline))).toBe("closed");
  });

  it("counts late data as a timeout", () => {
    const host = new Host();
    const rig = connectRig(host, generateStaticKeypair());
    rig.now = HANDSHAKE_TIMEOUT_MS;
    rig.pump();
    expect(types(rig.hostLog)).toEqual(["failed"]);
    const [failed] = rig.hostLog;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("timeout");

    // The same on the client side.
    const { session } = ClientSession.connect(host.id, host.keys.publicKey, generateStaticKeypair(), 0);
    const events = session.receive(
      encodeClientEnvelope({ type: "data", payload: new Uint8Array(8) }),
      HANDSHAKE_TIMEOUT_MS,
    );
    expect(types(events)).toEqual(["failed"]);
  });

  it("ends the deadline once the transport opens", () => {
    const host = new Host();
    const rig = connectRig(host, generateStaticKeypair());
    expect(rig.client.nextDeadline()).not.toBeNull();
    rig.pump();
    expect(rig.client.nextDeadline()).toBeNull();
    expect(rig.host.nextDeadline()).toBeNull();
    expect(rig.client.tick(10 * HANDSHAKE_TIMEOUT_MS)).toEqual([]);
    expect(rig.host.tick(10 * HANDSHAKE_TIMEOUT_MS)).toEqual([]);
  });

  it("carries a message of 200 000 bytes in fragments", () => {
    const host = new Host();
    const rig = connectRig(host, generateStaticKeypair());
    rig.pump();

    // Client to host.
    const paste: Message = { type: "paste", pane: 7, text: "abcdefghij".repeat(20_000) };
    const frames = rig.client.send(paste, 0);
    expect(frames.length, "200 000 bytes need 4 or more records").toBeGreaterThanOrEqual(4);
    for (const frame of frames) {
      expect(frame.length).toBeLessThanOrEqual(MAX_FRAME);
    }
    rig.toHost.push(...frames);
    rig.pump();
    expect(rig.hostMessages()).toEqual([paste]);

    // Host to client.
    const big = snapshot(2, "x".repeat(200_000));
    const payloads = rig.host.send(big, 0);
    expect(payloads.length).toBeGreaterThanOrEqual(4);
    rig.toClient.push(...payloads);
    rig.pump();
    expect(rig.clientMessages()).toEqual([big]);
  });

  it("closes the client on the events of the relay", () => {
    const host = new Host();
    const device = generateStaticKeypair();

    const first = ClientSession.connect(host.id, host.keys.publicKey, device, 0).session;
    expect(first.receive(encodeClientEnvelope({ type: "host-unavailable" }), 0)).toEqual([
      { type: "host-unavailable" },
    ]);
    expect(first.isClosed).toBe(true);
    expect(clientCode(() => first.receive(new Uint8Array(0), 0))).toBe("closed");

    const second = ClientSession.connect(host.id, host.keys.publicKey, device, 0).session;
    const frame = encodeClientEnvelope({ type: "closed", reason: CloseReason.hostRejected });
    expect(second.receive(frame, 0)).toEqual([{ type: "closed", reason: CloseReason.hostRejected }]);
    expect(second.isClosed).toBe(true);
  });

  it("fails the client on garbage from the relay", () => {
    const host = new Host();
    const { session } = ClientSession.connect(host.id, host.keys.publicKey, generateStaticKeypair(), 0);
    const events = session.receive(new Uint8Array([0xff, 0x00]), 0);
    expect(types(events)).toEqual(["failed"]);
    const [failed] = events;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("envelope");
    expect(session.isClosed).toBe(true);
  });

  it("fails the client silently on a Noise message that does not decrypt", () => {
    const host = new Host();
    const { session } = ClientSession.connect(host.id, host.keys.publicKey, generateStaticKeypair(), 0);
    const frame = encodeClientEnvelope({ type: "data", payload: new Uint8Array(96).fill(7) });
    const events = session.receive(frame, 0);
    // No `send`: the client sends nothing after a failed msg2 (spec 02 §2).
    expect(types(events)).toEqual(["failed"]);
    const [failed] = events;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("handshake");
    expect(session.isClosed).toBe(true);
  });

  it("makes the host refuse a bad handshake-start", () => {
    const host = new Host();
    let channel = new HostChannel(host.keys, host.id, 0);
    let events = channel.receive(new Uint8Array([0xf6]), 0);
    expect(types(events)).toEqual(["failed"]);
    const [failed] = events;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("handshake-start");
    expect(failed.error.closeReason).toBe(CloseReason.hostRejected);
    expect(channel.isClosed).toBe(true);

    // An unknown handshake kind.
    channel = new HostChannel(host.keys, host.id, 0);
    events = channel.receive(new Uint8Array([0x82, 0x02, 0x40]), 0); // [2, h'']
    expect(types(events)).toEqual(["failed"]);
    const [unknown] = events;
    if (unknown?.type !== "failed") throw new Error("failure expected");
    expect(unknown.error.code).toBe("handshake-start");
  });

  it("makes the host refuse a prologue of another host_id", () => {
    const host = new Host();
    const other = computeHostId(new Uint8Array(32).fill(0x43));
    // The client dials `other`, the channel belongs to `host`.
    const { frames } = ClientSession.connect(other, host.keys.publicKey, generateStaticKeypair(), 0);
    const channel = new HostChannel(host.keys, host.id, 0);
    const events = channel.receive(payloadOf(frames[0]), 0);
    expect(types(events)).toEqual(["failed"]);
    const [failed] = events;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("handshake");
    expect(failed.error.detail).toBe("decrypt");
    expect(failed.error.closeReason).toBe(CloseReason.hostRejected);
    expect(channel.isClosed).toBe(true);
  });

  it("wipes the host key copy once the first payload arrives", () => {
    const host = new Host();
    const original = host.keys.privateKey.slice();
    const channel = new HostChannel(host.keys, host.id, 0);
    channel.receive(new Uint8Array([0xf6]), 0);
    // The caller's key pair is untouched; only the channel's copy was wiped.
    expect(host.keys.privateKey).toEqual(original);
  });

  /** A client written by hand, on top of the public handshake and record layers, to send what `ClientSession` never would. */
  function manualClient(host: Host, channel: HostChannel): Session {
    const device = generateStaticKeypair();
    const { handshake, msg1 } = ClientHandshake.connect({
      local: device,
      hostId: host.id,
      hostKey: host.keys.publicKey,
    });
    const start = encodeHandshakeStart({ kind: HandshakeStartKind.connect, message1: msg1 });
    const events = channel.receive(start, 0);
    const [first] = events;
    if (first?.type !== "send") {
      throw new Error("msg2 expected first");
    }
    const step = handshake.readMessage2(first.payload);
    if (step.kind !== "established") {
      throw new Error("IK must be established");
    }
    return new Session(step.transport, { nowMs: 0 });
  }

  it("makes the host answer error protocol to a message that does not decode", () => {
    const host = new Host();
    const channel = new HostChannel(host.keys, host.id, 0);
    const client = manualClient(host, channel);

    const record = client.seal(new Uint8Array([0xf6]), 0)[0];
    if (record === undefined) throw new Error("a record was expected");
    const events = channel.receive(record, 0);
    expect(types(events)).toEqual(["send", "failed"]);
    const [reply, failed] = events;
    if (reply?.type !== "send" || failed?.type !== "failed") throw new Error("error then failure expected");
    expect(failed.error.code).toBe("message");
    expect(failed.error.closeReason).toBe(CloseReason.normal);

    const bytes = client.open(reply.payload, 0);
    if (bytes === null) throw new Error("a whole message was expected");
    expect(decodeMessage(bytes, "host")).toEqual({
      ok: true,
      message: { type: "error", code: ErrorCode.protocol },
    });
    expect(channel.isClosed).toBe(true);
  });

  it("makes the host refuse a message only the host sends", () => {
    const host = new Host();
    const channel = new HostChannel(host.keys, host.id, 0);
    const client = manualClient(host, channel);

    const record = client.seal(encodeMessage(welcome()), 0)[0];
    if (record === undefined) throw new Error("a record was expected");
    const events = channel.receive(record, 0);
    expect(types(events)).toEqual(["send", "failed"]);
    const [, failed] = events;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("message");
    expect(failed.error.detail).toBe("wrong-direction");
  });

  it("makes the host fail on a corrupted record without answering", () => {
    const host = new Host();
    const channel = new HostChannel(host.keys, host.id, 0);
    const client = manualClient(host, channel);

    const record = client.seal(encodeMessage(hello()), 0)[0];
    if (record === undefined) throw new Error("a record was expected");
    record[record.length - 1] = (record[record.length - 1] ?? 0) ^ 1;
    const events = channel.receive(record, 0);
    expect(types(events)).toEqual(["failed"]);
    const [failed] = events;
    if (failed?.type !== "failed") throw new Error("failure expected");
    expect(failed.error.code).toBe("record");
    expect(channel.isClosed).toBe(true);
  });

  it("makes the host answer error protocol to a pair-request that does not decode", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const { handshake, msg1 } = ClientHandshake.pair({
      local: device,
      hostId: host.id,
      expectedHostKey: host.keys.publicKey,
    });
    const channel = new HostChannel(host.keys, host.id, 0);
    const started = channel.receive(
      encodeHandshakeStart({ kind: HandshakeStartKind.pair, message1: msg1 }),
      0,
    );
    const [msg2] = started;
    if (msg2?.type !== "send") throw new Error("msg2 expected");
    expect(handshake.readMessage2(msg2.payload).kind).toBe("needs-pair-request");
    // A payload that is valid CBOR but not a pair-request.
    const { msg3, transport } = handshake.writeMessage3(new Uint8Array([0xf6]));
    const events = channel.receive(msg3, 0);
    expect(types(events)).toEqual(["send", "failed"]);
    const [reply, failed] = events;
    if (reply?.type !== "send" || failed?.type !== "failed") throw new Error("error then failure expected");
    expect(failed.error.code).toBe("pair-request");
    expect(failed.error.closeReason).toBe(CloseReason.normal);
    const client = new Session(transport, { nowMs: 0 });
    const bytes = client.open(reply.payload, 0);
    if (bytes === null) throw new Error("a whole message was expected");
    expect(decodeMessage(bytes, "host")).toEqual({
      ok: true,
      message: { type: "error", code: ErrorCode.protocol },
    });
    expect(channel.isClosed).toBe(true);
  });

  it("makes the client answer error protocol to a host message that does not decode", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const { session: client, frames } = ClientSession.connect(
      host.id,
      host.keys.publicKey,
      device,
      0,
    );
    const start = decodeHandshakeStart(payloadOf(frames[0]));
    const result = HostHandshake.respond({
      kind: HandshakeKind.connect,
      local: host.keys,
      hostId: host.id,
      msg1: start.message1,
    });
    const hostSession = new Session(result.transport, { nowMs: 0 });

    expect(
      client.receive(encodeClientEnvelope({ type: "data", payload: result.msg2 }), 0),
    ).toEqual([{ type: "handshake-complete" }]);

    // A host that sends a message only the client may send.
    const record = hostSession.seal(encodeMessage(hello()), 0)[0];
    if (record === undefined) throw new Error("a record was expected");
    const events = client.receive(encodeClientEnvelope({ type: "data", payload: record }), 0);
    expect(types(events)).toEqual(["send", "failed"]);
    const [reply, failed] = events;
    if (reply?.type !== "send" || failed?.type !== "failed") throw new Error("error then failure expected");
    expect(failed.error.code).toBe("message");
    const bytes = hostSession.open(payloadOf(reply.frame), 0);
    if (bytes === null) throw new Error("a whole message was expected");
    expect(decodeMessage(bytes, "client")).toEqual({
      ok: true,
      message: { type: "error", code: ErrorCode.protocol },
    });
    expect(client.isClosed).toBe(true);
  });

  it("enforces the directions when sending", () => {
    const host = new Host();
    const rig = connectRig(host, generateStaticKeypair());

    // Before the handshake ends there is no channel to send on.
    expect(clientCode(() => rig.client.send(hello(), 0))).toBe("not-open");
    expect(hostCode(() => rig.host.send(welcome(), 0))).toBe("not-open");

    rig.pump();
    expect(clientCode(() => rig.client.send(welcome(), 0))).toBe("not-sendable");
    expect(hostCode(() => rig.host.send(hello(), 0))).toBe("not-sendable");
    // Rejected sends do not close the session.
    expect(rig.client.isOpen && rig.host.isOpen).toBe(true);

    // `goodbye` goes both ways.
    const goodbye: Message = { type: "goodbye", reason: GoodbyeReason.normal };
    rig.clientSends(goodbye);
    rig.hostSends(goodbye);
    expect(rig.hostMessages()).toEqual([goodbye]);
    expect(rig.clientMessages()).toEqual([goodbye]);
  });

  it("refuses a device name out of range and another major at pair", () => {
    const host = new Host();
    const device = generateStaticKeypair();
    const token = PairingToken.generate(0);

    for (const name of ["", "x".repeat(65)]) {
      expect(clientCode(() => ClientSession.pair(host.invite(token), device, name, Platform.other, 0))).toBe(
        "pair-request",
      );
    }

    const invite = { ...host.invite(token), major: 1 };
    expect(clientCode(() => ClientSession.pair(invite, device, "ok", Platform.other, 0))).toBe(
      "unsupported-major",
    );
  });

  it("does not let a used token pair a second device", () => {
    const host = new Host();
    const first = generateStaticKeypair();
    const second = generateStaticKeypair();
    const token = PairingToken.generate(0);

    // First device: the token matches and is consumed.
    let rig = pairingRig(host, first, token);
    rig.pump();
    let last = rig.lastHostEvent();
    if (last?.type !== "pair-requested") throw new Error("pair request expected");
    expect(token.matchesAndConsume(last.request.token, 10)).toBe(true);

    // Second device with the same QR: the token no longer matches.
    rig = pairingRig(host, second, token);
    rig.pump();
    last = rig.lastHostEvent();
    if (last?.type !== "pair-requested") throw new Error("pair request expected");
    expect(token.matchesAndConsume(last.request.token, 20)).toBe(false);
  });
});
