//! §12.2 Аннотации и заметки.
//!
//! Заметка — выделенный текст со страницы плюс пользовательский
//! комментарий, привязанный к URL. Хранится в одной обычной таблице
//! `notes` + FTS5-зеркало `notes_fts(selection, comment)` для поиска
//! по содержимому заметок. URL и source-context — обычные колонки,
//! не FTS-индексируются (это поиск «по тексту заметки», а не «по URL»).
//!
//! Phase 0 покрывает: создание, обновление, удаление, поиск.
//! Range API (для восстановления highlight-наложений при повторном
//! открытии страницы), экспорт в Markdown/JSON и UI — отдельные задачи.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Одна заметка пользователя.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: i64,
    pub url: String,
    /// Выделенный текст со страницы (selection).
    pub selection: String,
    /// Окружающий абзац / context (опционально).
    pub context: String,
    /// Пользовательский комментарий (опционально).
    pub comment: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteSearchHit {
    pub note: Note,
    /// Сниппет вокруг матча, c markdown-подсветкой `**...**`.
    pub snippet: String,
    pub score: f64,
}

pub struct Notes {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Notes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notes").finish()
    }
}

impl Notes {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("notes open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("notes open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS notes (
                id         INTEGER PRIMARY KEY,
                url        TEXT NOT NULL,
                selection  TEXT NOT NULL,
                context    TEXT NOT NULL DEFAULT '',
                comment    TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS notes_url_idx ON notes(url);
            CREATE INDEX IF NOT EXISTS notes_created_idx ON notes(created_at DESC);
            CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                selection, comment,
                content = 'notes',
                content_rowid = 'id',
                tokenize = 'unicode61 remove_diacritics 2'
            );
            -- Контент-table-связи (external content FTS): синхронизация
            -- через триггеры — стандартный паттерн FTS5 §4.4.3.
            CREATE TRIGGER IF NOT EXISTS notes_ai AFTER INSERT ON notes BEGIN
                INSERT INTO notes_fts (rowid, selection, comment)
                VALUES (new.id, new.selection, new.comment);
            END;
            CREATE TRIGGER IF NOT EXISTS notes_ad AFTER DELETE ON notes BEGIN
                INSERT INTO notes_fts (notes_fts, rowid, selection, comment)
                VALUES ('delete', old.id, old.selection, old.comment);
            END;
            CREATE TRIGGER IF NOT EXISTS notes_au AFTER UPDATE ON notes BEGIN
                INSERT INTO notes_fts (notes_fts, rowid, selection, comment)
                VALUES ('delete', old.id, old.selection, old.comment);
                INSERT INTO notes_fts (rowid, selection, comment)
                VALUES (new.id, new.selection, new.comment);
            END;
            "#,
        )
        .map_err(|e| Error::Storage(format!("notes init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Создать заметку. Возвращает её id.
    pub fn add(
        &self,
        url: &str,
        selection: &str,
        context: &str,
        comment: &str,
        created_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO notes (url, selection, context, comment, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![url, selection, context, comment, created_at],
        )
        .map_err(|e| Error::Storage(format!("notes add: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Обновить selection / context / comment по id. created_at не меняется.
    pub fn update(
        &self,
        id: i64,
        selection: &str,
        context: &str,
        comment: &str,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        conn.execute(
            "UPDATE notes SET selection = ?2, context = ?3, comment = ?4 WHERE id = ?1",
            params![id, selection, context, comment],
        )
        .map_err(|e| Error::Storage(format!("notes update: {e}")))?;
        Ok(())
    }

    /// Удалить заметку по id.
    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        conn.execute("DELETE FROM notes WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("notes delete: {e}")))?;
        Ok(())
    }

    /// Получить заметку по id.
    pub fn get(&self, id: i64) -> Result<Option<Note>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        let n = conn
            .query_row(
                "SELECT id, url, selection, context, comment, created_at FROM notes WHERE id = ?1",
                params![id],
                row_to_note,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("notes get: {e}")))?;
        Ok(n)
    }

    /// Все заметки для конкретного URL (для восстановления highlight-
    /// наложений при открытии страницы). Сортировка по created_at ASC —
    /// в хронологическом порядке создания.
    pub fn list_for_url(&self, url: &str) -> Result<Vec<Note>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, selection, context, comment, created_at
                 FROM notes WHERE url = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| Error::Storage(format!("notes list_for_url prepare: {e}")))?;
        let rows = stmt
            .query_map(params![url], row_to_note)
            .map_err(|e| Error::Storage(format!("notes list_for_url query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("notes row: {e}")))?);
        }
        Ok(out)
    }

    /// Последние N заметок (по убыванию created_at).
    pub fn recent(&self, limit: i64) -> Result<Vec<Note>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, selection, context, comment, created_at
                 FROM notes ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("notes recent prepare: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_note)
            .map_err(|e| Error::Storage(format!("notes recent query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("notes row: {e}")))?);
        }
        Ok(out)
    }

    /// Полнотекстовый поиск по selection + comment.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<NoteSearchHit>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT n.id, n.url, n.selection, n.context, n.comment, n.created_at,
                        snippet(notes_fts, 0, '**', '**', '…', 32) AS snip,
                        bm25(notes_fts) AS score
                 FROM notes_fts
                 JOIN notes n ON n.id = notes_fts.rowid
                 WHERE notes_fts MATCH ?1
                 ORDER BY bm25(notes_fts)
                 LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("notes search prepare: {e}")))?;
        let rows = stmt
            .query_map(params![query, limit], |row| {
                let note = Note {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    selection: row.get(2)?,
                    context: row.get(3)?,
                    comment: row.get(4)?,
                    created_at: row.get(5)?,
                };
                Ok(NoteSearchHit {
                    note,
                    snippet: row.get(6)?,
                    score: row.get(7)?,
                })
            })
            .map_err(|e| Error::Storage(format!("notes search query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("notes search row: {e}")))?);
        }
        Ok(out)
    }

    /// Общее число заметок.
    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("notes count: {e}")))?;
        Ok(n)
    }

    /// Удалить все заметки. Триггеры notes_ad чистят FTS индекс.
    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("notes mutex poisoned".into()))?;
        conn.execute("DELETE FROM notes", [])
            .map_err(|e| Error::Storage(format!("notes clear: {e}")))?;
        Ok(())
    }
}

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        url: row.get(1)?,
        selection: row.get(2)?,
        context: row.get(3)?,
        comment: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Notes {
        Notes::open_in_memory().unwrap()
    }

