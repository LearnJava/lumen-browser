//! HTTP `Content-Encoding: gzip` / `deflate` декодеры — реализации
//! `ContentDecoder` поверх `flate2` (provisional accelerator, §5 lumen-plan).
//!
//! gzip (RFC 1952) и deflate (RFC 1951 / zlib-обёртка RFC 1950) — два самых
//! распространённых HTTP-кодирования наряду с brotli. Без них Lumen вынужден
//! объявлять только `Accept-Encoding: br`, и любой сервер/CDN, отдающий gzip
//! (а это большинство реального веба для совместимости), либо переключается на
//! identity (медленнее), либо — если игнорирует Accept-Encoding — отдаёт gzip,
//! который `apply_content_encoding` не сможет снять. RP-3 закрывает это.
//!
//! Это не safe-критичные форматы (декомпрессия — чистая алгебра без секретов),
//! свой парсер когда-нибудь возможен, но graduation criterion фактически
//! «никогда»: форматы стабильны с 1990-х.

use std::io::Read;

use flate2::read::{MultiGzDecoder, ZlibDecoder};
use lumen_core::error::{Error, Result};
use lumen_core::ext::ContentDecoder;

/// `ContentDecoder` для `Content-Encoding: gzip`. Stateless: один экземпляр
/// можно использовать concurrently для нескольких ответов.
///
/// Использует `MultiGzDecoder` (а не `GzDecoder`), чтобы корректно читать
/// конкатенированные gzip-члены — некоторые серверы стримят тело несколькими
/// gzip-блоками подряд; одиночный `GzDecoder` остановился бы на первом.
#[derive(Debug, Default, Clone, Copy)]
pub struct GzipContentDecoder;

impl GzipContentDecoder {
    /// Новый декодер. Состояния нет — это zero-sized type.
    pub const fn new() -> Self {
        Self
    }
}

impl ContentDecoder for GzipContentDecoder {
    fn encoding(&self) -> &'static str {
        "gzip"
    }

    fn decode(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = MultiGzDecoder::new(input);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| Error::Other(format!("gzip decode failed: {e}")))?;
        Ok(out)
    }
}

/// `ContentDecoder` для `Content-Encoding: deflate`. Stateless.
///
/// Спецификация (RFC 7230) предписывает zlib-обёрнутый deflate (RFC 1950),
/// но исторически часть серверов отдаёт «сырой» deflate (RFC 1951) без zlib-
/// заголовка. Реальные браузеры терпимы к обоим: пробуем zlib, при неудаче
/// откатываемся на raw. Так как тело декодируется целиком (не streaming),
/// повторный проход по тем же байтам безопасен.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeflateContentDecoder;

impl DeflateContentDecoder {
    /// Новый декодер. Состояния нет — это zero-sized type.
    pub const fn new() -> Self {
        Self
    }
}

impl ContentDecoder for DeflateContentDecoder {
    fn encoding(&self) -> &'static str {
        "deflate"
    }

    fn decode(&self, input: &[u8]) -> Result<Vec<u8>> {
        // Сначала zlib-обёртка (RFC 1950) — спецификационно корректный вариант.
        let mut zlib = ZlibDecoder::new(input);
        let mut out = Vec::new();
        match zlib.read_to_end(&mut out) {
            Ok(_) => Ok(out),
            Err(zlib_err) => {
                // Откат на raw deflate (RFC 1951) для серверов без zlib-заголовка.
                out.clear();
                let mut raw = flate2::read::DeflateDecoder::new(input);
                raw.read_to_end(&mut out).map_err(|raw_err| {
                    Error::Other(format!(
                        "deflate decode failed (zlib: {zlib_err}; raw: {raw_err})"
                    ))
                })?;
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
    use flate2::Compression;
    use std::io::Write;

    fn gzip_encode(data: &[u8]) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn zlib_encode(data: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn raw_deflate_encode(data: &[u8]) -> Vec<u8> {
        let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn gzip_round_trip_ascii() {
        let payload = b"Hello, World!";
        let encoded = gzip_encode(payload);
        let d = GzipContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), payload);
    }

    /// Кириллический UTF-8 round-trip. «Русский — first-class» (принцип №7).
    #[test]
    fn gzip_round_trip_cyrillic() {
        let payload = "Привет, мир!".as_bytes();
        let encoded = gzip_encode(payload);
        let d = GzipContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), payload);
    }

    #[test]
    fn gzip_empty_round_trip() {
        let encoded = gzip_encode(b"");
        let d = GzipContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), Vec::<u8>::new());
    }

    /// Конкатенированные gzip-члены должны читаться целиком (MultiGzDecoder).
    #[test]
    fn gzip_multi_member() {
        let mut encoded = gzip_encode(b"first");
        encoded.extend_from_slice(&gzip_encode(b"second"));
        let d = GzipContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), b"firstsecond");
    }

    #[test]
    fn gzip_invalid_errors() {
        let d = GzipContentDecoder::new();
        let err = d.decode(&[0xff_u8; 32]).expect_err("must reject invalid gzip");
        match err {
            Error::Other(msg) => assert!(msg.contains("gzip decode failed")),
            other => panic!("expected Error::Other, got {other:?}"),
        }
    }

    #[test]
    fn deflate_zlib_round_trip() {
        let payload = b"Hello, World!";
        let encoded = zlib_encode(payload);
        let d = DeflateContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), payload);
    }

    /// Сырой (без zlib-заголовка) deflate должен сниматься через fallback.
    #[test]
    fn deflate_raw_fallback_round_trip() {
        let payload = b"raw deflate body";
        let encoded = raw_deflate_encode(payload);
        let d = DeflateContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), payload);
    }

    #[test]
    fn deflate_cyrillic_round_trip() {
        let payload = "Привет, мир!".as_bytes();
        let encoded = zlib_encode(payload);
        let d = DeflateContentDecoder::new();
        assert_eq!(d.decode(&encoded).unwrap(), payload);
    }

    #[test]
    fn deflate_invalid_errors() {
        let d = DeflateContentDecoder::new();
        let err = d.decode(&[0xff_u8; 4]).expect_err("must reject invalid deflate");
        match err {
            Error::Other(msg) => assert!(msg.contains("deflate decode failed")),
            other => panic!("expected Error::Other, got {other:?}"),
        }
    }

    #[test]
    fn encoding_names() {
        assert_eq!(GzipContentDecoder::new().encoding(), "gzip");
        assert_eq!(DeflateContentDecoder::new().encoding(), "deflate");
    }

    #[test]
    fn is_dyn_safe_and_send_sync() {
        fn check_dyn(_: &dyn ContentDecoder) {}
        fn check_ss<T: Send + Sync>() {}
        check_dyn(&GzipContentDecoder::new());
        check_dyn(&DeflateContentDecoder::new());
        check_ss::<GzipContentDecoder>();
        check_ss::<DeflateContentDecoder>();
    }
}
