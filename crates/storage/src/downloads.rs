//! Download history — журнал скачиваний пользователя.
//!
//! Phase 0: persistent storage layer (CRUD + статусы + сортировка).
//! Реальная очередь загрузок, прогресс-репортинг, resume, MIME-sniffing
//! — задача `lumen-network` + `lumen-shell`. Здесь только запись о том,
//! что было скачано (или попыталось скачаться).

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Статус скачивания.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DownloadStatus {
    /// В процессе скачивания.
    #[default]
    Pending,
    /// Успешно завершено.
    Done,
    /// Прервано пользователем (или ошибкой).
    Cancelled,
    /// Сетевая / I/O ошибка.
    Failed,
}

impl DownloadStatus {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
    fn from_db_str(s: &str) -> Self {
        match s {
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Одна запись о скачивании.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadEntry {
    pub id: i64,
    pub url: String,
    /// Полный путь к файлу на диске. Пустая строка если ещё не выбран.
    pub file_path: String,
    /// Имя файла без пути (для UI).
    pub filename: String,
    pub mime_type: String,
    /// Известный размер (Content-Length) — `None` если сервер не сообщил.
    pub total_size: Option<i64>,
    /// Сколько байт уже скачано.
    pub bytes_received: i64,
    pub status: DownloadStatus,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    /// Опц. сообщение об ошибке (для status=Failed).
    pub error: Option<String>,
}

pub struct Downloads {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Downloads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Downloads").finish()
    }
}

impl Downloads {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("downloads open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("downloads open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS downloads (
                id             INTEGER PRIMARY KEY,
                url            TEXT NOT NULL,
                file_path      TEXT NOT NULL DEFAULT '',
                filename       TEXT NOT NULL DEFAULT '',
                mime_type      TEXT NOT NULL DEFAULT '',
                total_size     INTEGER,
                bytes_received INTEGER NOT NULL DEFAULT 0,
                status         TEXT NOT NULL DEFAULT 'pending',
                started_at     INTEGER NOT NULL,
                completed_at   INTEGER,
                error          TEXT
            );
            CREATE INDEX IF NOT EXISTS downloads_status_idx ON downloads(status);
            CREATE INDEX IF NOT EXISTS downloads_started_idx ON downloads(started_at DESC);
            "#,
        )
        .map_err(|e| Error::Storage(format!("downloads init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Создать запись о новом скачивании. Возвращает id.
    pub fn start(
        &self,
        url: &str,
        filename: &str,
        file_path: &str,
        mime_type: &str,
        total_size: Option<i64>,
        started_at: i64,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO downloads (url, filename, file_path, mime_type, total_size, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![url, filename, file_path, mime_type, total_size, started_at],
        )
        .map_err(|e| Error::Storage(format!("downloads start: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Обновить bytes_received (для прогресса).
    pub fn update_progress(&self, id: i64, bytes_received: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.execute(
            "UPDATE downloads SET bytes_received = ?1 WHERE id = ?2",
            params![bytes_received, id],
        )
        .map_err(|e| Error::Storage(format!("downloads update_progress: {e}")))?;
        Ok(())
    }

    /// Зафиксировать успешное завершение.
    pub fn complete(&self, id: i64, completed_at: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.execute(
            "UPDATE downloads SET status = 'done', completed_at = ?1 WHERE id = ?2",
            params![completed_at, id],
        )
        .map_err(|e| Error::Storage(format!("downloads complete: {e}")))?;
        Ok(())
    }

    /// Зафиксировать отмену пользователем.
    pub fn cancel(&self, id: i64, completed_at: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.execute(
            "UPDATE downloads SET status = 'cancelled', completed_at = ?1 WHERE id = ?2",
            params![completed_at, id],
        )
        .map_err(|e| Error::Storage(format!("downloads cancel: {e}")))?;
        Ok(())
    }

    /// Зафиксировать ошибку.
    pub fn fail(&self, id: i64, completed_at: i64, error: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.execute(
            "UPDATE downloads SET status = 'failed', completed_at = ?1, error = ?2 WHERE id = ?3",
            params![completed_at, error, id],
        )
        .map_err(|e| Error::Storage(format!("downloads fail: {e}")))?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<DownloadEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.query_row(
            "SELECT id, url, file_path, filename, mime_type, total_size,
                    bytes_received, status, started_at, completed_at, error
             FROM downloads WHERE id = ?1",
            params![id],
            row_to_entry,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("downloads get: {e}")))
    }

    /// Все записи в порядке started_at DESC.
    pub fn list_all(&self, limit: i64) -> Result<Vec<DownloadEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, file_path, filename, mime_type, total_size,
                        bytes_received, status, started_at, completed_at, error
                 FROM downloads ORDER BY started_at DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(format!("downloads list prepare: {e}")))?;
        let rows = stmt
            .query_map(params![limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("downloads list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("downloads row: {e}")))?);
        }
        Ok(out)
    }

    /// Только в указанном статусе.
    pub fn list_by_status(&self, status: DownloadStatus, limit: i64) -> Result<Vec<DownloadEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, url, file_path, filename, mime_type, total_size,
                        bytes_received, status, started_at, completed_at, error
                 FROM downloads WHERE status = ?1 ORDER BY started_at DESC LIMIT ?2",
            )
            .map_err(|e| Error::Storage(format!("downloads list_status prepare: {e}")))?;
        let rows = stmt
            .query_map(params![status.as_db_str(), limit], row_to_entry)
            .map_err(|e| Error::Storage(format!("downloads list_status query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("downloads row: {e}")))?);
        }
        Ok(out)
    }

    /// Удалить запись (например, после удаления файла или clear-history).
    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("downloads delete: {e}")))?;
        Ok(())
    }

    /// Удалить все завершённые (done/cancelled/failed). Pending не трогаются.
    pub fn clear_completed(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM downloads WHERE status IN ('done', 'cancelled', 'failed')",
                [],
            )
            .map_err(|e| Error::Storage(format!("downloads clear_completed: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("downloads mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM downloads", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("downloads count: {e}")))?;
        Ok(n)
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadEntry> {
    Ok(DownloadEntry {
        id: row.get(0)?,
        url: row.get(1)?,
        file_path: row.get(2)?,
        filename: row.get(3)?,
        mime_type: row.get(4)?,
        total_size: row.get(5)?,
        bytes_received: row.get(6)?,
        status: DownloadStatus::from_db_str(&row.get::<_, String>(7)?),
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
        error: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Downloads {
        Downloads::open_in_memory().unwrap()
    }

    #[test]
    fn start_creates_pending_record() {
        let d = make();
        let id = d
            .start(
                "https://example.com/file.pdf",
                "file.pdf",
                "/Downloads/file.pdf",
                "application/pdf",
                Some(102400),
                100,
            )
            .unwrap();
        let e = d.get(id).unwrap().unwrap();
        assert_eq!(e.url, "https://example.com/file.pdf");
        assert_eq!(e.filename, "file.pdf");
        assert_eq!(e.mime_type, "application/pdf");
        assert_eq!(e.total_size, Some(102400));
        assert_eq!(e.bytes_received, 0);
        assert_eq!(e.status, DownloadStatus::Pending);
        assert_eq!(e.completed_at, None);
    }

    #[test]
    fn update_progress_increments_bytes() {
        let d = make();
        let id = d.start("https://x/", "x", "/x", "", None, 100).unwrap();
        d.update_progress(id, 1024).unwrap();
        assert_eq!(d.get(id).unwrap().unwrap().bytes_received, 1024);
        d.update_progress(id, 4096).unwrap();
        assert_eq!(d.get(id).unwrap().unwrap().bytes_received, 4096);
    }

    #[test]
    fn complete_sets_status_and_time() {
        let d = make();
        let id = d.start("https://x/", "x", "/x", "", None, 100).unwrap();
        d.complete(id, 500).unwrap();
        let e = d.get(id).unwrap().unwrap();
        assert_eq!(e.status, DownloadStatus::Done);
        assert_eq!(e.completed_at, Some(500));
    }

    #[test]
    fn cancel_sets_status_and_time() {
        let d = make();
        let id = d.start("https://x/", "x", "/x", "", None, 100).unwrap();
        d.cancel(id, 200).unwrap();
        let e = d.get(id).unwrap().unwrap();
        assert_eq!(e.status, DownloadStatus::Cancelled);
        assert_eq!(e.completed_at, Some(200));
    }

    #[test]
    fn fail_records_error_message() {
        let d = make();
        let id = d.start("https://x/", "x", "/x", "", None, 100).unwrap();
        d.fail(id, 300, "connection reset").unwrap();
        let e = d.get(id).unwrap().unwrap();
        assert_eq!(e.status, DownloadStatus::Failed);
        assert_eq!(e.completed_at, Some(300));
        assert_eq!(e.error, Some("connection reset".to_string()));
    }

    #[test]
    fn list_all_desc_by_started_at() {
        let d = make();
        d.start("https://a/", "a", "/a", "", None, 100).unwrap();
        d.start("https://c/", "c", "/c", "", None, 300).unwrap();
        d.start("https://b/", "b", "/b", "", None, 200).unwrap();
        let all = d.list_all(10).unwrap();
        let filenames: Vec<&str> = all.iter().map(|e| e.filename.as_str()).collect();
        assert_eq!(filenames, vec!["c", "b", "a"]);
    }

    #[test]
    fn list_by_status_filters() {
        let d = make();
        let id1 = d.start("https://a/", "a", "/a", "", None, 100).unwrap();
        let id2 = d.start("https://b/", "b", "/b", "", None, 200).unwrap();
        d.start("https://c/", "c", "/c", "", None, 300).unwrap();
        d.complete(id1, 150).unwrap();
        d.cancel(id2, 250).unwrap();
        // pending → один c.
        let pending = d.list_by_status(DownloadStatus::Pending, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].filename, "c");
        // done → один a.
        let done = d.list_by_status(DownloadStatus::Done, 10).unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].filename, "a");
        // cancelled → один b.
        let cancelled = d.list_by_status(DownloadStatus::Cancelled, 10).unwrap();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].filename, "b");
    }

    #[test]
    fn clear_completed_removes_done_cancelled_failed_only() {
        let d = make();
        let id_pending = d.start("https://p/", "p", "/p", "", None, 100).unwrap();
        let id_done = d.start("https://d/", "d", "/d", "", None, 200).unwrap();
        let id_cancelled = d.start("https://c/", "c", "/c", "", None, 300).unwrap();
        let id_failed = d.start("https://f/", "f", "/f", "", None, 400).unwrap();
        d.complete(id_done, 250).unwrap();
        d.cancel(id_cancelled, 350).unwrap();
        d.fail(id_failed, 450, "err").unwrap();
        let removed = d.clear_completed().unwrap();
        assert_eq!(removed, 3);
        // Pending остался.
        assert!(d.get(id_pending).unwrap().is_some());
        assert!(d.get(id_done).unwrap().is_none());
        assert!(d.get(id_cancelled).unwrap().is_none());
        assert!(d.get(id_failed).unwrap().is_none());
    }

    #[test]
    fn delete_individual() {
        let d = make();
        let id = d.start("https://x/", "x", "/x", "", None, 100).unwrap();
        d.delete(id).unwrap();
        assert!(d.get(id).unwrap().is_none());
    }

    #[test]
    fn cyrillic_filename_and_url() {
        let d = make();
        let id = d
            .start(
                "https://пример.рф/документ.pdf",
                "документ.pdf",
                "/Загрузки/документ.pdf",
                "application/pdf",
                None,
                100,
            )
            .unwrap();
        let e = d.get(id).unwrap().unwrap();
        assert_eq!(e.filename, "документ.pdf");
        assert_eq!(e.file_path, "/Загрузки/документ.pdf");
    }

    #[test]
    fn count_works() {
        let d = make();
        assert_eq!(d.count().unwrap(), 0);
        d.start("https://a/", "a", "/a", "", None, 100).unwrap();
        d.start("https://b/", "b", "/b", "", None, 200).unwrap();
        assert_eq!(d.count().unwrap(), 2);
    }

    #[test]
    fn total_size_optional() {
        let d = make();
        let id = d.start("https://x/", "x", "/x", "", None, 100).unwrap();
        let e = d.get(id).unwrap().unwrap();
        assert_eq!(e.total_size, None);
    }
}
