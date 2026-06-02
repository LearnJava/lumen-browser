//! §12.4 Поиск по содержимому открытых вкладок.
//!
//! Живой in-memory FTS5-индекс над текстом *открытых* вкладок. В отличие
//! от [`crate::fts::HistoryFts`] (зеркало истории на диске, ключ — `History.id`)
//! этот индекс:
//!
//! * **никогда не пишется на диск** — содержит только сейчас открытые
//!   вкладки; закрытие приложения = пустой индекс при следующем старте;
//! * **ключ — `tab_id`** (живой идентификатор вкладки в shell), а не
//!   стабильный rowid истории;
//! * **переиндексируется на лету** — при навигации/перерисовке shell зовёт
//!   [`OpenTabsIndex::index_tab`] с новым текстом, при закрытии вкладки —
//!   [`OpenTabsIndex::remove_tab`].
//!
//! Назначение — мгновенный «найти среди открытых вкладок» (omnibox-команда
//! `@tabs <query>` / Ctrl+Shift+A switcher): пользователь помнит, что
//! «где-то была открыта статья про X», но не помнит в какой вкладке.
//!
//! Схема — три FTS5-колонки `(url, title, text)`, токенайзер `unicode61
//! remove_diacritics 2` (как у [`HistoryFts`]); ранжирование `bm25`.
//!
//! Shell-wiring (handoff P3): держать один `OpenTabsIndex` на процесс,
//! звать `index_tab(tab_id, url, title, extracted_text)` после загрузки/
//! навигации каждой вкладки и `remove_tab(tab_id)` при её закрытии;
//! `search(query, n)` — для omnibox `@tabs`.
//!
//! [`HistoryFts`]: crate::fts::HistoryFts

use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

/// Результат поиска по открытым вкладкам.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenTabHit {
    /// Идентификатор открытой вкладки (живой shell tab id). Хранится как
    /// rowid FTS5-таблицы.
    pub tab_id: i64,
    /// URL вкладки на момент последней индексации.
    pub url: String,
    /// Заголовок вкладки на момент последней индексации.
    pub title: String,
    /// Сниппет вокруг первого матча в `text`, с markdown-подсветкой
    /// `**...**` совпавших слов (до ~32 токенов).
    pub snippet: String,
    /// BM25-score; меньше = релевантнее (negated per FTS5).
    pub score: f64,
}

/// Живой in-memory FTS5-индекс над открытыми вкладками. Не персистится —
/// существует только в памяти на время жизни процесса. Потокобезопасен
/// (внутренний `Mutex<Connection>`).
pub struct OpenTabsIndex {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for OpenTabsIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenTabsIndex").finish()
    }
}

