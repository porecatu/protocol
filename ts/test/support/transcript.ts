// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Transcripts written by this package (`sessoes-ts/`): whole conversations
 * between a {@link ClientSession} and a {@link HostChannel} of the TypeScript
 * implementation, with fixed keys, fixed ephemerals and a fixed clock, in the
 * same format as the vectors the Rust tool writes (`sessoes/`). The Rust crate
 * replays them byte for byte (`rust/tests/vectors.rs`), which is the "TS to
 * Rust" half of the cross test.
 *
 * Each step is one message (or one handshake message, or the host closing the
 * channel). The frames are the ones the **sender** writes: the client's are
 * `data` envelopes, the host's are `channel-data` envelopes on its own
 * connection to the relay. The first client frame carries a `handshake-start`.
 *
 * Both sides run in this process: the sender seals, the receiver opens, so a
 * transcript is only produced when the two state machines agree with each
 * other.
 */

import { expect } from "vitest";
import { decodeCbor, type CborValue } from "../../src/cbor/index.js";
import { hexEncode } from "../../src/encoding.js";
import { decodeHandshakeStart } from "../../src/envelope.js";
import { hostId as deriveHostId } from "../../src/hostId.js";
import { PROTOCOL_VERSION } from "../../src/index.js";
import {
  CursorShape,
  ErrorCode,
  MOD,
  NamedKey,
  PaneState,
  Platform,
  ProfileKind,
  SplitDir,
} from "../../src/message/codes.js";
import { encodeMessage } from "../../src/message/index.js";
import { encodePairRequest } from "../../src/message/pairing.js";
import type { Message, PairRequest } from "../../src/message/types.js";
import { testKeypair } from "../../src/testing/index.js";
import { ClientSession } from "../../src/session/client.js";
import { HostChannel } from "../../src/session/host.js";
import {
  channelFrame,
  clientPayload,
  clientSends,
  closeFrame,
  dataFrame,
  decodeItem,
  fixedRandom,
  hexes,
  hostPayload,
  hostSends,
  keypairOf,
  RELAY_URL,
  type SessionKeys,
} from "./session.js";
import { toTyped } from "./vectors.js";

/** The channel number the host sees. */
const CHANNEL = 3;
const HOST_NAME = "Workstation";

/** `n` bytes counting up from `from`. */
function seq(from: number, n: number): Uint8Array {
  return Uint8Array.from({ length: n }, (_, i) => (from + i) & 0xff);
}

/** The four fixed private keys of the transcripts: `testKeypair` pairs, so a test can derive them again. */
export function transcriptKeys(): SessionKeys {
  return {
    hostStatic: testKeypair(1).privateKey,
    hostEphemeral: testKeypair(2).privateKey,
    clientStatic: testKeypair(100).privateKey,
    clientEphemeral: testKeypair(101).privateKey,
  };
}

/** The host id every transcript is addressed to (any routing key: it is only hashed). */
export function transcriptHostId(): string {
  return deriveHostId(new Uint8Array(32).fill(0x7e));
}

interface StepJson {
  readonly de: "cliente" | "host";
  readonly now_ms: number;
  readonly mensagem: unknown;
  readonly registros_hex: string[];
  readonly envelope_hex: string[];
}

function bstr(bytes: Uint8Array): { bstr: string } {
  return { bstr: hexEncode(bytes) };
}

/** What a transcript is built from. */
interface Setup {
  readonly kind: "XX" | "IK";
  /** The token the host holds (pairing only). */
  readonly tokenHost: Uint8Array | null;
  /** The device keys on the host's list. */
  readonly paired: readonly Uint8Array[];
}

/** One conversation being recorded. */
export class Conversation {
  private readonly keys = transcriptKeys();
  private readonly hostId = transcriptHostId();
  private readonly steps: StepJson[] = [];
  private client: ClientSession | null = null;
  private host: HostChannel | null = null;

  constructor(private readonly setup: Setup) {}

  private push(
    de: StepJson["de"],
    nowMs: number,
    item: CborValue | null,
    records: readonly Uint8Array[],
    frames: readonly Uint8Array[],
  ): void {
    this.steps.push({
      de,
      now_ms: nowMs,
      mensagem: item === null ? null : toTyped(item),
      registros_hex: hexes(records),
      envelope_hex: hexes(frames),
    });
  }

  private session(): { client: ClientSession; host: HostChannel } {
    if (this.client === null || this.host === null) {
      throw new Error("the handshake did not run");
    }
    return { client: this.client, host: this.host };
  }

