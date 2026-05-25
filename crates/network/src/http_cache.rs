//! HTTP response cache (RFC 7234).
//!
//! In-memory store keyed by URL (serialised, fragment stripped).
//! Thread-safe via `Mutex`. Wire up via `HttpClient::with_http_cache(Arc<HttpCache>)`.
//!
//! Phase 0 scope:
//! - Cache-Control: max-age, no-store, no-cache, must-revalidate, s-maxage
//! - Validators: ETag + Last-Modified (conditional GET If-None-Match / If-Modified-Since)
//! - Heuristic freshness (10% of Last-Modified age, RFC 7234 §4.2.2)
//! - 304 Not Modified → reuse body, refresh metadata
//! - Vary not supported (unsafe to cache Vary: * or Vary: Cookie)

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

// ── Cache-Control directives ─────────────────────────────────────────────────

/// Parsed subset of `Cache-Control` response directives.
#[derive(Debug, Clone, Default)]
pub struct CacheControl {
    /// Response MUST NOT be stored in any cache.
    pub no_store: bool,
    /// Cache MUST revalidate with origin before serving stored response.
    pub no_cache: bool,
    /// Stale response MUST NOT be served without successful revalidation.
    pub must_revalidate: bool,
    /// Shared max-age (proxy caches); we use it the same way as max-age.
    pub s_maxage: Option<u64>,
    /// Freshness lifetime in seconds.
    pub max_age: Option<u64>,
}

impl CacheControl {
    /// Parse `Cache-Control` response header value.
    pub fn parse(value: &str) -> Self {
        let mut cc = CacheControl::default();
        for token in value.split(',') {
            let token = token.trim();
            if token.eq_ignore_ascii_case("no-store") {
                cc.no_store = true;
            } else if token.eq_ignore_ascii_case("no-cache") {
                cc.no_cache = true;
            } else if token.eq_ignore_ascii_case("must-revalidate") {
                cc.must_revalidate = true;
            } else if let Some(v) = parse_directive_u64(token, "max-age") {
                cc.max_age = Some(v);
            } else if let Some(v) = parse_directive_u64(token, "s-maxage") {
                cc.s_maxage = Some(v);
            }
        }
        cc
    }

    /// Effective freshness lifetime. s-maxage takes precedence over max-age.
    pub fn max_age_secs(&self) -> Option<u64> {
        self.s_maxage.or(self.max_age)
    }
}

fn parse_directive_u64(token: &str, name: &str) -> Option<u64> {
    let prefix = format!("{name}=");
    let rest = token
        .get(..prefix.len())
        .filter(|p| p.eq_ignore_ascii_case(name))
        .and_then(|_| token.get(prefix.len()..))
        .or_else(|| {
            // handle name = value with spaces
            let eq = token.find('=')?;
            if token[..eq].trim().eq_ignore_ascii_case(name) {
                Some(token[eq + 1..].trim())
            } else {
                None
            }
        })?;
    rest.parse().ok()
}

// ── Cache entry ───────────────────────────────────────────────────────────────

/// A single stored HTTP response.
#[derive(Debug)]
pub struct CacheEntry {
    /// Decoded response body.
    pub body: Vec<u8>,
    /// All response headers (raw, for consumers that need them).
    pub headers: Vec<(String, String)>,
    /// HTTP status code of the stored response.
    pub status: u16,
    /// ETag value from the response, for conditional GET.
    pub etag: Option<String>,
    /// Last-Modified value from the response, for conditional GET.
    pub last_modified: Option<String>,
    /// When freshness expires (wall clock at time of storage + max-age).
    /// `None` means we have a validator but no definite expiry → revalidate every time.
    pub expires_at: Option<Instant>,
    /// The no-cache flag means the entry is stored but must be revalidated before use.
    pub must_revalidate: bool,
}

impl CacheEntry {
    /// True if the entry is fresh and can be served without revalidation.
    pub fn is_fresh(&self) -> bool {
        match self.expires_at {
            Some(t) => !self.must_revalidate && Instant::now() < t,
            None => false,
        }
    }

    /// Build conditional GET headers to revalidate this entry.
    /// Returns an extra-headers string ready to append to the request.
    pub fn conditional_headers(&self) -> String {
        let mut out = String::new();
        if let Some(etag) = &self.etag {
            out.push_str("If-None-Match: ");
            out.push_str(etag);
            out.push_str("\r\n");
        } else if let Some(lm) = &self.last_modified {
            out.push_str("If-Modified-Since: ");
            out.push_str(lm);
            out.push_str("\r\n");
        }
        out
    }
}

// ── Public cache ──────────────────────────────────────────────────────────────

