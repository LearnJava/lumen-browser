//! HTTP/1.1 клиент с TLS через rustls (Exception #3).
//!
//! Реализует `lumen_core::ext::NetworkTransport`.
//! Поддерживает: HTTP и HTTPS, редиректы (до 5), chunked transfer encoding,
//! **HTTP/1.1 keep-alive + connection pool** (переиспользование TCP/TLS
//! между запросами к одному origin-у), retry-on-stale при попытке писать
//! в закрытое сервером idle-соединение.
//! TLS handshake негоциирует ALPN `[h2, http/1.1]`; HTTP/2 пока возвращает
//! placeholder-ошибку (5A.1 — клиентский H2-стек в 5A.2–5A.6).
//! Не поддерживает: HTTP/2 wire-protocol, кэширование, аутентификацию.
//!
//! URL парсится в `lumen_core::url::Url` — никакого собственного парсера здесь
//! не держим. Из `Url` берём scheme, host (Punycode для DNS/TLS/Host header
//! через `host_ascii`), effective_port и `path_and_query` для request line.
//!
//! **Для тестирования:** используй [`MockTransport`] — реализация [`NetworkTransport`]
//! (через `lumen_core::ext`), которая возвращает заранее зарегистрированные
//! fixture-данные вместо реальных HTTP-запросов.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::ClientConnection;
use rustls::pki_types::ServerName;

use lumen_core::error::{Error, Result};
use lumen_core::event::{Event, RequestStage, TabId};
use lumen_core::ext::{
    ContentDecoder, CookieProvider, DnsResolver, EventSink, FetchInterceptor, HstsEnforcement,
    HttpAuthScheme, HttpCredentialProvider, JsFetchProvider, JsFetchResult, JsSseEvent, JsSseProvider,
    JsSseSession, JsWebSocketProvider, JsWebSocketSession, JsWsEvent, NetworkTransport, NoopEventSink,
    RequestFilter, SseProvider, SseSession, WebSocketProvider, WebSocketSession,
};
use lumen_core::url::Url;

mod auth;
mod brotli;
mod cors;
mod dns;
mod doh;
mod dot;
pub mod filter;
pub mod h2;
pub mod http;
pub mod http_cache;
mod hsts;
mod hsts_preload;
mod mixed_content;
mod mock;
mod origin;
mod pool;
mod range;
mod sandbox;
pub mod sse;
pub mod tls;
pub mod webauthn;
pub(crate) mod websocket;
pub use auth::StaticCredentialProvider;
pub use webauthn::VirtualAuthenticator;
pub use brotli::BrotliContentDecoder;
pub use filter::{EasyListFilter, HostsFilter, CompositeFilter};
pub use http_cache::HttpCache;
pub use http::{HttpProfile, H2Settings, H2StreamPriority, ClientHintsProfile, HeaderOrder};
pub use mock::MockTransport;
pub use tls::{
    TlsProfile, TlsHandshakeInfo, CHROME_130_JA3_SNAPSHOT, CHROME_130_JA4_SNAPSHOT,
    ChromeJa3Snapshot, JA4ChromeSnapshot, http_to_tls_profile,
};
pub use cors::{
    CorsError, CorsRequest, CredentialsMode, DEFAULT_PREFLIGHT_MAX_AGE_SECONDS,
    MAX_SAFELISTED_HEADER_VALUE_LEN, PreflightCache, PreflightResult, build_preflight_headers,
    check_cors_response_headers, evaluate_preflight_response, is_cors_safelisted_content_type,
    is_cors_safelisted_method, is_cors_safelisted_request_header, is_forbidden_request_header,
    needs_preflight, unsafe_request_header_names,
};
pub use dns::SystemDnsResolver;
pub use doh::{CachedDnsResolver, DohResolver};
pub use dot::{DotResolver, DOT_DEFAULT_PORT};
pub use hsts_preload::{HstsPreloadList, get_preload_list};
pub use mixed_content::{
    MixedContentLevel, MixedContentMode, MixedContentPolicy, RequestDestination,
    block_reason as mixed_content_block_reason, classify_subresource_request,
};
pub use origin::{Origin, OriginError};
pub use h2::pool::H2Pool;
pub use pool::ConnectionPool;
pub use range::{
    ContentRange, MultiRangeResponse, RangePart, RangeRequest, RangeResponse, RangeSpec,
    RangeValidator, parse_boundary_from_content_type, parse_content_range,
    parse_multipart_byteranges,
};
pub use sandbox::{SandboxFlags, parse_sandbox_value};

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

/// Построить ASCII-origin из URL: `scheme://host[:port]`.
/// Стандартные порты опускаются (80 для http, 443 для https).
fn build_origin(url: &Url) -> String {
    let scheme = url.scheme();
    let host = url.host_ascii().unwrap_or_default();
    let default_port: u16 = if scheme == "https" { 443 } else { 80 };
    match url.effective_port() {
        Some(p) if p != default_port => format!("{scheme}://{host}:{p}"),
        _ => format!("{scheme}://{host}"),
    }
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
    /// True when ALPN negotiated HTTP/2. The connection cannot be used for
    /// HTTP/1.1; `fetch_single` hands the raw stream to the H2 driver.
    is_h2: bool,
}

impl Connection {
    fn new(stream: RawStream) -> Self {
        Self {
            reader: BufReader::new(stream),
            closed: false,
            is_h2: false,
        }
    }

    /// Unwrap the inner stream. Only valid before any reads have been performed
    /// (fresh connection, BufReader buffer is empty).
    fn into_stream(self) -> RawStream {
        self.reader.into_inner()
    }

    /// Записать HTTP-запрос в stream. Используется `Connection: keep-alive`
    /// (HTTP/1.1 default, но явно для ясности и для совместимости с серверами,
    /// которые криво интерпретируют отсутствие хедера). Опциональный `range`
    /// добавляет header `Range: bytes=START-END` / `bytes=START-` / `bytes=-N`
    /// (RFC 7233 §3.1); невалидный RangeSpec (`end < start`, `suffix=0`)
    /// тихо опускает header — fetch получит full response (200 OK), не упадёт.
    /// Опциональный `if_range` — `If-Range` validator (RFC 7233 §3.2),
    /// добавляется только вместе с Range. Опциональный `authorization` —
    /// готовая строка для header `Authorization` (Basic / Digest),
    /// формируется на уровень выше после 401-retry.
    #[allow(clippy::too_many_arguments)]
    fn write_request(
        &mut self,
        method: &str,
        host: &str,
        path: &str,
        range: Option<&RangeRequest>,
        if_range: Option<&RangeValidator>,
        authorization: Option<&str>,
        accept_encoding: Option<&str>,
        extra_headers: &str,
        http_profile: HttpProfile,
    ) -> Result<()> {
        let range_value = range.and_then(|r| r.header_value());
        let range_header = match &range_value {
            Some(value) => format!("Range: {value}\r\n"),
            None => String::new(),
        };
        // If-Range шлём только если есть валидный Range — header без Range
        // ничего не значит для сервера (RFC 7233 §3.2 «sent with a Range
        // header field»).
        let if_range_header = match (&range_value, if_range) {
            (Some(_), Some(v)) => format!("If-Range: {}\r\n", v.header_value()),
            _ => String::new(),
        };
        let auth_header = match authorization {
            Some(value) => format!("Authorization: {value}\r\n"),
            None => String::new(),
        };
        // `extra_headers` уже содержит свои CRLF после каждой строки (формат
        // pre-built). Используется CORS-путём для `Origin` / `Access-Control-*`
        // и для пользовательских author-headers. Caller гарантирует, что
        // среди них нет дублей `Host`/`Connection`/`Content-Length` и т.п.
        //
        // Range/If-Range/Auth идут после fingerprint-заголовков (Chrome order).
        let combined_extra = format!("{range_header}{if_range_header}{auth_header}{extra_headers}");
        let accept_enc = accept_encoding.unwrap_or("");
        let header_block = http::build_request_headers(host, accept_enc, &combined_extra, http_profile);
        let req = format!("{method} {path} HTTP/1.1\r\n{header_block}");
        let stream = self.reader.get_mut();
        stream
            .write_all(req.as_bytes())
            .map_err(|e| Error::Network(format!("write request: {e}")))?;
        stream
            .flush()
            .map_err(|e| Error::Network(format!("flush request: {e}")))?;
        Ok(())
    }

    /// Write an HTTP request with a body (POST/PUT/PATCH/DELETE).
    ///
    /// Adds `Content-Type` and `Content-Length` headers automatically.
    /// `extra_headers` may contain additional pre-formatted `Key: Value\r\n` lines.
    #[allow(clippy::too_many_arguments)]
    fn write_request_with_body(
        &mut self,
        method: &str,
        host: &str,
        path: &str,
        content_type: &str,
        body: &[u8],
        extra_headers: &str,
        http_profile: HttpProfile,
    ) -> Result<()> {
        // Content-Type and Content-Length come after fingerprint headers (Chrome order).
        let body_headers = format!(
            "Content-Type: {content_type}\r\nContent-Length: {}\r\n{extra_headers}",
            body.len()
        );
        let header_block = http::build_request_headers(host, "", &body_headers, http_profile);
        let req = format!("{method} {path} HTTP/1.1\r\n{header_block}");
        let stream = self.reader.get_mut();
        stream
            .write_all(req.as_bytes())
            .map_err(|e| Error::Network(format!("write request: {e}")))?;
        stream
            .write_all(body)
            .map_err(|e| Error::Network(format!("write body: {e}")))?;
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

/// Returns an iterator over all values for the given header name (case-insensitive).
/// `Set-Cookie` may appear multiple times per RFC 7230 §3.2.2.
fn all_header_values<'a>(
    headers: &'a [(String, String)],
    name: &str,
) -> impl Iterator<Item = &'a str> {
    let name_lc = name.to_ascii_lowercase();
    headers
        .iter()
        .filter(move |(k, _)| k.to_ascii_lowercase() == name_lc)
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

/// Применить цепочку Content-Encoding декодеров к body. Парсит header-значение
/// `Content-Encoding` (comma-separated, case-insensitive), ищет совпадающий
/// декодер в `decoders` и прогоняет body через `decode()` в порядке,
/// **обратном** order в header-е (RFC 7231 §3.1.2.2 — encodings applied
/// в обратном порядке к телу: «If multiple encodings have been applied to
/// the representation, the content codings are listed in the order in which
/// they were applied»). `identity` и пустые токены пропускаются.
///
/// Отсутствие header-а / `identity` / `Content-Encoding:` пустой — body
/// возвращается как есть. Encoding, для которого нет декодера, → Err
/// (мы не объявляли его в Accept-Encoding; server нарушил RFC 7231 — лучше
/// падать чем возвращать пользователю мусор).
fn apply_content_encoding(
    body: Vec<u8>,
    headers: &[(String, String)],
    decoders: &[Arc<dyn ContentDecoder>],
) -> Result<Vec<u8>> {
    let header_value = match header_value(headers, "content-encoding") {
        Some(v) => v,
        None => return Ok(body),
    };
    let encodings: Vec<String> = header_value
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s != "identity")
        .collect();
    if encodings.is_empty() {
        return Ok(body);
    }
    let mut current = body;
    // RFC 7231: apply encodings in REVERSE order — последний в header-е
    // был применён первым на сервере, значит первым его и снимаем.
    for encoding in encodings.iter().rev() {
        let decoder = decoders
            .iter()
            .find(|d| d.encoding().eq_ignore_ascii_case(encoding))
            .ok_or_else(|| {
                Error::Network(format!(
                    "unsupported Content-Encoding '{encoding}' (no decoder registered)"
                ))
            })?;
        current = decoder.decode(&current)?;
    }
    Ok(current)
}

// ── Connect ─────────────────────────────────────────────────────────────────

/// Открыть TCP (или TLS поверх TCP) к указанному origin. Резолв host →
/// SocketAddr-ы делегируется в `resolver` (default = SystemDnsResolver).
/// При нескольких адресах (DNS round-robin или IPv4+IPv6 dual-stack)
/// пробуем connect по каждому до первого успешного; ошибка от последнего
/// поднимается наверх, если ни один не подошёл.
fn connect(
    host: &str,
    port: u16,
    is_tls: bool,
    resolver: &dyn DnsResolver,
    tls_profile: tls::TlsProfile,
) -> Result<Connection> {
    // Префикс `resolve ` на всех DNS-ошибках (включая ошибку самого
    // resolver-а) — чтобы `classify_failure_stage` надёжно отнёс их к
    // `RequestStage::Dns` без знания внутреннего формата resolver-сообщения.
    let addrs = resolver
        .resolve(host, port)
        .map_err(|e| Error::Network(format!("resolve {host}:{port}: {e}")))?;
    if addrs.is_empty() {
        return Err(Error::Network(format!(
            "resolve {host}:{port}: no addresses"
        )));
    }

    let mut last_err: Option<Error> = None;
    let mut tcp_opt: Option<TcpStream> = None;
    for addr in &addrs {
        match TcpStream::connect(addr) {
            Ok(s) => {
                tcp_opt = Some(s);
                break;
            }
            Err(e) => {
                last_err = Some(Error::Network(format!("connect {addr}: {e}")));
            }
        }
    }
    let tcp = tcp_opt.ok_or_else(|| {
        last_err.unwrap_or_else(|| Error::Network(format!("connect {host}:{port}: no addresses")))
    })?;

    if !is_tls {
        return Ok(Connection::new(RawStream::Plain(tcp)));
    }

    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|e| Error::Network(format!("invalid hostname '{host}': {e}")))?;

    let mut conn = ClientConnection::new(tls_config_for_profile(tls_profile), server_name)
        .map_err(|e| Error::Network(format!("TLS handshake: {e}")))?;

    // Завершаем handshake до отправки данных — иначе ALPN protocol неизвестен,
    // а нам нужно знать версию (HTTP/1.1 vs HTTP/2) до формирования request bytes.
    let mut tcp = tcp;
    conn.complete_io(&mut tcp)
        .map_err(|e| Error::Network(format!("TLS handshake: {e}")))?;
    let is_h2 = check_negotiated_alpn(conn.alpn_protocol())?;

    let mut c = Connection::new(RawStream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))));
    c.is_h2 = is_h2;
    Ok(c)
}


/// Получить TLS конфиг для указанного профиля. Конфиги кэшируются
/// отдельно для каждого профиля.
pub(crate) fn tls_config_for_profile(profile: tls::TlsProfile) -> Arc<rustls::ClientConfig> {
    use std::sync::OnceLock;
    use std::collections::HashMap;

    static CONFIGS: OnceLock<HashMap<tls::TlsProfile, Arc<rustls::ClientConfig>>> = OnceLock::new();

    let configs = CONFIGS.get_or_init(|| {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let mut map = HashMap::new();
        for prof in &[tls::TlsProfile::Standard, tls::TlsProfile::Strict, tls::TlsProfile::Tor] {
            let cfg = tls::build_client_config(*prof, root_store.clone());
            map.insert(*prof, Arc::new(cfg));
        }
        map
    });

    configs
        .get(&profile)
        .cloned()
        .unwrap_or_else(|| {
            // Fallback: construct on-the-fly if not cached (shouldn't happen)
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Arc::new(tls::build_client_config(profile, root_store))
        })
}

