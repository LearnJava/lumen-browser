//! COMPAT-ERRORS: типизация сбоя навигации главного документа кодами Chromium.
//!
//! Сетевой слой возвращает `Error::Network(String)`, но сообщения имеют
//! стабильные префиксы по точке возникновения (`resolve …`, `connect …`,
//! `TLS handshake …`, `too many redirects`) и несут номер ошибки ОС
//! (`(os error 10061)`). [`NetErrorCode::classify`] разбирает их в код, под
//! который написаны русскоязычные инструкции на странице ошибки
//! (`docs/research/site-compat/report.md` §8.4, COMPAT-ERR-00): внизу страницы
//! мелко стоит имя как в Chromium (`ERR_NAME_NOT_RESOLVED`).

/// Код сбоя навигации; имя — как в `net_error_list.h` Chromium.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetErrorCode {
    /// Имя хоста не разрешилось (DNS не ответил или ответ пуст).
    NameNotResolved,
    /// Сервер отверг соединение (RST на SYN).
    ConnectionRefused,
    /// Сервер не ответил за отведённое время.
    ConnectionTimedOut,
    /// Соединение сброшено уже после установки.
    ConnectionReset,
    /// Соединение закрыто до конца ответа.
    ConnectionClosed,
    /// Сеть или хост недостижимы (нет маршрута).
    AddressUnreachable,
    /// Цепочка перенаправлений слишком длинная или без `Location`.
    TooManyRedirects,
    /// Ответ не разобрался как HTTP.
    InvalidResponse,
    /// Рукопожатие TLS не удалось не по причине сертификата.
    SslProtocolError,
    /// Локального файла нет.
    FileNotFound,
    /// Адрес не разобрался.
    InvalidUrl,
    /// Схема адреса не поддерживается.
    UnknownUrlScheme,
    /// Запрос заблокирован политикой браузера (CORS, CSP, перехватчик).
    BlockedByClient,
    /// Сервер ответил 5xx и не прислал тела: показывать нечего, кроме самого статуса.
    HttpStatus(u16),
    /// Причина не распознана.
    Failed,
}

impl NetErrorCode {
    /// Имя кода как в Chromium, например `ERR_NAME_NOT_RESOLVED`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::NameNotResolved => "ERR_NAME_NOT_RESOLVED",
            Self::ConnectionRefused => "ERR_CONNECTION_REFUSED",
            Self::ConnectionTimedOut => "ERR_CONNECTION_TIMED_OUT",
            Self::ConnectionReset => "ERR_CONNECTION_RESET",
            Self::ConnectionClosed => "ERR_CONNECTION_CLOSED",
            Self::AddressUnreachable => "ERR_ADDRESS_UNREACHABLE",
            Self::TooManyRedirects => "ERR_TOO_MANY_REDIRECTS",
            Self::InvalidResponse => "ERR_INVALID_RESPONSE",
            Self::SslProtocolError => "ERR_SSL_PROTOCOL_ERROR",
            Self::FileNotFound => "ERR_FILE_NOT_FOUND",
            Self::InvalidUrl => "ERR_INVALID_URL",
            Self::UnknownUrlScheme => "ERR_UNKNOWN_URL_SCHEME",
            Self::BlockedByClient => "ERR_BLOCKED_BY_CLIENT",
            Self::HttpStatus(_) => "ERR_HTTP_RESPONSE_CODE_FAILURE",
            Self::Failed => "ERR_FAILED",
        }
    }

    /// Разбирает текст ошибки загрузки (`Error::to_string()` сетевого слоя,
    /// `io::Error` файловой загрузки) в код. Приоритет у префиксов точки
    /// возникновения, затем — номер ошибки ОС, затем — слова.
    #[must_use]
    pub fn classify(message: &str) -> Self {
        let msg = message.strip_prefix("network error: ").unwrap_or(message);
        let lower = msg.to_ascii_lowercase();
        if let Some(status) = lower.strip_prefix(HTTP_STATUS_PREFIX).and_then(|n| n.trim().parse().ok()) {
            return Self::HttpStatus(status);
        }
        if lower.starts_with("blocked") || lower.contains("err_blocked_by_client") {
            return Self::BlockedByClient;
        }
        if lower.starts_with("resolve ") && !lower.starts_with("resolve redirect") {
            return Self::NameNotResolved;
        }
        if lower.contains("too many redirects") || lower.contains("redirect without location") {
            return Self::TooManyRedirects;
        }
        if lower.starts_with("unsupported scheme") {
            return Self::UnknownUrlScheme;
        }
        if lower.starts_with("invalid url") || lower.starts_with("invalid hostname") {
            return Self::InvalidUrl;
        }
        if lower.starts_with("tls handshake") || lower.starts_with("unexpected alpn") {
            return Self::SslProtocolError;
        }
        if let Some(code) = os_error_number(&lower).and_then(code_from_os_error) {
            return code;
        }
        if lower.contains("timed out") || lower.contains("timeout") {
            return Self::ConnectionTimedOut;
        }
        if lower.contains("refused") {
            return Self::ConnectionRefused;
        }
        if lower.contains("connection reset") {
            return Self::ConnectionReset;
        }
        if lower.contains("eof") || lower.contains("connection abort") || lower.contains("broken pipe") {
            return Self::ConnectionClosed;
        }
        if lower.contains("no such file") || lower.contains("not found") || lower.contains("cannot find the") {
            return Self::FileNotFound;
        }
        if lower.starts_with("connect ") {
            return Self::AddressUnreachable;
        }
        if lower.contains("malformed") || lower.contains("invalid status") || lower.contains("invalid http") {
            return Self::InvalidResponse;
        }
        Self::Failed
    }
}

