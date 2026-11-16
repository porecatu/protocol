// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  DEVICE_NAME_MAX_BYTES,
  PROTOCOL_MAJOR,
  clampDeviceName,
  clientChannelUrl,
  formatPairingUri,
  isSecureRelayUrl,
  normalizeRelayUrl,
  parsePairingUri,
  presenceUrl,
  type PairingInvite,
} from "../src/pairingUri.js";
import { hostId } from "../src/hostId.js";
import { utf8Encode } from "../src/encoding.js";

// Fixed cases shared with the Rust implementation (see hostId.test.ts for
// how the host id was computed): key A = 00 01 .. 1f, key B = 32 x ff.
const KEY_A = Uint8Array.from({ length: 32 }, (_, i) => i);
const KEY_B = new Uint8Array(32).fill(0xff);
const TOKEN = Uint8Array.from({ length: 16 }, (_, i) => i);
const HOST_ID_A = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";

const INVITE: PairingInvite = {
  major: 0,
  relayUrl: "wss://Relay.Exemplo.com.br:8443/porecatu/",
  hostId: hostId(KEY_A),
  hostKey: KEY_B,
  token: TOKEN,
  hostName: "Mesa do Léo (casa)",
};

// Written by hand from spec 02 §3.1: base64url(32 x ff) is 42 '_' then '8';
// base64url(00..0f) is AAECAwQFBgcICQoLDA0ODw (Python urlsafe_b64encode).
const EXPECTED_URI =
  "porecatu://pair?v=0" +
  "&relay=wss%3A%2F%2Frelay.exemplo.com.br%3A8443%2Fporecatu" +
  `&host=${HOST_ID_A}` +
  `&key=${"_".repeat(42)}8` +
  "&token=AAECAwQFBgcICQoLDA0ODw" +
  "&name=Mesa%20do%20L%C3%A9o%20%28casa%29";

/** EXPECTED_URI with one parameter replaced, removed (`null`) or appended. */
function uriWith(param: string, value: string | null): string {
  const parts = EXPECTED_URI.slice("porecatu://pair?".length).split("&");
  const rest = parts.filter((p) => !p.startsWith(`${param}=`));
  if (value !== null) {
    rest.push(`${param}=${value}`);
  }
  return `porecatu://pair?${rest.join("&")}`;
}

describe("formatPairingUri", () => {
  it("writes the exact bytes of the spec", () => {
    expect(formatPairingUri(INVITE)).toBe(EXPECTED_URI);
  });

  it("keeps the parameter order v relay host key token name", () => {
    const names = formatPairingUri(INVITE)
      .split("?")[1]
      ?.split("&")
      .map((p) => p.split("=")[0]);
    expect(names).toEqual(["v", "relay", "host", "key", "token", "name"]);
  });

  it("uses uppercase hex only", () => {
    expect(formatPairingUri(INVITE)).not.toMatch(/%[0-9A-F]?[a-f]/);
  });

  it("refuses invalid fields instead of emitting them", () => {
    expect(() => formatPairingUri({ ...INVITE, relayUrl: "ws://relay.exemplo.com.br" })).toThrow(RangeError);
    expect(() => formatPairingUri({ ...INVITE, relayUrl: "https://x" })).toThrow(RangeError);
    expect(() => formatPairingUri({ ...INVITE, hostId: "abc" })).toThrow(RangeError);
    expect(() => formatPairingUri({ ...INVITE, hostKey: new Uint8Array(31) })).toThrow(RangeError);
    expect(() => formatPairingUri({ ...INVITE, token: new Uint8Array(17) })).toThrow(RangeError);
    expect(() => formatPairingUri({ ...INVITE, hostName: "" })).toThrow(RangeError);
    expect(() => formatPairingUri({ ...INVITE, hostName: "a".repeat(33) })).toThrow(RangeError);
  });
});

