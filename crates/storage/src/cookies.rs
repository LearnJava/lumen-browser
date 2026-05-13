//! Cookie jar поверх SQLite (RFC 6265 + RFC 6265bis для SameSite).
//!
//! Хранит cookies персистентно с поддержкой:
//! - Domain matching (RFC 6265 §5.1.3): exact или subdomain;
//! - Path matching (§5.1.4): prefix-match с правилами разделителя;
//! - Expires / Max-Age (через timestamp `expires_at`); session cookies
//!   (без expires) хранятся в той же таблице с `expires_at = NULL`;
//! - Secure flag (отправлять только через HTTPS);
//! - HttpOnly flag (не доступны JS — пометка для будущей интеграции);
//! - SameSite (Strict / Lax / None) — RFC 6265bis;
//! - **Total cookie protection** (§9.2 плана): партиционирование по
//!   `top_level_site` — третьесторонний cookie для одного сайта-источника
//!   не виден через другой top-level контекст.
//!
//! Этот модуль — только storage layer. HTTP-парсер `Set-Cookie` и
//! интеграция с `lumen-network` — отдельные задачи. Тут лежит схема,
//! типы и матчинг.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

/// SameSite политика cookie. RFC 6265bis §4.1.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SameSite {
    /// Cookie отправляется только для same-site навигации.
    Strict,
    /// Cookie отправляется при top-level cross-site навигации (default браузеров).
    #[default]
    Lax,
    /// Cookie отправляется во всех случаях (требует `Secure`).
    None,
}

impl SameSite {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }

    fn from_db_str(s: &str) -> Self {
        match s {
            "Strict" => Self::Strict,
            "None" => Self::None,
            // default — Lax (включая повреждённые / пустые значения).
            _ => Self::Lax,
        }
    }
}

/// Один cookie с атрибутами. domain хранится lowercase, path — как есть.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    pub domain: String,
    pub path: String,
    pub name: String,
    pub value: String,
    /// Unix timestamp (секунды) истечения. `None` — session cookie.
    pub expires_at: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

/// Cookie jar — обёртка над SQLite-БД cookies.
pub struct CookieJar {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for CookieJar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieJar").finish()
    }
}

