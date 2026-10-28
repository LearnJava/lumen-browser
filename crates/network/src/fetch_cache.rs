//! HTTP cache policy of a page `fetch()` (FETCH-HTTP-CACHE).
//!
//! `HttpClient::fetch_request` runs a page request through the same shared
//! [`HttpCacheBackend`] the navigation and subresource paths use, driven by
//! the request's `cache` mode (Fetch §4.5 "HTTP-network-or-cache fetch") and
//! its own `Cache-Control` request directives (RFC 9111 §5.2.1). This module
//! holds the pure decisions — mode parsing, the request lines a mode adds,
//! "serve / revalidate / go to the network", `Vary` matching and invalidation
//! — so they are unit-testable without a socket; `js_providers.rs` wires them
//! to the network.

use crate::http::headers::DROP_DEFAULT_HEADER;
use crate::http_cache::{CacheEntrySnapshot, HttpCacheBackend};
use lumen_core::url::Url;

/// Largest response body the `fetch()` path stores. Bigger ones (media, bulk
/// downloads through `fetch()`) would only churn the 50 MB LRU.
pub(crate) const MAX_STORED_BODY: usize = 8 * 1024 * 1024;

/// Stored-response header carrying the request-header values the response
/// varied on (`Vary`), see [`vary_marker`]. Never shown to the page.
pub(crate) const VARY_MARKER_HEADER: &str = "x-lumen-vary-req";

/// `Request.cache` (Fetch §5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheMode {
    /// Fresh entry is served, stale one is revalidated, otherwise network.
    Default,
    /// Network only; the response is not stored.
    NoStore,
    /// Network only; the response is stored.
    Reload,
    /// Always revalidate a stored entry (when it has a validator).
    NoCache,
    /// Any stored entry, fresh or stale, is served.
    ForceCache,
    /// Like `ForceCache`, but a miss is a network error.
    OnlyIfCached,
}

impl CacheMode {
    /// Parse the shim's string; an empty or unknown one is `default`.
    pub(crate) fn parse(s: &str) -> Self {
        match s {
            "no-store" => Self::NoStore,
            "reload" => Self::Reload,
            "no-cache" => Self::NoCache,
            "force-cache" => Self::ForceCache,
            "only-if-cached" => Self::OnlyIfCached,
            _ => Self::Default,
        }
    }

    /// Fetch §4.5 step "If httpRequest's cache mode is default and
    /// httpRequest's header list contains `If-Modified-Since`,
    /// `If-None-Match`, `If-Unmodified-Since`, `If-Match` or `If-Range`, then
    /// set httpRequest's cache mode to no-store."
    pub(crate) fn for_request(self, author_headers: &[(String, String)]) -> Self {
        let conditional = ["if-modified-since", "if-none-match", "if-unmodified-since", "if-match", "if-range"];
        if self == Self::Default
            && author_headers
                .iter()
                .any(|(k, _)| conditional.iter().any(|c| k.eq_ignore_ascii_case(c)))
        {
            Self::NoStore
        } else {
            self
        }
    }

    /// Whether a network response is written to the cache.
    pub(crate) fn stores_response(self) -> bool {
        self != Self::NoStore
    }

    /// Request lines the mode adds when the author set no such header
    /// (Fetch §4.5): `no-cache` → `Cache-Control: max-age=0`;
    /// `no-store`/`reload` → `Pragma: no-cache` + `Cache-Control: no-cache`.
    /// Every other mode drops the profile's navigation `Cache-Control:
    /// max-age=0`, which a subresource request never carries.
    pub(crate) fn request_lines(self, author_headers: &[(String, String)]) -> String {
        let has = |name: &str| author_headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
        let mut out = String::new();
        match self {
            Self::NoCache => {
                if !has("cache-control") {
                    out.push_str("Cache-Control: max-age=0\r\n");
                }
            }
            Self::NoStore | Self::Reload => {
                if !has("pragma") {
                    out.push_str("Pragma: no-cache\r\n");
                }
                if !has("cache-control") {
                    out.push_str("Cache-Control: no-cache\r\n");
                }
            }
            Self::Default | Self::ForceCache | Self::OnlyIfCached => {
                if !has("cache-control") {
                    out.push_str(DROP_DEFAULT_HEADER);
                    out.push_str(": cache-control\r\n");
                }
            }
        }
        out
    }
}

