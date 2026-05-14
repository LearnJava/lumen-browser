//! Referrer Policy store — per-origin настройки HTTP Referer-header.
//!
//! Referrer Policy spec: <https://w3c.github.io/webappsec-referrer-policy/>
//! 8 значений (`no-referrer`, `no-referrer-when-downgrade`, ...) определяют,
//! что отправлять в Referer-header при cross-origin / same-origin /
//! downgrade-запросах.
//!
//! Phase 0: storage layer. Реальный matcher (origin → policy) + apply
//! при HTTP-request — задача `lumen-network`.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReferrerPolicy {
    /// `no-referrer` — Referer вообще не отправляется.
    NoReferrer,
    /// `no-referrer-when-downgrade` — отправляется при upgrade/same,
    /// но не при HTTPS → HTTP downgrade.
    NoReferrerWhenDowngrade,
    /// `origin` — отправляется только origin (`https://example.com/`),
    /// без path.
    Origin,
    /// `origin-when-cross-origin` — full URL для same-origin, только
    /// origin для cross-origin.
    OriginWhenCrossOrigin,
    /// `same-origin` — full URL только для same-origin, ничего для cross.
    SameOrigin,
    /// `strict-origin` — origin только для same-/upgrade-/equal-secure;
    /// ничего для downgrade.
    StrictOrigin,
    /// `strict-origin-when-cross-origin` (default в современных браузерах).
    #[default]
    StrictOriginWhenCrossOrigin,
    /// `unsafe-url` — полный URL для всех запросов (опасно для приватности).
    UnsafeUrl,
}

impl ReferrerPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoReferrer => "no-referrer",
            Self::NoReferrerWhenDowngrade => "no-referrer-when-downgrade",
            Self::Origin => "origin",
            Self::OriginWhenCrossOrigin => "origin-when-cross-origin",
            Self::SameOrigin => "same-origin",
            Self::StrictOrigin => "strict-origin",
            Self::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
            Self::UnsafeUrl => "unsafe-url",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "no-referrer" => Some(Self::NoReferrer),
            "no-referrer-when-downgrade" => Some(Self::NoReferrerWhenDowngrade),
            "origin" => Some(Self::Origin),
            "origin-when-cross-origin" => Some(Self::OriginWhenCrossOrigin),
            "same-origin" => Some(Self::SameOrigin),
            "strict-origin" => Some(Self::StrictOrigin),
            "strict-origin-when-cross-origin" | "" => {
                // Empty (`Referrer-Policy:`) → default.
                Some(Self::StrictOriginWhenCrossOrigin)
            }
            "unsafe-url" => Some(Self::UnsafeUrl),
            _ => None,
        }
    }
}

pub struct ReferrerPolicies {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for ReferrerPolicies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferrerPolicies").finish()
    }
}

impl ReferrerPolicies {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("referrer_policy open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("referrer_policy open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS referrer_policies (
                origin     TEXT PRIMARY KEY,
                policy     TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            ) WITHOUT ROWID;
            "#,
        )
        .map_err(|e| Error::Storage(format!("referrer_policy init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Установить policy для origin. Перезаписывает существующую.
    pub fn set(&self, origin: &str, policy: ReferrerPolicy, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("referrer_policy mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO referrer_policies (origin, policy, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (origin) DO UPDATE SET
                 policy = excluded.policy,
                 updated_at = excluded.updated_at",
            params![origin, policy.as_str(), now_unix],
        )
        .map_err(|e| Error::Storage(format!("referrer_policy set: {e}")))?;
        Ok(())
    }

    /// Получить policy для origin. Если нет записи — None
    /// (caller использует global default
    /// `StrictOriginWhenCrossOrigin`).
    pub fn get(&self, origin: &str) -> Result<Option<ReferrerPolicy>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("referrer_policy mutex poisoned".into()))?;
        let row: Option<String> = conn
            .query_row(
                "SELECT policy FROM referrer_policies WHERE origin = ?1",
                params![origin],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("referrer_policy get: {e}")))?;
        Ok(row.and_then(|s| ReferrerPolicy::parse(&s)))
    }

    /// Получить policy с fallback на default (если нет per-origin).
    pub fn get_or_default(&self, origin: &str) -> Result<ReferrerPolicy> {
        Ok(self.get(origin)?.unwrap_or_default())
    }

    pub fn delete(&self, origin: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("referrer_policy mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM referrer_policies WHERE origin = ?1",
            params![origin],
        )
        .map_err(|e| Error::Storage(format!("referrer_policy delete: {e}")))?;
        Ok(())
    }

    pub fn list_all(&self) -> Result<Vec<(String, ReferrerPolicy)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("referrer_policy mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, policy FROM referrer_policies ORDER BY origin ASC",
            )
            .map_err(|e| Error::Storage(format!("referrer_policy list prepare: {e}")))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| Error::Storage(format!("referrer_policy list query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            let (origin, policy_str) =
                r.map_err(|e| Error::Storage(format!("referrer_policy row: {e}")))?;
            if let Some(policy) = ReferrerPolicy::parse(&policy_str) {
                out.push((origin, policy));
            }
        }
        Ok(out)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("referrer_policy mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM referrer_policies", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("referrer_policy count: {e}")))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> ReferrerPolicies {
        ReferrerPolicies::open_in_memory().unwrap()
    }

