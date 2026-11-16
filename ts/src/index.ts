// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * `@porecatu/protocol`: reference TypeScript implementation of the Porecatu
 * remote-access protocol (specification in `docs/especificacao/`).
 *
 * Pure and sans-IO: bytes and a monotonic `now` (ms) go in, bytes and events
 * come out. No sockets, timers, storage or Node APIs.
 */

/** Wire protocol version implemented by this package, as `[major, minor]`. */
export const PROTOCOL_VERSION = [0, 1] as const;

// Identity: host_id, fingerprint, pairing URI, relay URLs (spec 01 §2–§3, 02 §1, §3.1).
export * from "./ct.js";
export * from "./encoding.js";
export * from "./fingerprint.js";
export * from "./hostId.js";
export * from "./pairingUri.js";

// Layer 2: relay envelope (spec 01 §4) and handshake-start (spec 02 §2).
export * from "./envelope.js";
// Both modules define the same 32-byte constant; this explicit export resolves the ambiguity.
export { ROUTING_KEY_LEN } from "./hostId.js";

// Layer 3: Noise handshakes, transport and records (spec 02).
export * from "./session/handshake.js";
export * from "./session/keys.js";
export * from "./session/record.js";
export * from "./session/client.js";
export * from "./session/host.js";
export * from "./session/pairingToken.js";

// Layer 4: application messages (spec 03).
export * from "./message/index.js";
// The `pair-request` body of handshake message 3 (spec 02 §3), not a message.
export { decodePairRequest, encodePairRequest } from "./message/pairing.js";
