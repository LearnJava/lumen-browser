//! HTTP response cache (RFC 7234).
//!
//! Two backends share the [`HttpCacheBackend`] trait:
//! - [`HttpCache`]     — in-memory LRU, 50 MB cap. Fast, cleared on restart.
//! - [`DiskHttpCache`] — SQLite-backed, survives restarts (C-2 Phase 1).
//!
//! Phase 0 scope:
//! - Cache-Control: max-age, no-store, no-cache, must-revalidate, s-maxage
//! - Validators: ETag + Last-Modified (conditional GET If-None-Match / If-Modified-Since)
//! - Heuristic freshness (10% of Last-Modified age, RFC 7234 §4.2.2)
//! - 304 Not Modified → reuse body, refresh metadata
//! - Vary not supported (unsafe to cache Vary: * or Vary: Cookie)

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use lru::LruCache;
use rusqlite::{params, Connection};
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
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
    /// `public`: the response may be heuristically cached even for a status
    /// that is not cacheable by default (RFC 9111 §4.2.2).
    pub public: bool,
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
            } else if token.eq_ignore_ascii_case("public") {
                cc.public = true;
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

/// A single stored HTTP response (in-memory representation).
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
    /// When the entry was stored or last revalidated by a 304.
    pub stored_at: Instant,
    /// Age (seconds) the response had already accumulated at `stored_at`
    /// (the `Age` header, RFC 9111 §4.2.3).
    pub initial_age: u64,
    /// Freshness lifetime in seconds. `None` means we have a validator but
    /// no definite expiry → revalidate every time.
    pub lifetime: Option<u64>,
    /// The response said `Cache-Control: no-cache`: stored, but must be
    /// revalidated before every use.
    pub no_cache: bool,
}

impl CacheEntry {
    /// Current age in milliseconds (RFC 9111 §4.2.3, `current_age`).
    pub fn age_ms(&self) -> u64 {
        let resident = u64::try_from(self.stored_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        resident.saturating_add(self.initial_age.saturating_mul(1000))
    }

    /// True if the entry is fresh and can be served without revalidation.
    pub fn is_fresh(&self) -> bool {
        !self.no_cache && self.lifetime.is_some_and(|l| self.age_ms() < l.saturating_mul(1000))
    }

    /// Build conditional GET headers to revalidate this entry.
    /// Returns an extra-headers string ready to append to the request.
    pub fn conditional_headers(&self) -> String {
        conditional_headers_for(self.etag.as_deref(), self.last_modified.as_deref())
    }
}

/// `If-None-Match` (preferred) or `If-Modified-Since` request lines for the
/// given validators; empty when there is none.
fn conditional_headers_for(etag: Option<&str>, last_modified: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(etag) = etag {
        out.push_str("If-None-Match: ");
        out.push_str(etag);
        out.push_str("\r\n");
    } else if let Some(lm) = last_modified {
        out.push_str("If-Modified-Since: ");
        out.push_str(lm);
        out.push_str("\r\n");
    }
    out
}

/// Storage-time freshness facts of a response (RFC 9111 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseMeta {
    /// Freshness lifetime in seconds; `None` — no explicit or heuristic one.
    pub lifetime: Option<u64>,
    /// `Age` header value in seconds (0 when absent).
    pub initial_age: u64,
}

/// Statuses a cache may keep with heuristic freshness without `public`
/// (RFC 9110 §15.1).
fn heuristically_cacheable(status: u16) -> bool {
    matches!(status, 200 | 203 | 204 | 206 | 300 | 301 | 308 | 404 | 405 | 410 | 414 | 501)
}

/// Statuses never stored: partial content is not reassembled, 304 carries no
/// body of its own, 1xx are interim.
pub(crate) fn status_storable(status: u16) -> bool {
    !(status < 200 || status == 206 || status == 304)
}

/// Freshness lifetime of a response, in the order RFC 9111 §4.2.1 gives:
/// `max-age` (`s-maxage` is for shared caches, this one is private), then
/// `Expires - Date`, then heuristic 10 % of `Date - Last-Modified`. An
/// unparsable `Expires` means already expired (§5.3).
pub fn response_meta(status: u16, headers: &[(String, String)], now_unix: u64) -> ResponseMeta {
    let cc = CacheControl::parse(header_value(headers, "cache-control").unwrap_or_default());
    let date = header_value(headers, "date")
        .and_then(parse_http_date_to_unix)
        .unwrap_or(now_unix);
    let lifetime = if let Some(max_age) = cc.max_age {
        Some(max_age)
    } else if let Some(expires) = header_value(headers, "expires") {
        Some(parse_http_date_to_unix(expires).map_or(0, |e| e.saturating_sub(date)))
    } else if let Some(lm) = header_value(headers, "last-modified")
        && (cc.public || heuristically_cacheable(status))
    {
        parse_http_date_to_unix(lm).map(|lm| date.saturating_sub(lm) / 10)
    } else {
        None
    };
    let initial_age = header_value(headers, "age")
        .and_then(|a| a.trim().parse::<u64>().ok())
        .unwrap_or(0);
    ResponseMeta { lifetime, initial_age }
}

/// Fold the headers of a 304 into the stored ones (RFC 9111 §3.2): same-named
/// stored fields are replaced, except those describing the body we keep.
pub(crate) fn merge_304_headers(stored: &mut Vec<(String, String)>, headers_304: &[(String, String)]) {
    const KEEP: &[&str] = &[
        "content-length", "content-encoding", "content-range", "transfer-encoding",
        "connection", "keep-alive", "proxy-connection", "te", "trailer", "upgrade",
    ];
    for (name, value) in headers_304 {
        if KEEP.iter().any(|k| name.eq_ignore_ascii_case(k)) {
            continue;
        }
        stored.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        stored.push((name.clone(), value.clone()));
    }
}

