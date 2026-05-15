//! Origin tuple по HTML Living Standard §7.5 — фундамент Same-Origin
//! Policy / CORS / Mixed Content / iframe sandbox decisions.
//!
//! Поверх `lumen_core::url::Url`: тут только нормализация (scheme в lower-case,
//! host в ASCII через Punycode, явный effective port) и операции сравнения,
//! актуальные для security-проверок.
//!
//! Что **не** включено:
//! - opaque origin (HTML LS §7.5 — sandboxed iframe / data: URL получают
//!   уникальный opaque origin, не равный никому, кроме самого себя).
//!   Реализация откладывается до момента, когда у нас появится Document model;
//!   до тех пор `Origin::from_url` для нерасшиваемых схем (data:/blob:/about:)
//!   возвращает `Err(OriginError::Opaque)`.
//! - `document.domain` setter — намеренно deprecated в HTML LS, в Lumen
//!   реализовывать не будем (нарушает same-origin guarantee).

use lumen_core::url::Url;

/// «Tuple origin» = `(scheme, host, port)`. Сравнение — компонент-к-компоненту,
/// все три нормализованы:
/// - scheme — ASCII-lowercase;
/// - host — ASCII (Punycode для IDN), case-insensitive в нашей реализации
///   (host у `Url` уже хранится lowercase-нормализованным);
/// - port — effective (с учётом схемы; 80 для http, 443 для https).
///
/// Для same-origin сравнения см. [`Origin::same_origin`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

/// Ошибки извлечения origin из URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginError {
    /// Схема не поддерживается / не имеет tuple origin (data:, blob:, about:,
    /// file: и любые другие без `://host`). В терминологии HTML LS — opaque.
    Opaque,
    /// Schema поддерживается, но host пустой или неконвертируем в ASCII.
    NoHost,
}

impl std::fmt::Display for OriginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OriginError::Opaque => write!(f, "URL has no tuple origin (opaque)"),
            OriginError::NoHost => write!(f, "URL has empty/invalid host"),
        }
    }
}

impl std::error::Error for OriginError {}

impl Origin {
    /// Извлечь tuple origin из `Url`. Возвращает `Err(OriginError::Opaque)`
    /// для схем без понятия origin (data:/blob:/about:/file:/javascript:),
    /// `Err(NoHost)` если host пуст или не конвертируется в ASCII.
    ///
    /// Соответствует HTML LS §7.5 «origin of a URL» для tuple-кейса.
    pub fn from_url(url: &Url) -> Result<Self, OriginError> {
        let scheme = url.scheme();
        match scheme {
            "http" | "https" | "ws" | "wss" => {}
            _ => return Err(OriginError::Opaque),
        }
        let host = url
            .host_ascii()
            .map_err(|_| OriginError::NoHost)?
            .to_ascii_lowercase();
        if host.is_empty() {
            return Err(OriginError::NoHost);
        }
        // `Url::effective_port` пока знает только http/https. Для ws/wss
        // фолбэк локально — пока URL parser в lumen-core не расширен.
        let port = url
            .port()
            .or_else(|| url.effective_port())
            .or_else(|| default_port_for(scheme))
            .ok_or(OriginError::NoHost)?;
        Ok(Self {
            scheme: scheme.to_ascii_lowercase(),
            host,
            port,
        })
    }

    /// Конструктор из готовых компонентов (для тестов и внутренних случаев,
    /// когда `Url` ещё не построен). Scheme — приводится к lower-case.
    pub fn new(scheme: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            scheme: scheme.into().to_ascii_lowercase(),
            host: host.into().to_ascii_lowercase(),
            port,
        }
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Same-origin сравнение по HTML LS §7.5 «same origin» для tuple-origin-ов:
    /// все три компонента (scheme, host, port) совпадают.
    ///
    /// Opaque origin-ы здесь не существуют — `from_url` отказался бы их строить;
    /// в HTML LS opaque origin same-origin **только** с самим собой, не с другим
    /// opaque, и эту инвариант мы будем держать через Rust identity при появлении
    /// Document model.
    pub fn same_origin(&self, other: &Self) -> bool {
        self == other
    }