  private newHost(nowMs: number): HostChannel {
    return new HostChannel(keypairOf(this.keys.hostStatic), this.hostId, nowMs, {
      random: fixedRandom(this.keys.hostEphemeral),
    });
  }

  /** `XX`: `msg1` at `t[0]`, `msg2` at `t[1]`, `msg3` (the `pair-request`) at `t[2]`. */
  startXX(t: readonly [number, number, number], request: PairRequest): void {
    const hostPublic = testKeypair(1).publicKey;
    const { session: client, frames } = ClientSession.pair(
      {
        major: PROTOCOL_VERSION[0],
        relayUrl: RELAY_URL,
        hostId: this.hostId,
        hostKey: hostPublic,
        token: request.token.slice(),
        hostName: HOST_NAME,
      },
      keypairOf(this.keys.clientStatic),
      request.device,
      request.platform,
      t[0],
      { random: fixedRandom(this.keys.clientEphemeral) },
    );
    const startPayload = clientPayload(frames[0] as Uint8Array);
    this.push("cliente", t[0], null, [decodeHandshakeStart(startPayload).message1], frames);

    const host = this.newHost(t[0]);
    const answered = host.receive(startPayload, t[1]);
    const msg2 = hostSends(answered);
    expect(answered.map((event) => event.type)).toEqual(["send"]);
    this.push("host", t[1], null, msg2, msg2.map((payload) => channelFrame(CHANNEL, payload)));

    // The client reads msg2 at the instant of its msg3.
    const written = client.receive(dataFrame(msg2[0] as Uint8Array), t[2]);
    expect(written.map((event) => event.type)).toEqual(["send", "handshake-complete"]);
    const sent = clientSends(written);
    this.push("cliente", t[2], decodeCbor(encodePairRequest(request)), sent.map(clientPayload), sent);
    const reading = host.receive(clientPayload(sent[0] as Uint8Array), t[2]);
    expect(reading).toEqual([
      { type: "pair-requested", deviceKey: testKeypair(100).publicKey, request },
    ]);
    this.client = client;
    this.host = host;
  }

  /** `IK`: `msg1` at `t[0]`, `msg2` at `t[1]`. */
  startIK(t: readonly [number, number]): void {
    const { session: client, frames } = ClientSession.connect(
      this.hostId,
      testKeypair(1).publicKey,
      keypairOf(this.keys.clientStatic),
      t[0],
      { random: fixedRandom(this.keys.clientEphemeral) },
    );
    const startPayload = clientPayload(frames[0] as Uint8Array);
    this.push("cliente", t[0], null, [decodeHandshakeStart(startPayload).message1], frames);

    const host = this.newHost(t[0]);
    const answered = host.receive(startPayload, t[1]);
    const msg2 = hostSends(answered);
    expect(answered.map((event) => event.type)).toEqual(["send", "device-connecting"]);
    this.push("host", t[1], null, msg2, msg2.map((payload) => channelFrame(CHANNEL, payload)));
    expect(client.receive(dataFrame(msg2[0] as Uint8Array), t[1])).toEqual([
      { type: "handshake-complete" },
    ]);
    this.client = client;
    this.host = host;
  }

  /** The client sends `message`; the host's state machine reads it. */
  clientSays(nowMs: number, message: Message): void {
    const { client, host } = this.session();
    const frames = client.send(message, nowMs);
    const events = frames.flatMap((frame) => host.receive(clientPayload(frame), nowMs));
    expect(events).toEqual([{ type: "message", message: decodeItem(encodeMessage(message), "client") }]);
    this.push("cliente", nowMs, decodeCbor(encodeMessage(message)), frames.map(clientPayload), frames);
  }

  private hostSent(nowMs: number, message: Message, payloads: readonly Uint8Array[]): void {
    const { client } = this.session();
    const frames = payloads.map((payload) => channelFrame(CHANNEL, payload));
    const events = frames.flatMap((frame) =>
      client.receive(dataFrame(hostPayload(frame, CHANNEL)), nowMs),
    );
    expect(events).toEqual([{ type: "message", message: decodeItem(encodeMessage(message), "host") }]);
    this.push("host", nowMs, decodeCbor(encodeMessage(message)), payloads, frames);
  }

  /** The host sends `message`; the client's state machine reads it. */
  hostSays(nowMs: number, message: Message): void {
    this.hostSent(nowMs, message, this.session().host.send(message, nowMs));
  }

