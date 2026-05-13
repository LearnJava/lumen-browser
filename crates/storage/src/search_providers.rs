//! Search providers — поисковые движки для omnibox. Каждая запись:
//! имя, шаблон URL с placeholder `{query}`, опц. icon-URL, default-флаг.
//!
//! `SearchProviderEntry` имплементирует `lumen_core::ext::SearchProvider`
//! через `query_url(query) -> Url`, который подставляет URL-encoded query
//! на место `{query}`. Хранилище — SqliteStorage.
//!
//! Phase 0 покрывает storage + минимальный URL-encoding. Suggest API
//! (типа OpenSearch autocomplete) — отдельная задача.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::ext::SearchProvider;
use lumen_core::url::Url;
use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Один поисковый провайдер.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchProviderEntry {
    pub id: i64,
    pub name: String,
    /// URL-template с placeholder `{query}`, например
    /// `https://duckduckgo.com/?q={query}`.
    pub url_template: String,
    /// Опциональный URL favicon-ки / иконки провайдера.
    pub icon_url: Option<String>,
    pub is_default: bool,
}

impl SearchProviderEntry {
    /// Подставить query на место `{query}` с URL-encoding по RFC 3986
    /// (encode-everything-except unreserved). Получить итоговый
    /// `lumen_core::url::Url`. Если template не содержит `{query}`,
    /// query просто игнорируется (template используется как есть).
    pub fn build_url(&self, query: &str) -> std::result::Result<Url, lumen_core::Error> {
        let encoded = url_encode_query(query);
        let resolved = self.url_template.replace("{query}", &encoded);
        Url::parse(&resolved)
    }
}

impl SearchProvider for SearchProviderEntry {
    fn name(&self) -> &str {
        &self.name
    }

    fn query_url(&self, query: &str) -> Url {
        // Trait требует Url по значению — fallback на пустой URL при
        // ошибке парсинга (не должно случаться при валидном template).
        self.build_url(query).unwrap_or_else(|_| {
            Url::parse("about:blank").unwrap_or_else(|_| {
                // Гарантированно валидная заглушка.
                Url::parse("http://invalid/").expect("about:blank fallback")
            })
        })
    }
}

/// URL-encode query string по RFC 3986: оставляем нетронутыми
/// `unreserved = A-Z a-z 0-9 - _ . ~`; остальное — percent-encode.
fn url_encode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len() + query.len() / 4);
    for byte in query.as_bytes() {
        let c = *byte;
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push('%');
            out.push(HEX[(c >> 4) as usize] as char);
            out.push(HEX[(c & 0x0F) as usize] as char);
        }
    }
    out
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Реестр поисковых провайдеров.
pub struct SearchProviders {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SearchProviders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchProviders").finish()
    }
}