impl CookieJar {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("cookies open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("cookies open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS cookies (
                top_level_site TEXT NOT NULL DEFAULT '',
                domain         TEXT NOT NULL,
                path           TEXT NOT NULL,
                name           TEXT NOT NULL,
                value          TEXT NOT NULL,
                expires_at     INTEGER,
                secure         INTEGER NOT NULL DEFAULT 0,
                http_only      INTEGER NOT NULL DEFAULT 0,
                same_site      TEXT NOT NULL DEFAULT 'Lax',
                PRIMARY KEY (top_level_site, domain, path, name)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS cookies_by_domain
                ON cookies (domain, top_level_site);
            "#,
        )
        .map_err(|e| Error::Storage(format!("cookies init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Записать (или обновить) cookie. domain нормализуется к lowercase.
    pub fn set(&self, cookie: Cookie, top_level_site: Option<&str>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cookies mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO cookies
             (top_level_site, domain, path, name, value, expires_at, secure, http_only, same_site)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (top_level_site, domain, path, name) DO UPDATE SET
                 value = excluded.value,
                 expires_at = excluded.expires_at,
                 secure = excluded.secure,
                 http_only = excluded.http_only,
                 same_site = excluded.same_site",
            params![
                top_level_site.unwrap_or(""),
                cookie.domain.to_lowercase(),
                cookie.path,
                cookie.name,
                cookie.value,
                cookie.expires_at,
                cookie.secure as i32,
                cookie.http_only as i32,
                cookie.same_site.as_db_str(),
            ],
        )
        .map_err(|e| Error::Storage(format!("cookies set: {e}")))?;
        Ok(())
    }

    /// Удалить конкретный cookie по (domain, path, name, top_level_site).
    pub fn delete(
        &self,
        domain: &str,
        path: &str,
        name: &str,
        top_level_site: Option<&str>,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cookies mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM cookies WHERE top_level_site = ?1 AND domain = ?2
             AND path = ?3 AND name = ?4",
            params![
                top_level_site.unwrap_or(""),
                domain.to_lowercase(),
                path,
                name,
            ],
        )
        .map_err(|e| Error::Storage(format!("cookies delete: {e}")))?;
        Ok(())
    }

    /// Удалить все expired cookies (`expires_at < now`). Session cookies
    /// (`expires_at IS NULL`) не трогаются — для них зачистка отдельная
    /// (например, при закрытии сессии).
    pub fn clear_expired(&self, now_unix: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cookies mutex poisoned".into()))?;
        let count = conn
            .execute(
                "DELETE FROM cookies WHERE expires_at IS NOT NULL AND expires_at < ?1",
                params![now_unix],
            )
            .map_err(|e| Error::Storage(format!("cookies clear_expired: {e}")))?;
        Ok(count)
    }

    /// Удалить все session cookies (`expires_at IS NULL`). Зовётся при
    /// закрытии профиля / sign-out / clear browsing data.
    pub fn clear_session(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cookies mutex poisoned".into()))?;
        let count = conn
            .execute("DELETE FROM cookies WHERE expires_at IS NULL", [])
            .map_err(|e| Error::Storage(format!("cookies clear_session: {e}")))?;
        Ok(count)
    }

    /// Получить все cookies, применимые к данному запросу. Фильтрация:
    /// 1. Same `top_level_site` partition.
    /// 2. Domain-match: cookie.domain == request_host ИЛИ request_host
    ///    оканчивается на ".cookie.domain" (RFC 6265 §5.1.3).
    /// 3. Path-match: RFC 6265 §5.1.4 — equal или prefix с разделителем.
    /// 4. Если cookie.secure — request должен идти через HTTPS.
    /// 5. Cookie не expired относительно `now_unix`.
    pub fn get_for_request(
        &self,
        request_host: &str,
        request_path: &str,
        is_secure: bool,
        now_unix: i64,
        top_level_site: Option<&str>,
    ) -> Result<Vec<Cookie>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("cookies mutex poisoned".into()))?;
        let request_host_lc = request_host.to_lowercase();
        let tls = top_level_site.unwrap_or("");
        // Подтягиваем кандидатов по host-индексу: cookie.domain == host
        // или cookie.domain является suffix-ом без leading-dot rule.
        // Удобно фильтровать host-prefix в Rust, чтобы не строить
        // регулярки в SQL.
        let mut stmt = conn
            .prepare_cached(
                "SELECT domain, path, name, value, expires_at, secure, http_only, same_site
                 FROM cookies
                 WHERE top_level_site = ?1
                   AND (expires_at IS NULL OR expires_at >= ?2)",
            )
            .map_err(|e| Error::Storage(format!("cookies prepare get: {e}")))?;
        let rows = stmt
            .query_map(params![tls, now_unix], |row| {
                Ok(Cookie {
                    domain: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    value: row.get(3)?,
                    expires_at: row.get(4)?,
                    secure: row.get::<_, i32>(5)? != 0,
                    http_only: row.get::<_, i32>(6)? != 0,
                    same_site: SameSite::from_db_str(&row.get::<_, String>(7)?),
                })
            })
            .map_err(|e| Error::Storage(format!("cookies query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            let c = r.map_err(|e| Error::Storage(format!("cookies row: {e}")))?;
            if c.secure && !is_secure {
                continue;
            }
            if !domain_matches(&request_host_lc, &c.domain) {
                continue;
            }
            if !path_matches(request_path, &c.path) {
                continue;
            }
            out.push(c);
        }
        Ok(out)
    }
}

/// RFC 6265 §5.1.3 — request_host совпадает с cookie_domain ИЛИ
/// request_host оканчивается на `.cookie_domain`. cookie_domain не
/// должен совпадать с IP-адресом — но проверку IP оставляем на caller-а
/// (тут только string-matching).
fn domain_matches(request_host: &str, cookie_domain: &str) -> bool {
    if request_host == cookie_domain {
        return true;
    }
    if cookie_domain.is_empty() {
        return false;
    }
    if let Some(stripped) = request_host.strip_suffix(cookie_domain)
        && stripped.ends_with('.')
    {
        return true;
    }
    false
}

/// RFC 6265 §5.1.4 — equality ИЛИ prefix с разделителем `/`.
fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    if cookie_path == request_path {
        return true;
    }
    if !request_path.starts_with(cookie_path) {
        return false;
    }
    // После prefix-а должен быть `/` (либо в самом cookie_path в конце,
    // либо в continuation request_path).
    if cookie_path.ends_with('/') {
        return true;
    }
    matches!(
        request_path.as_bytes().get(cookie_path.len()),
        Some(&b'/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_jar() -> CookieJar {
        CookieJar::open_in_memory().unwrap()
    }

    fn make_cookie(domain: &str, path: &str, name: &str, value: &str) -> Cookie {
        Cookie {
            domain: domain.to_string(),
            path: path.to_string(),
            name: name.to_string(),
            value: value.to_string(),
            expires_at: None,
            secure: false,
            http_only: false,
            same_site: SameSite::Lax,
        }
    }

    // ── domain_matches ──

    #[test]
    fn domain_match_exact() {
        assert!(domain_matches("example.com", "example.com"));
    }

    #[test]
    fn domain_match_subdomain() {
        assert!(domain_matches("sub.example.com", "example.com"));
        assert!(domain_matches("a.b.example.com", "example.com"));
    }

    #[test]
    fn domain_match_no_prefix_without_dot() {
        // anotherexample.com НЕ должен матчить example.com (нет точки между).
        assert!(!domain_matches("anotherexample.com", "example.com"));
    }

    #[test]
    fn domain_match_no_reverse() {
        // Cookie для sub.example.com НЕ должен матчить запрос к example.com.
        assert!(!domain_matches("example.com", "sub.example.com"));
    }

    // ── path_matches ──

    #[test]
    fn path_match_exact() {
        assert!(path_matches("/foo", "/foo"));
    }

    #[test]
    fn path_match_prefix_with_trailing_slash() {
        assert!(path_matches("/foo/bar", "/foo/"));
    }

    #[test]
    fn path_match_prefix_with_continuation_slash() {
        // cookie_path="/foo" — следующий байт после prefix-а в request_path
        // должен быть `/`, иначе не match.
        assert!(path_matches("/foo/bar", "/foo"));
        assert!(!path_matches("/foobar", "/foo"));
    }

    #[test]
    fn path_match_root() {
        // Cookie с path=/ матчит любой путь.
        assert!(path_matches("/", "/"));
        assert!(path_matches("/anything", "/"));
        assert!(path_matches("/anything/deep", "/"));
    }

    // ── CookieJar CRUD ──

    #[test]
    fn set_and_get_roundtrip() {
        let jar = make_jar();
        let c = make_cookie("example.com", "/", "session", "abc123");
        jar.set(c.clone(), None).unwrap();
        let got = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "session");
        assert_eq!(got[0].value, "abc123");
    }

    #[test]
    fn set_overwrites_same_key() {
        let jar = make_jar();
        let c1 = make_cookie("example.com", "/", "k", "v1");
        let mut c2 = c1.clone();
        c2.value = "v2".into();
        jar.set(c1, None).unwrap();
        jar.set(c2, None).unwrap();
        let got = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].value, "v2");
    }

    #[test]
    fn delete_removes_cookie() {
        let jar = make_jar();
        let c = make_cookie("example.com", "/", "k", "v");
        jar.set(c, None).unwrap();
        jar.delete("example.com", "/", "k", None).unwrap();
        assert!(jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn expired_cookies_not_returned() {
        let jar = make_jar();
        let mut c = make_cookie("example.com", "/", "k", "v");
        c.expires_at = Some(100);
        jar.set(c, None).unwrap();
        // Сейчас now = 200, cookie expired.
        let got = jar
            .get_for_request("example.com", "/", false, 200, None)
            .unwrap();
        assert!(got.is_empty());
        // А сейчас now = 50, cookie ещё валиден.
        let got2 = jar
            .get_for_request("example.com", "/", false, 50, None)
            .unwrap();
        assert_eq!(got2.len(), 1);
    }

    #[test]
    fn clear_expired_removes_only_past() {
        let jar = make_jar();
        let mut c_expired = make_cookie("example.com", "/", "old", "x");
        c_expired.expires_at = Some(100);
        let mut c_future = make_cookie("example.com", "/", "new", "x");
        c_future.expires_at = Some(1000);
        let c_session = make_cookie("example.com", "/", "session", "x");

        jar.set(c_expired, None).unwrap();
        jar.set(c_future, None).unwrap();
        jar.set(c_session, None).unwrap();

        let removed = jar.clear_expired(500).unwrap();
        assert_eq!(removed, 1);
        let got = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        // Остались: new + session. now=0 не expire-ит ничего.
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn secure_cookie_only_over_https() {
        let jar = make_jar();
        let mut c = make_cookie("example.com", "/", "secure_tok", "x");
        c.secure = true;
        jar.set(c, None).unwrap();
        // HTTP запрос — secure cookie не отдаём.
        let http = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        assert!(http.is_empty());
        // HTTPS — отдаём.
        let https = jar
            .get_for_request("example.com", "/", true, 0, None)
            .unwrap();
        assert_eq!(https.len(), 1);
    }

    #[test]
    fn domain_subdomain_matching_in_jar() {
        let jar = make_jar();
        let c = make_cookie("example.com", "/", "k", "v");
        jar.set(c, None).unwrap();
        // sub.example.com должен получить cookie.
        let sub = jar
            .get_for_request("sub.example.com", "/", false, 0, None)
            .unwrap();
        assert_eq!(sub.len(), 1);
    }

    #[test]
    fn top_level_site_partitions_cookies() {
        let jar = make_jar();
        // Тот же сторонний домен и cookie, но через разные top-level
        // сайты → разные значения (total cookie protection).
        let mut c_news = make_cookie("ads.com", "/", "id", "news_user");
        c_news.same_site = SameSite::None;
        let mut c_blog = make_cookie("ads.com", "/", "id", "blog_user");
        c_blog.same_site = SameSite::None;
        jar.set(c_news, Some("https://news.com")).unwrap();
        jar.set(c_blog, Some("https://blog.com")).unwrap();

        let news = jar
            .get_for_request("ads.com", "/", false, 0, Some("https://news.com"))
            .unwrap();
        let blog = jar
            .get_for_request("ads.com", "/", false, 0, Some("https://blog.com"))
            .unwrap();
        assert_eq!(news[0].value, "news_user");
        assert_eq!(blog[0].value, "blog_user");
    }

    #[test]
    fn domain_stored_lowercase() {
        let jar = make_jar();
        let c = make_cookie("Example.COM", "/", "k", "v");
        jar.set(c, None).unwrap();
        let got = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].domain, "example.com");
    }

    #[test]
    fn path_filtering_applies() {
        let jar = make_jar();
        let c = make_cookie("example.com", "/admin", "k", "v");
        jar.set(c, None).unwrap();
        assert_eq!(
            jar.get_for_request("example.com", "/admin/x", false, 0, None)
                .unwrap()
                .len(),
            1
        );
        assert!(jar
            .get_for_request("example.com", "/other", false, 0, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn samesite_preserved_through_roundtrip() {
        let jar = make_jar();
        for ss in [SameSite::Strict, SameSite::Lax, SameSite::None] {
            let mut c = make_cookie("example.com", "/", &format!("k_{ss:?}"), "v");
            c.same_site = ss;
            jar.set(c, None).unwrap();
        }
        let got = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        let policies: Vec<SameSite> = got.iter().map(|c| c.same_site).collect();
        assert!(policies.contains(&SameSite::Strict));
        assert!(policies.contains(&SameSite::Lax));
        assert!(policies.contains(&SameSite::None));
    }

    #[test]
    fn clear_session_removes_only_session_cookies() {
        let jar = make_jar();
        let session = make_cookie("example.com", "/", "session", "x");
        let mut persistent = make_cookie("example.com", "/", "remember", "y");
        persistent.expires_at = Some(9_999_999);
        jar.set(session, None).unwrap();
        jar.set(persistent, None).unwrap();
        let removed = jar.clear_session().unwrap();
        assert_eq!(removed, 1);
        let got = jar
            .get_for_request("example.com", "/", false, 0, None)
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "remember");
    }
}
