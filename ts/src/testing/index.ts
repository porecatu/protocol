// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * `@porecatu/protocol/testing`: test helpers for code that talks the Porecatu
 * protocol. **For tests and the "fake desktop" of a development build only**:
 * nothing here belongs in a production path (the keys are predictable, the
 * fake host accepts what a test tells it to).
 *
 * - {@link testKeypair}: deterministic static key pairs from a seed.
 * - {@link createTestClock}: a manual monotonic clock (`now()` in ms).
 * - {@link createMemoryRelay}: an in-memory relay. A host attaches under its
 *   `host_id`; clients connect to it. It speaks the envelope of spec 01 §4 on
 *   both sides: it opens a channel (`channel-opened`), translates `data` to
 *   `channel-data` and back, answers `host-unavailable` when no host is
 *   registered, and ends a channel with `closed { reason }` (and
 *   `channel-closed` to the host). The frames it hands out are the exact
 *   frames a `ClientSession` (client side) and a `HostChannel` (host side)
 *   exchange with a real relay.
 * - {@link createFakeHost}: a minimal desktop on top of the relay. It pairs
 *   (`XX`, one live token), accepts paired devices (`IK`), answers `hello`
 *   with `welcome`, `theme`, `profiles` and a synthetic `workspace`, and
 *   answers `subscribe` with a `grid-snapshot`. Everything else a client
 *   sends is recorded in `received` and otherwise ignored.
 *
 * Delivery is synchronous: a call returns after every frame it caused, in
 * both directions, was delivered (frames produced while delivering are
 * queued, not nested, so the order is deterministic).
 *
 * Like the rest of the package it is sans-IO and has no Node dependency.
 */

import { blake2s } from "@noble/hashes/blake2.js";
import { ctEqual } from "../ct.js";
import {
  channelDataLength,
  CloseReason,
  decodeClientEnvelope,
  decodeHostEnvelope,
  encodeClientEnvelope,
  encodeHostEnvelope,
  EnvelopeError,
  MAX_FRAME,
} from "../envelope.js";
import { hostId as deriveHostId } from "../hostId.js";
import { PROTOCOL_VERSION } from "../index.js";
import {
  CursorShape,
  ErrorCode,
  PairRejectReason,
  PaneState,
  ProfileKind,
} from "../message/codes.js";
import type { Message, Profile, Theme, WorkspaceMessage } from "../message/types.js";
import { PROTOCOL_MAJOR, type PairingInvite } from "../pairingUri.js";
import { HostChannel, HostError, type HostEvent } from "../session/host.js";
import { publicKeyFromPrivate, type RandomSource, type StaticKeypair } from "../session/keys.js";
import { PairingToken } from "../session/pairingToken.js";

// --- Keys and clock ---

/**
 * A deterministic static key pair: the private key is `BLAKE2s` of a fixed
 * label and `seed`, so the same seed gives the same pair on every run. **For
 * tests only**: the keys are predictable.
 */
export function testKeypair(seed: number): StaticKeypair {
  if (!Number.isSafeInteger(seed) || seed < 0) {
    throw new RangeError("seed must be a non-negative safe integer");
  }
  const label = "porecatu-protocol/testing/keypair/";
  const input = new Uint8Array(label.length + 8);
  for (let i = 0; i < label.length; i++) {
    input[i] = label.charCodeAt(i);
  }
  // The seed as 8 bytes, big endian (never the 32-bit overflow of bit operators).
  let rest = seed;
  for (let i = input.length - 1; i >= label.length; i--) {
    input[i] = rest % 256;
    rest = Math.floor(rest / 256);
  }
  const privateKey = blake2s(input, { dkLen: 32 });
  return { publicKey: publicKeyFromPrivate(privateKey), privateKey };
}

/** A manual monotonic clock, in milliseconds. */
export interface TestClock {
  readonly now: () => number;
  /** Moves the clock forward; a negative or non-integer step is refused. */
  readonly advance: (ms: number) => void;
}

/** A {@link TestClock} starting at `startMs` (default 0). */
export function createTestClock(startMs = 0): TestClock {
  let current = startMs;
  return {
    now: () => current,
    advance(ms: number): void {
      if (!Number.isSafeInteger(ms) || ms < 0) {
        throw new RangeError("advance step must be a non-negative integer");
      }
      current += ms;
    },
  };
}