    /// «Potentially trustworthy origin» по W3C Secure Contexts §3.1:
    /// - scheme = `https` / `wss` — всегда trustworthy;
    /// - host = `localhost` или поддомен `.localhost` — trustworthy;
    /// - host = IPv4 loopback `127.0.0.0/8` — trustworthy;
    /// - host = IPv6 loopback `::1` (любое forma `[::1]`) — trustworthy;
    /// - остальное — нет.
    ///
    /// `file:` URLs тут не возникают: они opaque и до сюда не дойдут. UA-схемы
    /// (`about:`, `data:`, `blob:`) — то же.
    pub fn is_potentially_trustworthy(&self) -> bool {
        match self.scheme.as_str() {
            "https" | "wss" => return true,
            _ => {}
        }
        is_loopback_host(&self.host)
    }

    /// Сериализация origin в каноническую форму для заголовков HTTP (`Origin:`,
    /// `Sec-Fetch-Site:` базовая логика) и для cookie-domain matching сверху.
    /// Формат:
    /// - `scheme://host` если port — default для схемы;
    /// - `scheme://host:port` иначе.
    ///
    /// Соответствует HTML LS §7.5 «serialization of an origin».
    pub fn serialize(&self) -> String {
        if Some(self.port) == default_port_for(&self.scheme) {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.serialize())
    }
}

fn default_port_for(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

/// Поддерживает literal loopback из W3C Secure Contexts §3.1: имя `localhost`
/// и любой `*.localhost`, IPv4 `127.0.0.0/8`, IPv6 `::1`. Bracketed-форма
/// IPv6 в `Url::host` хранится с обрамляющими `[]` (HTTP request line);
/// принимаем оба варианта.
fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if is_ipv4_loopback(host) {
        return true;
    }
    let stripped = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    is_ipv6_loopback(stripped)
}

fn is_ipv4_loopback(host: &str) -> bool {
    let mut parts = host.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if first != "127" {
        return false;
    }
    let mut count = 1;
    for octet in parts {
        if octet.parse::<u8>().is_err() {
            return false;
        }
        count += 1;
    }
    count == 4
}

