// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The `@porecatu/protocol/testing` helpers: deterministic keys, the in-memory
 * relay and the fake host, driven by a real `ClientSession`; plus the
 * `destroy`/`close` of the sessions.
 */

import { describe, expect, it } from "vitest";
import {
  ClientSession,
  CloseReason,
  ErrorCode,
  GoodbyeReason,
  PairRejectReason,
  PROTOCOL_VERSION,
  Platform,
  decodeClientEnvelope,
  encodeClientEnvelope,
  encodeHostEnvelope,
  hexEncode,
  type ClientEvent,
  type Message,
  type PairingInvite,
  type StaticKeypair,
} from "../src/index.js";
import {
  createFakeHost,
  createMemoryRelay,
  createTestClock,
  testKeypair,
  type ClientLink,
  type FakeHost,
  type MemoryRelay,
  type TestClock,
} from "../src/testing/index.js";

const RELAY_URL = "wss://relay.example.org";

/** A real client session wired to the relay. */
class Client {
  events: ClientEvent[] = [];
  session!: ClientSession;
  link!: ClientLink;

  constructor(
    private readonly relay: MemoryRelay,
    private readonly clock: TestClock,
    readonly device: StaticKeypair,
  ) {}

  private pump(events: ClientEvent[]): void {
    for (const event of events) {
      this.events.push(event);
      if (event.type === "send") {
        this.link.send(event.frame);
      }
    }
  }

  private attach(hostId: string, session: ClientSession, frames: Uint8Array[]): void {
    this.session = session;
    this.link = this.relay.connectClient(hostId, (frame) => {
      if (!this.session.isClosed) {
        this.pump(this.session.receive(frame, this.clock.now()));
      }
    });
    for (const frame of frames) {
      this.link.send(frame);
    }
  }

  pair(invite: PairingInvite): void {
    const { session, frames } = ClientSession.pair(invite, this.device, "Phone", Platform.android, this.clock.now());
    this.attach(invite.hostId, session, frames);
  }

  connect(hostId: string, hostKey: Uint8Array): void {
    const { session, frames } = ClientSession.connect(hostId, hostKey, this.device, this.clock.now());
    this.attach(hostId, session, frames);
  }

  /** Connects to a host id with nobody registered, to see the relay's answer. */
  connectBlind(hostId: string): void {
    const { session, frames } = ClientSession.connect(hostId, testKeypair(1).publicKey, this.device, this.clock.now());
    this.attach(hostId, session, frames);
  }

  send(message: Message): void {
    for (const frame of this.session.send(message, this.clock.now())) {
      this.link.send(frame);
    }
  }

  messages(): Message[] {
    return this.events.flatMap((event) => (event.type === "message" ? [event.message] : []));
  }
}

function world(): { relay: MemoryRelay; clock: TestClock; host: FakeHost; client: Client } {
  const relay = createMemoryRelay();
  const clock = createTestClock(1000);
  const host = createFakeHost({ relay, now: clock.now });
  return { relay, clock, host, client: new Client(relay, clock, testKeypair(100)) };
}

const HELLO: Message = {
  type: "hello",
  version: PROTOCOL_VERSION,
  app: "porecatu-remote",
  appVersion: "0.0.0",
};

describe("testKeypair", () => {
  it("is deterministic and distinct per seed", () => {
    const a = testKeypair(1);
    expect(hexEncode(a.privateKey)).toBe(hexEncode(testKeypair(1).privateKey));
    expect(hexEncode(a.publicKey)).toBe(hexEncode(testKeypair(1).publicKey));
    expect(hexEncode(a.privateKey)).not.toBe(hexEncode(testKeypair(2).privateKey));
    expect(a.publicKey).toHaveLength(32);
  });

  it("handles seeds beyond 32 bits and refuses invalid ones", () => {
    expect(hexEncode(testKeypair(2 ** 40).privateKey)).not.toBe(hexEncode(testKeypair(0).privateKey));
    expect(() => testKeypair(-1)).toThrow(RangeError);
    expect(() => testKeypair(1.5)).toThrow(RangeError);
  });
});

describe("createTestClock", () => {
  it("moves forward only", () => {
    const clock = createTestClock(5);
    clock.advance(10);
    expect(clock.now()).toBe(15);
    expect(() => clock.advance(-1)).toThrow(RangeError);
  });
});

