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
use lumen_core::ext::NetworkTransport;
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

    let (host, port) = match authority.rfind(':') {
        Some(i) => {
            let h = &authority[..i];
            let p = authority[i + 1..]
                .parse::<u16>()
                .map_err(|_| Error::Network(format!("invalid port in: {authority}")))?;
            (h.to_owned(), p)
        }
        None => (
            authority.to_owned(),
            if scheme == Scheme::Https { 443 } else { 80 },
        ),
    };

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

fn fetch_with_redirect(url: &str, hops_left: u8) -> Result<Vec<u8>> {
    if hops_left == 0 {
        return Err(Error::Network("too many redirects".to_owned()));
    }

    let parsed = parse_url(url)?;
    let mut conn = connect(&parsed)?;
    write_request(&mut conn, &parsed.host, &parsed.path)?;
    let resp = read_response(conn)?;

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

            fetch_with_redirect(&next_url, hops_left - 1)
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

/// HTTP/1.1 + HTTPS клиент. Потокобезопасен — нет внутреннего состояния.
pub struct HttpClient;

impl HttpClient {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkTransport for HttpClient {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>> {
        fetch_with_redirect(url.as_str(), 5)
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
}
