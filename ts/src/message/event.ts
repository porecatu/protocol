// SPDX-License-Identifier: GPL-3.0-or-later

/** Events and errors (spec 03 §9): `bell` and `error`. */

import { asUint } from "../cbor/index.js";
import { body, enumFrom, type BodyCodec } from "./codec.js";
import { ERROR_CODES } from "./codes.js";
import type { Bell, ErrorMessage } from "./types.js";

export const errorCodec: BodyCodec<ErrorMessage> = {
  toBody: (m) =>
    body([
      ["code", m.code],
      ["id", m.id],
      ["pane", m.pane],
    ]),
  fromBody: (r) => {
    const code = enumFrom(ERROR_CODES, asUint(r.required("code")));
    const id = r.optional("id");
    const pane = r.optional("pane");
    return {
      type: "error",
      code,
      ...(id === undefined ? {} : { id: asUint(id) }),
      ...(pane === undefined ? {} : { pane: asUint(pane) }),
    };
  },
};

export const bellCodec: BodyCodec<Bell> = {
  toBody: (m) => body([["pane", m.pane]]),
  fromBody: (r) => ({ type: "bell", pane: asUint(r.required("pane")) }),
};
