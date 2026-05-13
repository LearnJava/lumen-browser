//! Парсер чанка `IHDR` — заголовка PNG-файла.
//!
//! Структура (PNG §11.2.2):
//!
//! ```text
//! offset | bytes | поле
//! -------+-------+-----------------------------------
//!     0  |   4   | width  (big-endian u32, > 0)
//!     4  |   4   | height (big-endian u32, > 0)
//!     8  |   1   | bit_depth  (1, 2, 4, 8, 16)
//!     9  |   1   | color_type (0, 2, 3, 4, 6)
//!    10  |   1   | compression_method (0)
//!    11  |   1   | filter_method      (0)
//!    12  |   1   | interlace_method   (0 = none, 1 = Adam7)
//! ```
//!
//! Здесь же — отображение `(color_type, bit_depth)` в высокоуровневый
//! `PixelFormat` крейта. Phase 0 принимает только 8-битную глубину
//! и пропускает palette/Adam7 как `Unsupported`.

use crate::{DecodeError, IhdrError, PixelFormat, UnsupportedReason};

/// Распарсенный IHDR. Хранит «сырой» `color_type` + `bit_depth`, чтобы
/// дальнейшие шаги (например, distinct логика для grayscale vs palette)
/// могли иметь доступ к точному варианту.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ihdr {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: ColorType,
    pub interlaced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorType {
    Grayscale,
    Rgb,
    Palette,
    GrayscaleAlpha,
    Rgba,
}

impl ColorType {
    fn from_raw(v: u8) -> Result<Self, IhdrError> {
        Ok(match v {
            0 => Self::Grayscale,
            2 => Self::Rgb,
            3 => Self::Palette,
            4 => Self::GrayscaleAlpha,
            6 => Self::Rgba,
            _ => return Err(IhdrError::UnknownColorType(v)),
        })
    }

}

impl Ihdr {
    pub(crate) fn parse(data: &[u8]) -> Result<Self, DecodeError> {
        if data.len() != 13 {
            return Err(DecodeError::BadIhdr(IhdrError::WrongLength(
                u32::try_from(data.len()).unwrap_or(u32::MAX),
            )));
        }
        let width = u32::from_be_bytes(data[0..4].try_into().unwrap());
        let height = u32::from_be_bytes(data[4..8].try_into().unwrap());
        if width == 0 || height == 0 {
            return Err(DecodeError::BadIhdr(IhdrError::ZeroDimension));
        }
        let bit_depth = data[8];
        let raw_color = data[9];
        let compression = data[10];
        let filter = data[11];
        let interlace = data[12];

        if compression != 0 {
            return Err(DecodeError::BadIhdr(IhdrError::BadCompression(compression)));
        }
        if filter != 0 {
            return Err(DecodeError::BadIhdr(IhdrError::BadFilter(filter)));
        }
        let color_type = ColorType::from_raw(raw_color).map_err(DecodeError::BadIhdr)?;

        // Проверка разрешённых сочетаний bit_depth × color_type
        // по PNG §11.2.2 таблице 11.1.
        let allowed = match color_type {
            ColorType::Grayscale => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
            ColorType::Rgb | ColorType::GrayscaleAlpha | ColorType::Rgba => {
                matches!(bit_depth, 8 | 16)
            }
            ColorType::Palette => matches!(bit_depth, 1 | 2 | 4 | 8),
        };
        if !allowed {
            return Err(DecodeError::BadIhdr(IhdrError::BadBitDepthForColorType {
                bit_depth,
                color_type: raw_color,
            }));
        }

        let interlaced = match interlace {
            0 => false,
            1 => true,
            other => return Err(DecodeError::BadIhdr(IhdrError::UnknownInterlace(other))),
        };

        Ok(Self {
            width,
            height,
            bit_depth,
            color_type,
            interlaced,
        })
    }