  /** The host answers the `IK` of a device that is not on its list: `error { not-paired }`. */
  hostRejectsNotPaired(nowMs: number): void {
    const payloads = this.session().host.rejectNotPaired(nowMs);
    this.hostSent(nowMs, { type: "error", code: ErrorCode.notPaired }, payloads);
  }

  /** The host closes the channel: `close-channel { normal }`. */
  hostCloses(nowMs: number): void {
    this.push("host", nowMs, null, [], [closeFrame(CHANNEL)]);
  }

  /** The vector, in the key order of the Rust tool's files. `closing` is the code of a closed conversation. */
  finish(descricao: string, closing: string | null): Record<string, unknown> {
    return {
      descricao,
      versao: [...PROTOCOL_VERSION],
      host_id: this.hostId,
      canal: CHANNEL,
      handshake: this.setup.kind,
      chaves: {
        host_estatica: bstr(this.keys.hostStatic),
        host_efemera: bstr(this.keys.hostEphemeral),
        cliente_estatica: bstr(this.keys.clientStatic),
        cliente_efemera: bstr(this.keys.clientEphemeral),
      },
      token_host: this.setup.tokenHost === null ? null : bstr(this.setup.tokenHost),
      pareados: this.setup.paired.map(bstr),
      passos: this.steps,
      resultado:
        closing === null
          ? { estado: "aberta", erro: null }
          : { estado: "fechada", erro: { camada: "mensagem", codigo: closing } },
    };
  }
}

// ---------------------------------------------------------------------------
// The messages of the conversations
// ---------------------------------------------------------------------------

const hello: Message = {
  type: "hello",
  version: PROTOCOL_VERSION,
  app: "porecatu-remote",
  appVersion: "0.1.0-ts",
};

const welcome: Message = {
  type: "welcome",
  version: PROTOCOL_VERSION,
  hostName: HOST_NAME,
  porecatuVersion: "0.9.11",
  maxSubscriptions: 4,
  hostInstance: seq(0xb0, 16),
};

const theme: Message = {
  type: "theme",
  fg: 0xe6e1cf,
  bg: 0x0f1419,
  cursor: 0xf29718,
  selectionBg: 0x253340,
  palette: Array.from({ length: 16 }, (_, i) => 0x101010 * i + 0x0a0a0a),
  groupColors: [0xff8800, 0x00aaff, 0x55cc55],
};

const profiles: Message = {
  type: "profiles",
  list: [
    { id: "auto:pwsh", name: "PowerShell", kind: ProfileKind.local },
    { id: "ssh:build", name: "Build box", kind: ProfileKind.ssh },
  ],
  defaultProfile: "auto:pwsh",
};

const workspace: Message = {
  type: "workspace",
  rev: 4,
  windows: [
    {
      id: 1,
      title: HOST_NAME,
      pinned: [],
      items: [
        {
          tab: 2,
          title: "build",
          custom: true,
          profile: "auto:pwsh",
          remote: null,
          activity: true,
          bell: false,
          layout: {
            dir: SplitDir.sideBySide,
            ratio: 32768,
            first: {
              pane: 3,
              title: "pwsh",
              cwd: "C:/Users/dev/porecatu",
              state: PaneState.running,
              exit: null,
              cols: 40,
              rows: 2,
            },
            second: {
              pane: 4,
              title: "htop",
              cwd: null,
              state: PaneState.running,
              exit: null,
              cols: 39,
              rows: 2,
            },
          },
          focused: 3,
        },
      ],
      active: 2,
    },
  ],
};

const snapshot: Message = {
  type: "grid-snapshot",
  pane: 3,
  seq: 0,
  cols: 40,
  rows: 2,
  history: 0,
  dropped: 0,
  styles: [
    [null, null, 0],
    [[1, 0xff8800], [0, 4], 0x01],
  ],
  lines: [
    [
      false,
      [
        [0, 1, 0, "$ "],
        [2, 1, 1, "ls -la"],
      ],
    ],
    [
      true,
      [
        [0, 2, 0, "日本"],
        [4, 2, 0, ["👨‍👩‍👧"]],
      ],
    ],
  ],
  cursor: { row: 0, col: 8, shape: CursorShape.beam, visible: true },
  alt: false,
};

const diff = (seqNumber: number, text: string): Message => ({
  type: "grid-diff",
  pane: 3,
  seq: seqNumber,
  history: 0,
  dropped: 0,
  styles: [[null, null, 0]],
  lines: [[1, [false, [[0, 1, 0, text]]]]],
  cursor: { row: 1, col: text.length, shape: CursorShape.block, visible: true },
  alt: false,
});

