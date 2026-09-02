//! HTTP/1.1 response body framing/decoding: chunked transfer-encoding,
//! Content-Length/EOF framing and the streaming read path
//! (`read_response_streamed`). Split out of `network/lib.rs` (SPLIT-NW0).

use std::io::{BufRead, Read};

use lumen_core::error::{Error, Result};

use crate::http1::response::read_head;
use crate::{ChunkSink, Connection, Response, header_value};

/// Прочитать chunked-тело **полностью**, включая trailer-секцию и финальный
/// CRLF. Без дочитывания trailer-а в BufReader остаётся хвост от прошлого
/// ответа, который сломает следующий request на том же соединении — это и
/// есть отличие от прежней реализации, которая работала только с
/// `Connection: close`.
pub(crate) fn read_chunked<R: BufRead>(reader: &mut R) -> Result<Vec<u8>> {
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

/// Размер порции для streaming-чтения/декодирования тела с сокета.
const STREAM_BODY_READ: usize = 8 * 1024;

/// Framing тела ответа, определённый по заголовкам. Управляет
/// инкрементальным чтением тела в streaming-пути.
#[derive(Clone, Copy)]
pub(crate) enum BodyFraming {
    /// `Content-Length: N` — ровно N байт.
    Length(usize),
    /// `Transfer-Encoding: chunked`.
    Chunked,
    /// Ни длины, ни chunked — читаем до EOF (только при `Connection: close`).
    Eof,
}

/// Стратегия декодирования тела для прогрессивного preview-sink-а. Тело всегда
/// читается с сокета как СЫРЫЕ байты и возвращается сырым (финальный
/// `apply_content_encoding` снимает кодировку авторитетно у caller-а); этот
/// enum задаёт лишь как получить ДЕКОДИРОВАННЫЕ байты для sink-а.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum StreamDecode {
    /// `identity` / нет `Content-Encoding` — сырые байты уже декодированы.
    Identity,
    Brotli,
    Gzip,
    Deflate,
    /// Несколько кодировок или неизвестная — инкрементальный decode невозможен;
    /// тело читается буферизованно, sink не вызывается (preview до LoadDone нет).
    Unsupported,
}

/// Выбрать streaming-стратегию по заголовку `Content-Encoding`.
pub(crate) fn detect_stream_decode(headers: &[(String, String)]) -> StreamDecode {
    let raw = match header_value(headers, "content-encoding") {
        Some(v) => v,
        None => return StreamDecode::Identity,
    };
    let tokens: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s != "identity")
        .collect();
    match tokens.as_slice() {
        [] => StreamDecode::Identity,
        [one] => match one.as_str() {
            "br" => StreamDecode::Brotli,
            "gzip" | "x-gzip" => StreamDecode::Gzip,
            "deflate" => StreamDecode::Deflate,
            _ => StreamDecode::Unsupported,
        },
        _ => StreamDecode::Unsupported,
    }
}

/// `Read`-адаптер над framed-телом ответа: знает Content-Length / chunked /
/// read-to-EOF и выдаёт сырые байты тела (без head-секции). Прозрачен для
/// content-encoding — поверх можно навесить streaming-декодер.
pub(crate) struct BodyReader<'a, R: BufRead> {
    reader: &'a mut R,
    framing: BodyFraming,
    /// Length: оставшиеся байты тела; Chunked: байты в текущем chunk-е.
    remaining: usize,
    /// Конец тела достигнут (last chunk / Content-Length исчерпан / EOF).
    done: bool,
}

impl<'a, R: BufRead> BodyReader<'a, R> {
    pub(crate) fn new(reader: &'a mut R, framing: BodyFraming) -> Self {
        let remaining = match framing {
            BodyFraming::Length(n) => n,
            _ => 0,
        };
        Self { reader, framing, remaining, done: false }
    }
}

