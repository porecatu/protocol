// SPDX-License-Identifier: GPL-3.0-or-later

import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { describe, expect, it } from "vitest";
import { CipherState } from "../src/session/cipherState.js";
import {
  ClientHandshake,
  HandshakeError,
  HandshakeKind,
  HostHandshake,
  MAX_NOISE_MESSAGE,
  MAX_PLAINTEXT,
  NOISE_IK,
  NOISE_XX,
  prologue,
  type HandshakeErrorCode,
  type Transport,
} from "../src/session/handshake.js";
import { HandshakeState, PATTERN_IK, PATTERN_XX } from "../src/session/handshakeState.js";
import {
  defaultRandom,
  generateStaticKeypair,
  publicKeyFromPrivate,
  type RandomSource,
} from "../src/session/keys.js";
import { hkdf } from "../src/session/symmetricState.js";

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

function expectCode(fn: () => unknown, code: HandshakeErrorCode): void {
  try {
    fn();
  } catch (error) {
    expect(error).toBeInstanceOf(HandshakeError);
    expect((error as HandshakeError).code).toBe(code);
    return;
  }
  throw new Error(`expected HandshakeError ${code}`);
}

const text = (s: string): Uint8Array => Uint8Array.from(s, (c) => c.charCodeAt(0));

function exchange(a: Transport, b: Transport): void {
  for (let i = 0; i < 3; i++) {
    const ab = text(`client->host ${i}`);
    expect(b.decrypt(a.encrypt(ab))).toEqual(ab);
    const ba = text(`host->client ${i}`);
    expect(a.decrypt(b.encrypt(ba))).toEqual(ba);
  }
}

function exchangeWithRekey(client: Transport, host: Transport): void {
  exchange(client, host);
  // Client rekeys its sending cipher; the host follows on its receiving one.
  client.rekeyOutgoing();
  host.rekeyIncoming();
  // And the reverse direction.
  host.rekeyOutgoing();
  client.rekeyIncoming();
  exchange(client, host);
  // A one-sided rekey desynchronizes the direction.
  client.rekeyOutgoing();
  expectCode(() => host.decrypt(client.encrypt(text("x"))), "decrypt");
}

function pair(seed = 1) {
  const clientKey = generateStaticKeypair(seededRandom(seed));
  const hostKey = generateStaticKeypair(seededRandom(seed + 100));
  const { handshake, msg1 } = ClientHandshake.pair({
    local: clientKey,
    expectedHostKey: hostKey.publicKey,
    hostId: HOST_ID,
    random: seededRandom(seed + 200),
  });
  return { clientKey, hostKey, client: handshake, msg1 };
}

describe("constants and prologue", () => {
  it("names the protocols and limits", () => {
    expect(NOISE_XX).toBe("Noise_XX_25519_ChaChaPoly_BLAKE2s");
    expect(NOISE_IK).toBe("Noise_IK_25519_ChaChaPoly_BLAKE2s");
    expect(MAX_NOISE_MESSAGE).toBe(65_535);
    expect(MAX_PLAINTEXT).toBe(65_519);
    expect(HandshakeKind).toEqual({ pair: 0, connect: 1 });
  });

  it("builds the ASCII prologue", () => {
    expect(prologue("abc")).toEqual(text("porecatu-remote/0 abc"));
    expectCode(() => prologue("é"), "malformed");
  });
});

describe("keys", () => {
  it("derives the public key and uses the default random source", () => {
    const pair1 = generateStaticKeypair();
    expect(pair1.privateKey).toHaveLength(32);
    expect(pair1.publicKey).toEqual(publicKeyFromPrivate(pair1.privateKey));
    expect(defaultRandom(70_000)).toHaveLength(70_000);
  });

  it("matches the RFC 7748 §6.1 X25519 public key", () => {
    const alicePrivate = Uint8Array.from(
      "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a".match(/../g) ?? [],
      (h) => parseInt(h, 16),
    );
    const alicePublic = Array.from(publicKeyFromPrivate(alicePrivate), (b) =>
      b.toString(16).padStart(2, "0"),
    ).join("");
    expect(alicePublic).toBe("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a");
  });
});

