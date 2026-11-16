// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The message layer (spec 03): `encodeMessage` and `decodeMessage` over the 26
 * messages, with dispatch by type code and by direction.
 *
 * Pattern for the message modules (`session`, `workspace`, `grid`, `input`,
 * `action`, `pairing`, `event`):
 *
 * - Each message has one `BodyCodec<M>` (see `codec.ts`): `toBody` returns the
 *   body map, `fromBody` reads it. The envelope `[type, body]`, the type code
 *   and the direction check live here, never in the modules.
 * - Wire keys are exactly the spec's (snake_case); object fields are camelCase.
 *   Optional keys are optional fields: `body()` omits `undefined` entries, and
 *   `fromBody` leaves the field out when the key is absent (spread an empty
 *   object: `exactOptionalPropertyTypes` forbids an explicit `undefined`).
 * - Required keys are read with `reader.required`; unknown keys are never
 *   asked for, so they are ignored (spec 03 §1.1 item 3). Canonical form is
 *   checked once, by `decodeCbor`, before any body is read.
 * - Typed readers (`asUint`, `asText`, ...) throw `CborError`; a well-typed
 *   value outside its domain throws `DecodeError("out-of-domain")`
 *   (`enumFrom` does it for enums). `decodeMessage` turns both into
 *   `{ ok: false }`; it never throws on bad input.
 * - A body that is not implemented yet is `notImplemented("<name>")`; it throws
 *   a plain `Error`, which `decodeMessage` deliberately does not swallow.
 * - Register the codec in `CODECS` below; the table is typed so a missing or
 *   mistyped entry does not compile.
 */

import { decodeCbor, asMap, asUint, encodeCbor, expectArray, mapReader } from "../cbor/index.js";
import {
  actionCodec,
  actionResultCodec,
  confirmCodec,
  confirmRequestCodec,
} from "./action.js";
import { DecodeError, toDecodeError, type BodyCodec } from "./codec.js";
import { MessageType } from "./codes.js";
import { bellCodec, errorCodec } from "./event.js";
import {
  gridAckCodec,
  gridDiffCodec,
  gridSnapshotCodec,
  scrollbackPageCodec,
  scrollbackRequestCodec,
  subscribeCodec,
  subscriptionEndedCodec,
  unsubscribeCodec,
} from "./grid.js";
import { keyCodec, pasteCodec, textInputCodec } from "./input.js";
import { pairAcceptedCodec, pairRejectedCodec } from "./pairing.js";
import {
  goodbyeCodec,
  helloCodec,
  incompatibleCodec,
  welcomeCodec,
} from "./session.js";
import type { Direction, Message } from "./types.js";
import { profilesCodec, themeCodec, workspaceCodec } from "./workspace.js";

export * from "./codes.js";
export { DecodeError, type DecodeErrorReason } from "./codec.js";
export type * from "./types.js";

/** Who is allowed to send a message type (spec 03 §10). */
export type Sender = Direction | "both";

type MessageByType = { [M in Message as M["type"]]: M };

interface Entry<M> {
  readonly code: number;
  readonly sender: Sender;
  readonly codec: BodyCodec<M>;
}

type Table = { [T in Message["type"]]: Entry<MessageByType[T]> };