impl<R: BufRead> Read for BodyReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        match self.framing {
            BodyFraming::Length(_) => {
                if self.remaining == 0 {
                    self.done = true;
                    return Ok(0);
                }
                let take = buf.len().min(self.remaining);
                let n = self.reader.read(&mut buf[..take])?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "body shorter than Content-Length",
                    ));
                }
                self.remaining -= n;
                Ok(n)
            }
            BodyFraming::Eof => {
                match self.reader.read(buf) {
                    Ok(n) => {
                        if n == 0 {
                            self.done = true;
                        }
                        Ok(n)
                    }
                    // Обрыв TLS-соединения без close_notify на
                    // EOF-фреймировании — штатный конец тела (BUG-792):
                    // длина неизвестна, усечение неотличимо от нормы, всё
                    // выданное ранее уже у caller-а. Length/chunked ниже
                    // обрыв не прощают — там недобор доказуем.
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        self.done = true;
                        Ok(0)
                    }
                    Err(e) => Err(e),
                }
            }
            BodyFraming::Chunked => {
                if self.remaining == 0 {
                    // Следующая chunk-size строка.
                    let mut size_line = String::new();
                    self.reader.read_line(&mut size_line)?;
                    let size_hex = size_line
                        .trim_end_matches(['\r', '\n'])
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim();
                    let chunk_size = usize::from_str_radix(size_hex, 16).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
                    })?;
                    if chunk_size == 0 {
                        // last-chunk: дочитать trailer-секцию до пустой строки.
                        loop {
                            let mut line = String::new();
                            let n = self.reader.read_line(&mut line)?;
                            if n == 0 || line == "\r\n" || line == "\n" {
                                break;
                            }
                        }
                        self.done = true;
                        return Ok(0);
                    }
                    self.remaining = chunk_size;
                }
                let take = buf.len().min(self.remaining);
                let n = self.reader.read(&mut buf[..take])?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "EOF inside chunk",
                    ));
                }
                self.remaining -= n;
                if self.remaining == 0 {
                    // CRLF после данных chunk-а.
                    let mut crlf = [0u8; 2];
                    self.reader.read_exact(&mut crlf)?;
                }
                Ok(n)
            }
        }
    }
}

/// `Read`-обёртка, копирующая все прочитанные байты в `captured`. Позволяет
/// поверх streaming-декодера сохранить СЫРОЕ (ещё сжатое) тело для возврата
/// caller-у — финальный `apply_content_encoding` декодирует его авторитетно.
struct TeeReader<'a, R: Read> {
    inner: R,
    captured: &'a mut Vec<u8>,
}

impl<R: Read> Read for TeeReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.captured.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

/// Прочитать EOF-фреймированное тело: цикл `read` с накоплением, где
/// `UnexpectedEof` от TLS-слоя завершает тело УСПЕШНО, а не ошибкой.
///
/// `rustls` различает корректное завершение (`close_notify` → `Ok(0)`) и обрыв
/// соединения (`UnexpectedEof` — защита от truncation-атаки). На
/// EOF-фреймировании длина тела неизвестна в принципе, поэтому усечение
/// неотличимо от нормы, и байты, прочитанные до обрыва, — это и есть тело;
/// выбрасывать их из-за способа закрытия соединения нельзя (BUG-792: wptserve
/// отдаёт close-delimited ответы без `close_notify`, из-за чего весь
/// `.https.`-корпус WPT приходил в движок как сетевая ошибка). На Length и
/// chunked послабление не действует: недобор байт там доказуемое усечение.
pub(crate) fn read_body_to_eof<R: BufRead>(reader: &mut R) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; STREAM_BODY_READ];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(buf),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(buf),
            Err(e) => return Err(Error::Network(format!("read body: {e}"))),
        }
    }
}

/// Прочитать framed-тело целиком (без content-encoding decode). Эквивалент
/// body-секции `read_response`, параметризованный уже определённым `framing`.
pub(crate) fn read_framed_buffered<R: BufRead>(reader: &mut R, framing: BodyFraming) -> Result<Vec<u8>> {
    match framing {
        BodyFraming::Chunked => read_chunked(reader),
        BodyFraming::Length(len) => {
            let mut buf = vec![0u8; len];
            reader
                .read_exact(&mut buf)
                .map_err(|e| Error::Network(format!("read body: {e}")))?;
            Ok(buf)
        }
        BodyFraming::Eof => read_body_to_eof(reader),
    }
}

