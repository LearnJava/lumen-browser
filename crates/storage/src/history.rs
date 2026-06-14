//! История посещённых страниц поверх SQLite. Закладка под §12.1
//! «Полнотекстовый поиск по истории» — этот модуль реализует таблицу
//! `history` + базовый visit-tracking. FTS5-индекс над `text` живёт
//! в отдельном крейте `lumen-knowledge` (или встанет рядом отдельным
//! модулем) — здесь только основа.
//!
//! Схема:
//! ```sql
//! CREATE TABLE history (
//!     id            INTEGER PRIMARY KEY,
//!     url           TEXT NOT NULL,
//!     title         TEXT NOT NULL DEFAULT '',
//!     visit_date    INTEGER NOT NULL,  -- Unix timestamp
//!     visit_count   INTEGER NOT NULL DEFAULT 1,
//!     favicon_hash  BLOB,              -- sha256, NULL если нет
//!     text_sha256   BLOB               -- sha256 от readability-extract
//! );
//! ```
//!
//! `url` — primary lookup key (через индекс по url). Повторный визит
//! инкрементит `visit_count` и обновляет `visit_date` (last-visit
//! semantics). История — глобальная per-profile (не партиционируется
//! по origin), потому что это пользовательская временная линия,
//! не приватный storage.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Запись истории. Возвращается при чтении / поиске.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub visit_date: i64,
    pub visit_count: i64,
    pub favicon_hash: Option<Vec<u8>>,
    pub text_sha256: Option<Vec<u8>>,
}

/// История пользователя.
pub struct History {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for History {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("History").finish()
    }
}

