//! §12.3 Read-later / офлайн-чтение.
//!
//! Сохранённые страницы хранятся как:
//! - **HTML-snapshot** (BLOB) — полная страница в self-contained форме
//!   (data-URI inline-images / inline-styles); рендерится локально без сети;
//! - **Extracted text** — readability-extract, идёт во FTS5-индекс §12.1
//!   через `ReadLaterFts` (отдельная FTS5-таблица; общая БД-файл с
//!   историей даёт сквозной поиск по обоим источникам).
//!
//! Phase 0 покрывает storage layer: schema, CRUD, FTS, статусы
//! (unread / read / archived), теги. Сам readability-extract,
//! загрузка ресурсов при save, и UI — отдельные задачи.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Статус read-later записи.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadStatus {
    /// Только что сохранена, не читалась.
    #[default]
    Unread,
    /// Прочитана пользователем (пометка явно или scroll-to-bottom).
    Read,
    /// Заархивирована (скрыта из active list, но не удалена).
    Archived,
}

impl ReadStatus {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Read => "read",
            Self::Archived => "archived",
        }
    }

    fn from_db_str(s: &str) -> Self {
        match s {
            "read" => Self::Read,
            "archived" => Self::Archived,
            _ => Self::Unread,
        }
    }
}

/// Одна сохранённая страница.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadLaterEntry {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// HTML-snapshot для офлайн-просмотра.
    pub html_snapshot: Vec<u8>,
    /// Plain-text extract (для FTS и preview).
    pub text: String,
    pub status: ReadStatus,
    pub saved_at: i64,
    /// Последний раз открыта (для LRU eviction).
    pub last_accessed: Option<i64>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadLaterSearchHit {
    pub entry: ReadLaterEntry,
    pub snippet: String,
    pub score: f64,
}

pub struct ReadLater {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for ReadLater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadLater").finish()
    }
}

impl ReadLater {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("read_later open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("read_later open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS read_later (
                id            INTEGER PRIMARY KEY,
                url           TEXT NOT NULL UNIQUE,
                title         TEXT NOT NULL DEFAULT '',
                html_snapshot BLOB NOT NULL,
                text          TEXT NOT NULL DEFAULT '',
                status        TEXT NOT NULL DEFAULT 'unread',
                saved_at      INTEGER NOT NULL,
                last_accessed INTEGER
            );
            CREATE TABLE IF NOT EXISTS read_later_tags (
                entry_id INTEGER NOT NULL,
                tag      TEXT NOT NULL,
                PRIMARY KEY (entry_id, tag),
                FOREIGN KEY (entry_id) REFERENCES read_later(id) ON DELETE CASCADE
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS read_later_status_idx ON read_later(status);
            CREATE INDEX IF NOT EXISTS read_later_saved_idx ON read_later(saved_at DESC);
            CREATE INDEX IF NOT EXISTS read_later_tag_idx ON read_later_tags(tag);
            -- External content FTS5: индекс над title + text. Sync через triggers.
            CREATE VIRTUAL TABLE IF NOT EXISTS read_later_fts USING fts5(
                title, text,
                content = 'read_later',
                content_rowid = 'id',
                tokenize = 'unicode61 remove_diacritics 2'
            );
            CREATE TRIGGER IF NOT EXISTS read_later_ai AFTER INSERT ON read_later BEGIN
                INSERT INTO read_later_fts (rowid, title, text)
                VALUES (new.id, new.title, new.text);
            END;
            CREATE TRIGGER IF NOT EXISTS read_later_ad AFTER DELETE ON read_later BEGIN
                INSERT INTO read_later_fts (read_later_fts, rowid, title, text)
                VALUES ('delete', old.id, old.title, old.text);
            END;
            CREATE TRIGGER IF NOT EXISTS read_later_au AFTER UPDATE ON read_later BEGIN
                INSERT INTO read_later_fts (read_later_fts, rowid, title, text)
                VALUES ('delete', old.id, old.title, old.text);
                INSERT INTO read_later_fts (rowid, title, text)
                VALUES (new.id, new.title, new.text);
            END;
            "#,
        )
        .map_err(|e| Error::Storage(format!("read_later init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Сохранить новую страницу или обновить существующую. Возвращает id.
    pub fn save(
        &self,
        url: &str,
        title: &str,
        html_snapshot: &[u8],
        text: &str,
        tags: &[String],
        saved_at: i64,
    ) -> Result<i64> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::Storage(format!("read_later tx: {e}")))?;
        tx.execute(
            "INSERT INTO read_later (url, title, html_snapshot, text, saved_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (url) DO UPDATE SET
                 title = excluded.title,
                 html_snapshot = excluded.html_snapshot,
                 text = excluded.text",
            params![url, title, html_snapshot, text, saved_at],
        )
        .map_err(|e| Error::Storage(format!("read_later save upsert: {e}")))?;
        let id: i64 = tx
            .query_row("SELECT id FROM read_later WHERE url = ?1", params![url], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("read_later save lookup-id: {e}")))?;
        // Перезаписываем теги.
        tx.execute(
            "DELETE FROM read_later_tags WHERE entry_id = ?1",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("read_later save delete-tags: {e}")))?;
        {
            let mut stmt = tx
                .prepare("INSERT INTO read_later_tags (entry_id, tag) VALUES (?1, ?2)")
                .map_err(|e| Error::Storage(format!("read_later save prepare-tag: {e}")))?;
            let mut seen: HashSet<&str> = HashSet::new();
            for t in tags {
                if seen.insert(t.as_str()) {
                    stmt.execute(params![id, t])
                        .map_err(|e| Error::Storage(format!("read_later save tag: {e}")))?;
                }
            }
        }
        tx.commit()
            .map_err(|e| Error::Storage(format!("read_later commit: {e}")))?;
        Ok(id)
    }

