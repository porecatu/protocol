// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Noise conformance: the cacophony-format vectors for
 * Noise_XX/IK_25519_ChaChaPoly_BLAKE2s shipped with the snow crate (origin and
 * license in the fixture). The handshake state is driven directly, because
 * these vectors carry non-empty handshake payloads, which the protocol
 * wrappers refuse.
 */

import { describe, expect, it } from "vitest";
import { Transport } from "../src/session/handshake.js";
import { HandshakeState, PATTERN_IK, PATTERN_XX } from "../src/session/handshakeState.js";
import { publicKeyFromPrivate, type StaticKeypair } from "../src/session/keys.js";
import fixtureJson from "./fixtures/cacophony-xx-ik.json" with { type: "json" };

interface VectorMessage {
  payload: string;
  ciphertext: string;
}

interface Vector {
  source_file: string;
  protocol_name: string;
  init_prologue: string;
  init_static: string;
  init_ephemeral: string;
  init_remote_static?: string;
  resp_prologue: string;
  resp_static: string;
  resp_ephemeral: string;
  handshake_hash?: string;
  messages: VectorMessage[];
}

interface Fixture {
  source: string;
  license: string;
  vectors: Vector[];
}

const fixture: Fixture = fixtureJson;

function hex(text: string): Uint8Array {
  if (text.length % 2 !== 0) {
    throw new Error("odd hex");
  }
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = Number.parseInt(text.slice(2 * i, 2 * i + 2), 16);
  }
  return out;
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function keypair(privateHex: string): StaticKeypair {
  const privateKey = hex(privateHex);
  return { privateKey, publicKey: publicKeyFromPrivate(privateKey) };
}

const noRandom = (): Uint8Array => {
  throw new Error("vectors use fixed ephemeral keys");
};

function patternOf(name: string) {
  if (name.startsWith("Noise_XX_")) {
    return PATTERN_XX;
  }
  if (name.startsWith("Noise_IK_")) {
    return PATTERN_IK;
  }
  throw new Error(`unexpected protocol ${name}`);
}

describe("cacophony vectors", () => {
  it("covers both protocols, from cacophony itself", () => {
    const fromCacophony = fixture.vectors.filter((v) => v.source_file.endsWith("cacophony.txt"));
    expect(fromCacophony.map((v) => v.protocol_name).sort()).toEqual([
      "Noise_IK_25519_ChaChaPoly_BLAKE2s",
      "Noise_XX_25519_ChaChaPoly_BLAKE2s",
    ]);
  });

  for (const vector of fixture.vectors) {
    it(`${vector.protocol_name} (${vector.source_file})`, () => {
      const pattern = patternOf(vector.protocol_name);
      const initiator = new HandshakeState({
        protocolName: vector.protocol_name,
        pattern,
        initiator: true,
        prologue: hex(vector.init_prologue),
        s: keypair(vector.init_static),
        ...(vector.init_remote_static === undefined ? {} : { rs: hex(vector.init_remote_static) }),
        random: noRandom,
        ephemeral: hex(vector.init_ephemeral),
      });
      const responder = new HandshakeState({
        protocolName: vector.protocol_name,
        pattern,
        initiator: false,
        prologue: hex(vector.resp_prologue),
        s: keypair(vector.resp_static),
        random: noRandom,
        ephemeral: hex(vector.resp_ephemeral),
      });

      let initiatorTransport: Transport | null = null;
      let responderTransport: Transport | null = null;

      vector.messages.forEach((message, index) => {
        const payload = hex(message.payload);
        const fromInitiator = index % 2 === 0;
        let ciphertext: Uint8Array;
        let received: Uint8Array;
        if (initiatorTransport === null || responderTransport === null) {
          const [sender, receiver] = fromInitiator ? [initiator, responder] : [responder, initiator];
          ciphertext = sender.writeMessage(payload);
          received = receiver.readMessage(ciphertext);
          if (initiator.isComplete && responder.isComplete) {
            initiatorTransport = new Transport(initiator.result());
            responderTransport = new Transport(responder.result());
          }
        } else {
          const [sender, receiver] = fromInitiator
            ? [initiatorTransport, responderTransport]
            : [responderTransport, initiatorTransport];
          ciphertext = sender.encrypt(payload);
          received = receiver.decrypt(ciphertext);
        }
        expect(toHex(ciphertext), `message ${index}`).toBe(message.ciphertext);
        expect(toHex(received), `payload ${index}`).toBe(message.payload);
      });

      expect(initiatorTransport).not.toBeNull();
      const it_ = initiatorTransport as unknown as Transport;
      const rt = responderTransport as unknown as Transport;
      if (vector.handshake_hash !== undefined) {
        expect(toHex(it_.handshakeHash)).toBe(vector.handshake_hash);
        expect(toHex(rt.handshakeHash)).toBe(vector.handshake_hash);
      } else {
        expect(toHex(it_.handshakeHash)).toBe(toHex(rt.handshakeHash));
      }
      expect(toHex(it_.remoteStatic)).toBe(toHex(keypair(vector.resp_static).publicKey));
      expect(toHex(rt.remoteStatic)).toBe(toHex(keypair(vector.init_static).publicKey));
    });
  }
});