const otherDevice = testKeypair(7).publicKey;

// ---------------------------------------------------------------------------
// The transcripts
// ---------------------------------------------------------------------------

/** A transcript: where it goes under the vectors root, and its content. */
export interface Transcript {
  readonly path: string;
  readonly vector: Record<string, unknown>;
}

/** An `XX` pairing that the host accepts, then a session with input and a diff. */
function pairingAccepted(): Transcript {
  const token = seq(0xe0, 16);
  const c = new Conversation({ kind: "XX", tokenHost: token, paired: [] });
  c.startXX([0, 14, 31], { token, device: "iPhone 15", platform: Platform.ios });
  // The user confirms the fingerprint on the desktop four seconds later.
  c.hostSays(4_200, { type: "pair-accepted", hostName: HOST_NAME });
  c.clientSays(4_210, hello);
  c.hostSays(4_225, welcome);
  c.hostSays(4_226, theme);
  c.hostSays(4_227, profiles);
  c.hostSays(4_228, workspace);
  c.clientSays(4_300, { type: "subscribe", pane: 3 });
  c.hostSays(4_310, snapshot);
  c.clientSays(4_320, { type: "grid-ack", pane: 3, seq: 0 });
  c.hostSays(4_400, diff(1, "total 0"));
  c.clientSays(4_450, { type: "key", pane: 3, key: NamedKey.enter, mods: 0 });
  c.clientSays(4_460, { type: "key", pane: 3, key: "c", mods: MOD.ctrl });
  c.clientSays(4_470, { type: "text-input", pane: 3, text: "git status" });
  c.clientSays(4_480, { type: "paste", pane: 3, text: "echo ação 🙂" });
  c.hostSays(4_600, diff(2, "ação 🙂"));
  c.hostSays(4_650, { type: "bell", pane: 3 });
  c.clientSays(4_700, { type: "unsubscribe", pane: 3 });
  return {
    path: "sessoes-ts/pareamento-aceito.json",
    vector: c.finish(
      "pareamento XX aceito (transcrição do TypeScript): hello/welcome, theme/profiles/workspace com painel dividido, subscribe, grid-snapshot e grid-diff, teclas, texto, colar, bell e unsubscribe",
      null,
    ),
  };
}

/** An `IK` connection of a paired device, with an action and its result. */
function connectPaired(): Transcript {
  const c = new Conversation({
    kind: "IK",
    tokenHost: null,
    paired: [otherDevice, testKeypair(100).publicKey],
  });
  c.startIK([0, 9]);
  c.clientSays(120, hello);
  c.hostSays(131, welcome);
  c.hostSays(132, theme);
  c.hostSays(133, profiles);
  c.hostSays(134, workspace);
  c.clientSays(200, { type: "subscribe", pane: 3 });
  c.hostSays(215, snapshot);
  c.clientSays(230, { type: "action", id: 7, do: { type: "new-tab", window: 1 } });
  c.hostSays(245, { type: "action-result", id: 7, created: 5 });
  c.clientSays(300, { type: "grid-ack", pane: 3, seq: 0 });
  return {
    path: "sessoes-ts/conexao-ik-pareado.json",
    vector: c.finish(
      "conexão IK de um aparelho pareado (transcrição do TypeScript): hello/welcome, theme/profiles/workspace, subscribe, grid-snapshot, action new-tab com action-result e grid-ack",
      null,
    ),
  };
}

/** An `IK` of a device that is not on the host's list. */
function connectUnknown(): Transcript {
  const c = new Conversation({ kind: "IK", tokenHost: null, paired: [otherDevice] });
  c.startIK([0, 11]);
  // The host completes the handshake first, then its first message is the
  // refusal (spec 02 section 4).
  c.hostRejectsNotPaired(11);
  c.hostCloses(12);
  return {
    path: "sessoes-ts/conexao-ik-desconhecido.json",
    vector: c.finish(
      "conexão IK de um aparelho que não está na lista (transcrição do TypeScript): o handshake completa e a primeira mensagem do host é error not-paired",
      "not-paired",
    ),
  };
}

/** The three transcripts, in the order of their files. */
export function transcripts(): Transcript[] {
  return [pairingAccepted(), connectPaired(), connectUnknown()];
}

/** A transcript as the committed file has it: 2-space JSON and a final newline, like the Rust tool's. */
export function render(vector: Record<string, unknown>): string {
  return `${JSON.stringify(vector, null, 2)}\n`;
}