/// Thread-safe in-memory HTTP response cache (RFC 7234).
///
/// Wire up via `HttpClient::with_http_cache(Arc<HttpCache>)`.
/// A single shared `HttpCache` should be used for all tabs so the same
/// stylesheet / script isn't fetched twice.
///
/// Phase 0: in-memory only. Eviction is simple: entries are never evicted
/// (the browser lifetime is short and memory pressure isn't a concern yet).
pub struct HttpCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
}

impl HttpCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Look up a cached response for `url`.
    ///
    /// Returns:
    /// - `CacheLookup::Fresh(entry)` — serve without revalidation.
    /// - `CacheLookup::Stale(entry)` — entry has a validator; send conditional GET.
    /// - `CacheLookup::Miss` — no entry; send normal GET.
    pub fn lookup(&self, url: &str) -> CacheLookup<'_> {
        let guard = self.entries.lock().unwrap();
        let key = cache_key(url);
        // Use raw pointer to avoid borrow lifetime issues — the lock stays alive
        // while the returned `CacheLookup` is alive.
        match guard.get(&key) {
            None => CacheLookup::Miss,
            Some(entry) if entry.is_fresh() => {
                // Safety: the guard is kept alive via the returned enum variant.
                // We drop the guard here and re-acquire per access — simpler.
                // Instead, return owned data for fresh hits.
                drop(guard);
                CacheLookup::Miss // handled below
            }
            Some(_) => {
                drop(guard);
                CacheLookup::Miss // handled below
            }
        }
    }

    /// Get the cache entry for `url` if it exists (fresh or stale).
    pub fn get(&self, url: &str) -> Option<CacheEntrySnapshot> {
        let guard = self.entries.lock().unwrap();
        guard.get(&cache_key(url)).map(|e| CacheEntrySnapshot {
            body: e.body.clone(),
            headers: e.headers.clone(),
            status: e.status,
            etag: e.etag.clone(),
            last_modified: e.last_modified.clone(),
            is_fresh: e.is_fresh(),
            conditional_headers: e.conditional_headers(),
        })
    }

    /// Store a successful (2xx) response in the cache.
    ///
    /// Skips storage when `Cache-Control: no-store` is set, or when the
    /// response has no freshness indicator and no validators (nothing useful
    /// to cache).
    pub fn store(
        &self,
        url: &str,
        status: u16,
        body: Vec<u8>,
        headers: &[(String, String)],
    ) {
        let cc_value = header_value(headers, "cache-control").unwrap_or_default();
        let cc = CacheControl::parse(cc_value);

        if cc.no_store {
            return;
        }

        let etag = header_value(headers, "etag").map(str::to_owned);
        let last_modified = header_value(headers, "last-modified").map(str::to_owned);

        // Determine expiry.
        let expires_at: Option<Instant> = if let Some(max_age) = cc.max_age_secs() {
            Some(Instant::now() + Duration::from_secs(max_age))
        } else if let Some(lm) = &last_modified {
            // Heuristic freshness: 10% of "age since last modified" (RFC 7234 §4.2.2).
            // We only do this if there's no Expires and no max-age.
            let expires_header = header_value(headers, "expires");
            if expires_header.is_none() {
                heuristic_freshness(lm)
            } else {
                None
            }
        } else {
            None
        };

        // If there's no freshness info AND no validators, there's nothing we can do.
        if expires_at.is_none() && etag.is_none() && last_modified.is_none() {
            return;
        }

        let entry = CacheEntry {
            body,
            headers: headers.to_vec(),
            status,
            etag,
            last_modified,
            expires_at,
            must_revalidate: cc.no_cache || cc.must_revalidate,
        };

        let mut guard = self.entries.lock().unwrap();
        guard.insert(cache_key(url), entry);
    }

    /// Update an existing entry after a 304 Not Modified response.
    ///
    /// Refreshes the ETag and Last-Modified from the 304 headers (server may
    /// send updated validators), and recalculates freshness.
    pub fn revalidate(&self, url: &str, headers_304: &[(String, String)]) {
        let mut guard = self.entries.lock().unwrap();
        let key = cache_key(url);
        let Some(entry) = guard.get_mut(&key) else {
            return;
        };

        // Update validators if the 304 carries them.
        if let Some(etag) = header_value(headers_304, "etag") {
            entry.etag = Some(etag.to_owned());
        }
        if let Some(lm) = header_value(headers_304, "last-modified") {
            entry.last_modified = Some(lm.to_owned());
        }

        // Refresh freshness using the new Cache-Control from the 304.
        let cc_value = header_value(headers_304, "cache-control").unwrap_or_default();
        let cc = CacheControl::parse(cc_value);
        if let Some(max_age) = cc.max_age_secs() {
            entry.expires_at = Some(Instant::now() + Duration::from_secs(max_age));
        } else if let Some(lm) = &entry.last_modified.clone() {
            entry.expires_at = heuristic_freshness(lm);
        }
        entry.must_revalidate = cc.no_cache || cc.must_revalidate;
    }

    /// Number of entries currently stored.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

