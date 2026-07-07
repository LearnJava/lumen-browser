//! `Alt-Svc` response-header parser and cache — RFC 7838 (HTTP Alternative
//! Services), the discovery path that upgrades an origin to HTTP/3.
//!
//! A server that also speaks HTTP/3 advertises it on an ordinary H1/H2 response
//! with a header such as
//!
//! ```text
//! Alt-Svc: h3=":443"; ma=86400, h3-29=":443"; ma=3600
//! ```
//!
//! Each comma-separated *alternative service* names a protocol id (ALPN token),
//! an optional `host:port` authority, and optional parameters — of which only
//! `ma` (max-age, seconds; RFC 7838 §3.1) and `persist` are defined. This
//! module turns that header value into [`AltSvcEntry`] records and remembers the
//! HTTP/3 ones per origin in an [`AltSvcCache`] so a later request to the same
//! origin can try QUIC first.
//!
//! ## Scope
//!
//! Pure parse + in-memory cache — no IO and no clock reads on the parse path.
//! The cache stores a caller-supplied deadline ([`std::time::Instant`]) so its
//! TTL logic is deterministic under test; [`AltSvcCache::insert`] and
//! [`AltSvcCache::get`] take the current instant as an argument. Only the thin
//! [`AltSvcCache::insert_now`] / [`AltSvcCache::get_now`] convenience wrappers
//! read the wall clock.
//!
//! ## What is (and isn't) parsed
//!
//! - Only the `h3` ALPN token is retained. Draft tokens (`h3-29`, `h3-Q050`)
//!   and same-protocol advertisements (`h2`) are recognised as valid syntax but
//!   dropped — Lumen only speaks final HTTP/3.
//! - The `clear` value (RFC 7838 §3.1) empties the cache entry for the origin.
//! - Unknown parameters are ignored (RFC 7838 §3: a recipient MUST ignore
//!   parameters it does not understand), not treated as errors.
//! - `persist=1` is parsed but not acted upon (it only matters across network
//!   changes, which this cache does not track).

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The final HTTP/3 ALPN token (RFC 9114 §3.1). Draft tokens like `h3-29` use
/// a different, incompatible wire format and are deliberately not accepted.
pub const ALPN_H3: &str = "h3";

/// Default `max-age` when an alternative omits the `ma` parameter: 24 hours,
/// per RFC 7838 §3.1 ("the default value is 86400 (24 hours)").
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(86_400);

/// One parsed alternative service entry (RFC 7838 §3) for the `h3` protocol.
///
/// `host` is `None` when the advertisement used the empty-host form (`h3=":443"`),
/// meaning "same host as the origin, different port".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltSvcEntry {
    /// Alternative host, or `None` to reuse the origin's host.
    pub host: Option<String>,
    /// Alternative UDP port for QUIC.
    pub port: u16,
    /// Freshness lifetime from the `ma` parameter (defaults to
    /// [`DEFAULT_MAX_AGE`] when absent).
    pub max_age: Duration,
    /// Whether the advertisement carried `persist=1` (RFC 7838 §3.1). Retained
    /// for completeness; the cache does not currently persist across restarts.
    pub persist: bool,
}

impl AltSvcEntry {
    /// The concrete `(host, port)` the QUIC leg should connect to for this
    /// alternative (RFC 7838 §3): the advertised `host` when present, otherwise
    /// the origin's own `origin_host` (the empty-host form `h3=":443"` means
    /// "same host, this UDP port"). The port is always the alternative's.
    ///
    /// The returned host feeds both DNS resolution and the TLS SNI /
    /// certificate-verification name for the QUIC handshake. RFC 7838 §3.1
    /// requires the alternative to present a certificate valid for the *origin*
    /// host; Lumen (like browsers) uses the connect host as the SNI and lets the
    /// certificate cover it — the finer origin-authentication nuance is out of
    /// scope for this slice, which only routes the connection.
    #[must_use]
    pub fn connect_target(&self, origin_host: &str) -> (String, u16) {
        let host = self.host.clone().unwrap_or_else(|| origin_host.to_owned());
        (host, self.port)
    }
}

