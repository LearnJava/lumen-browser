//! HTTP/1.1 wire-protocol core: request serialization, response head/body
//! parsing (`SPLIT-NW0`, split out of `network/lib.rs`).

pub(crate) mod chunked;
pub(crate) mod request;
pub(crate) mod response;

use lumen_core::error::Error;
use lumen_core::net_error::NetErrorCode;

/// COMPAT-ERRORS срез 5: сбой записи запроса. Код — из `io::Error` (сброс,
/// обрыв, таймаут), иначе соединение считается закрытым пиром.
pub(crate) fn write_failure(context: &str, e: &std::io::Error) -> Error {
    Error::net_io(context, e, NetErrorCode::ConnectionClosed)
}

/// COMPAT-ERRORS срез 5: сбой чтения ответа после connect. Код — из
/// `io::Error`; `InvalidData` (не UTF-8 в строке статуса, битый gzip/brotli)
/// — ответ не разобрался (`InvalidResponse`), прочее — соединение закрыто.
pub(crate) fn read_failure(context: &str, e: &std::io::Error) -> Error {
    let fallback = if e.kind() == std::io::ErrorKind::InvalidData {
        NetErrorCode::InvalidResponse
    } else {
        NetErrorCode::ConnectionClosed
    };
    Error::net_io(context, e, fallback)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Error as IoError, ErrorKind};

    use lumen_core::error::Error;
    use lumen_core::net_error::NetErrorCode;

    use super::{read_failure, write_failure};
    use crate::http1::chunked::read_chunked;
    use crate::http1::response::parse_status;

    fn code(err: &Error) -> NetErrorCode {
        NetErrorCode::of_error(err)
    }

    #[test]
    fn read_failure_takes_the_code_from_the_io_error() {
        let reset = read_failure("read status", &IoError::new(ErrorKind::ConnectionReset, "x"));
        assert_eq!(code(&reset), NetErrorCode::ConnectionReset);
        let timeout = read_failure("read header", &IoError::new(ErrorKind::TimedOut, "x"));
        assert_eq!(code(&timeout), NetErrorCode::ConnectionTimedOut);
        // Windows: WSAECONNRESET приходит номером ОС, а не видом.
        let wsa = read_failure("read body", &IoError::from_raw_os_error(10054));
        assert_eq!(code(&wsa), NetErrorCode::ConnectionReset);
    }

    #[test]
    fn read_failure_falls_back_by_kind() {
        let bad = read_failure("read status", &IoError::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8"));
        assert_eq!(code(&bad), NetErrorCode::InvalidResponse);
        let other = read_failure("read body", &IoError::other("x"));
        assert_eq!(code(&other), NetErrorCode::ConnectionClosed);
        // Текст прежний: `Display` и разбор «stale keep-alive» по строке не меняются.
        assert_eq!(bad.to_string(), "network error: read status: stream did not contain valid UTF-8");
    }

    #[test]
    fn write_failure_maps_broken_pipe_to_closed() {
        let e = write_failure("write request", &IoError::new(ErrorKind::BrokenPipe, "x"));
        assert_eq!(code(&e), NetErrorCode::ConnectionClosed);
        let e = write_failure("flush request", &IoError::other("x"));
        assert_eq!(code(&e), NetErrorCode::ConnectionClosed);
    }

    #[test]
    fn malformed_status_line_is_an_invalid_response() {
        assert_eq!(code(&parse_status("garbage\r\n").unwrap_err()), NetErrorCode::InvalidResponse);
    }

    #[test]
    fn chunked_framing_errors_are_typed() {
        let bad_size = read_chunked(&mut Cursor::new(b"zz\r\nabc\r\n".to_vec())).unwrap_err();
        assert_eq!(code(&bad_size), NetErrorCode::InvalidResponse);
        // Размер объявлен 10, данных 3: обрыв посреди тела.
        let truncated = read_chunked(&mut Cursor::new(b"a\r\nabc".to_vec())).unwrap_err();
        assert_eq!(code(&truncated), NetErrorCode::ConnectionClosed);
    }

    /// «Тела нет» — это про МЕТОД запроса и про СТАТУС ответа, а не про
    /// заголовки: `Content-Length` в таком ответе описывает тело, которого
    /// сервер не прислал, и читать его нельзя (RFC 9110 §9.3.2).
    #[test]
    fn no_body_detection_covers_head_and_no_body_statuses() {
        use crate::http1::chunked::response_has_no_body;
        // HEAD — тела нет при любом статусе.
        assert!(response_has_no_body(200, "HEAD"));
        assert!(response_has_no_body(404, "HEAD"));
        assert!(response_has_no_body(200, "head"));
        // 1xx/204/304 — тела нет при любом методе.
        assert!(response_has_no_body(100, "GET"));
        assert!(response_has_no_body(204, "GET"));
        assert!(response_has_no_body(304, "GET"));
        assert!(response_has_no_body(304, "POST"));
        // Обычные ответы тело имеют.
        assert!(!response_has_no_body(200, "GET"));
        assert!(!response_has_no_body(206, "GET"));
        assert!(!response_has_no_body(404, "GET"));
        assert!(!response_has_no_body(500, "POST"));
    }
}
