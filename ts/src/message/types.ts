// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Types of the 29 messages of spec 03 and of their components. Messages are
 * unions discriminated by `type` (the spec name, kebab-case); fields are
 * camelCase and the codec maps them to the snake_case keys of the wire. Spans,
 * styles, lines and colors are positional `readonly` tuples, exactly as on the
 * wire. Ids are plain numbers (`uint` up to 2^53 - 1).
 */

import type {
  ConfirmKind,
  CursorShape,
  ErrorCode,
  GoodbyeReason,
  NamedKey,
  PairRejectReason,
  PaneState,
  Platform,
  ProfileKind,
  SessionsMode,
  SplitDir,
  SubscriptionEndReason,
} from "./codes.js";

// --- Identifiers (spec 03 §3) ---

export type WindowId = number;
export type GroupId = number;
export type TabId = number;
export type PaneId = number;
export type ProfileId = string;
export type RequestId = number;
export type SessionId = number;

/** `[major, minor]`. */
export type Version = readonly [major: number, minor: number];

/** `rgb = uint .le 0xFFFFFF`, as `0xRRGGBB`. */
export type Rgb = number;

// --- Workspace components (spec 03 §4) ---

export interface Pane {
  readonly pane: PaneId;
  readonly title: string;
  readonly cwd: string | null;
  readonly state: PaneState;
  readonly exit: number | null;
  readonly cols: number;
  readonly rows: number;
}

export interface Split {
  readonly dir: SplitDir;
  /** `1..65535`; the first child gets `ratio / 65536`. */
  readonly ratio: number;
  readonly first: PaneNode;
  readonly second: PaneNode;
}

/** A leaf has the key `pane`; a split has `dir`. */
export type PaneNode = Pane | Split;

export interface Tab {
  readonly tab: TabId;
  readonly title: string;
  readonly custom: boolean;
  readonly profile: ProfileId | null;
  readonly remote: string | null;
  readonly activity: boolean;
  readonly bell: boolean;
  readonly layout: PaneNode;
  readonly focused: PaneId;
}

export interface Group {
  readonly group: GroupId;
  readonly name: string;
  readonly color: number;
  readonly collapsed: boolean;
  readonly tabs: readonly Tab[];
}

/** A group has the key `group`; a tab has `tab`. */
export type Item = Tab | Group;

export interface WorkspaceWindow {
  readonly id: WindowId;
  readonly title: string;
  readonly pinned: readonly Tab[];
  readonly items: readonly Item[];
  readonly active: TabId | null;
}

export interface Profile {
  readonly id: ProfileId;
  readonly name: string;
  readonly kind: ProfileKind;
}

// --- Grid components (spec 03 §5) ---

/** `null` is the default color; `[0, n]` is indexed; `[1, rgb]` is direct. */
export type Color = null | readonly [kind: 0, index: number] | readonly [kind: 1, rgb: Rgb];

export type Style = readonly [fg: Color, bg: Color, flags: number];

/**
 * `[col, width, style, text]` (spec 03 §5.2). A plain `string` is an ordinary
 * span (one code point per cell); a one-element array holds a single
 * multi-code-point grapheme in one cell (ADR-0009).
 */
export type Span = readonly [
  col: number,
  width: 1 | 2,
  style: number,
  text: string | readonly [string],
];

export type Line = readonly [wrapped: boolean, spans: readonly Span[]];

/** One entry of `grid-diff.lines`: `[row, line]`. */
export type RowLine = readonly [row: number, line: Line];

export interface Cursor {
  readonly row: number;
  readonly col: number;
  readonly shape: CursorShape;
  readonly visible: boolean;
}

// --- Actions (spec 03 §7) ---

