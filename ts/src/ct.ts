// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Constant-time comparison for secrets and identities: the host key from the
 * pairing QR (spec 02 §3.2) and the pairing token. Arrays of different lengths
 * compare unequal; lengths are never secret in this package.
 */
export function ctEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) {
    return false;
  }
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  }
  return diff === 0;
}