describe("CipherState", () => {
  const key = new Uint8Array(32).fill(7);

  it("uses the ChaChaPoly nonce: 4 zero bytes then n as 64-bit little endian", () => {
    const cs = new CipherState();
    cs.initializeKey(key);
    cs.setNonce(0x0102_0304_0506_0708n);
    const ad = text("ad");
    const plaintext = text("hello");
    const expected = chacha20poly1305(
      key,
      Uint8Array.of(0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1),
      ad,
    ).encrypt(plaintext);
    expect(cs.encryptWithAd(ad, plaintext)).toEqual(expected);
    expect(cs.nonce).toBe(0x0102_0304_0506_0709n);
  });

  it("rekeys to the first 32 bytes of ENCRYPT(k, 2^64-1, empty, zeros)", () => {
    const cs = new CipherState();
    cs.initializeKey(key);
    cs.setNonce(5n);
    cs.rekey();
    const newKey = chacha20poly1305(
      key,
      Uint8Array.of(0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff),
      new Uint8Array(0),
    )
      .encrypt(new Uint8Array(32))
      .slice(0, 32);
    const reference = new CipherState();
    reference.initializeKey(newKey);
    reference.setNonce(5n);
    expect(cs.nonce).toBe(5n);
    expect(cs.encryptWithAd(new Uint8Array(0), text("x"))).toEqual(
      reference.encryptWithAd(new Uint8Array(0), text("x")),
    );
  });

  it("keeps n on decryption failure and refuses n = 2^64 - 1", () => {
    const a = new CipherState();
    const b = new CipherState();
    a.initializeKey(key);
    b.initializeKey(key);
    const ct = a.encryptWithAd(new Uint8Array(0), text("m"));
    const bad = ct.slice();
    bad[0] = (bad[0] ?? 0) ^ 1;
    expectCode(() => b.decryptWithAd(new Uint8Array(0), bad), "decrypt");
    expect(b.nonce).toBe(0n);
    expect(b.decryptWithAd(new Uint8Array(0), ct)).toEqual(text("m"));
    expect(b.nonce).toBe(1n);

    a.setNonce(0xffff_ffff_ffff_fffen);
    a.encryptWithAd(new Uint8Array(0), text("last"));
    expect(a.nonce).toBe(0xffff_ffff_ffff_ffffn);
    expectCode(() => a.encryptWithAd(new Uint8Array(0), text("x")), "wrong-state");
    b.setNonce(0xffff_ffff_ffff_ffffn);
    expectCode(() => b.decryptWithAd(new Uint8Array(0), ct), "wrong-state");
  });

  it("passes data through without a key", () => {
    const cs = new CipherState();
    cs.initializeKey(null);
    expect(cs.encryptWithAd(text("h"), text("plain"))).toEqual(text("plain"));
  });
});

describe("Noise HKDF", () => {
  it("chains outputs as in Noise §4.3", () => {
    const [o1, o2, o3] = hkdf(new Uint8Array(32).fill(1), text("ikm"), 3);
    const [p1, p2] = hkdf(new Uint8Array(32).fill(1), text("ikm"), 2);
    expect(o1).toEqual(p1);
    expect(o2).toEqual(p2);
    expect(o3).toHaveLength(32);
    expect(o3).not.toEqual(o2);
  });
});

