//! `loca` table — index from glyph id to byte offset within `glyf` table.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/loca>.
//!
//! Формат зависит от `head.indexToLocFormat`:
//! - Short: массив `u16`, фактический offset = значение × 2 (так
//!   умещаются смещения до 128 КБ).
//! - Long: массив `u32`, offset как есть.
//!
//! Всего записей `numGlyphs + 1`; последняя — конец последнего глифа,
//! поэтому длина глифа N = loca[N+1] − loca[N]. Если loca[N] == loca[N+1],
//! у глифа нет outline (например, space).

use crate::face::FontError;
use crate::head::IndexToLocFormat;

pub struct Loca<'a> {
    data: &'a [u8],
    format: IndexToLocFormat,
    num_glyphs: u16,
}

impl<'a> Loca<'a> {
    pub fn parse(
        data: &'a [u8],
        format: IndexToLocFormat,
        num_glyphs: u16,
    ) -> Result<Self, FontError> {
        let bytes_per_entry = match format {
            IndexToLocFormat::Short => 2,
            IndexToLocFormat::Long => 4,
        };
        let expected = (num_glyphs as usize + 1).saturating_mul(bytes_per_entry);
        if data.len() < expected {
            return Err(FontError::UnexpectedEof);
        }
        Ok(Self {
            data,
            format,
            num_glyphs,
        })
    }

    /// Возвращает `(offset, length)` в байтах внутри `glyf`-таблицы,
    /// либо `None` если глиф пустой (нет outline) или индекс вне диапазона.
    pub fn glyph_range(&self, glyph_id: u16) -> Option<(u32, u32)> {
        if glyph_id >= self.num_glyphs {
            return None;
        }
        let start = self.offset_at(glyph_id as usize)?;
        let end = self.offset_at(glyph_id as usize + 1)?;
        if end <= start {
            None
        } else {
            Some((start, end - start))
        }
    }

    fn offset_at(&self, i: usize) -> Option<u32> {
        match self.format {
            IndexToLocFormat::Short => {
                let off = i * 2;
                let bytes: [u8; 2] = self.data.get(off..off + 2)?.try_into().ok()?;
                Some(u16::from_be_bytes(bytes) as u32 * 2)
            }
            IndexToLocFormat::Long => {
                let off = i * 4;
                let bytes: [u8; 4] = self.data.get(off..off + 4)?.try_into().ok()?;
                Some(u32::from_be_bytes(bytes))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_format_halves_offsets() {
        // loca[0]=0, loca[1]=10 (×2=20), loca[2]=15 (×2=30), loca[3]=25 (×2=50).
        // Глифов 3, записей 4.
        let mut data = Vec::new();
        for v in [0u16, 10, 15, 25] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        let loca = Loca::parse(&data, IndexToLocFormat::Short, 3).unwrap();
        assert_eq!(loca.glyph_range(0), Some((0, 20)));
        assert_eq!(loca.glyph_range(1), Some((20, 10)));
        assert_eq!(loca.glyph_range(2), Some((30, 20)));
    }

    #[test]
    fn long_format_raw_offsets() {
        let mut data = Vec::new();
        for v in [0u32, 100, 250, 500] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        let loca = Loca::parse(&data, IndexToLocFormat::Long, 3).unwrap();
        assert_eq!(loca.glyph_range(0), Some((0, 100)));
        assert_eq!(loca.glyph_range(2), Some((250, 250)));
    }

    #[test]
    fn empty_glyph_returns_none() {
        // loca[1] == loca[2] → у глифа 1 нет outline (space).
        let mut data = Vec::new();
        for v in [0u32, 100, 100, 200] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        let loca = Loca::parse(&data, IndexToLocFormat::Long, 3).unwrap();
        assert_eq!(loca.glyph_range(1), None);
        assert_eq!(loca.glyph_range(0), Some((0, 100)));
        assert_eq!(loca.glyph_range(2), Some((100, 100)));
    }

    #[test]
    fn out_of_range_glyph_returns_none() {
        let mut data = Vec::new();
        for v in [0u32, 100] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        let loca = Loca::parse(&data, IndexToLocFormat::Long, 1).unwrap();
        assert_eq!(loca.glyph_range(1), None);
        assert_eq!(loca.glyph_range(99), None);
    }

    #[test]
    fn truncated_data_rejected() {
        // Нужно (3+1)*2 = 8 байт, дадим 6.
        let data = vec![0u8; 6];
        assert!(matches!(
            Loca::parse(&data, IndexToLocFormat::Short, 3),
            Err(FontError::UnexpectedEof)
        ));
    }
}
