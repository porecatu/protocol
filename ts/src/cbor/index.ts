// SPDX-License-Identifier: GPL-3.0-or-later

export { decodeCbor } from "./decode.js";
export { encodeCbor, MAX_DEPTH } from "./encode.js";
export { CborError, type CborErrorReason } from "./errors.js";
export {
  asArray,
  asBool,
  asBytes,
  asInt,
  asMap,
  asText,
  asUint,
  expectArray,
  isNull,
  mapReader,
  type MapReader,
} from "./reader.js";
export { CborMap, type CborValue } from "./value.js";
