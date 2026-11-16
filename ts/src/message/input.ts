// SPDX-License-Identifier: GPL-3.0-or-later

/** Semantic input (spec 03 §6): `key`, `text-input` and `paste`. */

import { asText, asUint, CborError } from "../cbor/index.js";
import { body, DecodeError, enumFrom, type BodyCodec } from "./codec.js";
import { NAMED_KEYS, type NamedKey } from "./codes.js";
import type { Key, Paste, TextInput } from "./types.js";

/** True when `text` is exactly one Unicode scalar value (the decoder rejects lone surrogates). */
function isSingleScalar(text: string): boolean {
  // At most two UTF-16 units can make one scalar value; bound the work first.
  if (text.length === 0 || text.length > 2) {
    return false;
  }
  return Array.from(text).length === 1;
}

/** `named-key / tstr`: a number is a {@link NamedKey}; text is one scalar value. */
function keyFromValue(value: unknown): NamedKey | string {
  if (typeof value === "number") {
    return enumFrom(NAMED_KEYS, asUint(value));
  }
  const text = asText(value as string);
  if (!isSingleScalar(text)) {
    throw new DecodeError("out-of-domain");
  }
  return text;
}

export const keyCodec: BodyCodec<Key> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["key", m.key],
      ["mods", m.mods],
    ]),
  fromBody: (r) => {
    const raw = r.required("key");
    if (typeof raw !== "number" && typeof raw !== "string") {
      throw new CborError("wrong-type");
    }
    return {
      type: "key",
      pane: asUint(r.required("pane")),
      key: keyFromValue(raw),
      mods: asUint(r.required("mods")),
    };
  },
};

export const textInputCodec: BodyCodec<TextInput> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["text", m.text],
    ]),
  fromBody: (r) => ({
    type: "text-input",
    pane: asUint(r.required("pane")),
    text: asText(r.required("text")),
  }),
};

export const pasteCodec: BodyCodec<Paste> = {
  toBody: (m) =>
    body([
      ["pane", m.pane],
      ["text", m.text],
    ]),
  fromBody: (r) => ({
    type: "paste",
    pane: asUint(r.required("pane")),
    text: asText(r.required("text")),
  }),
};