// --- In-memory relay ---

/** Receives the frames the relay sends to one side. */
export type FrameSink = (frame: Uint8Array) => void;

/** The host's end of the relay: send host-family frames, or leave. */
export interface HostLink {
  /**
   * Sends one host-family frame (`channel-data` or `close-channel`). Other
   * frame types are ignored: registration is {@link MemoryRelay.attachHost}.
   * Throws `EnvelopeError` for a frame that does not decode.
   */
  send(frame: Uint8Array): void;
  /** Drops the host: every open channel is closed with `host-gone`. Idempotent. */
  detach(): void;
}

/** One client's end of the relay. */
export interface ClientLink {
  /** The channel number the host sees, or `null` when there is no host or the channel ended. */
  readonly channel: number | null;
  /**
   * Sends one client-family frame (`data`). A frame the relay refuses (above
   * `MAX_FRAME`, a bad envelope) closes the channel as a real relay would
   * (`limit-exceeded`, `protocol-error`). Ignored once the channel ended.
   */
  send(frame: Uint8Array): void;
  /** Closes the channel from the client's side; the host sees `channel-closed { normal }`. Idempotent. */
  close(): void;
}

/** An in-memory relay between hosts and clients. */
export interface MemoryRelay {
  /** Registers a host under `hostId`; `onFrame` receives host-family frames. A host already there is replaced (`host-replaced`). */
  attachHost(hostId: string, onFrame: FrameSink): HostLink;
  /**
   * Opens a client channel to `hostId`; `onFrame` receives client-family
   * frames. With no host registered the first frame is `host-unavailable`.
   */
  connectClient(hostId: string, onFrame: FrameSink): ClientLink;
  /** Whether a host is registered under `hostId`. */
  hasHost(hostId: string): boolean;
  /** How many channels are open to `hostId`. */
  channelCount(hostId: string): number;
}

interface ChannelEntry {
  readonly onClient: FrameSink;
}

interface HostEntry {
  readonly onFrame: FrameSink;
  readonly channels: Map<number, ChannelEntry>;
  nextChannel: number;
  active: boolean;
}