/// Snapshot of a stored response with its current age and freshness lifetime.
fn make_snapshot(
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    status: u16,
    validators: (Option<String>, Option<String>),
    age_ms: u64,
    lifetime: Option<u64>,
) -> CacheEntrySnapshot {
    let (etag, last_modified) = validators;
    let cc = CacheControl::parse(header_value(&headers, "cache-control").unwrap_or_default());
    let is_fresh = !cc.no_cache && lifetime.is_some_and(|l| age_ms < l.saturating_mul(1000));
    let conditional_headers = conditional_headers_for(etag.as_deref(), last_modified.as_deref());
    CacheEntrySnapshot {
        body,
        headers,
        status,
        etag,
        last_modified,
        is_fresh,
        conditional_headers,
        age_secs: age_ms / 1000,
        lifetime_secs: lifetime,
        no_cache: cc.no_cache,
        must_revalidate: cc.must_revalidate,
    }
}

// ── Owned snapshot ────────────────────────────────────────────────────────────

/// Owned snapshot of a cache entry returned by `HttpCacheBackend::get`.
#[derive(Debug)]
pub struct CacheEntrySnapshot {
    /// Cached response body.
    pub body: Vec<u8>,
    /// All response headers.
    pub headers: Vec<(String, String)>,
    /// HTTP status code.
    pub status: u16,
    /// ETag for conditional GET.
    pub etag: Option<String>,
    /// Last-Modified for conditional GET.
    pub last_modified: Option<String>,
    /// True if the entry is fresh and can be served without revalidation.
    pub is_fresh: bool,
    /// Pre-built conditional GET headers string (If-None-Match or If-Modified-Since).
    pub conditional_headers: String,
    /// Current age in whole seconds (stored `Age` plus time in the cache).
    pub age_secs: u64,
    /// Freshness lifetime in seconds; `None` — validator-only entry.
    pub lifetime_secs: Option<u64>,
    /// The response carried `Cache-Control: no-cache`.
    pub no_cache: bool,
    /// The response carried `Cache-Control: must-revalidate`.
    pub must_revalidate: bool,
}

// ── Backend trait ──────────────────────────────────────────────────────────────

/// Shared interface for HTTP cache backends (in-memory and disk).
///
/// Both [`HttpCache`] (memory) and [`DiskHttpCache`] (SQLite) implement this.
/// Wrap in `Arc<dyn HttpCacheBackend>` to pass either to `HttpClient::with_http_cache`.
pub trait HttpCacheBackend: Send + Sync {
    /// Get the snapshot for `url` if it exists (fresh or stale with validators).
    ///
    /// Returns `None` on a true miss (no entry at all).
    fn get(&self, url: &str) -> Option<CacheEntrySnapshot>;

    /// Store a response. Ignores entries that must not be cached (no-store,
    /// no freshness info and no validators).
    fn store(&self, url: &str, status: u16, body: Vec<u8>, headers: &[(String, String)]);

    /// Refresh validators and freshness after receiving a 304 Not Modified.
    fn revalidate(&self, url: &str, headers_304: &[(String, String)]);

    /// Delete every entry whose URL satisfies `matches` (UX-PARTITION: clear one
    /// site's cache). Returns the number of removed entries.
    fn clear_matching(&self, matches: &dyn Fn(&str) -> bool) -> usize;

    /// Delete the entry stored for `url`, if any (cache invalidation after an
    /// unsafe request, RFC 9111 §4.4).
    fn remove(&self, url: &str);

    /// Number of entries stored. Used in integration tests.
    #[cfg(test)]
    fn len(&self) -> usize {
        0
    }

    /// True if the cache has no entries. Used in integration tests.
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ── In-memory cache ───────────────────────────────────────────────────────────

/// Thread-safe in-memory HTTP response cache with LRU eviction (RFC 7234).
///
/// Wire up via `HttpClient::with_http_cache(Arc::new(HttpCache::new()))`.
/// Phase 1: in-memory LRU cache with 50 MB capacity limit.
/// When entries exceed 50 MB, least-recently-used entries are evicted.
const MAX_CACHE_SIZE_BYTES: usize = 50 * 1024 * 1024; // 50 MB

pub struct HttpCache {
    entries: Mutex<LruCache<String, CacheEntry>>,
    current_size_bytes: Mutex<usize>,
}

impl HttpCache {
    /// Create an empty cache with LRU eviction and 50 MB limit.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(LruCache::new(NonZeroUsize::new(10000).unwrap())),
            current_size_bytes: Mutex::new(0),
        }
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

    fn calculate_entry_size(&self, entry: &CacheEntry) -> usize {
        let mut size = entry.body.len();
        size += entry.headers.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
        if let Some(etag) = &entry.etag {
            size += etag.len();
        }
        if let Some(lm) = &entry.last_modified {
            size += lm.len();
        }
        // Account for struct overhead (roughly)
        size += 64;
        size
    }
}