/// Начало сообщения о пустом ответе 5xx — см. [`http_status_message`].
const HTTP_STATUS_PREFIX: &str = "http status ";

/// Сообщение для `LoadEvent::LoadError`, когда главный документ пришёл с кодом
/// 5xx и пустым телом; [`NetErrorCode::classify`] возвращает по нему
/// [`NetErrorCode::HttpStatus`].
#[must_use]
pub fn http_status_message(status: u16) -> String {
    format!("{HTTP_STATUS_PREFIX}{status}")
}

/// Номер из хвоста `(os error N)` — `std::io::Error` печатает его на любом
/// языке ОС, в отличие от самого текста.
fn os_error_number(lower: &str) -> Option<u32> {
    let tail = &lower[lower.rfind("(os error ")? + "(os error ".len()..];
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Windows (WSA*) и POSIX (errno) номера сокетных ошибок.
fn code_from_os_error(n: u32) -> Option<NetErrorCode> {
    match n {
        10060 | 110 => Some(NetErrorCode::ConnectionTimedOut),
        10061 | 111 => Some(NetErrorCode::ConnectionRefused),
        10054 | 104 => Some(NetErrorCode::ConnectionReset),
        10053 | 103 => Some(NetErrorCode::ConnectionClosed),
        10050 | 10051 | 10065 | 101 | 113 => Some(NetErrorCode::AddressUnreachable),
        10001..=10003 | 11001 | 11004 => Some(NetErrorCode::NameNotResolved),
        2 | 3 => Some(NetErrorCode::FileNotFound),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(msg: &str) -> NetErrorCode {
        NetErrorCode::classify(msg)
    }

    #[test]
    fn dns_failure_is_name_not_resolved() {
        assert_eq!(code("network error: resolve nope.invalid: failed (os error 11001)"), NetErrorCode::NameNotResolved);
        assert_eq!(code("resolve a.b: no addresses returned"), NetErrorCode::NameNotResolved);
    }

    #[test]
    fn redirect_resolution_is_not_dns() {
        assert_eq!(code("resolve redirect '/x': invalid url"), NetErrorCode::Failed);
        assert_eq!(code("too many redirects"), NetErrorCode::TooManyRedirects);
    }

    #[test]
    fn connect_errors_use_os_number_not_localised_text() {
        let ru = "connect a.example:443: Подключение не установлено (os error 10061)";
        assert_eq!(code(ru), NetErrorCode::ConnectionRefused);
        assert_eq!(code("connect a:80: x (os error 10060)"), NetErrorCode::ConnectionTimedOut);
        assert_eq!(code("connect a:80: x (os error 111)"), NetErrorCode::ConnectionRefused);
        assert_eq!(code("connect a:80: x (os error 10051)"), NetErrorCode::AddressUnreachable);
    }

    #[test]
    fn connect_without_known_cause_is_unreachable() {
        assert_eq!(code("connect a:80: whatever"), NetErrorCode::AddressUnreachable);
        assert_eq!(code("connect a:80: timed out"), NetErrorCode::ConnectionTimedOut);
    }

    #[test]
    fn read_phase_failures() {
        assert_eq!(code("read body: connection reset by peer"), NetErrorCode::ConnectionReset);
        assert_eq!(code("H2: unexpected EOF"), NetErrorCode::ConnectionClosed);
        assert_eq!(code("read timed out"), NetErrorCode::ConnectionTimedOut);
    }

    #[test]
    fn scheme_url_tls_and_policy() {
        assert_eq!(code("unsupported scheme: gopher"), NetErrorCode::UnknownUrlScheme);
        assert_eq!(code("TLS handshake: alert 40"), NetErrorCode::SslProtocolError);
        assert_eq!(code("blocked: cors-actual: no ACAO"), NetErrorCode::BlockedByClient);
        assert_eq!(code("invalid url: relative"), NetErrorCode::InvalidUrl);
    }

    #[test]
    fn file_and_unknown() {
        assert_eq!(code("No such file or directory (os error 2)"), NetErrorCode::FileNotFound);
        assert_eq!(code("что-то странное"), NetErrorCode::Failed);
    }

    #[test]
    fn empty_server_error_roundtrips_through_the_message() {
        assert_eq!(code(&http_status_message(503)), NetErrorCode::HttpStatus(503));
        assert_eq!(code("network error: http status 502"), NetErrorCode::HttpStatus(502));
        assert_eq!(code("http status x"), NetErrorCode::Failed);
        assert_eq!(NetErrorCode::HttpStatus(504).name(), "ERR_HTTP_RESPONSE_CODE_FAILURE");
    }

    #[test]
    fn names_follow_chromium() {
        assert_eq!(NetErrorCode::NameNotResolved.name(), "ERR_NAME_NOT_RESOLVED");
        assert_eq!(NetErrorCode::Failed.name(), "ERR_FAILED");
    }
}
