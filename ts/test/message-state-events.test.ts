// SPDX-License-Identifier: GPL-3.0-or-later

/** Group P20: workspace, profiles, theme, bell, pair-accepted, pair-rejected. */

import { describe, expect, it } from "vitest";
import { CborMap, encodeCbor, type CborValue } from "../src/cbor/index.js";
import {
  decodeMessage,
  encodeMessage,
  type DecodeErrorReason,
  type DecodeResult,
  type Direction,
  type Message,
  type Tab,
} from "../src/message/index.js";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function fromHex(text: string): Uint8Array {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(text.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function failure(result: DecodeResult): DecodeErrorReason {
  if (result.ok) {
    throw new Error("expected a decode failure");
  }
  return result.error.reason;
}

function success(result: DecodeResult): Message {
  if (!result.ok) {
    throw new Error(`expected success, got ${result.error.reason}`);
  }
  return result.message;
}

/** A map from a plain object; the encoder sorts the keys. */
function map(entries: Record<string, CborValue | undefined>): CborMap {
  return new CborMap(
    Object.entries(entries).flatMap(([k, v]) => (v === undefined ? [] : [[k, v] as const])),
  );
}

function raw(type: number, entries: Record<string, CborValue | undefined>): Uint8Array {
  return encodeCbor([type, map(entries)]);
}

// The canonical entries below were encoded by an independent RFC 8949
// deterministic encoder written for this purpose (p20_hex.py: head byte of a
// text key is 0x60 + its length, map keys sorted by their encoded bytes), and
// the three short ones were also assembled by hand. They do not come from the
// TypeScript encoder. The Rust side uses the same entries and the same hex.

const BELL: Message = { type: "bell", pane: 7 };
// [50, {pane: 7}]; 50 = 0x18 0x32
const BELL_HEX = "82" + "1832" + "a1" + "6470616e65" + "07";

const PAIR_ACCEPTED: Message = { type: "pair-accepted", hostName: "pc" };
// [60, {host_name: "pc"}]; 60 = 0x18 0x3c
const PAIR_ACCEPTED_HEX = "82" + "183c" + "a1" + "69686f73745f6e616d65" + "627063";

const PAIR_REJECTED: Message = { type: "pair-rejected", reason: 1 };
// [61, {reason: 1 (user-declined)}]; 61 = 0x18 0x3d
const PAIR_REJECTED_HEX = "82" + "183d" + "a1" + "66726561736f6e" + "01";

const PROFILES: Message = {
  type: "profiles",
  list: [{ id: "auto:pwsh", name: "PowerShell", kind: 0 }],
  defaultProfile: "auto:pwsh",
};
const PROFILES_HEX =
  "820b" + // [11,
  "a2" + // {
  "646c697374" + // "list":
  "81a3" + // [ {
  "626964" + "696175746f3a70777368" + // "id": "auto:pwsh"
  "646b696e64" + "00" + // "kind": 0
  "646e616d65" + "6a506f7765725368656c6c" + // "name": "PowerShell"
  "6764656661756c74" + "696175746f3a70777368"; // "default": "auto:pwsh"

const THEME: Message = {
  type: "theme",
  fg: 0xd7dce3,
  bg: 0x15181d,
  cursor: 0x5ed3bc,
  selectionBg: 0x2b3a4a,
  palette: Array.from({ length: 16 }, (_, i) => i),
  groupColors: [0xff0000],
};
const THEME_HEX =
  "820c" + // [12,
  "a6" + // {
  "626267" + "1a0015181d" + // "bg"
  "626667" + "1a00d7dce3" + // "fg"
  "66637572736f72" + "1a005ed3bc" + // "cursor"
  "6770616c65747465" + "90" + "000102030405060708090a0b0c0d0e0f" + // "palette": 16 colors
  "6c67726f75705f636f6c6f7273" + "81" + "1a00ff0000" + // "group_colors"
  "6c73656c656374696f6e5f6267" + "1a002b3a4a"; // "selection_bg"

const PANE = {
  pane: 3,
  title: "a",
  cwd: null,
  state: 0,
  exit: null,
  cols: 80,
  rows: 24,
} as const;

const TAB: Tab = {
  tab: 2,
  title: "a",
  custom: false,
  profile: null,
  remote: null,
  activity: false,
  bell: false,
  layout: PANE,
  focused: 3,
};

const WORKSPACE: Message = {
  type: "workspace",
  rev: 1,
  windows: [{ id: 1, title: "a", pinned: [], items: [TAB], active: 2 }],
};
const WORKSPACE_HEX =
  "820a" + // [10,
  "a2" + // {
  "63726576" + "01" + // "rev": 1
  "6777696e646f7773" + "81" + "a5" + // "windows": [ window with 5 keys
  "626964" + "01" + // "id": 1
  "656974656d73" + "81" + "a9" + // "items": [ tab with 9 keys
  "63746162" + "02" + // "tab": 2
  "6462656c6c" + "f4" + // "bell": false
  "657469746c65" + "6161" + // "title": "a"
  "66637573746f6d" + "f4" + // "custom": false
  "666c61796f7574" + "a7" + // "layout": pane with 7 keys
  "63637764" + "f6" + // "cwd": null
  "64636f6c73" + "1850" + // "cols": 80
  "6465786974" + "f6" + // "exit": null
  "6470616e65" + "03" + // "pane": 3
  "64726f7773" + "1818" + // "rows": 24
  "657374617465" + "00" + // "state": running
  "657469746c65" + "6161" + // "title": "a"
  "6672656d6f7465" + "f6" + // "remote": null
  "67666f6375736564" + "03" + // "focused": 3
  "6770726f66696c65" + "f6" + // "profile": null
  "686163746976697479" + "f4" + // "activity": false
  "657469746c65" + "6161" + // window "title": "a"
  "66616374697665" + "02" + // "active": 2
  "6670696e6e6564" + "80"; // "pinned": []

describe("state and event codecs: exact bytes and round trips", () => {
  it("bell", () => {
    expect(hex(encodeMessage(BELL))).toBe(BELL_HEX);
    expect(success(decodeMessage(fromHex(BELL_HEX), "host"))).toEqual(BELL);
  });

  it("pair-accepted", () => {
    expect(hex(encodeMessage(PAIR_ACCEPTED))).toBe(PAIR_ACCEPTED_HEX);
    expect(success(decodeMessage(fromHex(PAIR_ACCEPTED_HEX), "host"))).toEqual(PAIR_ACCEPTED);
  });

  it("pair-rejected", () => {
    expect(hex(encodeMessage(PAIR_REJECTED))).toBe(PAIR_REJECTED_HEX);
    expect(success(decodeMessage(fromHex(PAIR_REJECTED_HEX), "host"))).toEqual(PAIR_REJECTED);
  });

  it("profiles", () => {
    expect(hex(encodeMessage(PROFILES))).toBe(PROFILES_HEX);
    expect(success(decodeMessage(fromHex(PROFILES_HEX), "host"))).toEqual(PROFILES);
  });

  it("theme", () => {
    expect(hex(encodeMessage(THEME))).toBe(THEME_HEX);
    expect(success(decodeMessage(fromHex(THEME_HEX), "host"))).toEqual(THEME);
  });

  it("workspace", () => {
    expect(hex(encodeMessage(WORKSPACE))).toBe(WORKSPACE_HEX);
    expect(success(decodeMessage(fromHex(WORKSPACE_HEX), "host"))).toEqual(WORKSPACE);
  });
});

function roundTrip(message: Message): void {
  expect(success(decodeMessage(encodeMessage(message), "host"))).toEqual(message);
}

describe("complete forms", () => {
  it("round-trips every enum value and every nullable field filled", () => {
    for (const state of [0, 1, 2, 3, 4, 5, 6] as const) {
      roundTrip({
        type: "workspace",
        rev: 9,
        windows: [
          {
            id: 1,
            title: "w",
            pinned: [],
            items: [
              {
                ...TAB,
                layout: { ...PANE, state, cwd: "C:\\x", exit: -1 },
              },
            ],
            active: null,
          },
        ],
      });
    }
    roundTrip({
      type: "profiles",
      list: [
        { id: "a", name: "A", kind: 0 },
        { id: "b", name: "B", kind: 1 },
        { id: "c", name: "C", kind: 2 },
        { id: "d", name: "D", kind: 3 },
      ],
      defaultProfile: "a",
    });
    roundTrip({ type: "profiles", list: [], defaultProfile: "" });
    for (const reason of [0, 1, 2] as const) {
      roundTrip({ type: "pair-rejected", reason });
    }
  });

  it("round-trips pinned tabs, groups, splits and a tab with every field set", () => {
    const full: Tab = {
      tab: 4,
      title: "ssh",
      custom: true,
      profile: "auto:pwsh",
      remote: "me@host",
      activity: true,
      bell: true,
      layout: {
        dir: 1,
        ratio: 32768,
        first: { ...PANE, pane: 10, state: 3, exit: 0 },
        second: {
          dir: 0,
          ratio: 65535,
          first: { ...PANE, pane: 11, cwd: "/tmp" },
          second: { ...PANE, pane: 12, state: 1, exit: 130 },
        },
      },
      focused: 11,
    };
    roundTrip({
      type: "workspace",
      rev: 2,
      windows: [
        {
          id: 1,
          title: "a",
          pinned: [full],
          items: [
            TAB,
            { group: 8, name: "g", color: 2, collapsed: true, tabs: [full, TAB] },
          ],
          active: 4,
        },
        { id: 2, title: "", pinned: [], items: [], active: null },
      ],
    });
    roundTrip({ type: "workspace", rev: 0, windows: [] });
  });

  it("round-trips theme at the rgb limits and with several group colors", () => {
    roundTrip({
      type: "theme",
      fg: 0xffffff,
      bg: 0,
      cursor: 1,
      selectionBg: 0x123456,
      palette: Array.from({ length: 16 }, (_, i) => i * 0x111111),
      groupColors: [0, 0xffffff, 0xff0000],
    });
  });
});

describe("unknown keys are ignored", () => {
  it.each([
    ["bell", raw(50, { pane: 7, extra: 1 }), BELL],
    ["pair-accepted", raw(60, { host_name: "pc", extra: [1] }), PAIR_ACCEPTED],
    ["pair-rejected", raw(61, { reason: 1, extra: "x" }), PAIR_REJECTED],
    [
      "profiles",
      raw(11, {
        list: [map({ id: "auto:pwsh", name: "PowerShell", kind: 0, extra: 1 })],
        default: "auto:pwsh",
        extra: 1,
      }),
      PROFILES,
    ],
  ] as const)("%s", (_name, bytes, expected) => {
    expect(success(decodeMessage(bytes, "host"))).toEqual(expected);
  });

  it("theme", () => {
    const bytes = raw(12, {
      fg: 0xd7dce3,
      bg: 0x15181d,
      cursor: 0x5ed3bc,
      selection_bg: 0x2b3a4a,
      palette: Array.from({ length: 16 }, (_, i) => i),
      group_colors: [0xff0000],
      extra: null,
    });
    expect(success(decodeMessage(bytes, "host"))).toEqual(THEME);
  });

  it("workspace, at every level", () => {
    const pane = map({ ...PANE, extra: 1 });
    const tab = map({
      tab: 2,
      title: "a",
      custom: false,
      profile: null,
      remote: null,
      activity: false,
      bell: false,
      layout: pane,
      focused: 3,
      extra: 1,
    });
    const bytes = raw(10, {
      rev: 1,
      windows: [map({ id: 1, title: "a", pinned: [], items: [tab], active: 2, extra: 1 })],
      extra: 1,
    });
    expect(success(decodeMessage(bytes, "host"))).toEqual(WORKSPACE);
  });
});

/** Raw (wire) forms to build bad inputs from. */
const rawPane = (over: Record<string, CborValue | undefined> = {}): CborMap =>
  map({ ...PANE, ...over });

const rawTab = (over: Record<string, CborValue | undefined> = {}): CborMap =>
  map({
    tab: 2,
    title: "a",
    custom: false,
    profile: null,
    remote: null,
    activity: false,
    bell: false,
    layout: rawPane(),
    focused: 3,
    ...over,
  });

const workspaceWith = (item: CborValue, extra: Record<string, CborValue | undefined> = {}): Uint8Array =>
  raw(10, {
    rev: 1,
    windows: [map({ id: 1, title: "a", pinned: [], items: [item], active: 2, ...extra })],
  });

const rawTheme = (over: Record<string, CborValue | undefined> = {}): Uint8Array =>
  raw(12, {
    fg: 1,
    bg: 2,
    cursor: 3,
    selection_bg: 4,
    palette: Array.from({ length: 16 }, (_, i) => i),
    group_colors: [5],
    ...over,
  });

describe("missing required keys", () => {
  it("bell, pair-accepted, pair-rejected", () => {
    expect(failure(decodeMessage(raw(50, {}), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(raw(60, {}), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(raw(61, {}), "host"))).toBe("missing-key");
  });

  it("profiles", () => {
    expect(failure(decodeMessage(raw(11, { list: [] }), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(raw(11, { default: "a" }), "host"))).toBe("missing-key");
    expect(
      failure(decodeMessage(raw(11, { list: [map({ id: "a", name: "A" })], default: "a" }), "host")),
    ).toBe("missing-key");
  });

  it("theme: each of the six keys", () => {
    for (const key of ["fg", "bg", "cursor", "selection_bg", "palette", "group_colors"]) {
      expect(failure(decodeMessage(rawTheme({ [key]: undefined }), "host"))).toBe("missing-key");
    }
  });

  it("workspace: rev, windows, window, tab and pane keys", () => {
    expect(failure(decodeMessage(raw(10, { windows: [] }), "host"))).toBe("missing-key");
    expect(failure(decodeMessage(raw(10, { rev: 1 }), "host"))).toBe("missing-key");
    for (const key of ["title", "pinned", "items", "active"]) {
      expect(failure(decodeMessage(workspaceWith(rawTab(), { [key]: undefined }), "host"))).toBe(
        "missing-key",
      );
    }
    // "id" omitted
    expect(
      failure(
        decodeMessage(
          raw(10, {
            rev: 1,
            windows: [map({ title: "a", pinned: [], items: [], active: null })],
          }),
          "host",
        ),
      ),
    ).toBe("missing-key");
    for (const key of [
      "title",
      "custom",
      "profile",
      "remote",
      "activity",
      "bell",
      "layout",
      "focused",
    ]) {
      expect(failure(decodeMessage(workspaceWith(rawTab({ [key]: undefined })), "host"))).toBe(
        "missing-key",
      );
    }
    // nullable keys must be present even when null
    for (const key of ["title", "cwd", "state", "exit", "cols", "rows"]) {
      expect(
        failure(
          decodeMessage(
            workspaceWith(rawTab({ layout: rawPane({ [key]: undefined }) })),
            "host",
          ),
        ),
      ).toBe("missing-key");
    }
  });
});

describe("values outside the domain", () => {
  const cases: ReadonlyArray<readonly [string, Uint8Array]> = [
    ["pair-rejected reason 3", raw(61, { reason: 3 })],
    ["profile kind 4", raw(11, { list: [map({ id: "a", name: "A", kind: 4 })], default: "a" })],
    ["theme rgb 0x1000000 in fg", rawTheme({ fg: 0x1000000 })],
    ["theme rgb out of range in the palette", rawTheme({ palette: [...Array.from({ length: 15 }, (_, i) => i), 0x1000000] })],
    ["theme rgb out of range in group_colors", rawTheme({ group_colors: [0x1000000] })],
    ["theme palette of 15", rawTheme({ palette: Array.from({ length: 15 }, (_, i) => i) })],
    ["theme palette of 17", rawTheme({ palette: Array.from({ length: 17 }, (_, i) => i) })],
    ["theme group_colors empty", rawTheme({ group_colors: [] })],
    ["pane state 7", workspaceWith(rawTab({ layout: rawPane({ state: 7 }) }))],
    [
      "split dir 2",
      workspaceWith(
        rawTab({
          layout: map({ dir: 2, ratio: 100, first: rawPane(), second: rawPane({ pane: 4 }) }),
        }),
      ),
    ],
    [
      "split ratio 0",
      workspaceWith(
        rawTab({
          layout: map({ dir: 0, ratio: 0, first: rawPane(), second: rawPane({ pane: 4 }) }),
        }),
      ),
    ],
    [
      "split ratio 65536",
      workspaceWith(
        rawTab({
          layout: map({ dir: 0, ratio: 65536, first: rawPane(), second: rawPane({ pane: 4 }) }),
        }),
      ),
    ],
    ["item with both tab and group", workspaceWith(rawTab({ group: 1, name: "g", color: 0, collapsed: false, tabs: [rawTab()] }))],
    ["item with neither tab nor group", workspaceWith(map({ title: "a" }))],
    [
      "group with no tabs",
      workspaceWith(map({ group: 1, name: "g", color: 0, collapsed: false, tabs: [] })),
    ],
    ["pane-node with both dir and pane", workspaceWith(rawTab({ layout: rawPane({ dir: 0 }) }))],
    ["pane-node with neither dir nor pane", workspaceWith(rawTab({ layout: map({ title: "a" }) }))],
  ];

  it.each(cases)("%s", (_name, bytes) => {
    expect(failure(decodeMessage(bytes, "host"))).toBe("out-of-domain");
  });

  it("accepts the ratio limits 1 and 65535", () => {
    for (const ratio of [1, 65535]) {
      const bytes = workspaceWith(
        rawTab({ layout: map({ dir: 0, ratio, first: rawPane(), second: rawPane({ pane: 4 }) }) }),
      );
      expect(decodeMessage(bytes, "host").ok).toBe(true);
    }
  });
});

describe("values of the wrong type or shape", () => {
  it("is refused", () => {
    expect(failure(decodeMessage(raw(50, { pane: "7" }), "host"))).toBe("wrong-type");
    expect(failure(decodeMessage(raw(60, { host_name: 5 }), "host"))).toBe("wrong-type");
    expect(failure(decodeMessage(raw(61, { reason: "user-declined" }), "host"))).toBe(
      "wrong-type",
    );
    expect(failure(decodeMessage(raw(11, { list: new CborMap(), default: "a" }), "host"))).toBe(
      "wrong-type",
    );
    expect(failure(decodeMessage(rawTheme({ palette: "x" }), "host"))).toBe("wrong-type");
    expect(failure(decodeMessage(raw(10, { rev: 1, windows: [5] }), "host"))).toBe("wrong-type");
    expect(failure(decodeMessage(workspaceWith(rawTab({ custom: 1 })), "host"))).toBe(
      "wrong-type",
    );
    expect(failure(decodeMessage(workspaceWith(rawTab({ profile: 3 })), "host"))).toBe(
      "wrong-type",
    );
    expect(
      failure(decodeMessage(workspaceWith(rawTab({ layout: rawPane({ exit: "x" }) })), "host")),
    ).toBe("wrong-type");
    // active may be null but not text
    expect(
      failure(decodeMessage(workspaceWith(rawTab(), { active: "2" }), "host")),
    ).toBe("wrong-type");
  });
});

describe("direction and damaged input", () => {
  it("only the host sends these messages", () => {
    for (const bytes of [
      BELL_HEX,
      PAIR_ACCEPTED_HEX,
      PAIR_REJECTED_HEX,
      PROFILES_HEX,
      THEME_HEX,
      WORKSPACE_HEX,
    ]) {
      expect(failure(decodeMessage(fromHex(bytes), "client"))).toBe("wrong-direction");
    }
  });

  it("never throws on truncated or damaged messages", () => {
    for (const text of [PROFILES_HEX, THEME_HEX, WORKSPACE_HEX]) {
      const valid = fromHex(text);
      const from: Direction = "host";
      for (let cut = 0; cut < valid.length; cut++) {
        expect(decodeMessage(valid.subarray(0, cut), from).ok).toBe(false);
      }
      for (let i = 0; i < valid.length; i++) {
        const damaged = valid.slice();
        damaged[i] = (damaged[i] ?? 0) ^ 0xff;
        decodeMessage(damaged, from); // any outcome is fine, a throw is not
      }
    }
  });
});