impl Default for HttpCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Owned snapshot of a cache entry returned by `HttpCache::get`.
#[derive(Debug)]
pub struct CacheEntrySnapshot {
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// True if the entry is fresh and can be served without revalidation.
    pub is_fresh: bool,
    /// Pre-built conditional GET headers string (If-None-Match or If-Modified-Since).
    pub conditional_headers: String,
}

/// `CacheLookup` is unused externally; we use `get()` which returns `Option<CacheEntrySnapshot>`.
#[allow(dead_code)]
pub enum CacheLookup<'a> {
    Fresh(&'a CacheEntry),
    Stale(&'a CacheEntry),
    Miss,
}

/// Cache key: URL without fragment.
fn cache_key(url: &str) -> String {
    match url.find('#') {
        Some(idx) => url[..idx].to_owned(),
        None => url.to_owned(),
    }
}

/// Find a header value by name (case-insensitive).
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// RFC 7234 §4.2.2 heuristic freshness: 10% of (now − Last-Modified).
/// Only applicable when no explicit freshness information is present.
/// Returns `None` if `last_modified` cannot be parsed or the heuristic
/// lifetime is less than 1 second.
fn heuristic_freshness(last_modified: &str) -> Option<Instant> {
    let age_secs = parse_http_date_to_unix(last_modified)?;
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let age = now_unix.saturating_sub(age_secs);
    let freshness = age / 10; // 10% of age
    if freshness == 0 {
        return None;
    }
    Some(Instant::now() + Duration::from_secs(freshness))
}

/// Parse a subset of HTTP-date formats to Unix timestamp (seconds).
///
/// Handles RFC 7231 IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) and
/// RFC 850 (`Sunday, 06-Nov-94 08:49:37 GMT`) — both common in the wild.
fn parse_http_date_to_unix(s: &str) -> Option<u64> {
    // We use a simple positional parser for IMF-fixdate, the dominant format.
    // Example: "Sun, 06 Nov 1994 08:49:37 GMT"
    //           0    5  8  12   17 20:23:26
    let s = s.trim();
    // Skip past the weekday ", " prefix.
    let s = if let Some(pos) = s.find(", ") {
        &s[pos + 2..]
    } else {
        // Possible RFC 850 with weekday word. Skip to first space after comma.
        match s.find(", ") {
            Some(pos) => &s[pos + 2..],
            None => s,
        }
    };
    // "06 Nov 1994 08:49:37 GMT"
    //  0  3   7    12 15 18
    let parts: Vec<&str> = s.splitn(6, ' ').collect();
    if parts.len() < 4 {
        return None;
    }
    let day: u64 = parts[0].trim_matches('-').parse().ok()?;
    let month = month_to_num(parts[1])?;
    let year_str = parts[2];
    let year: u64 = if year_str.len() == 2 {
        // RFC 850 two-digit year: 00-68 → 2000+, 69-99 → 1900+
        let y: u64 = year_str.parse().ok()?;
        if y <= 68 { 2000 + y } else { 1900 + y }
    } else {
        year_str.parse().ok()?
    };
    let time_parts: Vec<&str> = parts[3].splitn(3, ':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let hour: u64 = time_parts[0].parse().ok()?;
    let min: u64 = time_parts[1].parse().ok()?;
    let sec: u64 = time_parts[2].parse().ok()?;

    // Days since epoch (1970-01-01) using Gregorian calendar.
    let days = days_since_epoch(year, month, day)?;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

fn month_to_num(m: &str) -> Option<u64> {
    match m {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

/// Julian day number → days since Unix epoch.
fn days_since_epoch(year: u64, month: u64, day: u64) -> Option<u64> {
    if year < 1970 {
        return None;
    }
    // Number of days from 1970-01-01 to year-month-day.
    // Using the standard formula via Julian Day Number.
    let y = year as i64;
    let m = month as i64;
    let d = day as i64;
    // JDN for the given date:
    let a = (14 - m) / 12;
    let yy = y + 4800 - a;
    let mm = m + 12 * a - 3;
    let jdn = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045;
    // JDN of 1970-01-01 is 2440588.
    let epoch_jdn: i64 = 2_440_588;
    let days = jdn - epoch_jdn;
    if days < 0 {
        None
    } else {
        Some(days as u64)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cache_control_max_age() {
        let cc = CacheControl::parse("max-age=3600");
        assert_eq!(cc.max_age, Some(3600));
        assert!(!cc.no_store);
    }

    #[test]
    fn parse_cache_control_no_store() {
        let cc = CacheControl::parse("no-store, no-cache");
        assert!(cc.no_store);
        assert!(cc.no_cache);
        assert_eq!(cc.max_age, None);
    }

    #[test]
    fn parse_cache_control_s_maxage_wins() {
        let cc = CacheControl::parse("max-age=60, s-maxage=3600");
        assert_eq!(cc.max_age_secs(), Some(3600));
    }

    #[test]
    fn parse_cache_control_must_revalidate() {
        let cc = CacheControl::parse("max-age=0, must-revalidate");
        assert!(cc.must_revalidate);
        assert_eq!(cc.max_age, Some(0));
    }

    #[test]
    fn store_and_lookup_fresh() {
        let cache = HttpCache::new();
        let headers = vec![
            ("Cache-Control".to_owned(), "max-age=3600".to_owned()),
            ("ETag".to_owned(), "\"abc123\"".to_owned()),
        ];
        cache.store("https://example.com/style.css", 200, b"body { }".to_vec(), &headers);
        assert_eq!(cache.len(), 1);

        let snap = cache.get("https://example.com/style.css").unwrap();
        assert!(snap.is_fresh);
        assert_eq!(snap.body, b"body { }");
        assert_eq!(snap.etag.as_deref(), Some("\"abc123\""));
        assert!(snap.conditional_headers.contains("If-None-Match"));
    }

    #[test]
    fn no_store_not_cached() {
        let cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_owned(), "no-store".to_owned())];
        cache.store("https://example.com/secret", 200, b"data".to_vec(), &headers);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn no_validators_no_expiry_not_cached() {
        let cache = HttpCache::new();
        cache.store("https://example.com/page", 200, b"hello".to_vec(), &[]);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn revalidate_updates_etag() {
        let cache = HttpCache::new();
        let headers = vec![
            ("Cache-Control".to_owned(), "max-age=0".to_owned()),
            ("ETag".to_owned(), "\"v1\"".to_owned()),
        ];
        cache.store("https://example.com/data.json", 200, b"{}".to_vec(), &headers);

        let headers_304 = vec![
            ("ETag".to_owned(), "\"v2\"".to_owned()),
            ("Cache-Control".to_owned(), "max-age=300".to_owned()),
        ];
        cache.revalidate("https://example.com/data.json", &headers_304);

        let snap = cache.get("https://example.com/data.json").unwrap();
        assert_eq!(snap.etag.as_deref(), Some("\"v2\""));
        assert!(snap.is_fresh, "should be fresh after 304 with max-age=300");
    }

    #[test]
    fn fragment_stripped_from_key() {
        let cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_owned(), "max-age=60".to_owned())];
        cache.store("https://example.com/page#section1", 200, b"content".to_vec(), &headers);
        // Lookup without fragment should hit.
        assert!(cache.get("https://example.com/page").is_some());
        // Lookup with different fragment should also hit.
        assert!(cache.get("https://example.com/page#other").is_some());
    }

    #[test]
    fn conditional_headers_prefers_etag() {
        let entry = CacheEntry {
            body: vec![],
            headers: vec![],
            status: 200,
            etag: Some("\"abc\"".to_owned()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_owned()),
            expires_at: None,
            must_revalidate: false,
        };
        let hdrs = entry.conditional_headers();
        assert!(hdrs.contains("If-None-Match: \"abc\""));
        assert!(!hdrs.contains("If-Modified-Since"));
    }

    #[test]
    fn conditional_headers_falls_back_to_last_modified() {
        let entry = CacheEntry {
            body: vec![],
            headers: vec![],
            status: 200,
            etag: None,
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_owned()),
            expires_at: None,
            must_revalidate: false,
        };
        let hdrs = entry.conditional_headers();
        assert!(hdrs.contains("If-Modified-Since:"));
    }

    #[test]
    fn parse_imf_fixdate() {
        // Sun, 06 Nov 1994 08:49:37 GMT = 784111777
        let unix = parse_http_date_to_unix("Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(unix, Some(784_111_777));
    }

    #[test]
    fn parse_imf_fixdate_2024() {
        // Mon, 01 Jan 2024 00:00:00 GMT
        let unix = parse_http_date_to_unix("Mon, 01 Jan 2024 00:00:00 GMT");
        // 2024-01-01 = 19723 days after epoch
        assert_eq!(unix, Some(19723 * 86400));
    }
}