    #[test]
    fn set_and_get_policy() {
        let s = make();
        s.set("https://example.com", ReferrerPolicy::NoReferrer, 100)
            .unwrap();
        assert_eq!(
            s.get("https://example.com").unwrap(),
            Some(ReferrerPolicy::NoReferrer)
        );
    }

    #[test]
    fn get_missing_returns_none() {
        let s = make();
        assert_eq!(s.get("https://nope/").unwrap(), None);
    }

    #[test]
    fn get_or_default_returns_strict_origin_when_cross_origin() {
        let s = make();
        assert_eq!(
            s.get_or_default("https://nope/").unwrap(),
            ReferrerPolicy::StrictOriginWhenCrossOrigin
        );
    }

    #[test]
    fn set_overwrites_existing() {
        let s = make();
        s.set("https://x/", ReferrerPolicy::NoReferrer, 100).unwrap();
        s.set("https://x/", ReferrerPolicy::UnsafeUrl, 200).unwrap();
        assert_eq!(
            s.get("https://x/").unwrap(),
            Some(ReferrerPolicy::UnsafeUrl)
        );
    }

    #[test]
    fn delete_removes_policy() {
        let s = make();
        s.set("https://x/", ReferrerPolicy::Origin, 100).unwrap();
        s.delete("https://x/").unwrap();
        assert_eq!(s.get("https://x/").unwrap(), None);
    }

    #[test]
    fn list_all_ordered_by_origin() {
        let s = make();
        s.set("https://c/", ReferrerPolicy::Origin, 100).unwrap();
        s.set("https://a/", ReferrerPolicy::NoReferrer, 100).unwrap();
        s.set("https://b/", ReferrerPolicy::SameOrigin, 100).unwrap();
        let all = s.list_all().unwrap();
        let origins: Vec<&str> = all.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(origins, vec!["https://a/", "https://b/", "https://c/"]);
    }

    #[test]
    fn parse_all_8_values() {
        for (s, expected) in [
            ("no-referrer", ReferrerPolicy::NoReferrer),
            ("no-referrer-when-downgrade", ReferrerPolicy::NoReferrerWhenDowngrade),
            ("origin", ReferrerPolicy::Origin),
            ("origin-when-cross-origin", ReferrerPolicy::OriginWhenCrossOrigin),
            ("same-origin", ReferrerPolicy::SameOrigin),
            ("strict-origin", ReferrerPolicy::StrictOrigin),
            (
                "strict-origin-when-cross-origin",
                ReferrerPolicy::StrictOriginWhenCrossOrigin,
            ),
            ("unsafe-url", ReferrerPolicy::UnsafeUrl),
        ] {
            assert_eq!(ReferrerPolicy::parse(s), Some(expected));
        }
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(
            ReferrerPolicy::parse("NO-REFERRER"),
            Some(ReferrerPolicy::NoReferrer)
        );
        assert_eq!(
            ReferrerPolicy::parse("Strict-Origin"),
            Some(ReferrerPolicy::StrictOrigin)
        );
    }

    #[test]
    fn parse_empty_returns_default() {
        // Empty Referrer-Policy header → default.
        assert_eq!(
            ReferrerPolicy::parse(""),
            Some(ReferrerPolicy::StrictOriginWhenCrossOrigin)
        );
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(ReferrerPolicy::parse("garbage"), None);
        assert_eq!(ReferrerPolicy::parse("never"), None);
    }

    #[test]
    fn as_str_round_trip() {
        for p in [
            ReferrerPolicy::NoReferrer,
            ReferrerPolicy::NoReferrerWhenDowngrade,
            ReferrerPolicy::Origin,
            ReferrerPolicy::OriginWhenCrossOrigin,
            ReferrerPolicy::SameOrigin,
            ReferrerPolicy::StrictOrigin,
            ReferrerPolicy::StrictOriginWhenCrossOrigin,
            ReferrerPolicy::UnsafeUrl,
        ] {
            assert_eq!(ReferrerPolicy::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn default_is_strict_origin_when_cross_origin() {
        assert_eq!(
            ReferrerPolicy::default(),
            ReferrerPolicy::StrictOriginWhenCrossOrigin
        );
    }

    #[test]
    fn cyrillic_origin() {
        let s = make();
        s.set("https://пример.рф/", ReferrerPolicy::NoReferrer, 100).unwrap();
        assert_eq!(
            s.get("https://пример.рф/").unwrap(),
            Some(ReferrerPolicy::NoReferrer)
        );
    }

    #[test]
    fn count_works() {
        let s = make();
        assert_eq!(s.count().unwrap(), 0);
        s.set("https://a/", ReferrerPolicy::Origin, 100).unwrap();
        s.set("https://b/", ReferrerPolicy::SameOrigin, 100).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }
}