impl SearchProviders {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("search_providers open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("search_providers open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS search_providers (
                id           INTEGER PRIMARY KEY,
                name         TEXT NOT NULL UNIQUE,
                url_template TEXT NOT NULL,
                icon_url     TEXT,
                created_at   INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS default_search_provider (
                lock INTEGER PRIMARY KEY CHECK (lock = 0),
                provider_id INTEGER,
                FOREIGN KEY (provider_id) REFERENCES search_providers(id)
                    ON DELETE SET NULL
            );
            INSERT OR IGNORE INTO default_search_provider (lock, provider_id)
                VALUES (0, NULL);
            "#,
        )
        .map_err(|e| Error::Storage(format!("search_providers init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Добавить провайдера. Имя уникально.
    pub fn add(
        &self,
        name: &str,
        url_template: &str,
        icon_url: Option<&str>,
    ) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO search_providers (name, url_template, icon_url) VALUES (?1, ?2, ?3)",
            params![name, url_template, icon_url],
        )
        .map_err(|e| Error::Storage(format!("search_providers add: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    /// Получить провайдера по id.
    pub fn get(&self, id: i64) -> Result<Option<SearchProviderEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        let default_id = default_id_locked(&conn)?;
        let row = conn
            .query_row(
                "SELECT id, name, url_template, icon_url FROM search_providers WHERE id = ?1",
                params![id],
                |r| row_to_entry(r, default_id),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("search_providers get: {e}")))?;
        Ok(row)
    }

    pub fn get_by_name(&self, name: &str) -> Result<Option<SearchProviderEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        let default_id = default_id_locked(&conn)?;
        let row = conn
            .query_row(
                "SELECT id, name, url_template, icon_url FROM search_providers WHERE name = ?1",
                params![name],
                |r| row_to_entry(r, default_id),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("search_providers get_by_name: {e}")))?;
        Ok(row)
    }

    /// Все провайдеры в порядке создания.
    pub fn list_all(&self) -> Result<Vec<SearchProviderEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        let default_id = default_id_locked(&conn)?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, name, url_template, icon_url FROM search_providers
                 ORDER BY id ASC",
            )
            .map_err(|e| Error::Storage(format!("search_providers list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| row_to_entry(r, default_id))
            .map_err(|e| Error::Storage(format!("search_providers list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Storage(format!("search_providers row: {e}")))?);
        }
        Ok(out)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| Error::Storage(format!("search_providers pragma: {e}")))?;
        conn.execute("DELETE FROM search_providers WHERE id = ?1", params![id])
            .map_err(|e| Error::Storage(format!("search_providers delete: {e}")))?;
        Ok(())
    }

    pub fn set_default(&self, id: Option<i64>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        if let Some(pid) = id {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM search_providers WHERE id = ?1",
                    params![pid],
                    |r| r.get(0),
                )
                .map_err(|e| Error::Storage(format!("search_providers set_default check: {e}")))?;
            if exists == 0 {
                return Err(Error::NotFound(format!("search provider id {pid}")));
            }
        }
        conn.execute(
            "UPDATE default_search_provider SET provider_id = ?1 WHERE lock = 0",
            params![id],
        )
        .map_err(|e| Error::Storage(format!("search_providers set_default: {e}")))?;
        Ok(())
    }

    pub fn default(&self) -> Result<Option<SearchProviderEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        let id = match default_id_locked(&conn)? {
            Some(i) => i,
            None => return Ok(None),
        };
        let entry = conn
            .query_row(
                "SELECT id, name, url_template, icon_url FROM search_providers WHERE id = ?1",
                params![id],
                |r| row_to_entry(r, Some(id)),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("search_providers default: {e}")))?;
        Ok(entry)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("search_providers mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM search_providers", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("search_providers count: {e}")))?;
        Ok(n)
    }
}

fn default_id_locked(conn: &Connection) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT provider_id FROM default_search_provider WHERE lock = 0",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )
    .map_err(|e| Error::Storage(format!("search_providers default_id: {e}")))
}

