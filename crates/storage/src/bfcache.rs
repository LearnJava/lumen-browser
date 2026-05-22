//! Back-forward cache (bfcache) — in-memory snapshot store.
//!
//! HTML Living Standard §8.6 defines the bfcache: pages navigated away from
//! are "frozen" in memory, then "thawed" when the user navigates back —
//! avoiding a network round-trip. This module provides the storage layer.
//!
//! Phase 0 scope: in-memory only (no SQLite persistence across restarts).
//! Keyed by the page URL string; entries are evicted LRU when the cache
//! exceeds `max_size`.

use std::collections::{HashMap, VecDeque};

/// Snapshot of a page suitable for bfcache restoration.
#[derive(Debug, Clone)]
pub struct BfCacheEntry {
    /// Absolute URL string used as cache key.
    pub url: String,
    /// Decoded HTML source (UTF-8). Re-parsed on restore to avoid re-fetch.
    pub html: String,
    /// Horizontal scroll offset (CSS px) at the time of cache capture.
    pub scroll_x: f32,
    /// Vertical scroll offset (CSS px) at the time of cache capture.
    pub scroll_y: f32,
    /// `<title>` text of the page, if any.
    pub title: Option<String>,
}

/// In-memory LRU bfcache.
///
/// Entries are keyed by URL string. When `max_size` is exceeded the
/// oldest entry (by insertion/update order) is evicted.
pub struct BfCache {
    entries: HashMap<String, BfCacheEntry>,
    /// Insertion order — front = oldest, back = newest.
    order: VecDeque<String>,
    max_size: usize,
}

impl std::fmt::Debug for BfCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BfCache")
            .field("len", &self.entries.len())
            .field("max_size", &self.max_size)
            .finish()
    }
}

impl BfCache {
    /// Create an empty cache with the given capacity.
    ///
    /// `max_size = 0` means the cache never stores anything (effectively
    /// disabled). Reasonable default for browsers is 16–64.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            max_size,
        }
    }

    /// Store or update an entry.
    ///
    /// If the URL is already cached the existing entry is replaced and its
    /// position is moved to the back (most-recently-used). If adding a new
    /// entry would exceed `max_size` the oldest entry is evicted first.
    pub fn store(&mut self, entry: BfCacheEntry) {
        if self.max_size == 0 {
            return;
        }
        let url = entry.url.clone();
        if self.entries.contains_key(&url) {
            // Move to back (refresh LRU position).
            self.order.retain(|u| u != &url);
        } else if self.order.len() >= self.max_size {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
        self.order.push_back(url.clone());
        self.entries.insert(url, entry);
    }

    /// Return a reference to the entry for `url`, or `None` if not cached.
    pub fn retrieve(&self, url: &str) -> Option<&BfCacheEntry> {
        self.entries.get(url)
    }

    /// Remove the entry for `url` from the cache.
    pub fn remove(&mut self, url: &str) {
        if self.entries.remove(url).is_some() {
            self.order.retain(|u| u != url);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str) -> BfCacheEntry {
        BfCacheEntry {
            url: url.to_owned(),
            html: format!("<html><body>{url}</body></html>"),
            scroll_x: 0.0,
            scroll_y: 0.0,
            title: None,
        }
    }

    #[test]
    fn store_and_retrieve() {
        let mut cache = BfCache::new(8);
        cache.store(entry("https://example.com/"));
        let e = cache.retrieve("https://example.com/").unwrap();
        assert_eq!(e.url, "https://example.com/");
        assert!(e.html.contains("example.com"));
    }

    #[test]
    fn retrieve_missing_returns_none() {
        let cache = BfCache::new(8);
        assert!(cache.retrieve("https://missing.example/").is_none());
    }

    #[test]
    fn eviction_at_max_size() {
        let mut cache = BfCache::new(2);
        cache.store(entry("https://a/"));
        cache.store(entry("https://b/"));
        cache.store(entry("https://c/")); // evicts "a"
        assert!(cache.retrieve("https://a/").is_none());
        assert!(cache.retrieve("https://b/").is_some());
        assert!(cache.retrieve("https://c/").is_some());
    }

    #[test]
    fn update_refreshes_lru_position() {
        let mut cache = BfCache::new(2);
        cache.store(entry("https://a/"));
        cache.store(entry("https://b/"));
        // Refresh "a" — it moves to back; "b" becomes oldest.
        cache.store(entry("https://a/"));
        cache.store(entry("https://c/")); // evicts "b", not "a"
        assert!(cache.retrieve("https://a/").is_some());
        assert!(cache.retrieve("https://b/").is_none());
        assert!(cache.retrieve("https://c/").is_some());
    }

    #[test]
    fn clear_empties_cache() {
        let mut cache = BfCache::new(8);
        cache.store(entry("https://a/"));
        cache.store(entry("https://b/"));
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(cache.retrieve("https://a/").is_none());
    }

    #[test]
    fn remove_single_entry() {
        let mut cache = BfCache::new(8);
        cache.store(entry("https://a/"));
        cache.store(entry("https://b/"));
        cache.remove("https://a/");
        assert!(cache.retrieve("https://a/").is_none());
        assert!(cache.retrieve("https://b/").is_some());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn max_size_zero_stores_nothing() {
        let mut cache = BfCache::new(0);
        cache.store(entry("https://a/"));
        assert!(cache.is_empty());
    }

    #[test]
    fn scroll_and_title_preserved() {
        let mut cache = BfCache::new(8);
        cache.store(BfCacheEntry {
            url: "https://a/".to_owned(),
            html: "<html/>".to_owned(),
            scroll_x: 12.5,
            scroll_y: 340.0,
            title: Some("My Page".to_owned()),
        });
        let e = cache.retrieve("https://a/").unwrap();
        assert!((e.scroll_x - 12.5).abs() < f32::EPSILON);
        assert!((e.scroll_y - 340.0).abs() < f32::EPSILON);
        assert_eq!(e.title.as_deref(), Some("My Page"));
    }

    #[test]
    fn len_matches_stored_entries() {
        let mut cache = BfCache::new(8);
        assert_eq!(cache.len(), 0);
        cache.store(entry("https://a/"));
        assert_eq!(cache.len(), 1);
        cache.store(entry("https://b/"));
        assert_eq!(cache.len(), 2);
        // Duplicate — len stays 2.
        cache.store(entry("https://a/"));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let mut cache = BfCache::new(4);
        cache.store(entry("https://a/"));
        let s = format!("{cache:?}");
        assert!(s.contains("BfCache"));
    }
}
