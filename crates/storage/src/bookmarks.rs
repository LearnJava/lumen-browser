//! Закладки (bookmarks) поверх SQLite. Каждая запись имеет url, title,
//! опциональную папку (folder) и список тегов.
//!
//! Схема:
//! ```sql
//! CREATE TABLE bookmarks (
//!     id          INTEGER PRIMARY KEY,
//!     url         TEXT NOT NULL UNIQUE,
//!     title       TEXT NOT NULL DEFAULT '',
//!     folder      TEXT NOT NULL DEFAULT '',  -- путь типа /Work/Projects
//!     created_at  INTEGER NOT NULL,           -- Unix timestamp
//!     note        TEXT NOT NULL DEFAULT '',  -- пользовательская заметка
//!     summary     TEXT,                       -- AI-саммари страницы (NULL = нет)
//!     embedding   BLOB                        -- AI-эмбеддинг summary, f32 LE (NULL = нет)
//! );
//! CREATE TABLE bookmark_tags (
//!     bookmark_id INTEGER NOT NULL,
//!     tag         TEXT NOT NULL,
//!     PRIMARY KEY (bookmark_id, tag),
//!     FOREIGN KEY (bookmark_id) REFERENCES bookmarks(id) ON DELETE CASCADE
//! ) WITHOUT ROWID;
//! ```
//!
//! Папка — это просто строка-путь (`/Work/Projects`); иерархия
//! интерпретируется UI-уровнем. `bookmark_tags` — many-to-many
//! связь bookmark ↔ tag; ON DELETE CASCADE автоматически чистит теги
//! при удалении закладки.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Одна закладка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub folder: String,
    pub created_at: i64,
    pub note: String,
    pub tags: Vec<String>,
    /// AI-саммари содержимого страницы (§12.8). `None` если AI-модуль не
    /// настроен или саммари ещё не вычислено — не путать с пустой строкой.
    pub summary: Option<String>,
    /// AI-эмбеддинг [`Self::summary`] — f32-вектор, little-endian байты
    /// (см. [`embedding_to_bytes`]/[`embedding_from_bytes`]). `None` в паре с
    /// `summary: None`.
    pub embedding: Option<Vec<u8>>,
}

pub struct Bookmarks {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Bookmarks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bookmarks").finish()
    }
}