/// `Cache-Control` request directives that steer cache use (RFC 9111 §5.2.1).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RequestCc {
    pub(crate) no_cache: bool,
    pub(crate) no_store: bool,
    pub(crate) only_if_cached: bool,
    pub(crate) max_age: Option<u64>,
    /// `Some(None)` — `max-stale` without a value (any staleness is fine).
    pub(crate) max_stale: Option<Option<u64>>,
    pub(crate) min_fresh: Option<u64>,
}

impl RequestCc {
    /// Collect the directives of every `Cache-Control` author header.
    pub(crate) fn from_headers(author_headers: &[(String, String)]) -> Self {
        let mut cc = Self::default();
        for (_, value) in author_headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("cache-control")) {
            for token in value.split(',') {
                let token = token.trim();
                let (name, arg) = match token.split_once('=') {
                    Some((n, a)) => (n.trim(), Some(a.trim().trim_matches('"'))),
                    None => (token, None),
                };
                let secs = arg.and_then(|a| a.parse::<u64>().ok());
                match name.to_ascii_lowercase().as_str() {
                    "no-cache" => cc.no_cache = true,
                    "no-store" => cc.no_store = true,
                    "only-if-cached" => cc.only_if_cached = true,
                    "max-age" => cc.max_age = secs,
                    "max-stale" => cc.max_stale = Some(secs),
                    "min-fresh" => cc.min_fresh = secs,
                    _ => {}
                }
            }
        }
        cc
    }
}

/// What the `fetch()` path does with a request.
#[derive(Debug)]
pub(crate) enum Plan {
    /// Answer from the stored entry.
    Serve(CacheEntrySnapshot),
    /// Ask the origin with the entry's validators; a 304 means "serve it".
    Revalidate(CacheEntrySnapshot),
    /// Plain network request.
    Network,
    /// `Cache-Control: only-if-cached` request nothing usable satisfies:
    /// a synthetic 504 (RFC 9111 §5.2.1.7).
    GatewayTimeout,
    /// `cache: only-if-cached` miss: Fetch's network error.
    NetworkError,
}

/// Decide how to answer a GET from the cache `entry` (already `Vary`-matched).
pub(crate) fn plan(mode: CacheMode, rcc: &RequestCc, entry: Option<CacheEntrySnapshot>) -> Plan {
    let revalidate_or_network = |e: CacheEntrySnapshot| {
        if e.conditional_headers.is_empty() { Plan::Network } else { Plan::Revalidate(e) }
    };
    match mode {
        CacheMode::NoStore | CacheMode::Reload => Plan::Network,
        CacheMode::NoCache => entry.map_or(Plan::Network, revalidate_or_network),
        CacheMode::ForceCache => entry.map_or(Plan::Network, Plan::Serve),
        CacheMode::OnlyIfCached => entry.map_or(Plan::NetworkError, Plan::Serve),
        CacheMode::Default => {
            let Some(e) = entry else {
                return if rcc.only_if_cached { Plan::GatewayTimeout } else { Plan::Network };
            };
            if rcc.no_store {
                Plan::Network
            } else if rcc.no_cache {
                revalidate_or_network(e)
            } else if usable(&e, rcc) {
                Plan::Serve(e)
            } else if rcc.only_if_cached {
                Plan::GatewayTimeout
            } else {
                revalidate_or_network(e)
            }
        }
    }
}