export type ActionBody =
  | {
      readonly type: "new-tab";
      readonly window: WindowId;
      readonly group?: GroupId;
      readonly profile?: ProfileId;
    }
  | { readonly type: "close-tab"; readonly tab: TabId }
  | { readonly type: "close-pane"; readonly pane: PaneId }
  | { readonly type: "rename-tab"; readonly tab: TabId; readonly title: string | null }
  | {
      readonly type: "move-tab-to-group";
      readonly tab: TabId;
      readonly group: GroupId | null;
    }
  | { readonly type: "move-tab-to-window"; readonly tab: TabId; readonly window: WindowId }
  | {
      readonly type: "new-group";
      readonly tabs: readonly TabId[];
      readonly name: string;
      readonly color?: number;
    }
  | { readonly type: "rename-group"; readonly group: GroupId; readonly name: string }
  | { readonly type: "set-group-color"; readonly group: GroupId; readonly color: number }
  | {
      readonly type: "collapse-group";
      readonly group: GroupId;
      readonly collapsed: boolean;
    }
  | {
      readonly type: "move-group-to-window";
      readonly group: GroupId;
      readonly window: WindowId;
    }
  | { readonly type: "ungroup"; readonly group: GroupId }
  | { readonly type: "start-pane"; readonly pane: PaneId }
  | {
      readonly type: "restore-session";
      readonly session: SessionId;
      readonly window?: WindowId;
    };

// --- Pairing request (spec 02 §3): the payload of msg3, not a message ---

export interface PairRequest {
  /** 16 bytes. */
  readonly token: Uint8Array;
  /** 1 to 64 bytes of UTF-8. */
  readonly device: string;
  readonly platform: Platform;
}

// --- Messages ---

export interface Hello {
  readonly type: "hello";
  readonly version: Version;
  readonly app: string;
  readonly appVersion: string;
}

export interface Welcome {
  readonly type: "welcome";
  readonly version: Version;
  readonly hostName: string;
  readonly porecatuVersion: string;
  /** At least 1. */
  readonly maxSubscriptions: number;
  /** 16 bytes. */
  readonly hostInstance: Uint8Array;
}

export interface Incompatible {
  readonly type: "incompatible";
  readonly version: Version;
}

export interface Goodbye {
  readonly type: "goodbye";
  readonly reason: GoodbyeReason;
}

export interface WorkspaceMessage {
  readonly type: "workspace";
  readonly rev: number;
  readonly windows: readonly WorkspaceWindow[];
}

export interface Profiles {
  readonly type: "profiles";
  readonly list: readonly Profile[];
  readonly defaultProfile: ProfileId;
}

export interface Theme {
  readonly type: "theme";
  readonly fg: Rgb;
  readonly bg: Rgb;
  readonly cursor: Rgb;
  readonly selectionBg: Rgb;
  /** Exactly 16 colors. */
  readonly palette: readonly Rgb[];
  /** At least one color. */
  readonly groupColors: readonly Rgb[];
}

/** Spec 03 §4.4: the tab the client is showing, `null` when it shows none. */
export interface ViewTab {
  readonly type: "view-tab";
  readonly tab: TabId | null;
}

/** Spec 03 §4.5: asks for the saved named sessions; the host answers `sessions`. */
export interface ListSessions {
  readonly type: "list-sessions";
  readonly id: RequestId;
}

/** A saved named session in `sessions.list` (spec 03 §4.5). */
export interface NamedSession {
  readonly session: SessionId;
  readonly name: string;
  /** `false` when the host cannot restore it. */
  readonly readable: boolean;
}

/** Spec 03 §4.5: the saved sessions, the most recently saved first; only sent in answer to `list-sessions`. */
export interface Sessions {
  readonly type: "sessions";
  /** The `id` of the `list-sessions`. */
  readonly id: RequestId;
  readonly mode: SessionsMode;
  readonly list: readonly NamedSession[];
}

export interface Subscribe {
  readonly type: "subscribe";
  readonly pane: PaneId;
}

export interface Unsubscribe {
  readonly type: "unsubscribe";
  readonly pane: PaneId;
}

