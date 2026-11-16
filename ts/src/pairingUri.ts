// SPDX-License-Identifier: GPL-3.0-or-later

import {
  base64UrlDecode,
  base64UrlEncode,
  percentDecode,
  percentEncode,
  utf8CodePointLength,
  utf8Decode,
  utf8Encode,
} from "./encoding.js";
import { parseHostId } from "./hostId.js";

/** Major version of the protocol implemented here (spec 04). */
export const PROTOCOL_MAJOR = 0;

/** Longest device name the host accepts at pairing, in UTF-8 bytes (spec 02 §3.2). */
export const DEVICE_NAME_MAX_BYTES = 64;

/** Host name shown by the client: 1 to 32 Unicode scalar values (spec 02 §3.1). */
const HOST_NAME_MAX_CHARS = 32;

const KEY_LEN = 32;
const TOKEN_LEN = 16;

/** What the pairing QR carries (spec 02 §3.1). */
export interface PairingInvite {
  /** Major protocol version of the host. */
  major: number;
  /** Normalized relay base URL (`wss://…`, or `ws://` for a local host). */
  relayUrl: string;
  /** Address of the host at the relay. */
  hostId: string;
  /** Static X25519 public key of the host, 32 bytes. */
  hostKey: Uint8Array;
  /** One-time pairing token, 16 bytes. */
  token: Uint8Array;
  /** Host name for display, 1 to 32 scalar values. */
  hostName: string;
}

/** Why a pairing URI was refused. Typed codes only: no prose. */
export type PairingUriError =
  | "not-porecatu"
  | "missing-field"
  | "duplicate-field"
  | "malformed-field"
  | "version-mismatch"
  | "insecure-relay";

/** Result of {@link parsePairingUri}; `field` names the offending parameter when there is one. */
export type PairingUriResult =
  | { ok: true; invite: PairingInvite }
  | { ok: false; error: PairingUriError; field?: string };

const URI_PREFIX = "porecatu://pair";
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