impl Bookmarks {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("bookmarks open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("bookmarks open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS bookmarks (
                id         INTEGER PRIMARY KEY,
                url        TEXT NOT NULL UNIQUE,
                title      TEXT NOT NULL DEFAULT '',
                folder     TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                note       TEXT NOT NULL DEFAULT '',
                summary    TEXT,
                embedding  BLOB
            );
            CREATE TABLE IF NOT EXISTS bookmark_tags (
                bookmark_id INTEGER NOT NULL,
                tag         TEXT NOT NULL,
                PRIMARY KEY (bookmark_id, tag),
                FOREIGN KEY (bookmark_id) REFERENCES bookmarks(id) ON DELETE CASCADE
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS bookmark_folder_idx ON bookmarks(folder);
            CREATE INDEX IF NOT EXISTS bookmark_tag_idx ON bookmark_tags(tag);
            "#,
        )
        .map_err(|e| Error::Storage(format!("bookmarks init: {e}")))?;
        migrate_semantic_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Добавить или обновить закладку. Если url уже существует —
    /// обновляются title / folder / note (created_at сохраняется
    /// для оригинальной записи). Теги перезаписываются полностью.
    /// Возвращает id.
    pub fn add(
        &self,
        url: &str,
        title: &str,
        folder: &str,
        tags: &[String],
        note: &str,
        created_at: i64,
    ) -> Result<i64> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::Storage(format!("bookmarks tx: {e}")))?;
        tx.execute(
            "INSERT INTO bookmarks (url, title, folder, created_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (url) DO UPDATE SET
                 title = excluded.title,
                 folder = excluded.folder,
                 note = excluded.note",
            params![url, title, folder, created_at, note],
        )
        .map_err(|e| Error::Storage(format!("bookmarks add upsert: {e}")))?;
        // Получаем id — либо вставленной строки, либо существующей.
        let id: i64 = tx
            .query_row(
                "SELECT id FROM bookmarks WHERE url = ?1",
                params![url],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("bookmarks add lookup-id: {e}")))?;
        // Перезаписываем теги.
        tx.execute(
            "DELETE FROM bookmark_tags WHERE bookmark_id = ?1",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("bookmarks add delete-tags: {e}")))?;
        {
            let mut stmt = tx
                .prepare("INSERT INTO bookmark_tags (bookmark_id, tag) VALUES (?1, ?2)")
                .map_err(|e| Error::Storage(format!("bookmarks add prepare-tags: {e}")))?;
            // Дедупликация тегов (одинаковые в Vec — игнорируем повторы).
            let mut seen: HashSet<&str> = HashSet::new();
            for t in tags {
                if seen.insert(t.as_str()) {
                    stmt.execute(params![id, t])
                        .map_err(|e| Error::Storage(format!("bookmarks add tag: {e}")))?;
                }
            }
        }
        tx.commit()
            .map_err(|e| Error::Storage(format!("bookmarks commit: {e}")))?;
        Ok(id)
    }

    /// Получить закладку по url. None если нет.
    pub fn get(&self, url: &str) -> Result<Option<Bookmark>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT id, url, title, folder, created_at, note, summary, embedding
                 FROM bookmarks WHERE url = ?1",
                params![url],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<Vec<u8>>>(7)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Storage(format!("bookmarks get: {e}")))?;
        let Some((id, url, title, folder, created_at, note, summary, embedding)) = row else {
            return Ok(None);
        };
        let tags = fetch_tags(&conn, id)?;
        Ok(Some(Bookmark {
            id,
            url,
            title,
            folder,
            created_at,
            note,
            tags,
            summary,
            embedding,
        }))
    }

    /// Записать AI-саммари и эмбеддинг для закладки (§12.8, Step 6). No-op,
    /// если закладки с таким `url` нет. `None`/`None` очищает оба поля.
    pub fn set_semantic(
        &self,
        url: &str,
        summary: Option<&str>,
        embedding: Option<&[u8]>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        conn.execute(
            "UPDATE bookmarks SET summary = ?2, embedding = ?3 WHERE url = ?1",
            params![url, summary, embedding],
        )
        .map_err(|e| Error::Storage(format!("bookmarks set_semantic: {e}")))?;
        Ok(())
    }

    /// Удалить закладку (вместе с тегами благодаря ON DELETE CASCADE).
    pub fn delete(&self, url: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        conn.execute("DELETE FROM bookmarks WHERE url = ?1", params![url])
            .map_err(|e| Error::Storage(format!("bookmarks delete: {e}")))?;
        Ok(())
    }

    /// Все закладки, отсортированные по папке (ASC), затем по created_at DESC.
    ///
    /// Используется UI-панелью менеджера закладок для построения дерева папок
    /// и списка. Папка с пустой строкой (`""`, корень) сортируется первой.
    pub fn list_all(&self) -> Result<Vec<Bookmark>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        list_with_query(
            &conn,
            "SELECT id, url, title, folder, created_at, note, summary, embedding FROM bookmarks
             ORDER BY folder ASC, created_at DESC",
            params![],
        )
    }

    /// Переместить закладку в другую папку (DnD reorder в UI-панели).
    ///
    /// `folder` — новый путь папки (`""` = корень). Если закладки с таким `url`
    /// нет — no-op. Теги и created_at сохраняются.
    pub fn set_folder(&self, url: &str, folder: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        conn.execute(
            "UPDATE bookmarks SET folder = ?2 WHERE url = ?1",
            params![url, folder],
        )
        .map_err(|e| Error::Storage(format!("bookmarks set_folder: {e}")))?;
        Ok(())
    }

    /// Список закладок в данной папке (точное совпадение строки).
    /// Сортировка по created_at DESC.
    pub fn list_by_folder(&self, folder: &str) -> Result<Vec<Bookmark>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        list_with_query(
            &conn,
            "SELECT id, url, title, folder, created_at, note, summary, embedding FROM bookmarks
             WHERE folder = ?1 ORDER BY created_at DESC",
            params![folder],
        )
    }

    /// Список закладок с данным тегом. Сортировка по created_at DESC.
    pub fn list_by_tag(&self, tag: &str) -> Result<Vec<Bookmark>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        list_with_query(
            &conn,
            "SELECT b.id, b.url, b.title, b.folder, b.created_at, b.note, b.summary, b.embedding
             FROM bookmarks b
             JOIN bookmark_tags t ON t.bookmark_id = b.id
             WHERE t.tag = ?1
             ORDER BY b.created_at DESC",
            params![tag],
        )
    }

    /// Все уникальные теги в системе (для UI tag-cloud / autocomplete).
    pub fn all_tags(&self) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached("SELECT DISTINCT tag FROM bookmark_tags ORDER BY tag")
            .map_err(|e| Error::Storage(format!("bookmarks all_tags prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Storage(format!("bookmarks all_tags query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("bookmarks tag row: {e}")))?);
        }
        Ok(out)
    }

    /// Все уникальные папки.
    pub fn all_folders(&self) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT folder FROM bookmarks WHERE folder != '' ORDER BY folder",
            )
            .map_err(|e| Error::Storage(format!("bookmarks all_folders prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| Error::Storage(format!("bookmarks all_folders query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("bookmarks folder row: {e}")))?);
        }
        Ok(out)
    }

    /// Общее число закладок.
    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("bookmarks mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM bookmarks", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("bookmarks count: {e}")))?;
        Ok(n)
    }
}