/// Проверить ALPN-протокол, выбранный сервером.
/// Возвращает `true` если согласован HTTP/2, `false` для HTTP/1.1 или без ALPN.
/// Любой другой ALPN — ошибка (rustls должен был отклонить, но defensive).
fn check_negotiated_alpn(alpn: Option<&[u8]>) -> Result<bool> {
    match alpn {
        None | Some(b"http/1.1") => Ok(false),
        Some(b"h2") => Ok(true),
        Some(other) => Err(Error::Network(format!(
            "unexpected ALPN protocol: {:?}",
            String::from_utf8_lossy(other),
        ))),
    }
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
#[allow(clippy::too_many_arguments)]
fn fetch_single(
    pool: &ConnectionPool,
    h2_pool: Option<&H2Pool>,
    resolver: &dyn DnsResolver,
    tls_profile: tls::TlsProfile,
    http_profile: HttpProfile,
    host: &str,
    port: u16,
    is_tls: bool,
    method: &str,
    request_host_header: &str,
    request_path: &str,
    range: Option<&RangeRequest>,
    if_range: Option<&RangeValidator>,
    authorization: Option<&str>,
    accept_encoding: Option<&str>,
    extra_headers: &str,
) -> Result<Response> {
    let key = PoolKey {
        host: host.to_owned(),
        port,
        is_tls,
    };

    // HTTP/2 pool: try reusing an existing H2 connection for this origin.
    if let Some(h2p) = h2_pool {
        let h2_key = pool::PoolKey { host: host.to_owned(), port, is_tls };
        if let Some(h2_conn) = h2p.acquire(&h2_key) {
            let scheme = if is_tls { "https" } else { "http" };
            match h2_do_request_conn(h2_conn, scheme, request_host_header, request_path, extra_headers) {
                Ok((resp, h2_conn)) => {
                    h2p.release(h2_key, h2_conn);
                    return Ok(resp);
                }
                Err(e) if is_stale_error(&e) => {
                    // H2 conn went stale (server sent GOAWAY or closed socket).
                    // Evict and fall through to fresh connect below.
                    h2p.evict(&pool::PoolKey { host: host.to_owned(), port, is_tls });
                }
                Err(e) => return Err(e),
            }
        }
    }

    // Попытка 1: используем pooled connection, если он есть.
    if let Some(pooled) = pool.acquire(&key) {
        match do_request(
            pooled,
            method,
            request_host_header,
            request_path,
            range,
            if_range,
            authorization,
            accept_encoding,
            extra_headers,
            http_profile,
        ) {
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
    let conn = connect(host, port, is_tls, resolver, tls_profile)?;

    // HTTP/2: establish fresh H2Conn, use it, then store back in h2_pool.
    if conn.is_h2 {
        let scheme = if is_tls { "https" } else { "http" };
        return h2_do_request(conn, scheme, request_host_header, request_path, extra_headers, h2_pool, host, port, is_tls, http_profile);
    }

    let (resp, conn) = do_request(
        conn,
        method,
        request_host_header,
        request_path,
        range,
        if_range,
        authorization,
        accept_encoding,
        extra_headers,
        http_profile,
    )?;
    if !conn.closed {
        pool.release(key, conn);
    }
    Ok(resp)
}

/// Выполнить один HTTP/2 запрос, открыв свежее соединение. После успешного
/// ответа соединение возвращается в `h2_pool` (если передан).
#[allow(clippy::too_many_arguments)]
fn h2_do_request(
    conn: Connection,
    scheme: &str,
    authority: &str,
    path: &str,
    extra_headers: &str,
    h2_pool: Option<&H2Pool>,
    host: &str,
    port: u16,
    is_tls: bool,
    http_profile: HttpProfile,
) -> Result<Response> {
    use h2::conn::H2Conn;
    let stream = conn.into_stream();
    let mut h2 = H2Conn::connect_with_profile(stream, http_profile)?;

    let parsed_extra = parse_extra_headers_str(extra_headers);
    let extra_refs: Vec<(&[u8], &[u8])> = parsed_extra
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();

    let (status, headers, body) = h2.fetch("GET", scheme, authority, path, &extra_refs)?;

    if let Some(h2p) = h2_pool {
        let key = pool::PoolKey { host: host.to_owned(), port, is_tls };
        h2p.release(key, h2);
    }

    Ok(Response { status, headers, body })
}

/// Выполнить HTTP/2 запрос через уже существующее `H2Conn`. Возвращает
/// `(Response, H2Conn)` — caller решает, вернуть ли conn в пул.
fn h2_do_request_conn(
    mut h2: h2::conn::H2Conn<RawStream>,
    scheme: &str,
    authority: &str,
    path: &str,
    extra_headers: &str,
) -> Result<(Response, h2::conn::H2Conn<RawStream>)> {
    let parsed_extra = parse_extra_headers_str(extra_headers);
    let extra_refs: Vec<(&[u8], &[u8])> = parsed_extra
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();

    let (status, headers, body) = h2.fetch("GET", scheme, authority, path, &extra_refs)?;
    Ok((Response { status, headers, body }, h2))
}

/// Разобрать строку вида `"Key: Value\r\nKey2: Value2\r\n"` в вектор пар байт.
fn parse_extra_headers_str(s: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    for line in s.split("\r\n") {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            out.push((
                k.trim().to_ascii_lowercase().into_bytes(),
                v.trim().as_bytes().to_vec(),
            ));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn do_request(
    mut conn: Connection,
    method: &str,
    host: &str,
    path: &str,
    range: Option<&RangeRequest>,
    if_range: Option<&RangeValidator>,
    authorization: Option<&str>,
    accept_encoding: Option<&str>,
    extra_headers: &str,
    http_profile: HttpProfile,
) -> Result<(Response, Connection)> {
    conn.write_request(
        method,
        host,
        path,
        range,
        if_range,
        authorization,
        accept_encoding,
        extra_headers,
        http_profile,
    )?;
    let resp = read_response(&mut conn)?;
    Ok((resp, conn))
}

// ── CORS context ─────────────────────────────────────────────────────────────

/// Контекст CORS-enabled fetch-а, прокидывается через `fetch_with_redirect`
/// на каждый hop. `cache` обязателен для memoization preflight-результатов
/// по (requestor, target, credentials_mode). См. [`HttpClient::fetch_cors`].
///
/// На каждом hop:
/// 1. Если `Origin::from_url(url) != requestor` (cross-origin) — собираем
///    `CorsRequest` под текущий target и идём в preflight enforcement +
///    actual-response validation.
/// 2. Same-origin hop — поведение идентично обычному `fetch_subresource`
///    (Origin header не шлётся, ACAO не проверяется).
struct CorsContext<'a> {
    requestor: Origin,
    method: String,
    headers: Vec<(String, String)>,
    credentials_mode: cors::CredentialsMode,
    cache: &'a cors::PreflightCache,
}

/// Эмит RequestBlocked + Err для CORS-отказа. Reason имеет формат
/// `cors-<phase>: <CorsError>` чтобы наблюдатели могли различить preflight
/// и actual-response failures.
fn emit_cors_blocked(
    sink: Option<&dyn EventSink>,
    tab_id: TabId,
    url: &Url,
    phase: &str,
    err: &cors::CorsError,
) -> Error {
    let reason = format!("cors-{phase}: {err}");
    if let Some(s) = sink {
        s.emit(&Event::RequestBlocked {
            tab_id,
            url: url.clone(),
            reason: reason.clone(),
        });
    }
    Error::Network(format!("blocked: {reason}"))
}

/// Классифицировать стадию сетевого сбоя по тексту `Error::Network`.
///
/// Ошибки в этом крейте стрингово-типизированы (`Error::Network(String)`),
/// но сообщения имеют стабильные префиксы по точке возникновения:
/// `resolve …` (DNS), `connect …` (TCP), `TLS handshake …` / `invalid
/// hostname …` / `unexpected ALPN …` (TLS); всё остальное (`read …`,
/// `write …`, `EOF …`, `chunked …`) — стадия обмена данными `Read`.
/// Сопоставление по префиксу, а не подстроке, чтобы случайное вхождение
/// слова в URL/заголовке не сбило классификацию.
fn classify_failure_stage(reason: &str) -> RequestStage {
    if reason.starts_with("resolve ") {
        RequestStage::Dns
    } else if reason.starts_with("connect ") {
        RequestStage::Tcp
    } else if reason.starts_with("TLS handshake")
        || reason.starts_with("invalid hostname")
        || reason.starts_with("unexpected ALPN")
    {
        RequestStage::Tls
    } else {
        RequestStage::Read
    }
}

/// Эмит `RequestFailed` для сетевого сбоя `fetch_single` и возврат той же
/// ошибки наверх. Поддерживает инвариант «один `RequestStarted` → ровно один
/// терминальный event»: вызывается симметрично с `RequestStarted`, когда
/// `fetch_single` вернул `Err` до получения HTTP-статуса. Стадия выводится из
/// текста ошибки через [`classify_failure_stage`].
fn emit_request_failed(
    sink: Option<&dyn EventSink>,
    tab_id: TabId,
    url: &Url,
    err: Error,
) -> Error {
    if let Some(s) = sink {
        let reason = match &err {
            Error::Network(msg) => msg.clone(),
            other => other.to_string(),
        };
        s.emit(&Event::RequestFailed {
            tab_id,
            url: url.clone(),
            stage: classify_failure_stage(&reason),
            reason,
        });
    }
    err
}

/// Собрать значение `extra_headers` для actual cross-origin запроса:
/// `Origin` (RFC 6454 / Fetch §3.5) + author-headers, кроме тех, что мы и так
/// формируем в `write_request` (Host / Connection / User-Agent / Accept /
/// Accept-Encoding / Authorization / Range / If-Range). Author code НЕ должен
/// эти заголовки ставить — Fetch §4.4.4 «forbidden request-header name»
/// (caller отфильтровывал заранее), но защитимся case-insensitively.
fn build_actual_cross_origin_headers(
    requestor: &Origin,
    author_headers: &[(String, String)],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("Origin: {}\r\n", requestor.serialize()));
    for (k, v) in author_headers {
        let lower = k.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "host"
                | "connection"
                | "user-agent"
                | "accept"
                | "accept-encoding"
                | "authorization"
                | "range"
                | "if-range"
                | "content-length"
                | "origin"
        ) {
            continue;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out
}

/// Заголовки для preflight (Fetch §4.8 step 2-7) в виде pre-formatted string.
fn build_preflight_extra_headers(cors_req: &cors::CorsRequest) -> String {
    let pairs = cors::build_preflight_headers(cors_req);
    let mut out = String::new();
    for (k, v) in pairs {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out
}

// ── Редиректы ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn fetch_with_redirect(
    url: &Url,
    hops_left: u8,
    pool: &ConnectionPool,
    h2_pool: Option<&H2Pool>,
    resolver: &dyn DnsResolver,
    tls_profile: tls::TlsProfile,
    http_profile: HttpProfile,
    sink: Option<&dyn EventSink>,
    filter: Option<&dyn RequestFilter>,
    hsts_store: Option<&dyn HstsEnforcement>,
    credentials: Option<&dyn HttpCredentialProvider>,
    decoders: &[Arc<dyn ContentDecoder>],
    accept_encoding: Option<&str>,
    range: Option<&RangeRequest>,
    if_range: Option<&RangeValidator>,
    tab_id: TabId,
    mixed_content: Option<&MixedContentPolicy>,
    destination: Option<RequestDestination>,
    cors_ctx: Option<&CorsContext<'_>>,
    // Extra headers for HTTP cache conditional GETs (If-None-Match / If-Modified-Since).
    cache_extra_headers: &str,
    cookie_jar: Option<&dyn CookieProvider>,
    top_level_site: Option<&str>,
) -> Result<Response> {
    if hops_left == 0 {
        return Err(Error::Network("too many redirects".to_owned()));
    }

    // HSTS upgrade: до require_http_scheme (RFC 6797 §8.3 — канонизация URI
    // делается на этапе URI loading, не fetch), до RequestFilter и до Started.
    // Это значит, что filter и observer видят уже-upgraded URL: блок-листы
    // могут не пропустить https-вариант, а network log показывает реальный
    // URL, по которому пошёл трафик. Применяется на каждом redirect-hop —
    // симметрично с filter / sink / resolver.
    let now_unix = hsts::current_unix_time();
    let upgraded: Option<Url> = match hsts_store {
        Some(h) => hsts::maybe_upgrade_url_to_https(h, url, now_unix)?,
        None => None,
    };
    let url = upgraded.as_ref().unwrap_or(url);

    // require_http_scheme валидирует scheme/host/port раньше, чем мы откроем
    // сокет. События эмитим только если форма запроса прошла валидацию: на
    // bad scheme (`ftp://...`) ни RequestStarted, ни RequestCompleted, ни
    // RequestBlocked — байт даже не подумал улетать, и сам URL невалиден для
    // фильтра. Сетевые ошибки после валидации (DNS, refused, TLS handshake)
    // оставляют Started без Completed — это инвариант «started + missing
    // completed = network failure»; явный RequestFailed добавим, когда
    // увидим, что наблюдателям этого мало.
    let (host_ascii, port, is_tls) = require_http_scheme(url)?;

    // Mixed-content enforcement (W3C Mixed Content §5) — после HSTS upgrade
    // (если http→https произошёл, mixed-content уже не возникнет), перед
    // RequestFilter и Started. Активируется только когда оба значения
    // (policy и destination) заданы. fetch_subresource передаёт явный
    // destination; NetworkTransport::fetch использует Other как fallback
    // когда policy задана, None — когда нет (top-level navigation).
    //
    // Per redirect-hop: HTTPS → HTTP редирект на blockable destination
    // тоже блокируется (URL берётся именно тот, по которому пойдёт трафик).
    if let (Some(policy), Some(dest)) = (mixed_content, destination)
        && let Some(level) = policy.evaluate(url, dest)
    {
        let reason = mixed_content::block_reason(level);
        if let Some(s) = sink {
            s.emit(&Event::RequestBlocked {
                tab_id,
                url: url.clone(),
                reason: reason.clone(),
            });
        }
        return Err(Error::Network(format!("blocked: {reason}")));
    }

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

    // CORS preflight enforcement (Fetch §4.8) — после mixed-content / filter,
    // до RequestStarted / fetch_single. Включается только если caller
    // создал `CorsContext` (через `HttpClient::fetch_cors`); top-level
    // navigation и same-origin subresource этого не делают.
    //
    // Hop-локальная классификация: target_origin = Origin::from_url(url) на
    // ТЕКУЩЕМ hop-е. Cross-origin → собираем `CorsRequest` под этот hop и:
    //   1) lookup в кеше по `(requestor, target_origin, credentials_mode)`,
    //      покрывает ли cached PreflightResult текущий method+headers;
    //   2) если не покрывает И `needs_preflight(&req)` — шлём OPTIONS
    //      preflight (метод OPTIONS, extra-headers = Origin / ACRM / ACRH).
    //      На preflight тоже эмитятся RequestStarted+RequestCompleted —
    //      этот байт пользователь видит (принцип №4 «каждый исходящий байт
    //      виден»). При неуспехе — RequestBlocked + Err.
    //   3) при cache-hit или successful preflight — продолжаем к actual.
    //
    // Same-origin или `cors_ctx == None` → ветка не активируется.
    let mut cross_origin_target: Option<Origin> = None;
    if let Some(cx) = cors_ctx
        && let Ok(target_origin) = Origin::from_url(url)
        && !cx.requestor.same_origin(&target_origin)
    {
        cross_origin_target = Some(target_origin.clone());
        let cors_req = cors::CorsRequest {
            origin: cx.requestor.clone(),
            target: url.clone(),
            method: cx.method.clone(),
            headers: cx.headers.clone(),
            credentials_mode: cx.credentials_mode,
        };
        // Cache hit shortcut.
        if !cx.cache.allows(&cors_req) && cors::needs_preflight(&cors_req) {
            if let Some(s) = sink {
                s.emit(&Event::RequestStarted {
                    tab_id,
                    url: url.clone(),
                });
            }
            let preflight_extra = build_preflight_extra_headers(&cors_req);
            let preflight_resp = match fetch_single(
                pool,
                h2_pool,
                resolver,
                tls_profile,
                http_profile,
                &host_ascii,
                port,
                is_tls,
                "OPTIONS",
                &host_ascii,
                &url.path_and_query(),
                None,
                None,
                None,
                None,
                &preflight_extra,
            ) {
                Ok(r) => r,
                Err(e) => return Err(emit_request_failed(sink, tab_id, url, e)),
            };
            if let Some(s) = sink {
                s.emit(&Event::RequestCompleted {
                    tab_id,
                    url: url.clone(),
                    status: preflight_resp.status,
                });
            }
            match cors::evaluate_preflight_response(
                preflight_resp.status,
                &preflight_resp.headers,
                &cors_req,
            ) {
                Ok(result) => {
                    cx.cache.insert(
                        cx.requestor.clone(),
                        target_origin,
                        cx.credentials_mode,
                        result,
                    );
                }
                Err(err) => {
                    return Err(emit_cors_blocked(sink, tab_id, url, "preflight", &err));
                }
            }
        }
    }

    // Метод и cross-origin extra-headers для actual запроса.
    let actual_method = cors_ctx.map(|cx| cx.method.as_str()).unwrap_or("GET");
    let actual_extra_headers = {
        let mut h = match (cors_ctx, &cross_origin_target) {
            (Some(cx), Some(_)) => build_actual_cross_origin_headers(&cx.requestor, &cx.headers),
            _ => String::new(),
        };
        // Append cache conditional headers (If-None-Match / If-Modified-Since).
        if !cache_extra_headers.is_empty() {
            h.push_str(cache_extra_headers);
        }
        // Inject Cookie header (RFC 6265 §5.4). Cross-site is true when
        // top_level_site is set and differs from the request host (covers
        // both SameSite enforcement and Total Cookie Protection).
        if let Some(jar) = cookie_jar {
            let is_cross_site = match top_level_site {
                Some(tls) => !host_ascii.ends_with(tls) && host_ascii != tls,
                None => false,
            };
            let cookie_val = jar.get_for_request(
                &host_ascii,
                &url.path_and_query(),
                is_tls,
                top_level_site,
                is_cross_site,
            );
            if !cookie_val.is_empty() {
                h.push_str("Cookie: ");
                h.push_str(&cookie_val);
                h.push_str("\r\n");
            }
        }
        h
    };

    // 401-retry loop: первый запрос без Authorization, при 401 + creds —
    // один retry с Authorization-header построенным из challenge. Больше
    // одного retry на hop запрещено (две 401 подряд = неверные creds).
    //
    // Authorization намеренно НЕ переносится на redirect-hop: RFC 7235 §3.1
    // — implementations SHOULD NOT use credentials with arbitrary URIs; в
    // нашей рекурсивной модели свежий fetch_with_redirect для redirect-target
    // начинается с пустым `authorization`, и провайдер опрашивается заново
    // под новый origin/realm.
    let mut authorization: Option<String> = None;
    loop {
        if let Some(s) = sink {
            s.emit(&Event::RequestStarted {
                tab_id,
                url: url.clone(),
            });
        }

        let mut resp = match fetch_single(
            pool,
            h2_pool,
            resolver,
            tls_profile,
            http_profile,
            &host_ascii,
            port,
            is_tls,
            actual_method,
            &host_ascii,
            &url.path_and_query(),
            range,
            if_range,
            authorization.as_deref(),
            accept_encoding,
            &actual_extra_headers,
        ) {
            Ok(r) => r,
            Err(e) => return Err(emit_request_failed(sink, tab_id, url, e)),
        };

        // HSTS: сохранить policy из header-а, если ответ пришёл по HTTPS и
        // server прислал Strict-Transport-Security. RFC 6797 §8.1 — STS на
        // HTTP-ответе игнорируется (active attacker мог бы её подделать).
        // Best-effort: ошибки storage не валят fetch (см. doc HstsEnforcement).
        // Делается на каждом hop, не только финальном: 3xx-ответ тоже может
        // нести STS-policy.
        if let Some(h) = hsts_store {
            hsts::process_sts_response(h, url.scheme(), &host_ascii, &resp.headers, now_unix);
        }

        // RequestCompleted эмитим всегда после получения статуса, до анализа кода:
        // редирект-hop, 4xx, 5xx — всё это «outgoing byte был виден ответом».
        if let Some(s) = sink {
            s.emit(&Event::RequestCompleted {
                tab_id,
                url: url.clone(),
                status: resp.status,
            });
        }

        // CORS actual-response validation (Fetch §4.10) — на каждом
        // cross-origin hop, ДО status-branching. ACAO обязан присутствовать
        // в любом cross-origin ответе (включая 3xx с body), иначе response
        // — «cors-filtered», caller прав видеть тело не имеет. При ошибке
        // эмитим RequestBlocked + Err. Auth-retry (401 без ACAO) ловится
        // здесь же — это намеренно, без ACAO мы не имеем права повторять
        // запрос с Authorization для CORS-режима.
        if cross_origin_target.is_some()
            && let Some(cx) = cors_ctx
            && let Err(err) = cors::check_cors_response_headers(
                &resp.headers,
                &cx.requestor,
                cx.credentials_mode,
            )
        {
            return Err(emit_cors_blocked(sink, tab_id, url, "response", &err));
        }

        // Persist Set-Cookie headers (RFC 6265 §5.3) on every hop.
        // Best-effort: cookie errors never fail the fetch.
        if let Some(jar) = cookie_jar {
            let req_path = url.path_and_query();
            let default_path = req_path.split('?').next().unwrap_or("/");
            for val in all_header_values(&resp.headers, "set-cookie") {
                jar.process_set_cookie(val, &host_ascii, default_path, is_tls, top_level_site);
            }
        }

        match resp.status {
            200..=299 => {
                // Content-Encoding decoding: применяется только к финальному
                // (не redirect) ответу с success-статусом. 3xx редко несут body,
                // и application к промежуточным телам редиректа бессмысленна —
                // мы их выбрасываем. Decoding идёт на КАЖДОМ hop с финальным
                // успехом; для 4xx/5xx — нет (caller получает Err по статусу,
                // тело туда не доходит).
                resp.body = apply_content_encoding(resp.body, &resp.headers, decoders)?;
                return Ok(resp);
            }
            // 304 Not Modified: conditional GET confirmed the cached copy is
            // still valid. Return the response as-is (empty body); caller is
            // responsible for substituting the cached body and updating cache
            // metadata via HttpCache::revalidate().
            304 => return Ok(resp),
            301 | 302 | 303 | 307 | 308 => {
                let location = header_value(&resp.headers, "location")
                    .ok_or_else(|| Error::Network("redirect without Location".to_owned()))?;
                let next = url
                    .resolve(location)
                    .map_err(|e| Error::Network(format!("resolve redirect '{location}': {e}")))?;
                // Range пробрасывается в redirect-target: пользователь
                // запросил range на исходном URL, ожидает тот же range от
                // final-resource (это и есть смысл redirect для range-GET).
                // CORS context — тот же `requestor` через все hops, чтобы
                // cross-origin redirect-hop re-classify-ился под актуальный
                // target_origin (см. начало fetch_with_redirect).
                return fetch_with_redirect(
                    &next,
                    hops_left - 1,
                    pool,
                    h2_pool,
                    resolver,
                    tls_profile,
                    http_profile,
                    sink,
                    filter,
                    hsts_store,
                    credentials,
                    decoders,
                    accept_encoding,
                    range,
                    if_range,
                    tab_id,
                    mixed_content,
                    destination,
                    cors_ctx,
                    // Cache conditional headers are per-resource; RFC 7234 §4 —
                    // after a redirect the new URL may have different cache state.
                    // Drop conditional headers on redirect to avoid 304 surprises.
                    "",
                    cookie_jar,
                    top_level_site,
                );
            }
            401 if authorization.is_none() && credentials.is_some() => {
                // Распарсить WWW-Authenticate и попробовать построить Authorization.
                // Любая ошибка по пути (нет header-а, неподдерживаемая схема,
                // провайдер не нашёл creds, builder вернул None) → пробросить
                // 401 как есть, без retry.
                let www_auth = match header_value(&resp.headers, "www-authenticate") {
                    Some(v) => v.to_owned(),
                    None => return Err(Error::Network("HTTP 401".to_owned())),
                };
                let challenges = auth::parse_www_authenticate(&www_auth);
                let (scheme, parsed) = match auth::select_best_challenge(&challenges) {
                    Some(pair) => pair,
                    None => return Err(Error::Network("HTTP 401".to_owned())),
                };
                let origin = auth::origin_of(url);
                let api_challenge = auth::challenge_for_provider(&origin, scheme, parsed);
                let creds = match credentials.unwrap().credentials(&api_challenge) {
                    Some(c) => c,
                    None => return Err(Error::Network("HTTP 401".to_owned())),
                };
                let header = match scheme {
                    HttpAuthScheme::Basic => auth::build_basic_authorization(&creds),
                    HttpAuthScheme::Digest => match auth::build_digest_authorization(
                        &creds,
                        parsed,
                        actual_method,
                        &url.path_and_query(),
                    ) {
                        Some(h) => h,
                        None => return Err(Error::Network("HTTP 401".to_owned())),
                    },
                };
                authorization = Some(header);
                // Continue loop — повторим тот же hop с Authorization.
            }
            status => return Err(Error::Network(format!("HTTP {status}"))),
        }
    }
}

// ── Публичный API ────────────────────────────────────────────────────────────

/// HTTP proxy configuration (RFC 7230 proxy behavior).
///
/// Для HTTP: запрос отправляется на proxy-host:proxy-port с абсолютным URL в request line.
/// Для HTTPS: используется CONNECT-туннель (RFC 7231 §4.3.6) — отправляем
/// `CONNECT target-host:target-port HTTP/1.1` на proxy, затем TLS handshake
/// над полученным туннелем, затем обычный HTTPS-запрос с относительным путём.
/// Если auth присутствует, Proxy-Authorization (Basic) отправляется в обоих случаях.
pub struct HttpProxy {
    /// Hostname или IP адрес прокси-сервера.
    pub host: String,
    /// Порт прокси-сервера (обычно 3128 для Squid, 8080 для других).
    pub port: u16,
    /// Optional username:password для базовой аутентификации прокси.
    /// Формат: base64(username:password).
    pub auth: Option<String>,
}

impl HttpProxy {
    /// Создать новый прокси без аутентификации.
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            auth: None,
        }
    }

    /// Создать прокси с базовой аутентификацией (username:password).
    pub fn with_basic_auth(mut self, username: &str, password: &str) -> Self {
        let creds = format!("{}:{}", username, password);
        self.auth = Some(base64_encode(&creds));
        self
    }
}

/// Encode string to base64 (используется для Basic auth в Proxy-Authorization).
fn base64_encode(s: &str) -> String {
    const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = s.as_bytes();
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b1 = chunk[0];
        let b2 = chunk.get(1).copied().unwrap_or(0);
        let b3 = chunk.get(2).copied().unwrap_or(0);
        let n = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
        result.push(BASE64_CHARS[((n >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(BASE64_CHARS[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(BASE64_CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

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
    interceptor: Option<Arc<dyn FetchInterceptor>>,
    pool: Arc<ConnectionPool>,
    h2_pool: Option<Arc<H2Pool>>,
    resolver: Arc<dyn DnsResolver>,
    hsts: Option<Arc<dyn HstsEnforcement>>,
    credentials: Option<Arc<dyn HttpCredentialProvider>>,
    decoders: Vec<Arc<dyn ContentDecoder>>,
    tab_id: TabId,
    mixed_content: Option<MixedContentPolicy>,
    cors_cache: Option<Arc<cors::PreflightCache>>,
    /// RFC 7234 response cache. Optional — without it every request goes to the network.
    http_cache: Option<Arc<http_cache::HttpCache>>,
    /// RFC 6265 cookie jar. Injects `Cookie:` headers and persists `Set-Cookie:` responses.
    cookie_jar: Option<Arc<dyn CookieProvider>>,
    /// Registrable domain of the top-level page, used for Total Cookie Protection partitioning.
    top_level_site: Option<String>,
    /// HTTP fingerprinting profile (Standard/Strict/Tor) — determines header order and casing
    /// matching Chrome to avoid detection (ADR-007 Layer 3). Default is Standard.
    fingerprint_profile: HttpProfile,
    /// TLS fingerprinting profile — cipher suite order, kx_groups, ALPN, protocol versions.
    /// Derived from `fingerprint_profile` by default; can be overridden with `with_tls_profile`.
    tls_profile: tls::TlsProfile,
    /// HTTP proxy (RFC 7230) for routing requests through proxy server.
    /// Optional — without it requests go directly to target. With proxy:
    /// — HTTP: direct GET to proxy with absolute URL
    /// — HTTPS: CONNECT tunnel to proxy, then TLS over tunnel
    proxy: Option<Arc<HttpProxy>>,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            sink: None,
            filter: None,
            interceptor: None,
            pool: Arc::new(ConnectionPool::new()),
            h2_pool: None,
            resolver: Arc::new(SystemDnsResolver),
            hsts: None,
            credentials: None,
            decoders: Vec::new(),
            tab_id: TabId(0),
            mixed_content: None,
            cors_cache: None,
            http_cache: None,
            cookie_jar: None,
            top_level_site: None,
            fingerprint_profile: HttpProfile::Chrome,
            tls_profile: tls::TlsProfile::Standard,
            proxy: None,
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

    /// Подключить Service Worker перехватчик fetch-запросов. Проверяется
    /// до выхода в сеть: если `intercept()` вернул `Some(body)` — ответ
    /// берётся из SW-кэша без TCP-соединения. `None` — обычный сетевой fetch.
    ///
    /// Реализация — `lumen-storage::ServiceWorkerInterceptor` (SQLite-backed).
    /// Для тестов без SQLite используется `InMemoryFetchInterceptor`.
    #[must_use]
    pub fn with_interceptor(mut self, interceptor: Arc<dyn FetchInterceptor>) -> Self {
        self.interceptor = Some(interceptor);
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

    /// Подключить shared `H2Pool` (RFC 9113 §9.1.1). По умолчанию HTTP/2
    /// соединения открываются заново на каждый запрос. С подключённым пулом
    /// соединение переиспользуется: последовательные запросы к одному origin-у
    /// идут по одному TLS/TCP-сокету, stream ID монотонно растёт (1, 3, 5...).
    #[must_use]
    pub fn with_h2_pool(mut self, pool: Arc<H2Pool>) -> Self {
        self.h2_pool = Some(pool);
        self
    }

    /// Подключить DNS-резолвер. По умолчанию — `SystemDnsResolver` (через
    /// `(host, port).to_socket_addrs()`); подменяется на `CachedDnsResolver`
    /// (lumen-storage) для TTL-кеша, или на DoH/DoT для приватности (§13).
    #[must_use]
    pub fn with_dns_resolver(mut self, resolver: Arc<dyn DnsResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// Подключить HSTS-store (RFC 6797). По умолчанию — нет:
    /// http-запросы идут как есть, `Strict-Transport-Security` header
    /// в ответах игнорируется. С подключённым store:
    /// — pre-request: http→https upgrade для known-hosts (включая
    ///   includeSubDomains-родителей);
    /// — post-response: парсинг STS header из HTTPS-ответов, persist policy.
    /// Каждый redirect-hop проверяется независимо.
    ///
    /// Реализация — `lumen-storage::hsts::HstsStore`. Trait-граница
    /// `HstsEnforcement` (lumen-core::ext) позволяет lumen-network не
    /// зависеть от lumen-storage напрямую.
    #[must_use]
    pub fn with_hsts(mut self, hsts: Arc<dyn HstsEnforcement>) -> Self {
        self.hsts = Some(hsts);
        self
    }

    /// Подключить credential-провайдер для HTTP authentication (RFC 7235 /
    /// 7616 / 7617). По умолчанию — нет: запросы уходят без `Authorization`
    /// header, и 401 пробрасывается как `Err`. С подключённым провайдером:
    /// — на 401 + `WWW-Authenticate` выбирается сильнейший challenge
    ///   (Digest > Basic, внутри Digest — SHA-256 > MD5);
    /// — провайдеру передаётся `HttpAuthChallenge { origin, realm, scheme }`;
    /// — если он вернул `Some(creds)` — клиент шлёт второй запрос с
    ///   `Authorization`; иначе 401 пробрасывается наверх.
    /// Retry один на hop. Authorization не пересылается на 3xx-redirect:
    /// после redirect-а провайдер опрашивается заново с новым origin.
    #[must_use]
    pub fn with_credentials(
        mut self,
        credentials: Arc<dyn HttpCredentialProvider>,
    ) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Указать `TabId`, который попадёт в каждое emit-ое событие. В Phase 0
    /// (без вкладок) shell оставляет дефолтный `TabId(0)`.
    #[must_use]
    pub fn with_tab(mut self, tab_id: TabId) -> Self {
        self.tab_id = tab_id;
        self
    }

    /// Подключить mixed-content policy (W3C Mixed Content §5). По умолчанию
    /// нет: подресурс-fetch-и не классифицируются, любой URL уходит в сеть
    /// без оценки secure-context-а. С подключённой policy:
    /// — `fetch_subresource(url, destination)` классифицирует каждый запрос
    ///   относительно `top_level`-origin документа;
    /// — `Blockable` блокируется в обоих режимах (`SpecDefault` / `Strict`);
    /// — `OptionallyBlockable` блокируется только в `Strict`;
    /// — `NotMixed` (HTTPS / data: / blob: / loopback) — всегда пропускается;
    /// — каждый redirect-hop проверяется независимо (HTTPS → HTTPS → HTTP
    ///   на blockable subresource блокируется на финальном hop).
    ///
    /// `fetch(url)` через `NetworkTransport` НЕ enforce-ит mixed-content —
    /// это путь для top-level navigation, у которой нет «mixing» по
    /// определению (она сама задаёт secure-context).
    #[must_use]
    pub fn with_mixed_content_policy(
        mut self,
        top_level: Origin,
        mode: MixedContentMode,
    ) -> Self {
        self.mixed_content = Some(MixedContentPolicy::new(top_level, mode));
        self
    }

    /// Зарегистрировать `ContentDecoder` для одного encoding. Декодер попадает
    /// в `Accept-Encoding` запроса (имя через `encoding()`); при получении
    /// `Content-Encoding: <тот же encoding>` в ответе body прогоняется через
    /// `decode()`. Можно вызывать многократно для разных encoding-ов; порядок
    /// регистрации = порядок предпочтения в Accept-Encoding (первый — самый
    /// предпочитаемый).
    ///
    /// По умолчанию декодеры не подключены — `Accept-Encoding` не выставляется,
    /// и ответ с `Content-Encoding: <что-нибудь>` будет ошибкой
    /// (RFC 7231 §3.1.2.2 — если клиент не объявлял поддержку, server не
    /// должен использовать `Content-Encoding`, но реальные серверы это
    /// нарушают). По принципу политики зависимостей (§5) — добавлять
    /// декодеры в эту регистрацию должен caller (shell), не lumen-network:
    /// тестовая среда хочет тестировать без brotli, production — с ним.
    #[must_use]
    pub fn with_content_decoder(mut self, decoder: Arc<dyn ContentDecoder>) -> Self {
        self.decoders.push(decoder);
        self
    }

    /// Сформировать значение `Accept-Encoding` из зарегистрированных декодеров,
    /// или `None`, если декодеров нет. Имена через запятую, в порядке
    /// регистрации.
    fn accept_encoding_header(&self) -> Option<String> {
        if self.decoders.is_empty() {
            None
        } else {
            let parts: Vec<&str> = self.decoders.iter().map(|d| d.encoding()).collect();
            Some(parts.join(", "))
        }
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    /// Запросить только диапазон байт ресурса (RFC 7233). Если сервер
    /// поддерживает Range, ответит `206 Partial Content` с заголовком
    /// `Content-Range: bytes START-END/TOTAL` — поле `content_range`
    /// будет заполнено. Если сервер игнорирует Range и отдаёт `200 OK`
    /// с полным телом, `content_range` будет `None` (RFC 7233 §3.1
    /// явно разрешает оба ответа — клиент должен принять любой).
    ///
    /// `if_range` — опциональный validator (`If-Range`, RFC 7233 §3.2):
    /// если ресурс не изменился (ETag / Last-Modified совпадает), server
    /// отдаёт `206` с запрошенным диапазоном; если изменился — `200` с
    /// полным новым телом. Это защита от race condition при resume
    /// downloads. `None` — без `If-Range` (clean range request).
    ///
    /// 4xx/5xx, в том числе `416 Range Not Satisfiable`, возвращаются
    /// как `Err(Error::Network("HTTP 416"))` — caller отличает их от
    /// network failure по тексту.
    ///
    /// Phase 0 ограничения: только single range (closed `START-END`,
    /// open-ended `START-`, suffix `-N`); multi-range (`bytes=0-99,200-299`
    /// → multipart/byteranges) — не поддерживается. Range и `If-Range`
    /// пересылаются на redirect-target (3xx сохраняет conditional).
    pub fn with_cors_cache(mut self, cache: Arc<cors::PreflightCache>) -> Self {
        self.cors_cache = Some(cache);
        self
    }

    /// Attach a cookie store. The provider receives `Cookie:` injection
    /// requests and `Set-Cookie:` responses on every fetch.
    ///
    /// `top_level_site` — registrable domain of the top-level document (used
    /// for Total Cookie Protection partitioning and SameSite evaluation).
    /// Pass `None` when no top-level context is known (e.g. background fetch).
    #[must_use]
    pub fn with_cookie_jar(
        mut self,
        jar: Arc<dyn CookieProvider>,
        top_level_site: Option<String>,
    ) -> Self {
        self.cookie_jar = Some(jar);
        self.top_level_site = top_level_site;
        self
    }

    /// Подключить HTTP response cache (RFC 7234).
    ///
    /// Кэш проверяется до выхода в сеть. Свежие записи возвращаются сразу.
    /// Записи с истёкшей свежестью, но с валидаторами (`ETag`/`Last-Modified`)
    /// провоцируют conditional GET (`If-None-Match`/`If-Modified-Since`).
    /// 304 Not Modified → тело берётся из кэша, метаданные обновляются.
    ///
    /// Кэш можно делить между несколькими `HttpClient`-ами через `Arc::clone` —
    /// реализован через `Mutex<HashMap>` и thread-safe.
    ///
    /// Phase 0 ограничения: только GET-запросы кэшируются; range-запросы,
    /// POST/PUT и запросы с явным `cors_ctx` кэш пропускают; `Vary` не
    /// поддерживается (unsafe Vary не помечается).
    #[must_use]
    pub fn with_http_cache(mut self, cache: Arc<http_cache::HttpCache>) -> Self {
        self.http_cache = Some(cache);
        self
    }

    /// Подключить HTTP прокси (RFC 7230). По умолчанию прокси не подключён — запросы
    /// идут напрямую на целевой сервер. С подключённым прокси:
    /// — HTTP: запрос отправляется на прокси с абсолютным URL в request line
    /// — HTTPS: используется CONNECT-туннель (RFC 7231 §4.3.6)
    /// — оба: если прокси требует аутентификацию, добавляется Proxy-Authorization header
    #[must_use]
    pub fn with_proxy(mut self, proxy: Arc<HttpProxy>) -> Self {
        self.proxy = Some(proxy);
        self
    }

    /// Установить HTTP fingerprinting profile (Standard/Strict/Tor) для Chrome-matching
    /// header order и Client Hints handling (ADR-007 §3.1). По умолчанию — Standard.
    /// - Standard: полная Chrome 130+ совместимость (header order + Client Hints)
    /// - Strict: private-mode, Client Hints отключены
    /// - Tor: минимальный fingerprint для tor-browser
    #[must_use]
    pub fn with_fingerprint_profile(mut self, profile: HttpProfile) -> Self {
        self.fingerprint_profile = profile;
        // Derive TLS profile from HTTP profile unless already explicitly overridden.
        self.tls_profile = tls::http_to_tls_profile(profile);
        self
    }

    /// Получить текущий HTTP fingerprinting profile.
    pub fn fingerprint_profile(&self) -> HttpProfile {
        self.fingerprint_profile
    }

    /// Override the TLS fingerprint profile independently of the HTTP profile.
    ///
    /// Normally `TlsProfile` is derived from `HttpProfile`
    /// (`Strict` → `TlsProfile::Strict`, `TorBrowser` → `TlsProfile::Tor`,
    /// others → `TlsProfile::Standard`). Use this when you need fine-grained
    /// control — e.g., Chrome HTTP headers but TLS 1.3-only cipher suites.
    #[must_use]
    pub fn with_tls_profile(mut self, profile: tls::TlsProfile) -> Self {
        self.tls_profile = profile;
        self
    }

    /// Получить текущий TLS fingerprinting profile.
    pub fn tls_profile(&self) -> tls::TlsProfile {
        self.tls_profile
    }

    /// CORS-enabled fetch для cross-origin subresource (Fetch §3-§4).
    /// Поведение:
    /// - Same-origin: тождественно `fetch(url)` — preflight не шлётся,
    ///   Origin header не добавляется, ACAO не проверяется.
    /// - Cross-origin без preflight (`needs_preflight(&req) == false`,
    ///   например GET без custom headers и cookies-Omit): запрос уходит с
    ///   `Origin`-header, ответ валидируется через `check_cors_response_headers`.
    /// - Cross-origin с preflight: lookup в `PreflightCache`, если miss —
    ///   отправляется OPTIONS preflight с `Origin`/`ACRM`/`ACRH`; ответ
    ///   evaluatе через `evaluate_preflight_response`; успешный результат
    ///   кешируется на `Access-Control-Max-Age` секунд. Затем actual
    ///   запрос + actual-response validation.
    /// - На каждом redirect-hop hop-локальный target_origin переклассифицируется
    ///   (HTTPS → cross-origin redirect → re-preflight под новый target).
    /// - При CORS-отказе (preflight или response) эмитится `RequestBlocked`
    ///   с reason `cors-preflight: <CorsError>` или `cors-response: <CorsError>`,
    ///   функция возвращает `Err`.
    ///
    /// **Требует `with_cors_cache(...)`** — без подключённого кеша вызов
    /// возвращает Err. Кеш можно делить между несколькими `HttpClient`-ами
    /// (через `Arc::clone`) — кэш thread-safe.
    ///
    /// Phase 0 ограничения:
    /// - HttpClient в Phase 0 не поддерживает request body — POST/PUT/PATCH
    ///   уходят без body (Content-Length: 0). Для preflight + ACAO-проверки
    ///   это работает; для реальных XHR с JSON-body нужно body-pipeline.
    /// - Cookie-jar не интегрирован, credentials_mode влияет только на
    ///   ACAO=`*` rejection и ACAC=true requirement.
    /// - Forbidden request-headers caller обязан отфильтровать заранее
    ///   (`cors::is_forbidden_request_header`).
    pub fn fetch_cors(
        &self,
        request: cors::CorsRequest,
        destination: Option<RequestDestination>,
    ) -> Result<Vec<u8>> {
        let cache = self
            .cors_cache
            .as_deref()
            .ok_or_else(|| Error::Network("CORS preflight cache not configured (call with_cors_cache)".to_owned()))?;
        let target = request.target.clone();
        let cors_ctx = CorsContext {
            requestor: request.origin,
            method: request.method,
            headers: request.headers,
            credentials_mode: request.credentials_mode,
            cache,
        };
        let accept_encoding = self.accept_encoding_header();
        fetch_with_redirect(
            &target,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            None,
            None,
            self.tab_id,
            self.mixed_content.as_ref(),
            destination,
            Some(&cors_ctx),
            // CORS requests are not cached (credentials/Vary complications).
            "",
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
        )
        .map(|resp| resp.body)
    }

    pub fn fetch_range(
        &self,
        url: &Url,
        range: RangeSpec,
        if_range: Option<RangeValidator>,
    ) -> Result<RangeResponse> {
        let accept_encoding = self.accept_encoding_header();
        let request = RangeRequest::Single(range);
        let resp = fetch_with_redirect(
            url,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            Some(&request),
            if_range.as_ref(),
            self.tab_id,
            self.mixed_content.as_ref(),
            None,
            None,
            // Range requests are not cached.
            "",
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
        )?;
        let content_range = if resp.status == 206 {
            header_value(&resp.headers, "content-range").and_then(parse_content_range)
        } else {
            None
        };
        Ok(RangeResponse {
            status: resp.status,
            body: resp.body,
            content_range,
        })
    }

    /// Multi-range запрос (RFC 7233 §4.1). Один request на несколько
    /// диапазонов, единый `MultiRangeResponse` обратно — независимо от
    /// того, ответил сервер `200`, `206`-single или `206`-multipart.
    ///
    /// Сервер вправе:
    /// - проигнорировать Range и вернуть `200 OK` с полным телом — мы
    ///   нормализуем в один `RangePart { body=full, content_range=None }`;
    /// - вернуть `206` с обычным `Content-Range` (например, объединил
    ///   соседние диапазоны в один) — один RangePart с распарсенным
    ///   Content-Range;
    /// - вернуть `206` с `Content-Type: multipart/byteranges; boundary=X` —
    ///   парсим body на parts через `parse_multipart_byteranges`. Если
    ///   парсинг не дал ни одного part-а (пустой ответ, кривая boundary)
    ///   — отдаём `parts=Vec::new()`, status=206 (caller сам решит, что
    ///   делать с пустым multi-range).
    /// - `416 Range Not Satisfiable` или другой 4xx/5xx — `Err`.
    ///
    /// `specs` обязан содержать хотя бы один валидный spec, иначе вернём
    /// `Err(InvalidUrl)` — нечего слать в header. Это симметрично с
    /// поведением `fetch_range` на невалидном Single, кроме точки отказа.
    pub fn fetch_multi_range(
        &self,
        url: &Url,
        specs: &[RangeSpec],
        if_range: Option<RangeValidator>,
    ) -> Result<MultiRangeResponse> {
        let request = RangeRequest::Multi(specs.to_vec());
        if request.header_value().is_none() {
            return Err(Error::Network(
                "fetch_multi_range: пустой/невалидный набор диапазонов".to_owned(),
            ));
        }
        let accept_encoding = self.accept_encoding_header();
        let resp = fetch_with_redirect(
            url,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            Some(&request),
            if_range.as_ref(),
            self.tab_id,
            self.mixed_content.as_ref(),
            None,
            None,
            // Multi-range requests are not cached.
            "",
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
        )?;
        Ok(parse_multi_range_response(resp))
    }
}

/// Нормализатор HTTP-ответа на multi-range запрос в единый
/// `MultiRangeResponse`. Изолирован от `fetch_with_redirect` для удобства
/// юнит-тестов (без поднятия mock-TcpListener).
fn parse_multi_range_response(resp: Response) -> MultiRangeResponse {
    if resp.status != 206 {
        // 200 OK или любой иной success-ответ — Range проигнорирован,
        // отдаём как один part с полным телом (caller сам поймёт, что
        // нужно нарезать клиент-сайд, если ему важны границы).
        return MultiRangeResponse {
            status: resp.status,
            parts: vec![RangePart { body: resp.body, content_range: None }],
        };
    }
    // 206 — либо single Content-Range, либо multipart/byteranges.
    let ct = header_value(&resp.headers, "content-type")
        .and_then(parse_boundary_from_content_type);
    if let Some(boundary) = ct {
        let parts = parse_multipart_byteranges(&resp.body, &boundary).unwrap_or_default();
        return MultiRangeResponse { status: resp.status, parts };
    }
    // Single Content-Range form (сервер объединил соседние диапазоны).
    let content_range = header_value(&resp.headers, "content-range").and_then(parse_content_range);
    MultiRangeResponse {
        status: resp.status,
        parts: vec![RangePart {
            body: resp.body,
            content_range,
        }],
    }
}

impl HttpClient {
    /// Загрузить подресурс с проверкой mixed-content по подключённой
    /// `MixedContentPolicy`. Если policy не подключена (`with_mixed_content_policy`
    /// не вызван) — поведение идентично `fetch(url)`: загрузка без
    /// классификации.
    ///
    /// `destination` — назначение запроса по Fetch §3.2.7 (Script / Style /
    /// Image / ...); определяет уровень mixed-content (Blockable vs
    /// OptionallyBlockable). Caller (shell, HTML parser, layout) знает
    /// destination в момент инициации запроса (из тега / property /
    /// IntersectionObserver).
    pub fn fetch_subresource(
        &self,
        url: &Url,
        destination: RequestDestination,
    ) -> Result<Vec<u8>> {
        let url_str = url.to_string();
        let accept_encoding = self.accept_encoding_header();

        // HTTP cache check (RFC 7234).
        if let Some(cache) = &self.http_cache
            && let Some(snap) = cache.get(&url_str)
        {
            if snap.is_fresh {
                return Ok(snap.body);
            }
            if !snap.conditional_headers.is_empty() {
                // Stale entry with validators — conditional GET.
                let resp = fetch_with_redirect(
                    url,
                    5,
                    &self.pool,
                    self.h2_pool.as_deref(),
                    self.resolver.as_ref(),
                    self.tls_profile,
                    self.fingerprint_profile,
                    self.sink.as_deref(),
                    self.filter.as_deref(),
                    self.hsts.as_deref(),
                    self.credentials.as_deref(),
                    &self.decoders,
                    accept_encoding.as_deref(),
                    None,
                    None,
                    self.tab_id,
                    self.mixed_content.as_ref(),
                    Some(destination),
                    None,
                    &snap.conditional_headers,
                    self.cookie_jar.as_deref(),
                    self.top_level_site.as_deref(),
                )?;
                if resp.status == 304 {
                    cache.revalidate(&url_str, &resp.headers);
                    return Ok(snap.body);
                }
                cache.store(&url_str, resp.status, resp.body.clone(), &resp.headers);
                return Ok(resp.body);
            }
        }

        let resp = fetch_with_redirect(
            url,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            None,
            None,
            self.tab_id,
            self.mixed_content.as_ref(),
            Some(destination),
            None,
            "",
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
        )?;
        if let Some(cache) = &self.http_cache {
            cache.store(&url_str, resp.status, resp.body.clone(), &resp.headers);
        }
        Ok(resp.body)
    }
}

impl NetworkTransport for HttpClient {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>> {
        // SW intercept: check before any network I/O.
        if let Some(ref interceptor) = self.interceptor {
            let origin = build_origin(url);
            if let Some(body) = interceptor.intercept(url, &origin) {
                return Ok(body);
            }
        }

        let url_str = url.to_string();
        let accept_encoding = self.accept_encoding_header();
        // Когда mixed_content policy задана (клиент работает в secure-context),
        // используем RequestDestination::Other (Blockable) как fallback —
        // чтобы enforcement сработал даже без явного destination.
        // Для top-level navigation policy не задаётся, поэтому destination
        // остаётся None и check не активируется.
        let destination = self.mixed_content.as_ref().map(|_| RequestDestination::Other);

        // HTTP cache check (RFC 7234).
        if let Some(cache) = &self.http_cache
            && let Some(snap) = cache.get(&url_str)
        {
            if snap.is_fresh {
                return Ok(snap.body);
            }
            if !snap.conditional_headers.is_empty() {
                let resp = fetch_with_redirect(
                    url,
                    5,
                    &self.pool,
                    self.h2_pool.as_deref(),
                    self.resolver.as_ref(),
                    self.tls_profile,
                    self.fingerprint_profile,
                    self.sink.as_deref(),
                    self.filter.as_deref(),
                    self.hsts.as_deref(),
                    self.credentials.as_deref(),
                    &self.decoders,
                    accept_encoding.as_deref(),
                    None,
                    None,
                    self.tab_id,
                    self.mixed_content.as_ref(),
                    destination,
                    None,
                    &snap.conditional_headers,
                    self.cookie_jar.as_deref(),
                    self.top_level_site.as_deref(),
                )?;
                if resp.status == 304 {
                    cache.revalidate(&url_str, &resp.headers);
                    return Ok(snap.body);
                }
                cache.store(&url_str, resp.status, resp.body.clone(), &resp.headers);
                return Ok(resp.body);
            }
        }

        let resp = fetch_with_redirect(
            url,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            None,
            None,
            self.tab_id,
            self.mixed_content.as_ref(),
            destination,
            None,
            "",
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
        )?;
        if let Some(cache) = &self.http_cache {
            cache.store(&url_str, resp.status, resp.body.clone(), &resp.headers);
        }
        Ok(resp.body)
    }
}

fn http_status_text(status: u16) -> &'static str {
    match status {
        100 => "Continue", 101 => "Switching Protocols", 200 => "OK",
        201 => "Created", 202 => "Accepted", 204 => "No Content",
        206 => "Partial Content", 301 => "Moved Permanently", 302 => "Found",
        303 => "See Other", 304 => "Not Modified", 307 => "Temporary Redirect",
        308 => "Permanent Redirect", 400 => "Bad Request", 401 => "Unauthorized",
        403 => "Forbidden", 404 => "Not Found", 405 => "Method Not Allowed",
        408 => "Request Timeout", 409 => "Conflict", 410 => "Gone",
        413 => "Content Too Large", 414 => "URI Too Long",
        422 => "Unprocessable Content", 429 => "Too Many Requests",
        500 => "Internal Server Error", 501 => "Not Implemented",
        502 => "Bad Gateway", 503 => "Service Unavailable",
        504 => "Gateway Timeout", _ => "",
    }
}

impl JsFetchProvider for HttpClient {
    fn fetch_sync(&self, url: &str, method: &str) -> Result<JsFetchResult> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        match method.to_ascii_uppercase().as_str() {
            "GET" | "HEAD" => {}
            m => {
                return Err(Error::Network(format!(
                    "fetch: Phase 0 supports GET/HEAD only, got {m}"
                )));
            }
        }
        // SW intercept before network.
        if let Some(ref interceptor) = self.interceptor {
            let origin = build_origin(&url);
            if let Some(body) = interceptor.intercept(&url, &origin) {
                return Ok(JsFetchResult {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![],
                    body,
                });
            }
        }
        let accept_encoding = self.accept_encoding_header();
        let destination = self.mixed_content.as_ref().map(|_| RequestDestination::Other);
        let resp = fetch_with_redirect(
            &url,
            5,
            &self.pool,
            self.h2_pool.as_deref(),
            self.resolver.as_ref(),
            self.tls_profile,
            self.fingerprint_profile,
            self.sink.as_deref(),
            self.filter.as_deref(),
            self.hsts.as_deref(),
            self.credentials.as_deref(),
            &self.decoders,
            accept_encoding.as_deref(),
            None,
            None,
            self.tab_id,
            self.mixed_content.as_ref(),
            destination,
            None,
            "",
            self.cookie_jar.as_deref(),
            self.top_level_site.as_deref(),
        )?;
        Ok(JsFetchResult {
            status_text: http_status_text(resp.status).to_string(),
            status: resp.status,
            headers: resp
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body: resp.body,
        })
    }

    fn fetch_with_body_sync(
        &self,
        url: &str,
        method: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<JsFetchResult> {
        let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
        match method.to_ascii_uppercase().as_str() {
            "POST" | "PUT" | "PATCH" | "DELETE" => {}
            m => {
                return Err(Error::Network(format!(
                    "fetch_with_body: unsupported method {m}"
                )));
            }
        }
        let (host_ascii, port, is_tls) = require_http_scheme(&url)?;
        let path_and_query = url.path_and_query();
        let key = PoolKey { host: host_ascii.clone(), port, is_tls };

        // Try pooled connection first, fall back to fresh connect.
        let mut conn = if let Some(pooled) = self.pool.acquire(&key) {
            pooled
        } else {
            connect(&host_ascii, port, is_tls, self.resolver.as_ref(), self.tls_profile)?
        };

        // HTTP/2 connections don't support the body path yet — fall back to H1.
        if conn.is_h2 {
            let fresh = connect(&host_ascii, port, is_tls, self.resolver.as_ref(), self.tls_profile)?;
            conn = fresh;
        }

        conn.write_request_with_body(method, &host_ascii, &path_and_query, content_type, body, "", self.fingerprint_profile)?;
        let resp = read_response(&mut conn)?;
        if !conn.closed {
            self.pool.release(key, conn);
        }
        Ok(JsFetchResult {
            status_text: http_status_text(resp.status).to_string(),
            status: resp.status,
            headers: resp
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body: resp.body,
        })
    }
}

impl WebSocketProvider for HttpClient {
    fn connect_ws(
        &self,
        url: &Url,
        tab_id: TabId,
        sink: Arc<dyn EventSink>,
    ) -> Result<Box<dyn WebSocketSession>> {
        let ws = websocket::WebSocket::connect(
            url,
            self.resolver.as_ref(),
            self.hsts.as_deref(),
            sink,
            tab_id,
        )?;
        Ok(Box::new(ws))
    }
}

impl SseProvider for HttpClient {
    fn connect_sse(
        &self,
        url: &Url,
        tab_id: TabId,
        sink: Arc<dyn EventSink>,
    ) -> Result<Box<dyn SseSession>> {
        let es = sse::EventSource::connect(url, Arc::clone(&self.resolver), sink, tab_id)?;
        Ok(Box::new(es))
    }
}

// ── JsWebSocketSession ────────────────────────────────────────────────────────

/// Background-threaded WebSocket session for the JS runtime.
///
/// Spawns a receive thread that pushes `JsWsEvent`s into a shared queue.
/// JS calls `poll()` to drain the queue without blocking the script thread.
struct JsWebSocketSessionImpl {
    /// For sending: shared so both this struct and (indirectly) the bg thread
    /// can access the same underlying stream.
    session: Arc<std::sync::Mutex<Box<dyn WebSocketSession>>>,
    /// Buffered events produced by the background recv thread.
    queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsWsEvent>>>,
}

impl JsWebSocketSessionImpl {
    /// Create a new session, spawning a background thread to receive frames.
    fn new(ws: websocket::WebSocket) -> Self {
        let queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsWsEvent>>> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let session: Arc<std::sync::Mutex<Box<dyn WebSocketSession>>> =
            Arc::new(std::sync::Mutex::new(Box::new(ws)));

        let q2 = Arc::clone(&queue);
        let s2 = Arc::clone(&session);

        // The background thread calls recv() in a loop and pushes events into
        // the shared queue so JS can poll without blocking.
        std::thread::spawn(move || {
            loop {
                let result = s2.lock().unwrap().recv();
                match result {
                    Ok(lumen_core::ext::WsMessage::Text(t)) => {
                        q2.lock().unwrap().push_back(JsWsEvent::Message {
                            data: t.into_bytes(),
                            is_binary: false,
                        });
                    }
                    Ok(lumen_core::ext::WsMessage::Binary(b)) => {
                        q2.lock().unwrap().push_back(JsWsEvent::Message {
                            data: b,
                            is_binary: true,
                        });
                    }
                    Ok(lumen_core::ext::WsMessage::Close { code, reason }) => {
                        q2.lock()
                            .unwrap()
                            .push_back(JsWsEvent::Close { code, reason });
                        break;
                    }
                    Ok(
                        lumen_core::ext::WsMessage::Ping(_)
                        | lumen_core::ext::WsMessage::Pong(_),
                    ) => {
                        // Control frames handled internally by WebSocket::recv_inner.
                    }
                    Err(e) => {
                        q2.lock()
                            .unwrap()
                            .push_back(JsWsEvent::Error(e.to_string()));
                        break;
                    }
                }
            }
        });

        Self { session, queue }
    }
}

impl JsWebSocketSession for JsWebSocketSessionImpl {
    fn send_text(&self, text: &str) -> Result<()> {
        self.session.lock().unwrap().send_text(text)
    }

    fn send_binary(&self, data: &[u8]) -> Result<()> {
        self.session.lock().unwrap().send_binary(data)
    }

    fn poll(&self) -> Option<JsWsEvent> {
        self.queue.lock().unwrap().pop_front()
    }

    fn close(&self, code: u16, reason: &str) -> Result<()> {
        self.session.lock().unwrap().close(code, reason)
    }
}

impl JsWebSocketProvider for HttpClient {
    fn connect(&self, url: &str) -> Result<Box<dyn JsWebSocketSession>> {
        let parsed = Url::parse(url)
            .map_err(|e| Error::Network(format!("ws: invalid URL: {e}")))?;
        let ws = websocket::WebSocket::connect(
            &parsed,
            self.resolver.as_ref(),
            self.hsts.as_deref(),
            Arc::new(NoopEventSink),
            lumen_core::event::TabId(0),
        )?;
        let impl_ = JsWebSocketSessionImpl::new(ws);
        // Push the Open event immediately — handshake already completed.
        impl_.queue.lock().unwrap().push_back(JsWsEvent::Open);
        Ok(Box::new(impl_))
    }
}

// ── JsSseSession ──────────────────────────────────────────────────────────────

/// Background-threaded SSE session for the JS runtime.
///
/// Spawns a receive thread that drains the blocking [`SseSession::next_event`]
/// loop and pushes [`JsSseEvent`]s into a shared queue. JS calls `poll()` to
/// drain the queue without blocking the script thread — mirroring
/// [`JsWebSocketSessionImpl`].
struct JsSseSessionImpl {
    /// Buffered events produced by the background recv thread.
    queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsSseEvent>>>,
    /// Set by `close()` to ask the background thread to stop reconnecting.
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl JsSseSessionImpl {
    /// Create a new session, spawning a background thread that buffers events.
    ///
    /// The thread pushes [`JsSseEvent::Open`] first, then forwards every server
    /// event until the stream ends ([`JsSseEvent::Close`]) or errors
    /// ([`JsSseEvent::Error`]). The blocking [`SseSession::next_event`] cannot be
    /// interrupted mid-call, so `close()` sets a flag the loop checks before each
    /// read; an in-flight read finishes naturally when the server closes.
    fn new(mut session: Box<dyn SseSession>) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let queue: Arc<std::sync::Mutex<std::collections::VecDeque<JsSseEvent>>> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let closed = Arc::new(AtomicBool::new(false));

        let q2 = Arc::clone(&queue);
        let c2 = Arc::clone(&closed);

        std::thread::spawn(move || {
            q2.lock().unwrap().push_back(JsSseEvent::Open);
            loop {
                if c2.load(Ordering::Relaxed) {
                    session.close();
                    break;
                }
                match session.next_event() {
                    Ok(Some(ev)) => {
                        q2.lock().unwrap().push_back(JsSseEvent::Message {
                            event_type: ev.event_type,
                            data: ev.data,
                            id: ev.id,
                        });
                    }
                    Ok(None) => {
                        q2.lock().unwrap().push_back(JsSseEvent::Close);
                        break;
                    }
                    Err(e) => {
                        q2.lock().unwrap().push_back(JsSseEvent::Error(e.to_string()));
                        break;
                    }
                }
            }
        });

        Self { queue, closed }
    }
}

impl JsSseSession for JsSseSessionImpl {
    fn poll(&self) -> Option<JsSseEvent> {
        self.queue.lock().unwrap().pop_front()
    }

    fn close(&mut self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl JsSseProvider for HttpClient {
    fn connect_sse(&self, url: &str) -> Result<Box<dyn JsSseSession>> {
        let parsed = Url::parse(url)
            .map_err(|e| Error::Network(format!("sse: invalid URL: {e}")))?;
        // Reuse the synchronous SseProvider path; the EventSource handshake runs
        // here, then the background thread takes over event delivery.
        let session = <Self as SseProvider>::connect_sse(
            self,
            &parsed,
            lumen_core::event::TabId(0),
            Arc::new(NoopEventSink),
        )?;
        Ok(Box::new(JsSseSessionImpl::new(session)))
    }
}

// ── Service Worker in-memory interceptor (для тестов) ────────────────────────

/// In-memory реализация `FetchInterceptor` для тестов без SQLite.
///
/// Хранит `origin → cache_name → url → body`. Shell подключает
/// `ServiceWorkerInterceptor` из lumen-storage; эта заглушка используется
/// в unit-тестах lumen-network, где SQLite не нужен.
pub struct InMemoryFetchInterceptor {
    // (origin, url) → body
    cache: std::sync::Mutex<std::collections::HashMap<(String, String), Vec<u8>>>,
}

impl InMemoryFetchInterceptor {
    pub fn new() -> Self {
        Self {
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Добавить запись: ответ для (origin, url) берётся из кэша без сети.
    pub fn insert(&self, origin: impl Into<String>, url: impl Into<String>, body: Vec<u8>) {
        self.cache
            .lock()
            .unwrap()
            .insert((origin.into(), url.into()), body);
    }
}

impl Default for InMemoryFetchInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

impl FetchInterceptor for InMemoryFetchInterceptor {
    fn intercept(&self, url: &Url, origin: &str) -> Option<Vec<u8>> {
        self.cache
            .lock()
            .unwrap()
            .get(&(origin.to_string(), url.as_str().to_string()))
            .cloned()
    }
}

// ── Тесты ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::{HttpAuthChallenge, HttpCredentials};

    // ── JsSseSessionImpl (HTML Living Standard §9.2) ─────────────────────────

    /// Mock `SseSession` yielding a fixed event sequence, then `Ok(None)` (close).
    struct MockSseSession {
        events: std::collections::VecDeque<lumen_core::ext::SseEvent>,
    }
    impl SseSession for MockSseSession {
        fn next_event(&mut self) -> Result<Option<lumen_core::ext::SseEvent>> {
            Ok(self.events.pop_front())
        }
        fn close(&mut self) {}
    }

    /// Drain the JS-side queue until `want` events are collected or a deadline hits.
    fn drain_js_sse(sess: &JsSseSessionImpl, want: usize) -> Vec<JsSseEvent> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut out = Vec::new();
        while out.len() < want && std::time::Instant::now() < deadline {
            if let Some(ev) = sess.poll() {
                out.push(ev);
            } else {
                std::thread::yield_now();
            }
        }
        out
    }

    #[test]
    fn js_sse_session_poll_delivers_open_message_close() {
        use lumen_core::ext::SseEvent;
        let mut events = std::collections::VecDeque::new();
        events.push_back(SseEvent {
            event_type: "message".into(),
            data: "hi".into(),
            id: Some("7".into()),
            retry_ms: None,
        });
        let session: Box<dyn SseSession> = Box::new(MockSseSession { events });
        let impl_ = JsSseSessionImpl::new(session);
        // Expect: Open, Message{hi,7}, Close.
        let evs = drain_js_sse(&impl_, 3);
        assert_eq!(evs.len(), 3, "got {evs:?}");
        assert_eq!(evs[0], JsSseEvent::Open);
        assert_eq!(
            evs[1],
            JsSseEvent::Message {
                event_type: "message".into(),
                data: "hi".into(),
                id: Some("7".into()),
            }
        );
        assert_eq!(evs[2], JsSseEvent::Close);
    }

    // ── ALPN (5A.1) ──────────────────────────────────────────────────────────

    #[test]
    fn standard_tls_config_advertises_h2_then_http11() {
        // Server должен выбрать h2 (если умеет); fallback — http/1.1.
        // Порядок ALPN-protocols в ClientHello — клиентское предпочтение.
        let cfg = tls_config_for_profile(tls::TlsProfile::Standard);
        assert_eq!(
            cfg.alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        );
    }

    #[test]
    fn tls_config_for_profile_is_cached() {
        // Та же Arc должна возвращаться при повторных вызовах — иначе webpki-roots
        // парсится на каждый connect (порядка сотни сертификатов).
        let a = tls_config_for_profile(tls::TlsProfile::Standard);
        let b = tls_config_for_profile(tls::TlsProfile::Standard);
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn check_alpn_accepts_http11() {
        assert!(!check_negotiated_alpn(Some(b"http/1.1")).unwrap());
    }

    #[test]
    fn check_alpn_accepts_no_alpn() {
        assert!(!check_negotiated_alpn(None).unwrap());
    }

    #[test]
    fn check_alpn_accepts_h2() {
        assert!(check_negotiated_alpn(Some(b"h2")).unwrap());
    }

    #[test]
    fn check_alpn_rejects_unknown_proto() {
        let err = check_negotiated_alpn(Some(b"h3")).unwrap_err();
        assert!(format!("{err:?}").contains("unexpected ALPN"));
    }

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

    // ── H2Pool (5A.5) ────────────────────────────────────────────────────────

    #[test]
    fn http_client_with_h2_pool_builder() {
        // with_h2_pool() подключает пул без паники; обычные HTTP/1.1 запросы
        // не затрагиваются (pool просто не выдаёт entries для новых origin-ов).
        let pool = Arc::new(H2Pool::new());
        let client = HttpClient::new().with_h2_pool(pool.clone());
        // Пул пока пустой — acquire вернёт None, client уйдёт в обычный connect.
        // Без реального HTTP/2 сервера мы только проверяем, что API компилируется.
        let _ = client;
    }

    #[test]
    fn h2_pool_shared_between_clients() {
        // Один Arc<H2Pool> можно подключить к нескольким клиентам (как ConnectionPool).
        let pool = Arc::new(H2Pool::new());
        let _c1 = HttpClient::new().with_h2_pool(Arc::clone(&pool));
        let _c2 = HttpClient::new().with_h2_pool(Arc::clone(&pool));
        // Обе Arc-и ссылаются на одну структуру.
        assert_eq!(Arc::strong_count(&pool), 3);
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

    // ── Custom DnsResolver ───────────────────────────────────────────────────

    use std::collections::HashMap;
    use std::net::SocketAddr;

    /// Тестовый resolver: маппит hostname → фиксированный SocketAddr (с
    /// подменённым port на тот, что просит fetch). Используется, чтобы
    /// доказать: подменённый resolver реально применяется в connect-path —
    /// fetch к произвольному hostname типа "synthetic.test" приходит на
    /// loopback-listener.
    struct MockResolver {
        map: Mutex<HashMap<String, std::net::IpAddr>>,
        calls: AtomicUsize,
    }

    impl MockResolver {
        fn with(host: &str, ip: std::net::IpAddr) -> Self {
            let mut m = HashMap::new();
            m.insert(host.to_ascii_lowercase(), ip);
            Self {
                map: Mutex::new(m),
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl DnsResolver for MockResolver {
        fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let key = hostname.to_ascii_lowercase();
            let map = self.map.lock().unwrap();
            match map.get(&key) {
                Some(ip) => Ok(vec![SocketAddr::new(*ip, port)]),
                None => Err(Error::Network(format!("mock NXDOMAIN: {hostname}"))),
            }
        }
    }

    #[test]
    fn http_client_uses_custom_resolver_for_synthetic_hostname() {
        // Mock listener слушает на 127.0.0.1:<port>; HttpClient просят
        // fetch к URL с произвольным hostname "synthetic.test". Если resolver
        // не подменился — SystemDnsResolver не разрешит "synthetic.test" в
        // loopback, и fetch упадёт. Доказательство того, что with_dns_resolver
        // реально применяется в fetch-path.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nresolved!".to_vec()
        });

        let resolver = Arc::new(MockResolver::with(
            "synthetic.test",
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        ));
        let client = HttpClient::new().with_dns_resolver(resolver.clone());
        let url = Url::parse(&format!("http://synthetic.test:{port}/")).unwrap();

        let body = client.fetch(&url).expect("fetch through mock resolver");
        assert_eq!(body, b"resolved!");
        assert_eq!(resolver.call_count(), 1, "resolver вызван ровно один раз");

        server.join().unwrap();
    }

    #[test]
    fn http_client_resolver_err_propagates_as_fetch_err() {
        // Resolver отдаёт Err — fetch не должен звать TCP connect, должен
        // вернуть ту же ошибку как Network. RequestStarted эмитится
        // (URL валидный), но никакого Completed — resolver Err приходит
        // до сокета.
        let resolver = Arc::new(MockResolver::with(
            "known.host",
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        ));
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_dns_resolver(resolver.clone())
            .with_sink(sink.clone());
        let url = Url::parse("http://unknown.host/").unwrap();

        let err = client.fetch(&url).expect_err("resolver Err must bubble up");
        assert!(
            format!("{err:?}").contains("mock NXDOMAIN"),
            "expected NXDOMAIN reason, got {err:?}"
        );
        assert_eq!(resolver.call_count(), 1);

        let events = sink.events();
        assert!(
            matches!(events.first(), Some(Event::RequestStarted { .. })),
            "RequestStarted эмитится до resolver: {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, Event::RequestCompleted { .. })),
            "RequestCompleted не должен прозвучать — connect не состоялся: {events:?}"
        );
        // Инвариант «Started без Completed = failure»: терминальное событие —
        // RequestFailed со стадией Dns (resolver вернул ошибку).
        match events.last() {
            Some(Event::RequestFailed { stage, reason, .. }) => {
                assert_eq!(*stage, RequestStage::Dns, "DNS-сбой → стадия Dns");
                assert!(reason.contains("mock NXDOMAIN"), "reason несёт текст ошибки: {reason}");
            }
            other => panic!("expected RequestFailed(Dns) terminal event, got {other:?}"),
        }
    }

    #[test]
    fn classify_failure_stage_maps_message_prefixes() {
        // Сообщения Error::Network имеют стабильные префиксы по точке отказа —
        // classify_failure_stage относит каждый к своей стадии.
        assert_eq!(
            classify_failure_stage("resolve example.com:443: no addresses"),
            RequestStage::Dns
        );
        assert_eq!(
            classify_failure_stage("resolve host:80: mock NXDOMAIN"),
            RequestStage::Dns
        );
        assert_eq!(
            classify_failure_stage("connect 127.0.0.1:9: Connection refused (os error 111)"),
            RequestStage::Tcp
        );
        assert_eq!(
            classify_failure_stage("TLS handshake: invalid peer certificate"),
            RequestStage::Tls
        );
        assert_eq!(
            classify_failure_stage("invalid hostname '::1': not a valid DNS name"),
            RequestStage::Tls
        );
        assert_eq!(
            classify_failure_stage("unexpected ALPN protocol: \"spdy\""),
            RequestStage::Tls
        );
        // Всё, что не относится к connect-фазе, — обмен данными (Read).
        assert_eq!(classify_failure_stage("read status: UnexpectedEof"), RequestStage::Read);
        assert_eq!(classify_failure_stage("EOF before status line"), RequestStage::Read);
        assert_eq!(classify_failure_stage("chunked size: invalid digit"), RequestStage::Read);
        assert_eq!(classify_failure_stage("write request: BrokenPipe"), RequestStage::Read);
    }

    #[test]
    fn request_stage_as_str_tags() {
        assert_eq!(RequestStage::Dns.as_str(), "dns");
        assert_eq!(RequestStage::Tcp.as_str(), "tcp");
        assert_eq!(RequestStage::Tls.as_str(), "tls");
        assert_eq!(RequestStage::Read.as_str(), "read");
    }

    #[test]
    fn fetch_connection_refused_emits_request_failed_tcp() {
        // Bind→port→drop освобождает порт, на котором никто не слушает: connect
        // получает refused. Терминальное событие — RequestFailed(Tcp), а не
        // зависший RequestStarted.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new().with_sink(sink.clone()).with_tab(TabId(3));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();

        assert!(client.fetch(&url).is_err());

        let events = sink.events();
        assert!(matches!(events.first(), Some(Event::RequestStarted { .. })));
        match events.last() {
            Some(Event::RequestFailed { tab_id, stage, .. }) => {
                assert_eq!(*tab_id, TabId(3));
                assert_eq!(*stage, RequestStage::Tcp, "refused connect → стадия Tcp");
            }
            other => panic!("expected RequestFailed(Tcp), got {other:?}"),
        }
        assert!(
            !events.iter().any(|e| matches!(e, Event::RequestCompleted { .. })),
            "сбой connect не должен давать RequestCompleted"
        );
    }

    #[test]
    fn http_client_resolver_called_per_redirect_hop() {
        // 302 redirect на другой hostname → resolver должен вызваться дважды,
        // по одному на hop. Это инвариант симметричный с тем, как обрабатываются
        // sink-события и filter-проверки (per-hop).
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for i in 1..=2u32 {
                let (mut sock, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    let n = reader.read_line(&mut line).unwrap_or(0);
                    if n == 0 || line == "\r\n" || line == "\n" {
                        break;
                    }
                }
                let body: Vec<u8> = if i == 1 {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://hop-two.test:{port}/done\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .into_bytes()
                } else {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone"
                        .to_vec()
                };
                let _ = sock.write_all(&body);
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });

        let mut map = HashMap::new();
        map.insert(
            "hop-one.test".to_owned(),
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        );
        map.insert(
            "hop-two.test".to_owned(),
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        );
        let resolver = Arc::new(MockResolver {
            map: Mutex::new(map),
            calls: AtomicUsize::new(0),
        });
        let client = HttpClient::new().with_dns_resolver(resolver.clone());
        let url = Url::parse(&format!("http://hop-one.test:{port}/start")).unwrap();

        assert_eq!(client.fetch(&url).unwrap(), b"done");
        assert_eq!(resolver.call_count(), 2, "resolver вызван per hop");

        server.join().unwrap();
    }

    // ── HSTS integration ─────────────────────────────────────────────────────

    use lumen_core::ext::HstsEnforcement;

    /// In-memory HSTS-impl для integration-тестов — не требует SQLite.
    /// Семантика exact-match (без includeSubDomains-логики) — достаточно
    /// для проверки fetch-pathway; полное subdomain-поведение покрыто
    /// unit-тестами в src/hsts.rs.
    struct InMemHsts {
        hosts: Mutex<Vec<String>>,
    }

    impl InMemHsts {
        fn new() -> Self {
            Self {
                hosts: Mutex::new(Vec::new()),
            }
        }

        fn add(&self, host: &str) {
            self.hosts.lock().unwrap().push(host.to_owned());
        }
    }

    impl HstsEnforcement for InMemHsts {
        fn is_https_only(&self, host: &str, _now_unix: i64) -> bool {
            self.hosts.lock().unwrap().iter().any(|h| h == host)
        }

        fn record_sts(
            &self,
            host: &str,
            _max_age: u64,
            _include_subdomains: bool,
            _preload: bool,
            _now_unix: i64,
        ) {
            self.hosts.lock().unwrap().push(host.to_owned());
        }
    }

    #[test]
    fn without_hsts_http_stays_http() {
        // Sanity-check: HttpClient без with_hsts ведёт себя как раньше —
        // http URL не upgrade-ится, обычный fetch проходит. Регрессионный
        // тест: интеграция HSTS не должна сломать дефолтный поток.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        server.join().unwrap();
    }

    #[test]
    fn with_hsts_unknown_host_no_upgrade() {
        // HSTS-store подключён, но в нём нет нашего host-а → upgrade
        // не применяется, fetch идёт по http как обычно. Это инвариант:
        // unknown hosts остаются http (HSTS — opt-in, не блок-лист).
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nplain".to_vec()
        });

        let hsts: Arc<dyn HstsEnforcement> = Arc::new(InMemHsts::new());
        let client = HttpClient::new().with_hsts(hsts);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"plain");

        server.join().unwrap();
    }

    #[test]
    fn with_hsts_known_host_attempts_upgrade() {
        // HSTS-known host → клиент upgrade-ит на https://. Mock-сервер
        // слушает HTTP (без TLS), поэтому upgrade-attempt падает на TLS
        // handshake — это доказывает, что upgrade действительно произошёл.
        // Иначе на mock HTTP-сервере мы бы получили 200 OK, а не error.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        // Сервер просто принимает соединения и закрывает — нам важна сама
        // попытка TLS handshake клиента в момент, когда он считает, что
        // открыл TCP к HTTPS-серверу.
        let _server = thread::spawn(move || {
            for _ in 0..3 {
                if let Ok((sock, _)) = listener.accept() {
                    let _ = sock.shutdown(std::net::Shutdown::Both);
                }
            }
        });

        // Подменяем resolver, чтобы synthetic "upgrade.test" разрешался
        // в loopback (system DNS не знает таких имён). Punycode для не-IDN
        // host = сам host.
        let resolver = Arc::new(MockResolver::with(
            "upgrade.test",
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        ));
        let inmem = Arc::new(InMemHsts::new());
        inmem.add("upgrade.test");
        let hsts: Arc<dyn HstsEnforcement> = inmem;

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_dns_resolver(resolver)
            .with_hsts(hsts)
            .with_sink(sink.clone());

        // Делаем fetch по http — HSTS должен переписать на https и упасть
        // на TLS handshake против plain-HTTP сервера.
        let url = Url::parse(&format!("http://upgrade.test:{port}/")).unwrap();
        let err = client.fetch(&url).expect_err("upgrade-attempt must fail TLS");
        let msg = format!("{err:?}");
        // Конкретный текст ошибки rustls — не гарантия, но содержит TLS-связанные
        // токены: "tls" / "handshake" / "InvalidContentType" / similar. Главное —
        // запрос не дошёл до status-line на HTTP-сервере (иначе reason был бы 200).
        assert!(
            !msg.contains("HTTP 200"),
            "upgrade must redirect to https → TLS error, not 200 OK on plain port; got: {msg}"
        );

        // RequestStarted должен эмититься с UPGRADED URL — это важно для
        // network log: пользователь видит реальный URL, по которому пошёл трафик.
        let events = sink.events();
        let started_url = events.iter().find_map(|e| match e {
            Event::RequestStarted { url, .. } => Some(url.as_str().to_owned()),
            _ => None,
        });
        assert_eq!(
            started_url.as_deref(),
            Some(format!("https://upgrade.test:{port}/").as_str()),
            "RequestStarted должен содержать upgraded URL: {events:?}"
        );
    }

    #[test]
    fn with_hsts_https_url_stays_https() {
        // https URL не должен трогаться HSTS-интеграцией (нечего upgrade-ить).
        // Здесь мы не делаем реальный HTTPS-fetch (нет TLS mock), а проверяем
        // что builder/policy applied correctly через ту же async-resolver
        // pathway: fetch к https://known-host даёт TLS-ошибку, и в Started
        // URL должен остаться https (НЕ повторно upgrade-нутый или какой-то
        // ещё).
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let _server = thread::spawn(move || {
            for _ in 0..3 {
                if let Ok((sock, _)) = listener.accept() {
                    let _ = sock.shutdown(std::net::Shutdown::Both);
                }
            }
        });

        let resolver = Arc::new(MockResolver::with(
            "secure.test",
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        ));
        let inmem = Arc::new(InMemHsts::new());
        inmem.add("secure.test");
        let hsts: Arc<dyn HstsEnforcement> = inmem;

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_dns_resolver(resolver)
            .with_hsts(hsts)
            .with_sink(sink.clone());

        let url = Url::parse(&format!("https://secure.test:{port}/")).unwrap();
        let _ = client.fetch(&url); // ожидаем TLS error — нам важен только Started URL.

        let events = sink.events();
        let started_url = events.iter().find_map(|e| match e {
            Event::RequestStarted { url, .. } => Some(url.as_str().to_owned()),
            _ => None,
        });
        assert_eq!(
            started_url.as_deref(),
            Some(format!("https://secure.test:{port}/").as_str()),
            "https URL не должен трогаться upgrade-логикой: {events:?}"
        );
    }

    // ── HTTP Range requests ─────────────────────────────────────────────────

    /// Mock-сервер, проверяющий Range header в запросе и отдающий
    /// 206 Partial Content для honored range или 200 OK для full body.
    /// `expected_range` — точное ожидаемое значение Range header (без префикса
    /// `Range: `). Если None — Range header не должен присутствовать.
    fn mock_range_server(
        responder: impl Fn(Option<String>) -> Vec<u8> + Send + 'static,
    ) -> (u16, thread::JoinHandle<()>) {
        mock_range_server_full(move |range, _if_range| responder(range))
    }

    /// Расширенный mock — отдаёт responder-у и `Range:`, и `If-Range:` header-ы.
    /// Нужен для If-Range conditional тестов: проверяем оба header-а в одном
    /// сценарии без второго мока.
    fn mock_range_server_full(
        responder: impl Fn(Option<String>, Option<String>) -> Vec<u8> + Send + 'static,
    ) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut range_header: Option<String> = None;
            let mut if_range_header: Option<String> = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    break;
                }
                if let Some(v) = trimmed.strip_prefix("Range: ") {
                    range_header = Some(v.to_owned());
                }
                if let Some(v) = trimmed.strip_prefix("If-Range: ") {
                    if_range_header = Some(v.to_owned());
                }
            }
            let body = responder(range_header, if_range_header);
            let _ = sock.write_all(&body);
            let _ = sock.shutdown(std::net::Shutdown::Both);
        });
        (port, handle)
    }

    #[test]
    fn fetch_range_206_returns_partial_with_content_range() {
        // Сервер видит Range: bytes=0-4, отвечает 206 с заголовком
        // Content-Range и пятью байтами. RangeResponse.content_range
        // должен быть распарсен; body — точно 5 байт; status = 206.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4"));
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/100\r\nConnection: close\r\n\r\nhello"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::closed(0, 4), None).unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body, b"hello");
        assert_eq!(
            resp.content_range,
            Some(ContentRange { start: 0, end: 4, total: Some(100) })
        );

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_open_ended_sends_correct_header() {
        // bytes=500- (от 500 до конца) — сервер возвращает суффикс с
        // unknown-total (`/*`), что валидно для chunked-source.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=500-"));
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 500-503/*\r\nConnection: close\r\n\r\ntail"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::from(500), None).unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body, b"tail");
        assert_eq!(
            resp.content_range,
            Some(ContentRange { start: 500, end: 503, total: None })
        );

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_200_fallback_when_server_ignores_range() {
        // RFC 7233 §3.1: сервер вправе ответить 200 с full body на Range-запрос.
        // Клиент должен принять — body = full, content_range = None, status=200.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=0-9"));
            b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::closed(0, 9), None).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello world");
        assert!(resp.content_range.is_none());

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_416_not_satisfiable_returns_err() {
        // Сервер ответил 416 — fetch_range возвращает Err. По текущему API
        // мы не различаем 416 от других 4xx; caller проверяет текст ошибки
        // или просто отбрасывает попытку.
        let (port, server) = mock_range_server(|_| {
            b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */100\r\nConnection: close\r\n\r\n"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let err = client.fetch_range(&url, RangeSpec::closed(1000, 2000), None).unwrap_err();
        assert!(format!("{err:?}").contains("416"), "expected HTTP 416, got: {err:?}");

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_206_without_content_range_header() {
        // Дефектный сервер отдал 206, но без Content-Range. Не падаем —
        // body отдаём как есть, content_range = None. Caller сам решает,
        // считать ли такой ответ валидным.
        let (port, server) = mock_range_server(|_| {
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::closed(0, 2), None).unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body, b"abc");
        assert!(resp.content_range.is_none());

        server.join().unwrap();
    }

    #[test]
    fn fetch_without_range_does_not_send_range_header() {
        // Регрессия: обычный client.fetch() не должен слать Range header
        // (он опциональный). Mock проверяет, что range_header остался None.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range, None, "fetch() must not send Range header");
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_invalid_spec_silently_omits_header() {
        // RangeSpec.closed(100, 50): end < start — header_value возвращает
        // None, write_request не вставляет Range header. Сервер видит как
        // обычный GET, отдаёт 200 OK — fetch_range вернёт full body c
        // content_range = None (по сути fallback на полный fetch).
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range, None, "invalid range spec must omit header");
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfull".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::closed(100, 50), None).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"full");
        assert!(resp.content_range.is_none());

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_suffix_sends_bytes_dash_n() {
        // bytes=-N — последние N байт. RFC 7233 §2.1 «suffix-byte-range-spec».
        // Mock проверяет точный header и отвечает 206 с Content-Range,
        // указывающим какие именно байты были возвращены (`start=total-N`).
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=-10"));
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 10\r\nContent-Range: bytes 90-99/100\r\nConnection: close\r\n\r\nlast 10byt"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::suffix(10), None).unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body, b"last 10byt");
        assert_eq!(
            resp.content_range,
            Some(ContentRange { start: 90, end: 99, total: Some(100) })
        );

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_suffix_zero_omits_header() {
        // RangeSpec::suffix(0) — невалидно (RFC §2.1: suffix-length > 0);
        // header_value() возвращает None, fetch ходит без Range, ответ
        // приходит как обычный 200.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range, None, "suffix=0 must omit Range header");
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfull".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client.fetch_range(&url, RangeSpec::suffix(0), None).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"full");

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_if_range_etag_match_returns_206() {
        // If-Range ETag совпал — server отдаёт 206 с запрошенным range.
        // Mock проверяет, что и Range, и If-Range отправлены с правильными
        // значениями.
        let (port, server) = mock_range_server_full(|range, if_range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4"));
            assert_eq!(if_range.as_deref(), Some("\"v1\""));
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/10\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nhello"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_range(
                &url,
                RangeSpec::closed(0, 4),
                Some(RangeValidator::ETag("\"v1\"".to_owned())),
            )
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body, b"hello");
        assert!(resp.content_range.is_some());

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_if_range_etag_mismatch_returns_200_full_body() {
        // If-Range ETag НЕ совпал — server по RFC 7233 §3.2 должен отдать 200
        // с полным новым телом (диапазон проигнорирован, потому что ресурс
        // изменился). Клиент принимает: status=200, content_range=None,
        // body = full new resource.
        let (port, server) = mock_range_server_full(|range, if_range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4"));
            assert_eq!(if_range.as_deref(), Some("\"v1\""));
            // Server: ETag теперь "v2" → mismatch → 200 + full body.
            b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"v2\"\r\nConnection: close\r\n\r\nhello world"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_range(
                &url,
                RangeSpec::closed(0, 4),
                Some(RangeValidator::ETag("\"v1\"".to_owned())),
            )
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello world");
        assert!(resp.content_range.is_none());

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_if_range_last_modified_sent_verbatim() {
        // LastModified validator: header передаётся дословно (включая запятые,
        // пробелы и GMT). RFC 7233 §3.2 не требует трансформации.
        let date = "Tue, 15 Nov 1994 12:45:26 GMT";
        let date_owned = date.to_owned();
        let (port, server) = mock_range_server_full(move |range, if_range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4"));
            assert_eq!(if_range.as_deref(), Some(date_owned.as_str()));
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/10\r\nConnection: close\r\n\r\nhello"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_range(
                &url,
                RangeSpec::closed(0, 4),
                Some(RangeValidator::LastModified(date.to_owned())),
            )
            .unwrap();
        assert_eq!(resp.status, 206);

        server.join().unwrap();
    }

    #[test]
    fn fetch_range_if_range_omitted_when_range_invalid() {
        // If-Range без валидного Range не имеет смысла (RFC §3.2 «sent with a
        // Range header field»). Проверяем регрессию: invalid range (end < start
        // → header_value=None) приводит к тому, что и If-Range header не
        // попадает в запрос.
        let (port, server) = mock_range_server_full(|range, if_range| {
            assert_eq!(range, None, "invalid range omits Range");
            assert_eq!(if_range, None, "If-Range omitted without Range");
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let _ = client
            .fetch_range(
                &url,
                RangeSpec::closed(100, 50),
                Some(RangeValidator::ETag("\"v1\"".to_owned())),
            )
            .unwrap();

        server.join().unwrap();
    }

    // ── Multi-range / multipart/byteranges ──────────────────────────────────

    #[test]
    fn fetch_multi_range_206_multipart_two_parts() {
        // Сервер видит `Range: bytes=0-4,10-14`, отвечает 206 с
        // multipart/byteranges и двумя parts. fetch_multi_range нормализует
        // это в MultiRangeResponse с двумя RangePart-ами; status=206.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4,10-14"));
            let body = b"--BNDRY\r\n\
Content-Type: application/octet-stream\r\n\
Content-Range: bytes 0-4/100\r\n\r\n\
hello\r\n\
--BNDRY\r\n\
Content-Type: application/octet-stream\r\n\
Content-Range: bytes 10-14/100\r\n\r\n\
world\r\n\
--BNDRY--\r\n";
            let mut resp = Vec::new();
            resp.extend_from_slice(b"HTTP/1.1 206 Partial Content\r\n");
            resp.extend_from_slice(b"Content-Type: multipart/byteranges; boundary=BNDRY\r\n");
            resp.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            resp.extend_from_slice(b"Connection: close\r\n\r\n");
            resp.extend_from_slice(body);
            resp
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_multi_range(
                &url,
                &[RangeSpec::closed(0, 4), RangeSpec::closed(10, 14)],
                None,
            )
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.parts.len(), 2);
        assert_eq!(resp.parts[0].body, b"hello");
        assert_eq!(
            resp.parts[0].content_range,
            Some(ContentRange { start: 0, end: 4, total: Some(100) })
        );
        assert_eq!(resp.parts[1].body, b"world");
        assert_eq!(
            resp.parts[1].content_range,
            Some(ContentRange { start: 10, end: 14, total: Some(100) })
        );
        server.join().unwrap();
    }

    #[test]
    fn fetch_multi_range_206_single_content_range_form() {
        // RFC 7233 §4.1: сервер вправе объединить пересекающиеся
        // диапазоны и ответить обычным 206 с одним Content-Range, без
        // multipart. fetch_multi_range трактует как один RangePart с
        // распарсенным Content-Range.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4,3-9"));
            b"HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Length: 10\r\nContent-Range: bytes 0-9/100\r\nConnection: close\r\n\r\nhelloworld".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_multi_range(
                &url,
                &[RangeSpec::closed(0, 4), RangeSpec::closed(3, 9)],
                None,
            )
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.parts.len(), 1);
        assert_eq!(resp.parts[0].body, b"helloworld");
        assert_eq!(
            resp.parts[0].content_range,
            Some(ContentRange { start: 0, end: 9, total: Some(100) })
        );
        server.join().unwrap();
    }

    #[test]
    fn fetch_multi_range_200_fallback_when_server_ignores_range() {
        // Сервер проигнорировал Range — 200 OK с полным телом.
        // fetch_multi_range вернёт один RangePart с content_range=None.
        let (port, server) = mock_range_server(|range| {
            assert!(range.is_some(), "Range header должен быть отправлен");
            b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhelloworld!"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_multi_range(
                &url,
                &[RangeSpec::closed(0, 4), RangeSpec::closed(10, 14)],
                None,
            )
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.parts.len(), 1);
        assert_eq!(resp.parts[0].body, b"helloworld!");
        assert!(resp.parts[0].content_range.is_none());
        server.join().unwrap();
    }

    #[test]
    fn fetch_multi_range_416_returns_err() {
        // Запрошенные диапазоны вне ресурса — 416 Range Not Satisfiable.
        let (port, server) = mock_range_server(|_| {
            b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */100\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let err = client
            .fetch_multi_range(
                &url,
                &[RangeSpec::closed(1000, 2000), RangeSpec::closed(3000, 4000)],
                None,
            )
            .unwrap_err();
        assert!(matches!(err, Error::Network(_)));
        server.join().unwrap();
    }

    #[test]
    fn fetch_multi_range_empty_specs_returns_err_before_socket() {
        // Pre-condition: пустой vec невозможно сериализовать в header.
        // Возврат Err до открытия сокета — никакого TCP-трафика.
        let client = HttpClient::new();
        let url = Url::parse("http://127.0.0.1:1/").unwrap();
        let err = client.fetch_multi_range(&url, &[], None).unwrap_err();
        assert!(matches!(err, Error::Network(_)));
    }

    #[test]
    fn fetch_multi_range_all_invalid_specs_returns_err_before_socket() {
        // Все spec-ы невалидны → header_value=None → Err без сети.
        let client = HttpClient::new();
        let url = Url::parse("http://127.0.0.1:1/").unwrap();
        let err = client
            .fetch_multi_range(
                &url,
                &[RangeSpec::closed(100, 50), RangeSpec::suffix(0)],
                None,
            )
            .unwrap_err();
        assert!(matches!(err, Error::Network(_)));
    }

    #[test]
    fn fetch_multi_range_mixed_valid_invalid_specs_sends_only_valid() {
        // Невалидные spec-ы внутри Multi молча отбрасываются (см.
        // RangeRequest::header_value semantics). Сервер видит только
        // валидные диапазоны.
        let (port, server) = mock_range_server(|range| {
            assert_eq!(range.as_deref(), Some("bytes=0-4,200-299"));
            let body = b"--Z\r\nContent-Range: bytes 0-4/500\r\n\r\nhello\r\n--Z\r\nContent-Range: bytes 200-299/500\r\n\r\nbody\r\n--Z--\r\n";
            let mut resp = Vec::new();
            resp.extend_from_slice(b"HTTP/1.1 206 Partial Content\r\n");
            resp.extend_from_slice(b"Content-Type: multipart/byteranges; boundary=Z\r\n");
            resp.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            resp.extend_from_slice(b"Connection: close\r\n\r\n");
            resp.extend_from_slice(body);
            resp
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_multi_range(
                &url,
                &[
                    RangeSpec::closed(0, 4),
                    RangeSpec::closed(100, 50),
                    RangeSpec::suffix(0),
                    RangeSpec::closed(200, 299),
                ],
                None,
            )
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.parts.len(), 2);
        server.join().unwrap();
    }

    #[test]
    fn fetch_multi_range_206_multipart_quoted_boundary() {
        // Boundary в Content-Type — quoted-string. parse_boundary_from_content_type
        // должен корректно его распаковать.
        let (port, server) = mock_range_server(|_| {
            let body = b"--has space\r\nContent-Range: bytes 0-2/10\r\n\r\nabc\r\n--has space--\r\n";
            let mut resp = Vec::new();
            resp.extend_from_slice(b"HTTP/1.1 206 Partial Content\r\n");
            resp.extend_from_slice(b"Content-Type: multipart/byteranges; boundary=\"has space\"\r\n");
            resp.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            resp.extend_from_slice(b"Connection: close\r\n\r\n");
            resp.extend_from_slice(body);
            resp
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let resp = client
            .fetch_multi_range(&url, &[RangeSpec::closed(0, 2)], None)
            .unwrap();
        assert_eq!(resp.parts.len(), 1);
        assert_eq!(resp.parts[0].body, b"abc");
        server.join().unwrap();
    }

    // ── HTTP auth (Basic + Digest) ───────────────────────────────────────────

    /// Mock-сервер для auth-сценариев: каждое соединение получает request
    /// (полностью), сохраняет его в shared Vec и отвечает тем, что вернёт
    /// `responder(request_index, request_text)`. Это и заменяет
    /// «expectation matcher» из крупных testing-фреймворков — тест после
    /// `client.fetch` читает captured requests и assert-ит на Authorization.
    fn mock_auth_server<F>(
        accept_count: usize,
        captured: Arc<Mutex<Vec<String>>>,
        responder: F,
    ) -> (u16, thread::JoinHandle<()>)
    where
        F: Fn(usize, &str) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for i in 1..=accept_count {
                let (mut sock, _) = match listener.accept() {
                    Ok(p) => p,
                    Err(_) => break,
                };
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                let mut req_text = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let is_terminator = line == "\r\n" || line == "\n";
                    req_text.push_str(&line);
                    if is_terminator {
                        break;
                    }
                }
                captured.lock().unwrap().push(req_text.clone());
                let body = responder(i, &req_text);
                let _ = sock.write_all(&body);
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });
        (port, handle)
    }

    fn extract_authorization(req: &str) -> Option<String> {
        for line in req.lines() {
            if let Some((k, v)) = line.split_once(':')
                && k.trim().eq_ignore_ascii_case("authorization")
            {
                return Some(v.trim().to_string());
            }
        }
        None
    }

    #[test]
    fn auth_basic_401_then_200_with_authorization_on_retry() {
        // 1-й запрос — без Authorization → 401 + WWW-Authenticate Basic.
        // 2-й запрос — с Authorization: Basic ... → 200 OK.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(2, captured_cl, |i, _req| match i {
            1 => b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"WallyWorld\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload".to_vec(),
            _ => unreachable!(),
        });

        let provider = Arc::new(
            StaticCredentialProvider::new().with(
                &format!("http://127.0.0.1:{port}"),
                "WallyWorld",
                "Aladdin",
                "open sesame",
            ),
        );
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_credentials(provider)
            .with_sink(sink.clone());
        let url = Url::parse(&format!("http://127.0.0.1:{port}/secret")).unwrap();
        let body = client.fetch(&url).expect("fetch should succeed after retry");
        assert_eq!(body, b"payload");

        let requests = captured.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            extract_authorization(&requests[0]).is_none(),
            "first request must be sent without Authorization"
        );
        let auth_header = extract_authorization(&requests[1]).expect("second request needs Authorization");
        assert_eq!(auth_header, "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");

        // Events: Started, Completed(401), Started, Completed(200).
        let events = sink.events();
        assert_eq!(events.len(), 4, "expected 4 events for retry, got {events:?}");
        assert!(matches!(events[1], Event::RequestCompleted { status: 401, .. }));
        assert!(matches!(events[3], Event::RequestCompleted { status: 200, .. }));

        server.join().unwrap();
    }

    #[test]
    fn auth_digest_md5_401_then_200_response_is_md5() {
        // Сервер просит Digest MD5 — клиент должен вернуть Authorization:
        // Digest username, realm, nonce, uri, response, qop, nc, cnonce.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(2, captured_cl, |i, _req| match i {
            1 => b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"r\", qop=\"auth\", nonce=\"N\", algorithm=MD5\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
            _ => unreachable!(),
        });

        let provider = Arc::new(
            StaticCredentialProvider::new().with(
                &format!("http://127.0.0.1:{port}"),
                "r",
                "u",
                "p",
            ),
        );
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/path")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        let requests = captured.lock().unwrap().clone();
        let auth = extract_authorization(&requests[1]).expect("Authorization on retry");
        assert!(auth.starts_with("Digest "));
        assert!(auth.contains("username=\"u\""));
        assert!(auth.contains("realm=\"r\""));
        assert!(auth.contains("nonce=\"N\""));
        assert!(auth.contains("uri=\"/path\""));
        assert!(auth.contains("qop=auth"));
        assert!(auth.contains("algorithm=MD5"));
        // response — 32 hex digits (MD5).
        let resp_idx = auth.find("response=\"").unwrap() + "response=\"".len();
        let resp_end = auth[resp_idx..].find('"').unwrap() + resp_idx;
        assert_eq!(resp_end - resp_idx, 32);

        server.join().unwrap();
    }

    #[test]
    fn auth_digest_sha256_response_is_64_hex() {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(2, captured_cl, |i, _req| match i {
            1 => b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"r\", qop=\"auth\", nonce=\"N\", algorithm=SHA-256\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
            _ => unreachable!(),
        });

        let provider = Arc::new(
            StaticCredentialProvider::new().with(
                &format!("http://127.0.0.1:{port}"),
                "r",
                "u",
                "p",
            ),
        );
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        let requests = captured.lock().unwrap().clone();
        let auth = extract_authorization(&requests[1]).expect("Authorization on retry");
        assert!(auth.contains("algorithm=SHA-256"));
        let resp_idx = auth.find("response=\"").unwrap() + "response=\"".len();
        let resp_end = auth[resp_idx..].find('"').unwrap() + resp_idx;
        assert_eq!(resp_end - resp_idx, 64, "SHA-256 hex = 64 chars");

        server.join().unwrap();
    }

    #[test]
    fn auth_digest_prefers_sha256_when_server_offers_both() {
        // RFC 7235 §2.1: WWW-Authenticate может содержать список challenges.
        // Сервер предлагает MD5 и SHA-256 — клиент берёт сильнейший (SHA-256).
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(2, captured_cl, |i, _req| match i {
            1 => b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"r\", nonce=\"N1\", algorithm=MD5, Digest realm=\"r\", nonce=\"N2\", algorithm=SHA-256\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            _ => unreachable!(),
        });

        let provider = Arc::new(
            StaticCredentialProvider::new().with(
                &format!("http://127.0.0.1:{port}"),
                "r",
                "u",
                "p",
            ),
        );
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        client.fetch(&url).unwrap();

        let requests = captured.lock().unwrap().clone();
        let auth = extract_authorization(&requests[1]).expect("Authorization on retry");
        assert!(auth.contains("algorithm=SHA-256"));
        // nonce должен быть от SHA-256 challenge (N2), не MD5 (N1).
        assert!(auth.contains("nonce=\"N2\""));

        server.join().unwrap();
    }

    #[test]
    fn auth_no_provider_passes_401_as_error() {
        // Без with_credentials — 401 не вызывает retry, fetch возвращает Err.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(1, captured_cl, |_, _| {
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"r\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let err = client.fetch(&url).expect_err("401 must be propagated");
        assert!(format!("{err:?}").contains("401"));
        assert_eq!(captured.lock().unwrap().len(), 1, "no retry without provider");

        server.join().unwrap();
    }

    #[test]
    fn auth_provider_returns_none_passes_401_as_error() {
        // Провайдер не нашёл creds для (origin, realm) — клиент не делает retry.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(1, captured_cl, |_, _| {
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"r\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        // Provider с creds для *другого* origin — на запрашиваемый realm ответит None.
        let provider = Arc::new(
            StaticCredentialProvider::new().with("http://other.example", "r", "u", "p"),
        );
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert!(client.fetch(&url).is_err());
        assert_eq!(captured.lock().unwrap().len(), 1, "no retry on provider None");

        server.join().unwrap();
    }

    #[test]
    fn auth_unsupported_scheme_no_retry() {
        // Bearer / Negotiate / NTLM — не поддерживаются, 401 пробрасывается.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(1, captured_cl, |_, _| {
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"api\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let provider = Arc::new(StaticCredentialProvider::new().with("", "", "u", "p"));
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert!(client.fetch(&url).is_err());
        assert_eq!(captured.lock().unwrap().len(), 1);

        server.join().unwrap();
    }

    #[test]
    fn auth_one_retry_only_on_consecutive_401() {
        // Если retry-запрос тоже получил 401 (неверные creds) — клиент НЕ
        // делает второй retry, сразу возвращает Err.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(2, captured_cl, |_, _| {
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"r\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let provider = Arc::new(
            StaticCredentialProvider::new().with(&format!("http://127.0.0.1:{port}"), "r", "u", "p"),
        );
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert!(client.fetch(&url).is_err());
        assert_eq!(
            captured.lock().unwrap().len(),
            2,
            "exactly two requests: original + one retry"
        );

        server.join().unwrap();
    }

    #[test]
    fn auth_no_www_authenticate_header_no_retry() {
        // 401 без WWW-Authenticate — невалидный server response, retry
        // невозможен. Просто пробрасываем как Err.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(1, captured_cl, |_, _| {
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let provider = Arc::new(StaticCredentialProvider::new().with("", "", "u", "p"));
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert!(client.fetch(&url).is_err());
        assert_eq!(captured.lock().unwrap().len(), 1);

        server.join().unwrap();
    }

    #[test]
    fn auth_provider_sees_correct_origin_and_realm() {
        // Проверяем, что провайдер видит origin (scheme://host[:port], без
        // default-порта 80/443) и realm из challenge.
        struct CapturingProvider {
            seen: Mutex<Vec<HttpAuthChallenge>>,
        }
        impl HttpCredentialProvider for CapturingProvider {
            fn credentials(&self, c: &HttpAuthChallenge) -> Option<HttpCredentials> {
                self.seen.lock().unwrap().push(c.clone());
                Some(HttpCredentials {
                    username: "u".into(),
                    password: "p".into(),
                })
            }
        }

        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(2, captured_cl, |i, _| match i {
            1 => b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Admin Area\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            2 => b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            _ => unreachable!(),
        });

        let provider = Arc::new(CapturingProvider {
            seen: Mutex::new(Vec::new()),
        });
        let client = HttpClient::new().with_credentials(provider.clone());
        let url = Url::parse(&format!("http://127.0.0.1:{port}/secret")).unwrap();
        client.fetch(&url).unwrap();

        let seen = provider.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        // Non-default port — должен быть в origin.
        assert_eq!(seen[0].origin, format!("http://127.0.0.1:{port}"));
        assert_eq!(seen[0].realm, "Admin Area");
        assert_eq!(seen[0].scheme, HttpAuthScheme::Basic);

        server.join().unwrap();
    }

    // ── Content-Encoding pipeline ───────────────────────────────────────────

    /// `Hello, World!` сжатый эталонным brotli CLI (см. тесты в `brotli` модуле).
    const BROTLI_HELLO_WORLD: [u8; 17] = [
        0x0f, 0x06, 0x80, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64,
        0x21, 0x03,
    ];

    /// Mock-сервер, который перед ответом сохраняет полученный request
    /// (raw byte-block до пустой строки) в `captured`. Позволяет тестам
    /// проверять, какие headers улетели на сервер.
    fn mock_http_server_capturing<F>(
        captured: Arc<Mutex<Vec<String>>>,
        responder: F,
    ) -> (u16, thread::JoinHandle<()>)
    where
        F: Fn(usize) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let is_blank = line == "\r\n" || line == "\n";
                request.push_str(&line);
                if is_blank {
                    break;
                }
            }
            captured.lock().unwrap().push(request);
            let body = responder(1);
            let _ = sock.write_all(&body);
            let _ = sock.shutdown(std::net::Shutdown::Both);
        });
        (port, handle)
    }

    /// Mock decoder для unit-тестов цепочек encoding-ов. `name` — какое
    /// имя возвращает `encoding()`; `decode` просто разворачивает байты,
    /// чтобы тест мог детектировать, в каком порядке вызвался декодер.
    #[derive(Debug)]
    struct ReverseDecoder {
        name: &'static str,
    }
    impl ContentDecoder for ReverseDecoder {
        fn encoding(&self) -> &'static str {
            self.name
        }
        fn decode(&self, input: &[u8]) -> Result<Vec<u8>> {
            Ok(input.iter().rev().copied().collect())
        }
    }

    /// Mock decoder, который к каждому байту добавляет `delta`. Позволяет
    /// убедиться, что цепочка декодеров применяется в правильном порядке.
    #[derive(Debug)]
    struct ShiftDecoder {
        name: &'static str,
        delta: u8,
    }
    impl ContentDecoder for ShiftDecoder {
        fn encoding(&self) -> &'static str {
            self.name
        }
        fn decode(&self, input: &[u8]) -> Result<Vec<u8>> {
            Ok(input.iter().map(|b| b.wrapping_sub(self.delta)).collect())
        }
    }

    #[test]
    fn apply_content_encoding_no_header_passthrough() {
        let body = b"raw bytes".to_vec();
        let out = apply_content_encoding(body.clone(), &[], &[]).expect("ok");
        assert_eq!(out, body);
    }

    #[test]
    fn apply_content_encoding_identity_passthrough() {
        let headers = vec![("Content-Encoding".to_owned(), "identity".to_owned())];
        let body = b"plain".to_vec();
        let out = apply_content_encoding(body.clone(), &headers, &[]).expect("identity ok");
        assert_eq!(out, body);
    }

    #[test]
    fn apply_content_encoding_empty_header_passthrough() {
        let headers = vec![("Content-Encoding".to_owned(), "".to_owned())];
        let body = b"plain".to_vec();
        let out = apply_content_encoding(body.clone(), &headers, &[]).expect("empty ok");
        assert_eq!(out, body);
    }

    #[test]
    fn apply_content_encoding_unknown_encoding_errors() {
        let headers = vec![("Content-Encoding".to_owned(), "gzip".to_owned())];
        let err = apply_content_encoding(b"x".to_vec(), &headers, &[])
            .expect_err("must error on unknown");
        let msg = format!("{err:?}");
        assert!(msg.contains("gzip"), "unexpected message: {msg}");
        assert!(msg.contains("no decoder registered"), "unexpected: {msg}");
    }

    #[test]
    fn apply_content_encoding_brotli_decodes() {
        let headers = vec![("Content-Encoding".to_owned(), "br".to_owned())];
        let decoders: Vec<Arc<dyn ContentDecoder>> = vec![Arc::new(BrotliContentDecoder::new())];
        let out =
            apply_content_encoding(BROTLI_HELLO_WORLD.to_vec(), &headers, &decoders).expect("ok");
        assert_eq!(out, b"Hello, World!");
    }

    #[test]
    fn apply_content_encoding_header_case_insensitive() {
        // Сервер может вернуть «BR» вместо «br» — токены case-insensitive.
        let headers = vec![("Content-Encoding".to_owned(), "BR".to_owned())];
        let decoders: Vec<Arc<dyn ContentDecoder>> = vec![Arc::new(BrotliContentDecoder::new())];
        let out =
            apply_content_encoding(BROTLI_HELLO_WORLD.to_vec(), &headers, &decoders).expect("ok");
        assert_eq!(out, b"Hello, World!");
    }

    #[test]
    fn apply_content_encoding_reverse_order_for_stacked() {
        // RFC 7231 §3.1.2.2: encodings в header-е — в порядке применения.
        // Header «a, b» означает: сначала server применил «a», потом «b».
        // Снимать надо в обратном порядке: сначала «b», потом «a». Используем
        // mock-decoder-ы, которые отнимают свой delta — если порядок неверный,
        // байты получатся другие.
        // Исходные: b'X' = 0x58.
        // Симулируем server: применил `add 1` (a=`shift1`), потом `add 2` (b=`shift2`).
        // Result-байт на проводе: 0x58 + 1 + 2 = 0x5b.
        // Client header: «shift1, shift2».
        // Apply order: shift2 (− 2 → 0x59), потом shift1 (− 1 → 0x58) = `X`. ОК.
        let headers = vec![(
            "Content-Encoding".to_owned(),
            "shift1, shift2".to_owned(),
        )];
        let decoders: Vec<Arc<dyn ContentDecoder>> = vec![
            Arc::new(ShiftDecoder { name: "shift1", delta: 1 }),
            Arc::new(ShiftDecoder { name: "shift2", delta: 2 }),
        ];
        let out = apply_content_encoding(vec![0x5b], &headers, &decoders).expect("ok");
        assert_eq!(out, b"X");
    }

    #[test]
    fn apply_content_encoding_skips_identity_in_chain() {
        // `Content-Encoding: identity, br` — identity dropped, br применяется.
        let headers = vec![("Content-Encoding".to_owned(), "identity, br".to_owned())];
        let decoders: Vec<Arc<dyn ContentDecoder>> = vec![Arc::new(BrotliContentDecoder::new())];
        let out =
            apply_content_encoding(BROTLI_HELLO_WORLD.to_vec(), &headers, &decoders).expect("ok");
        assert_eq!(out, b"Hello, World!");
    }

    #[test]
    fn accept_encoding_header_omitted_when_no_decoders() {
        let client = HttpClient::new();
        assert!(client.accept_encoding_header().is_none());
    }

    #[test]
    fn accept_encoding_header_lists_decoders_in_order() {
        let client = HttpClient::new()
            .with_content_decoder(Arc::new(BrotliContentDecoder::new()))
            .with_content_decoder(Arc::new(ReverseDecoder { name: "rev" }));
        assert_eq!(client.accept_encoding_header().as_deref(), Some("br, rev"));
    }

    #[test]
    fn fetch_decodes_brotli_response_e2e() {
        // Mock сервер отдаёт Content-Encoding: br + brotli payload "Hello, World!".
        let mut response = Vec::new();
        response.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
        response.extend_from_slice(b"Content-Encoding: br\r\n");
        response.extend_from_slice(
            format!("Content-Length: {}\r\n", BROTLI_HELLO_WORLD.len()).as_bytes(),
        );
        response.extend_from_slice(b"Connection: close\r\n\r\n");
        response.extend_from_slice(&BROTLI_HELLO_WORLD);
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) =
            mock_http_server_capturing(captured.clone(), move |_| response.clone());

        let client = HttpClient::new().with_content_decoder(Arc::new(BrotliContentDecoder::new()));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let body = client.fetch(&url).expect("fetch");
        assert_eq!(body, b"Hello, World!");

        // Дополнительно: убедимся, что в request улетел Accept-Encoding: br.
        let req = captured.lock().unwrap()[0].clone();
        assert!(
            req.to_ascii_lowercase().contains("accept-encoding: br"),
            "no Accept-Encoding in request: {req:?}"
        );

        server.join().unwrap();
    }

    #[test]
    fn fetch_without_decoder_for_advertised_encoding_errors() {
        // Сервер вернул Content-Encoding: br, но клиент не регистрировал
        // декодер — это нарушение RFC 7231 (server должен использовать только
        // объявленные в Accept-Encoding кодеки), но реальные серверы такое
        // умеют. Лучше падать чем возвращать мусор.
        let mut response = Vec::new();
        response.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
        response.extend_from_slice(b"Content-Encoding: br\r\n");
        response.extend_from_slice(
            format!("Content-Length: {}\r\n", BROTLI_HELLO_WORLD.len()).as_bytes(),
        );
        response.extend_from_slice(b"Connection: close\r\n\r\n");
        response.extend_from_slice(&BROTLI_HELLO_WORLD);
        let (port, server) = mock_http_server(1, move |_| response.clone());

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        let err = client.fetch(&url).expect_err("must error");
        assert!(
            format!("{err:?}").contains("unsupported Content-Encoding"),
            "got: {err:?}"
        );
        server.join().unwrap();
    }

    #[test]
    fn fetch_no_accept_encoding_when_no_decoders() {
        // Без декодеров клиент НЕ выставляет Accept-Encoding header (а не пустое
        // значение): server должен трактовать отсутствие как «identity only».
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nfoo".to_vec();
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) =
            mock_http_server_capturing(captured.clone(), move |_| response.clone());

        let client = HttpClient::new();
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        client.fetch(&url).expect("fetch");

        let req = captured.lock().unwrap()[0].clone();
        assert!(
            !req.to_ascii_lowercase().contains("accept-encoding"),
            "Accept-Encoding leaked when no decoders: {req:?}"
        );
        server.join().unwrap();
    }

    #[test]
    fn auth_credentials_not_sent_proactively_first_request() {
        // Sanity: с подключённым provider'ом первый request всё равно идёт
        // без Authorization — credentials эмитятся только в ответ на 401.
        // (RFC 7235 §2.1: «server controls credential negotiation»; preemptive
        // Basic auth — отдельная фича, у нас явно не включена.)
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_cl = captured.clone();
        let (port, server) = mock_auth_server(1, captured_cl, |_, _| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });

        let provider = Arc::new(
            StaticCredentialProvider::new().with(&format!("http://127.0.0.1:{port}"), "r", "u", "p"),
        );
        let client = HttpClient::new().with_credentials(provider);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        let requests = captured.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(
            extract_authorization(&requests[0]).is_none(),
            "no proactive Authorization on first request"
        );

        server.join().unwrap();
    }

    // ── Mixed-content enforcement (W3C Mixed Content §5) ─────────────────────

    /// Helper: secure top-level origin для https://example.com.
    fn https_example_origin() -> Origin {
        Origin::from_url(&Url::parse("https://example.com/").unwrap()).unwrap()
    }

    #[test]
    fn fetch_subresource_without_policy_uses_no_enforcement() {
        // Без policy — поведение `fetch_subresource` тождественно `fetch`:
        // никакого RequestBlocked, classifier не вызывается. Используем
        // mock-сервер на 127.0.0.1, который trustworthy и сам по себе
        // не mixed-content.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });
        let url = Url::parse(&format!("http://127.0.0.1:{port}/lib.js")).unwrap();
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new().with_sink(sink.clone());
        assert_eq!(client.fetch_subresource(&url, RequestDestination::Script).unwrap(), b"ok");

        let events = sink.events();
        assert_eq!(events.len(), 2, "Started + Completed");
        assert!(!events.iter().any(|e| matches!(e, Event::RequestBlocked { .. })));

        server.join().unwrap();
    }

    #[test]
    fn fetch_subresource_blocks_http_script_on_https_page_in_spec_default() {
        // Сетевого сервера НЕТ: policy обязана блокировать запрос ДО connect-а.
        // Если enforcement не сработает — тест дойдёт до DNS/connect для
        // несуществующего хоста, который мы НЕ блокируем фильтром, и упадёт
        // с другим текстом ошибки.
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_tab(TabId(7))
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::SpecDefault);
        let url = Url::parse("http://cdn.invalid/lib.js").unwrap();

        let err = client
            .fetch_subresource(&url, RequestDestination::Script)
            .expect_err("blockable script on https page must be blocked");
        assert!(
            format!("{err:?}").contains("mixed-content"),
            "reason in err: {err:?}"
        );

        let events = sink.events();
        assert_eq!(events.len(), 1, "expected only RequestBlocked, got {events:?}");
        match &events[0] {
            Event::RequestBlocked { tab_id, url, reason } => {
                assert_eq!(*tab_id, TabId(7));
                assert_eq!(url.as_str(), "http://cdn.invalid/lib.js");
                assert_eq!(reason, "mixed-content: blockable");
            }
            other => panic!("expected RequestBlocked, got {other:?}"),
        }
    }

    #[test]
    fn fetch_subresource_allows_http_image_in_spec_default() {
        // OptionallyBlockable (image / media / prefetch) в SpecDefault режиме —
        // пропускаем. Используем mock-сервер; если бы enforcement ошибочно
        // заблокировал, сокет вообще не открылся бы.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\npng".to_vec()
        });
        // НЕ используем localhost / 127.0.0.1: они potentially-trustworthy
        // и дают NotMixed без всякой policy. Bind на 127.0.0.1, но запрос
        // ходим на 127.0.0.2 — это всё равно loopback range, но не сам port.
        // Простой ход — обойти trustworthy-фильтр через любой hostname,
        // который резолвит на 127.0.0.1 через etc/hosts мы делать не будем.
        // Вместо этого: для проверки «не блокирует» достаточно убедиться,
        // что для трастового host-а policy не вмешалась. Возьмём 127.0.0.1
        // и убедимся, что Started/Completed эмитятся (NotMixed путь).
        let url = Url::parse(&format!("http://127.0.0.1:{port}/pic.png")).unwrap();

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::SpecDefault);

        assert_eq!(
            client.fetch_subresource(&url, RequestDestination::Image).unwrap(),
            b"png"
        );

        let events = sink.events();
        assert_eq!(events.len(), 2, "Started + Completed");
        assert!(matches!(events[0], Event::RequestStarted { .. }));
        assert!(matches!(events[1], Event::RequestCompleted { status: 200, .. }));

        server.join().unwrap();
    }

    #[test]
    fn fetch_subresource_strict_blocks_optionally_blockable_image() {
        // Strict-режим: image тоже блокируется. Хост — не trustworthy.
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::Strict);
        let url = Url::parse("http://cdn.invalid/pic.png").unwrap();

        let err = client
            .fetch_subresource(&url, RequestDestination::Image)
            .expect_err("strict mode must block optionally-blockable");
        assert!(
            format!("{err:?}").contains("mixed-content"),
            "reason: {err:?}"
        );

        let events = sink.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::RequestBlocked { reason, .. } => {
                assert_eq!(reason, "mixed-content: optionally-blockable");
            }
            other => panic!("expected RequestBlocked, got {other:?}"),
        }
    }

    #[test]
    fn fetch_subresource_blocks_on_redirect_hop_to_http() {
        // hop1: HTTPS → 302 Location: http://cdn.invalid/lib.js. Нашему mock-у
        // достаточно отдать редирект; hop2 не должен попасть в сеть. Mock-сервер
        // мы держим HTTP-only (TLS в юнит-тесте слишком тяжело), а top-level
        // origin делаем `https://example.com/` — это даст secure-context от
        // policy. Чтобы hop1 (HTTP→HTTP redirect) сам не блокировался mixed-
        // content-ом, делаем запрос к 127.0.0.1 (trustworthy) — там NotMixed.
        // hop2 ведёт на non-trustworthy http://cdn.invalid → blockable.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 302 Found\r\nLocation: http://cdn.invalid/lib.js\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::SpecDefault);
        let url = Url::parse(&format!("http://127.0.0.1:{port}/redir")).unwrap();

        let err = client
            .fetch_subresource(&url, RequestDestination::Script)
            .expect_err("redirect to http script on https page must be blocked");
        assert!(format!("{err:?}").contains("mixed-content"));

        let events = sink.events();
        assert_eq!(events.len(), 3, "Started(hop1) + Completed(302) + Blocked(hop2), got {events:?}");
        match &events[0] {
            Event::RequestStarted { url, .. } => {
                assert_eq!(url.as_str(), &format!("http://127.0.0.1:{port}/redir"));
            }
            other => panic!("hop1 Started: {other:?}"),
        }
        match &events[1] {
            Event::RequestCompleted { status, .. } => assert_eq!(*status, 302),
            other => panic!("hop1 Completed(302): {other:?}"),
        }
        match &events[2] {
            Event::RequestBlocked { url, reason, .. } => {
                assert_eq!(url.as_str(), "http://cdn.invalid/lib.js");
                assert_eq!(reason, "mixed-content: blockable");
            }
            other => panic!("hop2 Blocked: {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_allows_trustworthy_http_url_with_mixed_content_policy() {
        // fetch() теперь использует RequestDestination::Other когда policy задана,
        // поэтому enforcement активируется. Но 127.0.0.1 — loopback (potentially
        // trustworthy по W3C Secure Contexts §3.1) → classify_subresource_request
        // возвращает NotMixed → блокировки нет, Started + Completed.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });
        let url = Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::Strict);
        assert_eq!(client.fetch(&url).unwrap(), b"ok");

        let events = sink.events();
        assert_eq!(events.len(), 2, "Started + Completed, got {events:?}");
        assert!(!events.iter().any(|e| matches!(e, Event::RequestBlocked { .. })));

        server.join().unwrap();
    }

    #[test]
    fn fetch_blocks_non_trustworthy_http_url_with_mixed_content_policy() {
        // Сетевого сервера НЕТ: policy обязана блокировать запрос ДО connect-а.
        // fetch() использует RequestDestination::Other (Blockable) когда policy
        // задана — enforce-ится идентично fetch_subresource с blockable dest.
        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_tab(TabId(7))
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::SpecDefault);
        let url = Url::parse("http://cdn.invalid/resource").unwrap();

        let err = client
            .fetch(&url)
            .expect_err("blockable fetch on https context must be blocked");
        assert!(
            format!("{err:?}").contains("mixed-content"),
            "reason in err: {err:?}"
        );

        let events = sink.events();
        assert_eq!(events.len(), 1, "expected only RequestBlocked, got {events:?}");
        match &events[0] {
            Event::RequestBlocked { tab_id, url, reason } => {
                assert_eq!(*tab_id, TabId(7));
                assert_eq!(url.as_str(), "http://cdn.invalid/resource");
                assert_eq!(reason, "mixed-content: blockable");
            }
            other => panic!("expected RequestBlocked, got {other:?}"),
        }
    }

    #[test]
    fn fetch_subresource_https_origin_target_passes_through() {
        // HTTPS subresource на HTTPS top-level — NotMixed: classifier пропускает,
        // policy не блокирует. Mock-сервер по 127.0.0.1 (HTTP, trustworthy) —
        // и здесь NotMixed по trustworthy host-у. Главное — что Started/Completed
        // идут, RequestBlocked нет.
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec()
        });
        let url = Url::parse(&format!("http://127.0.0.1:{port}/x.css")).unwrap();

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_mixed_content_policy(https_example_origin(), MixedContentMode::SpecDefault);
        assert_eq!(
            client.fetch_subresource(&url, RequestDestination::Style).unwrap(),
            b"hello"
        );

        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert!(!events.iter().any(|e| matches!(e, Event::RequestBlocked { .. })));

        server.join().unwrap();
    }

    #[test]
    fn fetch_subresource_insecure_top_level_never_blocks() {
        // top-level HTTP → концепции mixed-content нет, любой подресурс
        // допустим в любом режиме. Тест демонстрирует, что enforce
        // _не сработает_ для insecure top-level — иначе мы заблокировали бы
        // запрос ДО mock-сервера и тест не получил бы "ok".
        let (port, server) = mock_http_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });
        let url = Url::parse(&format!("http://127.0.0.1:{port}/lib.js")).unwrap();
        let insecure_top = Origin::from_url(&Url::parse("http://example.com/").unwrap()).unwrap();

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_mixed_content_policy(insecure_top, MixedContentMode::Strict);
        assert_eq!(
            client.fetch_subresource(&url, RequestDestination::Script).unwrap(),
            b"ok"
        );

        let events = sink.events();
        assert!(!events.iter().any(|e| matches!(e, Event::RequestBlocked { .. })));

        server.join().unwrap();
    }

    // ── CORS preflight enforcement (Fetch §3-§4) ─────────────────────────────

    /// Mock-сервер, который capture-ит сырые request-headers каждого
    /// принятого соединения (до пустой строки) и шлёт `responder(i)` —
    /// одно соединение на запрос (server закрывает после ответа, чтобы тесты
    /// не страдали от keep-alive interleaving).
    fn mock_cors_server<F>(
        accept_count: usize,
        captured: Arc<Mutex<Vec<String>>>,
        responder: F,
    ) -> (u16, thread::JoinHandle<()>)
    where
        F: Fn(usize) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for i in 1..=accept_count {
                let (mut sock, _) = match listener.accept() {
                    Ok(p) => p,
                    Err(_) => break,
                };
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                let mut req = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let is_blank = line == "\r\n" || line == "\n";
                    req.push_str(&line);
                    if is_blank {
                        break;
                    }
                }
                captured.lock().unwrap().push(req);
                let body = responder(i);
                let _ = sock.write_all(&body);
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });
        (port, handle)
    }

    fn cross_origin_requestor() -> Origin {
        Origin::from_url(&Url::parse("https://app.example.com/").unwrap()).unwrap()
    }

    fn cors_request(method: &str, target: &Url, headers: &[(&str, &str)]) -> CorsRequest {
        CorsRequest {
            origin: cross_origin_requestor(),
            target: target.clone(),
            method: method.to_owned(),
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            credentials_mode: CredentialsMode::SameOrigin,
        }
    }

    #[test]
    fn fetch_cors_requires_cache() {
        // Без with_cors_cache fetch_cors возвращает Err и в сеть НЕ ходит.
        let url = Url::parse("http://nonexistent.invalid/").unwrap();
        let client = HttpClient::new();
        let err = client
            .fetch_cors(cors_request("GET", &url, &[]), None)
            .expect_err("must error without cache");
        assert!(
            format!("{err:?}").contains("CORS preflight cache not configured"),
            "got: {err:?}"
        );
    }

    #[test]
    fn fetch_cors_simple_get_no_preflight_with_acao() {
        // CORS-safelisted GET без custom headers → preflight НЕ нужен; одна
        // accept-итерация. Сервер обязан вернуть `Access-Control-Allow-Origin`,
        // иначе actual-response validation падает.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(1, captured.clone(), |_| {
            b"HTTP/1.1 200 OK\r\n\
              Access-Control-Allow-Origin: https://app.example.com\r\n\
              Content-Length: 4\r\n\
              Connection: close\r\n\r\nbody"
                .to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(Arc::new(PreflightCache::new()));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/api/data")).unwrap();
        let body = client
            .fetch_cors(cors_request("GET", &url, &[]), None)
            .expect("fetch");
        assert_eq!(body, b"body");

        // Один запрос — actual GET, без preflight.
        let reqs = captured.lock().unwrap().clone();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].starts_with("GET /api/data "), "got: {:?}", reqs[0]);
        // Origin header обязан присутствовать на cross-origin.
        assert!(
            reqs[0].contains("Origin: https://app.example.com"),
            "missing Origin: {:?}",
            reqs[0]
        );

        // Только Started + Completed — без preflight pair.
        let events = sink.events();
        assert_eq!(events.len(), 2, "got: {events:?}");
        assert!(matches!(events[0], Event::RequestStarted { .. }));
        assert!(matches!(events[1], Event::RequestCompleted { status: 200, .. }));

        server.join().unwrap();
    }

    #[test]
    fn fetch_cors_simple_get_missing_acao_blocks() {
        // Cross-origin GET без Access-Control-Allow-Origin в ответе →
        // RequestBlocked + Err. Проверяет actual-response validation.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(1, captured.clone(), |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(Arc::new(PreflightCache::new()));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/api")).unwrap();
        let err = client
            .fetch_cors(cors_request("GET", &url, &[]), None)
            .expect_err("must block on missing ACAO");
        assert!(
            format!("{err:?}").contains("cors-response"),
            "got: {err:?}"
        );

        // Events: Started → Completed (got actual response) → Blocked.
        let events = sink.events();
        assert_eq!(events.len(), 3, "got: {events:?}");
        assert!(matches!(events[0], Event::RequestStarted { .. }));
        assert!(matches!(events[1], Event::RequestCompleted { status: 200, .. }));
        match &events[2] {
            Event::RequestBlocked { reason, .. } => {
                assert!(reason.starts_with("cors-response: "), "got: {reason}");
                assert!(reason.contains("Access-Control-Allow-Origin"), "got: {reason}");
            }
            other => panic!("expected RequestBlocked, got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_cors_custom_header_triggers_preflight_then_actual() {
        // GET с X-Custom header → preflight OPTIONS обязательно. Сервер
        // отвечает 204 на OPTIONS (с ACAO+ACAH), затем 200 на GET (с ACAO).
        // Ожидаем: 2 accept-итерации, 4 события (2×Started+Completed).
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(2, captured.clone(), |i| match i {
            1 => b"HTTP/1.1 204 No Content\r\n\
                   Access-Control-Allow-Origin: https://app.example.com\r\n\
                   Access-Control-Allow-Headers: x-custom\r\n\
                   Content-Length: 0\r\n\
                   Connection: close\r\n\r\n"
                .to_vec(),
            2 => b"HTTP/1.1 200 OK\r\n\
                   Access-Control-Allow-Origin: https://app.example.com\r\n\
                   Content-Length: 4\r\n\
                   Connection: close\r\n\r\nbody"
                .to_vec(),
            _ => unreachable!(),
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(Arc::new(PreflightCache::new()));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/api")).unwrap();
        let body = client
            .fetch_cors(cors_request("GET", &url, &[("X-Custom", "yes")]), None)
            .expect("fetch");
        assert_eq!(body, b"body");

        let reqs = captured.lock().unwrap().clone();
        assert_eq!(reqs.len(), 2, "expected preflight + actual, got: {reqs:?}");
        // 1) Preflight OPTIONS с Access-Control-Request-Method + Request-Headers.
        assert!(reqs[0].starts_with("OPTIONS /api "), "got: {:?}", reqs[0]);
        assert!(
            reqs[0].contains("Access-Control-Request-Method: GET"),
            "missing ACRM: {:?}",
            reqs[0]
        );
        assert!(
            reqs[0]
                .to_ascii_lowercase()
                .contains("access-control-request-headers: x-custom"),
            "missing ACRH: {:?}",
            reqs[0]
        );
        assert!(reqs[0].contains("Origin: https://app.example.com"));
        // 2) Actual GET c Origin + X-Custom.
        assert!(reqs[1].starts_with("GET /api "), "got: {:?}", reqs[1]);
        assert!(reqs[1].contains("Origin: https://app.example.com"));
        assert!(reqs[1].contains("X-Custom: yes"));

        // 4 события: preflight Started+Completed, actual Started+Completed.
        let events = sink.events();
        assert_eq!(events.len(), 4, "got: {events:?}");
        match &events[1] {
            Event::RequestCompleted { status, .. } => assert_eq!(*status, 204),
            other => panic!("expected preflight Completed(204), got {other:?}"),
        }
        match &events[3] {
            Event::RequestCompleted { status, .. } => assert_eq!(*status, 200),
            other => panic!("expected actual Completed(200), got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_cors_preflight_rejected_blocks_before_actual() {
        // Preflight 200 без ACAO → evaluate_preflight_response падает,
        // actual request НЕ отправляется. Server accept_count=1
        // подтверждает, что второго соединения не было.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(1, captured.clone(), |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(Arc::new(PreflightCache::new()));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/api")).unwrap();
        let err = client
            .fetch_cors(cors_request("GET", &url, &[("X-Custom", "x")]), None)
            .expect_err("preflight must fail");
        assert!(
            format!("{err:?}").contains("cors-preflight"),
            "got: {err:?}"
        );

        // Только preflight запрос — actual не ушёл.
        let reqs = captured.lock().unwrap().clone();
        assert_eq!(reqs.len(), 1, "got: {reqs:?}");
        assert!(reqs[0].starts_with("OPTIONS "));

        // Events: Started(preflight), Completed(preflight), Blocked.
        let events = sink.events();
        assert_eq!(events.len(), 3, "got: {events:?}");
        match &events[2] {
            Event::RequestBlocked { reason, .. } => {
                assert!(reason.starts_with("cors-preflight: "), "got: {reason}");
            }
            other => panic!("expected RequestBlocked, got {other:?}"),
        }

        server.join().unwrap();
    }

    #[test]
    fn fetch_cors_preflight_cached_skips_second_options() {
        // Первый запрос с PUT (non-simple) → preflight + actual. Кеш
        // запоминает на max-age=600 секунд. Второй идентичный запрос
        // обходит preflight (cache hit) и идёт сразу к actual.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(3, captured.clone(), |i| match i {
            1 => b"HTTP/1.1 204 No Content\r\n\
                   Access-Control-Allow-Origin: https://app.example.com\r\n\
                   Access-Control-Allow-Methods: PUT\r\n\
                   Access-Control-Max-Age: 600\r\n\
                   Content-Length: 0\r\n\
                   Connection: close\r\n\r\n"
                .to_vec(),
            2 | 3 => b"HTTP/1.1 200 OK\r\n\
                       Access-Control-Allow-Origin: https://app.example.com\r\n\
                       Content-Length: 2\r\n\
                       Connection: close\r\n\r\nok"
                .to_vec(),
            _ => unreachable!(),
        });

        let sink = Arc::new(CollectingSink::new());
        let cache = Arc::new(PreflightCache::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(cache.clone());
        let url = Url::parse(&format!("http://127.0.0.1:{port}/api")).unwrap();

        // 1-я итерация: preflight + actual = 2 accept-а.
        client
            .fetch_cors(cors_request("PUT", &url, &[]), None)
            .expect("first call");
        // 2-я итерация: только actual (cache hit) = 1 accept.
        client
            .fetch_cors(cors_request("PUT", &url, &[]), None)
            .expect("second call");

        let reqs = captured.lock().unwrap().clone();
        assert_eq!(reqs.len(), 3, "expected preflight + 2×actual, got: {reqs:?}");
        assert!(reqs[0].starts_with("OPTIONS "), "got: {:?}", reqs[0]);
        assert!(reqs[1].starts_with("PUT "), "got: {:?}", reqs[1]);
        assert!(reqs[2].starts_with("PUT "), "got: {:?}", reqs[2]);

        server.join().unwrap();
    }

    #[test]
    fn fetch_cors_credentials_include_rejects_wildcard_acao() {
        // credentials_mode=Include требует explicit-Origin, ACAO=`*`
        // обязан быть отвергнут (Fetch §4.10 шаг 2).
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(1, captured.clone(), |_| {
            b"HTTP/1.1 200 OK\r\n\
              Access-Control-Allow-Origin: *\r\n\
              Access-Control-Allow-Credentials: true\r\n\
              Content-Length: 2\r\n\
              Connection: close\r\n\r\nok"
                .to_vec()
        });

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(Arc::new(PreflightCache::new()));
        let url = Url::parse(&format!("http://127.0.0.1:{port}/api")).unwrap();
        let mut request = cors_request("GET", &url, &[]);
        request.credentials_mode = CredentialsMode::Include;
        let err = client
            .fetch_cors(request, None)
            .expect_err("wildcard ACAO with credentials must block");
        assert!(
            format!("{err:?}").contains("cors-response"),
            "got: {err:?}"
        );

        server.join().unwrap();
    }

    #[test]
    fn fetch_cors_same_origin_skips_enforcement() {
        // Если requestor.origin == target.origin — preflight не нужен,
        // ACAO не проверяется (даже отсутствует — ок). Сервер не возвращает
        // никаких CORS headers, и запрос проходит как обычный fetch.
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (port, server) = mock_cors_server(1, captured.clone(), |_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nsame".to_vec()
        });
        // Origin совпадает с target: http://127.0.0.1:PORT.
        let target = Url::parse(&format!("http://127.0.0.1:{port}/local")).unwrap();
        let requestor = Origin::from_url(&target).unwrap();

        let sink = Arc::new(CollectingSink::new());
        let client = HttpClient::new()
            .with_sink(sink.clone())
            .with_cors_cache(Arc::new(PreflightCache::new()));
        let req = CorsRequest {
            origin: requestor,
            target,
            method: "GET".to_owned(),
            headers: vec![("X-Custom".to_owned(), "y".to_owned())],
            credentials_mode: CredentialsMode::Include,
        };
        assert_eq!(client.fetch_cors(req, None).unwrap(), b"same");

        let reqs = captured.lock().unwrap().clone();
        assert_eq!(reqs.len(), 1);
        // Single GET — no preflight даже при non-simple header.
        assert!(reqs[0].starts_with("GET "), "got: {:?}", reqs[0]);
        // Origin header НЕ шлётся для same-origin запроса.
        assert!(
            !reqs[0].contains("Origin:"),
            "Origin should not be set: {:?}",
            reqs[0]
        );

        server.join().unwrap();
    }
}

// ── InMemoryFetchInterceptor tests ────────────────────────────────────────────

#[cfg(test)]
mod interceptor_tests {
    use super::*;

    #[test]
    fn build_origin_standard_ports_omitted() {
        let http = Url::parse("http://example.com/path").unwrap();
        assert_eq!(build_origin(&http), "http://example.com");

        let https = Url::parse("https://example.com/path").unwrap();
        assert_eq!(build_origin(&https), "https://example.com");
    }

    #[test]
    fn build_origin_non_standard_port_included() {
        let url = Url::parse("https://example.com:8443/path").unwrap();
        assert_eq!(build_origin(&url), "https://example.com:8443");

        let url2 = Url::parse("http://localhost:3000/api").unwrap();
        assert_eq!(build_origin(&url2), "http://localhost:3000");
    }

    #[test]
    fn in_memory_interceptor_miss_returns_none() {
        let i = InMemoryFetchInterceptor::new();
        let url = Url::parse("https://example.com/page").unwrap();
        assert!(i.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn in_memory_interceptor_hit_returns_body() {
        let i = InMemoryFetchInterceptor::new();
        i.insert(
            "https://example.com",
            "https://example.com/page",
            b"hello".to_vec(),
        );
        let url = Url::parse("https://example.com/page").unwrap();
        assert_eq!(
            i.intercept(&url, "https://example.com"),
            Some(b"hello".to_vec())
        );
    }

    #[test]
    fn in_memory_interceptor_wrong_origin_miss() {
        let i = InMemoryFetchInterceptor::new();
        i.insert("https://other.com", "https://example.com/page", b"x".to_vec());
        let url = Url::parse("https://example.com/page").unwrap();
        assert!(i.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn network_transport_uses_interceptor_before_network() {
        // Interceptor возвращает тело — сетевого запроса не должно быть.
        // Проверяем на несуществующем хосте: если interceptor не сработает,
        // fetch провалится с network error; если сработает — Ok.
        let interceptor = Arc::new(InMemoryFetchInterceptor::new());
        interceptor.insert(
            "https://no-such-host-lumen-test.invalid",
            "https://no-such-host-lumen-test.invalid/data",
            b"intercepted".to_vec(),
        );
        let client = HttpClient::new().with_interceptor(interceptor);
        let url = Url::parse("https://no-such-host-lumen-test.invalid/data").unwrap();
        let result = client.fetch(&url).unwrap();
        assert_eq!(result, b"intercepted");
    }
}

// ── HTTP cache integration tests ─────────────────────────────────────────────

#[cfg(test)]
mod http_cache_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    fn mock_server<F>(accept_count: usize, responder: F) -> (u16, thread::JoinHandle<()>)
    where
        F: Fn(usize) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for i in 1..=accept_count {
                let (mut sock, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
                    if line == "\r\n" || line == "\n" || line.is_empty() { break; }
                }
                let _ = sock.write_all(&responder(i));
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });
        (port, handle)
    }

    #[test]
    fn http_cache_fresh_hit_skips_network() {
        // Server accepts exactly 1 connection. Cache has a fresh entry.
        // If the cache is used, the server never gets a request → test passes.
        // If cache is bypassed, the server closes after 1 response and the
        // second request would also go there (we only start 1 server accept).
        let cache = Arc::new(http_cache::HttpCache::new());
        let url = "http://127.0.0.1:0/resource"; // port 0 → unused, never connected
        // Pre-populate cache with a fresh entry (max-age=3600).
        cache.store(
            url,
            200,
            b"cached body".to_vec(),
            &[
                ("Cache-Control".to_owned(), "max-age=3600".to_owned()),
                ("ETag".to_owned(), "\"v1\"".to_owned()),
            ],
        );
        let parsed = Url::parse(url).unwrap();
        // HttpClient with the cache — should return the cached body without
        // connecting (port 0 would fail to connect).
        let client = HttpClient::new().with_http_cache(cache);
        let result = client.fetch(&parsed).unwrap();
        assert_eq!(result, b"cached body");
    }

    #[test]
    fn http_cache_miss_fetches_and_stores() {
        // Server returns 200 with Cache-Control: max-age=60 and ETag.
        // First request should hit network and populate cache.
        // Second request should be served from cache (server only accepts once).
        let (port, server) = mock_server(1, |_| {
            b"HTTP/1.1 200 OK\r\nCache-Control: max-age=60\r\nETag: \"v1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
                .to_vec()
        });

        let cache = Arc::new(http_cache::HttpCache::new());
        let url = format!("http://127.0.0.1:{port}/data");
        let parsed = Url::parse(&url).unwrap();

        let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

        // First fetch — goes to network.
        let body1 = client.fetch(&parsed).unwrap();
        assert_eq!(body1, b"body");
        assert_eq!(cache.len(), 1, "should have stored the response");

        // Second fetch — served from cache (server thread has exited).
        let body2 = client.fetch(&parsed).unwrap();
        assert_eq!(body2, b"body");

        server.join().unwrap();
    }

    #[test]
    fn http_cache_stale_sends_conditional_get_304() {
        // Server sequence: first request fills cache (max-age=0, ETag=v1).
        // Second request is conditional GET → server returns 304.
        // Client should return the cached body.
        let (port, server) = mock_server(2, |i| match i {
            1 => b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
                .to_vec(),
            _ => b"HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n"
                .to_vec(),
        });

        let cache = Arc::new(http_cache::HttpCache::new());
        let url = format!("http://127.0.0.1:{port}/data");
        let parsed = Url::parse(&url).unwrap();
        let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

        // First fetch.
        let body1 = client.fetch(&parsed).unwrap();
        assert_eq!(body1, b"body");

        // Second fetch — stale entry, sends conditional GET, gets 304.
        let body2 = client.fetch(&parsed).unwrap();
        assert_eq!(body2, b"body", "should return cached body after 304");

        server.join().unwrap();
    }

    #[test]
    fn http_cache_stale_new_200_updates_cache() {
        // Server returns 200 twice (max-age=0, ETag changes between responses).
        // Second fetch gets a new 200 (not 304); cache should be updated.
        let (port, server) = mock_server(2, |i| match i {
            1 => b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v1\"\r\nContent-Length: 2\r\nConnection: close\r\n\r\nv1"
                .to_vec(),
            _ => b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v2\"\r\nContent-Length: 2\r\nConnection: close\r\n\r\nv2"
                .to_vec(),
        });

        let cache = Arc::new(http_cache::HttpCache::new());
        let url = format!("http://127.0.0.1:{port}/data");
        let parsed = Url::parse(&url).unwrap();
        let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

        let body1 = client.fetch(&parsed).unwrap();
        assert_eq!(body1, b"v1");

        let body2 = client.fetch(&parsed).unwrap();
        assert_eq!(body2, b"v2", "cache should update on new 200");

        let snap = cache.get(&url).unwrap();
        assert_eq!(snap.etag.as_deref(), Some("\"v2\""));

        server.join().unwrap();
    }

    #[test]
    fn http_cache_no_store_never_cached() {
        let (port, server) = mock_server(2, |_| {
            b"HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata"
                .to_vec()
        });

        let cache = Arc::new(http_cache::HttpCache::new());
        let url = format!("http://127.0.0.1:{port}/secret");
        let parsed = Url::parse(&url).unwrap();
        let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

        client.fetch(&parsed).unwrap();
        assert_eq!(cache.len(), 0, "no-store must not be cached");

        // Second fetch also goes to network (not cached).
        client.fetch(&parsed).unwrap();

        server.join().unwrap();
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn http_proxy_new_no_auth() {
        let proxy = HttpProxy::new("proxy.local".to_string(), 3128);
        assert_eq!(proxy.host, "proxy.local");
        assert_eq!(proxy.port, 3128);
        assert_eq!(proxy.auth, None);
    }

    #[test]
    fn http_proxy_with_basic_auth() {
        let proxy = HttpProxy::new("proxy.local".to_string(), 3128)
            .with_basic_auth("user", "pass");
        assert_eq!(proxy.host, "proxy.local");
        assert_eq!(proxy.port, 3128);
        assert!(proxy.auth.is_some());
        // Basic auth for "user:pass" should be base64-encoded "dXNlcjpwYXNz"
        assert_eq!(proxy.auth.as_ref().unwrap(), "dXNlcjpwYXNz");
    }

    #[test]
    fn http_client_with_proxy() {
        let proxy = Arc::new(HttpProxy::new("proxy.local".to_string(), 3128));
        let client = HttpClient::new().with_proxy(Arc::clone(&proxy));
        // Verify that the proxy was attached (no public accessor, so we just verify it doesn't crash)
        assert!(client.proxy.is_some());
    }

    #[test]
    fn base64_encode_empty_string() {
        let encoded = base64_encode("");
        assert_eq!(encoded, "");
    }

    #[test]
    fn base64_encode_single_byte() {
        let encoded = base64_encode("a");
        assert_eq!(encoded, "YQ==");
    }

    #[test]
    fn base64_encode_user_pass() {
        let encoded = base64_encode("user:pass");
        assert_eq!(encoded, "dXNlcjpwYXNz");
    }
}