describe("pairing (XX)", () => {
  it("completes, delivers the pair-request and exchanges transport messages", () => {
    const { clientKey, hostKey, client, msg1 } = pair();
    expect(msg1).toHaveLength(32);
    const host = HostHandshake.respond({
      kind: HandshakeKind.pair,
      local: hostKey,
      hostId: HOST_ID,
      msg1,
      random: seededRandom(300),
    });
    expect(host.kind).toBe(HandshakeKind.pair);
    expect(client.readMessage2(host.msg2)).toEqual({ kind: "needs-pair-request" });

    const request = text("pair-request bytes");
    const { msg3, transport: clientTransport } = client.writeMessage3(request);
    const done = host.handshake.readMessage3(msg3);
    expect(done.payload).toEqual(request);
    expect(done.remoteStatic).toEqual(clientKey.publicKey);
    expect(clientTransport.remoteStatic).toEqual(hostKey.publicKey);
    expect(clientTransport.handshakeHash).toEqual(done.transport.handshakeHash);

    exchangeWithRekey(clientTransport, done.transport);

    // Terminal: the handshakes cannot be reused.
    expectCode(() => client.writeMessage3(request), "wrong-state");
    expectCode(() => host.handshake.readMessage3(msg3), "wrong-state");
  });

  it("aborts with host-key-mismatch before msg3", () => {
    const clientKey = generateStaticKeypair(seededRandom(1));
    const hostKey = generateStaticKeypair(seededRandom(2));
    const otherKey = generateStaticKeypair(seededRandom(3));
    const { handshake, msg1 } = ClientHandshake.pair({
      local: clientKey,
      expectedHostKey: otherKey.publicKey,
      hostId: HOST_ID,
      random: seededRandom(4),
    });
    const host = HostHandshake.respond({ kind: 0, local: hostKey, hostId: HOST_ID, msg1 });
    expect(host.kind).toBe(HandshakeKind.pair);
    expectCode(() => handshake.readMessage2(host.msg2), "host-key-mismatch");
    expectCode(() => handshake.writeMessage3(text("req")), "wrong-state");
  });

  it("fails on a different prologue", () => {
    const { hostKey, client, msg1 } = pair(5);
    const host = HostHandshake.respond({
      kind: HandshakeKind.pair,
      local: hostKey,
      hostId: "another-host-id",
      msg1,
    });
    expectCode(() => client.readMessage2(host.msg2), "decrypt");
  });

  it("refuses a zero ephemeral key from the client (DH of zeros)", () => {
    const hostKey = generateStaticKeypair(seededRandom(6));
    expectCode(
      () =>
        HostHandshake.respond({
          kind: HandshakeKind.pair,
          local: hostKey,
          hostId: HOST_ID,
          msg1: new Uint8Array(32),
        }),
      "dh-zero",
    );
  });

  it("refuses a tampered msg3 and a non-empty msg1 payload", () => {
    const { hostKey, client, msg1 } = pair(7);
    const host = HostHandshake.respond({ kind: HandshakeKind.pair, local: hostKey, hostId: HOST_ID, msg1 });
    client.readMessage2(host.msg2);
    const { msg3 } = client.writeMessage3(text("req"));
    const bad = msg3.slice();
    bad[bad.length - 1] = (bad[bad.length - 1] ?? 0) ^ 0x80;
    expectCode(() => host.handshake.readMessage3(bad), "decrypt");
    expectCode(() => host.handshake.readMessage3(msg3), "wrong-state");

    const raw = new HandshakeState({
      protocolName: NOISE_XX,
      pattern: PATTERN_XX,
      initiator: true,
      prologue: prologue(HOST_ID),
      s: generateStaticKeypair(seededRandom(8)),
      random: seededRandom(9),
    });
    const withPayload = raw.writeMessage(text("not empty"));
    expectCode(
      () => HostHandshake.respond({ kind: 0, local: hostKey, hostId: HOST_ID, msg1: withPayload }),
      "malformed",
    );
  });

  it("refuses an unknown handshake kind and short messages", () => {
    const hostKey = generateStaticKeypair(seededRandom(10));
    expectCode(
      () => HostHandshake.respond({ kind: 2, local: hostKey, hostId: HOST_ID, msg1: new Uint8Array(32) }),
      "malformed",
    );
    expectCode(
      () => HostHandshake.respond({ kind: 0, local: hostKey, hostId: HOST_ID, msg1: new Uint8Array(10) }),
      "malformed",
    );
  });
});

