//! `maxp` table — максимальный профиль. Нам нужно `numGlyphs` (для loca/glyf).
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/maxp>.

use crate::binary::BinaryReader;
use crate::face::FontError;

#[derive(Debug, Clone, Copy)]
pub struct Maxp {
    pub num_glyphs: u16,
}

impl Maxp {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        // version: 0x00005000 (CFF, 6 байт всего) или 0x00010000 (TrueType, 32 байта).
        // Остальные поля нам не нужны до stage с растеризатором.
        r.skip(4).ok_or(FontError::UnexpectedEof)?;
        let num_glyphs = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        Ok(Self { num_glyphs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_num_glyphs() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x00010000u32.to_be_bytes());
        data.extend_from_slice(&1024u16.to_be_bytes());
        // дальше идёт ещё ~26 байт — не читаем, не должны
        data.extend_from_slice(&[0u8; 26]);
        let maxp = Maxp::parse(&data).unwrap();
        assert_eq!(maxp.num_glyphs, 1024);
    }

    #[test]
    fn parse_cff_short_version() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x00005000u32.to_be_bytes());
        data.extend_from_slice(&42u16.to_be_bytes());
        let maxp = Maxp::parse(&data).unwrap();
        assert_eq!(maxp.num_glyphs, 42);
    }

    #[test]
    fn truncated_rejected() {
        assert!(matches!(Maxp::parse(&[0, 0, 0, 0]), Err(FontError::UnexpectedEof)));
    }
}
