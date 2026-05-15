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

use lumen_core::ext::PublicSuffixList;
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

/// Распарсить значение HTTP-заголовка `Set-Cookie` в `Cookie`. Без PSL
/// проверок — backward-compat wrapper над [`parse_set_cookie_with_psl`]
/// для caller-ов, у которых PSL не подключён.
///
/// RFC 6265 §5.2. Формат:
///
/// ```text
/// name=value [; attr [; attr]...]
/// ```
///
/// Атрибуты (case-insensitive имена):
/// - `Expires=<rfc1123-date>` — Unix timestamp expires_at (только формат
///   RFC 1123 «Wed, 21 Oct 2015 07:28:00 GMT» в Phase 0; прочие формы
///   spec-а игнорируются — session cookie);
/// - `Max-Age=<seconds>` — приоритетнее Expires; expires_at = now + N;
///   отрицательное / 0 / нечисловое значение → session cookie;
/// - `Domain=<domain>` — leading-dot strip (RFC 6265 §5.2.3);
/// - `Path=<path>` — иначе default_path;
/// - `Secure` (без значения);
/// - `HttpOnly` (без значения);
/// - `SameSite=Strict|Lax|None` — иначе default Lax.
///
/// Возвращает `None` если name/value не распарсилось (нет `=` в первом
/// сегменте) или name пустое.
///
/// `now_unix` — текущий Unix timestamp (для расчёта Max-Age).
pub fn parse_set_cookie(
    header: &str,
    default_domain: &str,
    default_path: &str,
    now_unix: i64,
) -> Option<Cookie> {
    parse_set_cookie_with_psl(header, default_domain, default_path, now_unix, None)
}

/// Расширенная версия [`parse_set_cookie`] с опциональной проверкой
/// `PublicSuffixList` — реализует Storage Model RFC 6265bis §5.5
/// шаги 5 (Domain attribute) и применяет правило public-suffix:
///
/// 1. Если `Domain` attribute не указан → cookie host-only, domain =
///    `default_domain` (request host). Текущее поведение, PSL не влияет.
/// 2. Если `Domain` указан и `default_domain` НЕ domain-match-ит cookie
///    domain (например, server присылает `Domain=evil.com` для запроса
///    `victim.com`) → cookie reject-ится, возвращаем `None`.
/// 3. Если `Domain` указан и совпадает с известным public suffix:
///    - если `domain == default_domain` → cookie остаётся host-only
///      (Domain attribute treated as null);
///    - иначе → reject, `None`. Это блокирует «super-cookie»-атаки, где
///      `evil.com` пытается сохранить cookie с `Domain=co.uk`.
/// 4. Если PSL = `None`, шаг 3 пропускается — для caller-ов, у которых
///    PSL ещё не подключён (fail-open: cookie допускается, как и было
///    до этой функции).
///
/// Шаг 2 (domain-match) проверяется **всегда**, даже без PSL — это базовая
/// RFC 6265 §5.3 проверка, не требующая знания eTLD.
pub fn parse_set_cookie_with_psl(
    header: &str,
    default_domain: &str,
    default_path: &str,
    now_unix: i64,
    psl: Option<&dyn PublicSuffixList>,
) -> Option<Cookie> {
    let mut parts = header.split(';');
    let first = parts.next()?.trim();
    let eq = first.find('=')?;
    let name = first[..eq].trim();
    if name.is_empty() {
        return None;
    }
    let value = first[eq + 1..].trim().to_string();

    let default_domain_lc = default_domain.to_lowercase();
    let mut domain_attr: Option<String> = None;
    let mut path = default_path.to_string();
    let mut expires_at: Option<i64> = None;
    let mut max_age: Option<i64> = None;
    let mut secure = false;
    let mut http_only = false;
    let mut same_site = SameSite::Lax;

    for raw in parts {
        let attr = raw.trim();
        if attr.is_empty() {
            continue;
        }
        let (key, val) = match attr.find('=') {
            Some(i) => (attr[..i].trim(), attr[i + 1..].trim()),
            None => (attr, ""),
        };
        if key.eq_ignore_ascii_case("expires") {
            expires_at = parse_rfc1123_date(val);
        } else if key.eq_ignore_ascii_case("max-age") {
            // RFC 6265 §5.2.2: только цифры (опц. ведущий `-`); прочее — игнор.
            if let Ok(n) = val.parse::<i64>() {
                max_age = Some(n);
            }
        } else if key.eq_ignore_ascii_case("domain") {
            // Leading dot — strip (RFC 6265 §5.2.3 — host-relative).
            // Пустой Domain= после strip-а — игнорируем (как «не указан»).
            let stripped = val.strip_prefix('.').unwrap_or(val).to_lowercase();
            if !stripped.is_empty() {
                domain_attr = Some(stripped);
            }
        } else if key.eq_ignore_ascii_case("path") {
            // Пустой Path / не начинается с `/` — RFC 6265 §5.2.4 → default
            // (мы берём `default_path` как fallback).
            if val.starts_with('/') {
                path = val.to_string();
            }
        } else if key.eq_ignore_ascii_case("secure") {
            secure = true;
        } else if key.eq_ignore_ascii_case("httponly") {
            http_only = true;
        } else if key.eq_ignore_ascii_case("samesite") {
            if val.eq_ignore_ascii_case("strict") {
                same_site = SameSite::Strict;
            } else if val.eq_ignore_ascii_case("none") {
                same_site = SameSite::None;
            } else if val.eq_ignore_ascii_case("lax") {
                same_site = SameSite::Lax;
            }
        }
    }

    // Определить итоговый domain через RFC 6265bis §5.5 шаг 5.
    let domain = match domain_attr {
        None => default_domain_lc.clone(),
        Some(d) => {
            // RFC 6265 §5.3 step 4: request-host must domain-match Domain.
            if !domain_matches(&default_domain_lc, &d) {
                return None;
            }
            // RFC 6265bis §5.5: public-suffix защита.
            if let Some(psl) = psl
                && psl.is_public_suffix(&d)
            {
                if d == default_domain_lc {
                    // Cookie остаётся host-only (Domain attribute → null).
                    default_domain_lc.clone()
                } else {
                    // Super-cookie attempt — отвергаем.
                    return None;
                }
            } else {
                d
            }
        }
    };

    // Max-Age priority над Expires (RFC 6265 §5.2.2). Max-Age <= 0 →
    // expired cookie сразу (timestamp в прошлом).
    let final_expires = if let Some(ma) = max_age {
        Some(now_unix.saturating_add(ma))
    } else {
        expires_at
    };

    Some(Cookie {
        domain,
        path,
        name: name.to_string(),
        value,
        expires_at: final_expires,
        secure,
        http_only,
        same_site,
    })
}

