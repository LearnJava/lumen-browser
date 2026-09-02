//! HTTP/1.1 response head + status-line parsing (`read_head`/`read_response`/
//! `parse_status`). Split out of `network/lib.rs` (SPLIT-NW0).

use std::io::{BufRead, Read};

use lumen_core::error::{Error, Result};

use crate::http1::chunked::{read_body_to_eof, read_chunked};
use crate::{Connection, Response, header_value};

/// Разобранная head-секция ответа: `(status, headers, server_wants_close)`.
pub(crate) type ResponseHead = (u16, Vec<(String, String)>, bool);

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
/// Прочитать status-line + заголовки до пустой строки. Возвращает
/// `(status, headers, server_wants_close)`. При EOF до завершения head-секции
/// выставляет `conn.closed = true` и возвращает `Err`. Вынесено из
/// `read_response`, чтобы streaming-вариант (`read_response_streamed`) читал
/// head идентично — единый источник правды по разбору статуса/заголовков.
pub(crate) fn read_head(conn: &mut Connection) -> Result<ResponseHead> {
    // 1xx informational responses (RFC 9110 §15.2, e.g. 103 Early Hints)
    // precede the final response on the same connection and must be
    // skipped transparently — a client that doesn't special-case them
    // sees a 1xx status with no body where a 2xx/3xx/4xx/5xx was expected.
    // 101 Switching Protocols is excluded: it IS the final response for an
    // Upgrade request (WebSocket handshake), parsed separately by
    // `websocket::upgrade::expect_101`, never through this path. Cap at 20
    // to bound a misbehaving/malicious server flooding interim responses.
    const MAX_INTERIM_RESPONSES: u32 = 20;
    for _ in 0..MAX_INTERIM_RESPONSES {
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

        if (100..200).contains(&status) && status != 101 {
            continue;
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

        return Ok((status, headers, server_wants_close));
    }
    conn.closed = true;
    Err(Error::Network(format!(
        "too many 1xx interim responses (>{MAX_INTERIM_RESPONSES})"
    )))
}

pub(crate) fn read_response(conn: &mut Connection) -> Result<Response> {
    let (status, headers, server_wants_close) = read_head(conn)?;

    // Body: chunked > Content-Length > read-to-EOF. RFC 7230 §3.3.3 (7): a
    // response with neither applies the read-to-EOF fallback unconditionally
    // — NOT gated on an explicit `Connection: close` header. Many real
    // servers (Python's `http.server` / wptserve, the reference server WPT
    // tests run against) omit that header while still relying on
    // close-delimited framing; treating its absence as a hard protocol error
    // broke every fetch against them (found running P2-wpt S4).
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
    } else if status == 204 || status == 304 {
        // 204 No Content / 304 Not Modified не имеют тела (RFC 7230 §3.3.3).
        Vec::new()
    } else {
        // Ни chunked, ни Content-Length — читаем до EOF (RFC 7230 §3.3.3 п.7),
        // независимо от того, прислал ли сервер явный `Connection: close`.
        let res = read_body_to_eof(&mut conn.reader);
        // EOF-фреймирование исчерпывает соединение при любом исходе чтения.
        conn.closed = true;
        res?
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

pub(crate) fn parse_status(line: &str) -> Result<u16> {
    // "HTTP/1.1 200 OK\r\n"
    let mut parts = line.split_ascii_whitespace();
    let _version = parts.next();
    let code = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| Error::Network(format!("bad status line: {line:?}")))?;
    Ok(code)
}
