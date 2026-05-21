//! Lumen URL — структурированный тип.
//!
//! Парсит вход в поля (scheme/host/port/path/query/fragment) согласно
//! упрощённой грамматике RFC 3986 §3, ограниченной нашим scope: схемы
//! `http`, `https`, `file`, `data`. Хранит исходные Unicode-байты host
//! как есть; ASCII-форма (Punycode) доступна через [`Url::host_ascii`] —
//! по соглашению из decisions log она нужна только в network-слое
//! (DNS, TLS SNI, Host header). Адресная строка показывает оригинал.
//!
//! Это swap-point из §11 плана: тонкая обёртка над String заменена
//! на структуру с явными полями, потребители (network, shell) обращаются
//! к полям через аксессоры, никто из них больше не парсит URL ad-hoc.
//!
//! Сознательно не реализуем здесь:
//! - WHATWG URL Standard полностью (percent-decoding, IDNA UTS #46,
//!   `.`/`..` нормализация в path) — добавим, когда упрёмся;
//! - userinfo (`user:pass@`) — отбрасываем при парсинге, в http(s) deprecated.

use crate::error::{Error, Result};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Url {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
    serialized: String,
}

impl Url {
    /// Распарсить URL. Минимально требуется непустая `scheme:`.
    /// Для всех известных нам схем ожидаем `scheme://`.
    pub fn parse(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Err(Error::InvalidUrl("empty URL".into()));
        }

        let (scheme, rest) = split_scheme(s)?;

        // hier-part: для http/https/file требуем authority через `//`.
        // Для `data:` (и любых других unknown схем) — отдаём всё как path.
        let has_authority = rest.starts_with("//");

        let (host, port, path_start) = if has_authority {
            let after_slashes = &rest[2..];
            let auth_end = after_slashes
                .find(['/', '?', '#'])
                .unwrap_or(after_slashes.len());
            let authority = &after_slashes[..auth_end];
            let (host, port) = parse_authority(authority, &scheme)?;
            (host, port, &after_slashes[auth_end..])
        } else {
            (String::new(), None, rest)
        };

        let (path, after_path) = split_at_any(path_start, &['?', '#']);
        let mut path = path.to_owned();
        if has_authority && path.is_empty() {
            path.push('/');
        }

        let (query, after_query) = if let Some(after_q) = after_path.strip_prefix('?') {
            let (q, rest) = split_at_any(after_q, &['#']);
            (Some(q.to_owned()), rest)
        } else {
            (None, after_path)
        };

        let fragment = after_query.strip_prefix('#').map(str::to_owned);

        let serialized = serialize(
            &scheme,
            &host,
            port,
            &path,
            query.as_deref(),
            fragment.as_deref(),
        );

        Ok(Self {
            scheme,
            host,
            port,
            path,
            query,
            fragment,
            serialized,
        })
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// Порт с учётом дефолтов известных схем.
    pub fn effective_port(&self) -> Option<u16> {
        self.port.or_else(|| default_port(&self.scheme))
    }

    /// Host в ASCII-форме (Punycode) — для DNS, TLS SNI, Host header.
    /// Пустой host (например, `data:`) даёт пустую строку без ошибки.
    pub fn host_ascii(&self) -> Result<String> {
        if self.host.is_empty() {
            return Ok(String::new());
        }
        crate::idn::domain_to_ascii(&self.host).map_err(|e| {
            Error::InvalidUrl(format!("idn conversion failed for '{}': {e}", self.host))
        })
    }

    /// Path + `?query` (без fragment) — для HTTP request line.
    pub fn path_and_query(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }

    /// Разрешить относительный или абсолютный `reference` относительно `self`.
    /// Упрощённый алгоритм RFC 3986 §5.3 без нормализации `.`/`..`.
    pub fn resolve(&self, reference: &str) -> Result<Self> {
        if has_scheme(reference) {
            return Self::parse(reference);
        }
        if let Some(rest) = reference.strip_prefix("//") {
            return Self::parse(&format!("{}://{}", self.scheme, rest));
        }
        let base_authority = self.authority_for_serialize();
        if reference.starts_with('/') {
            return Self::parse(&format!(
                "{}://{}{}",
                self.scheme, base_authority, reference
            ));
        }
        if reference.is_empty() {
            return Ok(self.clone());
        }
        if let Some(frag) = reference.strip_prefix('#') {
            let mut next = self.clone();
            next.fragment = Some(frag.to_owned());
            next.serialized = serialize(
                &next.scheme,
                &next.host,
                next.port,
                &next.path,
                next.query.as_deref(),
                next.fragment.as_deref(),
            );
            return Ok(next);
        }
        if let Some(after_q) = reference.strip_prefix('?') {
            let (q, frag) = split_at_any(after_q, &['#']);
            let fragment = frag.strip_prefix('#').map(str::to_owned);
            let mut next = self.clone();
            next.query = Some(q.to_owned());
            next.fragment = fragment;
            next.serialized = serialize(
                &next.scheme,
                &next.host,
                next.port,
                &next.path,
                next.query.as_deref(),
                next.fragment.as_deref(),
            );
            return Ok(next);
        }
        let dir = self
            .path
            .rfind('/')
            .map(|i| &self.path[..=i])
            .unwrap_or("/");
        Self::parse(&format!(
            "{}://{}{}{}",
            self.scheme, base_authority, dir, reference
        ))
    }

