//! `avar` — Axis Variations Table. Описывает piecewise-linear
//! перенормализацию координат axis из CSS-side normalized space (когда
//! caller линейно отобразил `[min, default, max] → [-1, 0, 1]`) в
//! «spec-correct» normalized space — тот, по которому ищется в `gvar`
//! / glyph variations.
//!
//! Зачем перенормализация: у дизайнера axis может иметь дискретные
//! «дельтовые» точки. Например, `wght` от 100 до 900 с default 400.
//! Linear-нормализация: 400 → 0, 700 → 0.6. Но font designer мог нарисо-
//! вать дельту для 500 (Medium) и хочет, чтобы пользовательский 500 не
//! трактовался как 25% между 400 и 900 (линейно), а как «именно Medium».
//! `avar` хранит segment map типа `(from=0.25, to=0.5)`, и normalize
//! делает 500 → 0.5 вместо 0.25.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/avar>.
//!
//! Phase 0 ограничения:
//! - Поддерживаем только version 1.0 (без axis-value tables и feature
//!   variations из v2.0 — те для расширенных variations selectors).
//! - Парсер хранит маппинги per-axis; реальное consumer-применение
//!   через `Avar::normalize` использует piecewise-linear interp.

use crate::binary::BinaryReader;
use crate::face::FontError;

const AVAR: [u8; 4] = *b"avar";

/// Одна пара (fromCoord → toCoord) в segment map оси. Координаты в
/// `F2Dot14` (-2.0..2.0 с шагом 1/16384), что spec гарантирует
/// для нормализованных axis-value-ов.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisValueMap {
    pub from: f32,
    pub to: f32,
}

/// Segment map для одной оси: список пар, отсортированных по `from`.
/// Spec гарантирует минимум 3 точки: (-1.0 → -1.0), (0.0 → 0.0),
/// (1.0 → 1.0); реальные шрифты дополняют промежуточными.
///
/// Пустой `maps` (positionMapCount = 0) — допустимо для оси без
/// перенормализации; `normalize` возвращает input as-is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentMap {
    pub maps: Vec<AxisValueMap>,
}

