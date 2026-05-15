//! HTTP `Content-Encoding: br` декодер — реализация `ContentDecoder`
//! поверх `brotli-decompressor` (provisional accelerator, §5 lumen-plan).
//!
//! Brotli (RFC 7932) поддерживается 60–70% реального веба и почти всегда
//! предпочитается серверами при `Accept-Encoding: br`. Это не safe-критичный
//! формат (декомпрессия чистая алгебра без секретов), поэтому свой парсер
//! когда-нибудь возможен, но graduation criterion фактически «никогда» —
//! формат стабилен с 2016 года.

use std::io::Read;

use lumen_core::error::{Error, Result};
use lumen_core::ext::ContentDecoder;

/// Buffer-size для внутреннего ring-buffer-а `brotli-decompressor`. 4096 —
/// общий рекомендованный размер; больший buffer бессмысленно увеличивает
/// peak-RSS на короткие ответы, меньший — добавляет overhead на множественные
/// `read`-ы.
const READ_BUFFER_SIZE: usize = 4096;

/// `ContentDecoder` для `Content-Encoding: br`. Stateless: один экземпляр
/// можно использовать concurrently для нескольких ответов.
#[derive(Debug, Default, Clone, Copy)]
pub struct BrotliContentDecoder;

impl BrotliContentDecoder {
    pub const fn new() -> Self {
        Self
    }
}

impl ContentDecoder for BrotliContentDecoder {
    fn encoding(&self) -> &'static str {
        "br"
    }

    fn decode(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut decompressor =
            brotli_decompressor::Decompressor::new(input, READ_BUFFER_SIZE);
        let mut out = Vec::new();
        decompressor
            .read_to_end(&mut out)
            .map_err(|e| Error::Other(format!("brotli decode failed: {e}")))?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимальный валидный brotli stream — пустое содержимое. Получен через
    /// эталонный энкодер (`brotli` CLI из brotli-проекта Google): `echo -n "" | brotli -c`.
    #[test]
    fn empty_stream_decodes_to_empty() {
        let empty_brotli = &[0x3f_u8];
        let d = BrotliContentDecoder::new();
        let out = d.decode(empty_brotli).expect("decode empty");
        assert_eq!(out, Vec::<u8>::new());
    }

    /// Round-trip test vector от эталонного энкодера: `echo -n "Hello, World!" | brotli -c`.
    #[test]
    fn known_vector_hello_decodes() {
        let payload: [u8; 17] = [
            0x0f, 0x06, 0x80, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x57, 0x6f,
            0x72, 0x6c, 0x64, 0x21, 0x03,
        ];
        let d = BrotliContentDecoder::new();
        let out = d.decode(&payload).expect("decode hello");
        assert_eq!(out, b"Hello, World!");
    }

    /// Кириллический UTF-8 round-trip: `echo -n "Привет, мир!" | brotli -c`.
    /// «Русский — first-class» (принцип №7).
    #[test]
    fn known_vector_cyrillic_decodes() {
        let payload: [u8; 25] = [
            0x0f, 0x0a, 0x80, 0xd0, 0x9f, 0xd1, 0x80, 0xd0, 0xb8, 0xd0, 0xb2, 0xd0,
            0xb5, 0xd1, 0x82, 0x2c, 0x20, 0xd0, 0xbc, 0xd0, 0xb8, 0xd1, 0x80, 0x21,
            0x03,
        ];
        let d = BrotliContentDecoder::new();
        let out = d.decode(&payload).expect("decode cyrillic");
        assert_eq!(String::from_utf8(out).unwrap(), "Привет, мир!");
    }

    #[test]
    fn invalid_stream_returns_error() {
        // Все нули — невалидный brotli-stream после первого header-байта.
        let bad = [0xff_u8; 32];
        let d = BrotliContentDecoder::new();
        let err = d.decode(&bad).expect_err("must reject invalid brotli");
        match err {
            Error::Other(msg) => assert!(
                msg.contains("brotli decode failed"),
                "unexpected error message: {msg}"
            ),
            other => panic!("expected Error::Other, got {other:?}"),
        }
    }

    #[test]
    fn empty_input_returns_error() {
        // Пустой вход — невалидный brotli (заголовок отсутствует).
        let d = BrotliContentDecoder::new();
        let err = d.decode(&[]).expect_err("empty input must error");
        match err {
            Error::Other(msg) => assert!(msg.contains("brotli decode failed")),
            other => panic!("expected Error::Other, got {other:?}"),
        }
    }

    #[test]
    fn encoding_name_is_br() {
        let d = BrotliContentDecoder::new();
        assert_eq!(d.encoding(), "br");
    }

    #[test]
    fn is_dyn_safe() {
        fn check(_: &dyn ContentDecoder) {}
        check(&BrotliContentDecoder::new());
    }

    #[test]
    fn is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<BrotliContentDecoder>();
    }
}
