// SPDX-License-Identifier: GPL-3.0-or-later

/** Actions (spec 03 §7): `action`, `action-result`, `confirm-request` and `confirm`. */

import {
  asArray,
  asBool,
  asMap,
  asText,
  asUint,
  expectArray,
  isNull,
  mapReader,
  type CborValue,
  type MapReader,
} from "../cbor/index.js";
import { body, DecodeError, enumFrom, type BodyCodec } from "./codec.js";
import { ACTION_TYPES, ActionCode, CONFIRM_KINDS, ERROR_CODES } from "./codes.js";
import type { Action, ActionBody, ActionResult, Confirm, ConfirmRequest } from "./types.js";

/** `[code, { ... }]`, the positional form of an `action-body` (spec 03 §7). */
function actionBodyToValue(action: ActionBody): CborValue {
  const code = ActionCode[action.type];
  switch (action.type) {
    case "new-tab":
      return [
        code,
        body([
          ["window", action.window],
          ["group", action.group],
          ["profile", action.profile],
        ]),
      ];
    case "close-tab":
      return [code, body([["tab", action.tab]])];
    case "close-pane":
      return [code, body([["pane", action.pane]])];
    case "rename-tab":
      return [
        code,
        body([
          ["tab", action.tab],
          ["title", action.title],
        ]),
      ];
    case "move-tab-to-group":
      return [
        code,
        body([
          ["tab", action.tab],
          ["group", action.group],
        ]),
      ];
    case "move-tab-to-window":
      return [
        code,
        body([
          ["tab", action.tab],
          ["window", action.window],
        ]),
      ];
    case "new-group":
      return [
        code,
        body([
          ["tabs", [...action.tabs]],
          ["name", action.name],
          ["color", action.color],
        ]),
      ];
    case "rename-group":
      return [
        code,
        body([
          ["group", action.group],
          ["name", action.name],
        ]),
      ];
    case "set-group-color":
      return [
        code,
        body([
          ["group", action.group],
          ["color", action.color],
        ]),
      ];
    case "collapse-group":
      return [
        code,
        body([
          ["group", action.group],
          ["collapsed", action.collapsed],
        ]),
      ];
    case "move-group-to-window":
      return [
        code,
        body([
          ["group", action.group],
          ["window", action.window],
        ]),
      ];
    case "ungroup":
      return [code, body([["group", action.group]])];
    case "start-pane":
      return [code, body([["pane", action.pane]])];
    case "restore-session":
      return [
        code,
        body([
          ["session", action.session],
          ["window", action.window],
        ]),
      ];
  }
}

/** An optional `uint` key: left out of the result when absent (`exactOptionalPropertyTypes`). */
function optionalUint(r: MapReader, key: string): number | undefined {
  const value = r.optional(key);
  return value === undefined ? undefined : asUint(value);
}

function optionalText(r: MapReader, key: string): string | undefined {
  const value = r.optional(key);
  return value === undefined ? undefined : asText(value);
}

/** The action with wire `code`; `undefined` for a reserved or unassigned code (13, 15, ...). */
function actionTypeOf(code: number): ActionBody["type"] | undefined {
  return ACTION_TYPES.find((type) => ActionCode[type] === code);
}

function actionBodyFromValue(value: CborValue | undefined): ActionBody {
  const items = expectArray(value, 2);
  const code = asUint(items[0]);
  const type = actionTypeOf(code);
  if (type === undefined) {
    throw new DecodeError("out-of-domain");
  }
  const r = mapReader(asMap(items[1]));
  switch (type) {
    case "new-tab": {
      // `group`: absent (the host's `tab.new` rule), `null` (a loose tab) or a group id.
      const rawGroup = r.optional("group");
      const group = rawGroup === undefined ? undefined : isNull(rawGroup) ? null : asUint(rawGroup);
      const profile = optionalText(r, "profile");
      return {
        type,
        window: asUint(r.required("window")),
        ...(group === undefined ? {} : { group }),
        ...(profile === undefined ? {} : { profile }),
      };
    }
    case "close-tab":
      return { type, tab: asUint(r.required("tab")) };
    case "close-pane":
      return { type, pane: asUint(r.required("pane")) };
    case "rename-tab": {
      const title = r.required("title");
      return {
        type,
        tab: asUint(r.required("tab")),
        title: isNull(title) ? null : asText(title),
      };
    }
    case "move-tab-to-group": {
      const group = r.required("group");
      return {
        type,
        tab: asUint(r.required("tab")),
        group: isNull(group) ? null : asUint(group),
      };
    }
    case "move-tab-to-window":
      return {
        type,
        tab: asUint(r.required("tab")),
        window: asUint(r.required("window")),
      };
    case "new-group": {
      const tabs = asArray(r.required("tabs")).map((t) => asUint(t));
      const name = asText(r.required("name"));
      const color = optionalUint(r, "color");
      if (tabs.length === 0) {
        throw new DecodeError("out-of-domain");
      }
      return { type, tabs, name, ...(color === undefined ? {} : { color }) };
    }
    case "rename-group":
      return {
        type,
        group: asUint(r.required("group")),
        name: asText(r.required("name")),
      };
    case "set-group-color":
      return {
        type,
        group: asUint(r.required("group")),
        color: asUint(r.required("color")),
      };
    case "collapse-group":
      return {
        type,
        group: asUint(r.required("group")),
        collapsed: asBool(r.required("collapsed")),
      };
    case "move-group-to-window":
      return {
        type,
        group: asUint(r.required("group")),
        window: asUint(r.required("window")),
      };
    case "ungroup":
      return { type, group: asUint(r.required("group")) };
    case "start-pane":
      return { type, pane: asUint(r.required("pane")) };
    case "restore-session": {
      const window = optionalUint(r, "window");
      return {
        type,
        session: asUint(r.required("session")),
        ...(window === undefined ? {} : { window }),
      };
    }
  }
}

export const actionCodec: BodyCodec<Action> = {
  toBody: (m) =>
    body([
      ["id", m.id],
      ["do", actionBodyToValue(m.do)],
    ]),
  fromBody: (r) => ({
    type: "action",
    id: asUint(r.required("id")),
    do: actionBodyFromValue(r.required("do")),
  }),
};

export const actionResultCodec: BodyCodec<ActionResult> = {
  toBody: (m) =>
    body([
      ["id", m.id],
      ["error", m.error],
      ["created", m.created],
    ]),
  fromBody: (r) => {
    const id = asUint(r.required("id"));
    const error = optionalUint(r, "error");
    // `created` is a tab-id or a group-id; only the originating action says which.
    const created = optionalUint(r, "created");
    return {
      type: "action-result",
      id,
      ...(error === undefined ? {} : { error: enumFrom(ERROR_CODES, error) }),
      ...(created === undefined ? {} : { created }),
    };
  },
};

export const confirmRequestCodec: BodyCodec<ConfirmRequest> = {
  toBody: (m) =>
    body([
      ["id", m.id],
      ["kind", m.kind],
      ["processes", [...m.processes]],
    ]),
  fromBody: (r) => ({
    type: "confirm-request",
    id: asUint(r.required("id")),
    kind: enumFrom(CONFIRM_KINDS, asUint(r.required("kind"))),
    processes: asArray(r.required("processes")).map((p) => asText(p)),
  }),
};

export const confirmCodec: BodyCodec<Confirm> = {
  toBody: (m) =>
    body([
      ["id", m.id],
      ["accept", m.accept],
    ]),
  fromBody: (r) => ({
    type: "confirm",
    id: asUint(r.required("id")),
    accept: asBool(r.required("accept")),
  }),
};
