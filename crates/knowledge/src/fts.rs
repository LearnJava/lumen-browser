//! FTS5-индекс над текстовым содержимым посещённых страниц.
//!
//! Схема:
//! ```sql
//! CREATE VIRTUAL TABLE history_fts USING fts5(
//!     url, title, text,
//!     tokenize = 'unicode61 remove_diacritics 2'
//! );
//! ```
//!
//! `unicode61` — встроенный токенайзер SQLite, делит по Unicode word
//! boundaries, lowercase нормализация, remove_diacritics=2 убирает
//! комбинирующие диакритические знаки. Достаточно для базового
//! поиска по русскому и латинице.
//!
//! Ранжирование: встроенный `bm25(history_fts)`. По умолчанию большие
//! значения = меньше релевантности (negated по contract FTS5);
//! `ORDER BY bm25(history_fts)` даёт самый релевантный первым.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

/// Результат полнотекстового поиска.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// rowid в FTS5-таблице. Соответствует `History.id` при общей БД.
    pub rowid: i64,
    pub url: String,
    pub title: String,
    /// Сниппет с подсветкой матчей — markdown-bold `**...**` вокруг
    /// совпавших слов. Длина ~30 токенов вокруг первого матча.
    pub snippet: String,
    /// BM25-score; меньше = релевантнее (negated per FTS5).
    pub score: f64,
}

/// FTS5-индекс над `(url, title, text)`. Открывается отдельной БД-файлом
/// или in-memory. Может использоваться поверх той же `Connection`-файла,
/// что и `History` (если открыть с тем же путём — SQLite это поддерживает).
pub struct HistoryFts {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for HistoryFts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryFts").finish()
    }
}

impl HistoryFts {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("knowledge open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("knowledge open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE VIRTUAL TABLE IF NOT EXISTS history_fts USING fts5(
                url, title, text,
                tokenize = 'unicode61 remove_diacritics 2'
            );
            "#,
        )
        .map_err(|e| Error::Storage(format!("knowledge init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Добавить или обновить запись в индексе. `rowid` обычно совпадает
    /// с `History.id`. Если запись с таким rowid уже есть — обновляем
    /// все три колонки (FTS5 не поддерживает UPSERT напрямую, делаем
    /// DELETE+INSERT в одной транзакции).
    pub fn index(&self, rowid: i64, url: &str, title: &str, text: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("knowledge mutex poisoned".into()))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Storage(format!("knowledge tx: {e}")))?;
        tx.execute(
            "DELETE FROM history_fts WHERE rowid = ?1",
            params![rowid],
        )
        .map_err(|e| Error::Storage(format!("knowledge delete-before-insert: {e}")))?;
        tx.execute(
            "INSERT INTO history_fts (rowid, url, title, text) VALUES (?1, ?2, ?3, ?4)",
            params![rowid, url, title, text],
        )
        .map_err(|e| Error::Storage(format!("knowledge insert: {e}")))?;
        tx.commit()
            .map_err(|e| Error::Storage(format!("knowledge commit: {e}")))?;
        Ok(())
    }

    /// Удалить запись по rowid.
    pub fn unindex(&self, rowid: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("knowledge mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM history_fts WHERE rowid = ?1",
            params![rowid],
        )
        .map_err(|e| Error::Storage(format!("knowledge unindex: {e}")))?;
        Ok(())
    }

    /// Полнотекстовый поиск по `text` с ранжированием bm25. `query` —
    /// в FTS5-синтаксисе (см. <https://sqlite.org/fts5.html#full_text_query_syntax>):
    /// `"foo bar"` AND-конъюнкция, `foo OR bar` для disjunction, `^foo`
    /// — начало документа, и т.д. Простая строка из 1-2 слов работает
    /// как implicit AND.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<SearchHit>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("knowledge mutex poisoned".into()))?;
        // snippet(table, col_index, start, end, ellipsis, max_tokens):
        // col_index = 2 (text — третья колонка после url, title);
        // markup `**...**` для подсветки; ellipsis `…`; max 32 токена.
        let mut stmt = conn
            .prepare(
                "SELECT rowid, url, title,
                        snippet(history_fts, 2, '**', '**', '…', 32) AS snip,
                        bm25(history_fts) AS score
                 FROM history_fts
                 WHERE history_fts MATCH ?1
                 ORDER BY bm25(history_fts)
                 LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("knowledge prepare search: {e}")))?;
        let rows = stmt
            .query_map(params![query, limit], |row| {
                Ok(SearchHit {
                    rowid: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    snippet: row.get(3)?,
                    score: row.get(4)?,
                })
            })
            .map_err(|e| Error::Storage(format!("knowledge query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("knowledge row: {e}")))?);
        }
        Ok(out)
    }

    /// Полная очистка индекса.
    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("knowledge mutex poisoned".into()))?;
        conn.execute("DELETE FROM history_fts", [])
            .map_err(|e| Error::Storage(format!("knowledge clear: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> HistoryFts {
        HistoryFts::open_in_memory().unwrap()
    }

    #[test]
    fn index_then_search_basic() {
        let f = make();
        f.index(
            1,
            "https://example.com/rust",
            "Rust language",
            "Rust is a systems programming language",
        )
        .unwrap();
        let hits = f.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rowid, 1);
        assert_eq!(hits[0].url, "https://example.com/rust");
        // Snippet должен содержать подсветку матча `**rust**` (FTS5
        // matches тоже case-insensitive через unicode61).
        let snip_lc = hits[0].snippet.to_lowercase();
        assert!(snip_lc.contains("**rust**"), "snippet = {}", hits[0].snippet);
    }

    #[test]
    fn search_returns_no_hits_for_unknown() {
        let f = make();
        f.index(1, "https://example.com/", "title", "some text").unwrap();
        assert!(f.search("nonexistent", 10).unwrap().is_empty());
    }

    #[test]
    fn search_implicit_and() {
        let f = make();
        f.index(1, "https://a/", "t1", "apple banana cherry").unwrap();
        f.index(2, "https://b/", "t2", "apple grape").unwrap();
        // implicit AND — должен вернуть только запись 1.
        let hits = f.search("apple banana", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rowid, 1);
    }

    #[test]
    fn search_or_query() {
        let f = make();
        f.index(1, "https://a/", "t1", "apple").unwrap();
        f.index(2, "https://b/", "t2", "banana").unwrap();
        f.index(3, "https://c/", "t3", "cherry").unwrap();
        let hits = f.search("apple OR banana", 10).unwrap();
        let rowids: Vec<i64> = hits.iter().map(|h| h.rowid).collect();
        assert_eq!(rowids.len(), 2);
        assert!(rowids.contains(&1));
        assert!(rowids.contains(&2));
    }

    #[test]
    fn ranking_more_matches_first() {
        let f = make();
        // Документ с двумя матчами "rust" должен ранжироваться выше.
        f.index(1, "https://a/", "Rust intro", "rust language").unwrap();
        f.index(
            2,
            "https://b/",
            "Rust deep dive",
            "rust rust rust — это системный язык программирования",
        )
        .unwrap();
        let hits = f.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 2);
        // BM25 для документа с 3 матчами должен быть меньше (более релевантен).
        // Document 2 должен быть первым.
        assert_eq!(hits[0].rowid, 2);
        assert_eq!(hits[1].rowid, 1);
        assert!(hits[0].score < hits[1].score, "scores: {:?}, {:?}", hits[0].score, hits[1].score);
    }

