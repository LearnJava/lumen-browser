//! `OS/2` table — метрики шрифта для font matcher-а и точной типографики.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/os2>.
//!
//! Парсер разбирает основной набор полей (v0+):
//! - `usWeightClass` / `fsSelection` — для font matcher (CSS Fonts L4 §5.2).
//! - **Subscript / Superscript metrics** — для CSS `vertical-align: sub|super`
//!   геометрии (4 поля × 2 = 8 fields, в font units).
//! - **Strikeout metrics** — `yStrikeoutSize` и `yStrikeoutPosition` для
//!   `text-decoration: line-through` (вместо hardcoded ratio).
//! - **Typographic ascender/descender/line-gap** — рекомендуемые font
//!   designer-ом значения; spec-correct альтернатива `hhea.ascent/descent`.
//! - **Windows ascent/descent** — обычно превышают typo значения; нужны
//!   для совместимости с Win32 рендером.
//!
//! v2+ дополнительно содержит:
//! - **x-height** (`sxHeight`) — высота строчных букв без выносных
//!   элементов; нужна для CSS `ex` unit и font-relative spacing.
//! - **cap-height** (`sCapHeight`) — высота прописных букв; нужна для
//!   `cap` unit (CSS Values L4) и баланса line-height.
//!
//! v3+ / v4+ / v5+ поля (additional unicode ranges, optical size limits,
//! и т.д.) пока пропускаются.

use crate::binary::BinaryReader;
use crate::face::FontError;

const OS2: [u8; 4] = *b"OS/2";

