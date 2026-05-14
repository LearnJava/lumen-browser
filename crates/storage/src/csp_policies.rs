//! Content-Security-Policy parser + per-origin store.
//!
//! Spec: <https://www.w3.org/TR/CSP3/>. CSP-заголовок — список
//! `directive source ...; directive source ...`. Каждая directive (например,
//! `script-src`, `style-src`, `default-src`) задаёт список разрешённых
//! источников.
//!
//! Phase 0: парсер директив + storage per-origin. Реальное enforcement
//! в network (отклонять fetch не из source-list) — отдельная задача,
//! требует hook в `HttpClient::fetch_with_redirect`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Парсит CSP-заголовок в map `directive → sources`.
/// Directive имена — ASCII case-insensitive (нормализуются в lower-case);
/// source-значения сохраняются case-sensitive (URLs / scheme).
///
/// Грамматика (упрощённая, по CSP3 §3): `policy = directive *(";" directive)`,
/// `directive = directive-name 1*WSP directive-value`, `directive-value =
/// *(token | quoted-string)` через whitespace.
///
/// Пустые директивы (`directive ;` без value) сохраняются как `Vec::new()`.
pub fn parse_csp_header(text: &str) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for piece in text.split(';') {
        let mut parts = piece.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let name_lc = name.to_ascii_lowercase();
        let sources: Vec<String> = parts.map(|s| s.to_string()).collect();
        // CSP3: первая встреченная директива побеждает (для дубликатов),
        // последующие игнорируются. Сохраняем только если ещё не было.
        out.entry(name_lc).or_insert(sources);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspPolicy {
    pub origin: String,
    /// Сырая строка заголовка (для дебага и round-trip).
    pub header_text: String,
    /// Разобранные директивы. Имена в lower-case.
    pub directives: HashMap<String, Vec<String>>,
    pub fetched_at: i64,
}

pub struct CspPolicies {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for CspPolicies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CspPolicies").finish()
    }
}

impl CspPolicies {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("csp_policies open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("csp_policies open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS csp_policies (
                origin       TEXT PRIMARY KEY,
                header_text  TEXT NOT NULL,
                fetched_at   INTEGER NOT NULL
            ) WITHOUT ROWID;
            "#,
        )
        .map_err(|e| Error::Storage(format!("csp_policies init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn store(&self, origin: &str, header_text: &str, fetched_at: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("csp_policies mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO csp_policies (origin, header_text, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (origin) DO UPDATE SET
                 header_text = excluded.header_text,
                 fetched_at = excluded.fetched_at",
            params![origin, header_text, fetched_at],
        )
        .map_err(|e| Error::Storage(format!("csp_policies store: {e}")))?;
        Ok(())
    }

    pub fn get(&self, origin: &str) -> Result<Option<CspPolicy>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("csp_policies mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT origin, header_text, fetched_at FROM csp_policies WHERE origin = ?1",
                params![origin],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Storage(format!("csp_policies get: {e}")))?;
        Ok(row.map(|(origin, header_text, fetched_at)| {
            let directives = parse_csp_header(&header_text);
            CspPolicy {
                origin,
                header_text,
                directives,
                fetched_at,
            }
        }))
    }

    pub fn delete(&self, origin: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("csp_policies mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM csp_policies WHERE origin = ?1",
            params![origin],
        )
        .map_err(|e| Error::Storage(format!("csp_policies delete: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("csp_policies mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM csp_policies", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("csp_policies count: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_csp_directives() {
        let d = parse_csp_header("default-src 'self'; script-src 'self' https://cdn.example");
        assert_eq!(d.get("default-src").unwrap(), &vec!["'self'".to_string()]);
        assert_eq!(
            d.get("script-src").unwrap(),
            &vec!["'self'".to_string(), "https://cdn.example".to_string()]
        );
    }

    #[test]
    fn parse_csp_lowercases_directive_names() {
        let d = parse_csp_header("DEFAULT-SRC 'self'");
        assert!(d.contains_key("default-src"));
        assert!(!d.contains_key("DEFAULT-SRC"));
    }

    #[test]
    fn parse_csp_preserves_source_case() {
        let d = parse_csp_header("img-src https://CDN.EXAMPLE");
        assert_eq!(d.get("img-src").unwrap(), &vec!["https://CDN.EXAMPLE".to_string()]);
    }

    #[test]
    fn parse_csp_empty_directive() {
        let d = parse_csp_header("default-src;");
        assert!(d.get("default-src").unwrap().is_empty());
    }

    #[test]
    fn parse_csp_first_duplicate_wins() {
        let d = parse_csp_header("default-src 'self'; default-src 'none'");
        assert_eq!(d.get("default-src").unwrap(), &vec!["'self'".to_string()]);
    }

    #[test]
    fn parse_csp_extra_whitespace_normalized() {
        let d = parse_csp_header("  default-src   'self'  ;   img-src  *  ");
        assert_eq!(d.get("default-src").unwrap(), &vec!["'self'".to_string()]);
        assert_eq!(d.get("img-src").unwrap(), &vec!["*".to_string()]);
    }

    #[test]
    fn store_and_get_round_trip() {
        let s = CspPolicies::open_in_memory().unwrap();
        s.store("https://a/", "default-src 'self'", 100).unwrap();
        let p = s.get("https://a/").unwrap().unwrap();
        assert_eq!(p.header_text, "default-src 'self'");
        assert!(p.directives.contains_key("default-src"));
    }

    #[test]
    fn store_overwrites() {
        let s = CspPolicies::open_in_memory().unwrap();
        s.store("https://a/", "default-src 'self'", 100).unwrap();
        s.store("https://a/", "default-src 'none'", 200).unwrap();
        let p = s.get("https://a/").unwrap().unwrap();
        assert_eq!(p.header_text, "default-src 'none'");
        assert_eq!(p.fetched_at, 200);
    }

    #[test]
    fn delete_removes() {
        let s = CspPolicies::open_in_memory().unwrap();
        s.store("https://a/", "default-src 'self'", 100).unwrap();
        s.delete("https://a/").unwrap();
        assert!(s.get("https://a/").unwrap().is_none());
    }

    #[test]
    fn count_works() {
        let s = CspPolicies::open_in_memory().unwrap();
        assert_eq!(s.count().unwrap(), 0);
        s.store("https://a/", "default-src 'self'", 100).unwrap();
        s.store("https://b/", "default-src 'none'", 200).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }
}