    #[test]
    fn cyrillic_text_search() {
        let f = make();
        f.index(
            1,
            "https://пример.рф/",
            "Главная страница",
            "Это статья о русском языке и его особенностях",
        )
        .unwrap();
        let hits = f.search("русском", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Главная страница");
    }

    #[test]
    fn index_overwrites_existing() {
        let f = make();
        f.index(1, "https://a/", "old title", "old text").unwrap();
        f.index(1, "https://a/", "new title", "new text").unwrap();
        let hits_new = f.search("new", 10).unwrap();
        assert_eq!(hits_new.len(), 1);
        assert_eq!(hits_new[0].title, "new title");
        let hits_old = f.search("old", 10).unwrap();
        assert!(hits_old.is_empty(), "old text should be removed");
    }

    #[test]
    fn unindex_removes_entry() {
        let f = make();
        f.index(1, "https://a/", "t", "hello world").unwrap();
        f.unindex(1).unwrap();
        assert!(f.search("hello", 10).unwrap().is_empty());
    }

    #[test]
    fn search_limit_respected() {
        let f = make();
        for i in 1..=10 {
            f.index(i, &format!("https://e{i}/"), "t", "shared keyword").unwrap();
        }
        let hits = f.search("shared", 3).unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn clear_wipes_index() {
        let f = make();
        f.index(1, "https://a/", "t", "x").unwrap();
        f.index(2, "https://b/", "t", "x").unwrap();
        f.clear().unwrap();
        assert!(f.search("x", 10).unwrap().is_empty());
    }

    #[test]
    fn case_insensitive_via_unicode61() {
        // unicode61 lower-case normalizes — `RUST` находит `rust`.
        let f = make();
        f.index(1, "https://a/", "T", "Rust is great").unwrap();
        let hits = f.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }
}