    fn authority_for_serialize(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{}", self.host, p),
            None => self.host.clone(),
        }
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized)
    }
}

// ── Парсинг ──────────────────────────────────────────────────────────────────

fn has_scheme(s: &str) -> bool {
    // scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

fn split_scheme(s: &str) -> Result<(String, &str)> {
    if !has_scheme(s) {
        return Err(Error::InvalidUrl(format!("missing scheme: {s:?}")));
    }
    let colon = s.find(':').expect("has_scheme guaranteed `:`");
    let scheme = s[..colon].to_ascii_lowercase();
    Ok((scheme, &s[colon + 1..]))
}

fn parse_authority(authority: &str, scheme: &str) -> Result<(String, Option<u16>)> {
    // Отбрасываем userinfo (`user:pass@`) — для http(s) deprecated.
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };

    if host_port.is_empty() {
        // `file://path` имеет пустой host — это нормально.
        if scheme == "file" {
            return Ok((String::new(), None));
        }
        return Err(Error::InvalidUrl(format!("empty host in {scheme}://")));
    }

    match host_port.rfind(':') {
        Some(i) => {
            let host = host_port[..i].to_owned();
            let port_str = &host_port[i + 1..];
            if port_str.is_empty() {
                Ok((host, None))
            } else {
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| Error::InvalidUrl(format!("invalid port: {port_str:?}")))?;
                Ok((host, Some(port)))
            }
        }
        None => Ok((host_port.to_owned(), None)),
    }
}