impl History {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("history open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("history open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS history (
                id           INTEGER PRIMARY KEY,
                url          TEXT NOT NULL UNIQUE,
                title        TEXT NOT NULL DEFAULT '',
                visit_date   INTEGER NOT NULL,
                visit_count  INTEGER NOT NULL DEFAULT 1,
                favicon_hash BLOB,
                text_sha256  BLOB
            );
            CREATE INDEX IF NOT EXISTS history_visit_date_idx
                ON history (visit_date DESC);
            "#,
        )
        .map_err(|e| Error::Storage(format!("history init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Зафиксировать визит. Если url уже встречался — обновляем title /
    /// visit_date и инкрементируем visit_count; иначе вставляем новую
    /// строку с visit_count=1.
    ///
    /// `visit_date` — Unix timestamp в секундах. `title` — title из
    /// `<title>` страницы (или пустая строка, если нет).
    pub fn record_visit(&self, url: &str, title: &str, visit_date: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        // UPSERT: insert или update (visit_count += 1, visit_date := newer).
        // Title обновляем только если новый непустой (старый мог быть
        // получше; либо первая запись без title-парсинга).
        conn.execute(
            "INSERT INTO history (url, title, visit_date, visit_count)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT (url) DO UPDATE SET
                 title = CASE WHEN excluded.title = '' THEN title ELSE excluded.title END,
                 visit_date = MAX(history.visit_date, excluded.visit_date),
                 visit_count = history.visit_count + 1",
            params![url, title, visit_date],
        )
        .map_err(|e| Error::Storage(format!("history record_visit: {e}")))?;
        Ok(())
    }

    /// Установить favicon-hash для url. Никак не аффектит visit_count.
    pub fn set_favicon(&self, url: &str, favicon_hash: &[u8]) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        conn.execute(
            "UPDATE history SET favicon_hash = ?1 WHERE url = ?2",
            params![favicon_hash, url],
        )
        .map_err(|e| Error::Storage(format!("history set_favicon: {e}")))?;
        Ok(())
    }

    /// Установить text_sha256 (для дедупликации readability-content).
    pub fn set_text_sha256(&self, url: &str, sha: &[u8]) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        conn.execute(
            "UPDATE history SET text_sha256 = ?1 WHERE url = ?2",
            params![sha, url],
        )
        .map_err(|e| Error::Storage(format!("history set_text_sha256: {e}")))?;
        Ok(())
    }

    /// Найти запись по URL.
    pub fn get(&self, url: &str) -> Result<Option<HistoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        let entry = conn
            .query_row(
                "SELECT id, url, title, visit_date, visit_count, favicon_hash, text_sha256
                 FROM history WHERE url = ?1",
                params![url],
                row_to_entry,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("history get: {e}")))?;
        Ok(entry)
    }

    /// Последние N записей (по убыванию visit_date).
    pub fn recent(&self, limit: i64) -> Result<Vec<HistoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, title, visit_date, visit_count, favicon_hash, text_sha256
                 FROM history ORDER BY visit_date DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("history prepare recent: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("history query recent: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("history row: {e}")))?);
        }
        Ok(out)
    }

    /// Топ-N записей по visit_count. Удобно для new-tab «most visited».
    pub fn most_visited(&self, limit: i64) -> Result<Vec<HistoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, title, visit_date, visit_count, favicon_hash, text_sha256
                 FROM history ORDER BY visit_count DESC, visit_date DESC LIMIT ?1",
            )
            .map_err(|e| {
                Error::Storage(format!("history prepare most_visited: {e}"))
            })?;
        let rows = stmt
            .query_map(params![limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("history query most_visited: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("history row: {e}")))?);
        }
        Ok(out)
    }

    /// Поиск по url и title: case-insensitive substring match.
    ///
    /// Возвращает до `limit` записей, отсортированных по `visit_count DESC`,
    /// `visit_date DESC`. Пустой `q` немедленно возвращает пустой список —
    /// используй [`recent`] или [`most_visited`] вместо этого.
    ///
    /// Предназначен для омнибокс-автодополнения: при вводе URL-фрагмента
    /// (`"rust-lang"`, `"https://crates"`) пользователь получает совпадающие
    /// посещённые страницы без полнотекстового FTS5-индекса.
    pub fn search_prefix(&self, q: &str, limit: i64) -> Result<Vec<HistoryEntry>> {
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        // Экранируем LIKE-спецсимволы, затем оборачиваем в %...%.
        let escaped = q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, title, visit_date, visit_count, favicon_hash, text_sha256
                 FROM history
                 WHERE url LIKE ?1 ESCAPE '\\'
                    OR lower(title) LIKE lower(?1) ESCAPE '\\'
                 ORDER BY visit_count DESC, visit_date DESC
                 LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("history prepare search_prefix: {e}")))?;
        let rows = stmt
            .query_map(params![pattern, limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("history query search_prefix: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(
                r.map_err(|e| Error::Storage(format!("history row search_prefix: {e}")))?,
            );
        }
        Ok(out)
    }

    /// Удалить запись по url. Никаких ошибок, если url не существует.
    pub fn delete(&self, url: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        conn.execute("DELETE FROM history WHERE url = ?1", params![url])
            .map_err(|e| Error::Storage(format!("history delete: {e}")))?;
        Ok(())
    }

    /// Удалить все записи с `visit_date < before`. Возвращает число
    /// удалённых строк. Для quota-managed eviction старых записей.
    pub fn delete_older_than(&self, before: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        let count = conn
            .execute(
                "DELETE FROM history WHERE visit_date < ?1",
                params![before],
            )
            .map_err(|e| Error::Storage(format!("history delete_older_than: {e}")))?;
        Ok(count)
    }

    /// Полная очистка истории.
    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("history mutex poisoned".into()))?;
        conn.execute("DELETE FROM history", [])
            .map_err(|e| Error::Storage(format!("history clear: {e}")))?;
        Ok(())
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        visit_date: row.get(3)?,
        visit_count: row.get(4)?,
        favicon_hash: row.get(5)?,
        text_sha256: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> History {
        History::open_in_memory().unwrap()
    }

    #[test]
    fn record_visit_creates_new_entry() {
        let h = make();
        h.record_visit("https://example.com/", "Example", 100).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.url, "https://example.com/");
        assert_eq!(e.title, "Example");
        assert_eq!(e.visit_date, 100);
        assert_eq!(e.visit_count, 1);
    }

    #[test]
    fn record_visit_increments_existing() {
        let h = make();
        h.record_visit("https://example.com/", "Example", 100).unwrap();
        h.record_visit("https://example.com/", "Example", 200).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.visit_count, 2);
        // visit_date = max(old, new) = 200.
        assert_eq!(e.visit_date, 200);
    }

    #[test]
    fn record_visit_max_keeps_latest_date() {
        let h = make();
        h.record_visit("https://example.com/", "Example", 200).unwrap();
        // Старый визит — visit_date НЕ должен затирать новый.
        h.record_visit("https://example.com/", "Example", 100).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.visit_date, 200);
        assert_eq!(e.visit_count, 2);
    }

    #[test]
    fn record_visit_empty_title_preserves_existing() {
        let h = make();
        h.record_visit("https://example.com/", "Good Title", 100).unwrap();
        // Второй визит с пустым title — не должен затирать.
        h.record_visit("https://example.com/", "", 200).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.title, "Good Title");
    }

    #[test]
    fn record_visit_nonempty_title_overwrites() {
        let h = make();
        h.record_visit("https://example.com/", "Old", 100).unwrap();
        h.record_visit("https://example.com/", "New", 200).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.title, "New");
    }

    #[test]
    fn get_missing_returns_none() {
        let h = make();
        assert!(h.get("https://nope.com/").unwrap().is_none());
    }

    #[test]
    fn set_favicon_persists() {
        let h = make();
        h.record_visit("https://example.com/", "x", 100).unwrap();
        h.set_favicon("https://example.com/", &[0xAB, 0xCD]).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.favicon_hash, Some(vec![0xAB, 0xCD]));
    }

    #[test]
    fn set_text_sha256_persists() {
        let h = make();
        h.record_visit("https://example.com/", "x", 100).unwrap();
        let sha: Vec<u8> = (0..32).collect();
        h.set_text_sha256("https://example.com/", &sha).unwrap();
        let e = h.get("https://example.com/").unwrap().unwrap();
        assert_eq!(e.text_sha256, Some(sha));
    }

    #[test]
    fn recent_returns_descending_by_visit_date() {
        let h = make();
        h.record_visit("https://a.com/", "A", 100).unwrap();
        h.record_visit("https://b.com/", "B", 300).unwrap();
        h.record_visit("https://c.com/", "C", 200).unwrap();
        let r = h.recent(10).unwrap();
        assert_eq!(r.len(), 3);
        // Order: b (300), c (200), a (100).
        assert_eq!(r[0].url, "https://b.com/");
        assert_eq!(r[1].url, "https://c.com/");
        assert_eq!(r[2].url, "https://a.com/");
    }

    #[test]
    fn recent_respects_limit() {
        let h = make();
        for i in 0..5 {
            h.record_visit(&format!("https://e{i}.com/"), "x", i).unwrap();
        }
        assert_eq!(h.recent(3).unwrap().len(), 3);
    }

    #[test]
    fn most_visited_descending_by_count() {
        let h = make();
        h.record_visit("https://a.com/", "A", 100).unwrap();
        h.record_visit("https://b.com/", "B", 100).unwrap();
        h.record_visit("https://b.com/", "B", 200).unwrap();
        h.record_visit("https://b.com/", "B", 300).unwrap();
        let mv = h.most_visited(10).unwrap();
        assert_eq!(mv[0].url, "https://b.com/");
        assert_eq!(mv[0].visit_count, 3);
        assert_eq!(mv[1].url, "https://a.com/");
        assert_eq!(mv[1].visit_count, 1);
    }

    #[test]
    fn delete_removes_entry() {
        let h = make();
        h.record_visit("https://example.com/", "x", 100).unwrap();
        h.delete("https://example.com/").unwrap();
        assert!(h.get("https://example.com/").unwrap().is_none());
    }

    #[test]
    fn delete_missing_noop() {
        let h = make();
        h.delete("https://nope.com/").unwrap();
    }

    #[test]
    fn delete_older_than_removes_old_only() {
        let h = make();
        h.record_visit("https://old1.com/", "x", 100).unwrap();
        h.record_visit("https://old2.com/", "x", 200).unwrap();
        h.record_visit("https://new.com/", "x", 1000).unwrap();
        let removed = h.delete_older_than(500).unwrap();
        assert_eq!(removed, 2);
        assert!(h.get("https://new.com/").unwrap().is_some());
        assert!(h.get("https://old1.com/").unwrap().is_none());
    }

    #[test]
    fn clear_wipes_all() {
        let h = make();
        h.record_visit("https://a.com/", "x", 100).unwrap();
        h.record_visit("https://b.com/", "x", 200).unwrap();
        h.clear().unwrap();
        assert!(h.recent(10).unwrap().is_empty());
    }

    #[test]
    fn cyrillic_url_and_title_preserved() {
        let h = make();
        let url = "https://пример.рф/страница";
        h.record_visit(url, "Главная страница", 100).unwrap();
        let e = h.get(url).unwrap().unwrap();
        assert_eq!(e.url, url);
        assert_eq!(e.title, "Главная страница");
    }

    // ── search_prefix tests ──────────────────────────────────────────────────

    #[test]
    fn search_prefix_matches_url_substring() {
        let h = make();
        h.record_visit("https://rust-lang.org/", "Rust", 100).unwrap();
        h.record_visit("https://crates.io/", "crates.io", 100).unwrap();
        let results = h.search_prefix("rust-lang", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://rust-lang.org/");
    }

    #[test]
    fn search_prefix_matches_title_case_insensitive() {
        let h = make();
        h.record_visit("https://example.com/", "The Rust Book", 100).unwrap();
        h.record_visit("https://other.com/", "Python Docs", 100).unwrap();
        let results = h.search_prefix("rust book", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/");
    }

    #[test]
    fn search_prefix_empty_query_returns_empty() {
        let h = make();
        h.record_visit("https://example.com/", "Example", 100).unwrap();
        let results = h.search_prefix("", 5).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_prefix_ordered_by_visit_count() {
        let h = make();
        h.record_visit("https://rust-lang.org/", "Rust", 100).unwrap();
        h.record_visit("https://rust-lang.org/book/", "Rust Book", 100).unwrap();
        h.record_visit("https://rust-lang.org/book/", "Rust Book", 200).unwrap();
        h.record_visit("https://rust-lang.org/book/", "Rust Book", 300).unwrap();
        let results = h.search_prefix("rust-lang", 5).unwrap();
        assert_eq!(results.len(), 2);
        // book has visit_count=3, root has visit_count=1.
        assert_eq!(results[0].url, "https://rust-lang.org/book/");
        assert_eq!(results[0].visit_count, 3);
    }

    #[test]
    fn search_prefix_respects_limit() {
        let h = make();
        for i in 0..10 {
            h.record_visit(
                &format!("https://rust{i}.example.com/"),
                "Rust Crate",
                i * 100,
            )
            .unwrap();
        }
        let results = h.search_prefix("rust", 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn search_prefix_no_match_returns_empty() {
        let h = make();
        h.record_visit("https://example.com/", "Example", 100).unwrap();
        let results = h.search_prefix("zzz_not_found_xyz", 5).unwrap();
        assert!(results.is_empty());
    }
}