describe("parsePairingUri", () => {
  it("reads back what formatPairingUri wrote", () => {
    const r = parsePairingUri(EXPECTED_URI);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.invite).toEqual({ ...INVITE, relayUrl: "wss://relay.exemplo.com.br:8443/porecatu" });
      expect(formatPairingUri(r.invite)).toBe(EXPECTED_URI);
    }
  });

  it("ignores unknown parameters, wherever they are", () => {
    const r = parsePairingUri(`${EXPECTED_URI}&x=1&x=2&future=%FF`);
    expect(r.ok).toBe(true);
    const r2 = parsePairingUri(`porecatu://pair?zz=1&${EXPECTED_URI.slice("porecatu://pair?".length)}`);
    expect(r2.ok).toBe(true);
  });

  it("accepts the scheme in any case and ignores a fragment", () => {
    expect(parsePairingUri(`PORECATU${EXPECTED_URI.slice(8)}#frag`).ok).toBe(true);
  });

  it("refuses a repeated required parameter", () => {
    expect(parsePairingUri(`${EXPECTED_URI}&name=Outro`)).toEqual({
      ok: false,
      error: "duplicate-field",
      field: "name",
    });
    expect(parsePairingUri(`${EXPECTED_URI}&v=0`)).toEqual({ ok: false, error: "duplicate-field", field: "v" });
  });

  it("refuses a missing required parameter", () => {
    for (const field of ["v", "relay", "host", "key", "token", "name"]) {
      expect(parsePairingUri(uriWith(field, null))).toEqual({ ok: false, error: "missing-field", field });
    }
  });

  it("refuses a URI that is not a Porecatu pairing", () => {
    for (const uri of ["", "https://pair?v=0", "porecatu://other?v=0", "porecatu:pair?v=0", "porecatu://pairing?v=0"]) {
      expect(parsePairingUri(uri)).toEqual({ ok: false, error: "not-porecatu" });
    }
  });

  it("refuses another major version, before looking at the rest", () => {
    expect(parsePairingUri(uriWith("v", "1"))).toEqual({ ok: false, error: "version-mismatch", field: "v" });
    expect(parsePairingUri("porecatu://pair?v=1")).toEqual({ ok: false, error: "version-mismatch", field: "v" });
    expect(parsePairingUri(uriWith("v", "x"))).toEqual({ ok: false, error: "malformed-field", field: "v" });
    expect(PROTOCOL_MAJOR).toBe(0);
  });

  it("refuses an insecure relay but accepts ws:// for local hosts", () => {
    const enc = (s: string): string => encodeURIComponent(s);
    expect(parsePairingUri(uriWith("relay", enc("ws://relay.exemplo.com.br")))).toEqual({
      ok: false,
      error: "insecure-relay",
      field: "relay",
    });
    for (const url of ["ws://localhost:8080", "ws://127.0.0.1:8080", "ws://[::1]:8080"]) {
      const r = parsePairingUri(uriWith("relay", enc(url)));
      expect(r.ok).toBe(true);
      if (r.ok) {
        expect(r.invite.relayUrl).toBe(url);
      }
    }
  });

  it("refuses a malformed relay", () => {
    for (const bad of ["", "relay.exemplo.com.br", "https%3A%2F%2Fx", "wss%3A%2F%2F", "wss%3A%2F%2Fa%40b", "%ZZ"]) {
      expect(parsePairingUri(uriWith("relay", bad))).toEqual({ ok: false, error: "malformed-field", field: "relay" });
    }
  });

  it("refuses a malformed host id", () => {
    for (const bad of ["", "abc", HOST_ID_A.toUpperCase(), `${HOST_ID_A}a`]) {
      expect(parsePairingUri(uriWith("host", bad))).toEqual({ ok: false, error: "malformed-field", field: "host" });
    }
  });

  it("refuses key and token of the wrong size or alphabet", () => {
    const b64 = (n: number): string => "A".repeat(n);
    expect(parsePairingUri(uriWith("key", `${"_".repeat(41)}A`))).toEqual({
      ok: false,
      error: "malformed-field",
      field: "key",
    }); // 41 chars decode to 30 bytes
    expect(parsePairingUri(uriWith("key", b64(44)))).toEqual({ ok: false, error: "malformed-field", field: "key" });
    expect(parsePairingUri(uriWith("key", `${"_".repeat(42)}=`))).toEqual({
      ok: false,
      error: "malformed-field",
      field: "key",
    });
    expect(parsePairingUri(uriWith("key", ""))).toEqual({ ok: false, error: "malformed-field", field: "key" });
    expect(parsePairingUri(uriWith("token", b64(21)))).toEqual({ ok: false, error: "malformed-field", field: "token" });
    expect(parsePairingUri(uriWith("token", b64(23)))).toEqual({ ok: false, error: "malformed-field", field: "token" });
  });

  it("refuses a 31-byte key", () => {
    // 31 bytes of ff encode to 41 base64url characters ending in 'w'.
    expect(parsePairingUri(uriWith("key", `${"_".repeat(40)}w`))).toEqual({
      ok: false,
      error: "malformed-field",
      field: "key",
    });
  });

  it("enforces 1 to 32 scalar values in the name", () => {
    const encName = (s: string): string => encodeURIComponent(s);
    const ok32 = parsePairingUri(uriWith("name", "a".repeat(32)));
    expect(ok32.ok).toBe(true);
    expect(parsePairingUri(uriWith("name", "a".repeat(33)))).toEqual({
      ok: false,
      error: "malformed-field",
      field: "name",
    });
    expect(parsePairingUri(uriWith("name", ""))).toEqual({ ok: false, error: "malformed-field", field: "name" });
    // An emoji is one scalar value (two UTF-16 units): 31 'a' + emoji = 32.
    const r = parsePairingUri(uriWith("name", encName(`${"a".repeat(31)}😀`)));
    expect(r.ok).toBe(true);
    expect(parsePairingUri(uriWith("name", encName(`${"a".repeat(32)}😀`))).ok).toBe(false);
  });

  it("refuses a name that is not valid UTF-8 or a bad percent escape", () => {
    expect(parsePairingUri(uriWith("name", "%FF%FE"))).toEqual({ ok: false, error: "malformed-field", field: "name" });
    expect(parsePairingUri(uriWith("name", "abc%"))).toEqual({ ok: false, error: "malformed-field", field: "name" });
    expect(parsePairingUri(uriWith("name", "abc%4"))).toEqual({ ok: false, error: "malformed-field", field: "name" });
  });
});

