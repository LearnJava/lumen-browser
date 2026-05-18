//! `post` — PostScript Information Table. Содержит italic-angle и
//! рекомендуемые underline metrics из font designer-а.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/post>.
//!
//! Phase 0 — парсим только header (одинаковый у всех version-ов 1.0 /
//! 2.0 / 2.5 / 3.0 / 4.0). Glyph name table (только v2.0 / v2.5) пока
//! не нужен: glyph_id → char-name mapping используется для PostScript-
//! совместимости и debug-наглядности, не для рендера. Добавим, когда
//! P2 дойдёт до экспорта Print PDF.

use crate::binary::BinaryReader;
use crate::face::FontError;

const POST: [u8; 4] = *b"post";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Post {
    /// Raw `Fixed` версия (16.16) — для diagnostics. Spec-defined values:
    /// `0x00010000` (v1.0), `0x00020000` (v2.0), `0x00025000` (v2.5),
    /// `0x00030000` (v3.0), `0x00040000` (v4.0).
    pub version: u32,
    /// Italic angle в градусах (counter-clockwise). 0.0 для прямого
    /// начертания, отрицательные значения для типичного italic-slant
    /// вправо (например, -12.0 для Helvetica Oblique). F16Dot16
    /// (Fixed 16.16) → f32 = raw / 65536.
    pub italic_angle: f32,
    /// Рекомендованная Y-координата (от baseline) для underline,
    /// в font units. Отрицательная — линия под baseline.
    pub underline_position: i16,
    /// Толщина underline-линии в font units.
    pub underline_thickness: i16,
    /// `true` если шрифт monospace (все advance widths одинаковы).
    /// Подсказка для layout-а; реальная monospace-проверка делается
    /// сравнением advance widths из `hmtx`.
    pub is_fixed_pitch: bool,
}

