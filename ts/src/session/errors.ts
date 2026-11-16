// SPDX-License-Identifier: GPL-3.0-or-later

/** Error codes of the session layer (handshake and transport). No prose crosses the wire. */
export type HandshakeErrorCode =
  | "host-key-mismatch"
  | "dh-zero"
  | "decrypt"
  | "malformed"
  | "wrong-state";

/**
 * Failure of a Noise handshake or transport operation (spec 02 §2).
 *
 * - `host-key-mismatch`: the static key revealed by the host in the pairing
 *   `msg2` differs from the key read from the QR code (spec 02 §3.2).
 * - `dh-zero`: a DH operation produced 32 zero bytes, or the peer key is a
 *   low-order point the curve library refuses up front (spec 02 §2).
 * - `decrypt`: authentication failure of a handshake or transport message.
 * - `malformed`: a message of the wrong size or shape.
 * - `wrong-state`: the operation is not valid in the current state (including
 *   any use of a handshake after a failure, and nonce exhaustion).
 *
 * The message is the code itself; user-facing text belongs to the consumers.
 */
export class HandshakeError extends Error {
  readonly code: HandshakeErrorCode;

  constructor(code: HandshakeErrorCode) {
    super(code);
    this.name = "HandshakeError";
    this.code = code;
  }
}