export interface GridSnapshot {
  readonly type: "grid-snapshot";
  readonly pane: PaneId;
  readonly seq: number;
  readonly cols: number;
  readonly rows: number;
  readonly history: number;
  /** Lines already dropped from the top of the pane's history (spec 03 §5.5). */
  readonly dropped: number;
  readonly styles: readonly Style[];
  readonly lines: readonly Line[];
  readonly cursor: Cursor;
  readonly alt: boolean;
}

export interface GridDiff {
  readonly type: "grid-diff";
  readonly pane: PaneId;
  readonly seq: number;
  readonly history: number;
  readonly dropped: number;
  readonly styles: readonly Style[];
  readonly lines: readonly RowLine[];
  readonly cursor: Cursor;
  readonly alt: boolean;
}

export interface GridAck {
  readonly type: "grid-ack";
  readonly pane: PaneId;
  readonly seq: number;
}

export interface ScrollbackRequest {
  readonly type: "scrollback-request";
  readonly id: RequestId;
  readonly pane: PaneId;
  readonly offset: number;
  /** At most 500. */
  readonly count: number;
}

export interface ScrollbackPage {
  readonly type: "scrollback-page";
  readonly id: RequestId;
  readonly pane: PaneId;
  readonly history: number;
  readonly dropped: number;
  readonly offset: number;
  readonly styles: readonly Style[];
  readonly lines: readonly Line[];
}

export interface SubscriptionEnded {
  readonly type: "subscription-ended";
  readonly pane: PaneId;
  readonly reason: SubscriptionEndReason;
}

export interface Key {
  readonly type: "key";
  readonly pane: PaneId;
  /** A {@link NamedKey} number, or exactly one Unicode scalar value as text. */
  readonly key: NamedKey | string;
  /** Bits of `MOD`. */
  readonly mods: number;
}

export interface TextInput {
  readonly type: "text-input";
  readonly pane: PaneId;
  readonly text: string;
}

export interface Paste {
  readonly type: "paste";
  readonly pane: PaneId;
  readonly text: string;
}

export interface Action {
  readonly type: "action";
  readonly id: RequestId;
  /** `do` on the wire. */
  readonly do: ActionBody;
}

export interface ActionResult {
  readonly type: "action-result";
  readonly id: RequestId;
  /** Absent means success. */
  readonly error?: ErrorCode;
  /** The `tab-id` of a `new-tab` or the `group-id` of a `new-group`. */
  readonly created?: number;
}

export interface ConfirmRequest {
  readonly type: "confirm-request";
  readonly id: RequestId;
  readonly kind: ConfirmKind;
  readonly processes: readonly string[];
}

export interface Confirm {
  readonly type: "confirm";
  readonly id: RequestId;
  readonly accept: boolean;
}

export interface Bell {
  readonly type: "bell";
  readonly pane: PaneId;
}

export interface PairAccepted {
  readonly type: "pair-accepted";
  readonly hostName: string;
}

export interface PairRejected {
  readonly type: "pair-rejected";
  readonly reason: PairRejectReason;
}

export interface ErrorMessage {
  readonly type: "error";
  readonly code: ErrorCode;
  readonly id?: RequestId;
  readonly pane?: PaneId;
}

/** Messages the host sends (`H -> C` and `both`). */
export type HostMessage =
  | Welcome
  | Incompatible
  | Goodbye
  | WorkspaceMessage
  | Profiles
  | Theme
  | Sessions
  | GridSnapshot
  | GridDiff
  | ScrollbackPage
  | SubscriptionEnded
  | ActionResult
  | ConfirmRequest
  | Bell
  | PairAccepted
  | PairRejected
  | ErrorMessage;

/** Messages the client sends (`C -> H` and `both`). */
export type ClientMessage =
  | Hello
  | Goodbye
  | ViewTab
  | ListSessions
  | Subscribe
  | Unsubscribe
  | GridAck
  | ScrollbackRequest
  | Key
  | TextInput
  | Paste
  | Action
  | Confirm
  | ErrorMessage;

export type Message = HostMessage | ClientMessage;

/** Who sent a message: the side whose messages are being decoded. */
export type Direction = "host" | "client";
