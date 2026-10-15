//! COMPAT-ERRORS: размеченный набор поломок навигации (`report.md` §8.4,
//! «Проверка группы»: ≥60 случаев по классам, ≥90% верных вердиктов).
//!
//! Строки — тексты, которые сетевой слой действительно кладёт в
//! `Error::Network`/`NetFailure` (формат — `format!` в `crates/network`),
//! в том числе с локализованным текстом ОС: код определяют номер ошибки и
//! префикс точки возникновения, а не слова. Ожидаемый код — тот, под который
//! написана страница (`NetErrorCode::name` — имя Chromium).
//!
//! Тест требует полного совпадения: расхождение — либо дефект `classify`, либо
//! неверная разметка, а не допустимая погрешность. Порог приёмки (90%) проверяется
//! отдельно и защищает от тихого раздувания набора «Failed».

use crate::net_error::NetErrorCode as C;

/// (класс поломки, текст `Error::to_string()`, ожидаемый код).
const CORPUS: &[(&str, &str, C)] = &[
    // --- DNS ---
    ("dns", "network error: resolve nope.invalid:443: failed to lookup address information (os error 11001)", C::NameNotResolved),
    ("dns", "resolve a.b: no addresses returned", C::NameNotResolved),
    ("dns", "resolve example.com:80: Temporary failure in name resolution", C::NameNotResolved),
    ("dns", "resolve example.com:443: Этот хост неизвестен. (os error 11001)", C::NameNotResolved),
    ("dns", "resolve example.com:443: Не удалось разрешить имя (os error 11004)", C::NameNotResolved),
    ("dns", "resolve SOCKS5 proxy p.example:1080: no addresses returned", C::NameNotResolved),
    ("dns", "resolve xn--80ak6aa92e.com:443: DoH: RCODE=3", C::NameNotResolved),
    // --- connect ---
    ("refused", "connect example.com:443: Подключение не установлено, т.к. конечный компьютер отверг запрос на подключение. (os error 10061)", C::ConnectionRefused),
    ("refused", "connect 127.0.0.1:8080: Connection refused (os error 111)", C::ConnectionRefused),
    ("refused", "connect [::1]:80: refused", C::ConnectionRefused),
    ("refused", "connect SOCKS5 proxy p.example:1080: Connection refused (os error 111)", C::ConnectionRefused),
    ("timeout", "connect 203.0.113.7:443: Попытка установить соединение была безуспешной, т.к. от другого компьютера за требуемое время не получен нужный отклик (os error 10060)", C::ConnectionTimedOut),
    ("timeout", "connect 203.0.113.7:443: Connection timed out (os error 110)", C::ConnectionTimedOut),
    ("timeout", "connect a.example:80: timed out", C::ConnectionTimedOut),
    ("timeout", "connect a.example:80: connection attempt timed out after 10s", C::ConnectionTimedOut),
    ("unreachable", "connect [2001:db8::1]:443: Network is unreachable (os error 101)", C::AddressUnreachable),
    ("unreachable", "connect 10.0.0.9:80: No route to host (os error 113)", C::AddressUnreachable),
    ("unreachable", "connect a:80: Попытка выполнить операцию на недоступной сети (os error 10051)", C::AddressUnreachable),
    ("unreachable", "connect a:80: Сокет не может быть использован: хост недоступен (os error 10065)", C::AddressUnreachable),
    ("unreachable", "connect a.example:80: no usable address", C::AddressUnreachable),
    // --- обрыв после соединения ---
    ("reset", "read status: Удаленный хост принудительно разорвал существующее подключение. (os error 10054)", C::ConnectionReset),
    ("reset", "read status: Connection reset by peer (os error 104)", C::ConnectionReset),
    ("reset", "read body: connection reset by peer", C::ConnectionReset),
    ("reset", "read header: connection reset", C::ConnectionReset),
    ("reset", "flush request: Connection reset by peer (os error 104)", C::ConnectionReset),
    ("closed", "write request: Broken pipe (os error 32)", C::ConnectionClosed),
    ("closed", "write request: Программа на вашем хост-компьютере разорвала установленное подключение. (os error 10053)", C::ConnectionClosed),
    ("closed", "write body: connection aborted", C::ConnectionClosed),
    ("closed", "EOF before status line", C::ConnectionClosed),
    ("closed", "EOF in headers", C::ConnectionClosed),
    ("closed", "EOF before CONNECT status line", C::ConnectionClosed),
    ("closed", "read body: failed to fill whole buffer", C::ConnectionClosed),
    ("closed", "read status: unexpected end of file", C::ConnectionClosed),
    ("closed", "H2: unexpected EOF", C::ConnectionClosed),
    ("closed", "H2 connection lost: connection closed", C::ConnectionClosed),
    ("read-timeout", "read status: timed out", C::ConnectionTimedOut),
    ("read-timeout", "read body: Попытка установить соединение была безуспешной (os error 10060)", C::ConnectionTimedOut),
    ("read-timeout", "H2 I/O: operation timed out", C::ConnectionTimedOut),
    ("read-timeout", "DoT: read length prefix: timed out", C::ConnectionTimedOut),
    // --- перенаправления ---
    ("redirects", "too many redirects", C::TooManyRedirects),
    ("redirects", "network error: too many redirects", C::TooManyRedirects),
    ("redirects", "redirect without Location", C::TooManyRedirects),
    // --- адрес и схема ---
    ("url", "invalid url: relative URL without a base", C::InvalidUrl),
    ("url", "invalid hostname 'a..b': empty label", C::InvalidUrl),
    ("url", "invalid url: empty host", C::InvalidUrl),
    ("scheme", "unsupported scheme: gopher", C::UnknownUrlScheme),
    ("scheme", "unsupported scheme: tel", C::UnknownUrlScheme),
    ("scheme", "unsupported scheme: ftp", C::UnknownUrlScheme),
    // --- TLS ---
    ("tls", "TLS handshake: received fatal alert: HandshakeFailure", C::SslProtocolError),
    ("tls", "TLS handshake: alert 40", C::SslProtocolError),
    ("tls", "TLS handshake: peer is incompatible: no shared cipher suites", C::SslProtocolError),
    ("tls", "TLS handshake over tunnel: received fatal alert: ProtocolVersion", C::SslProtocolError),
    ("tls", "unexpected ALPN protocol: spdy/3", C::SslProtocolError),
    // --- политика браузера ---
    ("blocked", "blocked: cors-actual: no Access-Control-Allow-Origin", C::BlockedByClient),
    ("blocked", "blocked: CSP connect-src", C::BlockedByClient),
    ("blocked", "blocked: mixed content", C::BlockedByClient),
    ("blocked", "net::ERR_BLOCKED_BY_CLIENT", C::BlockedByClient),
    // --- разбор ответа ---
    ("response", "bad status line: \"garbage\"", C::InvalidResponse),
    ("response", "invalid chunk size: \"zz\"", C::InvalidResponse),
    ("response", "H2: response missing :status", C::InvalidResponse),
    ("response", "malformed HTTP response", C::InvalidResponse),
    ("response", "invalid status code 99", C::InvalidResponse),
    // --- файл ---
    ("file", "No such file or directory (os error 2)", C::FileNotFound),
    ("file", "Не удаётся найти указанный файл. (os error 2)", C::FileNotFound),
    ("file", "Системе не удаётся найти указанный путь. (os error 3)", C::FileNotFound),
    ("file", "file: C:\\nope\\a.html: The system cannot find the file specified.", C::FileNotFound),
    // --- 5xx без тела ---
    ("5xx", "http status 500", C::HttpStatus(500)),
    ("5xx", "network error: http status 502", C::HttpStatus(502)),
    ("5xx", "http status 503", C::HttpStatus(503)),
    ("5xx", "http status 504", C::HttpStatus(504)),
    // --- не распознано: честное «ERR_FAILED», а не угаданный вердикт ---
    ("unknown", "что-то странное", C::Failed),
    ("unknown", "unexpected tunnel stream type", C::Failed),
    ("unknown", "resolve redirect '/x': invalid url", C::Failed),
];