fn split_at_any<'a>(s: &'a str, chars: &[char]) -> (&'a str, &'a str) {
    match s.find(|c: char| chars.contains(&c)) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws"  => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

fn serialize(
    scheme: &str,
    host: &str,
    port: Option<u16>,
    path: &str,
    query: Option<&str>,
    fragment: Option<&str>,
) -> String {
    let mut out = String::with_capacity(scheme.len() + host.len() + path.len() + 8);
    out.push_str(scheme);
    out.push(':');
    if !host.is_empty() || scheme == "http" || scheme == "https" || scheme == "file" {
        out.push_str("//");
        out.push_str(host);
        if let Some(p) = port {
            out.push(':');
            out.push_str(&p.to_string());
        }
    }
    out.push_str(path);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_fails() {
        assert!(Url::parse("").is_err());
    }

    #[test]
    fn parse_no_scheme_fails() {
        assert!(Url::parse("example.com").is_err());
    }

    #[test]
    fn parse_https_basic() {
        let u = Url::parse("https://example.com/path").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host(), "example.com");
        assert_eq!(u.port(), None);
        assert_eq!(u.path(), "/path");
        assert_eq!(u.query(), None);
        assert_eq!(u.fragment(), None);
        assert_eq!(u.effective_port(), Some(443));
    }

    #[test]
    fn parse_http_default_port_path_normalized() {
        let u = Url::parse("http://example.com").unwrap();
        assert_eq!(u.scheme(), "http");
        assert_eq!(u.path(), "/");
        assert_eq!(u.effective_port(), Some(80));
        assert_eq!(u.as_str(), "http://example.com/");
    }

    #[test]
    fn parse_explicit_port() {
        let u = Url::parse("http://localhost:8080/index.html").unwrap();
        assert_eq!(u.port(), Some(8080));
        assert_eq!(u.effective_port(), Some(8080));
        assert_eq!(u.path(), "/index.html");
    }

    #[test]
    fn parse_query_and_fragment() {
        let u = Url::parse("https://x.test/a/b?foo=1&bar=2#sec").unwrap();
        assert_eq!(u.path(), "/a/b");
        assert_eq!(u.query(), Some("foo=1&bar=2"));
        assert_eq!(u.fragment(), Some("sec"));
        assert_eq!(u.path_and_query(), "/a/b?foo=1&bar=2");
    }

    #[test]
    fn parse_fragment_only() {
        let u = Url::parse("https://x.test/#frag").unwrap();
        assert_eq!(u.path(), "/");
        assert_eq!(u.fragment(), Some("frag"));
    }

    #[test]
    fn parse_query_no_fragment() {
        let u = Url::parse("https://x.test/?q=1").unwrap();
        assert_eq!(u.query(), Some("q=1"));
        assert_eq!(u.fragment(), None);
    }

    #[test]
    fn parse_scheme_case_insensitive() {
        let u = Url::parse("HTTPS://Example.com/").unwrap();
        assert_eq!(u.scheme(), "https");
        // host case оставляем как есть (DNS case-insensitive, но семантически
        // не наше дело нормализовать — некоторые WAF чувствительны).
        assert_eq!(u.host(), "Example.com");
    }

    #[test]
    fn parse_cyrillic_idn_unicode_preserved() {
        let u = Url::parse("https://президент.рф/").unwrap();
        assert_eq!(u.host(), "президент.рф");
        assert_eq!(u.as_str(), "https://президент.рф/");
    }

    #[test]
    fn host_ascii_punycode() {
        let u = Url::parse("https://президент.рф/path").unwrap();
        assert_eq!(u.host_ascii().unwrap(), "xn--d1abbgf6aiiy.xn--p1ai");
    }

    #[test]
    fn host_ascii_empty_for_data_url() {
        let u = Url::parse("data:text/plain,hello").unwrap();
        assert_eq!(u.scheme(), "data");
        assert_eq!(u.host(), "");
        assert_eq!(u.host_ascii().unwrap(), "");
    }

    #[test]
    fn file_url_no_authority_after_double_slash() {
        let u = Url::parse("file:///tmp/a.html").unwrap();
        assert_eq!(u.scheme(), "file");
        assert_eq!(u.host(), "");
        assert_eq!(u.path(), "/tmp/a.html");
    }

    #[test]
    fn userinfo_dropped() {
        let u = Url::parse("http://user:pass@example.com/").unwrap();
        assert_eq!(u.host(), "example.com");
    }

    #[test]
    fn invalid_port_fails() {
        assert!(Url::parse("http://example.com:notaport/").is_err());
    }

    #[test]
    fn empty_host_fails_for_http() {
        assert!(Url::parse("http:///path").is_err());
    }

    #[test]
    fn as_str_roundtrip_with_query_fragment() {
        let u = Url::parse("https://x.test:8443/a?q=1#f").unwrap();
        assert_eq!(u.as_str(), "https://x.test:8443/a?q=1#f");
    }

    #[test]
    fn resolve_absolute() {
        let base = Url::parse("https://example.com/page").unwrap();
        let r = base.resolve("https://other.com/foo").unwrap();
        assert_eq!(r.as_str(), "https://other.com/foo");
    }

    #[test]
    fn resolve_protocol_relative() {
        let base = Url::parse("https://example.com/page").unwrap();
        let r = base.resolve("//cdn.test/lib.js").unwrap();
        assert_eq!(r.as_str(), "https://cdn.test/lib.js");
    }

    #[test]
    fn resolve_absolute_path() {
        let base = Url::parse("https://example.com/dir/page").unwrap();
        let r = base.resolve("/style.css").unwrap();
        assert_eq!(r.as_str(), "https://example.com/style.css");
    }

    #[test]
    fn resolve_relative_path() {
        let base = Url::parse("https://example.com/dir/page.html").unwrap();
        let r = base.resolve("css/style.css").unwrap();
        assert_eq!(r.as_str(), "https://example.com/dir/css/style.css");
    }

    #[test]
    fn resolve_relative_root_path() {
        let base = Url::parse("https://example.com/").unwrap();
        let r = base.resolve("about.html").unwrap();
        assert_eq!(r.as_str(), "https://example.com/about.html");
    }

    #[test]
    fn resolve_fragment_only() {
        let base = Url::parse("https://example.com/page?q=1").unwrap();
        let r = base.resolve("#sec").unwrap();
        assert_eq!(r.as_str(), "https://example.com/page?q=1#sec");
    }

    #[test]
    fn resolve_query_only() {
        let base = Url::parse("https://example.com/page?old=1#f").unwrap();
        let r = base.resolve("?new=2").unwrap();
        assert_eq!(r.as_str(), "https://example.com/page?new=2");
    }

    #[test]
    fn resolve_preserves_port() {
        let base = Url::parse("http://localhost:8080/dir/page").unwrap();
        let r = base.resolve("/abs").unwrap();
        assert_eq!(r.as_str(), "http://localhost:8080/abs");
        let r2 = base.resolve("rel.html").unwrap();
        assert_eq!(r2.as_str(), "http://localhost:8080/dir/rel.html");
    }
}
