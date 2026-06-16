//! Ad-block filter-list state backed by SQLite.
//!
//! Stores the **structural** state of the external-filter-list subsystem:
//!
//! - `subscriptions` — which lists the user is subscribed to (EasyList,
//!   EasyPrivacy, …): the canonical URL, a human title and an enabled flag.
//! - `list_meta` — per-list cache metadata: the on-disk slug, source URL,
//!   conditional-GET validators (`ETag` / `Last-Modified`), last-fetch unix
//!   timestamp, parsed rule count and a content hash used to skip re-parsing
//!   when a `200 OK` body is byte-identical to the cached copy.
//!
//! **What lives elsewhere (by design):** the list *bodies* themselves
//! (`lists/<slug>.txt`, ~2 MB each) are stored as plain files, not BLOBs — they
//! are read once at startup into the in-memory matcher. The hot `should_block`
//! path runs entirely in RAM (`EasyListFilter`), never touching this DB. This
//! store holds only queryable structural state. See the task spec
//! `docs/tasks/p2-adblock-filter-lists.md` §1.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

// ── Value types ──────────────────────────────────────────────────────────────

/// A filter-list subscription the user follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    /// Canonical download URL of the list (also the primary key).
    pub url: String,
    /// Human-readable title shown in UI (e.g. `"EasyList"`).
    pub title: String,
    /// Whether this subscription is active. `false` = kept but not loaded.
    pub enabled: bool,
}

/// Cache metadata for one downloaded filter list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListMeta {
    /// Human-readable on-disk slug; links the row to `lists/<slug>.txt`.
    pub slug: String,
    /// Source URL the body was fetched from.
    pub url: String,
    /// `ETag` validator from the last `200 OK` (for conditional GET).
    pub etag: Option<String>,
    /// `Last-Modified` validator from the last `200 OK`.
    pub last_modified: Option<String>,
    /// Unix seconds of the last successful fetch (or `304` revalidation).
    pub fetched_at: i64,
    /// Number of network rules parsed from the body (informational).
    pub rule_count: i64,
    /// Hash of the body text; lets refresh skip re-parsing when unchanged.
    pub content_hash: Option<String>,
}

// ── AdblockStore ──────────────────────────────────────────────────────────────

/// SQLite-backed store for ad-block subscriptions and list cache metadata.
///
/// Thread-safe handle wrapping a connection + mutex. Open once at startup via
/// [`AdblockStore::open`]; mutate as lists are refreshed.
pub struct AdblockStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for AdblockStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdblockStore").finish_non_exhaustive()
    }
}