/** An in-memory relay; see the module comment. */
export function createMemoryRelay(): MemoryRelay {
  const hosts = new Map<string, HostEntry>();
  const queue: Array<() => void> = [];
  let draining = false;

  /** Runs `job` now, or after the job that is running, never nested. */
  function schedule(job: () => void): void {
    queue.push(job);
    if (draining) {
      return;
    }
    draining = true;
    try {
      for (let next = queue.shift(); next !== undefined; next = queue.shift()) {
        next();
      }
    } finally {
      draining = false;
    }
  }

  function toClient(channel: ChannelEntry, frame: Uint8Array): void {
    schedule(() => channel.onClient(frame));
  }

  function toHost(host: HostEntry, frame: Uint8Array): void {
    schedule(() => {
      if (host.active) {
        host.onFrame(frame);
      }
    });
  }

  /** Ends a channel: `closed { toClient }` to the client, `channel-closed { toHost }` to the host when given. */
  function closeChannel(
    host: HostEntry,
    number: number,
    toClientReason: CloseReason,
    toHostReason: CloseReason | null,
  ): void {
    const channel = host.channels.get(number);
    if (channel === undefined) {
      return;
    }
    host.channels.delete(number);
    toClient(channel, encodeClientEnvelope({ type: "closed", reason: toClientReason }));
    if (toHostReason !== null) {
      toHost(host, encodeHostEnvelope({ type: "channel-closed", channel: number, reason: toHostReason }));
    }
  }

  function dropHost(id: string, host: HostEntry, reason: CloseReason): void {
    if (!host.active) {
      return;
    }
    host.active = false;
    if (hosts.get(id) === host) {
      hosts.delete(id);
    }
    for (const number of [...host.channels.keys()]) {
      closeChannel(host, number, reason, null);
    }
  }

  return {
    attachHost(hostId: string, onFrame: FrameSink): HostLink {
      const previous = hosts.get(hostId);
      if (previous !== undefined) {
        dropHost(hostId, previous, CloseReason.hostReplaced);
      }
      const entry: HostEntry = { onFrame, channels: new Map(), nextChannel: 1, active: true };
      hosts.set(hostId, entry);
      return {
        send(frame: Uint8Array): void {
          if (!entry.active) {
            return;
          }
          const envelope = decodeHostEnvelope(frame);
          if (envelope.type === "channel-data") {
            const channel = entry.channels.get(envelope.channel);
            if (channel !== undefined) {
              toClient(channel, encodeClientEnvelope({ type: "data", payload: envelope.payload.slice() }));
            }
          } else if (envelope.type === "close-channel") {
            closeChannel(entry, envelope.channel, envelope.reason, null);
          }
        },
        detach(): void {
          dropHost(hostId, entry, CloseReason.hostGone);
        },
      };
    },

    connectClient(hostId: string, onFrame: FrameSink): ClientLink {
      const host = hosts.get(hostId);
      const channel: ChannelEntry = { onClient: onFrame };
      if (host === undefined) {
        toClient(channel, encodeClientEnvelope({ type: "host-unavailable" }));
        return { channel: null, send: () => undefined, close: () => undefined };
      }
      const number = host.nextChannel;
      host.nextChannel += 1;
      host.channels.set(number, channel);
      toHost(host, encodeHostEnvelope({ type: "channel-opened", channel: number }));
      return {
        get channel(): number | null {
          return host.channels.get(number) === channel ? number : null;
        },
        send(frame: Uint8Array): void {
          if (host.channels.get(number) !== channel) {
            return;
          }
          if (frame.length > MAX_FRAME) {
            closeChannel(host, number, CloseReason.limitExceeded, CloseReason.limitExceeded);
            return;
          }
          let envelope;
          try {
            envelope = decodeClientEnvelope(frame);
          } catch (error) {
            if (error instanceof EnvelopeError) {
              closeChannel(host, number, CloseReason.protocolError, CloseReason.protocolError);
              return;
            }
            throw error;
          }
          if (envelope.type !== "data") {
            // `host-unavailable` and `closed` only travel from the relay to the client.
            closeChannel(host, number, CloseReason.protocolError, CloseReason.protocolError);
            return;
          }
          if (channelDataLength(number, envelope.payload.length) > MAX_FRAME) {
            closeChannel(host, number, CloseReason.limitExceeded, CloseReason.limitExceeded);
            return;
          }
          toHost(
            host,
            encodeHostEnvelope({ type: "channel-data", channel: number, payload: envelope.payload.slice() }),
          );
        },
        close(): void {
          if (host.channels.get(number) !== channel) {
            return;
          }
          host.channels.delete(number);
          toHost(host, encodeHostEnvelope({ type: "channel-closed", channel: number, reason: CloseReason.normal }));
        },
      };
    },

    hasHost: (hostId: string): boolean => hosts.has(hostId),
    channelCount: (hostId: string): number => hosts.get(hostId)?.channels.size ?? 0,
  };
}

// --- Fake host ---

/** A message the fake host received, with the channel it came on. */
export interface ReceivedMessage {
  readonly channel: number;
  readonly message: Message;
}

/** Options of {@link createFakeHost}. */
export interface FakeHostOptions {
  readonly relay: MemoryRelay;
  /** Time source for handshake deadlines and the pairing token. */
  readonly now: () => number;
  /** Host static key pair; default `testKeypair(1)`. */
  readonly hostKey?: StaticKeypair;
  /** `host_id` to register under; default derived from a fixed routing key. */
  readonly hostId?: string;
  /** Name shown by the client; default `"Test Desktop"`. */
  readonly hostName?: string;
  /** Static public keys of devices that are already paired. */
  readonly pairedDevices?: readonly Uint8Array[];
  /** `max_subscriptions` of the `welcome`; default 4. */
  readonly maxSubscriptions?: number;
  /** Randomness for ephemeral keys and tokens; default `globalThis.crypto`. */
  readonly random?: RandomSource;
}

/** A minimal desktop on the in-memory relay. */
export interface FakeHost {
  readonly hostId: string;
  /** Host static public key. */
  readonly hostKey: Uint8Array;
  /** Every message received from clients, in order. */
  readonly received: readonly ReceivedMessage[];
  /** Static public keys of the paired devices (copies). */
  pairedDevices(): Uint8Array[];
  isPaired(deviceKey: Uint8Array): boolean;
  /**
   * Starts a pairing: a new single-use token valid for five minutes (it
   * replaces the previous one) and the invitation a QR code would carry.
   */
  startPairing(relayUrl: string): PairingInvite;
  /** Sends a message on every open channel (a `bell`, a new `workspace`, ...). */
  broadcast(message: Message): void;
  /** Leaves the relay: open channels end with `host-gone`. */
  detach(): void;
}

