//! WebP декодер (VP8 lossy + VP8L lossless) поверх `image-webp` 0.2.
//!
//! Поддерживает:
//! - VP8 (lossy, baseline) → YUV → RGBA8
//! - VP8L (lossless) → RGBA8
//! - Анимированные WebP (ANIM chunk) — декодируется только первый кадр
//!   (Phase 0: full animation — Wave 3).
//!
//! Не поддерживается в Phase 0: ICC colour management, EXIF/XMP metadata,
//! background-colour compositing для анимации.

use std::io::{BufReader, Cursor};

use image_webp::WebPDecoder;

/// Начало WebP-контейнера: первые 4 байта файла.
pub const WEBP_RIFF: [u8; 4] = *b"RIFF";

/// Магическое слово на байтах 8–11 WebP-файла: `WEBP`.
pub const WEBP_TAG: [u8; 4] = *b"WEBP";

/// Ошибка декодирования WebP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebpError(pub String);

impl core::fmt::Display for WebpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WebpError {}

/// Проверяет WebP-сигнатуру без полной валидации.
///
/// Формат контейнера RIFF: байты 0–3 = `RIFF`, байты 8–11 = `WEBP`.
/// Размер поля (байты 4–7) не проверяется.
#[must_use]
pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes[..4] == WEBP_RIFF && bytes[8..12] == WEBP_TAG
}

/// Декодирует WebP-файл в RGBA8 (4 байта на пиксель, row-major).
///
/// Возвращает `(ширина, высота, rgba8_data)`.
///
/// Phase 0: анимированные WebP — читается только первый кадр (frame 0);
/// полная анимация запланирована на Wave 3.
///
/// # Errors
/// [`WebpError`] с текстом диагностики.
pub fn decode_webp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), WebpError> {
    let reader = BufReader::new(Cursor::new(bytes));
    let mut decoder =
        WebPDecoder::new(reader).map_err(|e| WebpError(format!("WebP header: {e}")))?;

    let (width, height) = decoder.dimensions();
    let has_alpha = decoder.has_alpha();

    let buf_size = decoder
        .output_buffer_size()
        .ok_or_else(|| WebpError("WebP: нулевые размеры изображения".to_string()))?;

    let mut raw = vec![0u8; buf_size];
    decoder
        .read_image(&mut raw)
        .map_err(|e| WebpError(format!("WebP decode: {e}")))?;

    // image-webp отдаёт RGB8 если нет альфа-канала; нормализуем в RGBA8.
    let rgba8 = if has_alpha {
        raw
    } else {
        let mut out = Vec::with_capacity(width as usize * height as usize * 4);
        for rgb in raw.chunks_exact(3) {
            out.extend_from_slice(rgb);
            out.push(255);
        }
        out
    };

    Ok((width, height, rgba8))
}

/// Реализация [`lumen_core::ext::ImageDecoder`] для WebP.
///
/// Подключается в `lumen-image::decode()` как дополнительный диспетчер и
/// регистрируется в `supported_mime_types()` для фильтрации `<source type>`.
pub struct WebpImageDecoder;

impl lumen_core::ext::ImageDecoder for WebpImageDecoder {
    fn format_name(&self) -> &'static str {
        "webp"
    }

    fn sniff(&self, bytes: &[u8]) -> bool {
        is_webp(bytes)
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["image/webp"]
    }

    fn decode_rgba8(
        &self,
        bytes: &[u8],
    ) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        decode_webp(bytes).map_err(|e| e.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::ImageDecoder as _;

    // Minimal 1×1 RGBA WebP (VP8L lossless) generated with WebPEncoder.
    // Created once via `generate_fixture_webp()` below and hard-coded as bytes
    // so the test-suite has zero external file dependencies.
    fn make_1x1_rgba_webp(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        use image_webp::{ColorType, WebPEncoder};
        let mut out = Vec::new();
        WebPEncoder::new(&mut out)
            .encode(&[r, g, b, a], 1, 1, ColorType::Rgba8)
            .expect("encode 1×1 WebP");
        out
    }

    fn make_4x4_rgb_webp() -> Vec<u8> {
        use image_webp::{ColorType, WebPEncoder};
        // 4×4 solid green (no alpha)
        let data: Vec<u8> = (0..16).flat_map(|_| [0u8, 200u8, 0u8]).collect();
        let mut out = Vec::new();
        WebPEncoder::new(&mut out)
            .encode(&data, 4, 4, ColorType::Rgb8)
            .expect("encode 4×4 RGB WebP");
        out
    }

    #[test]
    fn signature_positive() {
        let webp = make_1x1_rgba_webp(255, 0, 0, 255);
        assert!(is_webp(&webp), "закодированный WebP должен иметь RIFF/WEBP сигнатуру");
    }

    #[test]
    fn signature_negative_png() {
        let png_sig = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert!(!is_webp(png_sig));
    }

    #[test]
    fn signature_too_short() {
        assert!(!is_webp(b"RIFF"));
        assert!(!is_webp(b""));
    }

    #[test]
    fn decode_rgba_roundtrip() {
        let webp = make_1x1_rgba_webp(200, 100, 50, 128);
        let (w, h, rgba) = decode_webp(&webp).expect("decode 1×1 RGBA WebP");
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(rgba.len(), 4);
        // VP8L lossless → pixel-exact
        assert_eq!(rgba[0], 200, "R");
        assert_eq!(rgba[1], 100, "G");
        assert_eq!(rgba[2], 50, "B");
        assert_eq!(rgba[3], 128, "A");
    }

    #[test]
    fn decode_rgb_expands_to_rgba8() {
        let webp = make_4x4_rgb_webp();
        let (w, h, rgba) = decode_webp(&webp).expect("decode 4×4 RGB WebP");
        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert_eq!(rgba.len(), 4 * 4 * 4, "RGBA8: 4 bytes/pixel");
        // Every 4th byte (alpha) must be 255
        for i in (3..rgba.len()).step_by(4) {
            assert_eq!(rgba[i], 255, "alpha byte at {i} должен быть 255");
        }
    }

    #[test]
    fn decode_invalid_returns_error() {
        let result = decode_webp(b"not a webp at all");
        assert!(result.is_err(), "невалидные байты должны вернуть ошибку");
    }

    #[test]
    fn image_decoder_trait_sniff() {
        let webp = make_1x1_rgba_webp(0, 0, 255, 255);
        let dec = WebpImageDecoder;
        assert!(dec.sniff(&webp));
        assert!(!dec.sniff(b"not webp"));
    }

    #[test]
    fn image_decoder_trait_mime_types() {
        let dec = WebpImageDecoder;
        assert!(dec.mime_types().contains(&"image/webp"));
    }

    #[test]
    fn image_decoder_trait_decode_rgba8() {
        let webp = make_1x1_rgba_webp(10, 20, 30, 40);
        let dec = WebpImageDecoder;
        let (w, h, rgba) = dec.decode_rgba8(&webp).expect("decode via trait");
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba.len(), 4);
    }
}