describe("connection (IK)", () => {
  function connect(seed: number) {
    const clientKey = generateStaticKeypair(seededRandom(seed));
    const hostKey = generateStaticKeypair(seededRandom(seed + 1));
    const { handshake, msg1 } = ClientHandshake.connect({
      local: clientKey,
      hostKey: hostKey.publicKey,
      hostId: HOST_ID,
      random: seededRandom(seed + 2),
    });
    return { clientKey, hostKey, client: handshake, msg1 };
  }

  it("completes in one round trip and exchanges transport messages", () => {
    const { clientKey, hostKey, client, msg1 } = connect(20);
    // e (32) + encrypted s (32 + 16) + encrypted empty payload (16).
    expect(msg1).toHaveLength(96);
    const host = HostHandshake.respond({
      kind: HandshakeKind.connect,
      local: hostKey,
      hostId: HOST_ID,
      msg1,
      random: seededRandom(23),
    });
    expect(host.remoteStatic).toEqual(clientKey.publicKey);
    // e (32) + encrypted empty payload (16).
    expect(host.msg2).toHaveLength(48);
    const step = client.readMessage2(host.msg2);
    if (step.kind !== "established") {
      throw new Error("expected an established transport");
    }
    expect(step.transport.remoteStatic).toEqual(hostKey.publicKey);
    expect(step.transport.handshakeHash).toEqual(host.transport.handshakeHash);
    exchangeWithRekey(step.transport, host.transport);
    expectCode(() => client.readMessage2(host.msg2), "wrong-state");
  });

  it("completes even for an unknown device", () => {
    const { client, hostKey, msg1 } = connect(30);
    const host = HostHandshake.respond({ kind: 1, local: hostKey, hostId: HOST_ID, msg1 });
    expect(host.kind).toBe(HandshakeKind.connect);
    expect(client.readMessage2(host.msg2).kind).toBe("established");
  });

  it("fails on a different prologue", () => {
    const { hostKey, msg1 } = connect(40);
    expectCode(
      () => HostHandshake.respond({ kind: 1, local: hostKey, hostId: "another-host-id", msg1 }),
      "decrypt",
    );
  });

  it("fails when the host key is not the one the client expects", () => {
    const { msg1 } = connect(50);
    const wrongHost = generateStaticKeypair(seededRandom(99));
    expectCode(() => HostHandshake.respond({ kind: 1, local: wrongHost, hostId: HOST_ID, msg1 }), "decrypt");
  });

  it("refuses a host key of 32 zeros (DH of zeros)", () => {
    expectCode(
      () =>
        ClientHandshake.connect({
          local: generateStaticKeypair(seededRandom(60)),
          hostKey: new Uint8Array(32),
          hostId: HOST_ID,
          random: seededRandom(61),
        }),
      "dh-zero",
    );
  });

  it("refuses a non-empty msg2 payload", () => {
    const { client, hostKey, msg1 } = connect(70);
    const raw = new HandshakeState({
      protocolName: NOISE_IK,
      pattern: PATTERN_IK,
      initiator: false,
      prologue: prologue(HOST_ID),
      s: hostKey,
      random: seededRandom(71),
    });
    raw.readMessage(msg1);
    const msg2 = raw.writeMessage(text("not empty"));
    expectCode(() => client.readMessage2(msg2), "malformed");
  });
});

describe("transport limits", () => {
  it("bounds plaintext and ciphertext sizes", () => {
    const { hostKey, client, msg1 } = pair(80);
    const host = HostHandshake.respond({ kind: HandshakeKind.pair, local: hostKey, hostId: HOST_ID, msg1 });
    client.readMessage2(host.msg2);
    const { msg3, transport } = client.writeMessage3(text("r"));
    const { transport: hostTransport } = host.handshake.readMessage3(msg3);

    const max = transport.encrypt(new Uint8Array(MAX_PLAINTEXT));
    expect(max).toHaveLength(MAX_NOISE_MESSAGE);
    expect(hostTransport.decrypt(max)).toHaveLength(MAX_PLAINTEXT);
    expectCode(() => transport.encrypt(new Uint8Array(MAX_PLAINTEXT + 1)), "malformed");
    expectCode(() => hostTransport.decrypt(new Uint8Array(MAX_NOISE_MESSAGE + 1)), "malformed");
    expectCode(() => hostTransport.decrypt(new Uint8Array(5)), "decrypt");
  });
});
