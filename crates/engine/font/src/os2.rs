//! `OS/2` table — метрики, нужные font matcher-у (вес, начертание).
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/os2>.
//!
//! Фактически нам из всей таблицы нужны только два поля:
//! - `usWeightClass` (offset 4) — числовой вес 1..1000 (CSS Fonts L4 §5).
//!   В реальных шрифтах обычно 100, 200, …, 900; «промежуточные» значения
//!   встречаются у variable-фонтов и у некоторых дизайнерских face-ов.
//! - `fsSelection` (offset 62) — битовая маска: bit 0 = italic, bit 5 = bold,
//!   bit 9 = oblique (только в OS/2 v4+; нижние биты живут с v0).
//!
//! Поэтому парсер минимальный: проверяем длину, читаем `usWeightClass`,
//! пропускаем 56 байт до `fsSelection` и читаем его. Остальные поля
//! (panose, ulUnicodeRange*, achVendID, ySubscript*, …) вытащим, когда
//! понадобятся (color management, vertical metrics, и т.д.).

use crate::binary::BinaryReader;
use crate::face::FontError;

const OS2: [u8; 4] = *b"OS/2";

/// Минимальный набор полей `OS/2`, нужный font matcher-у.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Os2 {
    /// `usWeightClass` — CSS-совместимый числовой вес 1..1000.
    /// Значение `0` (некорректное по спеке, но встречается у битых файлов)
    /// принудительно поднимается до `400` (Regular) при парсинге.
    pub weight_class: u16,
    /// `fsSelection` — bitset. См. [`Os2::is_italic`] и [`Os2::is_bold`].
    pub fs_selection: u16,
}

impl Os2 {
    /// Bit 0 — italic.
    pub const FS_ITALIC: u16 = 0x0001;
    /// Bit 5 — bold (дублирует `usWeightClass >= 700`, но иногда расходится).
    pub const FS_BOLD: u16 = 0x0020;
    /// Bit 9 — oblique. Появился в OS/2 v4 (2007); более старые face-ы могут
    /// маркировать наклонные начертания только через bit 0.
    pub const FS_OBLIQUE: u16 = 0x0200;

    /// Italic flag из `fsSelection`.
    pub fn is_italic(self) -> bool {
        self.fs_selection & Self::FS_ITALIC != 0
    }

    /// Oblique flag (OS/2 v4+).
    pub fn is_oblique(self) -> bool {
        self.fs_selection & Self::FS_OBLIQUE != 0
    }

    /// Bold flag из `fsSelection`. Не источник истины для веса —
    /// используй `weight_class` напрямую; только как дополнительный сигнал.
    pub fn is_bold(self) -> bool {
        self.fs_selection & Self::FS_BOLD != 0
    }

    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        // version (2) + xAvgCharWidth (2)
        r.skip(4).ok_or(FontError::InvalidTable(OS2))?;
        let raw_weight = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;
        // OS/2 spec: значения 0 и >1000 «non-conforming». В реальных битых
        // файлах 0 встречается (особенно у старых .ttf с незаполненной OS/2);
        // приводим к Regular, чтобы matcher не падал.
        let weight_class = if raw_weight == 0 { 400 } else { raw_weight.min(1000) };
        // от текущего offset (=6) до fsSelection (=62) — 56 байт:
        //   usWidthClass (2) + fsType (2)
        //   ySubscriptXSize/YSize/XOffset/YOffset (8)
        //   ySuperscriptXSize/YSize/XOffset/YOffset (8)
        //   yStrikeoutSize/Position (4)
        //   sFamilyClass (2)
        //   panose (10)
        //   ulUnicodeRange1..4 (16)
        //   achVendID (4)
        // итого 56.
        r.skip(56).ok_or(FontError::InvalidTable(OS2))?;
        let fs_selection = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;
        Ok(Self {
            weight_class,
            fs_selection,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимальный синтетический OS/2 v0: 78 байт.
    fn build_os2(weight: u16, fs_selection: u16) -> Vec<u8> {
        let mut out = Vec::with_capacity(78);
        out.extend_from_slice(&0u16.to_be_bytes()); // version
        out.extend_from_slice(&500i16.to_be_bytes()); // xAvgCharWidth
        out.extend_from_slice(&weight.to_be_bytes());
        // 56 zero-bytes до fsSelection:
        out.extend(std::iter::repeat_n(0u8, 56));
        out.extend_from_slice(&fs_selection.to_be_bytes());
        out
    }

    #[test]
    fn parses_regular_weight_normal_style() {
        let data = build_os2(400, 0x0040); // FS_REGULAR (bit 6) — нормальный, не italic
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 400);
        assert!(!os2.is_italic());
        assert!(!os2.is_bold());
        assert!(!os2.is_oblique());
    }

    #[test]
    fn parses_bold_italic() {
        let data = build_os2(700, Os2::FS_ITALIC | Os2::FS_BOLD);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 700);
        assert!(os2.is_italic());
        assert!(os2.is_bold());
        assert!(!os2.is_oblique());
    }

    #[test]
    fn parses_oblique_bit_v4() {
        let data = build_os2(400, Os2::FS_OBLIQUE);
        let os2 = Os2::parse(&data).unwrap();
        assert!(os2.is_oblique());
        assert!(!os2.is_italic());
    }

    #[test]
    fn weight_zero_normalised_to_regular() {
        let data = build_os2(0, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 400);
    }

    #[test]
    fn weight_above_1000_clamped() {
        let data = build_os2(1500, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 1000);
    }

    #[test]
    fn truncated_table_rejected() {
        let data = vec![0u8; 10]; // меньше минимально нужных 64 байт
        assert!(matches!(Os2::parse(&data), Err(FontError::InvalidTable(_))));
    }

    #[test]
    fn light_weight_value_preserved() {
        let data = build_os2(300, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 300);
    }

    #[test]
    fn variable_font_intermediate_weight() {
        // Variable fonts могут выставлять «промежуточные» weight class,
        // например 350 для семидюймового Light. Парсер не должен это
        // округлять или отбрасывать.
        let data = build_os2(350, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 350);
    }
}