/// The [`AltSvcCache`] key for an origin: its authority in `host:port` form
/// (RFC 9110 §4.3.1), keyed on the *original* request's host and port — not the
/// alternative's. Both the response-scan that inserts an advertisement and the
/// dispatch that looks one up must derive the key the same way, so they share
/// this one function.
#[must_use]
pub fn origin_key(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// Parse an `Alt-Svc` header value into the HTTP/3 alternatives it advertises.
///
/// Returns an empty vector when the value is the special `clear` token, when it
/// advertises no `h3` alternatives, or when it is syntactically malformed enough
/// that no complete alternative can be recovered (RFC 7838 §3: unparsable
/// members are ignored rather than aborting the whole header). Multiple `h3`
/// entries are returned in header order.
#[must_use]
pub fn parse(value: &str) -> Vec<AltSvcEntry> {
    let trimmed = value.trim();
    // RFC 7838 §3.1: the single token "clear" invalidates all alternatives.
    if trimmed.eq_ignore_ascii_case("clear") {
        return Vec::new();
    }

    let mut out = Vec::new();
    // Alternatives are comma-separated; a member with an unparsable authority
    // is skipped, not fatal.
    for member in trimmed.split(',') {
        if let Some(entry) = parse_member(member) {
            out.push(entry);
        }
    }
    out
}

/// Parse one comma-delimited alternative (`protocol="authority"; p=v; …`),
/// returning `Some` only for a well-formed `h3` advertisement.
fn parse_member(member: &str) -> Option<AltSvcEntry> {
    let mut parts = member.split(';');

    // First part is `protocol-id="alt-authority"` (RFC 7838 §3).
    let (proto, authority) = split_once_trim(parts.next()?, '=')?;
    if !proto.eq_ignore_ascii_case(ALPN_H3) {
        // Draft h3 tokens and non-h3 protocols: valid syntax, not for us.
        return None;
    }

    // The authority is a quoted string: `"host:port"` or `":port"`.
    let authority = unquote(authority.trim())?;
    let (host, port) = parse_authority(&authority)?;

    let mut max_age = DEFAULT_MAX_AGE;
    let mut persist = false;
    for param in parts {
        let Some((key, val)) = split_once_trim(param, '=') else {
            // A bare token with no '=' is not a defined parameter — ignore it.
            continue;
        };
        let val = unquote(val.trim()).unwrap_or_else(|| val.trim().to_string());
        match key.to_ascii_lowercase().as_str() {
            "ma" => {
                // ma is delta-seconds; a malformed value falls back to default.
                if let Ok(secs) = val.parse::<u64>() {
                    max_age = Duration::from_secs(secs);
                }
            }
            "persist" => persist = val == "1",
            _ => {} // RFC 7838 §3: ignore unknown parameters.
        }
    }

    Some(AltSvcEntry { host, port, max_age, persist })
}

/// Split an `alt-authority` (`host:port` or `:port`) into an optional host and a
/// port, returning `None` if the port is missing or not a valid `u16`.
fn parse_authority(authority: &str) -> Option<(Option<String>, u16)> {
    // The authority always ends in `:port`; host is whatever precedes the last
    // colon (empty → same host as origin).
    let (host_part, port_part) = authority.rsplit_once(':')?;
    let port: u16 = port_part.parse().ok()?;
    let host = if host_part.is_empty() { None } else { Some(host_part.to_string()) };
    Some((host, port))
}

/// Split `s` on the first `delim`, trimming ASCII whitespace from both halves;
/// `None` if `delim` is absent.
fn split_once_trim(s: &str, delim: char) -> Option<(&str, &str)> {
    let (a, b) = s.split_once(delim)?;
    Some((a.trim(), b.trim()))
}

/// Strip one layer of surrounding double quotes from `s`, returning the inner
/// text; unquoted input is returned as-is. `None` only for a lone `"` that has
/// no closing quote.
fn unquote(s: &str) -> Option<String> {
    if let Some(rest) = s.strip_prefix('"') {
        // Must have a matching closing quote.
        let inner = rest.strip_suffix('"')?;
        // RFC 7230 quoted-string: unescape `\c` back to `c`.
        let mut out = String::with_capacity(inner.len());
        let mut escaped = false;
        for ch in inner.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                out.push(ch);
            }
        }
        Some(out)
    } else {
        Some(s.to_string())
    }
}

