//! Lumen URL — структурированный тип.
//!
//! LIB-6 (ADR-027, docs/conformance/2026-08-31.md): scheme/host/path/query/
//! fragment splitting, percent-encoding normalization, dot-segment removal
//! and relative-reference resolution are the WHATWG URL Standard's state
//! machine — a spec decides what's correct here, not us — so [`WhatwgUrl`]
//! (the `url` crate) is the parsing engine (`inner` field below). The own
//! hand-rolled parser this replaced passed only 41.95% of
//! `tests/wpt/url/resources/urltestdata.json` (`cargo test -p lumen-core
//! --test url_conformance_census -- --ignored --nocapture`), concentrated in
//! two entirely missing algorithm branches (IPv6 `[...]` literals, forbidden/
//! control host code point handling) rather than isolated IDNA/percent-
//! encoding gaps.
//!
//! **[`Url::host`] deliberately does NOT come from `inner`.** The WHATWG
//! parser always ASCII/IDNA-normalizes (Punycode + lowercase) the host it
//! stores, discarding the original text — but the address bar's IDN
//! homograph-spoof guard (`crates/shell/src/address_bar.rs::
//! guard_display_text`, DS-6) substring-replaces the *exact* text the user
//! typed or the exact serialization shown on screen, and needs the original
//! Unicode/original-case substring to find it. `host()` therefore keeps its
//! own best-effort raw extraction ([`raw_host_from_input`]) — mirroring
//! `inner`'s accept/reject decision (so `Url::parse`/[`Url::resolve`] reject
//! exactly what the spec rejects) but not its normalization. `serialized`
//! (backing [`Url::as_str`]/[`Display`]) is rebuilt from `inner`'s
//! spec-correct scheme/port/path/query/fragment with this raw host spliced
//! back in, for the same reason — the address bar shows the Unicode form.
//!
//! ASCII-form host (Punycode, DNS/TLS SNI/`Host:` header) is
//! [`Url::host_ascii`] — unrelated to `inner`'s own normalization, still
//! powered by `crate::idn`/`crate::punycode` (LIB-6: this stays ours, it is
//! the product decision of *what to show the user*, not a parsing question).
//!
//! Сознательно не реализуем здесь:
//! - IDNA UTS #46 mapping/validation for [`Url::host`] (zero-width
//!   collapsing, ideographic full stop, confusable normalization) — the raw
//!   substring is kept as-is; [`crate::idn::display_host`] does its own,
//!   unrelated homograph/mixed-script detection on top of it.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use crate::error::{Error, Result};
use std::fmt;
use url::Url as WhatwgUrl;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Url {
    /// Raw, unnormalized host substring (Unicode, original case) — see the
    /// module doc for why this cannot come from `inner`.
    host: String,
    /// `scheme://host(raw)[:port]path[?query][#fragment]` — same shape as
    /// `inner.as_str()` except the host is the raw substring above.
    serialized: String,
    /// WHATWG-conformant backing: source of scheme/port/path/query/fragment
    /// and of `.join()` for [`Url::resolve`].
    inner: WhatwgUrl,
}

impl Url {
    /// Распарсить URL по WHATWG URL Standard (через `inner`). `host()` на
    /// результате — raw substring из `s`, не ASCII/IDNA-нормализованный.
    pub fn parse(s: &str) -> Result<Self> {
        let inner =
            WhatwgUrl::parse(s).map_err(|e| Error::InvalidUrl(format!("{s:?}: {e}")))?;
        let host = raw_host_from_input(s);
        let serialized = serialize(&inner, &host);
        Ok(Self { host, serialized, inner })
    }

    pub fn scheme(&self) -> &str {
        self.inner.scheme()
    }

