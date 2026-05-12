//! `head` table — глобальный заголовок шрифта.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/head>.
//!
//! Нам нужны:
//! - `unitsPerEm` — масштаб (обычно 1024 или 2048). Координаты глифов и
//!   advance widths выражены в этих юнитах.
//! - `indexToLocFormat` — формат таблицы `loca` (short/long).
//! - bounding box всего шрифта — пригодится для атласа.

use crate::binary::BinaryReader;
use crate::face::FontError;

const MAGIC_NUMBER: u32 = 0x5F0F3CF5;
const HEAD: [u8; 4] = *b"head";

#[derive(Debug, Clone, Copy)]
pub struct Head {
    pub units_per_em: u16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    pub index_to_loc_format: IndexToLocFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexToLocFormat {
    /// Смещения в `loca` хранятся как `u16`, делятся пополам на чтение.
    Short,
    /// Смещения в `loca` — `u32`, читаются как есть.
    Long,
}

impl Head {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        // version (4) + fontRevision (4) + checkSumAdjustment (4) = 12 байт пропускаем
        r.skip(12).ok_or(FontError::UnexpectedEof)?;
        let magic = r.read_u32().ok_or(FontError::UnexpectedEof)?;
        if magic != MAGIC_NUMBER {
            return Err(FontError::InvalidTable(HEAD));
        }
        r.skip(2).ok_or(FontError::UnexpectedEof)?; // flags
        let units_per_em = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        if units_per_em == 0 {
            return Err(FontError::InvalidTable(HEAD));
        }
        r.skip(16).ok_or(FontError::UnexpectedEof)?; // created (8) + modified (8)
        let x_min = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let y_min = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let x_max = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let y_max = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        // macStyle (2) + lowestRecPPEM (2) + fontDirectionHint (2)
        r.skip(6).ok_or(FontError::UnexpectedEof)?;
        let itlf = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let index_to_loc_format = match itlf {
            0 => IndexToLocFormat::Short,
            1 => IndexToLocFormat::Long,
            _ => return Err(FontError::InvalidTable(HEAD)),
        };
        // glyphDataFormat — нам не нужен
        Ok(Self {
            units_per_em,
            x_min,
            y_min,
            x_max,
            y_max,
            index_to_loc_format,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_head(units_per_em: u16, loc_format: i16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x00010000u32.to_be_bytes()); // version
        out.extend_from_slice(&0u32.to_be_bytes()); // fontRevision
        out.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment
        out.extend_from_slice(&MAGIC_NUMBER.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // flags
        out.extend_from_slice(&units_per_em.to_be_bytes());
        out.extend_from_slice(&[0u8; 16]); // created + modified
        out.extend_from_slice(&(-100i16).to_be_bytes()); // xMin
        out.extend_from_slice(&(-200i16).to_be_bytes()); // yMin
        out.extend_from_slice(&1100i16.to_be_bytes()); // xMax
        out.extend_from_slice(&900i16.to_be_bytes()); // yMax
        out.extend_from_slice(&[0u8; 6]); // macStyle + lowestRecPPEM + fontDirectionHint
        out.extend_from_slice(&loc_format.to_be_bytes());
        out.extend_from_slice(&0i16.to_be_bytes()); // glyphDataFormat
        out
    }

    #[test]
    fn parse_units_per_em_and_bbox() {
        let data = make_head(2048, 0);
        let head = Head::parse(&data).unwrap();
        assert_eq!(head.units_per_em, 2048);
        assert_eq!(head.x_min, -100);
        assert_eq!(head.y_max, 900);
        assert_eq!(head.index_to_loc_format, IndexToLocFormat::Short);
    }

    #[test]
    fn parse_long_loc_format() {
        let data = make_head(1000, 1);
        let head = Head::parse(&data).unwrap();
        assert_eq!(head.index_to_loc_format, IndexToLocFormat::Long);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut data = make_head(1024, 0);
        data[12..16].copy_from_slice(&0xdeadbeefu32.to_be_bytes());
        assert!(matches!(Head::parse(&data), Err(FontError::InvalidTable(_))));
    }

    #[test]
    fn rejects_zero_units_per_em() {
        let data = make_head(0, 0);
        assert!(matches!(Head::parse(&data), Err(FontError::InvalidTable(_))));
    }

    #[test]
    fn rejects_invalid_loc_format() {
        let data = make_head(1024, 5);
        assert!(matches!(Head::parse(&data), Err(FontError::InvalidTable(_))));
    }
}