describe("memory relay", () => {
  it("answers host-unavailable when no host is registered", () => {
    const { client } = world();
    client.connectBlind("k".repeat(32));
    expect(client.events).toEqual([{ type: "host-unavailable" }]);
    expect(client.session.isClosed).toBe(true);
    expect(client.link.channel).toBeNull();
  });

  it("numbers channels, closes them and reports the reasons", () => {
    const relay = createMemoryRelay();
    const toHost: Uint8Array[] = [];
    const hostLink = relay.attachHost("h", (frame) => toHost.push(frame));
    const toClient: Uint8Array[] = [];
    const link = relay.connectClient("h", (frame) => toClient.push(frame));
    expect(link.channel).toBe(1);
    expect(relay.channelCount("h")).toBe(1);

    link.send(encodeClientEnvelope({ type: "data", payload: new Uint8Array([9]) }));
    hostLink.send(encodeHostEnvelope({ type: "channel-data", channel: 1, payload: new Uint8Array([8]) }));
    expect(toHost).toHaveLength(2); // channel-opened, channel-data
    const reply = decodeClientEnvelope(toClient[0] as Uint8Array);
    expect(reply).toEqual({ type: "data", payload: new Uint8Array([8]) });

    hostLink.send(encodeHostEnvelope({ type: "close-channel", channel: 1, reason: CloseReason.hostRejected }));
    expect(decodeClientEnvelope(toClient[1] as Uint8Array)).toEqual({
      type: "closed",
      reason: CloseReason.hostRejected,
    });
    expect(link.channel).toBeNull();
    expect(relay.channelCount("h")).toBe(0);
  });

  it("closes with host-gone when the host leaves and host-replaced when another registers", () => {
    const relay = createMemoryRelay();
    const first = relay.attachHost("h", () => undefined);
    const seen: Uint8Array[] = [];
    relay.connectClient("h", (frame) => seen.push(frame));
    relay.attachHost("h", () => undefined);
    expect(decodeClientEnvelope(seen[0] as Uint8Array)).toEqual({
      type: "closed",
      reason: CloseReason.hostReplaced,
    });
    first.detach();
    expect(relay.hasHost("h")).toBe(true);

    const second = relay.connectClient("h", (frame) => seen.push(frame));
    relay.attachHost("g", () => undefined).detach();
    expect(relay.hasHost("g")).toBe(false);
    expect(second.channel).toBe(1);
  });

  it("tells the host when the client closes", () => {
    const relay = createMemoryRelay();
    const toHost: Uint8Array[] = [];
    relay.attachHost("h", (frame) => toHost.push(frame));
    const link = relay.connectClient("h", () => undefined);
    link.close();
    link.close();
    expect(toHost).toHaveLength(2); // channel-opened, channel-closed
  });

  it("answers a bad frame or an oversized one with a limit or protocol close", () => {
    const relay = createMemoryRelay();
    const toHost: Uint8Array[] = [];
    relay.attachHost("h", (frame) => toHost.push(frame));
    const seen: Uint8Array[] = [];
    const bad = relay.connectClient("h", (frame) => seen.push(frame));
    bad.send(new Uint8Array([0xff]));
    expect(decodeClientEnvelope(seen[0] as Uint8Array)).toEqual({
      type: "closed",
      reason: CloseReason.protocolError,
    });
    const big = relay.connectClient("h", (frame) => seen.push(frame));
    big.send(encodeClientEnvelope({ type: "data", payload: new Uint8Array(69_995) }));
    expect(decodeClientEnvelope(seen[1] as Uint8Array)).toEqual({
      type: "closed",
      reason: CloseReason.limitExceeded,
    });
  });
});