impl SegmentMap {
    /// Применяет piecewise-linear перенормализацию: ищет сегмент, в
    /// который попадает `coord`, и линейно интерполирует между его
    /// концами. Outside-of-range — clamp к границам.
    ///
    /// Пустой map (нет точек) — identity, возвращает `coord` без
    /// изменений.
    pub fn normalize(&self, coord: f32) -> f32 {
        if self.maps.is_empty() {
            return coord;
        }
        // Clamp к min/max от первого/последнего сегмента.
        let first = self.maps[0];
        if coord <= first.from {
            return first.to;
        }
        let last = self.maps[self.maps.len() - 1];
        if coord >= last.from {
            return last.to;
        }
        // Бинарный поиск был бы O(log n), но N обычно ≤ 10 — line scan
        // проще и без edge-кейсов для float-сравнения.
        for w in self.maps.windows(2) {
            let a = w[0];
            let b = w[1];
            if coord >= a.from && coord <= b.from {
                let span = b.from - a.from;
                if span <= f32::EPSILON {
                    return a.to; // вырожденный сегмент — берём начало
                }
                let t = (coord - a.from) / span;
                return a.to + t * (b.to - a.to);
            }
        }
        // Не должно случиться (clamp + cover диапазона выше), но
        // safer fallback — последняя точка.
        last.to
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Avar {
    /// Segment maps в порядке axes из `fvar` (axis index identical).
    /// Пустой `segments` если в шрифте нет axes (defensive — обычно
    /// `avar` отсутствует целиком в таком случае).
    pub segments: Vec<SegmentMap>,
}

impl Avar {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(AVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(AVAR))?;
        if major != 1 {
            // v2 (с axis-value tables) — будущая работа.
            return Err(FontError::InvalidTable(AVAR));
        }
        // reserved (uint16) — должно быть 0, но не валидируем строго:
        // защитимся skip-ом.
        r.skip(2).ok_or(FontError::InvalidTable(AVAR))?;
        let axis_count = r.read_u16().ok_or(FontError::InvalidTable(AVAR))? as usize;

        let mut segments = Vec::with_capacity(axis_count);
        for _ in 0..axis_count {
            let map_count = r.read_u16().ok_or(FontError::InvalidTable(AVAR))? as usize;
            let mut maps = Vec::with_capacity(map_count);
            for _ in 0..map_count {
                let from = read_f2dot14(&mut r).ok_or(FontError::InvalidTable(AVAR))?;
                let to = read_f2dot14(&mut r).ok_or(FontError::InvalidTable(AVAR))?;
                maps.push(AxisValueMap { from, to });
            }
            segments.push(SegmentMap { maps });
        }
        Ok(Self { segments })
    }

    /// Перенормализация для axis под индексом `axis_index`. `coord`
    /// — уже линейно нормализованная (`[-1, 0, 1]`) координата от
    /// caller-а. Возвращает spec-correct normalized для lookup в
    /// `gvar` / `HVAR`.
    ///
    /// Если `axis_index` вне диапазона `self.segments` (битый шрифт
    /// или missing axis) — возвращает `coord` as-is (identity).
    pub fn normalize(&self, axis_index: usize, coord: f32) -> f32 {
        self.segments
            .get(axis_index)
            .map_or(coord, |s| s.normalize(coord))
    }
}

/// `F2Dot14` (fixed-point 2.14): big-endian i16, value = raw / 16384.0.
/// OpenType хранит нормализованные axis values в этом формате.
fn read_f2dot14(r: &mut BinaryReader<'_>) -> Option<f32> {
    Some(f32::from(r.read_i16()?) / 16384.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_f2dot14(v: f32) -> [u8; 2] {
        let raw = (v * 16384.0).round() as i16;
        raw.to_be_bytes()
    }

    /// Строит минимальный синтетический avar v1.0 с указанными segment
    /// maps. Каждая map — Vec<(from, to)>.
    fn build_avar(maps_per_axis: &[Vec<(f32, f32)>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        out.extend_from_slice(&(maps_per_axis.len() as u16).to_be_bytes());
        for axis_maps in maps_per_axis {
            out.extend_from_slice(&(axis_maps.len() as u16).to_be_bytes());
            for (from, to) in axis_maps {
                out.extend_from_slice(&put_f2dot14(*from));
                out.extend_from_slice(&put_f2dot14(*to));
            }
        }
        out
    }

    #[test]
    fn parses_empty_avar() {
        let data = build_avar(&[]);
        let avar = Avar::parse(&data).unwrap();
        assert_eq!(avar.segments.len(), 0);
    }

    #[test]
    fn parses_identity_axis() {
        // Spec-required identity: (-1→-1, 0→0, 1→1).
        let data = build_avar(&[vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]]);
        let avar = Avar::parse(&data).unwrap();
        assert_eq!(avar.segments.len(), 1);
        assert_eq!(avar.segments[0].maps.len(), 3);
    }

    #[test]
    fn identity_normalize_returns_input() {
        let data = build_avar(&[vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]]);
        let avar = Avar::parse(&data).unwrap();
        assert!((avar.normalize(0, 0.5) - 0.5).abs() < 1e-3);
        assert!((avar.normalize(0, -0.25) - (-0.25)).abs() < 1e-3);
    }

    #[test]
    fn empty_segment_map_is_identity() {
        // axisCount=1, но 0 точек в map → identity.
        let data = build_avar(&[vec![]]);
        let avar = Avar::parse(&data).unwrap();
        assert!((avar.normalize(0, 0.42) - 0.42).abs() < 1e-6);
    }

    #[test]
    fn remap_intermediate_point() {
        // Дизайнер ставит Medium (500 wght) в normalized 0.5 вместо
        // линейного 0.2. Сегменты: (-1→-1, 0→0, 0.2→0.5, 1→1).
        let data = build_avar(&[vec![
            (-1.0, -1.0),
            (0.0, 0.0),
            (0.2, 0.5),
            (1.0, 1.0),
        ]]);
        let avar = Avar::parse(&data).unwrap();
        // На точке: 0.2 → 0.5.
        assert!((avar.normalize(0, 0.2) - 0.5).abs() < 1e-3);
        // Между 0.0 и 0.2 линейно: 0.1 → 0.25.
        assert!((avar.normalize(0, 0.1) - 0.25).abs() < 1e-3);
        // Между 0.2 и 1.0: 0.6 → (0.5 + (0.6-0.2)/(1.0-0.2) * (1.0-0.5)) = 0.75.
        assert!((avar.normalize(0, 0.6) - 0.75).abs() < 1e-3);
    }

    #[test]
    fn normalize_clamps_below_min() {
        let data = build_avar(&[vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]]);
        let avar = Avar::parse(&data).unwrap();
        // Coord -2.0 → clamp к -1.0 (первая точка).
        assert!((avar.normalize(0, -2.0) - (-1.0)).abs() < 1e-3);
    }

