//! Per-origin-group isolation context for `BrowserSession` (Phase 1: 8E).
//!
//! Each browsing context (tab, iframe, worker) can be scoped to an *origin group*
//! so that cookies, `localStorage`, `sessionStorage`, and `IndexedDB` are
//! completely isolated from other origin groups.
//!
//! # Origin grouping
//!
//! Origins are grouped by their eTLD+1 registrable domain so that subdomains
//! share the same isolation context (e.g. `www.example.com` and
//! `api.example.com` both belong to group `"example.com"`).  This matches
//! browser "same-site" semantics.
//!
//! # Usage
//!
//! ```rust,no_run
//! use lumen_driver::{InProcessSession, BrowserSession};
//!
//! // Fully-isolated session for example.com origin group.
//! let mut session = InProcessSession::with_origin_isolation("https://example.com");
//! session.navigate("https://example.com/page").unwrap();
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lumen_core::web_storage::WebStorage;
use lumen_core::ext::{IdbBackend, StorageBackend};
use lumen_storage::store::InMemoryStorage;
use lumen_storage::cookies::CookieJar;
use lumen_storage::indexed_db::IdbStore;

/// eTLD+1 site identifier used to group related origins.
///
/// Subdomains of the same registrable domain share one `OriginGroup`.
/// Computed from the scheme+host+port of an origin URL without a full PSL
/// lookup: in Phase 0 we extract everything after the last dot-separated
/// prefix that exceeds two labels (conservative, good enough for isolation).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OriginGroup {
    /// Registrable domain or host, lowercase (e.g. `"example.com"`).
    pub site: String,
}

impl OriginGroup {
    /// Derive the origin group from a full origin URL or host string.
    ///
    /// Examples:
    /// - `"https://www.example.com"` → `OriginGroup { site: "example.com" }`
    /// - `"https://api.example.com/path"` → `OriginGroup { site: "example.com" }`
    /// - `"https://localhost:3000"` → `OriginGroup { site: "localhost" }`
    /// - `"file:///path"` → `OriginGroup { site: "local" }`
    pub fn for_origin(origin: &str) -> Self {
        let host = extract_host(origin);
        let site = registrable_domain(&host);
        Self { site }
    }
}

/// Per-origin-group isolation container.
///
/// Holds the storage components that must be isolated per origin group:
/// - `CookieJar` — RFC 6265bis cookie store scoped to this origin group.
/// - `localStorage` — one `WebStorage` per origin (persists within the session).
/// - `sessionStorage` — one `WebStorage` per origin (cleared on navigation).
/// - IndexedDB backend — shared `StorageBackend` partitioned by origin.
///
/// Multiple `OriginIsolationContext` instances never share state; creating two
/// contexts for the same origin string still gives independent storage.
pub struct OriginIsolationContext {
    /// The origin group this context is scoped to.
    pub group: OriginGroup,
    /// Per-origin `localStorage` partitions (spec: persists across page reloads).
    local_storage: HashMap<String, Arc<Mutex<WebStorage>>>,
    /// Per-origin `sessionStorage` partitions (spec: cleared on navigation).
    session_storage: HashMap<String, Arc<Mutex<WebStorage>>>,
    /// Shared in-memory backend for all `IdbStore` instances in this context.
    /// `Arc<Mutex>` so multiple `IdbStore` instances can share the same store.
    idb_backend: Arc<Mutex<dyn StorageBackend>>,
    /// RFC 6265bis cookie jar scoped to this origin group.
    cookie_jar: Arc<CookieJar>,
}

impl OriginIsolationContext {
    /// Create a new isolation context for the given origin (URL or host string).
    ///
    /// All storage starts empty. The cookie jar is in-memory (ephemeral);
    /// for persistent storage wire a `SqliteStorage` backend instead.
    pub fn new(origin: &str) -> Self {
        let group = OriginGroup::for_origin(origin);
        let idb_backend: Arc<Mutex<dyn StorageBackend>> =
            Arc::new(Mutex::new(InMemoryStorage::new()));
        let cookie_jar = Arc::new(
            CookieJar::open_in_memory()
                .expect("in-memory cookie jar must succeed"),
        );
        Self {
            group,
            local_storage: HashMap::new(),
            session_storage: HashMap::new(),
            idb_backend,
            cookie_jar,
        }
    }

    /// The site identifier (eTLD+1) of this context's origin group.
    pub fn site(&self) -> &str {
        &self.group.site
    }