describe("normalizeRelayUrl", () => {
  it("lowercases scheme and host and drops trailing slash, query and fragment", () => {
    expect(normalizeRelayUrl("WSS://Relay.Exemplo.COM.br")).toBe("wss://relay.exemplo.com.br");
    expect(normalizeRelayUrl("wss://relay.exemplo.com.br/")).toBe("wss://relay.exemplo.com.br");
    expect(normalizeRelayUrl("wss://relay.exemplo.com.br///")).toBe("wss://relay.exemplo.com.br");
    expect(normalizeRelayUrl("wss://relay.exemplo.com.br/a?x=1#f")).toBe("wss://relay.exemplo.com.br/a");
    expect(normalizeRelayUrl("wss://relay.exemplo.com.br?x=1")).toBe("wss://relay.exemplo.com.br");
  });

  it("keeps the port and the path prefix (case preserved)", () => {
    expect(normalizeRelayUrl("wss://Relay.Exemplo.com.br:8443/porecatu/")).toBe(
      "wss://relay.exemplo.com.br:8443/porecatu",
    );
    expect(normalizeRelayUrl("wss://h/Sub/Path/")).toBe("wss://h/Sub/Path");
    expect(normalizeRelayUrl("wss://h:443")).toBe("wss://h:443");
  });

  it("handles IPv6 literals", () => {
    expect(normalizeRelayUrl("ws://[::1]:8080")).toBe("ws://[::1]:8080");
    expect(normalizeRelayUrl("ws://[::1]/")).toBe("ws://[::1]");
    expect(normalizeRelayUrl("wss://[2001:DB8::1]")).toBe("wss://[2001:db8::1]");
  });

  it("is idempotent", () => {
    for (const url of ["wss://Relay.Exemplo.com.br:8443/porecatu/", "ws://[::1]:8080/", "wss://a"]) {
      const once = normalizeRelayUrl(url);
      expect(once).not.toBeNull();
      expect(normalizeRelayUrl(once ?? "")).toBe(once);
    }
  });

  it("refuses other schemes, user info, empty hosts, bad ports and unsafe paths", () => {
    for (const bad of [
      "",
      "relay.exemplo.com.br",
      "https://relay.exemplo.com.br",
      "wss://",
      "wss:///path",
      "wss://user@relay.exemplo.com.br",
      "wss://relay.exemplo.com.br:",
      "wss://relay.exemplo.com.br:99999",
      "wss://relay.exemplo.com.br:80a",
      "wss://relay.exemplo.com.br/a b",
      "wss://relay.exemplo.com.br/é",
      "wss://relay.exemplo.com.br/%zz",
      "wss://[::1",
      "wss://[zz]",
      "wss://[::1]x",
    ]) {
      expect(normalizeRelayUrl(bad), bad).toBeNull();
    }
  });
});