/// Adds `summary`/`embedding` columns to a `bookmarks` table created before
/// Step 6 (§12.8). `CREATE TABLE IF NOT EXISTS` above only covers fresh
/// databases — existing on-disk files need an explicit `ALTER TABLE`.
fn migrate_semantic_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(bookmarks)")
        .map_err(|e| Error::Storage(format!("bookmarks migrate prepare: {e}")))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| Error::Storage(format!("bookmarks migrate query: {e}")))?;
    let mut columns: HashSet<String> = HashSet::new();
    for r in rows {
        columns.insert(r.map_err(|e| Error::Storage(format!("bookmarks migrate row: {e}")))?);
    }
    drop(stmt);
    if !columns.contains("summary") {
        conn.execute("ALTER TABLE bookmarks ADD COLUMN summary TEXT", [])
            .map_err(|e| Error::Storage(format!("bookmarks migrate add summary: {e}")))?;
    }
    if !columns.contains("embedding") {
        conn.execute("ALTER TABLE bookmarks ADD COLUMN embedding BLOB", [])
            .map_err(|e| Error::Storage(format!("bookmarks migrate add embedding: {e}")))?;
    }
    Ok(())
}

/// Serialises an embedding vector to little-endian bytes for BLOB storage.
/// Paired with [`embedding_from_bytes`]; see [`Bookmark::embedding`].
pub fn embedding_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialises bytes produced by [`embedding_to_bytes`] back into an `f32` vector.
/// A trailing partial `f32` (byte count not a multiple of 4) is dropped.
pub fn embedding_from_bytes(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity between two embeddings, for semantic-bookmark ranking
/// (§12.8, Step 6). Returns `0.0` for empty/mismatched-length inputs or when
/// either vector has zero norm, so callers can treat it as "no match"
/// without a separate validity check.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn fetch_tags(conn: &Connection, bookmark_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT tag FROM bookmark_tags WHERE bookmark_id = ?1 ORDER BY tag",
        )
        .map_err(|e| Error::Storage(format!("bookmarks fetch_tags prepare: {e}")))?;
    let rows = stmt
        .query_map(params![bookmark_id], |r| r.get::<_, String>(0))
        .map_err(|e| Error::Storage(format!("bookmarks fetch_tags query: {e}")))?;
    let mut tags = Vec::new();
    for r in rows {
        tags.push(r.map_err(|e| Error::Storage(format!("bookmarks tag row: {e}")))?);
    }
    Ok(tags)
}