    /// Get (or create) the `localStorage` partition for `origin`.
    ///
    /// Returns a shared `Arc<Mutex<WebStorage>>` that persists across page
    /// reloads within the lifetime of this `OriginIsolationContext`.
    pub fn local_storage_for(&mut self, origin: &str) -> Arc<Mutex<WebStorage>> {
        self.local_storage
            .entry(origin.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(WebStorage::default())))
            .clone()
    }

    /// Get (or create) the `sessionStorage` partition for `origin`.
    ///
    /// Returns a shared `Arc<Mutex<WebStorage>>`. Clear on navigation via
    /// `clear_session_storage_for()`.
    pub fn session_storage_for(&mut self, origin: &str) -> Arc<Mutex<WebStorage>> {
        self.session_storage
            .entry(origin.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(WebStorage::default())))
            .clone()
    }

    /// Clear `sessionStorage` for `origin` (spec: cleared on top-level navigation).
    pub fn clear_session_storage_for(&mut self, origin: &str) {
        self.session_storage.remove(origin);
    }

    /// Clear all `sessionStorage` partitions in this context.
    pub fn clear_all_session_storage(&mut self) {
        self.session_storage.clear();
    }

    /// Create an `IdbStore` scoped to `origin` using this context's backend.
    ///
    /// Multiple calls for the same `origin` return independent handle objects
    /// that share the same underlying in-memory storage — matching IndexedDB
    /// spec persistence within a session.
    pub fn idb_store_for(&self, origin: &str) -> IdbStore {
        IdbStore::new(Arc::clone(&self.idb_backend), origin)
    }

    /// Save an IndexedDB JSON snapshot for `origin`.
    pub fn idb_save(&self, origin: &str, snapshot: &str) {
        self.idb_store_for(origin).save(snapshot);
    }

    /// Load the IndexedDB JSON snapshot for `origin`, or `None` if absent.
    pub fn idb_load(&self, origin: &str) -> Option<String> {
        self.idb_store_for(origin).load()
    }

    /// Shared `Arc<CookieJar>` for this origin group.
    ///
    /// Pass this to `CookieJarProvider::new()` to wire the cookie jar into
    /// `HttpClient` for network requests within this context.
    pub fn cookie_jar(&self) -> Arc<CookieJar> {
        Arc::clone(&self.cookie_jar)
    }

    /// Check whether two origins belong to the same origin group (same eTLD+1).
    pub fn same_group(&self, origin: &str) -> bool {
        OriginGroup::for_origin(origin) == self.group
    }
}

// ── URL utilities ─────────────────────────────────────────────────────────────

/// Extract the hostname from a URL string or return the input as-is.
fn extract_host(origin: &str) -> String {
    // Strip scheme (e.g. "https://").
    let after_scheme = if let Some(i) = origin.find("://") {
        &origin[i + 3..]
    } else {
        origin
    };
    // Strip path, query, fragment.
    let host_port = match after_scheme.find('/') {
        Some(i) => &after_scheme[..i],
        None => after_scheme,
    };
    // Strip port.
    let host = match host_port.rfind(':') {
        // IPv6 address: "[::]" — keep brackets, no port strip needed here.
        Some(i) if !host_port.contains('[') => &host_port[..i],
        _ => host_port,
    };
    host.to_lowercase()
}