describe("isSecureRelayUrl", () => {
  it("accepts wss anywhere and ws only on local hosts", () => {
    expect(isSecureRelayUrl("wss://relay.exemplo.com.br")).toBe(true);
    expect(isSecureRelayUrl("ws://localhost")).toBe(true);
    expect(isSecureRelayUrl("ws://LOCALHOST:8080/x")).toBe(true);
    expect(isSecureRelayUrl("ws://127.0.0.1:8080")).toBe(true);
    expect(isSecureRelayUrl("ws://[::1]:8080")).toBe(true);
    expect(isSecureRelayUrl("ws://relay.exemplo.com.br")).toBe(false);
    expect(isSecureRelayUrl("ws://127.0.0.2")).toBe(false);
    expect(isSecureRelayUrl("ws://localhost.exemplo.com")).toBe(false);
    expect(isSecureRelayUrl("http://localhost")).toBe(false);
    expect(isSecureRelayUrl("garbage")).toBe(false);
  });
});

describe("relay URLs", () => {
  it("derives the presence URL, turning wss into https and ws into http", () => {
    expect(presenceUrl("wss://exemplo.com.br/relay", HOST_ID_A)).toBe(
      `https://exemplo.com.br/relay/v0/presence/${HOST_ID_A}`,
    );
    expect(presenceUrl("wss://Exemplo.com.br:8443/", HOST_ID_A)).toBe(
      `https://exemplo.com.br:8443/v0/presence/${HOST_ID_A}`,
    );
    expect(presenceUrl("ws://localhost:8080", HOST_ID_A)).toBe(`http://localhost:8080/v0/presence/${HOST_ID_A}`);
  });

  it("derives the client channel URL", () => {
    expect(clientChannelUrl("wss://exemplo.com.br/relay/", HOST_ID_A)).toBe(
      `wss://exemplo.com.br/relay/v0/client/${HOST_ID_A}`,
    );
  });

  it("throws on a malformed relay or host id", () => {
    expect(() => presenceUrl("nope", HOST_ID_A)).toThrow(RangeError);
    expect(() => clientChannelUrl("wss://x", "short")).toThrow(RangeError);
  });
});

describe("clampDeviceName", () => {
  it("leaves a short name alone", () => {
    expect(clampDeviceName("iPhone do Léo")).toBe("iPhone do Léo");
    expect(clampDeviceName("")).toBe("");
  });

  it("cuts at 64 UTF-8 bytes", () => {
    const out = clampDeviceName("a".repeat(100));
    expect(out).toBe("a".repeat(DEVICE_NAME_MAX_BYTES));
  });

  it("never splits a character", () => {
    // 'é' is 2 bytes: 63 'a' + 'é' would be 65, so the 'é' is dropped.
    const out = clampDeviceName(`${"a".repeat(63)}é`);
    expect(out).toBe("a".repeat(63));
    // Exactly 64 bytes is kept whole.
    expect(clampDeviceName(`${"a".repeat(62)}é`)).toBe(`${"a".repeat(62)}é`);
    // A 4-byte emoji that would straddle the limit is dropped whole.
    const emoji = clampDeviceName(`${"a".repeat(61)}😀`);
    expect(emoji).toBe("a".repeat(61));
    expect(utf8Encode(emoji).length).toBeLessThanOrEqual(DEVICE_NAME_MAX_BYTES);
    expect(clampDeviceName("😀".repeat(20))).toBe("😀".repeat(16));
  });
});
