//! JPEG XL декодер (ISO/IEC 18181), через чистый-Rust `jxl-oxide`.
//!
//! Phase 0 ограничения (симметрично `avif/mod.rs`):
//! - Только первый (и единственный) кадр — анимация не декодируется.
//! - CMYK/CMYKA (`PixelFormat::Cmyk`/`Cmyka`) не поддержан — [`JxlError::Decode`]
//!   (редкий путь для web JPEG XL; типографский CMYK не встречается в браузере).
//! - ICC-профиль не извлекается (icc_profile поле в `Image` → None), как у AVIF.
//!
//! `jxl-oxide` не под feature — весь стек `jxl-*` чистый Rust без cmake/meson/nasm
//! (GAP-avif срез 3, `docs/tasks/ph3-avif.md`), в отличие от AVIF-декодера рядом.

use core::fmt;
use jxl_oxide::JxlImage;

/// JPEG XL magic bytes (naked): FF 0A.
const JXL_NAKED_MAGIC: [u8; 2] = [0xFF, 0x0A];

/// JPEG XL ISOBMFF container signature (box type).
const JXL_ISOBMFF_BOX_TYPE: [u8; 4] = *b"jxl ";

/// Ошибка декодирования JPEG XL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JxlError {
    /// Байты не являются валидным JPEG XL (ни naked `FF 0A`, ни ISOBMFF `jxl `).
    InvalidSignature,
    /// Сигнатура распознана, но декодер вернул ошибку (либо формат не поддержан).
    Decode(String),
}

