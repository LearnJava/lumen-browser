//! HTTP/1.1 клиент с TLS через rustls (Exception #3).
//!
//! Реализует `lumen_core::ext::NetworkTransport`.
//! Поддерживает: HTTP и HTTPS, редиректы (до 5), chunked transfer encoding,
//! **HTTP/1.1 keep-alive + connection pool** (переиспользование TCP/TLS
//! между запросами к одному origin-у), retry-on-stale при попытке писать
//! в закрытое сервером idle-соединение.
//! Не поддерживает: HTTP/2, кэширование, аутентификацию.
//!
//! URL парсится в `lumen_core::url::Url` — никакого собственного парсера здесь
//! не держим. Из `Url` берём scheme, host (Punycode для DNS/TLS/Host header
//! через `host_ascii`), effective_port и `path_and_query` для request line.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::ClientConnection;
use rustls::pki_types::ServerName;

use lumen_core::error::{Error, Result};
use lumen_core::event::{Event, TabId};
use lumen_core::ext::{EventSink, NetworkTransport, RequestFilter};
use lumen_core::url::Url;

mod pool;
pub use pool::ConnectionPool;

use pool::PoolKey;

/// Проверяет, что схема URL поддерживается транспортом (http/https) и
/// извлекает всё, что нужно для connect: ASCII-форму host (Punycode для
/// IDN — RFC 7230 §5.4, RFC 6066 §3), effective port (80/443 по схеме) и
/// флаг TLS. Bad scheme (`ftp://`, `data:`, `file://`) — ранний выход без
/// каких-либо побочных эффектов.
fn require_http_scheme(url: &Url) -> Result<(String, u16, bool)> {
    let is_tls = match url.scheme() {
        "http" => false,
        "https" => true,
        other => return Err(Error::Network(format!("unsupported scheme: {other}"))),
    };
    let host = url
        .host_ascii()
        .map_err(|e| Error::Network(e.to_string()))?;
    if host.is_empty() {
        return Err(Error::Network(format!(
            "empty host in URL: {}",
            url.as_str()
        )));
    }
    let port = url
        .effective_port()
        .ok_or_else(|| Error::Network(format!("no port for URL: {}", url.as_str())))?;
    Ok((host, port, is_tls))
}

// ── TCP + TLS stream ─────────────────────────────────────────────────────────

/// Низкоуровневый stream — сырое TCP или TLS-обёртка над ним. Не содержит
/// буферизации; буфера живут на уровень выше в `Connection`.
pub(crate) enum RawStream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for RawStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            RawStream::Plain(s) => s.read(buf),
            RawStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for RawStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            RawStream::Plain(s) => s.write(buf),
            RawStream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            RawStream::Plain(s) => s.flush(),
            RawStream::Tls(s) => s.flush(),
        }
    }
}

/// Persistent HTTP-соединение, пригодное к переиспользованию между запросами.
///
/// Содержит `BufReader<RawStream>` (постоянный, не пересоздаётся на каждый
/// запрос — иначе входной буфер с остатками предыдущего ответа уйдёт в drop)
/// и флаг `closed`, который выставляется, если сервер прислал
/// `Connection: close` или мы получили EOF до завершения ответа. `closed`
/// соединение нельзя возвращать в пул.
pub(crate) struct Connection {
    reader: BufReader<RawStream>,
    closed: bool,
}

impl Connection {
    fn new(stream: RawStream) -> Self {
        Self {
            reader: BufReader::new(stream),
            closed: false,
        }
    }

