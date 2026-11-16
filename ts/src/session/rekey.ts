// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Rekey schedule of the sending side (spec 02 §5): each side marks `REKEY`
 * on a record and swaps its sending key at least every 100 000 records sent
 * or every 60 minutes, whichever comes first.
 *
 * Pure rule over a `nowMs` received from outside (monotonic milliseconds);
 * this module never reads a clock.
 */

/** When the sender must rekey. */
export interface RekeyPolicy {
  /** Records sent under one key (the one being sealed included). */
  readonly maxRecords: number;
  /** Milliseconds a key may be used, counted from the previous rekey. */
  readonly maxAgeMs: number;
}

/** The limits of the specification: 100 000 records, 60 minutes. */
export const DEFAULT_REKEY_POLICY: RekeyPolicy = {
  maxRecords: 100_000,
  maxAgeMs: 3_600_000,
};

/** State of the sending key since its last rekey. */
export interface RekeyState {
  /** Records already sealed under the current sending key. */
  sentSinceRekey: number;
  /** `nowMs` at which the current sending key started, or `null` until the first record. */
  keyStartedAtMs: number | null;
}

/** A fresh state: no record sent, no start time yet (set by the first record). */
export function newRekeyState(startedAtMs: number | null = null): RekeyState {
  return { sentSinceRekey: 0, keyStartedAtMs: startedAtMs };
}

/**
 * Whether the record about to be sealed must carry `REKEY`.
 *
 * `sentSinceRekey` counts the records already sealed under the current key,
 * so the record being sealed is number `sentSinceRekey + 1`; it carries
 * `REKEY` when that number reaches `maxRecords`, or when the key is at
 * least `maxAgeMs` old at `nowMs`.
 */
export function shouldRekey(policy: RekeyPolicy, state: RekeyState, nowMs: number): boolean {
  if (state.sentSinceRekey + 1 >= policy.maxRecords) {
    return true;
  }
  const started = state.keyStartedAtMs;
  return started !== null && nowMs - started >= policy.maxAgeMs;
}