const DEFAULT_ROUTING_KEY = new Uint8Array(32).fill(7);
const PANE = 1;

/** The synthetic `theme`. */
function syntheticTheme(): Theme {
  return {
    type: "theme",
    fg: 0xd4d4d4,
    bg: 0x1e1e1e,
    cursor: 0xffffff,
    selectionBg: 0x264f78,
    palette: [
      0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
      0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
    ],
    groupColors: [0x2472c8, 0x0dbc79, 0xe5e510, 0xbc3fbc],
  };
}

const SYNTHETIC_PROFILES: readonly Profile[] = [
  { id: "default", name: "Default", kind: ProfileKind.local },
];

/** The synthetic `workspace`: one window, one tab, one running pane of 80x24. */
function syntheticWorkspace(): WorkspaceMessage {
  return {
    type: "workspace",
    rev: 1,
    windows: [
      {
        id: 1,
        title: "Test Desktop",
        pinned: [],
        items: [
          {
            tab: 1,
            title: "bash",
            custom: false,
            profile: "default",
            remote: null,
            activity: false,
            bell: false,
            layout: {
              pane: PANE,
              title: "bash",
              cwd: null,
              state: PaneState.running,
              exit: null,
              cols: 80,
              rows: 24,
            },
            focused: PANE,
          },
        ],
        active: 1,
      },
    ],
  };
}

/** The synthetic `grid-snapshot` of the only pane: a prompt on the first line. */
function syntheticSnapshot(pane: number, seq: number): Message {
  const lines: Array<readonly [boolean, ReadonlyArray<readonly [number, 1 | 2, number, string]>]> = [
    [false, [[0, 1, 0, "$ "]]],
  ];
  for (let row = 1; row < 24; row++) {
    lines.push([false, []]);
  }
  return {
    type: "grid-snapshot",
    pane,
    seq,
    cols: 80,
    rows: 24,
    history: 0,
    dropped: 0,
    styles: [[null, null, 0]],
    lines,
    cursor: { row: 0, col: 2, shape: CursorShape.block, visible: true },
    alt: false,
  };
}

interface ChannelState {
  readonly channel: HostChannel;
  /** Pane to the `seq` its next grid message takes (spec 03 §5.3: 0 on the first of a subscription). */
  readonly subscriptions: Map<number, number>;
}