    /// Обновить статус записи (mark read / archive).
    pub fn set_status(&self, id: i64, status: ReadStatus) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        conn.execute(
            "UPDATE read_later SET status = ?1 WHERE id = ?2",
            params![status.as_db_str(), id],
        )
        .map_err(|e| Error::Storage(format!("read_later set_status: {e}")))?;
        Ok(())
    }

    /// Обновить last_accessed (вызывается при открытии офлайн-копии).
    pub fn touch(&self, id: i64, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        conn.execute(
            "UPDATE read_later SET last_accessed = ?1 WHERE id = ?2",
            params![now_unix, id],
        )
        .map_err(|e| Error::Storage(format!("read_later touch: {e}")))?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<ReadLaterEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT id, url, title, html_snapshot, text, status, saved_at, last_accessed
                 FROM read_later WHERE id = ?1",
                params![id],
                row_to_entry,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("read_later get: {e}")))?;
        let Some(mut e) = row else { return Ok(None) };
        e.tags = fetch_tags(&conn, e.id)?;
        Ok(Some(e))
    }

    pub fn get_by_url(&self, url: &str) -> Result<Option<ReadLaterEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT id, url, title, html_snapshot, text, status, saved_at, last_accessed
                 FROM read_later WHERE url = ?1",
                params![url],
                row_to_entry,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("read_later get_by_url: {e}")))?;
        let Some(mut e) = row else { return Ok(None) };
        e.tags = fetch_tags(&conn, e.id)?;
        Ok(Some(e))
    }

    /// Список записей с указанным статусом, сортировка по saved_at DESC.
    pub fn list_by_status(&self, status: ReadStatus, limit: i64) -> Result<Vec<ReadLaterEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, title, html_snapshot, text, status, saved_at, last_accessed
                 FROM read_later WHERE status = ?1 ORDER BY saved_at DESC LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("read_later list prepare: {e}")))?;
        let rows = stmt
            .query_map(params![status.as_db_str(), limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("read_later list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            let mut e = r.map_err(|e| Error::Storage(format!("read_later row: {e}")))?;
            e.tags = fetch_tags(&conn, e.id)?;
            out.push(e);
        }
        Ok(out)
    }

    /// Полнотекстовый поиск.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<ReadLaterSearchHit>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.url, r.title, r.html_snapshot, r.text, r.status, r.saved_at, r.last_accessed,
                        snippet(read_later_fts, 1, '**', '**', '…', 32) AS snip,
                        bm25(read_later_fts) AS score
                 FROM read_later_fts
                 JOIN read_later r ON r.id = read_later_fts.rowid
                 WHERE read_later_fts MATCH ?1
                 ORDER BY bm25(read_later_fts)
                 LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("read_later search prepare: {e}")))?;
        let rows = stmt
            .query_map(params![query, limit], |row| {
                let mut e = ReadLaterEntry {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    html_snapshot: row.get(3)?,
                    text: row.get(4)?,
                    status: ReadStatus::from_db_str(&row.get::<_, String>(5)?),
                    saved_at: row.get(6)?,
                    last_accessed: row.get(7)?,
                    tags: Vec::new(),
                };
                // tags подгрузим post-rows.
                e.tags = Vec::new();
                Ok((
                    ReadLaterSearchHit {
                        entry: e,
                        snippet: row.get(8)?,
                        score: row.get(9)?,
                    },
                ))
            })
            .map_err(|e| Error::Storage(format!("read_later search query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            let (mut hit,) = r.map_err(|e| Error::Storage(format!("read_later search row: {e}")))?;
            hit.entry.tags = fetch_tags(&conn, hit.entry.id)?;
            out.push(hit);
        }
        Ok(out)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        conn.execute("DELETE FROM read_later WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("read_later delete: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("read_later mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM read_later", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("read_later count: {e}")))?;
        Ok(n)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReadLaterEntry> {
    Ok(ReadLaterEntry {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        html_snapshot: row.get(3)?,
        text: row.get(4)?,
        status: ReadStatus::from_db_str(&row.get::<_, String>(5)?),
        saved_at: row.get(6)?,
        last_accessed: row.get(7)?,
        tags: Vec::new(),
    })
}

fn fetch_tags(conn: &Connection, entry_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT tag FROM read_later_tags WHERE entry_id = ?1 ORDER BY tag",
        )
        .map_err(|e| Error::Storage(format!("read_later fetch_tags prepare: {e}")))?;
    let rows = stmt
        .query_map(params![entry_id], |r| r.get::<_, String>(0))
        .map_err(|e| Error::Storage(format!("read_later fetch_tags query: {e}")))?;
    let mut tags = Vec::new();
    for r in rows {
        tags.push(r.map_err(|e| Error::Storage(format!("read_later tag row: {e}")))?);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> ReadLater {
        ReadLater::open_in_memory().unwrap()
    }

    fn save_basic(r: &ReadLater, url: &str, title: &str, text: &str, t: i64) -> i64 {
        r.save(url, title, b"<html>...</html>", text, &[], t).unwrap()
    }

    #[test]
    fn save_and_get_basic() {
        let r = make();
        let id = r
            .save(
                "https://example.com/article",
                "Article",
                b"<html>content</html>",
                "Article body text",
                &["tech".to_string(), "interesting".to_string()],
                100,
            )
            .unwrap();
        let got = r.get(id).unwrap().unwrap();
        assert_eq!(got.title, "Article");
        assert_eq!(got.text, "Article body text");
        assert_eq!(got.status, ReadStatus::Unread);
        assert_eq!(got.tags, vec!["interesting".to_string(), "tech".to_string()]);
        assert_eq!(got.last_accessed, None);
    }

    #[test]
    fn save_duplicate_url_updates_in_place() {
        let r = make();
        let id1 = r.save("https://x/", "Old", b"v1", "old text", &[], 100).unwrap();
        let id2 = r.save("https://x/", "New", b"v2", "new text", &[], 200).unwrap();
        assert_eq!(id1, id2);  // same row
        let got = r.get(id1).unwrap().unwrap();
        assert_eq!(got.title, "New");
        assert_eq!(got.html_snapshot, b"v2");
        assert_eq!(got.text, "new text");
        // saved_at не должен меняться при ON CONFLICT.
        assert_eq!(got.saved_at, 100);
        assert_eq!(r.count().unwrap(), 1);
    }

    #[test]
    fn set_status_changes_state() {
        let r = make();
        let id = save_basic(&r, "https://x/", "t", "content", 100);
        r.set_status(id, ReadStatus::Read).unwrap();
        assert_eq!(r.get(id).unwrap().unwrap().status, ReadStatus::Read);
        r.set_status(id, ReadStatus::Archived).unwrap();
        assert_eq!(r.get(id).unwrap().unwrap().status, ReadStatus::Archived);
    }

    #[test]
    fn touch_sets_last_accessed() {
        let r = make();
        let id = save_basic(&r, "https://x/", "t", "x", 100);
        r.touch(id, 500).unwrap();
        assert_eq!(r.get(id).unwrap().unwrap().last_accessed, Some(500));
    }

    #[test]
    fn list_by_status_filters_correctly() {
        let r = make();
        let id1 = save_basic(&r, "https://a/", "A", "x", 100);
        let id2 = save_basic(&r, "https://b/", "B", "x", 200);
        let id3 = save_basic(&r, "https://c/", "C", "x", 300);
        r.set_status(id2, ReadStatus::Read).unwrap();
        r.set_status(id3, ReadStatus::Archived).unwrap();
        let unread = r.list_by_status(ReadStatus::Unread, 10).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, id1);

        let read = r.list_by_status(ReadStatus::Read, 10).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, id2);

        let archived = r.list_by_status(ReadStatus::Archived, 10).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, id3);
    }

    #[test]
    fn list_by_status_orders_desc_by_saved_at() {
        let r = make();
        save_basic(&r, "https://old/", "old", "x", 100);
        save_basic(&r, "https://new/", "new", "x", 300);
        save_basic(&r, "https://mid/", "mid", "x", 200);
        let unread = r.list_by_status(ReadStatus::Unread, 10).unwrap();
        let titles: Vec<&str> = unread.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["new", "mid", "old"]);
    }

    #[test]
    fn search_finds_by_text() {
        let r = make();
        save_basic(&r, "https://x/", "Rust", "Rust is awesome", 100);
        save_basic(&r, "https://y/", "Python", "Python is fine", 200);
        let hits = r.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.title, "Rust");
    }

    #[test]
    fn search_finds_by_title() {
        let r = make();
        save_basic(&r, "https://x/", "AwesomeBlog", "body text", 100);
        let hits = r.search("AwesomeBlog", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn search_loads_tags() {
        let r = make();
        r.save("https://x/", "t", b"", "rust async", &["tech".to_string()], 100)
            .unwrap();
        let hits = r.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.tags, vec!["tech".to_string()]);
    }

    #[test]
    fn fts_synced_after_delete() {
        let r = make();
        // Без дефисов и colon-ов — это FTS5 операторы (NOT, column-qualifier).
        let id = save_basic(&r, "https://x/", "t", "uniqueterm", 100);
        assert_eq!(r.search("uniqueterm", 10).unwrap().len(), 1);
        r.delete(id).unwrap();
        assert!(r.search("uniqueterm", 10).unwrap().is_empty());
    }

    #[test]
    fn delete_cascades_to_tags() {
        let r = make();
        let id = r
            .save(
                "https://x/",
                "t",
                b"",
                "x",
                &["a".to_string(), "b".to_string()],
                100,
            )
            .unwrap();
        r.delete(id).unwrap();
        assert!(r.get(id).unwrap().is_none());
        // Поскольку всё подчищено CASCADE-ом, перевывшая запись не должна вернуть тегов.
        let id2 = r.save("https://x/", "t2", b"", "y", &[], 200).unwrap();
        let got = r.get(id2).unwrap().unwrap();
        assert!(got.tags.is_empty());
    }

    #[test]
    fn cyrillic_save_search() {
        let r = make();
        save_basic(
            &r,
            "https://пример.рф/",
            "Главная статья",
            "Это статья про русский язык",
            100,
        );
        let hits = r.search("русский", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.title, "Главная статья");
    }

    #[test]
    fn count_total() {
        let r = make();
        assert_eq!(r.count().unwrap(), 0);
        save_basic(&r, "https://a/", "A", "x", 100);
        save_basic(&r, "https://b/", "B", "x", 200);
        assert_eq!(r.count().unwrap(), 2);
    }

    #[test]
    fn get_missing_returns_none() {
        let r = make();
        assert!(r.get(999).unwrap().is_none());
        assert!(r.get_by_url("https://nope/").unwrap().is_none());
    }

    #[test]
    fn html_snapshot_binary_preserved() {
        let r = make();
        let blob: Vec<u8> = (0..=255u8).collect();
        let id = r.save("https://x/", "t", &blob, "text", &[], 100).unwrap();
        let got = r.get(id).unwrap().unwrap();
        assert_eq!(got.html_snapshot, blob);
    }
}
