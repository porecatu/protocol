// SPDX-License-Identifier: GPL-3.0-or-later
//! Pairing URI carried by the QR code.
//!
//! ```text
//! porecatu://pair?v=0&relay=<url>&host=<host_id>&key=<pub>&token=<token>&name=<name>
//! ```
//!
//! The exact form is fixed by the specification (02, section 3.1): parameter
//! order, percent-encoding of every byte outside the RFC 3986 *unreserved*
//! set with uppercase hexadecimal, and a normalized `relay`. The module also
//! holds the relay URL rules shared with the transport (01, sections 1 and 2).

use core::fmt;

use crate::encoding;
use crate::host_id::{HostId, HostIdError};

/// Major version of the protocol, the `v` parameter of the URI and part of
/// the Noise prologue.
pub const PROTOCOL_MAJOR: u64 = 0;

/// Maximum size in bytes of a device name (`pair-request.device`).
pub const DEVICE_NAME_MAX_BYTES: usize = 64;

/// Maximum number of Unicode scalar values of the host name in the URI.
pub const HOST_NAME_MAX_CHARS: usize = 32;

const SCHEME_PREFIX: &str = "porecatu://pair";
const PATH_VERSION: &str = "v0";

/// Why a pairing URI (or a relay URL) was refused. The variants map to the
/// `uri` error codes of the test vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingUriError {
    /// Not a `porecatu://pair` URI (`not-porecatu`).
    NotPorecatu,
    /// A required parameter is absent (`missing-field`).
    MissingField(&'static str),
    /// A parameter appears more than once (`duplicate-field`).
    DuplicateField(&'static str),
    /// A parameter has an invalid value (`malformed-field`).
    MalformedField(&'static str),
    /// `v` differs from [`PROTOCOL_MAJOR`] (`version-mismatch`).
    VersionMismatch,
    /// `relay` is not `wss://` and not a local `ws://` address
    /// (`insecure-relay`).
    InsecureRelay,
}

impl PairingUriError {
    /// The code used by the test vectors (`erro.codigo` with `camada: uri`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotPorecatu => "not-porecatu",
            Self::MissingField(_) => "missing-field",
            Self::DuplicateField(_) => "duplicate-field",
            Self::MalformedField(_) => "malformed-field",
            Self::VersionMismatch => "version-mismatch",
            Self::InsecureRelay => "insecure-relay",
        }
    }
}

impl fmt::Display for PairingUriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPorecatu => f.write_str("not a porecatu pairing URI"),
            Self::MissingField(name) => write!(f, "missing field `{name}`"),
            Self::DuplicateField(name) => write!(f, "duplicate field `{name}`"),
            Self::MalformedField(name) => write!(f, "malformed field `{name}`"),
            Self::VersionMismatch => f.write_str("protocol version mismatch"),
            Self::InsecureRelay => f.write_str("relay must be wss:// (or ws:// on a local host)"),
        }
    }
}

impl core::error::Error for PairingUriError {}

/// Contents of the pairing URI.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingUri {
    /// Major protocol version (`v`).
    pub major: u64,
    /// Relay base URL. [`PairingUri::to_uri`] normalizes it; `parse` returns
    /// it already normalized.
    pub relay: String,
    /// Address of the host at the relay.
    pub host_id: HostId,
    /// Static public key of the host.
    pub host_key: [u8; 32],
    /// One-use pairing token. Secret.
    pub token: [u8; 16],
    /// Display name of the host, 1 to 32 Unicode scalar values.
    pub name: String,
}

impl fmt::Debug for PairingUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingUri")
            .field("major", &self.major)
            .field("relay", &self.relay)
            .field("host_id", &self.host_id)
            .field("host_key", &self.host_key)
            .field("token", &"<redacted>")
            .field("name", &self.name)
            .finish()
    }
}