impl AdblockStore {
    /// Open (or create) the SQLite store at `path`, creating tables if needed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref()).map_err(|e| Error::Storage(e.to_string()))?;
        Self::with_conn(conn)
    }

    /// Open an in-memory store (tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| Error::Storage(e.to_string()))?;
        Self::with_conn(conn)
    }

    fn with_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subscriptions (
                url      TEXT PRIMARY KEY,
                title    TEXT NOT NULL,
                enabled  INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS list_meta (
                slug          TEXT PRIMARY KEY,
                url           TEXT NOT NULL,
                etag          TEXT,
                last_modified TEXT,
                fetched_at    INTEGER NOT NULL DEFAULT 0,
                rule_count    INTEGER NOT NULL DEFAULT 0,
                content_hash  TEXT
            );",
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // ── subscriptions ──────────────────────────────────────────────────────

    /// All subscriptions, ordered by title for stable display.
    pub fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT url, title, enabled FROM subscriptions ORDER BY title")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Subscription {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    enabled: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(|e| Error::Storage(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(e.to_string()))?);
        }
        Ok(out)
    }

    /// Insert or update a subscription (keyed by URL).
    pub fn set_subscription(&self, url: &str, title: &str, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO subscriptions (url, title, enabled) VALUES (?1, ?2, ?3)
             ON CONFLICT(url) DO UPDATE SET title = ?2, enabled = ?3",
            params![url, title, i64::from(enabled)],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    /// Seed the given default subscriptions, but only when the table is empty.
    ///
    /// Idempotent across restarts: once the user has any subscription (even a
    /// disabled one), defaults are never re-inserted. Returns `true` if seeding
    /// happened.
    pub fn seed_defaults_if_empty(&self, defaults: &[Subscription]) -> Result<bool> {
        {
            let conn = self.conn.lock().unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM subscriptions", [], |r| r.get(0))
                .map_err(|e| Error::Storage(e.to_string()))?;
            if count > 0 {
                return Ok(false);
            }
        }
        for sub in defaults {
            self.set_subscription(&sub.url, &sub.title, sub.enabled)?;
        }
        Ok(true)
    }

    // ── list_meta ──────────────────────────────────────────────────────────

    /// Fetch cache metadata for a list slug, if present.
    pub fn get_meta(&self, slug: &str) -> Result<Option<ListMeta>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT slug, url, etag, last_modified, fetched_at, rule_count, content_hash
                 FROM list_meta WHERE slug = ?1",
                params![slug],
                |row| {
                    Ok(ListMeta {
                        slug: row.get(0)?,
                        url: row.get(1)?,
                        etag: row.get(2)?,
                        last_modified: row.get(3)?,
                        fetched_at: row.get(4)?,
                        rule_count: row.get(5)?,
                        content_hash: row.get(6)?,
                    })
                },
            )
            .ok();
        Ok(row)
    }

    /// Insert or replace cache metadata for a list (keyed by slug).
    pub fn upsert_meta(&self, meta: &ListMeta) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO list_meta
                (slug, url, etag, last_modified, fetched_at, rule_count, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                meta.slug,
                meta.url,
                meta.etag,
                meta.last_modified,
                meta.fetched_at,
                meta.rule_count,
                meta.content_hash,
            ],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(url: &str, title: &str, enabled: bool) -> Subscription {
        Subscription {
            url: url.to_owned(),
            title: title.to_owned(),
            enabled,
        }
    }

    #[test]
    fn empty_store_has_no_subscriptions() {
        let db = AdblockStore::open_in_memory().unwrap();
        assert!(db.list_subscriptions().unwrap().is_empty());
    }

    #[test]
    fn seed_defaults_populates_empty_table() {
        let db = AdblockStore::open_in_memory().unwrap();
        let defaults = vec![
            sub("https://easylist.to/easylist/easylist.txt", "EasyList", true),
            sub("https://easylist.to/easylist/easyprivacy.txt", "EasyPrivacy", true),
        ];
        assert!(db.seed_defaults_if_empty(&defaults).unwrap());
        let subs = db.list_subscriptions().unwrap();
        assert_eq!(subs.len(), 2);
        // Ordered by title: EasyList before EasyPrivacy.
        assert_eq!(subs[0].title, "EasyList");
        assert!(subs[0].enabled);
    }

    #[test]
    fn seed_defaults_is_noop_when_non_empty() {
        let db = AdblockStore::open_in_memory().unwrap();
        db.set_subscription("https://custom.example/list.txt", "Custom", true)
            .unwrap();
        let defaults = vec![sub("https://easylist.to/easylist.txt", "EasyList", true)];
        assert!(!db.seed_defaults_if_empty(&defaults).unwrap());
        assert_eq!(db.list_subscriptions().unwrap().len(), 1);
    }

    #[test]
    fn set_subscription_upserts() {
        let db = AdblockStore::open_in_memory().unwrap();
        db.set_subscription("https://a.example/l.txt", "A", true)
            .unwrap();
        // Update title + disable.
        db.set_subscription("https://a.example/l.txt", "A renamed", false)
            .unwrap();
        let subs = db.list_subscriptions().unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].title, "A renamed");
        assert!(!subs[0].enabled);
    }

    #[test]
    fn get_meta_absent_returns_none() {
        let db = AdblockStore::open_in_memory().unwrap();
        assert!(db.get_meta("easylist").unwrap().is_none());
    }

    #[test]
    fn upsert_and_get_meta_roundtrip() {
        let db = AdblockStore::open_in_memory().unwrap();
        let meta = ListMeta {
            slug: "easylist".to_owned(),
            url: "https://easylist.to/easylist/easylist.txt".to_owned(),
            etag: Some("\"abc123\"".to_owned()),
            last_modified: Some("Mon, 01 Jan 2026 00:00:00 GMT".to_owned()),
            fetched_at: 1_700_000_000,
            rule_count: 42_000,
            content_hash: Some("deadbeef".to_owned()),
        };
        db.upsert_meta(&meta).unwrap();
        let got = db.get_meta("easylist").unwrap().unwrap();
        assert_eq!(got, meta);
    }

    #[test]
    fn upsert_meta_replaces_on_same_slug() {
        let db = AdblockStore::open_in_memory().unwrap();
        let mut meta = ListMeta {
            slug: "easylist".to_owned(),
            url: "https://easylist.to/easylist.txt".to_owned(),
            etag: None,
            last_modified: None,
            fetched_at: 1,
            rule_count: 1,
            content_hash: None,
        };
        db.upsert_meta(&meta).unwrap();
        meta.fetched_at = 2;
        meta.rule_count = 99;
        meta.etag = Some("\"v2\"".to_owned());
        db.upsert_meta(&meta).unwrap();
        let got = db.get_meta("easylist").unwrap().unwrap();
        assert_eq!(got.fetched_at, 2);
        assert_eq!(got.rule_count, 99);
        assert_eq!(got.etag.as_deref(), Some("\"v2\""));
    }
}
