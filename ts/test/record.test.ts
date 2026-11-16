// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { ClientHandshake, HandshakeKind, HostHandshake, type Transport } from "../src/session/handshake.js";
import { generateStaticKeypair, type RandomSource } from "../src/session/keys.js";
import {
  DEFAULT_REKEY_POLICY,
  FLAG_CONTINUE,
  FLAG_REKEY,
  MAX_FRAGMENT,
  MAX_MESSAGE,
  RecordError,
  Session,
  type RecordErrorCode,
  type SessionOptions,
} from "../src/session/record.js";
import { newRekeyState, shouldRekey } from "../src/session/rekey.js";

const HOST_ID = "k3x9abcdefghijklmnopqrstuv";

/** Deterministic test randomness (not cryptographic). */
function seededRandom(seed: number): RandomSource {
  let state = seed >>> 0;
  return (n) => {
    const out = new Uint8Array(n);
    for (let i = 0; i < n; i++) {
      state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
      out[i] = state >>> 24;
    }
    return out;
  };
}

/** A real `IK` handshake; returns the two transports. */
function transports(seed = 1): { client: Transport; host: Transport } {
  const clientKey = generateStaticKeypair(seededRandom(seed));
  const hostKey = generateStaticKeypair(seededRandom(seed + 1));
  const { handshake, msg1 } = ClientHandshake.connect({
    local: clientKey,
    hostId: HOST_ID,
    hostKey: hostKey.publicKey,
    random: seededRandom(seed + 2),
  });
  const host = HostHandshake.respond({
    kind: HandshakeKind.connect,
    local: hostKey,
    hostId: HOST_ID,
    msg1,
    random: seededRandom(seed + 3),
  });
  const step = handshake.readMessage2(host.msg2);
  if (step.kind !== "established") {
    throw new Error("expected an established transport");
  }
  return { client: step.transport, host: host.transport };
}

function sessions(
  clientOptions?: SessionOptions,
  hostOptions?: SessionOptions,
): { client: Session; host: Session } {
  const { client, host } = transports();
  return { client: new Session(client, clientOptions), host: new Session(host, hostOptions) };
}

function patterned(length: number): Uint8Array {
  const out = new Uint8Array(length);
  for (let i = 0; i < length; i++) {
    out[i] = (i * 31 + (i >> 8)) & 0xff;
  }
  return out;
}

/** Feeds every record to `receiver`; returns the messages completed along the way. */
function deliver(receiver: Session, records: Uint8Array[], nowMs = 0): Uint8Array[] {
  const out: Uint8Array[] = [];
  for (const record of records) {
    const message = receiver.open(record, nowMs);
    if (message !== null) {
      out.push(message);
    }
  }
  return out;
}

function expectCode(fn: () => unknown, code: RecordErrorCode): void {
  try {
    fn();
  } catch (error) {
    expect(error).toBeInstanceOf(RecordError);
    expect((error as RecordError).code).toBe(code);
    return;
  }
  throw new Error(`expected RecordError ${code}`);
}

describe("constants", () => {
  it("match the specification", () => {
    expect(FLAG_CONTINUE).toBe(0x01);
    expect(FLAG_REKEY).toBe(0x02);
    expect(MAX_FRAGMENT).toBe(65_518);
    expect(MAX_MESSAGE).toBe(16_777_216);
    expect(DEFAULT_REKEY_POLICY).toEqual({ maxRecords: 100_000, maxAgeMs: 3_600_000 });
  });
});

describe("fragmentation", () => {
  const cases: [string, number, number][] = [
    ["an empty message", 0, 1],
    ["one byte", 1, 1],
    ["exactly one full fragment", 65_518, 1],
    ["one byte past one fragment", 65_519, 2],
    ["exactly two fragments", 2 * 65_518, 2],
    ["200 000 bytes", 200_000, 4],
  ];
  for (const [name, length, records] of cases) {
    it(`round-trips ${name} in ${records} record(s)`, () => {
      const { client, host } = sessions();
      const message = patterned(length);
      const sealed = client.seal(message, 0);
      expect(sealed).toHaveLength(records);
      for (const record of sealed) {
        expect(record.length).toBeLessThanOrEqual(65_535);
      }
      const received = deliver(host, sealed);
      expect(received).toHaveLength(1);
      expect(received[0]).toEqual(message);
    });
  }

  it("returns null until the last record", () => {
    const { client, host } = sessions();
    const sealed = client.seal(patterned(200_000), 0);
    for (const record of sealed.slice(0, -1)) {
      expect(host.open(record, 0)).toBeNull();
    }
    expect(host.open(sealed[sealed.length - 1] as Uint8Array, 0)).toHaveLength(200_000);
  });

  it("carries messages in both directions, one after another", () => {
    const { client, host } = sessions();
    for (let i = 0; i < 5; i++) {
      const up = patterned(70_000 + i);
      expect(deliver(host, client.seal(up, i))).toEqual([up]);
      const down = patterned(i);
      expect(deliver(client, host.seal(down, i))).toEqual([down]);
    }
  });

  it("exposes the authenticated peer key", () => {
    const clientKey = generateStaticKeypair(seededRandom(1));
    const hostKey = generateStaticKeypair(seededRandom(2));
    const { client, host } = sessions();
    expect(client.remoteStatic).toEqual(hostKey.publicKey);
    expect(host.remoteStatic).toEqual(clientKey.publicKey);
  });

  it("marks CONTINUE on every record but the last", () => {
    const { client, host } = transports();
    const session = new Session(client);
    const sealed = session.seal(patterned(200_000), 0);
    const flags = sealed.map((record) => host.decrypt(record)[0]);
    expect(flags).toEqual([FLAG_CONTINUE, FLAG_CONTINUE, FLAG_CONTINUE, 0]);
  });

  it("refuses a message above 16 MiB and accepts exactly 16 MiB", () => {
    const { client, host } = sessions();
    const big = new Uint8Array(MAX_MESSAGE);
    const sealed = client.seal(big, 0);
    expect(deliver(host, sealed)[0]?.length).toBe(MAX_MESSAGE);
    const { client: other } = sessions();
    expectCode(() => other.seal(new Uint8Array(MAX_MESSAGE + 1), 0), "too-large");
    expectCode(() => other.seal(new Uint8Array(0), 0), "failed");
  });
});