impl PairingUri {
    /// Serializes in the exact form of the specification. A `relay` that
    /// does not normalize is written as it is (the reader will refuse it).
    pub fn to_uri(&self) -> String {
        let relay = normalize_relay_url(&self.relay).unwrap_or_else(|_| self.relay.clone());
        let mut out = String::with_capacity(256);
        out.push_str(SCHEME_PREFIX);
        out.push_str("?v=");
        out.push_str(&self.major.to_string());
        out.push_str("&relay=");
        percent_encode_into(&mut out, relay.as_bytes());
        out.push_str("&host=");
        percent_encode_into(&mut out, self.host_id.as_str().as_bytes());
        out.push_str("&key=");
        out.push_str(&encoding::encode_base64url(&self.host_key));
        out.push_str("&token=");
        out.push_str(&encoding::encode_base64url(&self.token));
        out.push_str("&name=");
        percent_encode_into(&mut out, self.name.as_bytes());
        out
    }

    /// Parses a pairing URI. Unknown parameters are ignored; a required one
    /// that is absent, repeated or malformed is refused.
    pub fn parse(uri: &str) -> Result<Self, PairingUriError> {
        // The scheme is case-insensitive; the `pair` path is exact. A
        // fragment is ignored.
        let rest =
            strip_prefix_ignore_case(uri, "porecatu://").ok_or(PairingUriError::NotPorecatu)?;
        let rest = rest.split('#').next().unwrap_or("");
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        if path != "pair" {
            return Err(PairingUriError::NotPorecatu);
        }

        const FIELDS: [&str; 6] = ["v", "relay", "host", "key", "token", "name"];
        let mut values: [Option<&str>; 6] = [None; 6];
        let mut duplicated = [false; 6];
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if let Some(i) = FIELDS.iter().position(|f| *f == key) {
                if values[i].is_some() {
                    duplicated[i] = true;
                } else {
                    values[i] = Some(value);
                }
            }
        }
        let raw = |i: usize| -> Result<&str, PairingUriError> {
            if duplicated[i] {
                return Err(PairingUriError::DuplicateField(FIELDS[i]));
            }
            values[i].ok_or(PairingUriError::MissingField(FIELDS[i]))
        };

        // The version decides whether the rest is even readable.
        let major = parse_major(&percent_decode_str(raw(0)?, "v")?)?;
        if major != PROTOCOL_MAJOR {
            return Err(PairingUriError::VersionMismatch);
        }

        let relay = normalize_relay_url(&percent_decode_str(raw(1)?, "relay")?)?;
        if !is_secure_normalized(&relay) {
            return Err(PairingUriError::InsecureRelay);
        }
        let host_id = HostId::parse(&percent_decode_str(raw(2)?, "host")?)
            .map_err(|_: HostIdError| PairingUriError::MalformedField("host"))?;
        let host_key = decode_fixed::<32>(&percent_decode_str(raw(3)?, "key")?, "key")?;
        let token = decode_fixed::<16>(&percent_decode_str(raw(4)?, "token")?, "token")?;
        let name = percent_decode_str(raw(5)?, "name")?;
        let name_chars = name.chars().count();
        if name_chars == 0 || name_chars > HOST_NAME_MAX_CHARS {
            return Err(PairingUriError::MalformedField("name"));
        }

        Ok(Self {
            major,
            relay,
            host_id,
            host_key,
            token,
            name,
        })
    }
}

fn parse_major(text: &str) -> Result<u64, PairingUriError> {
    let malformed = PairingUriError::MalformedField("v");
    // 1 to 9 digits, as in the TypeScript implementation (leading zeros read
    // as the number).
    if text.is_empty() || text.len() > 9 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(malformed);
    }
    text.parse().map_err(|_| malformed)
}