    /// Преобразовать `(color_type, bit_depth)` в публичный `PixelFormat`.
    /// Возвращает `Unsupported(...)`, если на текущем этапе формат
    /// принципиально не реализован (palette, sub-byte, 16-bit).
    pub(crate) fn pixel_format(&self) -> Result<PixelFormat, DecodeError> {
        if self.interlaced {
            return Err(DecodeError::Unsupported(UnsupportedReason::Interlaced));
        }
        if self.bit_depth == 16 {
            return Err(DecodeError::Unsupported(UnsupportedReason::SixteenBitDepth));
        }
        if matches!(self.color_type, ColorType::Palette) {
            return Err(DecodeError::Unsupported(UnsupportedReason::Palette));
        }
        if self.bit_depth != 8 {
            return Err(DecodeError::Unsupported(UnsupportedReason::SubByteDepth(
                self.bit_depth,
            )));
        }
        Ok(match self.color_type {
            ColorType::Grayscale => PixelFormat::Gray8,
            ColorType::GrayscaleAlpha => PixelFormat::GrayAlpha8,
            ColorType::Rgb => PixelFormat::Rgb8,
            ColorType::Rgba => PixelFormat::Rgba8,
            ColorType::Palette => unreachable!("palette уже отвергнут выше"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_ihdr(
        w: u32,
        h: u32,
        bit_depth: u8,
        color_type: u8,
        compression: u8,
        filter: u8,
        interlace: u8,
    ) -> Vec<u8> {
        let mut v = Vec::with_capacity(13);
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.push(bit_depth);
        v.push(color_type);
        v.push(compression);
        v.push(filter);
        v.push(interlace);
        v
    }

    #[test]
    fn parse_rgba8_basic() {
        let data = build_ihdr(10, 20, 8, 6, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert_eq!(h.width, 10);
        assert_eq!(h.height, 20);
        assert_eq!(h.bit_depth, 8);
        assert_eq!(h.color_type, ColorType::Rgba);
        assert!(!h.interlaced);
        assert_eq!(h.pixel_format().unwrap(), PixelFormat::Rgba8);
    }

    #[test]
    fn parse_gray8() {
        let data = build_ihdr(1, 1, 8, 0, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert_eq!(h.color_type, ColorType::Grayscale);
        assert_eq!(h.pixel_format().unwrap(), PixelFormat::Gray8);
    }

    #[test]
    fn parse_rgb8() {
        let data = build_ihdr(5, 5, 8, 2, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert_eq!(h.pixel_format().unwrap(), PixelFormat::Rgb8);
    }

    #[test]
    fn parse_gray_alpha8() {
        let data = build_ihdr(5, 5, 8, 4, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert_eq!(h.pixel_format().unwrap(), PixelFormat::GrayAlpha8);
    }

    #[test]
    fn rejects_wrong_length() {
        let data = vec![0u8; 12];
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::WrongLength(12)))
        ));
    }

    #[test]
    fn rejects_zero_width() {
        let data = build_ihdr(0, 10, 8, 6, 0, 0, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::ZeroDimension))
        ));
    }

    #[test]
    fn rejects_zero_height() {
        let data = build_ihdr(10, 0, 8, 6, 0, 0, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::ZeroDimension))
        ));
    }

    #[test]
    fn rejects_unknown_color_type() {
        let data = build_ihdr(1, 1, 8, 7, 0, 0, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::UnknownColorType(7)))
        ));
    }

    #[test]
    fn rejects_bad_compression() {
        let data = build_ihdr(1, 1, 8, 0, 1, 0, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::BadCompression(1)))
        ));
    }

    #[test]
    fn rejects_bad_filter() {
        let data = build_ihdr(1, 1, 8, 0, 0, 2, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::BadFilter(2)))
        ));
    }

    #[test]
    fn rejects_unknown_interlace() {
        let data = build_ihdr(1, 1, 8, 0, 0, 0, 2);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::UnknownInterlace(2)))
        ));
    }

    #[test]
    fn rejects_rgb_with_4bit_depth() {
        // RGB разрешает только bit_depth 8 или 16.
        let data = build_ihdr(1, 1, 4, 2, 0, 0, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::BadBitDepthForColorType { .. }))
        ));
    }

    #[test]
    fn rejects_palette_with_16bit_depth() {
        // Palette разрешает 1/2/4/8 бит.
        let data = build_ihdr(1, 1, 16, 3, 0, 0, 0);
        assert!(matches!(
            Ihdr::parse(&data),
            Err(DecodeError::BadIhdr(IhdrError::BadBitDepthForColorType { .. }))
        ));
    }

    #[test]
    fn pixel_format_rejects_interlaced() {
        let data = build_ihdr(1, 1, 8, 6, 0, 0, 1);
        let h = Ihdr::parse(&data).unwrap();
        assert!(h.interlaced);
        assert!(matches!(
            h.pixel_format(),
            Err(DecodeError::Unsupported(UnsupportedReason::Interlaced))
        ));
    }

    #[test]
    fn pixel_format_rejects_palette() {
        let data = build_ihdr(1, 1, 8, 3, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert!(matches!(
            h.pixel_format(),
            Err(DecodeError::Unsupported(UnsupportedReason::Palette))
        ));
    }

    #[test]
    fn pixel_format_rejects_16bit() {
        let data = build_ihdr(1, 1, 16, 2, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert!(matches!(
            h.pixel_format(),
            Err(DecodeError::Unsupported(UnsupportedReason::SixteenBitDepth))
        ));
    }

    #[test]
    fn pixel_format_rejects_sub_byte_depth() {
        let data = build_ihdr(1, 1, 4, 0, 0, 0, 0);
        let h = Ihdr::parse(&data).unwrap();
        assert!(matches!(
            h.pixel_format(),
            Err(DecodeError::Unsupported(UnsupportedReason::SubByteDepth(4)))
        ));
    }

}
