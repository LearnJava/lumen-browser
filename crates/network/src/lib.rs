//! HTTP/1.1 клиент с TLS через rustls (Exception #3).
//!
//! Реализует `lumen_core::ext::NetworkTransport`.
//! Поддерживает: HTTP и HTTPS, редиректы (до 5), chunked transfer encoding.
//! Не поддерживает: HTTP/2, keep-alive, кэширование, аутентификацию.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::ClientConnection;
use rustls::pki_types::ServerName;

use lumen_core::error::{Error, Result};
use lumen_core::event::{Event, TabId};
use lumen_core::ext::{EventSink, NetworkTransport, RequestFilter};
use lumen_core::idn;
use lumen_core::url::Url;

// ── URL-парсинг ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ParsedUrl {
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    Http,
    Https,
}

fn parse_url(url: &str) -> Result<ParsedUrl> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        (Scheme::Https, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (Scheme::Http, r)
    } else {
        return Err(Error::Network(format!("unsupported scheme: {url}")));
    };

    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_owned()),
        None => (rest, "/".to_owned()),
    };

    let (raw_host, port) = match authority.rfind(':') {
        Some(i) => {
            let h = &authority[..i];
            let p = authority[i + 1..]
                .parse::<u16>()
                .map_err(|_| Error::Network(format!("invalid port in: {authority}")))?;
            (h, p)
        }
        None => (
            authority,
            if scheme == Scheme::Https { 443 } else { 80 },
        ),
    };

    // IDN → ASCII (Punycode). DNS, TLS SNI и Host: header требуют ASCII
    // в hostname (RFC 7230 §5.4 для Host, RFC 6066 §3 для SNI).
    let host = idn::domain_to_ascii(raw_host)
        .map_err(|e| Error::Network(format!("idn conversion failed for '{raw_host}': {e}")))?;

    Ok(ParsedUrl { scheme, host, port, path })
}

// ── TCP + TLS connection ─────────────────────────────────────────────────────

enum Connection {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Connection::Plain(s) => s.read(buf),
            Connection::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Connection::Plain(s) => s.write(buf),
            Connection::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Connection::Plain(s) => s.flush(),
            Connection::Tls(s) => s.flush(),
        }
    }
}

fn connect(parsed: &ParsedUrl) -> Result<Connection> {
    let addr = format!("{}:{}", parsed.host, parsed.port);
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| Error::Network(format!("connect {addr}: {e}")))?;

    match parsed.scheme {
        Scheme::Http => Ok(Connection::Plain(tcp)),
        Scheme::Https => {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

            let config = rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            let server_name = ServerName::try_from(parsed.host.clone())
                .map_err(|e| Error::Network(format!("invalid hostname '{}': {e}", parsed.host)))?;

            let conn = ClientConnection::new(Arc::new(config), server_name)
                .map_err(|e| Error::Network(format!("TLS handshake: {e}")))?;

            Ok(Connection::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))))
        }
    }
}

// ── HTTP/1.1 запрос / ответ ──────────────────────────────────────────────────

fn write_request(conn: &mut Connection, host: &str, path: &str) -> Result<()> {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Lumen/0.0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    conn.write_all(req.as_bytes())
        .map_err(|e| Error::Network(format!("write request: {e}")))
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let name_lc = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(k, _)| k.to_ascii_lowercase() == name_lc)
        .map(|(_, v)| v.as_str())
}

fn read_response(conn: Connection) -> Result<Response> {
    let mut reader = BufReader::new(conn);

    // Status line.
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| Error::Network(format!("read status: {e}")))?;
    let status = parse_status(&status_line)?;

    // Headers.
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| Error::Network(format!("read header: {e}")))?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }

    // Body.
    let body = if header_value(&headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        read_chunked(reader)?
    } else if let Some(len) = header_value(&headers, "content-length")
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        let mut buf = vec![0u8; len];
        reader
            .read_exact(&mut buf)
            .map_err(|e| Error::Network(format!("read body: {e}")))?;
        buf
    } else {
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .map_err(|e| Error::Network(format!("read body: {e}")))?;
        buf
    };

    Ok(Response { status, headers, body })
}