    /// Записать HTTP-запрос в stream. Используется `Connection: keep-alive`
    /// (HTTP/1.1 default, но явно для ясности и для совместимости с серверами,
    /// которые криво интерпретируют отсутствие хедера).
    fn write_request(&mut self, host: &str, path: &str) -> Result<()> {
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Lumen/0.0.1\r\nAccept: */*\r\nConnection: keep-alive\r\n\r\n"
        );
        let stream = self.reader.get_mut();
        stream
            .write_all(req.as_bytes())
            .map_err(|e| Error::Network(format!("write request: {e}")))?;
        stream
            .flush()
            .map_err(|e| Error::Network(format!("flush request: {e}")))?;
        Ok(())
    }
}

// ── HTTP/1.1 ответ ───────────────────────────────────────────────────────────

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

/// Прочитать один HTTP-ответ из persistent connection. Не consume-ит
/// соединение — после возврата `Connection` пригоден к следующему
/// `write_request` (если `closed` остался false).
///
/// Корректно дочитывает: status-line, headers до `\r\n\r\n`, body по
/// `Content-Length` или `Transfer-Encoding: chunked` (включая trailer-секцию,
/// которая раньше пропускалась — без этого второй запрос на том же сокете
/// читал бы хвост от предыдущего chunked-ответа).
///
/// Если сервер прислал `Connection: close` или произошёл EOF до окончания
/// тела — выставляет `conn.closed = true`, и caller не должен возвращать
/// такое соединение в пул.
fn read_response(conn: &mut Connection) -> Result<Response> {
    // Status line.
    let mut status_line = String::new();
    let n = conn
        .reader
        .read_line(&mut status_line)
        .map_err(|e| Error::Network(format!("read status: {e}")))?;
    if n == 0 {
        conn.closed = true;
        return Err(Error::Network("EOF before status line".to_owned()));
    }
    let status = parse_status(&status_line)?;

    // Headers до пустой строки.
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut line = String::new();
        let n = conn
            .reader
            .read_line(&mut line)
            .map_err(|e| Error::Network(format!("read header: {e}")))?;
        if n == 0 {
            conn.closed = true;
            return Err(Error::Network("EOF in headers".to_owned()));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }

    // Решение о keep-alive: HTTP/1.1 default = keep-alive, отменяется явным
    // `Connection: close` (case-insensitive, может содержаться в списке через
    // запятую с другими токенами вроде `keep-alive`/`upgrade`).
    let server_wants_close = header_value(&headers, "connection")
        .map(|v| {
            v.split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("close"))
        })
        .unwrap_or(false);

    // Body: chunked > Content-Length > read-to-EOF (последнее — только если
    // сервер обещал закрыть соединение; для keep-alive без Content-Length
    // длина неизвестна, что нелегально по RFC 7230 §3.3.3).
    let is_chunked = header_value(&headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);
    let content_length =
        header_value(&headers, "content-length").and_then(|v| v.trim().parse::<usize>().ok());

    let body = if is_chunked {
        match read_chunked(&mut conn.reader) {
            Ok(b) => b,
            Err(e) => {
                conn.closed = true;
                return Err(e);
            }
        }
    } else if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        if let Err(e) = conn.reader.read_exact(&mut buf) {
            conn.closed = true;
            return Err(Error::Network(format!("read body: {e}")));
        }
        buf
    } else if server_wants_close || status == 204 || status == 304 {
        // 204 No Content / 304 Not Modified не имеют тела (RFC 7230 §3.3.3).
        // Иначе при Connection: close без Content-Length — читаем до EOF.
        if status == 204 || status == 304 {
            Vec::new()
        } else {
            let mut buf = Vec::new();
            if let Err(e) = conn.reader.read_to_end(&mut buf) {
                conn.closed = true;
                return Err(Error::Network(format!("read body: {e}")));
            }
            conn.closed = true;
            buf
        }
    } else {
        // RFC 7230: HTTP/1.1 без Content-Length и без chunked при keep-alive —
        // протокольная ошибка. Закрываем соединение, чтобы не отравить пул.
        conn.closed = true;
        return Err(Error::Network(
            "response without Content-Length or chunked".to_owned(),
        ));
    };

    if server_wants_close {
        conn.closed = true;
    }

    Ok(Response {
        status,
        headers,
        body,
    })
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

/// Прочитать chunked-тело **полностью**, включая trailer-секцию и финальный
/// CRLF. Без дочитывания trailer-а в BufReader остаётся хвост от прошлого
/// ответа, который сломает следующий request на том же соединении — это и
/// есть отличие от прежней реализации, которая работала только с
/// `Connection: close`.
fn read_chunked<R: BufRead>(reader: &mut R) -> Result<Vec<u8>> {
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
            // Last chunk: дочитать trailer-section (произвольно много
            // trailer-header строк) до пустой строки.
            loop {
                let mut line = String::new();
                let n = reader
                    .read_line(&mut line)
                    .map_err(|e| Error::Network(format!("chunked trailer: {e}")))?;
                if n == 0 {
                    // EOF — для chunked это допустимо после last-chunk
                    // (трейлер опционален), но caller должен mark соединение
                    // closed чтобы не положить мёртвый stream в пул.
                    break;
                }
                if line == "\r\n" || line == "\n" {
                    break;
                }
                // Не-пустые строки — это trailer-headers; в Phase 0 их игнорим.
            }
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

// ── Connect ─────────────────────────────────────────────────────────────────

fn connect(host: &str, port: u16, is_tls: bool) -> Result<Connection> {
    let addr = format!("{host}:{port}");
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| Error::Network(format!("connect {addr}: {e}")))?;

    if !is_tls {
        return Ok(Connection::new(RawStream::Plain(tcp)));
    }

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|e| Error::Network(format!("invalid hostname '{host}': {e}")))?;

    let conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| Error::Network(format!("TLS handshake: {e}")))?;

    Ok(Connection::new(RawStream::Tls(Box::new(
        rustls::StreamOwned::new(conn, tcp),
    ))))
}

// ── Pool integration ─────────────────────────────────────────────────────────

/// Решить, выглядит ли ошибка как «stale keep-alive»: сервер закрыл idle
/// соединение, и наш write / первый read получил EOF или RST. Такие ошибки
/// заслуживают однократного retry на свежем соединении.
fn is_stale_error(err: &Error) -> bool {
    let msg = format!("{err:?}");
    msg.contains("BrokenPipe")
        || msg.contains("ConnectionReset")
        || msg.contains("ConnectionAborted")
        || msg.contains("UnexpectedEof")
        || msg.contains("EOF before status line")
        || msg.contains("EOF in headers")
}

/// Один полный HTTP-запрос: acquire из пула (или connect), write_request,
/// read_response, release. При попадании на stale pooled connection —
/// однократный retry с свежим. Возвращает `Response` и в случае success
/// (соединение не закрыто) возвращает его в пул.
fn fetch_single(
    pool: &ConnectionPool,
    host: &str,
    port: u16,
    is_tls: bool,
    request_host_header: &str,
    request_path: &str,
) -> Result<Response> {
    let key = PoolKey {
        host: host.to_owned(),
        port,
        is_tls,
    };

    // Попытка 1: используем pooled connection, если он есть.
    if let Some(pooled) = pool.acquire(&key) {
        match do_request(pooled, request_host_header, request_path) {
            Ok((resp, conn)) => {
                if !conn.closed {
                    pool.release(key, conn);
                }
                return Ok(resp);
            }
            Err(e) if is_stale_error(&e) => {
                // Сервер успел закрыть idle-соединение — pooled умер. Дальше
                // упадём на ветку «новый connect»; pooled уже не возвращается.
            }
            Err(e) => return Err(e),
        }
    }

    // Попытка 2 (или 1, если пул был пуст): свежий connect.
    let conn = connect(host, port, is_tls)?;
    let (resp, conn) = do_request(conn, request_host_header, request_path)?;
    if !conn.closed {
        pool.release(key, conn);
    }
    Ok(resp)
}

fn do_request(
    mut conn: Connection,
    host: &str,
    path: &str,
) -> Result<(Response, Connection)> {
    conn.write_request(host, path)?;
    let resp = read_response(&mut conn)?;
    Ok((resp, conn))
}

// ── Редиректы ────────────────────────────────────────────────────────────────

fn fetch_with_redirect(
    url: &Url,
    hops_left: u8,
    pool: &ConnectionPool,
    sink: Option<&dyn EventSink>,
    filter: Option<&dyn RequestFilter>,
    tab_id: TabId,
) -> Result<Vec<u8>> {
    if hops_left == 0 {
        return Err(Error::Network("too many redirects".to_owned()));
    }

    // require_http_scheme валидирует scheme/host/port раньше, чем мы откроем
    // сокет. События эмитим только если форма запроса прошла валидацию: на
    // bad scheme (`ftp://...`) ни RequestStarted, ни RequestCompleted, ни
    // RequestBlocked — байт даже не подумал улетать, и сам URL невалиден для
    // фильтра. Сетевые ошибки после валидации (DNS, refused, TLS handshake)
    // оставляют Started без Completed — это инвариант «started + missing
    // completed = network failure»; явный RequestFailed добавим, когда
    // увидим, что наблюдателям этого мало.
    let (host_ascii, port, is_tls) = require_http_scheme(url)?;

    // Фильтрация — после валидации scheme/host (нет смысла спрашивать про
    // невалидный URL), но ДО RequestStarted: блокированный запрос НЕ ходит
    // в сеть и НЕ генерит Started/Completed. Каждый redirect-hop проверяется
    // независимо, поэтому переход с нейтрального адреса на трекер тоже
    // ловится.
    if let Some(f) = filter
        && let Some(reason) = f.should_block(url)
    {
        if let Some(s) = sink {
            s.emit(&Event::RequestBlocked {
                tab_id,
                url: url.clone(),
                reason: reason.clone(),
            });
        }
        return Err(Error::Network(format!("blocked: {reason}")));
    }

    if let Some(s) = sink {
        s.emit(&Event::RequestStarted {
            tab_id,
            url: url.clone(),
        });
    }

    let resp = fetch_single(
        pool,
        &host_ascii,
        port,
        is_tls,
        &host_ascii,
        &url.path_and_query(),
    )?;

    // RequestCompleted эмитим всегда после получения статуса, до анализа кода:
    // редирект-hop, 4xx, 5xx — всё это «outgoing byte был виден ответом».
    if let Some(s) = sink {
        s.emit(&Event::RequestCompleted {
            tab_id,
            url: url.clone(),
            status: resp.status,
        });
    }

    match resp.status {
        200..=299 => Ok(resp.body),
        301 | 302 | 303 | 307 | 308 => {
            let location = header_value(&resp.headers, "location")
                .ok_or_else(|| Error::Network("redirect without Location".to_owned()))?;
            let next = url
                .resolve(location)
                .map_err(|e| Error::Network(format!("resolve redirect '{location}': {e}")))?;
            fetch_with_redirect(&next, hops_left - 1, pool, sink, filter, tab_id)
        }
        status => Err(Error::Network(format!("HTTP {status}"))),
    }
}

// ── Публичный API ────────────────────────────────────────────────────────────

/// HTTP/1.1 + HTTPS клиент.
///
/// По умолчанию события никуда не уходят (sink не подключён), блокировок нет
/// (filter не подключён) и используется собственный fresh `ConnectionPool`.
/// Подключите свой `EventSink` через `with_sink`, чтобы наблюдать
/// `RequestStarted` / `RequestCompleted` / `RequestBlocked` для каждого
/// исходящего запроса (включая редирект-hops); подключите `RequestFilter`
/// через `with_filter`, чтобы отсеивать запросы по URL (трекеры / ad-blocker);
/// подключите общий `ConnectionPool` через `with_pool`, если хотите делить
/// keep-alive соединения между несколькими `HttpClient`-ами.
pub struct HttpClient {
    sink: Option<Arc<dyn EventSink>>,
    filter: Option<Arc<dyn RequestFilter>>,
    pool: Arc<ConnectionPool>,
    tab_id: TabId,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            sink: None,
            filter: None,
            pool: Arc::new(ConnectionPool::new()),
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

    /// Подключить shared `ConnectionPool`. По умолчанию у каждого `HttpClient`
    /// свой собственный fresh-пул. Общий пул полезен, если несколько клиентов
    /// делят одни и те же origin-ы (несколько вкладок одного браузера).
    #[must_use]
    pub fn with_pool(mut self, pool: Arc<ConnectionPool>) -> Self {
        self.pool = pool;
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
            url,
            5,
            &self.pool,
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
    fn require_http_scheme_http_default_port() {
        let url = Url::parse("http://example.com/").unwrap();
        let (host, port, is_tls) = require_http_scheme(&url).unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert!(!is_tls);
    }

    #[test]
    fn require_http_scheme_https_default_port() {
        let url = Url::parse("https://example.com/").unwrap();
        let (host, port, is_tls) = require_http_scheme(&url).unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
        assert!(is_tls);
    }

    #[test]
    fn require_http_scheme_explicit_port() {
        let url = Url::parse("https://example.com:8443/").unwrap();
        let (_, port, _) = require_http_scheme(&url).unwrap();
        assert_eq!(port, 8443);
    }

    #[test]
    fn require_http_scheme_rejects_ftp() {
        let url = Url::parse("ftp://example.com/").unwrap();
        let err = require_http_scheme(&url).unwrap_err();
        assert!(format!("{err:?}").contains("unsupported scheme"));
    }

    #[test]
    fn require_http_scheme_idn_host_returns_punycode() {
        // DNS / TLS SNI / Host header требуют ASCII (RFC 7230 §5.4, RFC 6066 §3).
        let url = Url::parse("https://президент.рф/").unwrap();
        let (host, _, _) = require_http_scheme(&url).unwrap();
        assert_eq!(host, "xn--d1abbgf6aiiy.xn--p1ai");
    }

    #[test]
    fn require_http_scheme_idn_with_port() {
        let url = Url::parse("http://пример.рф:8080/test").unwrap();
        let (host, port, _) = require_http_scheme(&url).unwrap();
        assert_eq!(host, "xn--e1afmkfd.xn--p1ai");
        assert_eq!(port, 8080);
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
        // "5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n" — last-chunk + пустой trailer.
        let data = b"5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&data[..]);
        let result = read_chunked(&mut reader).unwrap();
        assert_eq!(result, b"Hello World");
    }

    #[test]
    fn chunked_decode_single_chunk() {
        let data = b"4\r\ntest\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&data[..]);
        let result = read_chunked(&mut reader).unwrap();
        assert_eq!(result, b"test");
    }

    #[test]
    fn chunked_decode_empty() {
        let data = b"0\r\n\r\n";
        let mut reader = BufReader::new(&data[..]);
        let result = read_chunked(&mut reader).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn chunked_consumes_trailer_section() {
        // После last-chunk сервер может прислать trailer-headers перед финальным
        // CRLF. Они должны быть прочитаны и выброшены — иначе следующий
        // запрос на keep-alive соединении прочитает их как новый status-line.
        let data = b"3\r\nabc\r\n0\r\nX-Trailer: foo\r\n\r\nNEXT-RESPONSE-START";
        let mut reader = BufReader::new(&data[..]);
        let result = read_chunked(&mut reader).unwrap();
        assert_eq!(result, b"abc");
        // После read_chunked в reader-е должно остаться только "NEXT-RESPONSE-START".
        let mut leftover = String::new();
        reader.read_to_string(&mut leftover).unwrap();
        assert_eq!(leftover, "NEXT-RESPONSE-START");
    }

    #[test]
    fn redirect_resolve_relative_uses_url_resolve() {
        // Полный E2E проверяется через mock-сервер ниже
        // (fetch_emits_events_per_redirect_hop); здесь — точечно, что
        // используемый редиректами `Url::resolve` дружит с реальным base+ref.
        let base = Url::parse("http://localhost:8080/dir/page").unwrap();
        let abs = base.resolve("/next").unwrap();
        assert_eq!(abs.as_str(), "http://localhost:8080/next");
        let rel = base.resolve("sibling.html").unwrap();
        assert_eq!(rel.as_str(), "http://localhost:8080/dir/sibling.html");
    }

    #[test]
    fn is_stale_error_recognises_eof_and_resets() {
        assert!(is_stale_error(&Error::Network("EOF before status line".to_owned())));
        assert!(is_stale_error(&Error::Network("EOF in headers".to_owned())));
        assert!(is_stale_error(&Error::Network(
            "write request: BrokenPipe (os error 32)".to_owned()
        )));
        assert!(is_stale_error(&Error::Network(
            "read body: ConnectionReset".to_owned()
        )));
        assert!(!is_stale_error(&Error::Network("HTTP 500".to_owned())));
        assert!(!is_stale_error(&Error::Network("blocked: tracker".to_owned())));
    }

    // ── EventSink ────────────────────────────────────────────────────────────

    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// Однократный mock-сервер: каждое соединение обслуживается **отдельно**,
    /// после одного ответа socket закрывается. Удобен для прежних тестов и
    /// для проверки случая `Connection: close`.
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

    /// Keep-alive mock-сервер: один accept обслуживает несколько запросов
    /// подряд на одном сокете, отвечая `responder(i)` на i-й запрос. После
    /// `requests_to_serve` запросов сокет закрывается. `accept_counter`
    /// инкрементится на каждом accept-е, чтобы тест мог убедиться, что
    /// клиент действительно переиспользовал соединение (accept_count == 1).
    fn mock_keepalive_server<F>(
        requests_to_serve: usize,
        accept_counter: Arc<AtomicUsize>,
        responder: F,
    ) -> (u16, thread::JoinHandle<()>)
    where
        F: Fn(usize) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            accept_counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            for i in 1..=requests_to_serve {
                // Читаем один request до пустой строки.
                let mut got_any = false;
                loop {
                    let mut line = String::new();
                    let n = reader.read_line(&mut line).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    got_any = true;
                    if line == "\r\n" || line == "\n" {
                        break;
                    }
                }
                if !got_any {
                    break;
                }
                let body = responder(i);
                if sock.write_all(&body).is_err() {
                    break;
                }
            }
            let _ = sock.shutdown(std::net::Shutdown::Both);
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

    // ── Keep-alive / Connection Pool ─────────────────────────────────────────

    #[test]
    fn two_fetches_reuse_one_tcp_connection() {
        // Сервер обслуживает два запроса на одном accept-е (HTTP/1.1
        // keep-alive). Если HttpClient правильно переиспользует соединение,
        // accept_count останется == 1.
        let accept_counter = Arc::new(AtomicUsize::new(0));
        let (port, server) = mock_keepalive_server(2, accept_counter.clone(), |i| match i {
            1 => b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nFIR".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nSEC".to_vec(),
            _ => unreachable!(),
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"FIR");
        assert_eq!(client.fetch(&url).unwrap(), b"SEC");

        server.join().unwrap();
        assert_eq!(
            accept_counter.load(Ordering::SeqCst),
            1,
            "expected exactly 1 TCP accept (keep-alive reuse)"
        );
    }

    #[test]
    fn server_says_connection_close_drops_pool_entry() {
        // Сервер прислал `Connection: close` → соединение в пул не вернулось.
        // Второй запрос требует свежий accept.
        let accept_counter = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let counter = accept_counter.clone();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut sock, _) = listener.accept().expect("accept");
                counter.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    let n = reader.read_line(&mut line).unwrap_or(0);
                    if n == 0 || line == "\r\n" || line == "\n" {
                        break;
                    }
                }
                let _ = sock.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"ok");
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        server.join().unwrap();
        assert_eq!(
            accept_counter.load(Ordering::SeqCst),
            2,
            "Connection: close must force a fresh TCP connect on next request"
        );
    }

    #[test]
    fn stale_pooled_connection_triggers_retry() {
        // Сервер сначала отдаёт ответ + закрывает сокет (без Connection: close
        // — клиент думает «keep-alive»), потом на следующий accept отдаёт
        // нормальный ответ. Клиент должен заметить stale-write/read и сделать
        // retry на свежем connect-е. Ожидаем 2 accept-а, fetch проходит дважды.
        let accept_counter = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let counter = accept_counter.clone();
        let server = thread::spawn(move || {
            // Соединение 1: ответ + сразу shutdown (не дожидаясь второго
            // запроса) — это как «idle timeout у сервера».
            let (mut sock1, _) = listener.accept().expect("accept1");
            counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(sock1.try_clone().unwrap());
            loop {
                let mut line = String::new();
                let n = reader.read_line(&mut line).unwrap_or(0);
                if n == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
            }
            let _ = sock1.write_all(
                // Без Connection: close — сервер врёт про keep-alive,
                // но всё равно закрывает.
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst",
            );
            let _ = sock1.shutdown(std::net::Shutdown::Both);
            drop(sock1);

            // Соединение 2: после того, как клиент попытается переиспользовать
            // первое и упадёт со stale-error, он откроет новое.
            let (mut sock2, _) = listener.accept().expect("accept2");
            counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(sock2.try_clone().unwrap());
            loop {
                let mut line = String::new();
                let n = reader.read_line(&mut line).unwrap_or(0);
                if n == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
            }
            let _ = sock2.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecond",
            );
            let _ = sock2.shutdown(std::net::Shutdown::Both);
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"first");
        // Второй fetch должен сработать через retry-on-stale.
        assert_eq!(client.fetch(&url).unwrap(), b"second");

        server.join().unwrap();
        assert_eq!(accept_counter.load(Ordering::SeqCst), 2);
    }
}