/// Сколько случаев набор обязан содержать (`report.md` §8.4).
const MIN_CASES: usize = 60;
/// Порог верных вердиктов из приёмки группы.
const MIN_ACCURACY_PERCENT: usize = 90;

#[test]
fn corpus_is_labelled_and_covers_every_class() {
    assert!(CORPUS.len() >= MIN_CASES, "в наборе {} случаев, нужно ≥{MIN_CASES}", CORPUS.len());
    for class in [
        "dns", "refused", "timeout", "unreachable", "reset", "closed", "read-timeout", "redirects", "url", "scheme",
        "tls", "blocked", "response", "file", "5xx", "unknown",
    ] {
        assert!(CORPUS.iter().any(|(c, ..)| *c == class), "нет случаев класса {class}");
    }
}

#[test]
fn corpus_verdicts_are_right() {
    let wrong: Vec<String> = CORPUS
        .iter()
        .filter_map(|(class, msg, want)| {
            let got = C::classify(msg);
            (got != *want).then(|| format!("[{class}] {msg:?}: ждали {}, получили {}", want.name(), got.name()))
        })
        .collect();
    let right = CORPUS.len() - wrong.len();
    assert!(right * 100 >= CORPUS.len() * MIN_ACCURACY_PERCENT, "верных {right} из {}:\n{}", CORPUS.len(), wrong.join("\n"));
    assert!(wrong.is_empty(), "расхождения набора:\n{}", wrong.join("\n"));
}

/// Нераспознанное не выдаётся за конкретную причину и наоборот: ни одна
/// размеченная причина не скатывается в «Failed».
#[test]
fn no_labelled_cause_degrades_to_failed() {
    for (class, msg, want) in CORPUS {
        if *want != C::Failed {
            assert_ne!(C::classify(msg), C::Failed, "[{class}] {msg:?} потерял причину");
        }
    }
}