/// May `e` answer a request with these directives without asking the origin
/// (RFC 9111 §4: fresh, or stale within `max-stale`; `max-age` and `min-fresh`
/// tighten, `no-cache`/`must-revalidate` responses never go stale-served).
fn usable(e: &CacheEntrySnapshot, rcc: &RequestCc) -> bool {
    if e.no_cache || rcc.max_age.is_some_and(|m| e.age_secs > m) {
        return false;
    }
    let Some(lifetime) = e.lifetime_secs else { return false };
    if e.is_fresh {
        let remaining = lifetime.saturating_sub(e.age_secs);
        return rcc.min_fresh.is_none_or(|f| remaining >= f);
    }
    !e.must_revalidate
        && rcc
            .max_stale
            .is_some_and(|allowed| allowed.is_none_or(|n| e.age_secs.saturating_sub(lifetime) <= n))
}

/// Outcome of comparing the request against what a response varied on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VaryMarker {
    /// `Vary: *` or `Vary: Cookie` — the response cannot be reused safely.
    Unstorable,
    /// Nothing request-specific to remember.
    None,
    /// The request-header values the response varied on.
    Values(String),
}

/// Request headers the profile fills identically for every request; varying
/// on them never splits the cache.
const FIXED_BY_PROFILE: &[&str] = &["accept", "accept-encoding", "accept-language", "user-agent"];

/// The values of the headers `vary` names, in the request `author_headers`.
fn vary_values(vary: &str, author_headers: &[(String, String)]) -> VaryMarker {
    let mut names: Vec<String> = vary
        .split(',')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    if names.iter().any(|n| n == "*" || n == "cookie") {
        return VaryMarker::Unstorable;
    }
    names.retain(|n| !FIXED_BY_PROFILE.contains(&n.as_str()));
    names.sort();
    names.dedup();
    if names.is_empty() {
        return VaryMarker::None;
    }
    let parts: Vec<String> = names
        .iter()
        .map(|name| {
            match author_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                Some((_, v)) => format!("{name}:={}", v.trim()),
                None => format!("{name}:!"),
            }
        })
        .collect();
    VaryMarker::Values(parts.join("\n"))
}

/// What to remember about the request when storing `response_headers`.
pub(crate) fn vary_marker(
    response_headers: &[(String, String)],
    author_headers: &[(String, String)],
) -> VaryMarker {
    let vary = response_headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("vary"))
        .map(|(_, v)| v.as_str())
        .collect::<Vec<_>>()
        .join(",");
    vary_values(&vary, author_headers)
}

/// `entry` if it was stored for a request that sent the same `Vary`-named
/// headers as `author_headers` (RFC 9111 §4.1), else `None`.
pub(crate) fn vary_match(
    entry: CacheEntrySnapshot,
    author_headers: &[(String, String)],
) -> Option<CacheEntrySnapshot> {
    let stored = entry
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(VARY_MARKER_HEADER))
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    let current = match vary_marker(&entry.headers, author_headers) {
        VaryMarker::Unstorable => return None,
        VaryMarker::None => String::new(),
        VaryMarker::Values(v) => v,
    };
    (stored == current).then_some(entry)
}

/// Drop the cached responses an unsafe request's success makes stale
/// (RFC 9111 §4.4): the request URL and, when same-origin, the URLs its
/// response names in `Location` and `Content-Location`.
pub(crate) fn invalidate(cache: &dyn HttpCacheBackend, url: &Url, response_headers: &[(String, String)]) {
    cache.remove(url.as_str());
    for name in ["location", "content-location"] {
        for (_, value) in response_headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)) {
            if let Ok(target) = url.resolve(value.trim())
                && target.origin() == url.origin()
            {
                cache.remove(target.as_str());
            }
        }
    }
}

/// Methods Fetch calls unsafe: anything but GET, HEAD, OPTIONS, TRACE.
pub(crate) fn is_unsafe_method(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE")
}

#[cfg(test)]
#[path = "fetch_cache_tests.rs"]
mod tests;
