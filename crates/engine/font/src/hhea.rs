//! `hhea` table — горизонтальный header. Нам нужны метрики строки и
//! `numberOfHMetrics` для разбора `hmtx`.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/hhea>.

use crate::binary::BinaryReader;
use crate::face::FontError;

#[derive(Debug, Clone, Copy)]
pub struct Hhea {
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub advance_width_max: u16,
    pub number_of_h_metrics: u16,
}

impl Hhea {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        r.skip(4).ok_or(FontError::UnexpectedEof)?; // version Fixed
        let ascent = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let descent = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let line_gap = r.read_i16().ok_or(FontError::UnexpectedEof)?;
        let advance_width_max = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        // min_lsb, min_rsb, x_max_extent (i16 × 3) = 6 байт
        // caret_slope_rise, caret_slope_run, caret_offset (i16 × 3) = 6 байт
        // 4 reserved (i16 × 4) = 8 байт
        // metric_data_format (i16) = 2 байта
        // Итого 22 байта skip между advance_width_max и number_of_h_metrics.
        r.skip(22).ok_or(FontError::UnexpectedEof)?;
        let number_of_h_metrics = r.read_u16().ok_or(FontError::UnexpectedEof)?;
        Ok(Self {
            ascent,
            descent,
            line_gap,
            advance_width_max,
            number_of_h_metrics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hhea(ascent: i16, descent: i16, num_h_metrics: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x00010000u32.to_be_bytes()); // version
        out.extend_from_slice(&ascent.to_be_bytes());
        out.extend_from_slice(&descent.to_be_bytes());
        out.extend_from_slice(&0i16.to_be_bytes()); // line_gap
        out.extend_from_slice(&1500u16.to_be_bytes()); // advance_width_max
        out.extend_from_slice(&[0u8; 22]); // 22 байта skip-зоны до number_of_h_metrics
        out.extend_from_slice(&num_h_metrics.to_be_bytes());
        out
    }

    #[test]
    fn parse_basic_metrics() {
        let data = make_hhea(800, -200, 256);
        let hhea = Hhea::parse(&data).unwrap();
        assert_eq!(hhea.ascent, 800);
        assert_eq!(hhea.descent, -200);
        assert_eq!(hhea.number_of_h_metrics, 256);
    }

    #[test]
    fn truncated_rejected() {
        let data = vec![0u8; 20];
        assert!(matches!(Hhea::parse(&data), Err(FontError::UnexpectedEof)));
    }
}
