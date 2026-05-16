//! `fvar` — Font Variations table. Описывает variation axes (wght/wdth/slnt/
//! opsz/ital и custom-теги): для каждой оси хранятся `min` / `default` / `max`
//! значения в axis units. Это **enabler** для Variable Fonts Level 1 runtime —
//! сам интерполяционный pipeline (gvar / avar / HVAR / interpolation в
//! rasterizer-е) появится позже, когда P2 дойдёт до P2 «Variable fonts axes
//! runtime» в roadmap-е.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/fvar>.
//!
//! Phase 0 ограничения:
//! - Парсятся только **axis records**. `instance records` (named instances —
//!   «Bold», «Light Italic» и т.д.) опущены: они нужны для font-variation-
//!   settings UI / font-style: italic 30 picker-а, до которого CSS-сторона
//!   ещё не дошла.
//! - `axisNameID` сохраняется как `u16` — резолв в строку через `name` table
//!   делает caller (если нужно вывести "Weight" / "Width" в picker-е).

use crate::binary::BinaryReader;
use crate::face::FontError;

const FVAR: [u8; 4] = *b"fvar";

/// Одна variation axis. Все значения в native axis units (не CSS-нормализо-
/// ванные): caller, который хочет получить CSS-совместимый `font-weight: 700`,
/// сам сравнивает с `axis(b"wght")` диапазоном.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VariationAxis {
    /// 4-байтовый OpenType tag. Стандартные (CSS Fonts L4): `wght` (weight),
    /// `wdth` (width), `slnt` (slant), `opsz` (optical size), `ital` (italic).
    /// Custom tags разрешены спекой; CSS-сторона передаёт их через
    /// `font-variation-settings`.
    pub tag: [u8; 4],
    /// Минимум диапазона (включительно).
    pub min: f32,
    /// Default — значение, используемое font-ом «по умолчанию» (= classic
    /// shape без вариаций). CSS-side: при `font-variation-settings: normal`
    /// rasterizer должен использовать default для всех axes.
    pub default: f32,
    /// Максимум диапазона (включительно).
    pub max: f32,
    /// Bit 0 — axis скрыт от UI font picker-а (рекомендация шрифтового
    /// дизайнера). Bits 1-15 reserved.
    pub flags: u16,
    /// Индекс в `name` table для human-readable названия оси (например,
    /// «Weight», «Optical Size»). Resolve в строку — задача caller-а через
    /// [`crate::Name`].
    pub axis_name_id: u16,
}

impl VariationAxis {
    /// Bit 0 — `HIDDEN_AXIS` flag из spec. UI font picker не должен
    /// показывать такие axes как настраиваемые.
    pub const FLAG_HIDDEN: u16 = 0x0001;

    pub fn is_hidden(self) -> bool {
        self.flags & Self::FLAG_HIDDEN != 0
    }

    /// Зажать значение в `[min, max]`. Полезно при подаче CSS-уровневого
    /// значения axis-а в rasterizer (вне диапазона — undefined behaviour
    /// per spec; clamp — типовое поведение Chromium / WebKit).
    pub fn clamp(self, value: f32) -> f32 {
        if value < self.min {
            self.min
        } else if value > self.max {
            self.max
        } else {
            value
        }
    }
}

/// Все axes из `fvar`. Порядок — как в таблице (важно для `instanceCoord`
/// resolve, который мы пока не парсим).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fvar {
    pub axes: Vec<VariationAxis>,
}

impl Fvar {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
        if major != 1 {
            return Err(FontError::InvalidTable(FVAR));
        }
        let axes_array_offset = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        // countSizePairs (uint16) — reserved в spec, всегда 2. Пропускаем.
        r.skip(2).ok_or(FontError::InvalidTable(FVAR))?;
        let axis_count = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        let axis_size = r.read_u16().ok_or(FontError::InvalidTable(FVAR))? as usize;
        // instanceCount / instanceSize — не используем (instances не парсим).

        // По spec axisSize всегда 20: tag (4) + min/default/max (3×4 Fixed)
        // + flags (2) + nameID (2). Других значений в практике нет; если
        // встретили — отказ парсинга безопаснее, чем silent wrong-offsets.
        if axis_size != 20 {
            return Err(FontError::InvalidTable(FVAR));
        }

