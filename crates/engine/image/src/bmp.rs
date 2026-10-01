//! BMP-декодер (обёртка над `zune-bmp`, BUG-1097).
//!
//! WPT-хелпер `common/security-features/subresource/image.py` отдаёт
//! `image/bmp`; без этой ветки `<img>` получал `error` вместо `load`.

use zune_bmp::BmpDecoder;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;

use crate::{Image, PixelFormat};

/// Ошибка декодирования BMP (обёртка над `zune-bmp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BmpError(pub String);

impl core::fmt::Display for BmpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BmpError {}

/// `true`, если байты начинаются с BMP-сигнатуры `BM` и известного размера
/// DIB-заголовка (12/16/40/52/56/64/108/124).
#[must_use]
pub fn is_bmp(bytes: &[u8]) -> bool {
    zune_bmp::probe_bmp(bytes)
}

/// Декодирует BMP (1/4/8/16/24/32 bpp, RLE, bitfields, палитры) в
/// [`PixelFormat::Rgb8`] / [`PixelFormat::Rgba8`] / [`PixelFormat::Gray8`].
///
/// # Errors
/// [`BmpError`] — сигнатура совпала, но заголовок или данные некорректны.
pub fn decode_bmp(bytes: &[u8]) -> Result<Image, BmpError> {
    let mut decoder = BmpDecoder::new(ZCursor::new(bytes));
    decoder.decode_headers().map_err(|e| BmpError(format!("{e:?}")))?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| BmpError("нет размеров после decode_headers".into()))?;
    let format = match decoder.colorspace() {
        Some(ColorSpace::RGBA) => PixelFormat::Rgba8,
        Some(ColorSpace::RGB) => PixelFormat::Rgb8,
        Some(ColorSpace::Luma) => PixelFormat::Gray8,
        other => return Err(BmpError(format!("неподдерживаемое цветовое пространство: {other:?}"))),
    };
    let data = decoder.decode().map_err(|e| BmpError(format!("{e:?}")))?;
    let expected = width * height * format.channels();
    if data.len() != expected {
        return Err(BmpError(format!("размер буфера {} вместо {expected}", data.len())));
    }
    Ok(Image { width: width as u32, height: height as u32, format, data, icc_profile: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode;

    /// Минимальный BMP: BITMAPINFOHEADER, bottom-up, `bpp` бит на пиксель.
    fn make_bmp(w: u32, h: u32, bpp: u16, pixels: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"BM");
        v.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(&54u32.to_le_bytes());
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&bpp.to_le_bytes());
        v.extend_from_slice(&[0; 24]);
        v.extend_from_slice(pixels);
        v
    }

    #[test]
    fn signature() {
        assert!(is_bmp(&make_bmp(1, 1, 24, &[0, 0, 0, 0])));
        assert!(!is_bmp(b"BMxx"));
        assert!(!is_bmp(&crate::PNG_SIGNATURE));
    }

    #[test]
    fn bmp24_bgr_to_rgb_bottom_up() {
        // 2x2, строки по 8 байт (2*3 + 2 паддинга). Нижняя строка идёт первой.
        let px = [
            255, 0, 0, 0, 255, 0, 0, 0, // низ: синий, зелёный (BGR)
            0, 0, 255, 255, 255, 255, 0, 0, // верх: красный, белый
        ];
        let img = decode_bmp(&make_bmp(2, 2, 24, &px)).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.format, PixelFormat::Rgb8);
        assert_eq!(img.data, [255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 255, 0]);
    }

    #[test]
    fn bmp32_alpha_through_decode() {
        let img = decode(&make_bmp(2, 2, 32, &[10, 20, 30, 255, 1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255])).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.format, PixelFormat::Rgba8);
        // нижняя строка файла (первая в потоке) становится нижней строкой картинки
        assert_eq!(&img.data[8..12], &[30, 20, 10, 255]);
    }

    /// Байты, которые отдаёт `image.py` (WPT security-features): 3x2, 24 bpp,
    /// padding 3 байта на строку; пиксели (1,2,3)…(16,17,18) в RGB.
    #[test]
    fn wpt_image_py_output() {
        let bmp: [u8; 78] = [
            66, 77, 78, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0, 1, 0,
            24, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 11,
            10, 15, 14, 13, 18, 17, 16, 0, 0, 0, 3, 2, 1, 6, 5, 4, 9, 8, 7, 0, 0, 0,
        ];
        let img = decode(&bmp).unwrap();
        assert_eq!((img.width, img.height, img.format), (3, 2, PixelFormat::Rgb8));
        let want: Vec<u8> = (1..=18).collect();
        assert_eq!(img.data, want);
    }

    #[test]
    fn truncated_is_error() {
        let mut b = make_bmp(4, 4, 24, &[0; 48]);
        b.truncate(60);
        assert!(decode_bmp(&b).is_err());
    }
}
