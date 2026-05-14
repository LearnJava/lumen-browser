//! Permissions-Policy (W3C) parser + per-origin storage.
//!
//! Spec: <https://www.w3.org/TR/permissions-policy-1/>. Заменяет старый
//! Feature-Policy header. Синтаксис: список feature-allowlist пар, через
//! запятую: `geolocation=(self), camera=*, microphone=()`.
//!
//! Allowlist:
//! - `*` — все origins;
//! - `()` — nothing (feature заблокирована);
//! - `(self)` — same-origin;
//! - `(self "https://example.com")` — list разрешённых origins;
//! - `(src)` — для iframe-policy.
//!
//! Phase 0: парсер + хранение. Реальное enforcement (отказ в JS API
//! вызовах если feature заблокирована) — отдельная задача (Phase 3+).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Allowlist для одной feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionsAllowlist {
    /// `*` — все origins.
    All,
    /// `()` — заблокировано.
    None,
    /// `(self)` / `(src)` / list. `self`, `src`, и URL-источники
    /// хранятся как строки в порядке появления.
    Origins(Vec<String>),
}

impl PermissionsAllowlist {
    /// `true` если allowlist пуст (`()` или `Origins(vec![])`).
    pub fn is_blocked(&self) -> bool {
        match self {
            Self::None => true,
            Self::Origins(v) => v.is_empty(),
            Self::All => false,
        }
    }

    /// `true` если разрешено для текущего origin (`(self)` или `*`).
    pub fn allows_self(&self) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Origins(v) => v.iter().any(|s| s.eq_ignore_ascii_case("self") || s == "*"),
        }
    }
}

/// Парсит Permissions-Policy header.
/// Возвращает map `feature → allowlist`. Duplicate feature → первое
/// значение побеждает (per spec § 9.4 «merging policy directives»).
pub fn parse_permissions_policy(text: &str) -> HashMap<String, PermissionsAllowlist> {
    let mut out: HashMap<String, PermissionsAllowlist> = HashMap::new();
    for piece in split_top_level_commas(text) {
        let p = piece.trim();
        if p.is_empty() {
            continue;
        }
        // feature=value form.
        let Some(eq) = p.find('=') else {
            continue;
        };
        let name = p[..eq].trim().to_ascii_lowercase();
        let value = p[eq + 1..].trim();
        if name.is_empty() {
            continue;
        }
        let allowlist = parse_allowlist(value);
        out.entry(name).or_insert(allowlist);
    }
    out
}

/// Разбивает строку по `,` на верхнем уровне (с уважением к `()`).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start <= s.len() {
        out.push(&s[start..]);
    }
    out
}

fn parse_allowlist(value: &str) -> PermissionsAllowlist {
    let v = value.trim();
    if v == "*" {
        return PermissionsAllowlist::All;
    }
    // `()` или `(a b c)` — список.
    let inner = if let Some(stripped) = v.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        stripped.trim()
    } else {
        // Bare keyword (без скобок) — спека L2 допускает (`feature=self`).
        v
    };
    if inner.is_empty() {
        return PermissionsAllowlist::None;
    }
    // Разбиваем по whitespace, снимаем кавычки.
    let origins: Vec<String> = inner
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| c == '"' || c == '\'').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    PermissionsAllowlist::Origins(origins)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsPolicy {
    pub origin: String,
    /// Сырой header text — для round-trip и debug.
    pub header_text: String,
    /// Разобранные allowlist-ы. feature-имена в lower-case.
    pub allowlists: HashMap<String, PermissionsAllowlist>,
    pub fetched_at: i64,
}

pub struct PermissionsPolicies {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for PermissionsPolicies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionsPolicies").finish()
    }
}