impl fmt::Display for JxlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "не JPEG XL: naked/ISOBMFF сигнатура не найдена"),
            Self::Decode(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for JxlError {}

/// Detects JPEG XL image format.
///
/// Returns true if the byte sequence matches:
/// - Naked format: `FF 0A` header
/// - ISOBMFF container: an `ftyp` box (major or compatible brand `jxl `)
///
/// A real ISOBMFF-boxed JPEG XL is not just an `ftyp` box at offset 0: encoders
/// (e.g. `pillow-jxl-plugin`'s `use_container=True`) emit a leading 12-byte
/// `JXL ` *signature box* (`00 00 00 0C 4A 58 4C 20 0D 0A 87 0A`) before `ftyp`,
/// per ISO/IEC 18181-2. So this walks boxes sequentially instead of assuming
/// `ftyp` sits at a fixed offset — an earlier fixed-offset version missed every
/// real-world container sample (found while adding [`decode_jxl`]'s tests).
#[must_use]
pub fn is_jxl(bytes: &[u8]) -> bool {
    if bytes.len() >= 2 && bytes[..2] == JXL_NAKED_MAGIC {
        return true;
    }

    let mut offset = 0usize;
    while offset + 8 <= bytes.len() {
        let box_size =
            u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
        let box_type = &bytes[offset + 4..offset + 8];

        if box_type == b"ftyp" {
            let box_end = if box_size == 0 { bytes.len() } else { (offset + box_size).min(bytes.len()) };
            let brand_start = offset + 8;
            // Major brand (4 bytes), then minor version (4 bytes), then
            // compatible brands (4 bytes each) up to the box end.
            let mut i = brand_start;
            while i + 4 <= box_end {
                if bytes[i..i + 4] == JXL_ISOBMFF_BOX_TYPE {
                    return true;
                }
                i += 4;
            }
            return false;
        }

        // 0 means "box extends to end of file" (no further boxes to scan);
        // a size below the 8-byte header is malformed — stop rather than loop.
        if box_size < 8 {
            break;
        }
        offset += box_size;
    }

    false
}

/// Декодирует JPEG XL-файл в RGBA8 (4 байта на пиксель, row-major).
///
/// Возвращает `(ширина, высота, rgba8_данные)`. Grayscale/RGB без альфы
/// дополняются альфой 255 — `Image` в `lumen_image` всегда хранит `Rgba8`.
///
/// # Errors
/// - [`JxlError::InvalidSignature`] — сигнатура JPEG XL не найдена.
/// - [`JxlError::Decode`] — `jxl-oxide` не смог разобрать/декодировать кадр,
///   либо формат пикселей CMYK/CMYKA (не поддержан, Phase 0).
pub fn decode_jxl(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), JxlError> {
    if !is_jxl(bytes) {
        return Err(JxlError::InvalidSignature);
    }

    let image = JxlImage::read_with_defaults(bytes)
        .map_err(|e| JxlError::Decode(format!("jxl-oxide: {e}")))?;
    let pixel_format = image.pixel_format();
    if pixel_format.has_black() {
        return Err(JxlError::Decode(
            "JPEG XL: CMYK/CMYKA не поддерживается (Phase 0)".to_string(),
        ));
    }

    let render = image
        .render_frame(0)
        .map_err(|e| JxlError::Decode(format!("jxl-oxide: {e}")))?;
    let mut stream = render.stream();
    let width = stream.width();
    let height = stream.height();
    let channels = stream.channels() as usize;
    if channels == 0 {
        return Err(JxlError::Decode("JPEG XL: нулевое число каналов".to_string()));
    }
    let mut buf = vec![0u8; width as usize * height as usize * channels];
    stream.write_to_buffer(&mut buf);

    let is_gray = pixel_format.is_grayscale();
    let has_alpha = pixel_format.has_alpha();
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for px in buf.chunks_exact(channels) {
        let (r, g, b) = if is_gray { (px[0], px[0], px[0]) } else { (px[0], px[1], px[2]) };
        let a = if has_alpha { px[channels - 1] } else { 255 };
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    Ok((width, height, rgba))
}

/// Реализация [`lumen_core::ext::ImageDecoder`] для JPEG XL.
///
/// Регистрируется в диспетчере `lumen_image::decode()` и в
/// `supported_mime_types()` для фильтрации `<source type="image/jxl">`.
pub struct JxlImageDecoder;

impl lumen_core::ext::ImageDecoder for JxlImageDecoder {
    fn format_name(&self) -> &'static str {
        "jxl"
    }

    fn sniff(&self, bytes: &[u8]) -> bool {
        is_jxl(bytes)
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["image/jxl"]
    }

    fn decode_rgba8(&self, bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        decode_jxl(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::ImageDecoder as _;

    /// 4x3 RGB (без альфы), сгенерирован `Pillow` + `pillow-jxl-plugin`
    /// (`Image.new("RGB", ...).save(..., lossless=True)`), закодирован как naked
    /// JPEG XL (без ISOBMFF-контейнера, сигнатура `FF 0A`).
    const JXL_SAMPLE_RGB_4X3: &[u8] = &[
        0xFF, 0x0A, 0x10, 0x30, 0x10, 0x09, 0x08, 0x00, 0x01, 0x00, 0x54, 0x00, 0x4B, 0x18, 0x8B,
        0x15, 0xC2, 0x41, 0x83, 0x1D, 0xBA, 0x40, 0x00, 0x00, 0x23, 0x04, 0x83, 0xBE, 0x90, 0x50,
        0xEE, 0x01, 0x74,
    ];

    /// 3x2 RGBA (с альфой), тем же путём.
    const JXL_SAMPLE_RGBA_3X2: &[u8] = &[
        0xFF, 0x0A, 0x08, 0x40, 0xB0, 0x12, 0x08, 0x00, 0x10, 0x00, 0xAC, 0x00, 0x4B, 0x18, 0x8B,
        0x15, 0xC2, 0x11, 0xA2, 0x1E, 0xC2, 0xF6, 0xBA, 0x80, 0x14, 0x00, 0x85, 0xB2, 0xD1, 0xEB,
        0xFF, 0x56, 0x1E, 0xCD, 0x99, 0x53, 0x1D, 0xFD, 0xD1, 0x19, 0x0A, 0x33, 0x57, 0x47, 0x90,
        0xCD, 0x21, 0x80, 0xB9, 0xD8, 0x98, 0xCE, 0x89, 0x48, 0x01,
    ];

    /// 2x2 RGB, тот же кодек но `use_container=True` в pillow-jxl-plugin —
    /// упакован в ISOBMFF-бокс (`ftyp` brand `jxl `), не naked-поток.
    const JXL_SAMPLE_ISOBMFF_2X2: &[u8] = &[
        0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20, 0x0D, 0x0A, 0x87, 0x0A, 0x00, 0x00, 0x00,
        0x14, 0x66, 0x74, 0x79, 0x70, 0x6A, 0x78, 0x6C, 0x20, 0x00, 0x00, 0x00, 0x00, 0x6A, 0x78,
        0x6C, 0x20, 0x00, 0x00, 0x00, 0x1A, 0x6A, 0x78, 0x6C, 0x63, 0xFF, 0x0A, 0x08, 0x10, 0x10,
        0x09, 0x08, 0x00, 0x01, 0x00, 0x18, 0x00, 0x4B, 0x18, 0x8B, 0x15, 0x82, 0x01,
    ];

    #[test]
    fn is_jxl_detects_real_isobmff_signature_box_container() {
        // Регрессия: до фикса is_jxl() проверял `ftyp` только по фиксированному
        // смещению 4..8 и не находил его за реальной 12-байтной `JXL `
        // signature-боксом, которую эмитят настоящие энкодеры.
        assert!(is_jxl(JXL_SAMPLE_ISOBMFF_2X2));
    }

    #[test]
    fn decode_jxl_real_isobmff_sample() {
        let (w, h, rgba) =
            decode_jxl(JXL_SAMPLE_ISOBMFF_2X2).expect("ISOBMFF-контейнер должен декодироваться так же, как naked");
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba.len(), 2 * 2 * 4);
    }

    #[test]
    fn decode_jxl_real_rgb_sample() {
        let (w, h, rgba) = decode_jxl(JXL_SAMPLE_RGB_4X3).expect("реальный JXL-семпл должен декодироваться");
        assert_eq!((w, h), (4, 3));
        assert_eq!(rgba.len(), 4 * 3 * 4);
        // Непрозрачный источник (RGB без альфы) — альфа-канал должен быть заполнен 255.
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
    }

    #[test]
    fn decode_jxl_real_rgba_sample_preserves_alpha() {
        let (w, h, rgba) = decode_jxl(JXL_SAMPLE_RGBA_3X2).expect("реальный RGBA JXL-семпл должен декодироваться");
        assert_eq!((w, h), (3, 2));
        assert_eq!(rgba.len(), 3 * 2 * 4);
        // Исходный семпл содержит смешанные значения альфы (255, 128, 0, 64, 200, 10) —
        // не все пиксели непрозрачны.
        assert!(rgba.chunks_exact(4).any(|px| px[3] != 255));
    }

    #[test]
    fn decode_jxl_invalid_signature_error_on_non_jxl() {
        let result = decode_jxl(b"not a jxl file at all, definitely not");
        assert_eq!(result, Err(JxlError::InvalidSignature));
    }

    #[test]
    fn decode_jxl_garbage_after_signature_returns_decode_error() {
        let mut bytes = JXL_NAKED_MAGIC.to_vec();
        bytes.extend_from_slice(&[0u8; 16]);
        let result = decode_jxl(&bytes);
        assert!(matches!(result, Err(JxlError::Decode(_))), "ожидался Decode(_), получено {result:?}");
    }

    #[test]
    fn jxl_error_display_invalid_signature() {
        let s = format!("{}", JxlError::InvalidSignature);
        assert!(!s.is_empty());
    }

    #[test]
    fn jxl_error_display_decode() {
        let s = format!("{}", JxlError::Decode("test error".to_string()));
        assert!(s.contains("test error"));
    }

    #[test]
    fn image_decoder_trait_format_name() {
        assert_eq!(JxlImageDecoder.format_name(), "jxl");
    }

    #[test]
    fn image_decoder_trait_mime_types() {
        assert!(JxlImageDecoder.mime_types().contains(&"image/jxl"));
    }

    #[test]
    fn image_decoder_trait_sniff_positive() {
        assert!(JxlImageDecoder.sniff(JXL_SAMPLE_RGB_4X3));
    }

    #[test]
    fn image_decoder_trait_sniff_negative() {
        assert!(!JxlImageDecoder.sniff(b"not jxl"));
    }

    #[test]
    fn image_decoder_trait_decode_real_sample() {
        let result = JxlImageDecoder.decode_rgba8(JXL_SAMPLE_RGB_4X3);
        assert!(result.is_ok(), "реальный семпл должен декодироваться: {result:?}");
    }

    #[test]
    fn test_is_jxl_naked_format() {
        let jxl_naked = vec![0xFF, 0x0A, 0x00, 0x00];
        assert!(is_jxl(&jxl_naked));
    }

    #[test]
    fn test_is_jxl_naked_format_minimal() {
        let jxl_naked = vec![0xFF, 0x0A];
        assert!(is_jxl(&jxl_naked));
    }

    #[test]
    fn test_is_jxl_isobmff_major_brand() {
        // ftyp box with jxl major brand: size(4) + 'ftyp'(4) + 'jxl '(4) + ...
        let mut jxl_isobmff = vec![0x00, 0x00, 0x00, 0x14]; // box size = 20
        jxl_isobmff.extend_from_slice(b"ftyp");
        jxl_isobmff.extend_from_slice(b"jxl "); // major brand
        jxl_isobmff.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        assert!(is_jxl(&jxl_isobmff));
    }

    #[test]
    fn test_is_jxl_isobmff_compatible_brand() {
        // ftyp box with compatible brand jxl
        let mut jxl_isobmff = vec![0x00, 0x00, 0x00, 0x18]; // box size = 24
        jxl_isobmff.extend_from_slice(b"ftyp");
        jxl_isobmff.extend_from_slice(b"mj2 "); // different major brand
        jxl_isobmff.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        jxl_isobmff.extend_from_slice(b"jxl "); // compatible brand
        assert!(is_jxl(&jxl_isobmff));
    }

    #[test]
    fn test_is_jxl_not_jxl() {
        let png_sig = vec![0x89, 0x50, 0x4E, 0x47]; // PNG signature
        assert!(!is_jxl(&png_sig));

        let empty = vec![];
        assert!(!is_jxl(&empty));

        let single_byte = vec![0xFF];
        assert!(!is_jxl(&single_byte));
    }

}