/** A minimal desktop on top of `relay`; see the module comment. */
export function createFakeHost(options: FakeHostOptions): FakeHost {
  const { relay, now } = options;
  const key = options.hostKey ?? testKeypair(1);
  const id = options.hostId ?? deriveHostId(DEFAULT_ROUTING_KEY);
  const hostName = options.hostName ?? "Test Desktop";
  const maxSubscriptions = options.maxSubscriptions ?? 4;
  const paired: Uint8Array[] = (options.pairedDevices ?? []).map((device) => device.slice());
  const channels = new Map<number, ChannelState>();
  const received: ReceivedMessage[] = [];
  let token: PairingToken | null = null;

  const isPaired = (deviceKey: Uint8Array): boolean =>
    paired.some((known) => ctEqual(known, deviceKey));

  const link = relay.attachHost(id, (frame) => onFrame(frame));

  function sendPayloads(number: number, payloads: readonly Uint8Array[]): void {
    for (const payload of payloads) {
      link.send(encodeHostEnvelope({ type: "channel-data", channel: number, payload }));
    }
  }

  function closeChannel(number: number, reason: CloseReason): void {
    const state = channels.get(number);
    if (state === undefined) {
      return;
    }
    state.channel.close();
    channels.delete(number);
    link.send(encodeHostEnvelope({ type: "close-channel", channel: number, reason }));
  }

  function reply(number: number, state: ChannelState, ...messages: Message[]): void {
    for (const message of messages) {
      sendPayloads(number, state.channel.send(message, now()));
    }
  }

  function onMessage(number: number, state: ChannelState, message: Message): void {
    received.push({ channel: number, message });
    switch (message.type) {
      case "hello": {
        const [major, minor] = message.version;
        if (major !== PROTOCOL_VERSION[0] || minor !== PROTOCOL_VERSION[1]) {
          reply(number, state, { type: "incompatible", version: PROTOCOL_VERSION });
          closeChannel(number, CloseReason.normal);
          return;
        }
        reply(
          number,
          state,
          {
            type: "welcome",
            version: PROTOCOL_VERSION,
            hostName,
            porecatuVersion: "0.0.0-test",
            maxSubscriptions,
            hostInstance: new Uint8Array(16).fill(0x5a),
          },
          syntheticTheme(),
          { type: "profiles", list: SYNTHETIC_PROFILES, defaultProfile: "default" },
          syntheticWorkspace(),
        );
        return;
      }
      case "subscribe": {
        if (message.pane !== PANE) {
          reply(number, state, { type: "error", code: ErrorCode.unknownPane, pane: message.pane });
        } else if (
          state.subscriptions.size >= maxSubscriptions &&
          !state.subscriptions.has(message.pane)
        ) {
          reply(number, state, {
            type: "error",
            code: ErrorCode.tooManySubscriptions,
            pane: message.pane,
          });
        } else {
          // The first grid message of a subscription is seq 0; subscribing
          // again to a pane already subscribed sends the next one.
          const seq = state.subscriptions.get(message.pane) ?? 0;
          state.subscriptions.set(message.pane, seq + 1);
          reply(number, state, syntheticSnapshot(message.pane, seq));
        }
        return;
      }
      case "unsubscribe":
        state.subscriptions.delete(message.pane);
        return;
      case "goodbye":
        closeChannel(number, CloseReason.normal);
        return;
      default:
        return;
    }
  }

  function onEvents(number: number, state: ChannelState, events: readonly HostEvent[]): void {
    for (const event of events) {
      if (!channels.has(number)) {
        return;
      }
      switch (event.type) {
        case "send":
          sendPayloads(number, [event.payload]);
          break;
        case "device-connecting":
          if (!isPaired(event.deviceKey)) {
            sendPayloads(number, state.channel.rejectNotPaired(now()));
            closeChannel(number, CloseReason.normal);
          }
          break;
        case "pair-requested": {
          const matches = token !== null && token.matchesAndConsume(event.request.token, now());
          if (matches) {
            paired.push(event.deviceKey.slice());
            reply(number, state, { type: "pair-accepted", hostName });
          } else {
            reply(number, state, { type: "pair-rejected", reason: PairRejectReason.tokenInvalid });
            closeChannel(number, CloseReason.normal);
          }
          break;
        }
        case "message":
          onMessage(number, state, event.message);
          break;
        case "failed":
          closeChannel(number, event.error.closeReason);
          break;
      }
    }
  }

  function onFrame(frame: Uint8Array): void {
    const envelope = decodeHostEnvelope(frame);
    switch (envelope.type) {
      case "channel-opened": {
        const channelOptions = options.random === undefined ? {} : { random: options.random };
        channels.set(envelope.channel, {
          channel: new HostChannel(key, id, now(), channelOptions),
          subscriptions: new Map(),
        });
        break;
      }
      case "channel-data": {
        const state = channels.get(envelope.channel);
        if (state !== undefined) {
          let events: HostEvent[];
          try {
            events = state.channel.receive(envelope.payload, now());
          } catch (error) {
            if (error instanceof HostError) {
              return;
            }
            throw error;
          }
          onEvents(envelope.channel, state, events);
        }
        break;
      }
      case "channel-closed": {
        channels.get(envelope.channel)?.channel.close();
        channels.delete(envelope.channel);
        break;
      }
      default:
        break;
    }
  }

  return {
    hostId: id,
    hostKey: key.publicKey.slice(),
    received,
    pairedDevices: () => paired.map((device) => device.slice()),
    isPaired,
    startPairing(relayUrl: string): PairingInvite {
      token?.destroy();
      token = PairingToken.generate(now(), options.random);
      const invite: PairingInvite = {
        major: PROTOCOL_MAJOR,
        relayUrl,
        hostId: id,
        hostKey: key.publicKey.slice(),
        token: token.bytes,
        hostName,
      };
      return invite;
    },
    broadcast(message: Message): void {
      for (const [number, state] of channels) {
        if (state.channel.isOpen) {
          reply(number, state, message);
        }
      }
    },
    detach(): void {
      for (const state of channels.values()) {
        state.channel.close();
      }
      channels.clear();
      link.detach();
    },
  };
}
