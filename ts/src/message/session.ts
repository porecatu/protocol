// SPDX-License-Identifier: GPL-3.0-or-later

/** Session start and end (spec 03 §2): `hello`, `welcome`, `incompatible`, `goodbye`. */

import { asBytes, asText, asUint, expectArray, type CborValue } from "../cbor/index.js";
import { body, DecodeError, enumFrom, type BodyCodec } from "./codec.js";
import { GOODBYE_REASONS } from "./codes.js";
import type { Goodbye, Hello, Incompatible, Version, Welcome } from "./types.js";

/** `host_instance` length in bytes. */
export const HOST_INSTANCE_LEN = 16;

function versionToValue(version: Version): CborValue {
  return [version[0], version[1]];
}

function versionFromValue(value: CborValue | undefined): Version {
  const items = expectArray(value, 2);
  return [asUint(items[0]), asUint(items[1])];
}

export const helloCodec: BodyCodec<Hello> = {
  toBody: (m) =>
    body([
      ["version", versionToValue(m.version)],
      ["app", m.app],
      ["app_version", m.appVersion],
    ]),
  fromBody: (r) => ({
    type: "hello",
    version: versionFromValue(r.required("version")),
    app: asText(r.required("app")),
    appVersion: asText(r.required("app_version")),
  }),
};

export const welcomeCodec: BodyCodec<Welcome> = {
  toBody: (m) =>
    body([
      ["version", versionToValue(m.version)],
      ["host_name", m.hostName],
      ["porecatu_version", m.porecatuVersion],
      ["max_subscriptions", m.maxSubscriptions],
      ["host_instance", m.hostInstance],
    ]),
  fromBody: (r) => {
    const version = versionFromValue(r.required("version"));
    const hostName = asText(r.required("host_name"));
    const porecatuVersion = asText(r.required("porecatu_version"));
    const maxSubscriptions = asUint(r.required("max_subscriptions"));
    const hostInstance = asBytes(r.required("host_instance"));
    if (maxSubscriptions < 1 || hostInstance.length !== HOST_INSTANCE_LEN) {
      throw new DecodeError("out-of-domain");
    }
    return {
      type: "welcome",
      version,
      hostName,
      porecatuVersion,
      maxSubscriptions,
      hostInstance,
    };
  },
};

export const incompatibleCodec: BodyCodec<Incompatible> = {
  toBody: (m) => body([["version", versionToValue(m.version)]]),
  fromBody: (r) => ({
    type: "incompatible",
    version: versionFromValue(r.required("version")),
  }),
};

export const goodbyeCodec: BodyCodec<Goodbye> = {
  toBody: (m) => body([["reason", m.reason]]),
  fromBody: (r) => ({
    type: "goodbye",
    reason: enumFrom(GOODBYE_REASONS, asUint(r.required("reason"))),
  }),
};