impl Post {
    /// Sentinel-значения версий per spec.
    pub const VERSION_1_0: u32 = 0x0001_0000;
    pub const VERSION_2_0: u32 = 0x0002_0000;
    pub const VERSION_2_5: u32 = 0x0002_5000;
    pub const VERSION_3_0: u32 = 0x0003_0000;
    pub const VERSION_4_0: u32 = 0x0004_0000;

    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let version = r.read_u32().ok_or(FontError::InvalidTable(POST))?;
        // Spec: версия — Fixed 16.16. Известные: 1.0/2.0/2.5/3.0/4.0.
        // Не валидируем строго — реальные шрифты иногда имеют exotic
        // версии; нам всё равно нужны только первые 32 байта header.
        let italic_raw = r.read_i32().ok_or(FontError::InvalidTable(POST))?;
        let italic_angle = italic_raw as f32 / 65536.0;
        let underline_position = r.read_i16().ok_or(FontError::InvalidTable(POST))?;
        let underline_thickness = r.read_i16().ok_or(FontError::InvalidTable(POST))?;
        let is_fixed_pitch = r.read_u32().ok_or(FontError::InvalidTable(POST))? != 0;
        // minMemType42 / maxMemType42 / minMemType1 / maxMemType1 (4×u32)
        // — глубокая Type 42 PostScript embedding информация, не нужна.
        Ok(Self {
            version,
            italic_angle,
            underline_position,
            underline_thickness,
            is_fixed_pitch,
        })
    }

    /// `true` если italic_angle != 0 (шрифт имеет slant). Удобный
    /// shortcut, эквивалент `self.italic_angle.abs() > f32::EPSILON`.
    pub fn is_italic(self) -> bool {
        self.italic_angle.abs() > f32::EPSILON
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Строит синтетический post header (минимум 32 байта).
    fn build_post(
        version: u32,
        italic_raw: i32,
        underline_pos: i16,
        underline_thick: i16,
        is_fixed_pitch: bool,
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&italic_raw.to_be_bytes());
        out.extend_from_slice(&underline_pos.to_be_bytes());
        out.extend_from_slice(&underline_thick.to_be_bytes());
        out.extend_from_slice(&(if is_fixed_pitch { 1u32 } else { 0u32 }).to_be_bytes());
        // 4×u32 mem-table stubs (12-16 байт от offset 16).
        out.extend_from_slice(&[0u8; 16]);
        out
    }

    #[test]
    fn parses_post_v3_normal_proportional() {
        // v3.0 = 0x00030000, italic=0, underline_pos=-100, thick=50,
        // fixed-pitch=false.
        let data = build_post(0x0003_0000, 0, -100, 50, false);
        let post = Post::parse(&data).unwrap();
        assert_eq!(post.version, Post::VERSION_3_0);
        assert!((post.italic_angle - 0.0).abs() < 1e-3);
        assert_eq!(post.underline_position, -100);
        assert_eq!(post.underline_thickness, 50);
        assert!(!post.is_fixed_pitch);
        assert!(!post.is_italic());
    }

    #[test]
    fn parses_post_v2_italic_slant() {
        // italic_angle = -12.0 (typical для Italic-обозначенных шрифтов).
        // -12.0 × 65536 = -786432.
        let data = build_post(0x0002_0000, -786432, -120, 60, false);
        let post = Post::parse(&data).unwrap();
        assert_eq!(post.version, Post::VERSION_2_0);
        assert!((post.italic_angle - (-12.0)).abs() < 1e-3);
        assert!(post.is_italic());
    }

    #[test]
    fn parses_fixed_pitch_flag() {
        let data = build_post(0x0003_0000, 0, -100, 50, true);
        let post = Post::parse(&data).unwrap();
        assert!(post.is_fixed_pitch);
    }

    #[test]
    fn parses_fractional_italic_angle() {
        // -9.5° = -9.5 × 65536 = -622592.
        let data = build_post(0x0003_0000, -622592, 0, 0, false);
        let post = Post::parse(&data).unwrap();
        assert!((post.italic_angle - (-9.5)).abs() < 1e-3);
    }

    #[test]
    fn parses_positive_italic_angle() {
        // Spec позволяет positive italic_angle (counter-clockwise slant —
        // встречается у некоторых каллиграфических шрифтов).
        // 5.0° = 5.0 × 65536 = 327680.
        let data = build_post(0x0003_0000, 327680, 0, 0, false);
        let post = Post::parse(&data).unwrap();
        assert!((post.italic_angle - 5.0).abs() < 1e-3);
        assert!(post.is_italic());
    }

    #[test]
    fn parses_version_2_5() {
        let data = build_post(0x0002_5000, 0, -100, 50, false);
        let post = Post::parse(&data).unwrap();
        assert_eq!(post.version, Post::VERSION_2_5);
    }

    #[test]
    fn parses_version_4_0() {
        let data = build_post(0x0004_0000, 0, -100, 50, false);
        let post = Post::parse(&data).unwrap();
        assert_eq!(post.version, Post::VERSION_4_0);
    }

    #[test]
    fn parses_exotic_version_accepted() {
        // Не-стандартная версия (например, эмбеддинг-кастомизация в
        // некоторых внутренних шрифтах). Парсер не должен падать.
        let data = build_post(0x0001_0001, 0, -100, 50, false);
        let post = Post::parse(&data).unwrap();
        assert_eq!(post.version, 0x0001_0001);
    }

    #[test]
    fn rejects_truncated_header() {
        let data = build_post(0x0003_0000, 0, -100, 50, false);
        // Парсер читает 16 байт минимум (до is_fixed_pitch включительно).
        // Обрезаем до 14 — посередине is_fixed_pitch u32.
        let truncated = &data[..14];
        assert!(Post::parse(truncated).is_err());
    }

    #[test]
    fn rejects_empty_data() {
        assert!(Post::parse(&[]).is_err());
    }

    #[test]
    fn underline_position_signed() {
        // Spec: underlinePosition обычно отрицателен (Y растёт вверх
        // в font units, baseline = 0, underline ниже = negative Y).
        let data = build_post(0x0003_0000, 0, -200, 30, false);
        let post = Post::parse(&data).unwrap();
        assert!(post.underline_position < 0);
    }

    #[test]
    fn is_italic_treats_tiny_angles_as_normal() {
        // Numerical noise: 0.000001° ≈ нулевое значение, is_italic = false.
        // F16Dot16 точность 1/65536 ≈ 1.5e-5; раунд-trip даёт ~0.
        let data = build_post(0x0003_0000, 0, 0, 0, false);
        let post = Post::parse(&data).unwrap();
        assert!(!post.is_italic());
    }
}