/// Streaming-вариант [`read_response`]: тело читается с сокета порциями, и
/// каждая ДЕКОДИРОВАННАЯ порция передаётся в `sink` — это позволяет shell
/// начать парсинг/раскладку HTML до полного скачивания (PH1-2a).
///
/// `sink` вызывается ТОЛЬКО для финального 2xx-ответа. Для 3xx/4xx/5xx тело
/// читается буферизованно, sink не трогается (caller отбросит тело или пойдёт
/// за redirect). Возвращаемый `body` — СЫРОЙ (как в `read_response`): caller
/// снимает content-encoding через `apply_content_encoding`. sink получает уже
/// декодированные байты. Для сжатого тела decode выполняется дважды (streaming
/// preview + финальный авторитетный) — осознанный размен на простоту: документ
/// декодируется один раз за навигацию.
pub(crate) fn read_response_streamed(conn: &mut Connection, sink: ChunkSink<'_>) -> Result<Response> {
    let (status, headers, server_wants_close) = read_head(conn)?;

    let is_chunked = header_value(&headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false);
    let content_length =
        header_value(&headers, "content-length").and_then(|v| v.trim().parse::<usize>().ok());

    // RFC 7230 §3.3.3 (7): neither chunked nor Content-Length → read-to-EOF
    // framing applies unconditionally, not gated on an explicit `Connection:
    // close` header (see `read_response`'s matching comment — found running
    // P2-wpt S4 against wptserve, which omits that header).
    let framing = if is_chunked {
        BodyFraming::Chunked
    } else if let Some(len) = content_length {
        BodyFraming::Length(len)
    } else if status == 204 || status == 304 {
        BodyFraming::Length(0)
    } else {
        BodyFraming::Eof
    };

    let stream = (200..=299).contains(&status);
    let decode = if stream {
        detect_stream_decode(&headers)
    } else {
        StreamDecode::Unsupported
    };

    let mut raw = Vec::new();
    let read_err: Option<Error> = if !stream || decode == StreamDecode::Unsupported {
        // Не-2xx или неинкрементальная кодировка: буферизованное чтение, sink молчит.
        match read_framed_buffered(&mut conn.reader, framing) {
            Ok(b) => {
                raw = b;
                None
            }
            Err(e) => Some(e),
        }
    } else if decode == StreamDecode::Identity {
        let mut body = BodyReader::new(&mut conn.reader, framing);
        let mut buf = [0u8; STREAM_BODY_READ];
        let mut err = None;
        loop {
            match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&buf[..n]);
                    sink(&buf[..n]);
                }
                Err(e) => {
                    err = Some(Error::Network(format!("read body: {e}")));
                    break;
                }
            }
        }
        err
    } else {
        // Сжатое тело: tee сохраняет сырые байты, streaming-декодер выдаёт preview.
        let mut body = BodyReader::new(&mut conn.reader, framing);
        let mut tee = TeeReader {
            inner: &mut body,
            captured: &mut raw,
        };
        let mut err = None;
        {
            let mut dec: Box<dyn Read + '_> = match decode {
                StreamDecode::Brotli => {
                    Box::new(brotli_decompressor::Decompressor::new(&mut tee, STREAM_BODY_READ))
                }
                StreamDecode::Gzip => Box::new(flate2::read::MultiGzDecoder::new(&mut tee)),
                StreamDecode::Deflate => Box::new(flate2::read::ZlibDecoder::new(&mut tee)),
                StreamDecode::Identity | StreamDecode::Unsupported => unreachable!(),
            };
            let mut buf = [0u8; STREAM_BODY_READ];
            loop {
                match dec.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => sink(&buf[..n]),
                    Err(e) => {
                        err = Some(Error::Network(format!("decode body: {e}")));
                        break;
                    }
                }
            }
        }
        // Дочитать остаток framed-тела (декодер мог остановиться на конце
        // brotli/gzip-потока раньше Content-Length), чтобы соединение было
        // полностью прочитано и `raw` содержал всё сырое тело.
        if err.is_none() {
            let mut buf = [0u8; STREAM_BODY_READ];
            loop {
                match tee.read(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        err = Some(Error::Network(format!("read body: {e}")));
                        break;
                    }
                }
            }
        }
        err
    };

    if let Some(e) = read_err {
        conn.closed = true;
        return Err(e);
    }
    if server_wants_close || matches!(framing, BodyFraming::Eof) {
        conn.closed = true;
    }
    Ok(Response {
        status,
        headers,
        body: raw,
    })
}
