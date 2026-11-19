//! HTTP `Content-Encoding: zstd` декодер — реализация `ContentDecoder`
//! поверх `ruzstd` (pure-Rust, RFC 8878).
//!
//! Chrome 157 шлёт `Accept-Encoding: gzip, deflate, br, zstd` — без zstd
//! декодера Lumen не может честно рекламировать полный набор кодировок
//! Chrome (BUG-1826). `ruzstd` — provisional accelerator (§5), trait-anchor
//! `lumen_core::ext::ContentDecoder`; декомпрессия только на чтение,
//! компрессор не используется.

use std::io::Read;

use lumen_core::error::{Error, Result};
use lumen_core::net_error::NetErrorCode;
use lumen_core::ext::ContentDecoder;

/// `ContentDecoder` для `Content-Encoding: zstd`. Stateless: один экземпляр
/// можно использовать concurrently для нескольких ответов.
#[derive(Debug, Default, Clone, Copy)]
pub struct ZstdContentDecoder;

impl ZstdContentDecoder {
    /// Новый декодер. Состояния нет — это zero-sized type.
    pub const fn new() -> Self {
        Self
    }
}

impl ContentDecoder for ZstdContentDecoder {
    fn encoding(&self) -> &'static str {
        "zstd"
    }

    fn decode(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(input)
            .map_err(|e| Error::net_failure(NetErrorCode::ContentDecodingFailed, format!("zstd decode init failed: {e}")))?;
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| Error::net_failure(NetErrorCode::ContentDecodingFailed, format!("zstd decode failed: {e}")))?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Эталонный zstd-фрейм, сжатый системным `zstd` уровнем 3 (см. историю
    /// коммита): содержимое — `hello zstd`.
    #[test]
    fn decodes_reference_frame() {
        // Сгенерировано `zlib.zstdCompressSync("hello zstd\n")` (Node 26,
        // системный zstd) — независимый эталон, не ruzstd-компрессор.
        let frame: &[u8] = &[
            0x28, 0xb5, 0x2f, 0xfd, 0x20, 0x0b, 0x59, 0x00, 0x00, 0x68, 0x65, 0x6c,
            0x6c, 0x6f, 0x20, 0x7a, 0x73, 0x74, 0x64, 0x0a,
        ];
        let out = ZstdContentDecoder::new().decode(frame).expect("decode");
        assert_eq!(out, b"hello zstd\n");
    }

    #[test]
    fn rejects_garbage() {
        assert!(ZstdContentDecoder::new().decode(b"not a zstd frame").is_err());
    }
}