    #[test]
    fn add_and_get_basic() {
        let n = make();
        let id = n
            .add(
                "https://example.com/",
                "important sentence",
                "surrounding paragraph",
                "my comment",
                100,
            )
            .unwrap();
        assert!(id > 0);
        let got = n.get(id).unwrap().unwrap();
        assert_eq!(got.url, "https://example.com/");
        assert_eq!(got.selection, "important sentence");
        assert_eq!(got.context, "surrounding paragraph");
        assert_eq!(got.comment, "my comment");
        assert_eq!(got.created_at, 100);
    }

    #[test]
    fn update_changes_content() {
        let n = make();
        let id = n.add("https://x/", "old sel", "old ctx", "old", 100).unwrap();
        n.update(id, "new sel", "new ctx", "new").unwrap();
        let got = n.get(id).unwrap().unwrap();
        assert_eq!(got.selection, "new sel");
        assert_eq!(got.context, "new ctx");
        assert_eq!(got.comment, "new");
        // created_at не должен измениться.
        assert_eq!(got.created_at, 100);
    }

    #[test]
    fn delete_removes_note() {
        let n = make();
        let id = n.add("https://x/", "sel", "", "", 100).unwrap();
        n.delete(id).unwrap();
        assert!(n.get(id).unwrap().is_none());
    }

    #[test]
    fn list_for_url_ascending_by_created_at() {
        let n = make();
        n.add("https://x/", "first", "", "", 100).unwrap();
        n.add("https://x/", "third", "", "", 300).unwrap();
        n.add("https://x/", "second", "", "", 200).unwrap();
        n.add("https://other/", "skip", "", "", 150).unwrap();
        let notes = n.list_for_url("https://x/").unwrap();
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].selection, "first");
        assert_eq!(notes[1].selection, "second");
        assert_eq!(notes[2].selection, "third");
    }

    #[test]
    fn recent_descending_by_created_at() {
        let n = make();
        n.add("https://a/", "old", "", "", 100).unwrap();
        n.add("https://b/", "new", "", "", 300).unwrap();
        n.add("https://c/", "mid", "", "", 200).unwrap();
        let r = n.recent(10).unwrap();
        assert_eq!(r[0].selection, "new");
        assert_eq!(r[1].selection, "mid");
        assert_eq!(r[2].selection, "old");
    }

    #[test]
    fn search_finds_by_selection() {
        let n = make();
        n.add("https://x/", "Rust is great", "", "", 100).unwrap();
        n.add("https://y/", "Python is fine", "", "", 200).unwrap();
        let hits = n.search("Rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note.selection, "Rust is great");
        let snip_lc = hits[0].snippet.to_lowercase();
        assert!(snip_lc.contains("**rust**"));
    }

    #[test]
    fn search_finds_by_comment() {
        let n = make();
        n.add("https://x/", "boring text", "", "love this article", 100).unwrap();
        let hits = n.search("love", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn search_no_match_returns_empty() {
        let n = make();
        n.add("https://x/", "apple", "", "", 100).unwrap();
        assert!(n.search("banana", 10).unwrap().is_empty());
    }

    #[test]
    fn fts_synced_after_update() {
        let n = make();
        let id = n.add("https://x/", "apple", "", "", 100).unwrap();
        // Update переписывает selection.
        n.update(id, "banana", "", "").unwrap();
        // Старое значение `apple` больше не находится.
        assert!(n.search("apple", 10).unwrap().is_empty());
        // Новое `banana` находится.
        assert_eq!(n.search("banana", 10).unwrap().len(), 1);
    }

    #[test]
    fn fts_synced_after_delete() {
        let n = make();
        let id = n.add("https://x/", "deletable", "", "", 100).unwrap();
        assert_eq!(n.search("deletable", 10).unwrap().len(), 1);
        n.delete(id).unwrap();
        assert!(n.search("deletable", 10).unwrap().is_empty());
    }

    #[test]
    fn cyrillic_selection_and_comment() {
        let n = make();
        n.add(
            "https://пример.рф/",
            "Очень важное предложение",
            "",
            "интересная мысль",
            100,
        )
        .unwrap();
        let hits = n.search("важное", 10).unwrap();
        assert_eq!(hits.len(), 1);
        let hits2 = n.search("мысль", 10).unwrap();
        assert_eq!(hits2.len(), 1);
    }

    #[test]
    fn count_and_clear() {
        let n = make();
        n.add("https://a/", "x", "", "", 100).unwrap();
        n.add("https://b/", "y", "", "", 200).unwrap();
        assert_eq!(n.count().unwrap(), 2);
        n.clear().unwrap();
        assert_eq!(n.count().unwrap(), 0);
        // FTS тоже почищена.
        assert!(n.search("x", 10).unwrap().is_empty());
    }

    #[test]
    fn get_missing_returns_none() {
        let n = make();
        assert!(n.get(999).unwrap().is_none());
    }
}