fn list_with_query(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Bookmark>> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| Error::Storage(format!("bookmarks list prepare: {e}")))?;
    let rows = stmt
        .query_map(params, |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<Vec<u8>>>(7)?,
            ))
        })
        .map_err(|e| Error::Storage(format!("bookmarks list query: {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        let (id, url, title, folder, created_at, note, summary, embedding) =
            r.map_err(|e| Error::Storage(format!("bookmarks list row: {e}")))?;
        let tags = fetch_tags(conn, id)?;
        out.push(Bookmark {
            id,
            url,
            title,
            folder,
            created_at,
            note,
            tags,
            summary,
            embedding,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Bookmarks {
        Bookmarks::open_in_memory().unwrap()
    }

    #[test]
    fn add_basic_bookmark() {
        let b = make();
        let id = b
            .add("https://rust-lang.org/", "Rust", "", &[], "", 100)
            .unwrap();
        assert!(id > 0);
        let got = b.get("https://rust-lang.org/").unwrap().unwrap();
        assert_eq!(got.title, "Rust");
        assert_eq!(got.folder, "");
        assert!(got.tags.is_empty());
        assert_eq!(got.created_at, 100);
    }

    #[test]
    fn add_with_tags_and_folder() {
        let b = make();
        b.add(
            "https://example.com/",
            "Example",
            "/Reading",
            &["read-later".into(), "tech".into()],
            "interesting article",
            200,
        )
        .unwrap();
        let got = b.get("https://example.com/").unwrap().unwrap();
        assert_eq!(got.folder, "/Reading");
        assert_eq!(got.tags, vec!["read-later".to_string(), "tech".to_string()]);
        assert_eq!(got.note, "interesting article");
    }

    #[test]
    fn add_duplicate_url_updates_in_place() {
        let b = make();
        b.add("https://x/", "Old", "/A", &["tag1".into()], "", 100).unwrap();
        b.add("https://x/", "New", "/B", &["tag2".into()], "n", 200).unwrap();
        let got = b.get("https://x/").unwrap().unwrap();
        assert_eq!(got.title, "New");
        assert_eq!(got.folder, "/B");
        // created_at сохраняется от первой записи.
        assert_eq!(got.created_at, 100);
        // Теги перезаписываются.
        assert_eq!(got.tags, vec!["tag2".to_string()]);
        assert_eq!(got.note, "n");
        assert_eq!(b.count().unwrap(), 1);
    }

    #[test]
    fn duplicate_tags_in_input_deduplicated() {
        let b = make();
        b.add(
            "https://x/",
            "t",
            "",
            &["a".into(), "b".into(), "a".into()],
            "",
            100,
        )
        .unwrap();
        let got = b.get("https://x/").unwrap().unwrap();
        // `a` встретился дважды — должен быть один.
        assert_eq!(got.tags.iter().filter(|t| *t == "a").count(), 1);
        assert_eq!(got.tags.len(), 2);
    }

    #[test]
    fn delete_cascades_to_tags() {
        let b = make();
        b.add(
            "https://x/",
            "t",
            "",
            &["tag1".into(), "tag2".into()],
            "",
            100,
        )
        .unwrap();
        b.delete("https://x/").unwrap();
        // bookmark пропал.
        assert!(b.get("https://x/").unwrap().is_none());
        // и теги тоже пропали из глобального списка.
        assert!(b.all_tags().unwrap().is_empty());
    }

    #[test]
    fn list_by_folder() {
        let b = make();
        b.add("https://a/", "A", "/Work", &[], "", 100).unwrap();
        b.add("https://b/", "B", "/Work", &[], "", 200).unwrap();
        b.add("https://c/", "C", "/Personal", &[], "", 300).unwrap();
        let work = b.list_by_folder("/Work").unwrap();
        assert_eq!(work.len(), 2);
        // DESC by created_at: B (200), A (100).
        assert_eq!(work[0].url, "https://b/");
        assert_eq!(work[1].url, "https://a/");
        let personal = b.list_by_folder("/Personal").unwrap();
        assert_eq!(personal.len(), 1);
        assert_eq!(personal[0].url, "https://c/");
    }

    #[test]
    fn list_by_tag() {
        let b = make();
        b.add("https://a/", "A", "", &["rust".into()], "", 100).unwrap();
        b.add("https://b/", "B", "", &["rust".into(), "web".into()], "", 200)
            .unwrap();
        b.add("https://c/", "C", "", &["web".into()], "", 300).unwrap();
        let rust = b.list_by_tag("rust").unwrap();
        assert_eq!(rust.len(), 2);
        let urls: Vec<&str> = rust.iter().map(|b| b.url.as_str()).collect();
        assert!(urls.contains(&"https://a/"));
        assert!(urls.contains(&"https://b/"));

        let web = b.list_by_tag("web").unwrap();
        assert_eq!(web.len(), 2);
    }

    #[test]
    fn all_tags_returns_distinct_sorted() {
        let b = make();
        b.add("https://a/", "A", "", &["c".into(), "a".into()], "", 100).unwrap();
        b.add("https://b/", "B", "", &["a".into(), "b".into()], "", 200).unwrap();
        let tags = b.all_tags().unwrap();
        assert_eq!(tags, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn all_folders_skips_empty() {
        let b = make();
        // Folder="" (нет папки) не должен попасть в all_folders.
        b.add("https://root/", "Root", "", &[], "", 100).unwrap();
        b.add("https://a/", "A", "/Folder1", &[], "", 200).unwrap();
        b.add("https://b/", "B", "/Folder2", &[], "", 300).unwrap();
        let folders = b.all_folders().unwrap();
        assert_eq!(folders, vec!["/Folder1".to_string(), "/Folder2".to_string()]);
    }

    #[test]
    fn count_total_bookmarks() {
        let b = make();
        assert_eq!(b.count().unwrap(), 0);
        b.add("https://a/", "A", "", &[], "", 100).unwrap();
        b.add("https://b/", "B", "", &[], "", 200).unwrap();
        assert_eq!(b.count().unwrap(), 2);
    }

    #[test]
    fn cyrillic_url_title_tags() {
        let b = make();
        b.add(
            "https://пример.рф/статья",
            "Главная статья",
            "/Чтение",
            &["русский".into(), "технологии".into()],
            "интересно",
            100,
        )
        .unwrap();
        let got = b.get("https://пример.рф/статья").unwrap().unwrap();
        assert_eq!(got.title, "Главная статья");
        assert_eq!(got.folder, "/Чтение");
        assert!(got.tags.contains(&"русский".to_string()));
    }

    #[test]
    fn list_all_sorted_by_folder_then_recency() {
        let b = make();
        b.add("https://a/", "A", "/Work", &[], "", 100).unwrap();
        b.add("https://b/", "B", "/Work", &[], "", 200).unwrap();
        b.add("https://root/", "Root", "", &[], "", 50).unwrap();
        b.add("https://c/", "C", "/Personal", &[], "", 300).unwrap();
        let all = b.list_all().unwrap();
        let order: Vec<&str> = all.iter().map(|x| x.url.as_str()).collect();
        // "" (root) first, then /Personal, then /Work; within folder DESC by time.
        assert_eq!(
            order,
            vec!["https://root/", "https://c/", "https://b/", "https://a/"]
        );
    }

    #[test]
    fn set_folder_moves_bookmark() {
        let b = make();
        b.add("https://x/", "X", "/Old", &["t".into()], "n", 100).unwrap();
        b.set_folder("https://x/", "/New").unwrap();
        let got = b.get("https://x/").unwrap().unwrap();
        assert_eq!(got.folder, "/New");
        // Tags, note and created_at survive the move.
        assert_eq!(got.tags, vec!["t".to_string()]);
        assert_eq!(got.note, "n");
        assert_eq!(got.created_at, 100);
    }

    #[test]
    fn set_folder_missing_url_noop() {
        let b = make();
        // Must not error when the url is absent.
        b.set_folder("https://nope/", "/X").unwrap();
        assert_eq!(b.count().unwrap(), 0);
    }

    #[test]
    fn get_missing_returns_none() {
        let b = make();
        assert!(b.get("https://nope/").unwrap().is_none());
    }

    #[test]
    fn delete_missing_noop() {
        let b = make();
        b.delete("https://nope/").unwrap();
    }

    // ── Semantic fields (§12.8, Step 6) ─────────────────────────────────────

    #[test]
    fn bookmark_without_semantic_fields_has_no_schema_error() {
        let b = make();
        b.add("https://y/", "Y", "", &[], "", 100).unwrap();
        let got = b.get("https://y/").unwrap().unwrap();
        assert!(got.summary.is_none());
        assert!(got.embedding.is_none());
    }

    #[test]
    fn set_semantic_round_trips_summary_and_embedding() {
        let b = make();
        b.add("https://x/", "X", "", &[], "", 100).unwrap();
        let emb = embedding_to_bytes(&[0.1, 0.2, 0.3]);
        b.set_semantic("https://x/", Some("a short summary"), Some(&emb))
            .unwrap();
        let got = b.get("https://x/").unwrap().unwrap();
        assert_eq!(got.summary.as_deref(), Some("a short summary"));
        assert_eq!(
            embedding_from_bytes(got.embedding.as_deref().unwrap()),
            vec![0.1, 0.2, 0.3]
        );
    }

    #[test]
    fn set_semantic_missing_url_noop() {
        let b = make();
        b.set_semantic("https://nope/", Some("s"), None).unwrap();
        assert_eq!(b.count().unwrap(), 0);
    }

    #[test]
    fn embedding_bytes_roundtrip() {
        let v = vec![1.0f32, -2.5, 0.0, 3.75];
        let bytes = embedding_to_bytes(&v);
        assert_eq!(bytes.len(), v.len() * 4);
        assert_eq!(embedding_from_bytes(&bytes), v);
    }

    #[test]
    fn cosine_similarity_identical_vectors_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal_is_zero() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_similarity_mismatched_length_is_zero() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    }

    #[test]
    fn migrate_adds_missing_columns_to_legacy_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE bookmarks (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL DEFAULT '',
                folder TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                note TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        migrate_semantic_columns(&conn).unwrap();
        // Re-running on an already-migrated schema must not error.
        migrate_semantic_columns(&conn).unwrap();
        conn.execute(
            "INSERT INTO bookmarks (url, created_at, summary, embedding) VALUES (?1, 1, ?2, ?3)",
            params!["https://z/", "sum", vec![1u8, 2, 3, 4]],
        )
        .unwrap();
    }
}