impl PermissionsPolicies {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("permissions_policy open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("permissions_policy open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS permissions_policies (
                origin       TEXT PRIMARY KEY,
                header_text  TEXT NOT NULL,
                fetched_at   INTEGER NOT NULL
            ) WITHOUT ROWID;
            "#,
        )
        .map_err(|e| Error::Storage(format!("permissions_policy init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn store(&self, origin: &str, header_text: &str, fetched_at: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions_policy mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO permissions_policies (origin, header_text, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (origin) DO UPDATE SET
                 header_text = excluded.header_text,
                 fetched_at = excluded.fetched_at",
            params![origin, header_text, fetched_at],
        )
        .map_err(|e| Error::Storage(format!("permissions_policy store: {e}")))?;
        Ok(())
    }

    pub fn get(&self, origin: &str) -> Result<Option<PermissionsPolicy>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions_policy mutex poisoned".into()))?;
        let row = conn
            .query_row(
                "SELECT origin, header_text, fetched_at FROM permissions_policies WHERE origin = ?1",
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
            .map_err(|e| Error::Storage(format!("permissions_policy get: {e}")))?;
        Ok(row.map(|(origin, header_text, fetched_at)| {
            let allowlists = parse_permissions_policy(&header_text);
            PermissionsPolicy {
                origin,
                header_text,
                allowlists,
                fetched_at,
            }
        }))
    }

    pub fn delete(&self, origin: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions_policy mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM permissions_policies WHERE origin = ?1",
            params![origin],
        )
        .map_err(|e| Error::Storage(format!("permissions_policy delete: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("permissions_policy mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM permissions_policies", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("permissions_policy count: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_form() {
        let p = parse_permissions_policy("camera=*");
        assert_eq!(p.get("camera").unwrap(), &PermissionsAllowlist::All);
    }

    #[test]
    fn parse_none_form() {
        let p = parse_permissions_policy("camera=()");
        assert_eq!(p.get("camera").unwrap(), &PermissionsAllowlist::None);
    }

    #[test]
    fn parse_self_form() {
        let p = parse_permissions_policy("geolocation=(self)");
        match p.get("geolocation").unwrap() {
            PermissionsAllowlist::Origins(v) => assert_eq!(v, &vec!["self".to_string()]),
            other => panic!("expected Origins, got {other:?}"),
        }
    }

    #[test]
    fn parse_origins_list() {
        let p = parse_permissions_policy("geolocation=(self \"https://example.com\")");
        match p.get("geolocation").unwrap() {
            PermissionsAllowlist::Origins(v) => {
                assert_eq!(
                    v,
                    &vec!["self".to_string(), "https://example.com".to_string()]
                );
            }
            other => panic!("expected Origins, got {other:?}"),
        }
    }

    #[test]
    fn parse_multiple_features() {
        let p = parse_permissions_policy("camera=*, microphone=(), geolocation=(self)");
        assert_eq!(p.len(), 3);
        assert_eq!(p.get("camera").unwrap(), &PermissionsAllowlist::All);
        assert_eq!(p.get("microphone").unwrap(), &PermissionsAllowlist::None);
    }

    #[test]
    fn parse_lowercase_feature_names() {
        let p = parse_permissions_policy("CAMERA=*");
        assert!(p.contains_key("camera"));
    }

    #[test]
    fn parse_first_duplicate_wins() {
        let p = parse_permissions_policy("camera=*, camera=()");
        assert_eq!(p.get("camera").unwrap(), &PermissionsAllowlist::All);
    }

    #[test]
    fn parse_handles_quotes_in_origins() {
        let p = parse_permissions_policy("geolocation=('https://a.com')");
        match p.get("geolocation").unwrap() {
            PermissionsAllowlist::Origins(v) => assert_eq!(v, &vec!["https://a.com".to_string()]),
            other => panic!("expected Origins, got {other:?}"),
        }
    }

    #[test]
    fn split_top_level_commas_respects_parens() {
        let v = split_top_level_commas("a=(x, y), b=*, c=()");
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn is_blocked_for_none_and_empty_origins() {
        assert!(PermissionsAllowlist::None.is_blocked());
        assert!(PermissionsAllowlist::Origins(vec![]).is_blocked());
        assert!(!PermissionsAllowlist::All.is_blocked());
        assert!(!PermissionsAllowlist::Origins(vec!["self".into()]).is_blocked());
    }

    #[test]
    fn allows_self_helper() {
        assert!(PermissionsAllowlist::All.allows_self());
        assert!(!PermissionsAllowlist::None.allows_self());
        assert!(PermissionsAllowlist::Origins(vec!["self".into()]).allows_self());
        assert!(!PermissionsAllowlist::Origins(vec!["https://x".into()]).allows_self());
    }

    #[test]
    fn store_and_get_round_trip() {
        let s = PermissionsPolicies::open_in_memory().unwrap();
        s.store("https://a/", "camera=*", 100).unwrap();
        let p = s.get("https://a/").unwrap().unwrap();
        assert_eq!(p.header_text, "camera=*");
        assert!(p.allowlists.contains_key("camera"));
    }

    #[test]
    fn store_overwrites() {
        let s = PermissionsPolicies::open_in_memory().unwrap();
        s.store("https://a/", "camera=*", 100).unwrap();
        s.store("https://a/", "camera=()", 200).unwrap();
        let p = s.get("https://a/").unwrap().unwrap();
        assert_eq!(p.header_text, "camera=()");
        assert_eq!(p.fetched_at, 200);
    }

    #[test]
    fn delete_and_count() {
        let s = PermissionsPolicies::open_in_memory().unwrap();
        assert_eq!(s.count().unwrap(), 0);
        s.store("https://a/", "camera=*", 100).unwrap();
        s.store("https://b/", "camera=()", 100).unwrap();
        assert_eq!(s.count().unwrap(), 2);
        s.delete("https://a/").unwrap();
        assert_eq!(s.count().unwrap(), 1);
    }
}