/// A single cached alternative plus the instant it stops being fresh.
#[derive(Clone, Debug)]
struct CacheSlot {
    entry: AltSvcEntry,
    /// `inserted_at + max_age`; the entry is stale once `now >= expires_at`.
    expires_at: Instant,
}

/// Per-origin memory of advertised HTTP/3 alternatives (RFC 7838 §3).
///
/// Keyed by origin authority string (`host:port` of the *original* request, not
/// the alternative). A later lookup that is still fresh tells the fetch path to
/// try QUIC before falling back to the H2/H1 connection that produced the
/// advertisement.
#[derive(Debug, Default)]
pub struct AltSvcCache {
    entries: HashMap<String, CacheSlot>,
}

impl AltSvcCache {
    /// Create an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    /// Record the first `h3` alternative from a parsed `Alt-Svc` header for
    /// `origin`, expiring `now + entry.max_age`.
    ///
    /// An empty `alternatives` slice (e.g. from the `clear` token) removes any
    /// existing entry for the origin, matching RFC 7838 §3.1 invalidation.
    pub fn insert(&mut self, origin: &str, alternatives: &[AltSvcEntry], now: Instant) {
        match alternatives.first() {
            Some(entry) => {
                let expires_at = now + entry.max_age;
                self.entries
                    .insert(origin.to_string(), CacheSlot { entry: entry.clone(), expires_at });
            }
            None => {
                self.entries.remove(origin);
            }
        }
    }

    /// Look up a still-fresh HTTP/3 alternative for `origin` as of `now`.
    ///
    /// Returns `None` if there is no entry or it has expired; an expired entry
    /// is dropped as a side effect so the cache self-prunes on access.
    pub fn get(&mut self, origin: &str, now: Instant) -> Option<AltSvcEntry> {
        let slot = self.entries.get(origin)?;
        if now >= slot.expires_at {
            self.entries.remove(origin);
            return None;
        }
        Some(slot.entry.clone())
    }

    /// Clear any cached alternative for `origin` (RFC 7838 §2.4 "broken":
    /// called after a QUIC connection attempt fails so the origin falls back to
    /// H2/H1 and is not retried over h3).
    pub fn remove(&mut self, origin: &str) {
        self.entries.remove(origin);
    }

    /// [`insert`](Self::insert) using the current wall clock.
    pub fn insert_now(&mut self, origin: &str, alternatives: &[AltSvcEntry]) {
        self.insert(origin, alternatives, Instant::now());
    }

