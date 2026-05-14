//! Form autofill — сохранённые значения полей форм для autocomplete.
//!
//! Хранится по `(origin, field_name)` ключу + последнее значение +
//! `frequency` (сколько раз использовалось). UI выбирает наиболее
//! популярное значение для `<input autocomplete="email">` etc.
//!
//! Phase 0: storage layer. UI-prompt при submit ("сохранить значение?")
//! и detection autocomplete-атрибутов в форме — задача shell.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutofillEntry {
    pub origin: String,
    pub field_name: String,
    pub value: String,
    pub frequency: i64,
    pub last_used: i64,
}

pub struct Autofill {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Autofill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Autofill").finish()
    }
}

impl Autofill {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("autofill open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("autofill open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Composite PK по (origin, field_name, value) — повторное
        // submit-нутое значение не дублируется, а инкрементит frequency.
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS autofill (
                origin     TEXT NOT NULL,
                field_name TEXT NOT NULL,
                value      TEXT NOT NULL,
                frequency  INTEGER NOT NULL DEFAULT 1,
                last_used  INTEGER NOT NULL,
                PRIMARY KEY (origin, field_name, value)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS autofill_origin_field_idx
                ON autofill(origin, field_name);
            "#,
        )
        .map_err(|e| Error::Storage(format!("autofill init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Зафиксировать использование значения. Upsert: insert или
    /// frequency++/last_used updated.
    pub fn record(
        &self,
        origin: &str,
        field_name: &str,
        value: &str,
        now_unix: i64,
    ) -> Result<()> {
        if value.is_empty() {
            return Ok(());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("autofill mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO autofill (origin, field_name, value, last_used)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (origin, field_name, value) DO UPDATE SET
                 frequency = frequency + 1,
                 last_used = MAX(last_used, excluded.last_used)",
            params![origin, field_name, value, now_unix],
        )
        .map_err(|e| Error::Storage(format!("autofill record: {e}")))?;
        Ok(())
    }

    /// Получить все сохранённые значения для (origin, field_name),
    /// отсортированные по frequency DESC + last_used DESC.
    pub fn suggestions(
        &self,
        origin: &str,
        field_name: &str,
        limit: i64,
    ) -> Result<Vec<AutofillEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("autofill mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, field_name, value, frequency, last_used
                 FROM autofill WHERE origin = ?1 AND field_name = ?2
                 ORDER BY frequency DESC, last_used DESC LIMIT ?3",
            )
            .map_err(|e| Error::Storage(format!("autofill suggestions prepare: {e}")))?;
        let rows = stmt
            .query_map(params![origin, field_name, limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("autofill suggestions query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("autofill row: {e}")))?);
        }
        Ok(out)
    }

    /// Самое популярное значение для поля.
    pub fn best_for(&self, origin: &str, field_name: &str) -> Result<Option<String>> {
        let entries = self.suggestions(origin, field_name, 1)?;
        Ok(entries.into_iter().next().map(|e| e.value))
    }

    /// Удалить конкретное значение.
    pub fn delete(&self, origin: &str, field_name: &str, value: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("autofill mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM autofill WHERE origin = ?1 AND field_name = ?2 AND value = ?3",
            params![origin, field_name, value],
        )
        .map_err(|e| Error::Storage(format!("autofill delete: {e}")))?;
        Ok(())
    }

    /// Удалить все autofill-данные для origin (clear-site-data).
    pub fn clear_origin(&self, origin: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("autofill mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM autofill WHERE origin = ?1",
                params![origin],
            )
            .map_err(|e| Error::Storage(format!("autofill clear_origin: {e}")))?;
        Ok(n)
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("autofill mutex poisoned".into()))?;
        conn.execute("DELETE FROM autofill", [])
            .map_err(|e| Error::Storage(format!("autofill clear: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("autofill mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM autofill", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("autofill count: {e}")))?;
        Ok(n)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutofillEntry> {
    Ok(AutofillEntry {
        origin: row.get(0)?,
        field_name: row.get(1)?,
        value: row.get(2)?,
        frequency: row.get(3)?,
        last_used: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Autofill {
        Autofill::open_in_memory().unwrap()
    }

    #[test]
    fn record_inserts_new_value() {
        let a = make();
        a.record("https://x/", "email", "user@example.com", 100).unwrap();
        let s = a.suggestions("https://x/", "email", 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, "user@example.com");
        assert_eq!(s[0].frequency, 1);
    }

    #[test]
    fn record_increments_existing() {
        let a = make();
        a.record("https://x/", "name", "Alice", 100).unwrap();
        a.record("https://x/", "name", "Alice", 200).unwrap();
        a.record("https://x/", "name", "Alice", 300).unwrap();
        let s = a.suggestions("https://x/", "name", 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].frequency, 3);
        assert_eq!(s[0].last_used, 300);
    }

    #[test]
    fn record_empty_skipped() {
        let a = make();
        a.record("https://x/", "name", "", 100).unwrap();
        assert_eq!(a.count().unwrap(), 0);
    }

    #[test]
    fn suggestions_sorted_by_frequency_then_last_used() {
        let a = make();
        a.record("https://x/", "name", "rare", 100).unwrap();
        for _ in 0..5 {
            a.record("https://x/", "name", "frequent", 200).unwrap();
        }
        a.record("https://x/", "name", "medium", 300).unwrap();
        a.record("https://x/", "name", "medium", 400).unwrap();
        let s = a.suggestions("https://x/", "name", 10).unwrap();
        assert_eq!(s[0].value, "frequent");
        assert_eq!(s[0].frequency, 5);
        assert_eq!(s[1].value, "medium");
        assert_eq!(s[2].value, "rare");
    }

    #[test]
    fn suggestions_filtered_by_field_name() {
        let a = make();
        a.record("https://x/", "email", "a@b.c", 100).unwrap();
        a.record("https://x/", "name", "Alice", 200).unwrap();
        let s = a.suggestions("https://x/", "email", 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, "a@b.c");
    }

    #[test]
    fn suggestions_isolated_by_origin() {
        let a = make();
        a.record("https://a/", "email", "a@b.c", 100).unwrap();
        a.record("https://b/", "email", "x@y.z", 200).unwrap();
        let s_a = a.suggestions("https://a/", "email", 10).unwrap();
        assert_eq!(s_a.len(), 1);
        assert_eq!(s_a[0].value, "a@b.c");
    }

    #[test]
    fn best_for_returns_most_popular() {
        let a = make();
        a.record("https://x/", "email", "rare@x.com", 100).unwrap();
        for _ in 0..5 {
            a.record("https://x/", "email", "main@x.com", 200).unwrap();
        }
        assert_eq!(
            a.best_for("https://x/", "email").unwrap(),
            Some("main@x.com".to_string())
        );
    }

    #[test]
    fn best_for_missing_returns_none() {
        let a = make();
        assert!(a.best_for("https://nope/", "email").unwrap().is_none());
    }

    #[test]
    fn delete_specific_value() {
        let a = make();
        a.record("https://x/", "email", "old@x.com", 100).unwrap();
        a.record("https://x/", "email", "new@x.com", 200).unwrap();
        a.delete("https://x/", "email", "old@x.com").unwrap();
        let s = a.suggestions("https://x/", "email", 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, "new@x.com");
    }

    #[test]
    fn clear_origin_removes_all_fields() {
        let a = make();
        a.record("https://x/", "email", "a@x.com", 100).unwrap();
        a.record("https://x/", "name", "Alice", 100).unwrap();
        a.record("https://y/", "email", "b@y.com", 100).unwrap();
        let removed = a.clear_origin("https://x/").unwrap();
        assert_eq!(removed, 2);
        // y осталась.
        assert_eq!(a.count().unwrap(), 1);
    }

    #[test]
    fn cyrillic_values() {
        let a = make();
        a.record("https://пример.рф/", "имя", "Алиса", 100).unwrap();
        let s = a.suggestions("https://пример.рф/", "имя", 10).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].value, "Алиса");
    }
}