    /// Raw (unnormalized) host — Unicode, original case, exactly as it
    /// appeared in the parsed input. See the module doc.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.inner.port()
    }

    pub fn path(&self) -> &str {
        self.inner.path()
    }

    pub fn query(&self) -> Option<&str> {
        self.inner.query()
    }

    pub fn fragment(&self) -> Option<&str> {
        self.inner.fragment()
    }

    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// Порт с учётом дефолтов известных схем.
    pub fn effective_port(&self) -> Option<u16> {
        self.inner.port_or_known_default()
    }

    /// Host в ASCII-форме (Punycode) — для DNS, TLS SNI, Host header.
    /// Пустой host (например, `data:`) даёт пустую строку без ошибки.
    pub fn host_ascii(&self) -> Result<String> {
        if self.host.is_empty() {
            return Ok(String::new());
        }
        crate::idn::domain_to_ascii(&self.host).map_err(|e| {
            Error::InvalidUrl(format!("idn conversion failed for '{}': {e}", self.host))
        })
    }

    /// Path + `?query` (без fragment) — для HTTP request line.
    pub fn path_and_query(&self) -> String {
        match self.inner.query() {
            Some(q) => format!("{}?{}", self.inner.path(), q),
            None => self.inner.path().to_owned(),
        }
    }

    /// Разрешить относительный или абсолютный `reference` относительно
    /// `self`, через WHATWG "basic URL parser with base" (`inner.join`).
    /// Host — своя raw-экстракция из `reference`, если тот несёт собственный
    /// host (абсолютный или protocol-relative `//host/...`), иначе
    /// наследуется от `self.host` неизменным (как и в WHATWG-алгоритме).
    pub fn resolve(&self, reference: &str) -> Result<Self> {
        let joined = self
            .inner
            .join(reference)
            .map_err(|e| Error::InvalidUrl(format!("{reference:?}: {e}")))?;
        let host = if has_scheme(reference) || reference.starts_with("//") {
            raw_host_from_input(reference)
        } else {
            self.host.clone()
        };
        let serialized = serialize(&joined, &host);
        Ok(Self { host, serialized, inner: joined })
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized)
    }
}

// ── Raw host extraction (display-only, see module doc) ─────────────────────

/// scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
fn has_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

/// WHATWG basic URL parser steps 1–2: trim leading/trailing C0 control or
/// space, then remove every embedded ASCII tab/CR/LF. Applied before our own
/// raw host extraction so it agrees with what `inner` actually parsed.
fn preprocess(s: &str) -> String {
    let trimmed = s.trim_matches(|c: char| (c as u32) <= 0x20);
    trimmed.chars().filter(|&c| !matches!(c, '\t' | '\n' | '\r')).collect()
}

/// Schemes whose "special authority ignore slashes state" (WHATWG §4) skips
/// extra `/`/`\` right after the scheme's own `//` rather than reading them
/// as an empty-authority delimiter — verified against the `url` crate:
/// `http:////path`/`http://\path` both resolve to host "path". `file` is
/// deliberately excluded: it runs its own "file host state", which reads
/// straight-to-empty-host-then-path on a third slash instead
/// (`file:///tmp/x` -> host "", path "/tmp/x", also verified against the
/// crate — the crate's own `has_host()` is even `false` there, not `true`
/// with an empty string, though the WHATWG serializer still shows `//`;
/// [`serialize`] reads that decision off `inner.as_str()` instead of
/// re-deriving it for exactly this reason).
fn ignores_extra_authority_slashes(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp")
}

/// Best-effort raw (unnormalized) host substring for display — see module
/// doc. Not a validator: `Url::parse`/`resolve` already rejected anything
/// `inner` (the WHATWG parser) rejects before this ever runs.
fn raw_host_from_input(s: &str) -> String {
    let s = preprocess(s);
    let (scheme, rest): (&str, &str) = if has_scheme(&s) {
        match s.find(':') {
            Some(colon) => (&s[..colon], &s[colon + 1..]),
            None => ("", &s[..]),
        }
    } else {
        ("", &s[..])
    };
    let Some(mut after_slashes) = rest.strip_prefix("//") else {
        return String::new();
    };
    // "special authority ignore slashes state" (WHATWG §4): for a special
    // scheme, any further leading `/`/`\` right after the scheme's own `//`
    // is skipped, not treated as an empty-authority delimiter — verified
    // against the `url` crate itself: `http:////path` and `http://\path`
    // both resolve to host "path", not an empty host.
    if ignores_extra_authority_slashes(&scheme.to_ascii_lowercase()) {
        after_slashes = after_slashes.trim_start_matches(['/', '\\']);
    }
    let auth_end = after_slashes.find(['/', '?', '#']).unwrap_or(after_slashes.len());
    let authority = &after_slashes[..auth_end];
    // Userinfo (`user:pass@`) — не наше дело хранить, отбрасываем как и раньше.
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    if let Some(after_bracket) = host_port.strip_prefix('[') {
        // IPv6 literal — host несёт скобки целиком, порт (если есть) идёт после `]`.
        match after_bracket.find(']') {
            Some(end) => host_port[..=end + 1].to_owned(),
            None => host_port.to_owned(),
        }
    } else {
        match host_port.rfind(':') {
            Some(i) => host_port[..i].to_owned(),
            None => host_port.to_owned(),
        }
    }
}