impl Default for HttpCache {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpCacheBackend for HttpCache {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn get(&self, url: &str) -> Option<CacheEntrySnapshot> {
        let mut entries = self.entries.lock().unwrap();
        entries.get(&cache_key(url)).map(|e| {
            make_snapshot(
                e.body.clone(),
                e.headers.clone(),
                e.status,
                (e.etag.clone(), e.last_modified.clone()),
                e.age_ms(),
                e.lifetime,
            )
        })
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn store(&self, url: &str, status: u16, body: Vec<u8>, headers: &[(String, String)]) {
        let cc_value = header_value(headers, "cache-control").unwrap_or_default();
        let cc = CacheControl::parse(cc_value);

        if cc.no_store || !status_storable(status) {
            return;
        }

        let etag = header_value(headers, "etag").map(str::to_owned);
        let last_modified = header_value(headers, "last-modified").map(str::to_owned);
        let meta = response_meta(status, headers, unix_now());

        if meta.lifetime.is_none() && etag.is_none() && last_modified.is_none() {
            return;
        }

        let entry = CacheEntry {
            body,
            headers: headers.to_vec(),
            status,
            etag,
            last_modified,
            stored_at: Instant::now(),
            initial_age: meta.initial_age,
            lifetime: meta.lifetime,
            no_cache: cc.no_cache,
        };

        let entry_size = self.calculate_entry_size(&entry);
        let key = cache_key(url);

        let mut entries = self.entries.lock().unwrap();
        let mut size = self.current_size_bytes.lock().unwrap();

        if let Some(old_entry) = entries.peek(&key) {
            *size = size.saturating_sub(self.calculate_entry_size(old_entry));
        }

        *size = size.saturating_add(entry_size);

        while *size > MAX_CACHE_SIZE_BYTES {
            if let Some((_, evicted)) = entries.pop_lru() {
                *size = size.saturating_sub(self.calculate_entry_size(&evicted));
            } else {
                break;
            }
        }

        entries.put(key, entry);
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn revalidate(&self, url: &str, headers_304: &[(String, String)]) {
        let mut guard = self.entries.lock().unwrap();
        let key = cache_key(url);
        let Some(entry) = guard.get_mut(&key) else {
            return;
        };

        merge_304_headers(&mut entry.headers, headers_304);
        entry.etag = header_value(&entry.headers, "etag").map(str::to_owned);
        entry.last_modified = header_value(&entry.headers, "last-modified").map(str::to_owned);
        let meta = response_meta(entry.status, &entry.headers, unix_now());
        entry.stored_at = Instant::now();
        entry.initial_age = meta.initial_age;
        entry.lifetime = meta.lifetime;
        entry.no_cache =
            CacheControl::parse(header_value(&entry.headers, "cache-control").unwrap_or_default()).no_cache;
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn remove(&self, url: &str) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(old) = entries.pop(&cache_key(url)) {
            let mut size = self.current_size_bytes.lock().unwrap();
            *size = size.saturating_sub(self.calculate_entry_size(&old));
        }
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn clear_matching(&self, matches: &dyn Fn(&str) -> bool) -> usize {
        let mut entries = self.entries.lock().unwrap();
        let mut size = self.current_size_bytes.lock().unwrap();
        let keys: Vec<String> = entries
            .iter()
            .filter(|(k, _)| matches(k))
            .map(|(k, _)| k.clone())
            .collect();
        for key in &keys {
            if let Some(old) = entries.pop(key) {
                *size = size.saturating_sub(self.calculate_entry_size(&old));
            }
        }
        keys.len()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

// ── Legacy lookup enum ────────────────────────────────────────────────────────

/// `CacheLookup` is unused externally; we use `get()` which returns `Option<CacheEntrySnapshot>`.
#[allow(dead_code)]
pub enum CacheLookup<'a> {
    Fresh(&'a CacheEntry),
    Stale(&'a CacheEntry),
    Miss,
}

// ── Disk cache ────────────────────────────────────────────────────────────────

/// Error type for [`DiskHttpCache`] operations.
#[derive(Debug)]
pub enum DiskCacheError {
    /// SQLite error.
    Sqlite(rusqlite::Error),
    /// Could not determine the OS cache directory.
    NoCacheDir,
    /// I/O error creating cache directory.
    Io(String),
}

impl std::fmt::Display for DiskCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "SQLite error: {e}"),
            Self::NoCacheDir => write!(f, "could not determine Lumen cache directory"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl From<rusqlite::Error> for DiskCacheError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// SQLite-backed HTTP cache that survives browser restarts (RFC 7234 Phase 1).
///
/// Schema: `cache_entries(url TEXT PK, etag, last_modified, status, headers_json, body_blob, expires_at)`.
/// `expires_at` stores Unix seconds; `NULL` means no definite expiry (validator-only entry).
/// Thread-safe via `Mutex<Connection>`.
pub struct DiskHttpCache {
    conn: Mutex<Connection>,
}

impl DiskHttpCache {
    /// Open or create a cache database at `path`.
    ///
    /// Creates the `cache_entries` table if it does not exist.
    /// `path` is typically `lumen_cache_dir()?.join("http_cache.db")`.
    pub fn new(path: &Path) -> Result<Self, DiskCacheError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DiskCacheError::Io(e.to_string()))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cache_entries (
                url            TEXT    PRIMARY KEY,
                etag           TEXT,
                last_modified  TEXT,
                status         INTEGER NOT NULL,
                headers_json   TEXT    NOT NULL,
                body_blob      BLOB    NOT NULL,
                expires_at     INTEGER
            );",
        )?;
        // Age bookkeeping added with the fetch() cache path; databases written
        // before it lack the columns (the ALTER fails harmlessly once they exist).
        for col in ["stored_at_ms INTEGER", "initial_age INTEGER", "lifetime INTEGER"] {
            let _ = conn.execute(&format!("ALTER TABLE cache_entries ADD COLUMN {col}"), []);
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Open or create the default cache database at [`lumen_cache_dir`]`/http_cache.db`.
    ///
    /// Returns `Err(DiskCacheError::NoCacheDir)` when the OS cache directory cannot be
    /// determined (e.g., `HOME` and `APPDATA` are both unset).
    pub fn open_default() -> Result<Self, DiskCacheError> {
        let dir = lumen_cache_dir().ok_or(DiskCacheError::NoCacheDir)?;
        Self::new(&dir.join("http_cache.db"))
    }
}

impl HttpCacheBackend for DiskHttpCache {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn get(&self, url: &str) -> Option<CacheEntrySnapshot> {
        let conn = self.conn.lock().unwrap();
        let key = cache_key(url);
        let result = conn.query_row(
            "SELECT etag, last_modified, status, headers_json, body_blob, expires_at,
                    stored_at_ms, initial_age, lifetime
             FROM cache_entries WHERE url = ?1",
            params![key],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)? as u16,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                ))
            },
        );

        let (etag, last_modified, status, headers_json, body, expires_unix, stored_ms, initial_age, lifetime_col) =
            result.ok()?;
        let headers = deserialize_headers(&headers_json);

        // Rows written before the age columns existed have no `stored_at_ms`:
        // treat them as stored just now, with whatever expiry remains.
        let (age_ms, lifetime) = match stored_ms {
            Some(stored) => {
                let initial = u64::try_from(initial_age.unwrap_or(0)).unwrap_or(0);
                let age = unix_now_ms()
                    .saturating_sub(u64::try_from(stored).unwrap_or(0))
                    .saturating_add(initial.saturating_mul(1000));
                (age, lifetime_col.and_then(|l| u64::try_from(l).ok()))
            }
            None => match expires_unix {
                Some(exp) if exp > 0 => (0, Some((exp as u64).saturating_sub(unix_now()))),
                _ => (0, None),
            },
        };

        Some(make_snapshot(body, headers, status, (etag, last_modified), age_ms, lifetime))
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn store(&self, url: &str, status: u16, body: Vec<u8>, headers: &[(String, String)]) {
        let cc_value = header_value(headers, "cache-control").unwrap_or_default();
        let cc = CacheControl::parse(cc_value);
        if cc.no_store || !status_storable(status) {
            return;
        }

        let etag = header_value(headers, "etag").map(str::to_owned);
        let last_modified = header_value(headers, "last-modified").map(str::to_owned);
        let meta = response_meta(status, headers, unix_now());

        if meta.lifetime.is_none() && etag.is_none() && last_modified.is_none() {
            return;
        }

        let headers_json = serialize_headers(headers);
        let key = cache_key(url);
        let now_ms = unix_now_ms();
        let expires_unix = write_expiry(now_ms, &meta);
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO cache_entries
             (url, etag, last_modified, status, headers_json, body_blob, expires_at,
              stored_at_ms, initial_age, lifetime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                key, etag, last_modified, status as i64, headers_json, body, expires_unix,
                now_ms as i64, meta.initial_age as i64, meta.lifetime.map(|l| l as i64)
            ],
        );
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn revalidate(&self, url: &str, headers_304: &[(String, String)]) {
        let key = cache_key(url);
        let conn = self.conn.lock().unwrap();
        let Ok((status, headers_json)) = conn.query_row(
            "SELECT status, headers_json FROM cache_entries WHERE url = ?1",
            params![key],
            |row| Ok((row.get::<_, i64>(0)? as u16, row.get::<_, String>(1)?)),
        ) else {
            return;
        };

        let mut headers = deserialize_headers(&headers_json);
        merge_304_headers(&mut headers, headers_304);
        let etag = header_value(&headers, "etag").map(str::to_owned);
        let last_modified = header_value(&headers, "last-modified").map(str::to_owned);
        let meta = response_meta(status, &headers, unix_now());
        let now_ms = unix_now_ms();
        let _ = conn.execute(
            "UPDATE cache_entries
             SET etag = ?1, last_modified = ?2, headers_json = ?3, expires_at = ?4,
                 stored_at_ms = ?5, initial_age = ?6, lifetime = ?7
             WHERE url = ?8",
            params![
                etag, last_modified, serialize_headers(&headers), write_expiry(now_ms, &meta),
                now_ms as i64, meta.initial_age as i64, meta.lifetime.map(|l| l as i64), key
            ],
        );
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn remove(&self, url: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM cache_entries WHERE url = ?1", params![cache_key(url)]);
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn clear_matching(&self, matches: &dyn Fn(&str) -> bool) -> usize {
        let conn = self.conn.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT url FROM cache_entries") else {
            return 0;
        };
        let urls: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map(|rows| rows.filter_map(std::result::Result::ok).collect())
            .unwrap_or_default();
        drop(stmt);
        urls.iter()
            .filter(|u| matches(u))
            .filter_map(|u| conn.execute("DELETE FROM cache_entries WHERE url = ?1", params![u]).ok())
            .sum()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM cache_entries", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }
}

// ── Cache directory ───────────────────────────────────────────────────────────

/// Returns the Lumen cache directory for the current user.
///
/// - Windows: `%APPDATA%\lumen\cache`
/// - Unix:    `$XDG_CACHE_HOME/lumen` or `~/.cache/lumen`
///
/// Returns `None` only when neither `%APPDATA%`/`XDG_CACHE_HOME`/`HOME`
/// (nor `USERPROFILE` on Windows) is set.
pub fn lumen_cache_dir() -> Option<PathBuf> {
    let base: PathBuf = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))?
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))
            })?
    };
    Some(base.join("lumen").join("cache"))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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