/// Derive the registrable domain (eTLD+1) from a hostname.
///
/// Conservative heuristic without a full PSL: if the host has more than
/// two dot-separated labels we keep the last two (e.g. `"www.example.com"`
/// → `"example.com"`).  For country-code SLDs (e.g. `"co.uk"`) this may
/// over-group, but it is correct for the vast majority of test cases and
/// sufficient for origin-isolation semantics in Phase 0.
///
/// Special cases:
/// - `"localhost"` or any single-label host → returned as-is.
/// - `"file"` / empty → `"local"`.
fn registrable_domain(host: &str) -> String {
    if host.is_empty() || host == "file" {
        return "local".to_string();
    }
    let labels: Vec<&str> = host.split('.').collect();
    match labels.len() {
        0 => "local".to_string(),
        1 => labels[0].to_string(),
        2 => host.to_string(),
        _ => {
            // Keep last two labels: "www.example.com" → "example.com".
            let n = labels.len();
            format!("{}.{}", labels[n - 2], labels[n - 1])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── OriginGroup::for_origin ───────────────────────────────────────────────

    #[test]
    fn origin_group_strips_subdomain() {
        assert_eq!(
            OriginGroup::for_origin("https://www.example.com").site,
            "example.com"
        );
    }

    #[test]
    fn origin_group_deep_subdomain() {
        assert_eq!(
            OriginGroup::for_origin("https://api.v2.example.com").site,
            "example.com"
        );
    }

    #[test]
    fn origin_group_no_subdomain() {
        assert_eq!(
            OriginGroup::for_origin("https://example.com").site,
            "example.com"
        );
    }

    #[test]
    fn origin_group_localhost() {
        assert_eq!(
            OriginGroup::for_origin("http://localhost:3000").site,
            "localhost"
        );
    }

    #[test]
    fn origin_group_file_url() {
        assert_eq!(OriginGroup::for_origin("file:///path/to/file.html").site, "local");
    }

    #[test]
    fn origin_group_with_path() {
        // Path must be stripped.
        assert_eq!(
            OriginGroup::for_origin("https://example.com/some/path?q=1").site,
            "example.com"
        );
    }

    #[test]
    fn same_group_subdomains_match() {
        let ctx = OriginIsolationContext::new("https://example.com");
        assert!(ctx.same_group("https://api.example.com"));
        assert!(ctx.same_group("https://www.example.com"));
    }

    #[test]
    fn same_group_different_site_no_match() {
        let ctx = OriginIsolationContext::new("https://example.com");
        assert!(!ctx.same_group("https://other.com"));
    }

    // ── local_storage isolation ───────────────────────────────────────────────

    #[test]
    fn local_storage_per_origin_isolated() {
        let mut ctx = OriginIsolationContext::new("https://example.com");
        {
            let ls_a = ctx.local_storage_for("https://a.example.com");
            ls_a.lock().unwrap().set_item("key".into(), "valueA".into());
        }
        {
            let ls_b = ctx.local_storage_for("https://b.example.com");
            assert_eq!(ls_b.lock().unwrap().get_item("key"), None);
        }
    }

    #[test]
    fn local_storage_persists_within_context() {
        let mut ctx = OriginIsolationContext::new("https://example.com");
        ctx.local_storage_for("https://example.com")
            .lock()
            .unwrap()
            .set_item("persist".into(), "yes".into());
        // Second call returns the same Arc.
        let val = ctx
            .local_storage_for("https://example.com")
            .lock()
            .unwrap()
            .get_item("persist")
            .map(String::from);
        assert_eq!(val.as_deref(), Some("yes"));
    }

    #[test]
    fn two_contexts_do_not_share_local_storage() {
        let mut ctx1 = OriginIsolationContext::new("https://example.com");
        let mut ctx2 = OriginIsolationContext::new("https://example.com");
        ctx1.local_storage_for("https://example.com")
            .lock()
            .unwrap()
            .set_item("k".into(), "v1".into());
        let val = ctx2
            .local_storage_for("https://example.com")
            .lock()
            .unwrap()
            .get_item("k")
            .map(String::from);
        assert_eq!(val, None, "two OriginIsolationContexts must not share storage");
    }

    // ── sessionStorage ────────────────────────────────────────────────────────

    #[test]
    fn session_storage_cleared_on_navigation() {
        let mut ctx = OriginIsolationContext::new("https://example.com");
        ctx.session_storage_for("https://example.com")
            .lock()
            .unwrap()
            .set_item("tab".into(), "42".into());
        ctx.clear_session_storage_for("https://example.com");
        // After clear, a fresh partition is created.
        let val = ctx
            .session_storage_for("https://example.com")
            .lock()
            .unwrap()
            .get_item("tab")
            .map(String::from);
        assert_eq!(val, None);
    }

    #[test]
    fn clear_all_session_storage_empties_all_origins() {
        let mut ctx = OriginIsolationContext::new("https://example.com");
        ctx.session_storage_for("https://a.example.com")
            .lock()
            .unwrap()
            .set_item("x".into(), "1".into());
        ctx.session_storage_for("https://b.example.com")
            .lock()
            .unwrap()
            .set_item("y".into(), "2".into());
        ctx.clear_all_session_storage();
        let a = ctx
            .session_storage_for("https://a.example.com")
            .lock()
            .unwrap()
            .get_item("x")
            .map(String::from);
        assert_eq!(a, None);
    }

    // ── IndexedDB isolation ───────────────────────────────────────────────────

    #[test]
    fn idb_save_load_roundtrip() {
        let ctx = OriginIsolationContext::new("https://example.com");
        let snapshot = r#"{"db":{"version":1}}"#;
        ctx.idb_save("https://example.com", snapshot);
        assert_eq!(ctx.idb_load("https://example.com").as_deref(), Some(snapshot));
    }

    #[test]
    fn idb_isolated_between_origins_within_context() {
        let ctx = OriginIsolationContext::new("https://example.com");
        ctx.idb_save("https://a.example.com", "alpha");
        ctx.idb_save("https://b.example.com", "beta");
        assert_eq!(ctx.idb_load("https://a.example.com").as_deref(), Some("alpha"));
        assert_eq!(ctx.idb_load("https://b.example.com").as_deref(), Some("beta"));
    }

    #[test]
    fn idb_isolated_between_contexts() {
        let ctx1 = OriginIsolationContext::new("https://example.com");
        let ctx2 = OriginIsolationContext::new("https://example.com");
        ctx1.idb_save("https://example.com", "ctx1-data");
        assert_eq!(ctx2.idb_load("https://example.com"), None);
    }

    // ── cookie_jar ────────────────────────────────────────────────────────────

    #[test]
    fn cookie_jar_isolated_between_contexts() {
        use lumen_storage::cookies::{Cookie, SameSite};
        let ctx1 = OriginIsolationContext::new("https://example.com");
        let ctx2 = OriginIsolationContext::new("https://example.com");
        let c = Cookie {
            domain: "example.com".into(),
            path: "/".into(),
            name: "session".into(),
            value: "abc".into(),
            expires_at: None,
            secure: false,
            http_only: false,
            same_site: SameSite::Lax,
        };
        ctx1.cookie_jar().set(c, None).unwrap();
        let from_ctx2 = ctx2
            .cookie_jar()
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        assert!(from_ctx2.is_empty(), "cookie jars must be independent");
    }
}
