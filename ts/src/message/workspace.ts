// SPDX-License-Identifier: GPL-3.0-or-later

/** Workspace state (spec 03 §4): `workspace`, `profiles` and `theme`. */

import {
  asArray,
  asBool,
  asInt,
  asMap,
  asText,
  asUint,
  isNull,
  mapReader,
  type CborValue,
  type MapReader,
} from "../cbor/index.js";
import { body, DecodeError, enumFrom, type BodyCodec } from "./codec.js";
import { PANE_STATES, PROFILE_KINDS, SPLIT_DIRS } from "./codes.js";
import type {
  Group,
  Item,
  Pane,
  PaneNode,
  Profile,
  Profiles,
  Rgb,
  Split,
  Tab,
  Theme,
  WorkspaceMessage,
  WorkspaceWindow,
} from "./types.js";

/** Largest `rgb` value (`uint .le 0xFFFFFF`). */
const RGB_MAX = 0xffffff;

/** Largest `split.ratio`; the smallest is 1. */
const RATIO_MAX = 65535;

/** `theme.palette` length. */
const PALETTE_LEN = 16;

function outOfDomain(): never {
  throw new DecodeError("out-of-domain");
}

/** `tstr / null`. */
function textOrNull(value: CborValue | undefined): string | null {
  return isNull(value) ? null : asText(value);
}

/** `int / null`. */
function intOrNull(value: CborValue | undefined): number | null {
  return isNull(value) ? null : asInt(value);
}

function rgbFrom(value: CborValue | undefined): Rgb {
  const n = asUint(value);
  if (n > RGB_MAX) {
    return outOfDomain();
  }
  return n;
}

// --- pane-node ---

function paneToValue(pane: Pane): CborValue {
  return body([
    ["pane", pane.pane],
    ["title", pane.title],
    ["cwd", pane.cwd],
    ["state", pane.state],
    ["exit", pane.exit],
    ["cols", pane.cols],
    ["rows", pane.rows],
  ]);
}

function paneFromReader(r: MapReader): Pane {
  return {
    pane: asUint(r.required("pane")),
    title: asText(r.required("title")),
    cwd: textOrNull(r.required("cwd")),
    state: enumFrom(PANE_STATES, asUint(r.required("state"))),
    exit: intOrNull(r.required("exit")),
    cols: asUint(r.required("cols")),
    rows: asUint(r.required("rows")),
  };
}

function isSplit(node: PaneNode): node is Split {
  return "dir" in node;
}

function paneNodeToValue(node: PaneNode): CborValue {
  if (isSplit(node)) {
    return body([
      ["dir", node.dir],
      ["ratio", node.ratio],
      ["first", paneNodeToValue(node.first)],
      ["second", paneNodeToValue(node.second)],
    ]);
  }
  return paneToValue(node);
}

/** A split has `dir`, a pane has `pane`; both or neither is refused. */
function paneNodeFromValue(value: CborValue | undefined): PaneNode {
  const r = mapReader(asMap(value));
  const hasDir = r.optional("dir") !== undefined;
  const hasPane = r.optional("pane") !== undefined;
  if (hasDir === hasPane) {
    return outOfDomain();
  }
  if (hasPane) {
    return paneFromReader(r);
  }
  const dir = enumFrom(SPLIT_DIRS, asUint(r.required("dir")));
  const ratio = asUint(r.required("ratio"));
  if (ratio < 1 || ratio > RATIO_MAX) {
    return outOfDomain();
  }
  return {
    dir,
    ratio,
    first: paneNodeFromValue(r.required("first")),
    second: paneNodeFromValue(r.required("second")),
  };
}

// --- tab, group, item ---

function tabToValue(tab: Tab): CborValue {
  return body([
    ["tab", tab.tab],
    ["title", tab.title],
    ["custom", tab.custom],
    ["profile", tab.profile],
    ["remote", tab.remote],
    ["activity", tab.activity],
    ["bell", tab.bell],
    ["layout", paneNodeToValue(tab.layout)],
    ["focused", tab.focused],
  ]);
}

function tabFromReader(r: MapReader): Tab {
  return {
    tab: asUint(r.required("tab")),
    title: asText(r.required("title")),
    custom: asBool(r.required("custom")),
    profile: textOrNull(r.required("profile")),
    remote: textOrNull(r.required("remote")),
    activity: asBool(r.required("activity")),
    bell: asBool(r.required("bell")),
    layout: paneNodeFromValue(r.required("layout")),
    focused: asUint(r.required("focused")),
  };
}