impl OpenTabsIndex {
    /// Создать пустой in-memory индекс. По дизайну (§12.4) on-disk варианта
    /// нет: индекс открытых вкладок не переживает рестарт.
    pub fn new() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("open_tabs open_in_memory: {e}")))?;
        conn.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS open_tabs_fts USING fts5(
                url, title, text,
                tokenize = 'unicode61 remove_diacritics 2'
            );
            "#,
        )
        .map_err(|e| Error::Storage(format!("open_tabs init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Добавить или обновить вкладку в индексе. `tab_id` — живой shell tab id;
    /// при повторном вызове с тем же `tab_id` все три колонки перезаписываются
    /// (FTS5 не поддерживает UPSERT — DELETE+INSERT в одной транзакции).
    /// `text` — извлечённый видимый текст страницы.
    pub fn index_tab(&self, tab_id: i64, url: &str, title: &str, text: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("open_tabs mutex poisoned".into()))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(format!("open_tabs tx: {e}")))?;
        tx.execute(
            "DELETE FROM open_tabs_fts WHERE rowid = ?1",
            params![tab_id],
        )
        .map_err(|e| Error::Storage(format!("open_tabs delete-before-insert: {e}")))?;
        tx.execute(
            "INSERT INTO open_tabs_fts (rowid, url, title, text) VALUES (?1, ?2, ?3, ?4)",
            params![tab_id, url, title, text],
        )
        .map_err(|e| Error::Storage(format!("open_tabs insert: {e}")))?;
        tx.commit()
            .map_err(|e| Error::Storage(format!("open_tabs commit: {e}")))?;
        Ok(())
    }

    /// Убрать вкладку из индекса (при её закрытии). No-op, если вкладки нет.
    pub fn remove_tab(&self, tab_id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("open_tabs mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM open_tabs_fts WHERE rowid = ?1",
            params![tab_id],
        )
        .map_err(|e| Error::Storage(format!("open_tabs remove: {e}")))?;
        Ok(())
    }

    /// Полнотекстовый поиск по `(url, title, text)` среди открытых вкладок,
    /// ранжирование bm25. `query` — в FTS5-синтаксисе (implicit AND для
    /// нескольких слов, `OR`, `"phrase"`, и т.д.). Сниппет — по колонке
    /// `text`. Сортировка: самый релевантный первым.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<OpenTabHit>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("open_tabs mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT rowid, url, title,
                        snippet(open_tabs_fts, 2, '**', '**', '…', 32) AS snip,
                        bm25(open_tabs_fts) AS score
                 FROM open_tabs_fts
                 WHERE open_tabs_fts MATCH ?1
                 ORDER BY bm25(open_tabs_fts)
                 LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("open_tabs prepare search: {e}")))?;
        let rows = stmt
            .query_map(params![query, limit], |row| {
                Ok(OpenTabHit {
                    tab_id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    snippet: row.get(3)?,
                    score: row.get(4)?,
                })
            })
            .map_err(|e| Error::Storage(format!("open_tabs query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("open_tabs row: {e}")))?);
        }
        Ok(out)
    }

    /// Текущее число проиндексированных открытых вкладок.
    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("open_tabs mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM open_tabs_fts", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("open_tabs count: {e}")))?;
        Ok(n)
    }

    /// Очистить весь индекс (например, при выходе или сбросе сессии).
    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("open_tabs mutex poisoned".into()))?;
        conn.execute("DELETE FROM open_tabs_fts", [])
            .map_err(|e| Error::Storage(format!("open_tabs clear: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> OpenTabsIndex {
        OpenTabsIndex::new().unwrap()
    }

    #[test]
    fn index_then_search_basic() {
        let idx = make();
        idx.index_tab(
            7,
            "https://example.com/rust",
            "Rust language",
            "Rust is a systems programming language",
        )
        .unwrap();
        let hits = idx.search("systems", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tab_id, 7);
        assert_eq!(hits[0].url, "https://example.com/rust");
        assert_eq!(hits[0].title, "Rust language");
    }

    #[test]
    fn search_matches_title_and_url() {
        let idx = make();
        idx.index_tab(1, "https://docs.rs/serde", "Serde docs", "body text").unwrap();
        // Матч по title.
        assert_eq!(idx.search("Serde", 10).unwrap().len(), 1);
        // Матч по url-токену.
        assert_eq!(idx.search("docs", 10).unwrap().len(), 1);
    }

    #[test]
    fn snippet_highlights_match() {
        let idx = make();
        idx.index_tab(1, "https://a/", "T", "the quick brown fox jumps").unwrap();
        let hits = idx.search("brown", 10).unwrap();
        assert_eq!(hits.len(), 1);
        let snip_lc = hits[0].snippet.to_lowercase();
        assert!(snip_lc.contains("**brown**"), "snippet = {}", hits[0].snippet);
    }

    #[test]
    fn reindex_overwrites_same_tab() {
        let idx = make();
        idx.index_tab(3, "https://a/old", "Old title", "old content here").unwrap();
        // Навигация в той же вкладке — переиндексация под тем же tab_id.
        idx.index_tab(3, "https://a/new", "New title", "fresh content here").unwrap();
        // Старый контент больше не находится.
        assert!(idx.search("old", 10).unwrap().is_empty());
        // Новый — находится, и это одна и та же вкладка (не дубликат).
        let hits = idx.search("fresh", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tab_id, 3);
        assert_eq!(hits[0].url, "https://a/new");
        assert_eq!(idx.count().unwrap(), 1);
    }

    #[test]
    fn remove_tab_drops_from_index() {
        let idx = make();
        idx.index_tab(5, "https://a/", "T", "closable content").unwrap();
        assert_eq!(idx.search("closable", 10).unwrap().len(), 1);
        idx.remove_tab(5).unwrap();
        assert!(idx.search("closable", 10).unwrap().is_empty());
        assert_eq!(idx.count().unwrap(), 0);
    }

    #[test]
    fn remove_missing_tab_is_noop() {
        let idx = make();
        idx.index_tab(1, "https://a/", "T", "x").unwrap();
        // Закрытие несуществующей вкладки не ошибка и не трогает остальное.
        idx.remove_tab(999).unwrap();
        assert_eq!(idx.count().unwrap(), 1);
    }

    #[test]
    fn search_implicit_and_across_tabs() {
        let idx = make();
        idx.index_tab(1, "https://a/", "t1", "apple banana cherry").unwrap();
        idx.index_tab(2, "https://b/", "t2", "apple grape").unwrap();
        let hits = idx.search("apple banana", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tab_id, 1);
    }

    #[test]
    fn ranking_more_matches_first() {
        let idx = make();
        idx.index_tab(1, "https://a/", "intro", "rust language").unwrap();
        idx.index_tab(2, "https://b/", "deep", "rust rust rust системный язык").unwrap();
        let hits = idx.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].tab_id, 2);
        assert!(hits[0].score < hits[1].score, "scores: {:?}", hits);
    }

    #[test]
    fn search_limit_respected() {
        let idx = make();
        for i in 1..=10 {
            idx.index_tab(i, &format!("https://e{i}/"), "t", "shared keyword").unwrap();
        }
        assert_eq!(idx.search("shared", 3).unwrap().len(), 3);
        assert_eq!(idx.count().unwrap(), 10);
    }

    #[test]
    fn cyrillic_text_search() {
        let idx = make();
        idx.index_tab(
            1,
            "https://пример.рф/",
            "Главная",
            "Это статья о русском языке и его особенностях",
        )
        .unwrap();
        let hits = idx.search("русском", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Главная");
    }

    #[test]
    fn case_insensitive_via_unicode61() {
        let idx = make();
        idx.index_tab(1, "https://a/", "T", "Rust is great").unwrap();
        assert_eq!(idx.search("rust", 10).unwrap().len(), 1);
    }

    #[test]
    fn no_match_returns_empty() {
        let idx = make();
        idx.index_tab(1, "https://a/", "T", "hello world").unwrap();
        assert!(idx.search("nonexistent", 10).unwrap().is_empty());
    }

    #[test]
    fn clear_wipes_index() {
        let idx = make();
        idx.index_tab(1, "https://a/", "t", "x").unwrap();
        idx.index_tab(2, "https://b/", "t", "x").unwrap();
        idx.clear().unwrap();
        assert!(idx.search("x", 10).unwrap().is_empty());
        assert_eq!(idx.count().unwrap(), 0);
    }
}