/// `scheme://host(raw)[:port]path[?query][#fragment]` — `inner`'s own
/// fields except the host, which is `raw_host` (see module doc for why).
fn serialize(inner: &WhatwgUrl, raw_host: &str) -> String {
    // `inner.has_host()` is false for an EMPTY authority (`foo://`,
    // `file:///tmp/x`) — but `inner.as_str()` still shows `//` for those
    // (verified against the crate itself), so read the "show slashes"
    // decision straight off its own serialization instead of re-deriving it.
    let after_scheme = &inner.as_str()[inner.scheme().len() + 1..];
    let show_authority_slashes = after_scheme.starts_with("//");

    let mut out = String::with_capacity(inner.as_str().len());
    out.push_str(inner.scheme());
    out.push(':');
    if show_authority_slashes {
        out.push_str("//");
        out.push_str(raw_host);
        if let Some(p) = inner.port() {
            out.push(':');
            out.push_str(&p.to_string());
        }
    }
    out.push_str(inner.path());
    if let Some(q) = inner.query() {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = inner.fragment() {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_fails() {
        assert!(Url::parse("").is_err());
    }

    #[test]
    fn parse_no_scheme_fails() {
        assert!(Url::parse("example.com").is_err());
    }

    #[test]
    fn parse_https_basic() {
        let u = Url::parse("https://example.com/path").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host(), "example.com");
        assert_eq!(u.port(), None);
        assert_eq!(u.path(), "/path");
        assert_eq!(u.query(), None);
        assert_eq!(u.fragment(), None);
        assert_eq!(u.effective_port(), Some(443));
    }

    #[test]
    fn parse_http_default_port_path_normalized() {
        let u = Url::parse("http://example.com").unwrap();
        assert_eq!(u.scheme(), "http");
        assert_eq!(u.path(), "/");
        assert_eq!(u.effective_port(), Some(80));
        assert_eq!(u.as_str(), "http://example.com/");
    }

    #[test]
    fn parse_explicit_port() {
        let u = Url::parse("http://localhost:8080/index.html").unwrap();
        assert_eq!(u.port(), Some(8080));
        assert_eq!(u.effective_port(), Some(8080));
        assert_eq!(u.path(), "/index.html");
    }

    #[test]
    fn parse_query_and_fragment() {
        let u = Url::parse("https://x.test/a/b?foo=1&bar=2#sec").unwrap();
        assert_eq!(u.path(), "/a/b");
        assert_eq!(u.query(), Some("foo=1&bar=2"));
        assert_eq!(u.fragment(), Some("sec"));
        assert_eq!(u.path_and_query(), "/a/b?foo=1&bar=2");
    }

    #[test]
    fn parse_fragment_only() {
        let u = Url::parse("https://x.test/#frag").unwrap();
        assert_eq!(u.path(), "/");
        assert_eq!(u.fragment(), Some("frag"));
    }

    #[test]
    fn parse_query_no_fragment() {
        let u = Url::parse("https://x.test/?q=1").unwrap();
        assert_eq!(u.query(), Some("q=1"));
        assert_eq!(u.fragment(), None);
    }

    #[test]
    fn parse_scheme_case_insensitive() {
        let u = Url::parse("HTTPS://Example.com/").unwrap();
        assert_eq!(u.scheme(), "https");
        // host case оставляем как есть (DNS case-insensitive, но семантически
        // не наше дело нормализовать — некоторые WAF чувствительны).
        assert_eq!(u.host(), "Example.com");
    }

    #[test]
    fn parse_cyrillic_idn_unicode_preserved() {
        let u = Url::parse("https://президент.рф/").unwrap();
        assert_eq!(u.host(), "президент.рф");
        assert_eq!(u.as_str(), "https://президент.рф/");
    }

    #[test]
    fn host_ascii_punycode() {
        let u = Url::parse("https://президент.рф/path").unwrap();
        assert_eq!(u.host_ascii().unwrap(), "xn--d1abbgf6aiiy.xn--p1ai");
    }

    #[test]
    fn host_ascii_empty_for_data_url() {
        let u = Url::parse("data:text/plain,hello").unwrap();
        assert_eq!(u.scheme(), "data");
        assert_eq!(u.host(), "");
        assert_eq!(u.host_ascii().unwrap(), "");
    }

    #[test]
    fn file_url_no_authority_after_double_slash() {
        let u = Url::parse("file:///tmp/a.html").unwrap();
        assert_eq!(u.scheme(), "file");
        assert_eq!(u.host(), "");
        assert_eq!(u.path(), "/tmp/a.html");
    }

    #[test]
    fn userinfo_dropped() {
        let u = Url::parse("http://user:pass@example.com/").unwrap();
        assert_eq!(u.host(), "example.com");
    }

    #[test]
    fn invalid_port_fails() {
        assert!(Url::parse("http://example.com:notaport/").is_err());
    }

    #[test]
    fn empty_host_fails_for_http() {
        // A genuinely empty host (after userinfo, not right after the
        // scheme's own `//` — see `triple_slash_special_scheme_ignores_
        // extra_slash` below for why `http:///path` does NOT hit this path).
        assert!(Url::parse("http://user:pass@/path").is_err());
    }

    #[test]
    fn as_str_roundtrip_with_query_fragment() {
        let u = Url::parse("https://x.test:8443/a?q=1#f").unwrap();
        assert_eq!(u.as_str(), "https://x.test:8443/a?q=1#f");
    }

    #[test]
    fn resolve_absolute() {
        let base = Url::parse("https://example.com/page").unwrap();
        let r = base.resolve("https://other.com/foo").unwrap();
        assert_eq!(r.as_str(), "https://other.com/foo");
    }

    #[test]
    fn resolve_protocol_relative() {
        let base = Url::parse("https://example.com/page").unwrap();
        let r = base.resolve("//cdn.test/lib.js").unwrap();
        assert_eq!(r.as_str(), "https://cdn.test/lib.js");
    }

    #[test]
    fn resolve_absolute_path() {
        let base = Url::parse("https://example.com/dir/page").unwrap();
        let r = base.resolve("/style.css").unwrap();
        assert_eq!(r.as_str(), "https://example.com/style.css");
    }

    #[test]
    fn resolve_relative_path() {
        let base = Url::parse("https://example.com/dir/page.html").unwrap();
        let r = base.resolve("css/style.css").unwrap();
        assert_eq!(r.as_str(), "https://example.com/dir/css/style.css");
    }

    #[test]
    fn resolve_relative_root_path() {
        let base = Url::parse("https://example.com/").unwrap();
        let r = base.resolve("about.html").unwrap();
        assert_eq!(r.as_str(), "https://example.com/about.html");
    }

    #[test]
    fn resolve_fragment_only() {
        let base = Url::parse("https://example.com/page?q=1").unwrap();
        let r = base.resolve("#sec").unwrap();
        assert_eq!(r.as_str(), "https://example.com/page?q=1#sec");
    }

    #[test]
    fn resolve_query_only() {
        let base = Url::parse("https://example.com/page?old=1#f").unwrap();
        let r = base.resolve("?new=2").unwrap();
        assert_eq!(r.as_str(), "https://example.com/page?new=2");
    }

    #[test]
    fn resolve_dot_segment_one_level_up() {
        let base =
            Url::parse("http://127.0.0.1:8300/custom-elements/reactions/HTMLTableElement.html")
                .unwrap();
        let r = base
            .resolve("../resources/custom-elements-helpers.js")
            .unwrap();
        assert_eq!(
            r.as_str(),
            "http://127.0.0.1:8300/custom-elements/resources/custom-elements-helpers.js"
        );
    }

    #[test]
    fn resolve_dot_segment_two_levels_up() {
        let base = Url::parse(
            "http://127.0.0.1:8300/custom-elements/reactions/customized-builtins/x.html",
        )
        .unwrap();
        let r = base
            .resolve("../../resources/custom-elements-helpers.js")
            .unwrap();
        assert_eq!(
            r.as_str(),
            "http://127.0.0.1:8300/custom-elements/resources/custom-elements-helpers.js"
        );
    }

    #[test]
    fn parse_collapses_dot_segments_directly() {
        let u = Url::parse("https://example.com/a/../b").unwrap();
        assert_eq!(u.path(), "/b");
    }

    #[test]
    fn parse_collapses_single_dot_segment() {
        let u = Url::parse("https://example.com/a/./b").unwrap();
        assert_eq!(u.path(), "/a/b");
    }

    #[test]
    fn parse_trailing_dot_dot_collapses_to_parent() {
        let u = Url::parse("https://example.com/a/b/..").unwrap();
        assert_eq!(u.path(), "/a/");
    }

    #[test]
    fn parse_dot_dot_above_root_is_dropped() {
        let u = Url::parse("https://example.com/../a").unwrap();
        assert_eq!(u.path(), "/a");
    }

    #[test]
    fn resolve_preserves_port() {
        let base = Url::parse("http://localhost:8080/dir/page").unwrap();
        let r = base.resolve("/abs").unwrap();
        assert_eq!(r.as_str(), "http://localhost:8080/abs");
        let r2 = base.resolve("rel.html").unwrap();
        assert_eq!(r2.as_str(), "http://localhost:8080/dir/rel.html");
    }

    // ── LIB-6: regressions the WHATWG-conformant `inner` now closes ────────

    #[test]
    fn ipv6_literal_host_with_port() {
        let u = Url::parse("http://[::1]:8080/x").unwrap();
        assert_eq!(u.host(), "[::1]");
        assert_eq!(u.port(), Some(8080));
        assert_eq!(u.path(), "/x");
    }

    #[test]
    fn ipv6_literal_without_brackets_now_rejected() {
        // Own hand-rolled parser used to accept this ("too-permissive",
        // the largest LIB-0 failure class: 199/512) — the colons were parsed
        // as a bogus `host:port:garbage` split instead of a rejected literal.
        assert!(Url::parse("http://2001::1").is_err());
        assert!(Url::parse("http://[1::2]:3:4").is_err());
    }

    #[test]
    fn forbidden_host_code_points_stripped_not_stored() {
        // WHATWG preprocessing removes ASCII tab/newline from the whole
        // input before parsing — the raw host must reflect that too, not
        // store the literal control characters (LIB-0: field-mismatch host,
        // 101/512).
        let u = Url::parse("http://exa\tmple.\norg/").unwrap();
        assert_eq!(u.host(), "example.org");
    }

    #[test]
    fn empty_host_allowed_for_non_special_scheme() {
        // LIB-0 "unexpected-failure: empty host rejected" (40/512): unlike
        // http(s), a non-special scheme's authority may have an empty host.
        assert!(Url::parse("foo://").is_ok());
    }

    #[test]
    fn triple_slash_special_scheme_ignores_extra_slash() {
        // WHATWG "special authority ignore slashes state": the third `/` is
        // skipped, not read as an empty-authority delimiter — verified
        // against the `url` crate itself (`http:///path` -> host "path",
        // path "/", NOT an empty-host error). The pre-LIB-6 test asserting
        // this rejects was based on a misreading of the spec, not a real
        // requirement; the leniency is real and every major browser applies
        // it identically.
        let u = Url::parse("http:///path").unwrap();
        assert_eq!(u.host(), "path");
        assert_eq!(u.path(), "/");
        assert_eq!(u.as_str(), "http://path/");
    }

    #[test]
    fn empty_authority_keeps_slashes_in_serialization() {
        // `has_host()` is false for an empty non-special authority, but the
        // WHATWG serializer still shows `//` — `as_str()` must match `inner`.
        let u = Url::parse("foo://").unwrap();
        assert_eq!(u.host(), "");
        assert_eq!(u.as_str(), "foo://");
    }
}