function isPathChar(ch: string): boolean {
  return /^[A-Za-z0-9\-._~!$&'()*+,;=:@/]$/.test(ch);
}

/**
 * Normalizes a relay base URL (spec 02 §3.1): scheme and host lowercase, no
 * trailing slash, no query, no fragment; port and path prefix kept as given.
 * Only `ws` and `wss` are accepted; user info, an empty host, a bad port or a
 * path with characters outside RFC 3986 give `null`. Does not check security:
 * see {@link isSecureRelayUrl}.
 */
export function normalizeRelayUrl(input: string): string | null {
  const m = /^([A-Za-z]+):\/\/(.*)$/s.exec(input);
  if (m === null) {
    return null;
  }
  const scheme = (m[1] ?? "").toLowerCase();
  if (scheme !== "ws" && scheme !== "wss") {
    return null;
  }
  let rest = m[2] ?? "";
  const cut = rest.search(/[?#]/);
  if (cut >= 0) {
    rest = rest.slice(0, cut);
  }
  const slash = rest.indexOf("/");
  const authority = slash >= 0 ? rest.slice(0, slash) : rest;
  let path = slash >= 0 ? rest.slice(slash) : "";

  let host: string;
  let port = "";
  if (authority.startsWith("[")) {
    const close = authority.indexOf("]");
    if (close < 0) {
      return null;
    }
    const inner = authority.slice(1, close);
    if (!/^[0-9A-Fa-f:.]+$/.test(inner) || !inner.includes(":")) {
      return null;
    }
    host = `[${inner.toLowerCase()}]`;
    const tail = authority.slice(close + 1);
    if (tail !== "") {
      if (!tail.startsWith(":")) {
        return null;
      }
      port = tail.slice(1);
      if (port === "") {
        return null;
      }
    }
  } else {
    const colon = authority.indexOf(":");
    const name = colon >= 0 ? authority.slice(0, colon) : authority;
    if (colon >= 0) {
      port = authority.slice(colon + 1);
      if (port === "") {
        return null;
      }
    }
    if (!/^[A-Za-z0-9._~-]+$/.test(name)) {
      return null;
    }
    host = name.toLowerCase();
  }
  if (port !== "" && (!/^[0-9]{1,5}$/.test(port) || Number(port) > 65535)) {
    return null;
  }

  // Path: only RFC 3986 characters, `%` only as a valid `%HH`.
  const chars = Array.from(path);
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i] ?? "";
    if (ch === "%") {
      if (!/^[0-9A-Fa-f]$/.test(chars[i + 1] ?? "") || !/^[0-9A-Fa-f]$/.test(chars[i + 2] ?? "")) {
        return null;
      }
      i += 2;
    } else if (!isPathChar(ch)) {
      return null;
    }
  }
  path = path.replace(/\/+$/, "");

  return `${scheme}://${host}${port === "" ? "" : `:${port}`}${path}`;
}

/**
 * `true` when the relay may be used: `wss://`, or `ws://` only for the local
 * development hosts `localhost`, `127.0.0.1` and `[::1]` (spec 01 §1).
 * Malformed URLs are not secure.
 */
export function isSecureRelayUrl(input: string): boolean {
  const url = normalizeRelayUrl(input);
  if (url === null) {
    return false;
  }
  if (url.startsWith("wss://")) {
    return true;
  }
  const authority = url.slice("ws://".length).split("/")[0] ?? "";
  const host = authority.startsWith("[")
    ? authority.slice(0, authority.indexOf("]") + 1)
    : (authority.split(":")[0] ?? "");
  return LOCAL_HOSTS.has(host);
}

function requireRelay(relayBase: string): string {
  const url = normalizeRelayUrl(relayBase);
  if (url === null) {
    throw new RangeError("relay URL is not a valid ws:// or wss:// URL");
  }
  return url;
}

function requireHostId(id: string): string {
  if (parseHostId(id) === null) {
    throw new RangeError("malformed host id");
  }
  return id;
}

/**
 * URL of the presence query (spec 01 §2): the relay base with `wss` turned
 * into `https` (`ws` into `http`), then `/v0/presence/<host_id>`.
 * Throws `RangeError` for a malformed relay URL or host id.
 */
export function presenceUrl(relayBase: string, hostId: string): string {
  const base = requireRelay(relayBase);
  const http = base.startsWith("wss://") ? `https://${base.slice(6)}` : `http://${base.slice(5)}`;
  return `${http}/v0/presence/${requireHostId(hostId)}`;
}

/**
 * URL of the client WebSocket (spec 01 §2): `<relay>/v0/client/<host_id>`.
 * Throws `RangeError` for a malformed relay URL or host id.
 */
export function clientChannelUrl(relayBase: string, hostId: string): string {
  return `${requireRelay(relayBase)}/v0/client/${requireHostId(hostId)}`;
}

/** Counts Unicode scalar values the way the spec does (`Array.from`). */
function hostNameLength(name: string): number {
  return Array.from(name).length;
}

function hasLoneSurrogate(text: string): boolean {
  for (const ch of text) {
    const cp = ch.codePointAt(0) ?? 0;
    if (cp >= 0xd800 && cp <= 0xdfff) {
      return true;
    }
  }
  return false;
}

/**
 * Cuts `name` to at most {@link DEVICE_NAME_MAX_BYTES} UTF-8 bytes without
 * splitting a character.
 */
export function clampDeviceName(name: string): string {
  let bytes = 0;
  let out = "";
  for (const ch of name) {
    const len = utf8CodePointLength(ch.codePointAt(0) ?? 0xfffd);
    if (bytes + len > DEVICE_NAME_MAX_BYTES) {
      break;
    }
    bytes += len;
    out += ch;
  }
  return out;
}

/**
 * Builds the QR URI byte for byte as spec 02 §3.1 prescribes: parameters in
 * the order `v relay host key token name`, everything outside RFC 3986
 * unreserved percent-encoded with uppercase hex, relay normalized.
 * Throws `RangeError` when a field is invalid (the host must never emit it).
 */
export function formatPairingUri(invite: PairingInvite): string {
  if (!Number.isSafeInteger(invite.major) || invite.major < 0) {
    throw new RangeError("major version must be a non-negative integer");
  }
  const relay = requireRelay(invite.relayUrl);
  if (!isSecureRelayUrl(relay)) {
    throw new RangeError("relay must be wss:// (or ws:// for a local host)");
  }
  requireHostId(invite.hostId);
  if (invite.hostKey.length !== KEY_LEN) {
    throw new RangeError(`host key must be ${KEY_LEN} bytes`);
  }
  if (invite.token.length !== TOKEN_LEN) {
    throw new RangeError(`token must be ${TOKEN_LEN} bytes`);
  }
  const nameLen = hostNameLength(invite.hostName);
  if (nameLen < 1 || nameLen > HOST_NAME_MAX_CHARS || hasLoneSurrogate(invite.hostName)) {
    throw new RangeError(`host name must be 1 to ${HOST_NAME_MAX_CHARS} Unicode scalar values`);
  }
  return (
    `${URI_PREFIX}?v=${invite.major}` +
    `&relay=${percentEncode(utf8Encode(relay))}` +
    `&host=${invite.hostId}` +
    `&key=${base64UrlEncode(invite.hostKey)}` +
    `&token=${base64UrlEncode(invite.token)}` +
    `&name=${percentEncode(utf8Encode(invite.hostName))}`
  );
}

const KNOWN_FIELDS = ["v", "relay", "host", "key", "token", "name"] as const;
type Field = (typeof KNOWN_FIELDS)[number];

function fail(error: PairingUriError, field?: string): PairingUriResult {
  return field === undefined ? { ok: false, error } : { ok: false, error, field };
}

/** Percent-decodes a value that must be UTF-8 text. */
function decodeText(raw: string): string | null {
  const bytes = percentDecode(raw);
  return bytes === null ? null : utf8Decode(bytes);
}

/**
 * Parses and validates a pairing URI (spec 02 §3.1). Unknown parameters are
 * ignored; a required one that is missing, repeated or malformed is refused.
 *
 * Checks run in this order, and the first failure is returned: scheme and
 * path (`not-porecatu`); `v` (missing, repeated, malformed, then
 * `version-mismatch`, so a future major is reported as such whatever else it
 * changed); then `relay`, `host`, `key`, `token`, `name` in that order, each
 * missing, repeated or malformed; `insecure-relay` is checked right after
 * `relay` is decoded.
 */
export function parsePairingUri(uri: string): PairingUriResult {
  if (uri.length < URI_PREFIX.length || uri.slice(0, "porecatu://".length).toLowerCase() !== "porecatu://") {
    return fail("not-porecatu");
  }
  let rest = uri.slice("porecatu://".length);
  const hash = rest.indexOf("#");
  if (hash >= 0) {
    rest = rest.slice(0, hash);
  }
  const q = rest.indexOf("?");
  const path = q >= 0 ? rest.slice(0, q) : rest;
  if (path !== "pair") {
    return fail("not-porecatu");
  }
  const query = q >= 0 ? rest.slice(q + 1) : "";

  const found = new Map<Field, string[]>();
  for (const part of query.split("&")) {
    if (part === "") {
      continue;
    }
    const eq = part.indexOf("=");
    const name = eq >= 0 ? part.slice(0, eq) : part;
    const value = eq >= 0 ? part.slice(eq + 1) : "";
    const field = KNOWN_FIELDS.find((f) => f === name);
    if (field !== undefined) {
      const values = found.get(field) ?? [];
      values.push(value);
      found.set(field, values);
    }
  }

  const single = (field: Field): { raw: string } | PairingUriResult => {
    const values = found.get(field);
    if (values === undefined) {
      return fail("missing-field", field);
    }
    if (values.length > 1) {
      return fail("duplicate-field", field);
    }
    return { raw: values[0] ?? "" };
  };
  const isFailure = (x: { raw: string } | PairingUriResult): x is PairingUriResult => "ok" in x;

  const v = single("v");
  if (isFailure(v)) return v;
  if (!/^[0-9]{1,9}$/.test(v.raw)) {
    return fail("malformed-field", "v");
  }
  const major = Number(v.raw);
  if (major !== PROTOCOL_MAJOR) {
    return fail("version-mismatch", "v");
  }

  const relayField = single("relay");
  if (isFailure(relayField)) return relayField;
  const relayText = decodeText(relayField.raw);
  const relayUrl = relayText === null ? null : normalizeRelayUrl(relayText);
  if (relayUrl === null) {
    return fail("malformed-field", "relay");
  }
  if (!isSecureRelayUrl(relayUrl)) {
    return fail("insecure-relay", "relay");
  }

  const hostField = single("host");
  if (isFailure(hostField)) return hostField;
  const hostText = decodeText(hostField.raw);
  const hostId = hostText === null ? null : parseHostId(hostText);
  if (hostId === null) {
    return fail("malformed-field", "host");
  }

  const keyField = single("key");
  if (isFailure(keyField)) return keyField;
  const keyText = decodeText(keyField.raw);
  const hostKey = keyText === null ? null : base64UrlDecode(keyText);
  if (hostKey === null || hostKey.length !== KEY_LEN) {
    return fail("malformed-field", "key");
  }

  const tokenField = single("token");
  if (isFailure(tokenField)) return tokenField;
  const tokenText = decodeText(tokenField.raw);
  const token = tokenText === null ? null : base64UrlDecode(tokenText);
  if (token === null || token.length !== TOKEN_LEN) {
    return fail("malformed-field", "token");
  }

  const nameField = single("name");
  if (isFailure(nameField)) return nameField;
  const hostName = decodeText(nameField.raw);
  if (hostName === null) {
    return fail("malformed-field", "name");
  }
  const nameLen = hostNameLength(hostName);
  if (nameLen < 1 || nameLen > HOST_NAME_MAX_CHARS) {
    return fail("malformed-field", "name");
  }

  return { ok: true, invite: { major, relayUrl, hostId, hostKey, token, hostName } };
}