    /// [`get`](Self::get) using the current wall clock.
    pub fn get_now(&mut self, origin: &str) -> Option<AltSvcEntry> {
        self.get(origin, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical two-alternative header keeps only the `h3` entry with its
    /// max-age, dropping the `h3-29` draft advertisement.
    #[test]
    fn parses_h3_and_drops_draft() {
        let got = parse(r#"h3=":443"; ma=86400, h3-29=":443"; ma=3600"#);
        assert_eq!(
            got,
            vec![AltSvcEntry {
                host: None,
                port: 443,
                max_age: Duration::from_secs(86_400),
                persist: false,
            }]
        );
    }

    /// An explicit alternative host is retained.
    #[test]
    fn parses_explicit_host() {
        let got = parse(r#"h3="quic.example.net:8443"; ma=100; persist=1"#);
        assert_eq!(
            got,
            vec![AltSvcEntry {
                host: Some("quic.example.net".to_string()),
                port: 8443,
                max_age: Duration::from_secs(100),
                persist: true,
            }]
        );
    }

    /// Missing `ma` falls back to the RFC default of 24 hours.
    #[test]
    fn defaults_max_age_when_absent() {
        let got = parse(r#"h3=":443""#);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].max_age, DEFAULT_MAX_AGE);
    }

    /// The `clear` token yields no alternatives (case-insensitive).
    #[test]
    fn clear_token_empties() {
        assert!(parse("clear").is_empty());
        assert!(parse("  CLEAR  ").is_empty());
    }

    /// Non-h3 protocols and unparsable members are skipped without aborting the
    /// whole header.
    #[test]
    fn skips_non_h3_and_malformed() {
        // h2 alternative, then a member with a non-numeric port, then a valid h3.
        let got = parse(r#"h2=":443", h3=":notaport", h3=":8443"; ma=50"#);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].port, 8443);
        assert_eq!(got[0].max_age, Duration::from_secs(50));
    }

    /// Unknown parameters are ignored, a malformed `ma` falls back to default.
    #[test]
    fn ignores_unknown_params_and_bad_ma() {
        let got = parse(r#"h3=":443"; foo=bar; ma=notanumber; baz"#);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].max_age, DEFAULT_MAX_AGE);
    }

    /// A completely empty or whitespace value produces nothing.
    #[test]
    fn empty_value_is_empty() {
        assert!(parse("").is_empty());
        assert!(parse("   ").is_empty());
    }

    /// Cache insert/get round-trips while fresh and prunes once expired.
    #[test]
    fn cache_ttl_expiry() {
        let mut cache = AltSvcCache::new();
        let t0 = Instant::now();
        let alts = parse(r#"h3=":443"; ma=10"#);
        cache.insert("example.com:443", &alts, t0);

        // Fresh: available before the 10 s deadline.
        let hit = cache.get("example.com:443", t0 + Duration::from_secs(5)).expect("fresh");
        assert_eq!(hit.port, 443);

        // Expired: gone at/after the deadline, and self-pruned.
        assert!(cache.get("example.com:443", t0 + Duration::from_secs(10)).is_none());
        assert!(cache.entries.is_empty(), "expired slot removed on access");
    }

    /// Inserting an empty alternative set (from `clear`) removes the origin.
    #[test]
    fn cache_clear_removes_entry() {
        let mut cache = AltSvcCache::new();
        let t0 = Instant::now();
        cache.insert("example.com:443", &parse(r#"h3=":443""#), t0);
        assert!(cache.get("example.com:443", t0).is_some());

        cache.insert("example.com:443", &parse("clear"), t0);
        assert!(cache.get("example.com:443", t0).is_none());
    }

    /// The empty-host advertisement (`h3=":443"`) reuses the origin host; the
    /// port is always the alternative's.
    #[test]
    fn connect_target_reuses_origin_host_when_absent() {
        let entry = &parse(r#"h3=":8443""#)[0];
        assert_eq!(entry.connect_target("example.com"), ("example.com".to_owned(), 8443));
    }

    /// An explicit alternative host overrides the origin host.
    #[test]
    fn connect_target_uses_explicit_host() {
        let entry = &parse(r#"h3="quic.example.net:443""#)[0];
        assert_eq!(
            entry.connect_target("example.com"),
            ("quic.example.net".to_owned(), 443)
        );
    }

    /// The origin key is the `host:port` authority — the same string the insert
    /// and lookup sides must agree on.
    #[test]
    fn origin_key_is_host_colon_port() {
        assert_eq!(origin_key("example.com", 443), "example.com:443");
        assert_eq!(origin_key("example.com", 8443), "example.com:8443");
    }

    /// `remove` drops an origin (the "broken" fallback path).
    #[test]
    fn cache_remove_on_broken() {
        let mut cache = AltSvcCache::new();
        let t0 = Instant::now();
        cache.insert("example.com:443", &parse(r#"h3=":443""#), t0);
        cache.remove("example.com:443");
        assert!(cache.get("example.com:443", t0).is_none());
    }
}