/// Current Unix timestamp in milliseconds.
fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Absolute expiry (Unix seconds) written to the legacy `expires_at` column.
fn write_expiry(now_ms: u64, meta: &ResponseMeta) -> Option<i64> {
    meta.lifetime
        .map(|l| ((now_ms / 1000).saturating_add(l).saturating_sub(meta.initial_age)) as i64)
}

/// Current Unix timestamp in seconds.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Serialize headers to a minimal JSON array: `[["k","v"],...]`.
fn serialize_headers(headers: &[(String, String)]) -> String {
    let mut out = String::from('[');
    for (i, (k, v)) in headers.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, k);
        out.push(',');
        push_json_string(&mut out, v);
        out.push(']');
    }
    out.push(']');
    out
}

/// Deserialize headers from the format produced by [`serialize_headers`].
fn deserialize_headers(json: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let json = json.trim();
    if !json.starts_with('[') || !json.ends_with(']') {
        return out;
    }
    let inner = &json[1..json.len() - 1];
    let mut rest = inner.trim();
    while !rest.is_empty() {
        if !rest.starts_with('[') {
            break;
        }
        let close = find_matching_bracket(rest);
        let pair_str = &rest[1..close];
        if let Some((k, v)) = parse_json_pair(pair_str) {
            out.push((k, v));
        }
        rest = rest[close + 1..].trim_start_matches(',').trim_start();
    }
    out
}

