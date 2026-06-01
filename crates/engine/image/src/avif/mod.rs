//! AVIF декодер (AV1 Image File Format, ISO/IEC 23008-12).
//!
//! AVIF — ISOBMFF-контейнер с AV1-кодированным изображением.
//!
//! Phase 0 ограничения:
//! - Только первый (и единственный) кадр статичных AVIF.
//! - Анимированный AVIF (major_brand `avis`) распознаётся, но декодируется
//!   только первый кадр; полная анимация — Wave 3.
//! - ICC-профиль не извлекается (icc_profile поле → None).
//!
//! Фактическое декодирование требует feature `avif` в Cargo.toml lumen-image:
//! `cargo build -p lumen-image --features avif`. Без неё `is_avif()` работает,
//! `decode_avif()` возвращает `AvifError::Decode`.
//!
//! Feature "avif" подтягивает `image = "0.25"` → `libavif` (cmake + nasm).

/// Ошибка декодирования AVIF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvifError {
    /// Байты не являются валидным ISOBMFF-файлом с ftyp=avif/avis.
    InvalidSignature,
    /// Контейнер распознан, но декодер вернул ошибку.
    Decode(String),
}

impl core::fmt::Display for AvifError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "не AVIF: ftyp-бокс не найден или бренд не avif/avis"),
            Self::Decode(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AvifError {}

/// Проверяет AVIF-сигнатуру по ISOBMFF ftyp-боксу.
///
/// AVIF/AVIS — ISOBMFF-контейнер. Первый бокс в файле — `ftyp`:
/// - Байты 0–3: размер бокса (u32 big-endian).
/// - Байты 4–7: тип бокса (`ftyp`).
/// - Байты 8–11: major brand (`avif` или `avis`).
///
/// Метод проверяет только major brand; совместимые бренды не сканируются
/// (достаточно для 99 % реальных AVIF-файлов).
#[must_use]
pub fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    if &bytes[4..8] != b"ftyp" {
        return false;
    }
    let brand = &bytes[8..12];
    brand == b"avif" || brand == b"avis"
}

/// Декодирует AVIF-файл в RGBA8 (4 байта на пиксель, row-major).
///
/// Возвращает `(ширина, высота, rgba8_данные)`.
///
/// Требует feature `avif` в lumen-image: `cargo build --features avif`.
/// Без этой feature возвращает [`AvifError::Decode`] с пояснением.
///
/// # Errors
/// - [`AvifError::InvalidSignature`] — сигнатура AVIF не найдена.
/// - [`AvifError::Decode`] — декодирование не удалось (или feature отключена).
pub fn decode_avif(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), AvifError> {
    if !is_avif(bytes) {
        return Err(AvifError::InvalidSignature);
    }
    decode_avif_impl(bytes)
}

#[cfg(feature = "avif")]
fn decode_avif_impl(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), AvifError> {
    use image::{GenericImageView as _, ImageFormat};
    let img = image::load_from_memory_with_format(bytes, ImageFormat::Avif)
        .map_err(|e| AvifError::Decode(format!("libavif: {e}")))?;
    let (w, h) = img.dimensions();
    let rgba = img.into_rgba8();
    Ok((w, h, rgba.into_raw()))
}

#[cfg(not(feature = "avif"))]
fn decode_avif_impl(_bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), AvifError> {
    Err(AvifError::Decode(
        "AVIF: включите feature 'avif' в lumen-image (требует cmake + nasm)".to_string(),
    ))
}

/// Реализация [`lumen_core::ext::ImageDecoder`] для AVIF.
///
/// Регистрируется в диспетчере `lumen_image::decode()` и в
/// `supported_mime_types()` для фильтрации `<source type="image/avif">`.
pub struct AvifImageDecoder;

impl lumen_core::ext::ImageDecoder for AvifImageDecoder {
    fn format_name(&self) -> &'static str {
        "avif"
    }

    fn sniff(&self, bytes: &[u8]) -> bool {
        is_avif(bytes)
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["image/avif"]
    }

    fn decode_rgba8(&self, bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        decode_avif(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::ImageDecoder as _;

    /// Минимальный ftyp-бокс с major brand `avif`.
    fn make_avif_ftyp_header(brand: &[u8; 4]) -> Vec<u8> {
        let mut v = vec![
            0x00, 0x00, 0x00, 0x18, // box size = 24
            b'f', b't', b'y', b'p', // box type = ftyp
        ];
        v.extend_from_slice(brand); // major brand
        v.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        v.extend_from_slice(b"mif1"); // compatible brand
        v.extend_from_slice(&[0u8; 64]); // payload (garbage — не декодируется)
        v
    }

    #[test]
    fn avif_major_brand_detected() {
        let bytes = make_avif_ftyp_header(b"avif");
        assert!(is_avif(&bytes));
    }

    #[test]
    fn avis_major_brand_detected() {
        let bytes = make_avif_ftyp_header(b"avis");
        assert!(is_avif(&bytes));
    }

    #[test]
    fn other_brand_not_detected() {
        let bytes = make_avif_ftyp_header(b"mp42");
        assert!(!is_avif(&bytes));
    }

    #[test]
    fn too_short_not_detected() {
        assert!(!is_avif(&[]));
        assert!(!is_avif(&[0x00; 11]));
    }

    #[test]
    fn png_not_detected() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert!(!is_avif(png));
    }

    #[test]
    fn webp_not_detected() {
        let webp = b"RIFF\x00\x00\x00\x00WEBP\x00\x00";
        assert!(!is_avif(webp));
    }

    #[test]
    fn jpeg_not_detected() {
        let jpg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
        assert!(!is_avif(jpg));
    }

    #[test]
    fn invalid_signature_error_on_non_avif() {
        let result = decode_avif(b"not an avif file at all");
        assert_eq!(result, Err(AvifError::InvalidSignature));
    }

    #[test]
    fn avif_header_but_bad_payload_returns_decode_error() {
        // Сигнатура валидная, но данные мусорные → AvifError::Decode
        let bytes = make_avif_ftyp_header(b"avif");
        let result = decode_avif(&bytes);
        assert!(
            matches!(result, Err(AvifError::Decode(_))),
            "ожидался AvifError::Decode, получено {result:?}"
        );
    }

    #[test]
    fn avif_error_display_invalid_signature() {
        let s = format!("{}", AvifError::InvalidSignature);
        assert!(!s.is_empty());
    }

    #[test]
    fn avif_error_display_decode() {
        let s = format!("{}", AvifError::Decode("test error".to_string()));
        assert!(s.contains("test error"));
    }

    #[test]
    fn image_decoder_trait_format_name() {
        assert_eq!(AvifImageDecoder.format_name(), "avif");
    }

    #[test]
    fn image_decoder_trait_mime_types() {
        assert!(AvifImageDecoder.mime_types().contains(&"image/avif"));
    }

    #[test]
    fn image_decoder_trait_sniff_positive() {
        let bytes = make_avif_ftyp_header(b"avif");
        assert!(AvifImageDecoder.sniff(&bytes));
    }

    #[test]
    fn image_decoder_trait_sniff_negative() {
        assert!(!AvifImageDecoder.sniff(b"not avif"));
    }

    #[test]
    fn image_decoder_trait_decode_error_on_bad_payload() {
        let bytes = make_avif_ftyp_header(b"avif");
        let result = AvifImageDecoder.decode_rgba8(&bytes);
        assert!(result.is_err(), "мусорные данные должны вернуть ошибку");
    }
}