fn decode_fixed<const N: usize>(
    text: &str,
    field: &'static str,
) -> Result<[u8; N], PairingUriError> {
    let malformed = PairingUriError::MalformedField(field);
    let bytes = encoding::decode_base64url(text).map_err(|_| malformed)?;
    bytes.as_slice().try_into().map_err(|_| malformed)
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

// ---- percent-encoding ------------------------------------------------------

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn percent_encode_into(out: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in bytes {
        if is_unreserved(byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decodes `%HH` sequences (either hex case on read; `+` is a literal plus)
/// and requires the result to be UTF-8.
fn percent_decode_str(text: &str, field: &'static str) -> Result<String, PairingUriError> {
    let malformed = PairingUriError::MalformedField(field);
    let input = text.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            let hi = input.get(i + 1).copied().and_then(hex_value);
            let lo = input.get(i + 2).copied().and_then(hex_value);
            match (hi, lo) {
                (Some(hi), Some(lo)) => out.push((hi << 4) | lo),
                _ => return Err(malformed),
            }
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| malformed)
}

// ---- relay URL ---------------------------------------------------------------

/// Normalizes the relay base URL: scheme and host in lowercase, no trailing
/// slash, no query, no fragment; optional port and path prefix are kept.
///
/// The port is kept as written (`:443` stays) and so is the case of the path.
/// Only `ws` and `wss` are accepted; user info, an empty host, a bad port, a
/// path with characters outside RFC 3986 (or an invalid `%HH`) and any other
/// scheme are `MalformedField("relay")`. This does **not** check security
/// (a `ws://` to a remote host normalizes fine): see [`is_secure_relay_url`].
pub fn normalize_relay_url(url: &str) -> Result<String, PairingUriError> {
    let malformed = PairingUriError::MalformedField("relay");

    let (scheme, rest) = url.split_once("://").ok_or(malformed)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "ws" && scheme != "wss" {
        return Err(malformed);
    }

    // Query and fragment are not part of a base URL.
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err(malformed);
    }

    let (host, port) = split_host_port(authority).ok_or(malformed)?;
    let host = host.to_ascii_lowercase();

    if !valid_path(path) {
        return Err(malformed);
    }
    let path = path.trim_end_matches('/');

    let mut out = format!("{scheme}://{host}");
    if let Some(port) = port {
        out.push(':');
        out.push_str(port);
    }
    out.push_str(path);
    Ok(out)
}

/// RFC 3986 path characters; `%` only as a valid `%HH`.
fn valid_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let hi = bytes.get(i + 1).copied().and_then(hex_value);
            let lo = bytes.get(i + 2).copied().and_then(hex_value);
            if hi.is_none() || lo.is_none() {
                return false;
            }
            i += 3;
        } else if b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&b) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