/// Парсер RFC 1123 даты вида «Wed, 21 Oct 2015 07:28:00 GMT». Возвращает
/// Unix timestamp (секунды). Прочие формы (RFC 850, ANSI C asctime,
/// нестандартные дроби) — None.
fn parse_rfc1123_date(s: &str) -> Option<i64> {
    let s = s.trim();
    // Опц. day-of-week-prefix «Wed, ». Пропускаем до первой запятой+пробела
    // или просто берём остаток если запятой нет (некоторые серверы
    // присылают без day-of-week).
    let rest = match s.find(", ") {
        Some(i) => &s[i + 2..],
        None => s,
    };
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 5 {
        return None;
    }
    let day: u32 = tokens[0].parse().ok()?;
    let month = month_from_name(tokens[1])?;
    let year: i32 = tokens[2].parse().ok()?;
    // Год — 2-значный legacy form (RFC 850) → не валиден тут; берём 4-значный.
    if year < 1000 {
        return None;
    }
    let time = tokens[3];
    let mut tparts = time.split(':');
    let hh: u32 = tparts.next()?.parse().ok()?;
    let mm: u32 = tparts.next()?.parse().ok()?;
    let ss: u32 = tparts.next()?.parse().ok()?;
    // tokens[4] — обычно «GMT», игнорируем (other timezones — за пределами
    // Phase 0; RFC 6265 cookie attrs всегда GMT).
    Some(civil_to_unix(year, month, day, hh, mm, ss))
}