/// Find the closing `]` position matching the opening `[` at index 0.
fn find_matching_bracket(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' if in_str => escaped = true,
            b'"' => in_str = !in_str,
            b'[' if !in_str => depth += 1,
            b']' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    s.len().saturating_sub(1)
}

/// Parse `"key","value"` into `(key, value)`.
fn parse_json_pair(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let (key, rest) = parse_json_string(s)?;
    let rest = rest.trim_start_matches(',').trim_start();
    let (val, _) = parse_json_string(rest)?;
    Some((key, val))
}

/// Parse one JSON string literal from the start of `s`. Returns `(value, rest)`.
fn parse_json_string(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((out, &s[i + 1..])),
            b'\\' => {
                i += 1;
                match bytes.get(i).copied()? {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    other => out.push(other as char),
                }
            }
            c => out.push(c as char),
        }
        i += 1;
    }
    None
}

/// Append a JSON-escaped string literal to `buf`.
fn push_json_string(buf: &mut String, s: &str) {
    buf.push('"');
    for c in s.chars() {
        match c {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            other => buf.push(other),
        }
    }
    buf.push('"');
}

// ── HTTP-date parser ──────────────────────────────────────────────────────────

/// Parse a subset of HTTP-date formats to Unix timestamp (seconds).
///
/// Handles RFC 7231 IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) and
/// RFC 850 (`Sunday, 06-Nov-94 08:49:37 GMT`) — both common in the wild.
fn parse_http_date_to_unix(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = if let Some(pos) = s.find(", ") {
        &s[pos + 2..]
    } else {
        match s.find(", ") {
            Some(pos) => &s[pos + 2..],
            None => s,
        }
    };
    let parts: Vec<&str> = s.splitn(6, ' ').collect();
    if parts.len() < 4 {
        return None;
    }
    let day: u64 = parts[0].trim_matches('-').parse().ok()?;
    let month = month_to_num(parts[1])?;
    let year_str = parts[2];
    let year: u64 = if year_str.len() == 2 {
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

fn days_since_epoch(year: u64, month: u64, day: u64) -> Option<u64> {
    if year < 1970 {
        return None;
    }
    let y = year as i64;
    let m = month as i64;
    let d = day as i64;
    let a = (14 - m) / 12;
    let yy = y + 4800 - a;
    let mm = m + 12 * a - 3;
    let jdn = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32045;
    let epoch_jdn: i64 = 2_440_588;
    let days = jdn - epoch_jdn;
    if days < 0 { None } else { Some(days as u64) }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── HttpCache (memory) tests ───────────────────────────────────────────────

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
        assert!(cache.get("https://example.com/page").is_some());
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
            stored_at: Instant::now(),
            initial_age: 0,
            lifetime: None,
            no_cache: false,
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
            stored_at: Instant::now(),
            initial_age: 0,
            lifetime: None,
            no_cache: false,
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
        assert_eq!(unix, Some(19723 * 86400));
    }

    #[test]
    fn lru_eviction_respects_50mb_limit() {
        let cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_owned(), "max-age=3600".to_owned())];
        let large_body_20mb = vec![0u8; 20 * 1024 * 1024];

        cache.store("https://example.com/1.bin", 200, large_body_20mb.clone(), &headers);
        assert_eq!(cache.len(), 1);
        cache.store("https://example.com/2.bin", 200, large_body_20mb.clone(), &headers);
        assert_eq!(cache.len(), 2);
        cache.store("https://example.com/3.bin", 200, large_body_20mb.clone(), &headers);
        assert!(cache.len() <= 2, "LRU should evict old entries when cache exceeds 50MB");
        assert!(cache.get("https://example.com/1.bin").is_none());
        assert!(cache.get("https://example.com/2.bin").is_some());
        assert!(cache.get("https://example.com/3.bin").is_some());
    }

    #[test]
    fn lru_eviction_marks_accessed_as_recent() {
        let cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_owned(), "max-age=3600".to_owned())];
        let large_body_20mb = vec![0u8; 20 * 1024 * 1024];

        cache.store("https://example.com/1.bin", 200, large_body_20mb.clone(), &headers);
        cache.store("https://example.com/2.bin", 200, large_body_20mb.clone(), &headers);
        let _ = cache.get("https://example.com/1.bin");
        cache.store("https://example.com/3.bin", 200, large_body_20mb.clone(), &headers);

        assert!(cache.get("https://example.com/1.bin").is_some());
        assert!(cache.get("https://example.com/2.bin").is_none(), "2.bin should be evicted");
        assert!(cache.get("https://example.com/3.bin").is_some());
    }

    #[test]
    fn lru_eviction_on_update_same_entry() {
        let cache = HttpCache::new();
        let headers = vec![("Cache-Control".to_owned(), "max-age=3600".to_owned())];
        let large_body_20mb = vec![0u8; 20 * 1024 * 1024];
        let small_body = vec![0u8; 100];

        cache.store("https://example.com/large.bin", 200, large_body_20mb.clone(), &headers);
        assert_eq!(cache.len(), 1);

        cache.store("https://example.com/large.bin", 200, small_body.clone(), &headers);
        assert_eq!(cache.len(), 1, "same URL should update, not add");

        cache.store("https://example.com/other.bin", 200, large_body_20mb.clone(), &headers);
        assert_eq!(cache.len(), 2, "both should fit after shrinking first entry");
    }

    // ── DiskHttpCache tests ───────────────────────────────────────────────────

    /// RAII guard for a test-only temp SQLite DB file.
    ///
    /// Removes the file on drop, including during panic unwinding — unlike a bare
    /// `remove_file` at the tail of a test body, this can't leak when the test
    /// itself fails (BUG-632: a leaked file from a PID-reused, panicked prior run
    /// was silently inherited as non-empty by the next `DiskHttpCache::new` at the
    /// same path, causing that run to fail too — self-perpetuating). Also removes
    /// any pre-existing file at the chosen path before handing it out, so garbage
    /// left over from before this guard existed can't collide either.
    struct TmpDbGuard(PathBuf);

    impl TmpDbGuard {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static CTR: AtomicU64 = AtomicU64::new(0);
            let n = CTR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("lumen_test_http_cache_{}_{}.db", std::process::id(), n));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for TmpDbGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn disk_cache_new_creates_db() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn disk_cache_store_and_get_fresh() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let headers = vec![
            ("Cache-Control".to_owned(), "max-age=3600".to_owned()),
            ("ETag".to_owned(), "\"disk-v1\"".to_owned()),
        ];
        cache.store("https://example.com/page.css", 200, b"body{}".to_vec(), &headers);
        assert_eq!(cache.len(), 1);

        let snap = cache.get("https://example.com/page.css").unwrap();
        assert!(snap.is_fresh, "entry with max-age=3600 should be fresh");
        assert_eq!(snap.body, b"body{}");
        assert_eq!(snap.etag.as_deref(), Some("\"disk-v1\""));
    }

    #[test]
    fn disk_cache_no_store_not_persisted() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let headers = vec![("Cache-Control".to_owned(), "no-store".to_owned())];
        cache.store("https://example.com/secret", 200, b"data".to_vec(), &headers);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn disk_cache_stale_entry_not_fresh() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        // max-age=0 → expires immediately
        let headers = vec![
            ("Cache-Control".to_owned(), "max-age=0".to_owned()),
            ("ETag".to_owned(), "\"v1\"".to_owned()),
        ];
        cache.store("https://example.com/stale", 200, b"old".to_vec(), &headers);
        let snap = cache.get("https://example.com/stale").unwrap();
        assert!(!snap.is_fresh, "max-age=0 should not be fresh");
        assert!(snap.conditional_headers.contains("If-None-Match"));
    }

    #[test]
    fn disk_cache_revalidate_updates_etag() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let headers = vec![
            ("Cache-Control".to_owned(), "max-age=0".to_owned()),
            ("ETag".to_owned(), "\"v1\"".to_owned()),
        ];
        cache.store("https://example.com/data.json", 200, b"{}".to_vec(), &headers);

        let headers_304 = vec![
            ("ETag".to_owned(), "\"v2\"".to_owned()),
            ("Cache-Control".to_owned(), "max-age=600".to_owned()),
        ];
        cache.revalidate("https://example.com/data.json", &headers_304);

        let snap = cache.get("https://example.com/data.json").unwrap();
        assert_eq!(snap.etag.as_deref(), Some("\"v2\""), "ETag should be updated");
        assert!(snap.is_fresh, "should be fresh after revalidation with max-age=600");
    }

    #[test]
    fn disk_cache_survives_reopen() {
        let tmp = TmpDbGuard::new();
        {
            let cache = DiskHttpCache::new(&tmp.0).unwrap();
            let headers = vec![("Cache-Control".to_owned(), "max-age=3600".to_owned())];
            cache.store("https://example.com/persistent", 200, b"data".to_vec(), &headers);
            assert_eq!(cache.len(), 1);
        } // cache is dropped, connection closed

        let cache2 = DiskHttpCache::new(&tmp.0).unwrap();
        assert_eq!(cache2.len(), 1, "entry should persist across reopen");
        let snap = cache2.get("https://example.com/persistent").unwrap();
        assert_eq!(snap.body, b"data");
        assert!(snap.is_fresh);
    }

    #[test]
    fn clear_matching_removes_only_matching_urls() {
        let mem = HttpCache::new();
        let h = vec![("Cache-Control".to_owned(), "max-age=60".to_owned())];
        mem.store("https://a.example/x", 200, b"1".to_vec(), &h);
        mem.store("https://b.example/y", 200, b"2".to_vec(), &h);
        assert_eq!(mem.clear_matching(&|u| u.contains("a.example")), 1);
        assert!(mem.get("https://a.example/x").is_none());
        assert!(mem.get("https://b.example/y").is_some());

        let tmp = TmpDbGuard::new();
        let disk = DiskHttpCache::new(&tmp.0).unwrap();
        disk.store("https://a.example/x", 200, b"1".to_vec(), &h);
        disk.store("https://b.example/y", 200, b"2".to_vec(), &h);
        assert_eq!(disk.clear_matching(&|u| u.contains("a.example")), 1);
        assert_eq!(disk.len(), 1);
    }

    #[test]
    fn disk_cache_fragment_stripped() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let headers = vec![("Cache-Control".to_owned(), "max-age=60".to_owned())];
        cache.store("https://example.com/page#section", 200, b"content".to_vec(), &headers);
        assert!(cache.get("https://example.com/page").is_some());
        assert!(cache.get("https://example.com/page#other").is_some());
    }

    #[test]
    fn lumen_cache_dir_is_some() {
        // On CI and dev machines, HOME/APPDATA/USERPROFILE is always set.
        assert!(lumen_cache_dir().is_some());
    }

    // ── Header JSON serialization tests ───────────────────────────────────────

    #[test]
    fn header_serialization_roundtrip() {
        let headers = vec![
            ("Content-Type".to_owned(), "text/html; charset=utf-8".to_owned()),
            ("ETag".to_owned(), "\"v1\"".to_owned()),
            (
                "X-Special".to_owned(),
                r#"val with "quotes" and \backslash"#.to_owned(),
            ),
        ];
        let json = serialize_headers(&headers);
        let back = deserialize_headers(&json);
        assert_eq!(headers, back);
    }

    #[test]
    fn header_serialization_empty() {
        let json = serialize_headers(&[]);
        assert_eq!(json, "[]");
        let back = deserialize_headers(&json);
        assert!(back.is_empty());
    }

    // ── FETCH-HTTP-CACHE: freshness lifetime, Age, 304 header merge ──────────

    fn hdrs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    const NOW: u64 = 784_111_777; // Sun, 06 Nov 1994 08:49:37 GMT
    const NOW_DATE: &str = "Sun, 06 Nov 1994 08:49:37 GMT";

    #[test]
    fn meta_prefers_max_age_over_expires_and_ignores_s_maxage() {
        let m = response_meta(
            200,
            &hdrs(&[
                ("Cache-Control", "max-age=60, s-maxage=3600"),
                ("Expires", "Sun, 06 Nov 1994 09:49:37 GMT"),
            ]),
            NOW,
        );
        assert_eq!(m.lifetime, Some(60));
    }

    #[test]
    fn meta_expires_is_measured_from_date_and_bad_expires_is_stale() {
        let m = response_meta(
            200,
            &hdrs(&[("Date", NOW_DATE), ("Expires", "Sun, 06 Nov 1994 09:49:37 GMT")]),
            NOW + 500,
        );
        assert_eq!(m.lifetime, Some(3600), "Date, not receipt time, is the origin");
        assert_eq!(response_meta(200, &hdrs(&[("Expires", "0")]), NOW).lifetime, Some(0));
        let past = response_meta(200, &hdrs(&[("Expires", "Sun, 06 Nov 1994 07:49:37 GMT")]), NOW);
        assert_eq!(past.lifetime, Some(0));
    }

    #[test]
    fn meta_heuristic_is_a_tenth_of_age_for_cacheable_statuses_only() {
        let lm = hdrs(&[("Date", NOW_DATE), ("Last-Modified", "Sun, 06 Nov 1994 07:09:37 GMT")]);
        assert_eq!(response_meta(200, &lm, NOW).lifetime, Some(600));
        assert_eq!(response_meta(404, &lm, NOW).lifetime, Some(600));
        assert_eq!(response_meta(201, &lm, NOW).lifetime, None, "201 is not heuristically cacheable");
        let public = hdrs(&[
            ("Date", NOW_DATE),
            ("Last-Modified", "Sun, 06 Nov 1994 07:09:37 GMT"),
            ("Cache-Control", "public"),
        ]);
        assert_eq!(response_meta(201, &public, NOW).lifetime, Some(600), "`public` allows it");
        assert_eq!(response_meta(200, &[], NOW).lifetime, None);
    }

    #[test]
    fn meta_reads_the_age_header() {
        assert_eq!(response_meta(200, &hdrs(&[("Age", "1800")]), NOW).initial_age, 1800);
        assert_eq!(response_meta(200, &hdrs(&[("Age", "junk")]), NOW).initial_age, 0);
    }

    #[test]
    fn age_header_counts_against_the_lifetime() {
        let cache = HttpCache::new();
        let h = hdrs(&[("Cache-Control", "max-age=3600"), ("Age", "3600")]);
        cache.store("https://example.com/aged", 200, b"x".to_vec(), &h);
        let snap = cache.get("https://example.com/aged").unwrap();
        assert!(!snap.is_fresh);
        assert_eq!((snap.age_secs, snap.lifetime_secs), (3600, Some(3600)));
    }

    #[test]
    fn must_revalidate_alone_does_not_make_a_fresh_entry_stale() {
        let cache = HttpCache::new();
        let h = hdrs(&[("Cache-Control", "max-age=3600, must-revalidate")]);
        cache.store("https://example.com/m", 200, b"x".to_vec(), &h);
        let snap = cache.get("https://example.com/m").unwrap();
        assert!(snap.is_fresh && snap.must_revalidate);
        let h = hdrs(&[("Cache-Control", "max-age=3600, no-cache")]);
        cache.store("https://example.com/n", 200, b"x".to_vec(), &h);
        assert!(!cache.get("https://example.com/n").unwrap().is_fresh);
    }

    #[test]
    fn partial_and_not_modified_responses_are_not_stored() {
        let cache = HttpCache::new();
        let h = hdrs(&[("Cache-Control", "max-age=3600")]);
        cache.store("https://example.com/p", 206, b"x".to_vec(), &h);
        cache.store("https://example.com/q", 304, Vec::new(), &h);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn merge_304_replaces_named_headers_but_keeps_the_body_description() {
        let mut stored = hdrs(&[
            ("Test-Header", "A"),
            ("Content-Length", "4"),
            ("Content-Encoding", "gzip"),
            ("ETag", "\"v1\""),
        ]);
        merge_304_headers(
            &mut stored,
            &hdrs(&[("Test-Header", "B"), ("Content-Length", "0"), ("Content-Encoding", "identity")]),
        );
        let get = |n: &str| header_value(&stored, n).map(str::to_owned);
        assert_eq!(get("test-header").as_deref(), Some("B"));
        assert_eq!(get("content-length").as_deref(), Some("4"));
        assert_eq!(get("content-encoding").as_deref(), Some("gzip"));
        assert_eq!(get("etag").as_deref(), Some("\"v1\""));
    }

    #[test]
    fn revalidate_merges_headers_and_recomputes_freshness_in_memory() {
        let cache = HttpCache::new();
        let h = hdrs(&[("Cache-Control", "max-age=0"), ("ETag", "\"v1\""), ("Test-Header", "A")]);
        cache.store("https://example.com/r", 200, b"body".to_vec(), &h);
        cache.revalidate(
            "https://example.com/r",
            &hdrs(&[("Cache-Control", "max-age=600"), ("Test-Header", "B")]),
        );
        let snap = cache.get("https://example.com/r").unwrap();
        assert!(snap.is_fresh);
        assert_eq!(header_value(&snap.headers, "test-header"), Some("B"));
        assert_eq!(snap.body, b"body");
    }

    #[test]
    fn disk_revalidate_merges_headers_and_survives_reopen() {
        let tmp = TmpDbGuard::new();
        {
            let cache = DiskHttpCache::new(&tmp.0).unwrap();
            let h = hdrs(&[("Cache-Control", "max-age=0"), ("ETag", "\"v1\""), ("Test-Header", "A")]);
            cache.store("https://example.com/r", 200, b"body".to_vec(), &h);
            assert!(!cache.get("https://example.com/r").unwrap().is_fresh);
            cache.revalidate(
                "https://example.com/r",
                &hdrs(&[("Cache-Control", "max-age=600"), ("Test-Header", "B")]),
            );
        }
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let snap = cache.get("https://example.com/r").unwrap();
        assert!(snap.is_fresh);
        assert_eq!(header_value(&snap.headers, "test-header"), Some("B"));
        assert_eq!(snap.lifetime_secs, Some(600));
    }

    #[test]
    fn disk_age_header_counts_against_the_lifetime() {
        let tmp = TmpDbGuard::new();
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let h = hdrs(&[("Cache-Control", "max-age=1500"), ("Age", "2000")]);
        cache.store("https://example.com/aged", 200, b"x".to_vec(), &h);
        let snap = cache.get("https://example.com/aged").unwrap();
        assert!(!snap.is_fresh);
        assert_eq!(snap.lifetime_secs, Some(1500));
        assert!(snap.age_secs >= 2000);
    }

    #[test]
    fn remove_drops_one_entry_in_both_backends() {
        let mem = HttpCache::new();
        let tmp = TmpDbGuard::new();
        let disk = DiskHttpCache::new(&tmp.0).unwrap();
        let h = hdrs(&[("Cache-Control", "max-age=60")]);
        for cache in [&mem as &dyn HttpCacheBackend, &disk] {
            cache.store("https://example.com/a#frag", 200, b"a".to_vec(), &h);
            cache.store("https://example.com/b", 200, b"b".to_vec(), &h);
            cache.remove("https://example.com/a");
            assert!(cache.get("https://example.com/a").is_none());
            assert!(cache.get("https://example.com/b").is_some());
        }
    }

    #[test]
    fn disk_cache_opens_a_database_written_before_the_age_columns() {
        let tmp = TmpDbGuard::new();
        {
            let conn = Connection::open(&tmp.0).unwrap();
            conn.execute_batch(
                "CREATE TABLE cache_entries (
                    url TEXT PRIMARY KEY, etag TEXT, last_modified TEXT, status INTEGER NOT NULL,
                    headers_json TEXT NOT NULL, body_blob BLOB NOT NULL, expires_at INTEGER);",
            )
            .unwrap();
            let exp = (unix_now() + 600) as i64;
            conn.execute(
                "INSERT INTO cache_entries VALUES ('https://example.com/old', NULL, NULL, 200, '[]', x'6f6c64', ?1)",
                params![exp],
            )
            .unwrap();
        }
        let cache = DiskHttpCache::new(&tmp.0).unwrap();
        let snap = cache.get("https://example.com/old").unwrap();
        assert!(snap.is_fresh, "legacy row keeps its expiry");
        assert_eq!(snap.body, b"old");
    }
}