fn split_host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed.find(']')?;
        let inner = &bracketed[..end];
        if !inner.contains(':')
            || !inner
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
        {
            return None;
        }
        let after = &bracketed[end + 1..];
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return None,
        };
        (&authority[..end + 2], port)
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };

    if !host.starts_with('[') && (host.is_empty() || !host.bytes().all(is_unreserved)) {
        return None;
    }

    if let Some(text) = port {
        // 1 to 5 digits, at most 65535; kept as written.
        if text.is_empty()
            || text.len() > 5
            || !text.bytes().all(|b| b.is_ascii_digit())
            || text.parse::<u32>().is_ok_and(|p| p > 65535)
        {
            return None;
        }
    }
    Some((host, port))
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// `wss://`, or `ws://` to a local host, on an already normalized URL.
fn is_secure_normalized(normalized: &str) -> bool {
    let Some(rest) = normalized.strip_prefix("ws://") else {
        return normalized.starts_with("wss://");
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = if authority.starts_with('[') {
        authority.split_inclusive(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    is_local_host(host)
}

/// Whether the relay may be used: `wss://`, or `ws://` only to `localhost`,
/// `127.0.0.1` or `[::1]` (development). A URL that does not normalize is not
/// secure.
pub fn is_secure_relay_url(url: &str) -> bool {
    normalize_relay_url(url).is_ok_and(|normalized| is_secure_normalized(&normalized))
}

/// The base URL normalized when possible, otherwise just without trailing
/// slashes (the callers are tolerant, unlike the TypeScript side that throws).
fn base_or_trimmed(relay_base: &str) -> String {
    normalize_relay_url(relay_base).unwrap_or_else(|_| relay_base.trim_end_matches('/').to_owned())
}

/// URL of the presence query (01, section 2): `wss://` becomes `https://`,
/// `ws://` becomes `http://`, same authority and path prefix, followed by
/// `/v0/presence/<host_id>`.
pub fn presence_url(relay_base: &str, host_id: &HostId) -> String {
    let base = base_or_trimmed(relay_base);
    let http = if let Some(rest) = base.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = base.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        base
    };
    format!("{http}/{PATH_VERSION}/presence/{host_id}")
}

/// URL where the client opens its channel (01, section 2): the relay base
/// URL followed by `/v0/client/<host_id>`, scheme unchanged.
pub fn client_channel_url(relay_base: &str, host_id: &HostId) -> String {
    let base = base_or_trimmed(relay_base);
    format!("{base}/{PATH_VERSION}/client/{host_id}")
}

/// Cuts a device name to [`DEVICE_NAME_MAX_BYTES`] bytes without splitting a
/// character. An empty name stays empty (callers refuse it).
pub fn clamp_device_name(name: &str) -> String {
    if name.len() <= DEVICE_NAME_MAX_BYTES {
        return name.to_owned();
    }
    let mut end = DEVICE_NAME_MAX_BYTES;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_id::host_id;

    const KEY_B_B64: &str = "__________________________________________8";
    // base64url of bytes 00..0f, computed with Python:
    //   base64.urlsafe_b64encode(bytes(range(16))).decode().rstrip('=')
    const TOKEN_B64: &str = "AAECAwQFBgcICQoLDA0ODw";
    // host_id of the key 00..1f (see host_id tests, computed with Python).
    const HOST_A: &str = "awbfmb6x7xznqlxuypemflvjmgwzrvqo";

    const EXPECTED_URI: &str = "porecatu://pair?v=0\
        &relay=wss%3A%2F%2Frelay.exemplo.com.br%3A8443%2Fporecatu\
        &host=awbfmb6x7xznqlxuypemflvjmgwzrvqo\
        &key=__________________________________________8\
        &token=AAECAwQFBgcICQoLDA0ODw\
        &name=Mesa%20do%20L%C3%A9o%20%28casa%29";

    fn sample() -> PairingUri {
        let key_a: [u8; 32] = core::array::from_fn(|i| i as u8);
        PairingUri {
            major: 0,
            relay: "wss://Relay.Exemplo.com.br:8443/porecatu/".to_owned(),
            host_id: host_id(&key_a),
            host_key: [0xff; 32],
            token: core::array::from_fn(|i| i as u8),
            name: "Mesa do Léo (casa)".to_owned(),
        }
    }

    /// A valid URI with one parameter replaced (`Some`), removed (`None`).
    fn uri_with(field: &str, value: Option<&str>) -> String {
        let defaults = [
            ("v", "0".to_owned()),
            ("relay", "wss%3A%2F%2Frelay.example.com".to_owned()),
            ("host", HOST_A.to_owned()),
            ("key", KEY_B_B64.to_owned()),
            ("token", TOKEN_B64.to_owned()),
            ("name", "Desk".to_owned()),
        ];
        let params: Vec<String> = defaults
            .iter()
            .filter_map(|(name, default)| match (*name == field, value) {
                (true, None) => None,
                (true, Some(v)) => Some(format!("{name}={v}")),
                (false, _) => Some(format!("{name}={default}")),
            })
            .collect();
        format!("porecatu://pair?{}", params.join("&"))
    }

    #[test]
    fn to_uri_is_exact() {
        assert_eq!(sample().to_uri(), EXPECTED_URI);
    }

    #[test]
    fn parse_roundtrips_with_normalized_relay() {
        let parsed = PairingUri::parse(EXPECTED_URI).unwrap();
        let mut expected = sample();
        expected.relay = "wss://relay.exemplo.com.br:8443/porecatu".to_owned();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_uri(), EXPECTED_URI);
    }

    #[test]
    fn debug_redacts_the_token() {
        let text = format!("{:?}", sample());
        assert!(text.contains("<redacted>"));
        assert!(!text.contains("[0, 1, 2"));
    }

    #[test]
    fn unknown_parameters_are_ignored() {
        let uri = format!("{EXPECTED_URI}&future=%ZZ&other");
        assert!(PairingUri::parse(&uri).is_ok());
        let uri = EXPECTED_URI.replacen("?v=0", "?extra=1&v=0", 1);
        assert!(PairingUri::parse(&uri).is_ok());
    }

    #[test]
    fn repeated_parameter_is_refused() {
        let uri = format!("{EXPECTED_URI}&name=Other");
        assert_eq!(
            PairingUri::parse(&uri),
            Err(PairingUriError::DuplicateField("name"))
        );
        let uri = format!("{EXPECTED_URI}&v=0");
        assert_eq!(
            PairingUri::parse(&uri),
            Err(PairingUriError::DuplicateField("v"))
        );
    }

    #[test]
    fn missing_parameter_is_refused() {
        for field in ["v", "relay", "host", "key", "token", "name"] {
            let leaked = PairingUri::parse(&uri_with(field, None));
            match leaked {
                Err(PairingUriError::MissingField(name)) => assert_eq!(name, field),
                other => panic!("{field}: {other:?}"),
            }
        }
    }

    #[test]
    fn not_a_porecatu_uri() {
        for uri in [
            "",
            "https://pair?v=0",
            "porecatu://other?v=0",
            "porecatu://pairing?v=0",
            "porecatu:pair?v=0",
        ] {
            assert_eq!(
                PairingUri::parse(uri),
                Err(PairingUriError::NotPorecatu),
                "{uri}"
            );
        }
        assert_eq!(
            PairingUri::parse("porecatu://pair"),
            Err(PairingUriError::MissingField("v"))
        );
    }

    #[test]
    fn version_must_match() {
        assert_eq!(
            PairingUri::parse(&uri_with("v", Some("1"))),
            Err(PairingUriError::VersionMismatch)
        );
        assert_eq!(
            PairingUri::parse(&uri_with("v", Some("x"))),
            Err(PairingUriError::MalformedField("v"))
        );
        // 1 to 9 digits; leading zeros read as the number (as in TypeScript).
        assert!(PairingUri::parse(&uri_with("v", Some("00"))).is_ok());
        assert_eq!(
            PairingUri::parse(&uri_with("v", Some("0000000000"))),
            Err(PairingUriError::MalformedField("v"))
        );
        assert_eq!(
            PairingUri::parse(&uri_with("v", Some(""))),
            Err(PairingUriError::MalformedField("v"))
        );
        // A different major is refused before anything else is read.
        for uri in ["porecatu://pair?v=1&relay=junk", "porecatu://pair?v=1"] {
            assert_eq!(
                PairingUri::parse(uri),
                Err(PairingUriError::VersionMismatch)
            );
        }
    }

    #[test]
    fn reader_leniencies_and_empty_values() {
        // Scheme in any case, fragment ignored, raw characters outside the
        // unreserved set accepted in a value.
        let uri = EXPECTED_URI.replacen("porecatu://", "PoReCaTu://", 1);
        assert!(PairingUri::parse(&format!("{uri}#frag")).is_ok());
        let parsed = PairingUri::parse(&uri_with("name", Some("Léo (casa)"))).unwrap();
        assert_eq!(parsed.name, "Léo (casa)");
        // A known field with an empty value is malformed, not missing.
        for field in ["relay", "host", "key", "token", "name"] {
            assert_eq!(
                PairingUri::parse(&uri_with(field, Some(""))),
                Err(PairingUriError::MalformedField(match field {
                    "relay" => "relay",
                    "host" => "host",
                    "key" => "key",
                    "token" => "token",
                    _ => "name",
                })),
                "{field}"
            );
        }
        // The `pair` path is exact.
        assert_eq!(
            PairingUri::parse("porecatu://PAIR?v=0"),
            Err(PairingUriError::NotPorecatu)
        );
        assert_eq!(
            PairingUri::parse("porecatu://pair/?v=0"),
            Err(PairingUriError::NotPorecatu)
        );
        // Validation order: insecure relay is reported before a bad host.
        let uri = "porecatu://pair?v=0&relay=ws%3A%2F%2Fexample.com&host=bad";
        assert_eq!(PairingUri::parse(uri), Err(PairingUriError::InsecureRelay));
    }

    #[test]
    fn insecure_relay_is_refused() {
        assert_eq!(
            PairingUri::parse(&uri_with("relay", Some("ws%3A%2F%2Frelay.example.com"))),
            Err(PairingUriError::InsecureRelay)
        );
        assert_eq!(
            PairingUri::parse(&uri_with("relay", Some("ws%3A%2F%2Flocalhost.evil.com"))),
            Err(PairingUriError::InsecureRelay)
        );
    }

    #[test]
    fn local_ws_relays_are_accepted() {
        let parsed =
            PairingUri::parse(&uri_with("relay", Some("ws%3A%2F%2F%5B%3A%3A1%5D%3A8080"))).unwrap();
        assert_eq!(parsed.relay, "ws://[::1]:8080");
        // The encoded form of the normalized relay reads back the same.
        assert_eq!(PairingUri::parse(&parsed.to_uri()).unwrap(), parsed);
        for relay in [
            "ws://localhost",
            "ws://127.0.0.1:9000",
            "ws://LOCALHOST:1/p",
        ] {
            assert!(is_secure_relay_url(relay), "{relay}");
        }
    }

    #[test]
    fn key_and_token_sizes_are_checked() {
        let key31 = encoding::encode_base64url(&[1u8; 31]);
        assert_eq!(
            PairingUri::parse(&uri_with("key", Some(&key31))),
            Err(PairingUriError::MalformedField("key"))
        );
        let key33 = encoding::encode_base64url(&[1u8; 33]);
        assert_eq!(
            PairingUri::parse(&uri_with("key", Some(&key33))),
            Err(PairingUriError::MalformedField("key"))
        );
        let token17 = encoding::encode_base64url(&[1u8; 17]);
        assert_eq!(
            PairingUri::parse(&uri_with("token", Some(&token17))),
            Err(PairingUriError::MalformedField("token"))
        );
        // Padding is refused.
        let padded = format!("{KEY_B_B64}=");
        assert_eq!(
            PairingUri::parse(&uri_with("key", Some(&padded))),
            Err(PairingUriError::MalformedField("key"))
        );
    }

    #[test]
    fn host_must_be_a_host_id() {
        assert_eq!(
            PairingUri::parse(&uri_with("host", Some("short"))),
            Err(PairingUriError::MalformedField("host"))
        );
    }

    #[test]
    fn name_length_counts_scalar_values() {
        let thirty_two = "a".repeat(32);
        assert!(PairingUri::parse(&uri_with("name", Some(&thirty_two))).is_ok());
        let thirty_three = "a".repeat(33);
        assert_eq!(
            PairingUri::parse(&uri_with("name", Some(&thirty_three))),
            Err(PairingUriError::MalformedField("name"))
        );
        assert_eq!(
            PairingUri::parse(&uri_with("name", Some(""))),
            Err(PairingUriError::MalformedField("name"))
        );

        // An emoji counts as one value (4 bytes, 12 characters once encoded).
        let mut uri = sample();
        uri.name = "\u{1F600}".repeat(32);
        assert_eq!(PairingUri::parse(&uri.to_uri()).unwrap().name, uri.name);
        uri.name = "\u{1F600}".repeat(33);
        assert_eq!(
            PairingUri::parse(&uri.to_uri()),
            Err(PairingUriError::MalformedField("name"))
        );
    }

    #[test]
    fn percent_decoding_is_strict() {
        for bad in ["%", "%4", "%G1", "%FF", "%C3"] {
            assert_eq!(
                PairingUri::parse(&uri_with("name", Some(bad))),
                Err(PairingUriError::MalformedField("name")),
                "{bad}"
            );
        }
        // Lowercase hex is read, `+` is a plus sign.
        let parsed = PairingUri::parse(&uri_with("name", Some("a%c3%a9+b"))).unwrap();
        assert_eq!(parsed.name, "aé+b");
        // The writer always uses uppercase hex.
        let mut uri = sample();
        uri.name = "é+ ~-._".to_owned();
        assert!(uri.to_uri().ends_with("&name=%C3%A9%2B%20~-._"));
    }

    #[test]
    fn normalize_relay_cases() {
        let cases = [
            ("wss://relay.example.com", "wss://relay.example.com"),
            ("WSS://Relay.Example.COM/", "wss://relay.example.com"),
            (
                "wss://relay.example.com:443/",
                "wss://relay.example.com:443",
            ),
            ("wss://example.com/relay//", "wss://example.com/relay"),
            ("wss://example.com/a/b?x=1#frag", "wss://example.com/a/b"),
            ("wss://example.com?x=1", "wss://example.com"),
            ("ws://[::1]:8080/", "ws://[::1]:8080"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_relay_url(input).as_deref(),
                Ok(expected),
                "{input}"
            );
        }
        let bad = [
            "relay.example.com",
            "https://relay.example.com",
            "wss://",
            "wss://user@relay.example.com",
            "wss://relay.example.com:99999",
            "wss://relay.example.com:",
            "wss://relay example.com",
            "wss://relay.example.com/a b",
            "wss://[::1",
        ];
        for input in bad {
            assert_eq!(
                normalize_relay_url(input),
                Err(PairingUriError::MalformedField("relay")),
                "{input}"
            );
        }
        // Normalization does not judge security; `is_secure_relay_url` does.
        assert_eq!(
            normalize_relay_url("ws://relay.example.com").as_deref(),
            Ok("ws://relay.example.com")
        );
        assert!(!is_secure_relay_url("ws://relay.example.com"));
        assert!(!is_secure_relay_url("ws://localhost.evil.com"));
        assert!(!is_secure_relay_url("https://relay.example.com"));
        assert!(is_secure_relay_url("wss://relay.example.com"));
    }

    #[test]
    fn normalize_relay_mirrors_the_typescript_rules() {
        let cases = [
            // Port kept as written, 0 included; path case kept.
            ("wss://relay.example.com:0", "wss://relay.example.com:0"),
            (
                "wss://Relay.example.com:65535/Mixed/Case",
                "wss://relay.example.com:65535/Mixed/Case",
            ),
            // IPv6 in brackets is lowercased.
            ("wss://[2001:DB8::1]:8443", "wss://[2001:db8::1]:8443"),
            // RFC 3986 path characters and valid %HH.
            (
                "wss://example.com/a%20b/c:d@e;f=g",
                "wss://example.com/a%20b/c:d@e;f=g",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_relay_url(input).as_deref(),
                Ok(expected),
                "{input}"
            );
        }
        for input in [
            "wss://relay.example.com:65536",
            "wss://relay.example.com:123456",
            "wss://relay.example.com/a%zz",
            "wss://relay.example.com/a%2",
            "wss://relay.example.com/caf\u{e9}",
            "wss://[abc]",
            "wss://[::1]x",
            "wss://[::1]:",
        ] {
            assert_eq!(
                normalize_relay_url(input),
                Err(PairingUriError::MalformedField("relay")),
                "{input}"
            );
        }
    }

    #[test]
    fn relay_urls_for_presence_and_channel() {
        let id = HostId::parse(HOST_A).unwrap();
        assert_eq!(
            presence_url("wss://exemplo.com.br/relay", &id),
            format!("https://exemplo.com.br/relay/v0/presence/{HOST_A}")
        );
        assert_eq!(
            presence_url("wss://exemplo.com.br/", &id),
            format!("https://exemplo.com.br/v0/presence/{HOST_A}")
        );
        assert_eq!(
            presence_url("ws://localhost:8080", &id),
            format!("http://localhost:8080/v0/presence/{HOST_A}")
        );
        assert_eq!(
            client_channel_url("wss://exemplo.com.br/relay/", &id),
            format!("wss://exemplo.com.br/relay/v0/client/{HOST_A}")
        );
        assert_eq!(
            client_channel_url("ws://127.0.0.1:9", &id),
            format!("ws://127.0.0.1:9/v0/client/{HOST_A}")
        );
    }

    #[test]
    fn device_name_is_clamped_by_bytes() {
        assert_eq!(clamp_device_name(""), "");
        assert_eq!(clamp_device_name("Pixel 8"), "Pixel 8");
        let exact = "a".repeat(64);
        assert_eq!(clamp_device_name(&exact), exact);
        assert_eq!(clamp_device_name(&"a".repeat(80)), exact);
        // 32 two-byte characters fit exactly; one more byte must not split one.
        let two_byte = "é".repeat(40);
        assert_eq!(clamp_device_name(&two_byte), "é".repeat(32));
        let odd = format!("a{}", "é".repeat(40));
        let clamped = clamp_device_name(&odd);
        assert_eq!(clamped, format!("a{}", "é".repeat(31)));
        assert!(clamped.len() <= DEVICE_NAME_MAX_BYTES);
    }
}