describe("rekey policy", () => {
  it("fires by count, counting the record being sealed", () => {
    const policy = { maxRecords: 3, maxAgeMs: 1_000_000 };
    const state = newRekeyState(0);
    expect(shouldRekey(policy, state, 0)).toBe(false);
    state.sentSinceRekey = 1;
    expect(shouldRekey(policy, state, 0)).toBe(false);
    state.sentSinceRekey = 2;
    expect(shouldRekey(policy, state, 0)).toBe(true);
  });

  it("fires by age, from the start of the current key", () => {
    const policy = { maxRecords: 1_000, maxAgeMs: 100 };
    const state = newRekeyState(50);
    expect(shouldRekey(policy, state, 149)).toBe(false);
    expect(shouldRekey(policy, state, 150)).toBe(true);
    expect(shouldRekey(policy, newRekeyState(), 10_000)).toBe(false);
  });
});

describe("rekey on the wire", () => {
  it("flags REKEY on the third record and swaps the key after encrypting it", () => {
    const { client, host } = transports();
    const session = new Session(client, { policy: { maxRecords: 3, maxAgeMs: 1_000_000 } });
    const records = [1, 2, 3, 4].map((i) => session.seal(Uint8Array.of(i), 0)[0] as Uint8Array);
    const flags: (number | undefined)[] = [];
    for (const record of records) {
      const plain = host.decrypt(record);
      const f = plain[0];
      flags.push(f);
      if (f !== undefined && (f & FLAG_REKEY) !== 0) {
        host.rekeyIncoming();
      }
    }
    // Four records decrypt in order only if the host followed the rekey.
    expect(flags).toEqual([0, 0, FLAG_REKEY, 0]);
  });

  it("keeps both sides in step across many rekeys by count", () => {
    const policy = { maxRecords: 3, maxAgeMs: 1_000_000 };
    const { client, host } = sessions({ policy }, { policy });
    for (let i = 0; i < 20; i++) {
      const up = patterned(10 + i);
      expect(deliver(host, client.seal(up, i))).toEqual([up]);
      const down = patterned(20 + i);
      expect(deliver(client, host.seal(down, i))).toEqual([down]);
    }
  });

  it("rekeys in the middle of a fragmented message", () => {
    const policy = { maxRecords: 2, maxAgeMs: 1_000_000 };
    const { client, host } = sessions({ policy });
    const message = patterned(200_000);
    expect(deliver(host, client.seal(message, 0))).toEqual([message]);
    expect(deliver(host, client.seal(message, 0))).toEqual([message]);
  });

  it("rekeys by age", () => {
    const policy = { maxRecords: 1_000_000, maxAgeMs: 1_000 };
    const { client, host } = transports();
    const session = new Session(client, { policy, nowMs: 100 });
    const flagsOf = (nowMs: number): number | undefined => {
      const record = session.seal(Uint8Array.of(7), nowMs)[0] as Uint8Array;
      const f = host.decrypt(record)[0];
      if (f !== undefined && (f & FLAG_REKEY) !== 0) {
        host.rekeyIncoming();
      }
      return f;
    };
    expect(flagsOf(100)).toBe(0);
    expect(flagsOf(1_099)).toBe(0);
    expect(flagsOf(1_100)).toBe(FLAG_REKEY);
    // The age restarts at the rekey.
    expect(flagsOf(2_000)).toBe(0);
    expect(flagsOf(2_100)).toBe(FLAG_REKEY);
  });

  it("counts the age from the first seal when no start time is given", () => {
    const policy = { maxRecords: 1_000_000, maxAgeMs: 1_000 };
    const { client, host } = sessions({ policy }, { policy });
    expect(deliver(host, client.seal(Uint8Array.of(1), 5_000))).toEqual([Uint8Array.of(1)]);
    expect(deliver(host, client.seal(Uint8Array.of(2), 5_999))).toEqual([Uint8Array.of(2)]);
    expect(deliver(host, client.seal(Uint8Array.of(3), 6_000))).toEqual([Uint8Array.of(3)]);
    expect(deliver(host, client.seal(Uint8Array.of(4), 6_001))).toEqual([Uint8Array.of(4)]);
  });
});

