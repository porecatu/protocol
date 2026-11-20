// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Numeric domains of the message layer (spec 03). Every enum is a frozen
 * `as const` object keyed by the camelCase spelling of the spec name, plus an
 * array of its wire values and a union type with the same name as the object.
 * The numbers are the wire values and are normative.
 */

/** Wire type code of every message (spec 03 §10), keyed by the `type` literal. */
export const MessageType = {
  hello: 0,
  welcome: 1,
  incompatible: 2,
  goodbye: 3,
  workspace: 10,
  profiles: 11,
  theme: 12,
  "view-tab": 13,
  "list-sessions": 14,
  sessions: 15,
  subscribe: 20,
  unsubscribe: 21,
  "grid-snapshot": 22,
  "grid-diff": 23,
  "grid-ack": 24,
  "scrollback-request": 25,
  "scrollback-page": 26,
  "subscription-ended": 27,
  key: 30,
  "text-input": 31,
  paste: 32,
  action: 40,
  "action-result": 41,
  "confirm-request": 42,
  confirm: 43,
  bell: 50,
  "pair-accepted": 60,
  "pair-rejected": 61,
  error: 70,
} as const;

/** Wire type codes of all messages, in table order. */
export const MESSAGE_TYPE_CODES: readonly number[] = Object.values(MessageType);

/** `goodbye-reason` (spec 03 §2). */
export const GoodbyeReason = {
  normal: 0,
  remoteDisabled: 1,
  revoked: 2,
  hostQuitting: 3,
} as const;
export type GoodbyeReason = (typeof GoodbyeReason)[keyof typeof GoodbyeReason];
export const GOODBYE_REASONS: readonly GoodbyeReason[] = Object.values(GoodbyeReason);

/** `pane-state` (spec 03 §4.1). */
export const PaneState = {
  running: 0,
  exited: 1,
  notStarted: 2,
  connecting: 3,
  reconnecting: 4,
  disconnected: 5,
  awaitingDesktop: 6,
} as const;
export type PaneState = (typeof PaneState)[keyof typeof PaneState];
export const PANE_STATES: readonly PaneState[] = Object.values(PaneState);

/** `split.dir` (spec 03 §4.1). */
export const SplitDir = {
  sideBySide: 0,
  stacked: 1,
} as const;
export type SplitDir = (typeof SplitDir)[keyof typeof SplitDir];
export const SPLIT_DIRS: readonly SplitDir[] = Object.values(SplitDir);

/** `profile.kind` (spec 03 §4.2). */
export const ProfileKind = {
  local: 0,
  ssh: 1,
  wsl: 2,
  other: 3,
} as const;
export type ProfileKind = (typeof ProfileKind)[keyof typeof ProfileKind];
export const PROFILE_KINDS: readonly ProfileKind[] = Object.values(ProfileKind);

/** `cursor.shape` (spec 03 §5.3). */
export const CursorShape = {
  block: 0,
  underline: 1,
  beam: 2,
} as const;
export type CursorShape = (typeof CursorShape)[keyof typeof CursorShape];
export const CURSOR_SHAPES: readonly CursorShape[] = Object.values(CursorShape);

/** `subscription-ended.reason` (spec 03 §5.1). */
export const SubscriptionEndReason = {
  paneClosed: 0,
  hostLimit: 1,
} as const;
export type SubscriptionEndReason =
  (typeof SubscriptionEndReason)[keyof typeof SubscriptionEndReason];
export const SUBSCRIPTION_END_REASONS: readonly SubscriptionEndReason[] =
  Object.values(SubscriptionEndReason);

/** `sessions.mode` (spec 03 §4.5): where the desktop restores a session to. */
export const SessionsMode = {
  newWindow: 0,
  append: 1,
} as const;
export type SessionsMode = (typeof SessionsMode)[keyof typeof SessionsMode];
export const SESSIONS_MODES: readonly SessionsMode[] = Object.values(SessionsMode);