        let mut axes = Vec::with_capacity(axis_count);
        for i in 0..axis_count {
            let off = axes_array_offset
                .checked_add(i.checked_mul(axis_size).ok_or(FontError::InvalidTable(FVAR))?)
                .ok_or(FontError::InvalidTable(FVAR))?;
            let end = off
                .checked_add(axis_size)
                .ok_or(FontError::InvalidTable(FVAR))?;
            if end > data.len() {
                return Err(FontError::InvalidTable(FVAR));
            }
            let mut a = BinaryReader::new(&data[off..end]);
            let tag = a.read_tag().ok_or(FontError::InvalidTable(FVAR))?;
            let min = read_fixed(&mut a).ok_or(FontError::InvalidTable(FVAR))?;
            let default = read_fixed(&mut a).ok_or(FontError::InvalidTable(FVAR))?;
            let max = read_fixed(&mut a).ok_or(FontError::InvalidTable(FVAR))?;
            let flags = a.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
            let axis_name_id = a.read_u16().ok_or(FontError::InvalidTable(FVAR))?;
            axes.push(VariationAxis {
                tag,
                min,
                default,
                max,
                flags,
                axis_name_id,
            });
        }
        Ok(Self { axes })
    }

    /// Найти axis по tag-у. Возвращает `None`, если в шрифте нет такой
    /// оси (например, font не вариативный по `wght`).
    pub fn axis(&self, tag: &[u8; 4]) -> Option<&VariationAxis> {
        self.axes.iter().find(|a| &a.tag == tag)
    }

    /// `true`, если шрифт имеет хотя бы одну variation axis. Для non-variable
    /// fonts таблица `fvar` обычно отсутствует, и `Font::fvar()` вернёт
    /// `Err(TableNotFound)`; этот хелпер удобен в коде, который сам создаёт
    /// пустой `Fvar` через `Default`.
    pub fn is_variable(&self) -> bool {
        !self.axes.is_empty()
    }
}