    #[test]
    fn normalize_clamps_above_max() {
        let data = build_avar(&[vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]]);
        let avar = Avar::parse(&data).unwrap();
        assert!((avar.normalize(0, 2.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn multiple_axes_have_independent_maps() {
        // axis 0 — identity; axis 1 — remap (0 → 0.7).
        let data = build_avar(&[
            vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)],
            vec![(-1.0, -1.0), (0.0, 0.7), (1.0, 1.0)],
        ]);
        let avar = Avar::parse(&data).unwrap();
        assert!((avar.normalize(0, 0.0) - 0.0).abs() < 1e-3);
        assert!((avar.normalize(1, 0.0) - 0.7).abs() < 1e-3);
    }

    #[test]
    fn missing_axis_index_is_identity() {
        let data = build_avar(&[]);
        let avar = Avar::parse(&data).unwrap();
        // axis_index=5 — вне диапазона, возвращает coord as-is.
        assert!((avar.normalize(5, 0.42) - 0.42).abs() < 1e-6);
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_avar(&[vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]]);
        // major на offset 0; меняем 1 → 2.
        data[1] = 2;
        assert!(Avar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_table() {
        let data = build_avar(&[vec![(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]]);
        // Обрезаем последний AxisValueMap (4 байта).
        let truncated = &data[..data.len() - 4];
        assert!(Avar::parse(truncated).is_err());
    }

    #[test]
    fn fractional_remap_roundtrip() {
        let data = build_avar(&[vec![(-1.0, -1.0), (-0.5, -0.25), (0.5, 0.75), (1.0, 1.0)]]);
        let avar = Avar::parse(&data).unwrap();
        // F2Dot14 точность — 1/16384 ≈ 6e-5; точки round-trip-ятся.
        assert!((avar.segments[0].maps[1].from - (-0.5)).abs() < 1e-4);
        assert!((avar.segments[0].maps[1].to - (-0.25)).abs() < 1e-4);
    }

    #[test]
    fn degenerate_segment_returns_start_to() {
        // Две точки с одинаковым `from` (вырожденный сегмент).
        let data = build_avar(&[vec![
            (-1.0, -1.0),
            (0.0, 0.0),
            (0.5, 0.6),
            (0.5, 0.8),
            (1.0, 1.0),
        ]]);
        let avar = Avar::parse(&data).unwrap();
        // Между парой одинаковых `from` возвращаем либо `a.to`, либо
        // `b.to` (зависит от того, на каком сегменте scan остановился).
        // F2Dot14 round-trip: 0.6 → 0.59997..., 0.8 → 0.79997... .
        let result = avar.normalize(0, 0.5);
        assert!(
            (result - 0.6).abs() < 1e-3 || (result - 0.8).abs() < 1e-3,
            "got {result}, expected ≈ 0.6 or ≈ 0.8"
        );
    }
}