/// Расширенный набор полей `OS/2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Os2 {
    /// Версия таблицы (0..5). Влияет на наличие v2+-полей (x_height,
    /// cap_height).
    pub version: u16,
    /// `usWeightClass` — CSS-совместимый числовой вес 1..1000.
    /// Значение `0` (некорректное по спеке, но встречается у битых файлов)
    /// принудительно поднимается до `400` (Regular) при парсинге.
    pub weight_class: u16,
    /// `fsSelection` — bitset. См. [`Os2::is_italic`] и [`Os2::is_bold`].
    pub fs_selection: u16,

    // ───── Subscript metrics (CSS Values L4 §4.4 — vertical-align: sub) ─────
    /// `ySubscriptXSize` — горизонтальный масштаб sub-script glyph-а, в
    /// font units. Хинт font-designer-а; layout может игнорировать.
    pub subscript_x_size: i16,
    /// `ySubscriptYSize` — вертикальный масштаб (обычно `0.65–0.70` от
    /// em — но абсолютные значения в font units).
    pub subscript_y_size: i16,
    /// `ySubscriptXOffset` — смещение sub-script позиции по X от current
    /// position. Обычно 0; ненулевое для академических шрифтов.
    pub subscript_x_offset: i16,
    /// `ySubscriptYOffset` — смещение по Y (positive = ниже baseline).
    pub subscript_y_offset: i16,

    // ───── Superscript metrics ─────
    pub superscript_x_size: i16,
    pub superscript_y_size: i16,
    pub superscript_x_offset: i16,
    /// `ySuperscriptYOffset` — смещение super-script над baseline
    /// (positive = выше baseline; противоположно sub_y_offset).
    pub superscript_y_offset: i16,

    // ───── Strikeout (CSS text-decoration: line-through) ─────
    /// `yStrikeoutSize` — толщина линии strikeout в font units.
    pub strikeout_size: i16,
    /// `yStrikeoutPosition` — Y-позиция от baseline (positive = выше).
    pub strikeout_position: i16,

    // ───── Typographic metrics (hint от font designer) ─────
    /// `sTypoAscender` — рекомендуемый ascender. Для multi-platform
    /// баланса предпочтительнее `hhea.ascent`.
    pub typo_ascender: i16,
    /// `sTypoDescender` — обычно отрицательный.
    pub typo_descender: i16,
    /// `sTypoLineGap` — рекомендуемый межстрочный gap. Phase 0 не
    /// использует (layout у нас через `line-height` CSS).
    pub typo_line_gap: i16,

    // ───── Windows ascent/descent (для Win32 compat) ─────
    /// `usWinAscent` — Windows-specific максимальный bbox.top (unsigned).
    pub win_ascent: u16,
    /// `usWinDescent` — Windows-specific максимальный |bbox.bottom|.
    pub win_descent: u16,

    // ───── v2+ дополнительные метрики ─────
    /// `sxHeight` (v2+) — высота строчных букв (например, 'x') в font
    /// units. `None` для v0/v1 шрифтов; caller использует `cap_height *
    /// 0.5` как fallback или измеряет глиф 'x' напрямую.
    pub x_height: Option<i16>,
    /// `sCapHeight` (v2+) — высота прописных букв (например, 'H').
    /// `None` для v0/v1.
    pub cap_height: Option<i16>,
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
        let version = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;
        // xAvgCharWidth (2 байта) — пропускаем (не используем для рендера).
        r.skip(2).ok_or(FontError::InvalidTable(OS2))?;
        let raw_weight = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;
        // OS/2 spec: значения 0 и >1000 «non-conforming». В реальных битых
        // файлах 0 встречается (особенно у старых .ttf с незаполненной OS/2);
        // приводим к Regular, чтобы matcher не падал.
        let weight_class = if raw_weight == 0 { 400 } else { raw_weight.min(1000) };
        // usWidthClass (2) + fsType (2) — пропускаем.
        r.skip(4).ok_or(FontError::InvalidTable(OS2))?;

        // Subscript / Superscript / Strikeout (10 × i16 = 20 байт).
        let subscript_x_size = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let subscript_y_size = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let subscript_x_offset = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let subscript_y_offset = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let superscript_x_size = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let superscript_y_size = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let superscript_x_offset = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let superscript_y_offset = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let strikeout_size = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let strikeout_position = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;

        // sFamilyClass (2) + panose (10) + ulUnicodeRange1..4 (16) +
        // achVendID (4) = 32 байта — пропускаем.
        r.skip(32).ok_or(FontError::InvalidTable(OS2))?;

        let fs_selection = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;

        // usFirstCharIndex (2) + usLastCharIndex (2) = 4 байта — пропускаем.
        r.skip(4).ok_or(FontError::InvalidTable(OS2))?;

        let typo_ascender = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let typo_descender = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let typo_line_gap = r.read_i16().ok_or(FontError::InvalidTable(OS2))?;
        let win_ascent = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;
        let win_descent = r.read_u16().ok_or(FontError::InvalidTable(OS2))?;

        // v2+: после ulCodePageRange1/2 (8 байт) идут sxHeight + sCapHeight.
        let (x_height, cap_height) = if version >= 2 {
            // ulCodePageRange1/2 — 8 байт. Если файл обрезан до v0/v1, не
            // отвергаем — просто пропускаем v2+ поля.
            if r.skip(8).is_some() {
                let x = r.read_i16();
                let c = r.read_i16();
                (x, c)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Ok(Self {
            version,
            weight_class,
            fs_selection,
            subscript_x_size,
            subscript_y_size,
            subscript_x_offset,
            subscript_y_offset,
            superscript_x_size,
            superscript_y_size,
            superscript_x_offset,
            superscript_y_offset,
            strikeout_size,
            strikeout_position,
            typo_ascender,
            typo_descender,
            typo_line_gap,
            win_ascent,
            win_descent,
            x_height,
            cap_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Билдер минимального OS/2 v0 (78 байт).
    fn build_os2_v0(weight: u16, fs_selection: u16) -> Vec<u8> {
        build_os2_extended(
            0, weight, fs_selection, 650, 700, 0, 140, // subscript x/y size, x/y offset
            650, 700, 0, 480, // superscript x/y size, x/y offset
            49, 259, // strikeout size + position
            1854, -434, 67, 2189, 600, // typo ascender/descender/line-gap, win ascent/descent
        )
    }

    /// Полноценный builder OS/2 v0 / v2+. version >= 2 → добавляет 12 байт
    /// (ulCodePageRange1/2 + sxHeight + sCapHeight).
    #[allow(clippy::too_many_arguments)]
    fn build_os2_extended(
        version: u16,
        weight: u16,
        fs_selection: u16,
        sub_x: i16,
        sub_y: i16,
        sub_xo: i16,
        sub_yo: i16,
        sup_x: i16,
        sup_y: i16,
        sup_xo: i16,
        sup_yo: i16,
        strk_size: i16,
        strk_pos: i16,
        typo_asc: i16,
        typo_desc: i16,
        typo_lg: i16,
        win_asc: u16,
        win_desc: u16,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&500i16.to_be_bytes()); // xAvgCharWidth
        out.extend_from_slice(&weight.to_be_bytes());
        out.extend_from_slice(&5u16.to_be_bytes()); // usWidthClass
        out.extend_from_slice(&0u16.to_be_bytes()); // fsType
        // Sub/super/strikeout (10 × i16 = 20 байт).
        for v in [sub_x, sub_y, sub_xo, sub_yo, sup_x, sup_y, sup_xo, sup_yo, strk_size, strk_pos] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        // sFamilyClass (2) + panose[10] + ulUnicodeRange1..4 (16) + achVendID (4) = 32 байта.
        out.extend(std::iter::repeat_n(0u8, 32));
        out.extend_from_slice(&fs_selection.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // usFirstCharIndex
        out.extend_from_slice(&0xFFFFu16.to_be_bytes()); // usLastCharIndex
        out.extend_from_slice(&typo_asc.to_be_bytes());
        out.extend_from_slice(&typo_desc.to_be_bytes());
        out.extend_from_slice(&typo_lg.to_be_bytes());
        out.extend_from_slice(&win_asc.to_be_bytes());
        out.extend_from_slice(&win_desc.to_be_bytes());
        out
    }

    /// Билдер v2+ (добавляет ulCodePageRange1/2 + sxHeight + sCapHeight).
    fn append_v2_fields(buf: &mut Vec<u8>, x_height: i16, cap_height: i16) {
        buf.extend_from_slice(&0u32.to_be_bytes()); // ulCodePageRange1
        buf.extend_from_slice(&0u32.to_be_bytes()); // ulCodePageRange2
        buf.extend_from_slice(&x_height.to_be_bytes());
        buf.extend_from_slice(&cap_height.to_be_bytes());
    }

    #[test]
    fn parses_regular_weight_normal_style() {
        let data = build_os2_v0(400, 0x0040);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 400);
        assert!(!os2.is_italic());
        assert!(!os2.is_bold());
        assert!(!os2.is_oblique());
    }

    #[test]
    fn parses_bold_italic() {
        let data = build_os2_v0(700, Os2::FS_ITALIC | Os2::FS_BOLD);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 700);
        assert!(os2.is_italic());
        assert!(os2.is_bold());
    }

    #[test]
    fn parses_oblique_bit_v4() {
        let data = build_os2_v0(400, Os2::FS_OBLIQUE);
        let os2 = Os2::parse(&data).unwrap();
        assert!(os2.is_oblique());
    }

    #[test]
    fn weight_zero_normalised_to_regular() {
        let data = build_os2_v0(0, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 400);
    }

    #[test]
    fn weight_above_1000_clamped() {
        let data = build_os2_v0(1500, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 1000);
    }

    #[test]
    fn truncated_table_rejected() {
        let data = vec![0u8; 10];
        assert!(matches!(Os2::parse(&data), Err(FontError::InvalidTable(_))));
    }

    #[test]
    fn light_weight_value_preserved() {
        let data = build_os2_v0(300, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 300);
    }

    #[test]
    fn variable_font_intermediate_weight() {
        let data = build_os2_v0(350, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.weight_class, 350);
    }

    #[test]
    fn subscript_metrics_round_trip() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.subscript_x_size, 650);
        assert_eq!(os2.subscript_y_size, 700);
        assert_eq!(os2.subscript_x_offset, 0);
        assert_eq!(os2.subscript_y_offset, 140);
    }

    #[test]
    fn superscript_metrics_round_trip() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.superscript_x_size, 650);
        assert_eq!(os2.superscript_y_size, 700);
        assert_eq!(os2.superscript_y_offset, 480);
    }

    #[test]
    fn strikeout_metrics_round_trip() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.strikeout_size, 49);
        assert_eq!(os2.strikeout_position, 259);
    }

    #[test]
    fn typo_metrics_round_trip() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.typo_ascender, 1854);
        assert_eq!(os2.typo_descender, -434);
        assert_eq!(os2.typo_line_gap, 67);
    }

    #[test]
    fn win_metrics_round_trip() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.win_ascent, 2189);
        assert_eq!(os2.win_descent, 600);
    }

    #[test]
    fn v0_has_no_x_or_cap_height() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.version, 0);
        assert!(os2.x_height.is_none());
        assert!(os2.cap_height.is_none());
    }

    #[test]
    fn v2_x_and_cap_height_parsed() {
        let mut data = build_os2_extended(
            2, 400, 0,
            650, 700, 0, 140,
            650, 700, 0, 480,
            49, 259,
            1854, -434, 67, 2189, 600,
        );
        append_v2_fields(&mut data, 1000, 1490);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.version, 2);
        assert_eq!(os2.x_height, Some(1000));
        assert_eq!(os2.cap_height, Some(1490));
    }

    #[test]
    fn v3_x_and_cap_height_parsed_same_offset() {
        // v3 имеет тот же layout до sxHeight/sCapHeight, как и v2.
        let mut data = build_os2_extended(
            3, 400, 0,
            650, 700, 0, 140,
            650, 700, 0, 480,
            49, 259,
            1854, -434, 67, 2189, 600,
        );
        append_v2_fields(&mut data, 800, 1200);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.version, 3);
        assert_eq!(os2.x_height, Some(800));
        assert_eq!(os2.cap_height, Some(1200));
    }

    #[test]
    fn v5_x_and_cap_height_parsed_same_offset() {
        // v5 имеет дополнительные usLowerOpticalPointSize/usUpper после
        // cap_height — мы их не читаем; sxHeight/sCapHeight на тех же
        // позициях.
        let mut data = build_os2_extended(
            5, 400, 0,
            650, 700, 0, 140,
            650, 700, 0, 480,
            49, 259,
            1854, -434, 67, 2189, 600,
        );
        append_v2_fields(&mut data, 700, 1100);
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.version, 5);
        assert_eq!(os2.x_height, Some(700));
        assert_eq!(os2.cap_height, Some(1100));
    }

    #[test]
    fn v2_with_truncated_extended_section_falls_back_to_none() {
        // Объявлена v2, но дальше 78 байт нет (битый файл). Парсер должен
        // вернуть x_height/cap_height = None, не ошибку (graceful degrade).
        let data = build_os2_extended(
            2, 400, 0,
            650, 700, 0, 140,
            650, 700, 0, 480,
            49, 259,
            1854, -434, 67, 2189, 600,
        );
        let os2 = Os2::parse(&data).unwrap();
        assert_eq!(os2.version, 2);
        assert!(os2.x_height.is_none());
        assert!(os2.cap_height.is_none());
    }

    #[test]
    fn version_field_preserved() {
        let data0 = build_os2_v0(400, 0);
        assert_eq!(Os2::parse(&data0).unwrap().version, 0);
        // v1 layout идентичен v0 до конца основных полей.
        let mut data1 = build_os2_v0(400, 0);
        data1[1] = 1;
        assert_eq!(Os2::parse(&data1).unwrap().version, 1);
    }

    #[test]
    fn typo_descender_can_be_negative() {
        let data = build_os2_v0(400, 0);
        let os2 = Os2::parse(&data).unwrap();
        assert!(os2.typo_descender < 0);
    }
}