/// `F16Dot16` (fixed-point 16.16) → f32. OpenType хранит axis values в
/// этом формате — sign-extended big-endian i32, разделён на 65536.
fn read_fixed(r: &mut BinaryReader<'_>) -> Option<f32> {
    Some(r.read_i32()? as f32 / 65536.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Кладёт f32 как F16Dot16 (sign-extended 32-bit BE).
    fn put_fixed(v: f32) -> [u8; 4] {
        let raw = (v * 65536.0).round() as i32;
        raw.to_be_bytes()
    }

    /// (tag, min, default, max, flags, name_id) — для синтетических axes
    /// в тестах. Type alias чтобы успокоить clippy::type-complexity.
    type AxisSpec<'a> = (&'a [u8; 4], f32, f32, f32, u16, u16);

    /// Минимальная корректная fvar с указанным набором axes. instanceCount=0,
    /// instances не записываются — Phase 0 их не парсит.
    fn build_fvar(axes: &[AxisSpec<'_>]) -> Vec<u8> {
        let header_size = 16u16;
        let axis_size = 20u16;
        let axis_count = axes.len() as u16;
        let mut out = Vec::with_capacity(
            header_size as usize + axes.len() * axis_size as usize,
        );
        // header
        out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        out.extend_from_slice(&header_size.to_be_bytes()); // axesArrayOffset
        out.extend_from_slice(&2u16.to_be_bytes()); // reserved (countSizePairs)
        out.extend_from_slice(&axis_count.to_be_bytes()); // axisCount
        out.extend_from_slice(&axis_size.to_be_bytes()); // axisSize
        out.extend_from_slice(&0u16.to_be_bytes()); // instanceCount
        out.extend_from_slice(&0u16.to_be_bytes()); // instanceSize
        // axis records
        for (tag, min, default, max, flags, name_id) in axes {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&put_fixed(*min));
            out.extend_from_slice(&put_fixed(*default));
            out.extend_from_slice(&put_fixed(*max));
            out.extend_from_slice(&flags.to_be_bytes());
            out.extend_from_slice(&name_id.to_be_bytes());
        }
        out
    }

    #[test]
    fn parses_empty_fvar() {
        let data = build_fvar(&[]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(!fvar.is_variable());
        assert_eq!(fvar.axes.len(), 0);
    }

    #[test]
    fn parses_single_wght_axis() {
        let data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(fvar.is_variable());
        assert_eq!(fvar.axes.len(), 1);
        let a = &fvar.axes[0];
        assert_eq!(&a.tag, b"wght");
        assert!((a.min - 100.0).abs() < 1e-3);
        assert!((a.default - 400.0).abs() < 1e-3);
        assert!((a.max - 900.0).abs() < 1e-3);
        assert_eq!(a.flags, 0);
        assert_eq!(a.axis_name_id, 256);
        assert!(!a.is_hidden());
    }

    #[test]
    fn parses_multiple_axes() {
        let data = build_fvar(&[
            (b"wght", 100.0, 400.0, 900.0, 0, 256),
            (b"wdth", 50.0, 100.0, 200.0, 0, 257),
            (b"slnt", -15.0, 0.0, 15.0, 0, 258),
        ]);
        let fvar = Fvar::parse(&data).unwrap();
        assert_eq!(fvar.axes.len(), 3);
        assert_eq!(&fvar.axes[0].tag, b"wght");
        assert_eq!(&fvar.axes[1].tag, b"wdth");
        assert_eq!(&fvar.axes[2].tag, b"slnt");
        // slnt с negative min — F16Dot16 sign-extension работает.
        assert!((fvar.axes[2].min - (-15.0)).abs() < 1e-3);
    }

    #[test]
    fn lookup_axis_by_tag() {
        let data = build_fvar(&[
            (b"wght", 100.0, 400.0, 900.0, 0, 256),
            (b"opsz", 8.0, 14.0, 144.0, 0, 257),
        ]);
        let fvar = Fvar::parse(&data).unwrap();
        let opsz = fvar.axis(b"opsz").expect("opsz axis");
        assert!((opsz.default - 14.0).abs() < 1e-3);
        assert!(fvar.axis(b"GRAD").is_none(), "несуществующий tag — None");
    }

    #[test]
    fn hidden_flag_recognised() {
        let data = build_fvar(&[(b"GRAD", -200.0, 0.0, 200.0, VariationAxis::FLAG_HIDDEN, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        assert!(fvar.axes[0].is_hidden());
    }

    #[test]
    fn clamp_below_min_returns_min() {
        let axis = VariationAxis {
            tag: *b"wght",
            min: 100.0,
            default: 400.0,
            max: 900.0,
            flags: 0,
            axis_name_id: 256,
        };
        assert!((axis.clamp(50.0) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_above_max_returns_max() {
        let axis = VariationAxis {
            tag: *b"wght",
            min: 100.0,
            default: 400.0,
            max: 900.0,
            flags: 0,
            axis_name_id: 256,
        };
        assert!((axis.clamp(1500.0) - 900.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_in_range_returns_value() {
        let axis = VariationAxis {
            tag: *b"wght",
            min: 100.0,
            default: 400.0,
            max: 900.0,
            flags: 0,
            axis_name_id: 256,
        };
        assert!((axis.clamp(700.0) - 700.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        // Меняем majorVersion на 2.
        data[0] = 0;
        data[1] = 2;
        assert!(Fvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_wrong_axis_size() {
        let mut data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        // axisSize — на offset 10 (4×u16 в header до неё). Меняем 20 → 22.
        let pos = 10;
        data[pos] = 0;
        data[pos + 1] = 22;
        assert!(Fvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_table() {
        let data = build_fvar(&[(b"wght", 100.0, 400.0, 900.0, 0, 256)]);
        // Обрезаем последний axis record на 5 байт.
        let truncated = &data[..data.len() - 5];
        assert!(Fvar::parse(truncated).is_err());
    }

    #[test]
    fn fractional_axis_values_roundtrip() {
        let data = build_fvar(&[(b"slnt", -9.5, 0.25, 12.75, 0, 256)]);
        let fvar = Fvar::parse(&data).unwrap();
        // F16Dot16 (1/65536 precision) дробные round-trip-ятся точно.
        assert!((fvar.axes[0].min - (-9.5)).abs() < 1e-4);
        assert!((fvar.axes[0].default - 0.25).abs() < 1e-4);
        assert!((fvar.axes[0].max - 12.75).abs() < 1e-4);
    }
}