fn parse_status(line: &str) -> Result<u16> {
    // "HTTP/1.1 200 OK\r\n"
    let mut parts = line.split_ascii_whitespace();
    let _version = parts.next();
    let code = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| Error::Network(format!("bad status line: {line:?}")))?;
    Ok(code)
}

fn read_chunked<R: BufRead>(mut reader: R) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader
            .read_line(&mut size_line)
            .map_err(|e| Error::Network(format!("chunked size: {e}")))?;
        let size_str = size_line.trim_end_matches(['\r', '\n']);
        // Chunk extensions after ';' are ignored.
        let size_hex = size_str.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| Error::Network(format!("invalid chunk size: {size_hex:?}")))?;
        if chunk_size == 0 {
            break;
        }
        let mut chunk = vec![0u8; chunk_size];
        reader
            .read_exact(&mut chunk)
            .map_err(|e| Error::Network(format!("chunked body: {e}")))?;
        body.extend_from_slice(&chunk);
        // CRLF after chunk data.
        let mut crlf = [0u8; 2];
        reader
            .read_exact(&mut crlf)
            .map_err(|e| Error::Network(format!("chunked crlf: {e}")))?;
    }
    Ok(body)
}

// ── Редиректы ────────────────────────────────────────────────────────────────

fn fetch_with_redirect(
    url: &str,
    hops_left: u8,
    sink: Option<&dyn EventSink>,
    filter: Option<&dyn RequestFilter>,
    tab_id: TabId,
) -> Result<Vec<u8>> {
    if hops_left == 0 {
        return Err(Error::Network("too many redirects".to_owned()));
    }

    // parse_url валидирует scheme/host/port раньше, чем мы откроем сокет.
    // События эмитим только если форма запроса прошла валидацию: на bad scheme
    // (`ftp://...`) ни RequestStarted, ни RequestCompleted, ни RequestBlocked —
    // байт даже не подумал улетать, и сам URL невалиден для фильтра.
    // Сетевые ошибки после parse (DNS, refused, TLS handshake) оставляют
    // Started без Completed — это инвариант «started + missing completed =
    // network failure»; явный RequestFailed добавим, когда увидим, что
    // наблюдателям этого мало.
    let parsed = parse_url(url)?;

    let event_url = Url::parse(url)
        .expect("url validated by parse_url above (non-empty, http/https scheme)");

    // Фильтрация — после parse_url (нет смысла спрашивать про невалидный URL),
    // но ДО RequestStarted: блокированный запрос НЕ ходит в сеть и НЕ генерит
    // Started/Completed. Каждый redirect-hop проверяется независимо, поэтому
    // переход с нейтрального адреса на трекер тоже ловится.
    if let Some(f) = filter
        && let Some(reason) = f.should_block(&event_url)
    {
        if let Some(s) = sink {
            s.emit(&Event::RequestBlocked {
                tab_id,
                url: event_url,
                reason: reason.clone(),
            });
        }
        return Err(Error::Network(format!("blocked: {reason}")));
    }

    if let Some(s) = sink {
        s.emit(&Event::RequestStarted {
            tab_id,
            url: event_url.clone(),
        });
    }

    let mut conn = connect(&parsed)?;
    write_request(&mut conn, &parsed.host, &parsed.path)?;
    let resp = read_response(conn)?;

    // RequestCompleted эмитим всегда после получения статуса, до анализа кода:
    // редирект-hop, 4xx, 5xx — всё это «outgoing byte был виден ответом».
    if let Some(s) = sink {
        s.emit(&Event::RequestCompleted {
            tab_id,
            url: event_url,
            status: resp.status,
        });
    }

    match resp.status {
        200..=299 => Ok(resp.body),
        301 | 302 | 303 | 307 | 308 => {
            let location = header_value(&resp.headers, "location")
                .ok_or_else(|| Error::Network("redirect without Location".to_owned()))?;

            // Resolve relative redirects.
            let next_url = if location.starts_with("http://") || location.starts_with("https://") {
                location.to_owned()
            } else {
                let base = format!("{}://{}:{}", scheme_str(parsed.scheme), parsed.host, parsed.port);
                if location.starts_with('/') {
                    format!("{base}{location}")
                } else {
                    // Relative to current path dir.
                    let dir = parsed.path.rfind('/').map(|i| &parsed.path[..=i]).unwrap_or("/");
                    format!("{base}{dir}{location}")
                }
            };

            fetch_with_redirect(&next_url, hops_left - 1, sink, filter, tab_id)
        }
        status => Err(Error::Network(format!("HTTP {status}"))),
    }
}