describe("errors are fatal", () => {
  it("a reserved flag bit closes the session", () => {
    for (const bit of [0x04, 0x08, 0x10, 0x20, 0x40, 0x80]) {
      const { client, host } = transports();
      const session = new Session(host);
      const plain = Uint8Array.of(bit, 1, 2, 3);
      expectCode(() => session.open(client.encrypt(plain), 0), "reserved-flag");
      expectCode(() => session.open(client.encrypt(Uint8Array.of(0)), 0), "failed");
      expectCode(() => session.seal(Uint8Array.of(1), 0), "failed");
    }
  });

  it("a tampered record is a decrypt error and the session stays unusable", () => {
    const { client, host } = sessions();
    const [record] = client.seal(Uint8Array.of(1, 2, 3), 0) as [Uint8Array];
    const bad = record.slice();
    bad[bad.length - 1] = (bad[bad.length - 1] as number) ^ 1;
    expect(host.failed).toBe(false);
    expectCode(() => host.open(bad, 0), "decrypt");
    expect(host.failed).toBe(true);
    // Even the genuine record is refused now.
    expectCode(() => host.open(record, 0), "failed");
    expectCode(() => host.seal(Uint8Array.of(1), 0), "failed");
  });

  it("a record with no room for the flags byte is a decrypt error", () => {
    const { client, host } = transports();
    const session = new Session(host);
    expectCode(() => session.open(client.encrypt(new Uint8Array(0)), 0), "decrypt");
  });

  it("a record that is too short or too long is a decrypt error", () => {
    expectCode(() => sessions().host.open(new Uint8Array(3), 0), "decrypt");
    expectCode(() => sessions().host.open(new Uint8Array(65_536), 0), "decrypt");
  });

  it("reassembly above 16 MiB closes the session", () => {
    const { client, host } = transports();
    const receiver = new Session(host);
    const fragment = new Uint8Array(1 + MAX_FRAGMENT);
    fragment[0] = FLAG_CONTINUE;
    // This many full fragments still fit under the limit; one more overflows it.
    const fits = Math.floor(MAX_MESSAGE / MAX_FRAGMENT);
    for (let i = 0; i < fits; i++) {
      expect(receiver.open(client.encrypt(fragment), 0)).toBeNull();
    }
    expectCode(() => receiver.open(client.encrypt(fragment), 0), "too-large");
    expectCode(() => receiver.open(client.encrypt(Uint8Array.of(0)), 0), "failed");
  });

  it("a fragmented message of exactly 16 MiB reassembles", () => {
    const { client, host } = transports();
    const receiver = new Session(host);
    const fragment = new Uint8Array(1 + MAX_FRAGMENT);
    fragment[0] = FLAG_CONTINUE;
    const fits = Math.floor(MAX_MESSAGE / MAX_FRAGMENT);
    for (let i = 0; i < fits; i++) {
      expect(receiver.open(client.encrypt(fragment), 0)).toBeNull();
    }
    const rest = MAX_MESSAGE - fits * MAX_FRAGMENT;
    const last = receiver.open(client.encrypt(new Uint8Array(1 + rest)), 0);
    expect(last?.length).toBe(MAX_MESSAGE);
  });

  it("a failure in the middle of a message drops the partial message", () => {
    const { client, host } = sessions();
    const sealed = client.seal(patterned(200_000), 0);
    expect(host.open(sealed[0] as Uint8Array, 0)).toBeNull();
    const bad = (sealed[1] as Uint8Array).slice();
    bad[0] = (bad[0] as number) ^ 0xff;
    expectCode(() => host.open(bad, 0), "decrypt");
    expectCode(() => host.open(sealed[2] as Uint8Array, 0), "failed");
  });
});

describe("destroy", () => {
  it("closes the session: seal and open throw `failed`", () => {
    const { client, host } = sessions();
    const sealed = client.seal(new Uint8Array([1, 2, 3]), 0);
    host.destroy();
    expect(host.failed).toBe(true);
    expectCode(() => host.open(sealed[0] as Uint8Array, 0), "failed");
    expectCode(() => host.seal(new Uint8Array([1]), 0), "failed");
  });

  it("is idempotent and also valid after a failure", () => {
    const { client, host } = sessions();
    client.destroy();
    client.destroy();
    expect(client.failed).toBe(true);
    const bad = new Uint8Array(40);
    expectCode(() => host.open(bad, 0), "decrypt");
    host.destroy();
    expect(host.failed).toBe(true);
  });

  it("drops a partially reassembled message", () => {
    const { client, host } = sessions();
    const sealed = client.seal(patterned(200_000), 0);
    expect(host.open(sealed[0] as Uint8Array, 0)).toBeNull();
    host.destroy();
    expectCode(() => host.open(sealed[1] as Uint8Array, 0), "failed");
  });
});