describe("fake host", () => {
  it("pairs, then answers hello with welcome, theme, profiles and workspace", () => {
    const { host, client } = world();
    const invite = host.startPairing(RELAY_URL);
    client.pair(invite);
    expect(client.events.map((event) => event.type)).toContain("handshake-complete");
    expect(client.messages()).toEqual([{ type: "pair-accepted", hostName: "Test Desktop" }]);
    expect(host.isPaired(client.device.publicKey)).toBe(true);

    client.send(HELLO);
    const types = client.messages().map((message) => message.type);
    expect(types).toEqual(["pair-accepted", "welcome", "theme", "profiles", "workspace"]);
    expect(host.received.map((r) => r.message.type)).toEqual(["hello"]);
  });

  it("rejects a pairing whose token is not the live one", () => {
    const { host, client } = world();
    const invite = host.startPairing(RELAY_URL);
    host.startPairing(RELAY_URL); // replaces the token
    client.pair(invite);
    expect(client.messages()).toEqual([{ type: "pair-rejected", reason: PairRejectReason.tokenInvalid }]);
    expect(host.isPaired(client.device.publicKey)).toBe(false);
    expect(client.events.at(-1)).toEqual({ type: "closed", reason: CloseReason.normal });
    expect(client.session.isClosed).toBe(true);
  });

  it("rejects an expired token", () => {
    const { host, client, clock } = world();
    const invite = host.startPairing(RELAY_URL);
    clock.advance(300_000);
    client.pair(invite);
    expect(client.messages()).toEqual([{ type: "pair-rejected", reason: PairRejectReason.tokenInvalid }]);
  });

  it("accepts a paired device on IK, and subscribe gets a grid-snapshot", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const client = new Client(relay, clock, device);
    client.connect(host.hostId, host.hostKey);
    expect(client.session.isOpen).toBe(true);

    client.send(HELLO);
    client.send({ type: "subscribe", pane: 1 });
    const snapshot = client.messages().find((message) => message.type === "grid-snapshot");
    // Spec 03 §5.3: the first grid message of a subscription is seq 0.
    expect(snapshot).toMatchObject({ type: "grid-snapshot", pane: 1, seq: 0, cols: 80, rows: 24 });

    client.send({ type: "subscribe", pane: 99 });
    expect(client.messages().at(-1)).toEqual({ type: "error", code: ErrorCode.unknownPane, pane: 99 });
    client.send({ type: "grid-ack", pane: 1, seq: 0 });
    expect(host.received.at(-1)?.message).toEqual({ type: "grid-ack", pane: 1, seq: 0 });
  });

  it("accepts view-tab, with a tab and with null, records it and keeps the channel open", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const client = new Client(relay, clock, device);
    client.connect(host.hostId, host.hostKey);
    client.send(HELLO);
    const before = client.messages().length;

    client.send({ type: "view-tab", tab: 1 });
    client.send({ type: "view-tab", tab: null });
    expect(host.received.slice(-2).map((entry) => entry.message)).toEqual([
      { type: "view-tab", tab: 1 },
      { type: "view-tab", tab: null },
    ]);
    // No reply, no error, and the channel still works.
    expect(client.messages()).toHaveLength(before);
    expect(client.session.isOpen).toBe(true);
    client.send({ type: "subscribe", pane: 1 });
    expect(client.messages().at(-1)).toMatchObject({ type: "grid-snapshot", pane: 1 });
  });

  it("numbers the grid messages of a subscription from 0, and subscribing again takes the next seq", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const client = new Client(relay, clock, device);
    client.connect(host.hostId, host.hostKey);
    client.send(HELLO);

    const seqs = (): number[] =>
      client.messages().flatMap((message) => (message.type === "grid-snapshot" ? [message.seq] : []));
    client.send({ type: "subscribe", pane: 1 });
    // The same subscription asked again: the next seq (spec 03 section 4).
    client.send({ type: "subscribe", pane: 1 });
    expect(seqs()).toEqual([0, 1]);
    // A new subscription after an unsubscribe starts over.
    client.send({ type: "unsubscribe", pane: 1 });
    client.send({ type: "subscribe", pane: 1 });
    expect(seqs()).toEqual([0, 1, 0]);
  });

  it("answers an unknown device with error not-paired and closes", () => {
    const { host, client } = world();
    client.connect(host.hostId, host.hostKey);
    expect(client.messages()).toEqual([{ type: "error", code: ErrorCode.notPaired }]);
    expect(client.events.at(-1)).toEqual({ type: "closed", reason: CloseReason.normal });
  });

  it("answers an incompatible hello and closes", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const client = new Client(relay, clock, device);
    client.connect(host.hostId, host.hostKey);
    client.send({ ...HELLO, version: [0, 2] });
    expect(client.messages()).toEqual([{ type: "incompatible", version: PROTOCOL_VERSION }]);
    expect(client.session.isClosed).toBe(true);
  });

  it("ends the channel on goodbye and with host-gone when the host detaches", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const first = new Client(relay, clock, device);
    first.connect(host.hostId, host.hostKey);
    first.send({ type: "goodbye", reason: GoodbyeReason.normal });
    expect(first.session.isClosed).toBe(true);

    const second = new Client(relay, clock, device);
    second.connect(host.hostId, host.hostKey);
    expect(second.session.isOpen).toBe(true);
    host.detach();
    expect(second.events.at(-1)).toEqual({ type: "closed", reason: CloseReason.hostGone });
    expect(relay.hasHost(host.hostId)).toBe(false);
  });

  it("broadcasts to open channels", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const client = new Client(relay, clock, device);
    client.connect(host.hostId, host.hostKey);
    host.broadcast({ type: "bell", pane: 1 });
    expect(client.messages()).toEqual([{ type: "bell", pane: 1 }]);
  });
});

describe("destroy on close", () => {
  it("ClientSession.close ends the session and later sends throw", () => {
    const device = testKeypair(100);
    const relay = createMemoryRelay();
    const clock = createTestClock();
    const host = createFakeHost({ relay, now: clock.now, pairedDevices: [device.publicKey] });
    const client = new Client(relay, clock, device);
    client.connect(host.hostId, host.hostKey);
    client.session.close();
    client.session.close();
    expect(client.session.isClosed).toBe(true);
    expect(() => client.session.send(HELLO, 0)).toThrow(/closed/);
  });
});
