//! `hmtx` table — horizontal metrics per glyph (advance width + left side bearing).
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/hmtx>.
//!
//! Записей с полными метриками — `numberOfHMetrics` штук (из `hhea`).
//! Хвостовые глифы (`numGlyphs − numberOfHMetrics`) делят последний
//! advance_width с одним из «полных» и хранят только left-side-bearing.
//! Это экономит место в моноширинных шрифтах.

use crate::face::FontError;

pub struct Hmtx<'a> {
    data: &'a [u8],
    num_h_metrics: u16,
    num_glyphs: u16,
}

impl<'a> Hmtx<'a> {
    pub fn parse(data: &'a [u8], num_h_metrics: u16, num_glyphs: u16) -> Result<Self, FontError> {
        if num_h_metrics == 0 || num_h_metrics > num_glyphs {
            return Err(FontError::InvalidTable(*b"hmtx"));
        }
        let metrics = num_h_metrics as usize * 4;
        let trailing = (num_glyphs as usize - num_h_metrics as usize) * 2;
        if data.len() < metrics + trailing {
            return Err(FontError::UnexpectedEof);
        }
        Ok(Self {
            data,
            num_h_metrics,
            num_glyphs,
        })
    }

    pub fn advance_width(&self, glyph_id: u16) -> Option<u16> {
        if glyph_id >= self.num_glyphs {
            return None;
        }
        // У хвостовых глифов общий advance_width с последним «полным».
        let idx = glyph_id.min(self.num_h_metrics - 1);
        let off = idx as usize * 4;
        let bytes: [u8; 2] = self.data.get(off..off + 2)?.try_into().ok()?;
        Some(u16::from_be_bytes(bytes))
    }

    pub fn left_side_bearing(&self, glyph_id: u16) -> Option<i16> {
        if glyph_id >= self.num_glyphs {
            return None;
        }
        let off = if glyph_id < self.num_h_metrics {
            glyph_id as usize * 4 + 2
        } else {
            self.num_h_metrics as usize * 4
                + (glyph_id - self.num_h_metrics) as usize * 2
        };
        let bytes: [u8; 2] = self.data.get(off..off + 2)?.try_into().ok()?;
        Some(i16::from_be_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_metrics_for_each_glyph() {
        // num_h_metrics = 3, num_glyphs = 3. Все глифы получают свой advance_width.
        let mut data = Vec::new();
        for (aw, lsb) in [(500u16, 50i16), (700, 60), (900, 70)] {
            data.extend_from_slice(&aw.to_be_bytes());
            data.extend_from_slice(&lsb.to_be_bytes());
        }
        let hmtx = Hmtx::parse(&data, 3, 3).unwrap();
        assert_eq!(hmtx.advance_width(0), Some(500));
        assert_eq!(hmtx.advance_width(1), Some(700));
        assert_eq!(hmtx.advance_width(2), Some(900));
        assert_eq!(hmtx.left_side_bearing(0), Some(50));
        assert_eq!(hmtx.left_side_bearing(2), Some(70));
    }

    #[test]
    fn trailing_glyphs_share_last_advance_width() {
        // num_h_metrics = 2, num_glyphs = 4. Глифы 2 и 3 берут aw из глифа 1.
        let mut data = Vec::new();
        // glyph 0: (500, 50)
        data.extend_from_slice(&500u16.to_be_bytes());
        data.extend_from_slice(&50i16.to_be_bytes());
        // glyph 1: (700, 60)
        data.extend_from_slice(&700u16.to_be_bytes());
        data.extend_from_slice(&60i16.to_be_bytes());
        // glyph 2 trailing: только lsb = 70
        data.extend_from_slice(&70i16.to_be_bytes());
        // glyph 3 trailing: только lsb = 80
        data.extend_from_slice(&80i16.to_be_bytes());

        let hmtx = Hmtx::parse(&data, 2, 4).unwrap();
        assert_eq!(hmtx.advance_width(0), Some(500));
        assert_eq!(hmtx.advance_width(1), Some(700));
        assert_eq!(hmtx.advance_width(2), Some(700)); // от глифа 1
        assert_eq!(hmtx.advance_width(3), Some(700));
        assert_eq!(hmtx.left_side_bearing(2), Some(70));
        assert_eq!(hmtx.left_side_bearing(3), Some(80));
    }

    #[test]
    fn out_of_range_glyph_returns_none() {
        let data = vec![0u8; 4];
        let hmtx = Hmtx::parse(&data, 1, 1).unwrap();
        assert_eq!(hmtx.advance_width(99), None);
    }

    #[test]
    fn truncated_rejected() {
        // Нужно 1*4 + 0 = 4 байта, дадим 2.
        let data = vec![0u8; 2];
        assert!(matches!(
            Hmtx::parse(&data, 1, 1),
            Err(FontError::UnexpectedEof)
        ));
    }

    #[test]
    fn zero_h_metrics_rejected() {
        let data = vec![0u8; 100];
        assert!(matches!(
            Hmtx::parse(&data, 0, 10),
            Err(FontError::InvalidTable(_))
        ));
    }
}