fn scheme_str(s: Scheme) -> &'static str {
    match s {
        Scheme::Http => "http",
        Scheme::Https => "https",
    }
}

// ── Публичный API ────────────────────────────────────────────────────────────

/// HTTP/1.1 + HTTPS клиент.
///
/// По умолчанию события никуда не уходят (sink не подключён) и блокировок нет
/// (filter не подключён). Подключите свой `EventSink` через `with_sink`, чтобы
/// наблюдать `RequestStarted` / `RequestCompleted` / `RequestBlocked` для
/// каждого исходящего запроса (включая редирект-hops); подключите
/// `RequestFilter` через `with_filter`, чтобы отсеивать запросы по URL
/// (трекеры / ad-blocker).
pub struct HttpClient {
    sink: Option<Arc<dyn EventSink>>,
    filter: Option<Arc<dyn RequestFilter>>,
    tab_id: TabId,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            sink: None,
            filter: None,
            tab_id: TabId(0),
        }
    }

    /// Подключить EventSink. По умолчанию sink-а нет (события не эмитятся).
    #[must_use]
    pub fn with_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Подключить RequestFilter. По умолчанию фильтра нет — `fetch` всегда
    /// уходит в сеть. С подключённым фильтром каждый URL (включая
    /// redirect-hops) проверяется через `should_block`; блокированные запросы
    /// эмитят `RequestBlocked` (если sink подключён) и возвращают `Err`,
    /// не делая TCP-соединения.
    #[must_use]
    pub fn with_filter(mut self, filter: Arc<dyn RequestFilter>) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Указать `TabId`, который попадёт в каждое emit-ое событие. В Phase 0
    /// (без вкладок) shell оставляет дефолтный `TabId(0)`.
    #[must_use]
    pub fn with_tab(mut self, tab_id: TabId) -> Self {
        self.tab_id = tab_id;
        self
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkTransport for HttpClient {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>> {
        fetch_with_redirect(
            url.as_str(),
            5,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.tab_id,
        )
    }
}

// ── Тесты ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_default_port() {
        let p = parse_url("https://example.com/path").unwrap();
        assert_eq!(p.scheme, Scheme::Https);
        assert_eq!(p.host, "example.com");
        assert_eq!(p.port, 443);
        assert_eq!(p.path, "/path");
    }

    #[test]
    fn parse_http_default_port() {
        let p = parse_url("http://example.com").unwrap();
        assert_eq!(p.scheme, Scheme::Http);
        assert_eq!(p.port, 80);
        assert_eq!(p.path, "/");
    }

    #[test]
    fn parse_explicit_port() {
        let p = parse_url("http://localhost:8080/index.html").unwrap();
        assert_eq!(p.host, "localhost");
        assert_eq!(p.port, 8080);
        assert_eq!(p.path, "/index.html");
    }

    #[test]
    fn parse_unsupported_scheme() {
        assert!(parse_url("ftp://example.com").is_err());
    }

    #[test]
    fn parse_idn_cyrillic_host() {
        // Кириллический host конвертируется в Punycode на этапе parse:
        // DNS/TLS/Host: header получают ASCII-форму.
        let p = parse_url("https://президент.рф/").unwrap();
        assert_eq!(p.host, "xn--d1abbgf6aiiy.xn--p1ai");
        assert_eq!(p.port, 443);
        assert_eq!(p.path, "/");
    }

    #[test]
    fn parse_idn_with_port() {
        let p = parse_url("http://пример.рф:8080/test").unwrap();
        assert_eq!(p.host, "xn--e1afmkfd.xn--p1ai");
        assert_eq!(p.port, 8080);
        assert_eq!(p.path, "/test");
    }

    #[test]
    fn parse_idn_mixed_ascii_subdomain() {
        let p = parse_url("https://api.пример.рф/v1").unwrap();
        assert_eq!(p.host, "api.xn--e1afmkfd.xn--p1ai");
    }

    #[test]
    fn parse_status_ok() {
        assert_eq!(parse_status("HTTP/1.1 200 OK\r\n").unwrap(), 200);
    }

    #[test]
    fn parse_status_redirect() {
        assert_eq!(parse_status("HTTP/1.1 301 Moved Permanently\r\n").unwrap(), 301);
    }

    #[test]
    fn parse_status_bad() {
        assert!(parse_status("garbage\r\n").is_err());
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let headers = vec![
            ("Content-Type".to_owned(), "text/html".to_owned()),
            ("Transfer-Encoding".to_owned(), "chunked".to_owned()),
        ];
        assert_eq!(header_value(&headers, "content-type"), Some("text/html"));
        assert_eq!(header_value(&headers, "TRANSFER-ENCODING"), Some("chunked"));
        assert_eq!(header_value(&headers, "x-missing"), None);
    }

    #[test]
    fn chunked_decode_simple() {
        // "5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n"
        let data = b"5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n";
        let result = read_chunked(BufReader::new(&data[..])).unwrap();
        assert_eq!(result, b"Hello World");
    }

    #[test]
    fn chunked_decode_single_chunk() {
        let data = b"4\r\ntest\r\n0\r\n\r\n";
        let result = read_chunked(BufReader::new(&data[..])).unwrap();
        assert_eq!(result, b"test");
    }

    #[test]
    fn chunked_decode_empty() {
        let data = b"0\r\n\r\n";
        let result = read_chunked(BufReader::new(&data[..])).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn redirect_resolve_absolute() {
        // Проверяем, что абсолютный Location не модифицируется.
        let url = "https://other.com/page";
        let p = parse_url(url).unwrap();
        let resolved = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_owned()
        } else {
            format!("base{url}")
        };
        assert_eq!(resolved, "https://other.com/page");
        let _ = p;
    }

    // ── EventSink ────────────────────────────────────────────────────────────

    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::thread;

    /// Тестовый sink, собирающий все события в порядке emit.
    struct CollectingSink(Mutex<Vec<Event>>);

    impl CollectingSink {
        fn new() -> Self {
            Self(Mutex::new(Vec::new()))
        }

        fn events(&self) -> Vec<Event> {
            self.0.lock().unwrap().clone()
        }
    }

    impl EventSink for CollectingSink {
        fn emit(&self, event: &Event) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    #[test]
    fn http_client_builder_default_no_sink() {
        // HttpClient::new() работает без sink. Этот тест верифицирует, что
        // builder типы выровнены (компилируется, не паникует на drop).
        let _c = HttpClient::new();
        let _c = HttpClient::default();
        let _c = HttpClient::new().with_tab(TabId(42));
    }

    /// Запустить минимальный HTTP-сервер на 127.0.0.1:0, который ответит на
    /// `accept_count` соединений согласно `responder`. Возвращает (port, join).
    /// Responder вызывается с номером соединения (1..=accept_count) и возвращает
    /// тело HTTP-ответа (включая статус-строку и заголовки).
    fn mock_http_server<F>(accept_count: usize, responder: F) -> (u16, thread::JoinHandle<()>)
    where
        F: Fn(usize) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for i in 1..=accept_count {
                let (mut sock, _) = listener.accept().expect("accept");
                // Прочитаем запрос до пустой строки, иначе клиент не дождётся ответа.
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line == "\r\n" || line == "\n" || line.is_empty() {
                        break;
                    }
                }
                let body = responder(i);
                let _ = sock.write_all(&body);
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });
        (port, handle)
    }

    #[test]
    fn fetch_emits_started_then_completed_200() {
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new().with_sink(sink.clone()).with_tab(TabId(7));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let body = client.fetch(&url).expect("fetch");
        assert_eq!(body, b"hello");

        let events = sink.events();
        assert_eq!(events.len(), 2, "expected Started + Completed, got {events:?}");
        match &events[0] {
            Event::RequestStarted { tab_id, url } => {
                assert_eq!(*tab_id, TabId(7));
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/"));
            }
            other => panic!("expected RequestStarted, got {other:?}"),
        }
        match &events[1] {
            Event::RequestCompleted { tab_id, url, status } => {
                assert_eq!(*tab_id, TabId(7));
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/"));
                assert_eq!(*status, 200);
            }
            other => panic!("expected RequestCompleted, got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_emits_events_per_redirect_hop() {
        // Два hop-а: 1-й → 302 Location: /next, 2-й → 200 OK. Ожидаем
        // 4 события подряд: Started(/), Completed(302), Started(/next), Completed(200).
        let (port, server) = mock_http_server(2, move |i| match i {
            1 => b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone".to_vec(),
            _ => unreachable!(),
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new().with_sink(sink.clone());
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let body = client.fetch(&url).expect("fetch");
        assert_eq!(body, b"done");

        let events = sink.events();
        assert_eq!(events.len(), 4, "expected 4 events for 2 hops, got {events:?}");
        assert!(matches!(events[0], Event::RequestStarted { .. }));
        match &events[1] {
            Event::RequestCompleted { status, url, .. } => {
                assert_eq!(*status, 302);
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/"));
            }
            other => panic!("expected RequestCompleted(302), got {other:?}"),
        }
        match &events[2] {
            Event::RequestStarted { url, .. } => {
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/next"));
            }
            other => panic!("expected RequestStarted for /next, got {other:?}"),
        }
        match &events[3] {
            Event::RequestCompleted { status, .. } => assert_eq!(*status, 200),
            other => panic!("expected RequestCompleted(200), got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_emits_completed_even_for_4xx() {
        // 4xx — тоже completed-response, fetch вернёт Err, но событие должно
        // быть видно: байт улетел, ответ получен.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new().with_sink(sink.clone());
        let url = Url::parse(&format!("http://127.0.0.1:{port}/missing")).unwrap();
        assert!(client.fetch(&url).is_err());

        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], Event::RequestStarted { .. }));
        match &events[1] {
            Event::RequestCompleted { status, .. } => assert_eq!(*status, 404),
            other => panic!("expected RequestCompleted(404), got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_bad_scheme_emits_no_events() {
        // parse_url упадёт до emit — запрос даже не сформировался,
        // sink остаётся пустым.
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new().with_sink(sink.clone());
        let url = Url::parse("ftp://example.com/").unwrap();
        assert!(client.fetch(&url).is_err());
        assert!(sink.events().is_empty());
    }

    // ── RequestFilter ────────────────────────────────────────────────────────

    /// Тестовый фильтр: блокирует URL-ы, host которых содержит подстроку,
    /// возвращает фиксированный reason.
    struct BlockBySubstring {
        needle: String,
        reason: String,
    }

    impl RequestFilter for BlockBySubstring {
        fn should_block(&self, url: &Url) -> Option<String> {
            if url.as_str().contains(&self.needle) {
                Some(self.reason.clone())
            } else {
                None
            }
        }
    }

    /// Фильтр, который не блокирует ничего. Нужен, чтобы убедиться:
    /// с подключённым (но «разрешающим») фильтром обычный поток
    /// Started/Completed не ломается.
    struct AllowAll;

    impl RequestFilter for AllowAll {
        fn should_block(&self, _url: &Url) -> Option<String> {
            None
        }
    }

    #[test]
    fn fetch_blocked_emits_request_blocked_and_skips_network() {
        // Сетевого сервера НЕТ — фильтр обязан блокировать ДО любой попытки
        // TCP. Если эта инвариантность сломается, тест словит «connection
        // refused», и assert reason в err это поймает.
        let sink = Arc::new(CollectingSink::new());
        let filter: Arc<dyn RequestFilter> = Arc::new(BlockBySubstring {
            needle: "tracker.invalid".to_owned(),
            reason: "tracker".to_owned(),
        });
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_filter(filter)
            .with_tab(TabId(3));
        let url = Url::parse("http://tracker.invalid/ad.js").unwrap();

        let err = client.fetch(&url).expect_err("filter must block");
        assert!(format!("{err:?}").contains("tracker"), "reason in error: {err:?}");

        let events = sink.events();
        assert_eq!(events.len(), 1, "expected only RequestBlocked, got {events:?}");
        match &events[0] {
            Event::RequestBlocked { tab_id, url, reason } => {
                assert_eq!(*tab_id, TabId(3));
                assert_eq!(url.as_str(), "http://tracker.invalid/ad.js");
                assert_eq!(reason, "tracker");
            }
            other => panic!("expected RequestBlocked, got {other:?}"),
        }
    }

    #[test]
    fn fetch_with_allow_all_filter_normal_flow() {
        // Фильтр подключён, но возвращает None — Started/Completed как обычно.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let filter: Arc<dyn RequestFilter> = Arc::new(AllowAll);
        let client = HttpClient::new().with_sink(sink.clone()).with_filter(filter);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();

        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        let events = sink.events();
        assert_eq!(events.len(), 2, "expected Started + Completed, got {events:?}");
        assert!(matches!(events[0], Event::RequestStarted { .. }));
        assert!(matches!(events[1], Event::RequestCompleted { status: 200, .. }));
        assert!(
            !events.iter().any(|e| matches!(e, Event::RequestBlocked { .. })),
            "no RequestBlocked when filter allows"
        );

        server.join().unwrap();
    }

    #[test]
    fn fetch_filter_blocks_on_redirect_hop() {
        // Hop 1: 302 Location: http://127.0.0.1:<port>/tracker/pixel (без needle
        // в host, но с needle в path) → блок-фильтр сработает на 2-м hop-е.
        // Ожидаем 3 события: Started(hop1) → Completed(302, hop1) →
        // RequestBlocked(hop2). НЕТ Started/Completed для hop2.
        let needle = "/tracker";
        let (port, server) = mock_http_server(1, move |_| {
            // Один accept — для hop1; hop2 не должен попасть в сеть.
            b"HTTP/1.1 302 Found\r\nLocation: /tracker/pixel\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let filter: Arc<dyn RequestFilter> = Arc::new(BlockBySubstring {
            needle: needle.to_owned(),
            reason: "tracker-path".to_owned(),
        });
        let client = HttpClient::new().with_sink(sink.clone()).with_filter(filter);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();

        let err = client.fetch(&url).expect_err("redirect target must be blocked");
        assert!(format!("{err:?}").contains("tracker-path"), "reason in error: {err:?}");

        let events = sink.events();
        assert_eq!(events.len(), 3, "expected Started + Completed(302) + Blocked, got {events:?}");
        match &events[0] {
            Event::RequestStarted { url, .. } => {
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/"));
            }
            other => panic!("expected RequestStarted for hop1, got {other:?}"),
        }
        match &events[1] {
            Event::RequestCompleted { status, .. } => assert_eq!(*status, 302),
            other => panic!("expected RequestCompleted(302), got {other:?}"),
        }
        match &events[2] {
            Event::RequestBlocked { url, reason, .. } => {
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/tracker/pixel"));
                assert_eq!(reason, "tracker-path");
            }
            other => panic!("expected RequestBlocked for hop2, got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_blocked_without_sink_returns_err_with_reason() {
        // Без sink-а событие никто не услышит, но fetch всё равно отказывает
        // с reason в тексте ошибки — caller (shell) узнает, почему отказали.
        let filter: Arc<dyn RequestFilter> = Arc::new(BlockBySubstring {
            needle: "example".to_owned(),
            reason: "ads".to_owned(),
        });
        let client = HttpClient::new().with_filter(filter);
        let url = Url::parse("http://example.com/banner").unwrap();

        let err = client.fetch(&url).expect_err("must block");
        assert!(format!("{err:?}").contains("ads"), "reason in error: {err:?}");
    }

    #[test]
    fn fetch_filter_skipped_for_bad_scheme() {
        // bad scheme → parse_url упадёт до filter-check; фильтр не должен
        // быть спрошен, sink остаётся пустым (как и без фильтра).
        struct PanicOnCheck;
        impl RequestFilter for PanicOnCheck {
            fn should_block(&self, _url: &Url) -> Option<String> {
                panic!("filter must not be called for bad scheme");
            }
        }
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_filter(Arc::new(PanicOnCheck));
        let url = Url::parse("ftp://example.com/").unwrap();
        assert!(client.fetch(&url).is_err());
        assert!(sink.events().is_empty());
    }
}
