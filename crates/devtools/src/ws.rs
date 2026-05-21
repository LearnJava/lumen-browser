//! WebSocket codec — RFC 6455.
//!
//! Только то, что нужно DevTools: HTTP Upgrade handshake + text-frame
//! read/write. Binary, fragmented, compressed frames не поддерживаются —
//! CDP использует исключительно text frames.

use std::io::{self, Read, Write};

use lumen_core::hash::ws_accept_key;

#[derive(Debug)]
pub enum WsError {
    Io(io::Error),
    /// HTTP-запрос не является корректным WebSocket Upgrade.
    BadHandshake(&'static str),
    /// Получен unsupported opcode (напр. binary).
    UnsupportedOpcode(u8),
    /// Клиент закрыл соединение (Close frame или EOF).
    Closed,
}

impl From<io::Error> for WsError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::BadHandshake(r) => write!(f, "bad handshake: {r}"),
            Self::UnsupportedOpcode(op) => write!(f, "unsupported opcode 0x{op:x}"),
            Self::Closed => write!(f, "connection closed"),
        }
    }
}

/// Прочитать HTTP Upgrade запрос, проверить заголовки, отправить 101.
///
/// После успешного возврата `stream` переключён в WebSocket-режим.
pub fn upgrade<S: Read + Write>(stream: &mut S) -> Result<(), WsError> {
    // Читаем HTTP-запрос побайтово до CRLF CRLF (максимум 8 KB).
    let mut buf = Vec::with_capacity(512);
    let mut prev = [0u8; 3];
    loop {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b)?;
        buf.push(b[0]);
        // Обнаружение \r\n\r\n
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 8192 {
            return Err(WsError::BadHandshake("request too large"));
        }
        let _ = prev;
        prev = [prev[1], prev[2], b[0]];
    }

    let text = std::str::from_utf8(&buf).map_err(|_| WsError::BadHandshake("non-utf8"))?;

    // Извлечь Sec-WebSocket-Key
    let key = extract_header(text, "Sec-WebSocket-Key:")
        .ok_or(WsError::BadHandshake("missing Sec-WebSocket-Key"))?
        .trim();

    // Проверить Upgrade: websocket
    let upgrade_hdr = extract_header(text, "Upgrade:")
        .ok_or(WsError::BadHandshake("missing Upgrade header"))?
        .trim()
        .to_ascii_lowercase();
    if upgrade_hdr != "websocket" {
        return Err(WsError::BadHandshake("Upgrade is not websocket"));
    }

    let accept = ws_accept_key(key);

    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\
         \r\n"
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn extract_header<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    for line in text.lines() {
        if line.to_ascii_lowercase().starts_with(&name.to_ascii_lowercase()) {
            return Some(&line[name.len()..]);
        }
    }
    None
}

/// Прочитать один WebSocket фрейм (RFC 6455 §5.2).
///
/// Возвращает payload text фрейма. Close и Ping/Pong обрабатываются
/// прозрачно (Pong отправляется автоматически). Binary frames → ошибка.
pub fn read_text_frame<S: Read + Write>(stream: &mut S) -> Result<String, WsError> {
    loop {
        let frame = read_raw_frame(stream)?;
        match frame.opcode {
            0x1 => {
                // Text frame
                return String::from_utf8(frame.payload)
                    .map_err(|_| WsError::BadHandshake("non-utf8 payload"));
            }
            0x8 => return Err(WsError::Closed),
            0x9 => {
                // Ping — отвечаем Pong
                write_raw_frame(stream, 0xA, &frame.payload)?;
            }
            0xA => {} // Pong — игнорируем
            op => return Err(WsError::UnsupportedOpcode(op)),
        }
    }
}

/// Отправить text фрейм (server→client, без маски).
pub fn write_text_frame<S: Write>(stream: &mut S, text: &str) -> Result<(), WsError> {
    write_raw_frame(stream, 0x1, text.as_bytes())
}

struct RawFrame {
    opcode: u8,
    payload: Vec<u8>,
}

fn read_raw_frame<S: Read>(stream: &mut S) -> Result<RawFrame, WsError> {
    let mut hdr = [0u8; 2];
    stream.read_exact(&mut hdr)?;
    let _fin = (hdr[0] & 0x80) != 0;
    let opcode = hdr[0] & 0x0F;
    let masked = (hdr[1] & 0x80) != 0;
    let len7 = (hdr[1] & 0x7F) as u64;

    let payload_len: u64 = match len7 {
        126 => {
            let mut b = [0u8; 2];
            stream.read_exact(&mut b)?;
            u16::from_be_bytes(b) as u64
        }
        127 => {
            let mut b = [0u8; 8];
            stream.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => n,
    };

    // Защита от gigantic frames (DevTools сообщения < 1 MB)
    if payload_len > 1_048_576 {
        return Err(WsError::BadHandshake("frame too large"));
    }

    let mask = if masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len as usize];
    stream.read_exact(&mut payload)?;

    if let Some(mask) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }

    Ok(RawFrame { opcode, payload })
}

fn write_raw_frame<S: Write>(stream: &mut S, opcode: u8, data: &[u8]) -> Result<(), WsError> {
    let mut frame = Vec::with_capacity(10 + data.len());
    frame.push(0x80 | opcode); // FIN + opcode
    let len = data.len();
    if len < 126 {
        frame.push(len as u8); // no mask bit (server→client)
    } else if len < 65536 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(data);
    stream.write_all(&frame)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Минимальный In-memory Read+Write буфер для тестов.
    struct MockStream {
        read_buf: std::io::Cursor<Vec<u8>>,
        write_buf: Vec<u8>,
    }

    impl MockStream {
        fn new(input: Vec<u8>) -> Self {
            Self { read_buf: std::io::Cursor::new(input), write_buf: Vec::new() }
        }
    }

    impl Read for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.read_buf.read(buf)
        }
    }
    impl Write for MockStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.write_buf.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn make_text_frame(payload: &str, masked: bool) -> Vec<u8> {
        let data = payload.as_bytes();
        let mut frame = Vec::new();
        frame.push(0x81); // FIN + text opcode
        let len = data.len();
        if masked {
            frame.push(0x80 | len as u8);
            let mask = [0x37, 0xfa, 0x21, 0x3d];
            frame.extend_from_slice(&mask);
            for (i, b) in data.iter().enumerate() {
                frame.push(b ^ mask[i % 4]);
            }
        } else {
            frame.push(len as u8);
            frame.extend_from_slice(data);
        }
        frame
    }

    #[test]
    fn read_unmasked_text_frame() {
        let frame = make_text_frame("hello", false);
        let mut stream = MockStream::new(frame);
        let text = read_text_frame(&mut stream).unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn read_masked_text_frame() {
        let frame = make_text_frame("hello", true);
        let mut stream = MockStream::new(frame);
        let text = read_text_frame(&mut stream).unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn write_text_frame_format() {
        let mut stream = MockStream::new(vec![]);
        write_text_frame(&mut stream, "hi").unwrap();
        // FIN+text=0x81, len=2, payload
        assert_eq!(&stream.write_buf, &[0x81, 0x02, b'h', b'i']);
    }

    #[test]
    fn close_frame_returns_closed_error() {
        let frame = vec![0x88, 0x00]; // FIN + close, no payload
        let mut stream = MockStream::new(frame);
        assert!(matches!(read_text_frame(&mut stream), Err(WsError::Closed)));
    }

    #[test]
    fn ws_accept_key_rfc6455_example() {
        // RFC 6455 §1.3 example
        let accept = ws_accept_key("dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