fn row_to_entry(
    row: &rusqlite::Row<'_>,
    default_id: Option<i64>,
) -> rusqlite::Result<SearchProviderEntry> {
    let id: i64 = row.get(0)?;
    Ok(SearchProviderEntry {
        id,
        name: row.get(1)?,
        url_template: row.get(2)?,
        icon_url: row.get(3)?,
        is_default: Some(id) == default_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> SearchProviders {
        SearchProviders::open_in_memory().unwrap()
    }

    #[test]
    fn add_and_get() {
        let s = make();
        let id = s
            .add("DuckDuckGo", "https://duckduckgo.com/?q={query}", None)
            .unwrap();
        let p = s.get(id).unwrap().unwrap();
        assert_eq!(p.name, "DuckDuckGo");
        assert_eq!(p.url_template, "https://duckduckgo.com/?q={query}");
        assert!(p.icon_url.is_none());
        assert!(!p.is_default);
    }

    #[test]
    fn add_with_icon() {
        let s = make();
        let id = s
            .add(
                "Yandex",
                "https://yandex.ru/search/?text={query}",
                Some("https://yandex.ru/favicon.ico"),
            )
            .unwrap();
        let p = s.get(id).unwrap().unwrap();
        assert_eq!(p.icon_url, Some("https://yandex.ru/favicon.ico".to_string()));
    }

    #[test]
    fn duplicate_name_fails() {
        let s = make();
        s.add("X", "https://x/?q={query}", None).unwrap();
        assert!(s.add("X", "https://y/?q={query}", None).is_err());
    }

    #[test]
    fn list_all_includes_all_providers() {
        let s = make();
        s.add("A", "https://a/?q={query}", None).unwrap();
        s.add("B", "https://b/?q={query}", None).unwrap();
        let all = s.list_all().unwrap();
        assert_eq!(all.len(), 2);
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    #[test]
    fn set_default_marks_provider() {
        let s = make();
        let id_a = s.add("A", "https://a/?q={query}", None).unwrap();
        let id_b = s.add("B", "https://b/?q={query}", None).unwrap();
        s.set_default(Some(id_a)).unwrap();
        let pa = s.get(id_a).unwrap().unwrap();
        let pb = s.get(id_b).unwrap().unwrap();
        assert!(pa.is_default);
        assert!(!pb.is_default);
        assert_eq!(s.default().unwrap().unwrap().id, id_a);
    }

    #[test]
    fn set_default_none_clears() {
        let s = make();
        let id = s.add("A", "https://a/?q={query}", None).unwrap();
        s.set_default(Some(id)).unwrap();
        s.set_default(None).unwrap();
        assert!(s.default().unwrap().is_none());
    }

    #[test]
    fn delete_default_clears_default() {
        let s = make();
        let id = s.add("A", "https://a/?q={query}", None).unwrap();
        s.set_default(Some(id)).unwrap();
        s.delete(id).unwrap();
        // FK ON DELETE SET NULL.
        assert!(s.default().unwrap().is_none());
    }

    #[test]
    fn build_url_basic() {
        let p = SearchProviderEntry {
            id: 1,
            name: "DDG".into(),
            url_template: "https://duckduckgo.com/?q={query}".into(),
            icon_url: None,
            is_default: false,
        };
        let url = p.build_url("rust lang").unwrap();
        assert!(url.as_str().contains("?q=rust%20lang"));
    }

    #[test]
    fn build_url_special_chars() {
        // & / # ? = и проч. должны быть URL-encoded.
        let p = SearchProviderEntry {
            id: 1,
            name: "x".into(),
            url_template: "https://x/?q={query}".into(),
            icon_url: None,
            is_default: false,
        };
        let url = p.build_url("a&b=c").unwrap();
        assert!(url.as_str().contains("?q=a%26b%3Dc"));
    }

    #[test]
    fn build_url_cyrillic() {
        let p = SearchProviderEntry {
            id: 1,
            name: "x".into(),
            url_template: "https://x/?q={query}".into(),
            icon_url: None,
            is_default: false,
        };
        let url = p.build_url("привет").unwrap();
        // UTF-8 bytes "привет" = D0 BF D1 80 D0 B8 D0 B2 D0 B5 D1 82.
        assert!(url
            .as_str()
            .contains("?q=%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82"));
    }

    #[test]
    fn url_encode_preserves_unreserved() {
        assert_eq!(url_encode_query("abc-_.~"), "abc-_.~");
        assert_eq!(url_encode_query("A1z9"), "A1z9");
    }

    #[test]
    fn url_encode_encodes_space() {
        assert_eq!(url_encode_query(" "), "%20");
    }

    #[test]
    fn search_provider_trait_works() {
        let p = SearchProviderEntry {
            id: 1,
            name: "Google".into(),
            url_template: "https://google.com/?q={query}".into(),
            icon_url: None,
            is_default: false,
        };
        let dyn_provider: &dyn SearchProvider = &p;
        assert_eq!(dyn_provider.name(), "Google");
        let url = dyn_provider.query_url("test");
        assert_eq!(url.as_str(), "https://google.com/?q=test");
    }

    #[test]
    fn count_works() {
        let s = make();
        assert_eq!(s.count().unwrap(), 0);
        s.add("A", "https://a/?q={query}", None).unwrap();
        s.add("B", "https://b/?q={query}", None).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }
}