fn month_from_name(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// Конверсия Gregorian-даты в Unix timestamp. Алгоритм Хинннанта
/// (Howard Hinnant, «date_algorithms.html»): days_from_civil даёт
/// число дней с 1970-01-01 (отрицательно для дат ранее).
fn civil_to_unix(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
    let yi = if m <= 2 { y - 1 } else { y };
    let era = if yi >= 0 { yi } else { yi - 399 } / 400;
    let yoe = (yi - era * 400) as i64; // 0..400
    let m_adj = if m > 2 { m as i64 - 3 } else { m as i64 + 9 }; // 0..12
    let doy = (153 * m_adj + 2) / 5 + d as i64 - 1; // 0..366
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era as i64 * 146097 + doe - 719468;
    days * 86400 + hh as i64 * 3600 + mm as i64 * 60 + ss as i64
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

    // ── parse_set_cookie ──

    #[test]
    fn parse_basic_name_value() {
        let c = parse_set_cookie("session=abc123", "example.com", "/", 0).unwrap();
        assert_eq!(c.name, "session");
        assert_eq!(c.value, "abc123");
        assert_eq!(c.domain, "example.com");
        assert_eq!(c.path, "/");
        assert_eq!(c.expires_at, None);
        assert!(!c.secure);
        assert!(!c.http_only);
        assert_eq!(c.same_site, SameSite::Lax);
    }

    #[test]
    fn parse_with_attributes() {
        // request-host = host.example.com domain-match-ит Domain=example.com
        // (RFC 6265 §5.3 step 4); без этого parse_set_cookie возвращает None
        // — RFC 6265bis §5.5 шаг 5.1.
        let c = parse_set_cookie(
            "tok=xyz; Domain=.example.com; Path=/admin; Secure; HttpOnly; SameSite=Strict",
            "host.example.com",
            "/",
            0,
        )
        .unwrap();
        assert_eq!(c.name, "tok");
        assert_eq!(c.value, "xyz");
        // Leading dot strip.
        assert_eq!(c.domain, "example.com");
        assert_eq!(c.path, "/admin");
        assert!(c.secure);
        assert!(c.http_only);
        assert_eq!(c.same_site, SameSite::Strict);
    }

    #[test]
    fn parse_rejects_cross_origin_domain() {
        // RFC 6265 §5.3 step 4 / RFC 6265bis §5.5 step 5.1: server для
        // fallback.com не может присылать Set-Cookie c Domain=example.com
        // (domain-mismatch). Возврат None даже без PSL.
        assert!(parse_set_cookie(
            "tok=xyz; Domain=example.com",
            "fallback.com",
            "/",
            0,
        )
        .is_none());
    }

    #[test]
    fn parse_max_age_overrides_expires() {
        // Max-Age приоритетнее Expires.
        let c = parse_set_cookie(
            "k=v; Max-Age=3600; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
            "example.com",
            "/",
            1000,
        )
        .unwrap();
        // Max-Age = 3600, now = 1000 → expires_at = 4600.
        assert_eq!(c.expires_at, Some(4600));
    }

    #[test]
    fn parse_expires_rfc1123() {
        let c = parse_set_cookie(
            "k=v; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
            "example.com",
            "/",
            0,
        )
        .unwrap();
        // 2015-10-21T07:28:00Z = 1445412480 Unix.
        assert_eq!(c.expires_at, Some(1_445_412_480));
    }

    #[test]
    fn parse_expires_without_day_of_week() {
        // Некоторые серверы пропускают «Wed, » префикс.
        let c = parse_set_cookie(
            "k=v; Expires=21 Oct 2015 07:28:00 GMT",
            "example.com",
            "/",
            0,
        )
        .unwrap();
        assert_eq!(c.expires_at, Some(1_445_412_480));
    }

    #[test]
    fn parse_max_age_negative_means_expired() {
        // Max-Age <= 0 → cookie уже expired (timestamp в прошлом / now).
        let c = parse_set_cookie("k=v; Max-Age=-1", "example.com", "/", 100).unwrap();
        assert_eq!(c.expires_at, Some(99));
    }

    #[test]
    fn parse_samesite_variants() {
        for (header, expected) in [
            ("k=v; SameSite=Strict", SameSite::Strict),
            ("k=v; SameSite=lax", SameSite::Lax),  // case-insensitive
            ("k=v; SameSite=None", SameSite::None),
            ("k=v; SameSite=garbage", SameSite::Lax),  // unknown → default
            ("k=v", SameSite::Lax),  // missing → default
        ] {
            let c = parse_set_cookie(header, "example.com", "/", 0).unwrap();
            assert_eq!(c.same_site, expected, "header: {header}");
        }
    }

    #[test]
    fn parse_invalid_returns_none() {
        // Без `=` в первом сегменте.
        assert!(parse_set_cookie("just-a-name", "example.com", "/", 0).is_none());
        // Пустое имя (=value).
        assert!(parse_set_cookie("=value", "example.com", "/", 0).is_none());
    }

    #[test]
    fn parse_empty_value_ok() {
        // Пустое value — валидно (имя=пусто).
        let c = parse_set_cookie("k=", "example.com", "/", 0).unwrap();
        assert_eq!(c.name, "k");
        assert_eq!(c.value, "");
    }

    #[test]
    fn parse_path_must_start_with_slash() {
        // Path без `/` (RFC 6265 §5.2.4) → default_path.
        let c = parse_set_cookie("k=v; Path=admin", "example.com", "/fallback", 0).unwrap();
        assert_eq!(c.path, "/fallback");
    }

    #[test]
    fn parse_case_insensitive_attribute_names() {
        let c = parse_set_cookie(
            "k=v; secure; HTTPONLY; max-age=100; PATH=/x",
            "example.com",
            "/",
            0,
        )
        .unwrap();
        assert!(c.secure);
        assert!(c.http_only);
        assert_eq!(c.expires_at, Some(100));
        assert_eq!(c.path, "/x");
    }

    #[test]
    fn civil_to_unix_epoch() {
        assert_eq!(civil_to_unix(1970, 1, 1, 0, 0, 0), 0);
    }

    #[test]
    fn civil_to_unix_y2k() {
        // 2000-01-01T00:00:00Z = 946684800.
        assert_eq!(civil_to_unix(2000, 1, 1, 0, 0, 0), 946_684_800);
    }

    #[test]
    fn civil_to_unix_leap_year() {
        // 2020-02-29 (leap year) — должно работать. 2020-03-01 = 1583020800.
        // 2020-02-29 = 1582934400.
        assert_eq!(civil_to_unix(2020, 2, 29, 0, 0, 0), 1_582_934_400);
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

    // ── parse_set_cookie_with_psl — RFC 6265bis §5.5 шаг 5 ─────────────

    use crate::psl::PslProvider;

    #[test]
    fn psl_rejects_super_cookie_attempt() {
        // example.co.uk пытается поставить Domain=co.uk → super-cookie
        // attempt, отвергается.
        let psl = PslProvider::new();
        let c = parse_set_cookie_with_psl(
            "evil=1; Domain=co.uk",
            "example.co.uk",
            "/",
            0,
            Some(&psl),
        );
        assert!(c.is_none());
    }

    #[test]
    fn psl_rejects_bare_tld_domain() {
        // Domain=com — public suffix, отвергаем.
        let psl = PslProvider::new();
        assert!(parse_set_cookie_with_psl(
            "k=v; Domain=com",
            "example.com",
            "/",
            0,
            Some(&psl),
        )
        .is_none());
    }

    #[test]
    fn psl_treats_self_as_public_suffix_as_host_only() {
        // request-host сам является public suffix (странный случай —
        // например, прямое посещение `co.uk`). Set-Cookie c Domain=co.uk
        // допускается как host-only.
        let psl = PslProvider::new();
        let c = parse_set_cookie_with_psl(
            "k=v; Domain=co.uk",
            "co.uk",
            "/",
            0,
            Some(&psl),
        )
        .unwrap();
        // Cookie остаётся как host-only с domain = request-host.
        assert_eq!(c.domain, "co.uk");
    }

    #[test]
    fn psl_allows_legitimate_domain_attribute() {
        // example.co.uk ставит Domain=example.co.uk — registrable
        // domain, валидно.
        let psl = PslProvider::new();
        let c = parse_set_cookie_with_psl(
            "k=v; Domain=example.co.uk",
            "www.example.co.uk",
            "/",
            0,
            Some(&psl),
        )
        .unwrap();
        assert_eq!(c.domain, "example.co.uk");
    }

    #[test]
    fn psl_none_allows_super_cookie_fail_open() {
        // Без PSL — fail-open (старое поведение). Не идеально, но
        // лучше, чем падать у caller-ов, которые ещё не подключили PSL.
        let c = parse_set_cookie_with_psl(
            "evil=1; Domain=co.uk",
            "example.co.uk",
            "/",
            0,
            None,
        )
        .unwrap();
        assert_eq!(c.domain, "co.uk");
    }

    #[test]
    fn psl_keeps_existing_domain_match_check_independently() {
        // Domain-mismatch отвергается даже с PSL (RFC 6265 §5.3 step 4).
        let psl = PslProvider::new();
        assert!(parse_set_cookie_with_psl(
            "k=v; Domain=other.com",
            "example.com",
            "/",
            0,
            Some(&psl),
        )
        .is_none());
    }

    #[test]
    fn psl_idn_domain_match() {
        // Cyrillic IDN в Punycode-форме — PSL знает xn--p1ai (.рф).
        // request-host www.xn--e1afmkfd.xn--p1ai, Domain=xn--p1ai →
        // super-cookie attempt, отвергаем.
        let psl = PslProvider::new();
        assert!(parse_set_cookie_with_psl(
            "k=v; Domain=xn--p1ai",
            "www.xn--e1afmkfd.xn--p1ai",
            "/",
            0,
            Some(&psl),
        )
        .is_none());
        // Domain=xn--e1afmkfd.xn--p1ai (=пример.рф) — registrable, OK.
        let c = parse_set_cookie_with_psl(
            "k=v; Domain=xn--e1afmkfd.xn--p1ai",
            "www.xn--e1afmkfd.xn--p1ai",
            "/",
            0,
            Some(&psl),
        )
        .unwrap();
        assert_eq!(c.domain, "xn--e1afmkfd.xn--p1ai");
    }
}