function tabFromValue(value: CborValue | undefined): Tab {
  return tabFromReader(mapReader(asMap(value)));
}

function groupToValue(group: Group): CborValue {
  return body([
    ["group", group.group],
    ["name", group.name],
    ["color", group.color],
    ["collapsed", group.collapsed],
    ["tabs", group.tabs.map(tabToValue)],
  ]);
}

function groupFromReader(r: MapReader): Group {
  const group = asUint(r.required("group"));
  const name = asText(r.required("name"));
  const color = asUint(r.required("color"));
  const collapsed = asBool(r.required("collapsed"));
  const tabs = asArray(r.required("tabs")).map(tabFromValue);
  if (tabs.length === 0) {
    return outOfDomain();
  }
  return { group, name, color, collapsed, tabs };
}

function itemToValue(item: Item): CborValue {
  return "group" in item ? groupToValue(item) : tabToValue(item);
}

/** A group has `group`, a tab has `tab`; both or neither is refused. */
function itemFromValue(value: CborValue | undefined): Item {
  const r = mapReader(asMap(value));
  const hasGroup = r.optional("group") !== undefined;
  const hasTab = r.optional("tab") !== undefined;
  if (hasGroup === hasTab) {
    return outOfDomain();
  }
  return hasGroup ? groupFromReader(r) : tabFromReader(r);
}

// --- window ---

function windowToValue(window: WorkspaceWindow): CborValue {
  return body([
    ["id", window.id],
    ["title", window.title],
    ["pinned", window.pinned.map(tabToValue)],
    ["items", window.items.map(itemToValue)],
    ["active", window.active],
  ]);
}

function windowFromValue(value: CborValue | undefined): WorkspaceWindow {
  const r = mapReader(asMap(value));
  const active = r.required("active");
  return {
    id: asUint(r.required("id")),
    title: asText(r.required("title")),
    pinned: asArray(r.required("pinned")).map(tabFromValue),
    items: asArray(r.required("items")).map(itemFromValue),
    active: isNull(active) ? null : asUint(active),
  };
}

export const workspaceCodec: BodyCodec<WorkspaceMessage> = {
  toBody: (m) =>
    body([
      ["rev", m.rev],
      ["windows", m.windows.map(windowToValue)],
    ]),
  fromBody: (r) => ({
    type: "workspace",
    rev: asUint(r.required("rev")),
    windows: asArray(r.required("windows")).map(windowFromValue),
  }),
};

// --- profiles ---

function profileToValue(profile: Profile): CborValue {
  return body([
    ["id", profile.id],
    ["name", profile.name],
    ["kind", profile.kind],
  ]);
}

function profileFromValue(value: CborValue | undefined): Profile {
  const r = mapReader(asMap(value));
  return {
    id: asText(r.required("id")),
    name: asText(r.required("name")),
    kind: enumFrom(PROFILE_KINDS, asUint(r.required("kind"))),
  };
}

export const profilesCodec: BodyCodec<Profiles> = {
  toBody: (m) =>
    body([
      ["list", m.list.map(profileToValue)],
      ["default", m.defaultProfile],
    ]),
  fromBody: (r) => ({
    type: "profiles",
    list: asArray(r.required("list")).map(profileFromValue),
    defaultProfile: asText(r.required("default")),
  }),
};

// --- theme ---

export const themeCodec: BodyCodec<Theme> = {
  toBody: (m) =>
    body([
      ["fg", m.fg],
      ["bg", m.bg],
      ["cursor", m.cursor],
      ["selection_bg", m.selectionBg],
      ["palette", [...m.palette]],
      ["group_colors", [...m.groupColors]],
    ]),
  fromBody: (r) => {
    const fg = rgbFrom(r.required("fg"));
    const bg = rgbFrom(r.required("bg"));
    const cursor = rgbFrom(r.required("cursor"));
    const selectionBg = rgbFrom(r.required("selection_bg"));
    const palette = asArray(r.required("palette")).map(rgbFrom);
    const groupColors = asArray(r.required("group_colors")).map(rgbFrom);
    if (palette.length !== PALETTE_LEN || groupColors.length === 0) {
      return outOfDomain();
    }
    return { type: "theme", fg, bg, cursor, selectionBg, palette, groupColors };
  },
};