fn is_ipv6_loopback(host: &str) -> bool {
    // Минимально-корректный матчер на `::1` в нормализованной форме:
    // `::1`, `0:0:0:0:0:0:0:1`, `0000:0000:0000:0000:0000:0000:0000:0001`.
    if host == "::1" {
        return true;
    }
    let parts: Vec<&str> = host.split(':').collect();
    if parts.len() != 8 {
        return false;
    }
    for (i, p) in parts.iter().enumerate() {
        let v = match u32::from_str_radix(p, 16) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let expected = if i == 7 { 1 } else { 0 };
        if v != expected {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn http_origin_default_port() {
        let o = Origin::from_url(&url("http://example.com/path?q=1")).unwrap();
        assert_eq!(o.scheme(), "http");
        assert_eq!(o.host(), "example.com");
        assert_eq!(o.port(), 80);
        assert_eq!(o.serialize(), "http://example.com");
    }

    #[test]
    fn https_origin_default_port_443() {
        let o = Origin::from_url(&url("https://example.com/")).unwrap();
        assert_eq!(o.port(), 443);
        assert_eq!(o.serialize(), "https://example.com");
    }

    #[test]
    fn https_explicit_nonstandard_port_shows_in_serialization() {
        let o = Origin::from_url(&url("https://example.com:8443/")).unwrap();
        assert_eq!(o.port(), 8443);
        assert_eq!(o.serialize(), "https://example.com:8443");
    }

    #[test]
    fn http_explicit_default_port_omitted() {
        let o = Origin::from_url(&url("http://example.com:80/")).unwrap();
        assert_eq!(o.serialize(), "http://example.com");
    }

    #[test]
    fn host_lowercased() {
        let o = Origin::from_url(&url("https://Example.COM/")).unwrap();
        assert_eq!(o.host(), "example.com");
    }

    #[test]
    fn scheme_lowercased_via_new() {
        let o = Origin::new("HTTPS", "Example.com", 443);
        assert_eq!(o.scheme(), "https");
        assert_eq!(o.host(), "example.com");
    }

    #[test]
    fn data_url_is_opaque() {
        assert_eq!(
            Origin::from_url(&url("data:text/plain,hello")).unwrap_err(),
            OriginError::Opaque,
        );
    }

    #[test]
    fn file_url_is_opaque() {
        assert_eq!(
            Origin::from_url(&url("file:///etc/passwd")).unwrap_err(),
            OriginError::Opaque,
        );
    }

    #[test]
    fn ftp_is_opaque() {
        // FTP в Lumen не поддерживается транспортом, и concept origin для
        // него у нас не определён. Если когда-то добавим — тест поменяется.
        assert_eq!(
            Origin::from_url(&url("ftp://files.example.com/")).unwrap_err(),
            OriginError::Opaque,
        );
    }

    #[test]
    fn ws_and_wss_have_tuple_origin() {
        let ws = Origin::from_url(&url("ws://chat.example.com/")).unwrap();
        assert_eq!(ws.port(), 80);
        let wss = Origin::from_url(&url("wss://chat.example.com/")).unwrap();
        assert_eq!(wss.port(), 443);
    }

    #[test]
    fn same_origin_basic() {
        let a = Origin::from_url(&url("https://example.com/a")).unwrap();
        let b = Origin::from_url(&url("https://example.com/b?x=1")).unwrap();
        assert!(a.same_origin(&b));
    }

    #[test]
    fn different_scheme_is_cross_origin() {
        let http = Origin::from_url(&url("http://example.com/")).unwrap();
        let https = Origin::from_url(&url("https://example.com/")).unwrap();
        assert!(!http.same_origin(&https));
    }

    #[test]
    fn different_host_is_cross_origin() {
        let a = Origin::from_url(&url("https://a.example.com/")).unwrap();
        let b = Origin::from_url(&url("https://b.example.com/")).unwrap();
        assert!(!a.same_origin(&b));
    }

    #[test]
    fn different_port_is_cross_origin() {
        let a = Origin::from_url(&url("https://example.com/")).unwrap();
        let b = Origin::from_url(&url("https://example.com:8443/")).unwrap();
        assert!(!a.same_origin(&b));
    }

    #[test]
    fn https_is_potentially_trustworthy() {
        let o = Origin::from_url(&url("https://example.com/")).unwrap();
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn wss_is_potentially_trustworthy() {
        let o = Origin::from_url(&url("wss://chat.example.com/")).unwrap();
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn http_to_public_host_not_trustworthy() {
        let o = Origin::from_url(&url("http://example.com/")).unwrap();
        assert!(!o.is_potentially_trustworthy());
    }

    #[test]
    fn http_to_localhost_is_trustworthy() {
        let o = Origin::from_url(&url("http://localhost/")).unwrap();
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn http_to_localhost_subdomain_trustworthy() {
        let o = Origin::from_url(&url("http://dev.app.localhost/")).unwrap();
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn http_to_127_0_0_1_trustworthy() {
        let o = Origin::from_url(&url("http://127.0.0.1:3000/")).unwrap();
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn http_to_127_x_y_z_trustworthy() {
        // 127.0.0.0/8 — весь блок loopback по RFC 3330.
        let o = Origin::from_url(&url("http://127.42.7.9/")).unwrap();
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn http_to_ipv4_non_loopback_not_trustworthy() {
        let o = Origin::from_url(&url("http://10.0.0.1/")).unwrap();
        assert!(!o.is_potentially_trustworthy());
    }

    // IPv6 bracketed-форму (`http://[::1]/`) `lumen-core::url::Url` пока
    // не парсит — `:` в `[::1]` ломает `rfind(':')` в parse_authority.
    // Поэтому проверяем IPv6-loopback напрямую через `Origin::new` —
    // как только URL parser получит IPv6 brackets, тесты на parse поднимем
    // отдельной задачей.

    #[test]
    fn ipv6_loopback_short_form_trustworthy() {
        let o = Origin::new("http", "[::1]", 80);
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn ipv6_expanded_loopback_trustworthy() {
        let o = Origin::new("http", "[0:0:0:0:0:0:0:1]", 80);
        assert!(o.is_potentially_trustworthy());
    }

    #[test]
    fn ipv6_non_loopback_not_trustworthy() {
        let o = Origin::new("http", "[2001:db8::1]", 80);
        assert!(!o.is_potentially_trustworthy());
    }

    #[test]
    fn serialize_and_display_match() {
        let o = Origin::from_url(&url("https://example.com:8443/x")).unwrap();
        assert_eq!(format!("{o}"), o.serialize());
    }

    #[test]
    fn idn_host_punycoded_in_origin() {
        // host_ascii конвертирует IDN — origin должен хранить Punycode.
        let o = Origin::from_url(&url("https://пример.рф/")).unwrap();
        assert!(
            o.host().starts_with("xn--"),
            "host expected punycoded, got {:?}",
            o.host()
        );
    }
}