/** `named-key` (spec 03 §6). The numbers 14 to 20 are unassigned. */
export const NamedKey = {
  enter: 0,
  tab: 1,
  backspace: 2,
  escape: 3,
  up: 4,
  down: 5,
  left: 6,
  right: 7,
  home: 8,
  end: 9,
  pageUp: 10,
  pageDown: 11,
  insert: 12,
  delete: 13,
  f1: 21,
  f2: 22,
  f3: 23,
  f4: 24,
  f5: 25,
  f6: 26,
  f7: 27,
  f8: 28,
  f9: 29,
  f10: 30,
  f11: 31,
  f12: 32,
} as const;
export type NamedKey = (typeof NamedKey)[keyof typeof NamedKey];
export const NAMED_KEYS: readonly NamedKey[] = Object.values(NamedKey);

/** `confirm-request.kind` (spec 03 §7). */
export const ConfirmKind = {
  runningProcess: 0,
  sshConnection: 1,
} as const;
export type ConfirmKind = (typeof ConfirmKind)[keyof typeof ConfirmKind];
export const CONFIRM_KINDS: readonly ConfirmKind[] = Object.values(ConfirmKind);

/** `pair-rejected.reason` (spec 03 §8). */
export const PairRejectReason = {
  tokenInvalid: 0,
  userDeclined: 1,
  timeout: 2,
} as const;
export type PairRejectReason = (typeof PairRejectReason)[keyof typeof PairRejectReason];
export const PAIR_REJECT_REASONS: readonly PairRejectReason[] =
  Object.values(PairRejectReason);

/** `pair-request.platform` (spec 02 §3). */
export const Platform = {
  android: 0,
  ios: 1,
  other: 2,
} as const;
export type Platform = (typeof Platform)[keyof typeof Platform];
export const PLATFORMS: readonly Platform[] = Object.values(Platform);

/** `error-code` (spec 03 §9). The number 2 is reserved. */
export const ErrorCode = {
  protocol: 0,
  notPaired: 1,
  unknownWindow: 3,
  unknownGroup: 4,
  unknownTab: 5,
  unknownPane: 6,
  unknownProfile: 7,
  paneNotRunning: 8,
  notSubscribed: 9,
  tooManySubscriptions: 10,
  invalidArgument: 11,
  busy: 12,
  confirmTimeout: 13,
  cancelled: 14,
  unsupported: 15,
  lastTab: 16,
  unknownSession: 17,
  sessionUnreadable: 18,
} as const;
export type ErrorCode = (typeof ErrorCode)[keyof typeof ErrorCode];
export const ERROR_CODES: readonly ErrorCode[] = Object.values(ErrorCode);

/** Bits of `style.flags` (spec 03 §5.2). */
export const STYLE = {
  bold: 0x01,
  italic: 0x02,
  underline: 0x04,
  inverse: 0x08,
  dim: 0x10,
  strike: 0x20,
  hidden: 0x40,
} as const;

/** Bits of `key.mods` (spec 03 §6). */
export const MOD = {
  ctrl: 0x01,
  alt: 0x02,
  shift: 0x04,
  meta: 0x08,
} as const;

/**
 * Wire code of every action (spec 03 §7), keyed by the `type` literal. The
 * number 13 is reserved (it was `activate-tab`, removed in 0.5): like any code
 * that is not here, it is refused on decode.
 */
export const ActionCode = {
  "new-tab": 0,
  "close-tab": 1,
  "close-pane": 2,
  "rename-tab": 3,
  "move-tab-to-group": 4,
  "move-tab-to-window": 5,
  "new-group": 6,
  "rename-group": 7,
  "set-group-color": 8,
  "collapse-group": 9,
  "move-group-to-window": 10,
  ungroup: 11,
  "start-pane": 12,
  "restore-session": 14,
} as const;

/** Action discriminants (spec 03 §7), in the order of their wire codes (13 is a hole). */
export const ACTION_TYPES = Object.keys(ActionCode) as ReadonlyArray<keyof typeof ActionCode>;