const CODECS: Table = {
  hello: { code: MessageType.hello, sender: "client", codec: helloCodec },
  welcome: { code: MessageType.welcome, sender: "host", codec: welcomeCodec },
  incompatible: {
    code: MessageType.incompatible,
    sender: "host",
    codec: incompatibleCodec,
  },
  goodbye: { code: MessageType.goodbye, sender: "both", codec: goodbyeCodec },
  workspace: { code: MessageType.workspace, sender: "host", codec: workspaceCodec },
  profiles: { code: MessageType.profiles, sender: "host", codec: profilesCodec },
  theme: { code: MessageType.theme, sender: "host", codec: themeCodec },
  subscribe: { code: MessageType.subscribe, sender: "client", codec: subscribeCodec },
  unsubscribe: {
    code: MessageType.unsubscribe,
    sender: "client",
    codec: unsubscribeCodec,
  },
  "grid-snapshot": {
    code: MessageType["grid-snapshot"],
    sender: "host",
    codec: gridSnapshotCodec,
  },
  "grid-diff": { code: MessageType["grid-diff"], sender: "host", codec: gridDiffCodec },
  "grid-ack": { code: MessageType["grid-ack"], sender: "client", codec: gridAckCodec },
  "scrollback-request": {
    code: MessageType["scrollback-request"],
    sender: "client",
    codec: scrollbackRequestCodec,
  },
  "scrollback-page": {
    code: MessageType["scrollback-page"],
    sender: "host",
    codec: scrollbackPageCodec,
  },
  "subscription-ended": {
    code: MessageType["subscription-ended"],
    sender: "host",
    codec: subscriptionEndedCodec,
  },
  key: { code: MessageType.key, sender: "client", codec: keyCodec },
  "text-input": {
    code: MessageType["text-input"],
    sender: "client",
    codec: textInputCodec,
  },
  paste: { code: MessageType.paste, sender: "client", codec: pasteCodec },
  action: { code: MessageType.action, sender: "client", codec: actionCodec },
  "action-result": {
    code: MessageType["action-result"],
    sender: "host",
    codec: actionResultCodec,
  },
  "confirm-request": {
    code: MessageType["confirm-request"],
    sender: "host",
    codec: confirmRequestCodec,
  },
  confirm: { code: MessageType.confirm, sender: "client", codec: confirmCodec },
  bell: { code: MessageType.bell, sender: "host", codec: bellCodec },
  "pair-accepted": {
    code: MessageType["pair-accepted"],
    sender: "host",
    codec: pairAcceptedCodec,
  },
  "pair-rejected": {
    code: MessageType["pair-rejected"],
    sender: "host",
    codec: pairRejectedCodec,
  },
  error: { code: MessageType.error, sender: "both", codec: errorCodec },
};

/** Entries in a lookup by wire type code. */
const BY_CODE: ReadonlyMap<number, Entry<Message> & { readonly type: Message["type"] }> =
  new Map(
    (Object.keys(CODECS) as Array<Message["type"]>).map((type) => {
      const entry = CODECS[type] as Entry<Message>;
      return [entry.code, { ...entry, type }] as const;
    }),
  );

/** The wire type code of a message type. */
export function messageTypeCode(type: Message["type"]): number {
  return CODECS[type].code;
}

/** Who sends a message type (spec 03 §10). */
export function messageSender(type: Message["type"]): Sender {
  return CODECS[type].sender;
}

/** Encodes `[type, body]` deterministically (spec 03 §1.1). */
export function encodeMessage(message: Message): Uint8Array {
  const entry = CODECS[message.type] as Entry<Message>;
  return encodeCbor([entry.code, entry.codec.toBody(message)]);
}

export type DecodeResult =
  | { readonly ok: true; readonly message: Message }
  | { readonly ok: false; readonly error: DecodeError };

/**
 * Decodes one message sent by `from`. Anything wrong with the bytes (invalid
 * or non-canonical CBOR, unknown type, wrong direction, missing key, value out
 * of domain) comes back as `{ ok: false }`; the session answers it with
 * `error { code: protocol }` and closes.
 */
export function decodeMessage(bytes: Uint8Array, from: Direction): DecodeResult {
  try {
    const items = expectArray(decodeCbor(bytes), 2);
    const entry = BY_CODE.get(asUint(items[0]));
    if (entry === undefined) {
      throw new DecodeError("unknown-type");
    }
    if (entry.sender !== "both" && entry.sender !== from) {
      throw new DecodeError("wrong-direction");
    }
    const message = entry.codec.fromBody(mapReader(asMap(items[1])));
    return { ok: true, message };
  } catch (e) {
    return { ok: false, error: toDecodeError(e) };
  }
}
