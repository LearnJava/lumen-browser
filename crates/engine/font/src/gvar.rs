//! `gvar` — Glyph Variations Table. Хранит per-glyph deltas для outline
//! points (и phantom-points) в зависимости от текущих normalized axis
//! coordinates. В отличие от `HVAR`/`VVAR`/`MVAR`, gvar **не** использует
//! `ItemVariationStore`: формат специфичный — каждая variation описывается
//! tuple-ом (peak + опциональный intermediate region) + списком точек +
//! двумя массивами deltas (x и y).
//!
//! При active variation-instance runtime для каждого glyph:
//! 1. Читает base-outline из `glyf` (точки + phantom points).
//! 2. Для каждой tuple-variation вычисляет scalar по tent-функции
//!    (общий с IVS алгоритм — `tuple_scalar` из этого модуля), умножает
//!    deltas на scalar и прибавляет к coords указанных точек (или ко всем,
//!    если point list = "all").
//! 3. Для точек, которых variation не упоминает напрямую, применяется
//!    **IUP** (Interpolation of Untouched Points) — отложено до интеграции
//!    в rasterizer, parser даёт только сырой набор delta-векторов.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/gvar>.
//!
//! Phase 0 ограничения:
//! - Только v1.0.
//! - Парсер; IUP и применение deltas к контурам — отдельная задача в
//!   rasterizer-е (когда подключится CSS `font-variation-settings` cascade).

use crate::binary::BinaryReader;
use crate::face::FontError;

const GVAR: [u8; 4] = *b"gvar";

/// Flag bit в `gvar.flags`: long-format offsets (uint32) вместо short
/// (uint16 × 2). Аналог `head.indexToLocFormat` для `loca`.
const FLAG_LONG_OFFSETS: u16 = 0x0001;

/// Flags в `tupleVariationCount` (GlyphVariationData header).
const SHARED_POINT_NUMBERS: u16 = 0x8000;
const TUPLE_COUNT_MASK: u16 = 0x0FFF;

/// Flags в `tupleIndex` (TupleVariationHeader).
const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
const INTERMEDIATE_REGION: u16 = 0x4000;
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;
const TUPLE_INDEX_MASK: u16 = 0x0FFF;

/// Какие точки glyph-а трогает variation: либо явный список индексов,
/// либо «все точки» (включая 4 phantom points).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointNumbers {
    /// Variation применяется ко всем точкам glyph-а (включая phantom);
    /// длина `x_deltas`/`y_deltas` равна `glyph_point_count + 4`. Точное
    /// число знает caller через `glyf`.
    All,
    /// Явный отсортированный список point indices, к которым применяется
    /// variation. Длина `x_deltas`/`y_deltas` = `points.len()`.
    Explicit(Vec<u16>),
}

/// Описание одной tuple-variation для glyph-а.
#[derive(Debug, Clone, PartialEq)]
pub struct TupleVariation {
    /// Координаты peak-точки в normalized axis space (F2DOT14, диапазон
    /// −1.0..=1.0). Длина = `axis_count`. При `coords == peak` scalar = 1.0.
    pub peak: Vec<f32>,
    /// Опциональный intermediate region `(start, end)` для tent-функции.
    /// Если `None` — используется default region из peak (start от 0/peak,
    /// end до peak/0 в зависимости от знака peak — см. `tuple_scalar`).
    pub intermediate: Option<(Vec<f32>, Vec<f32>)>,
    /// Точки glyph-а, к которым применяется variation.
    pub points: PointNumbers,
    /// Delta-векторы X (по точкам в порядке `points`). Длина равна
    /// `points.len()` (для `Explicit`) либо `glyph_point_count + 4` (для
    /// `All` — определяется caller-ом по glyf).
    pub x_deltas: Vec<i16>,
    /// Delta-векторы Y (того же размера, что `x_deltas`).
    pub y_deltas: Vec<i16>,
}

/// Полный набор tuple-variations для одного glyph-а.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlyphVariationData {
    pub tuple_variations: Vec<TupleVariation>,
}

/// Распарсенная gvar-таблица. Хранит per-glyph offsets в массив сырых
/// `GlyphVariationData` — реальный разбор откладывается до `parse_glyph`
/// (большая часть glyph-ов в variation font НЕ имеет вариаций, и для
/// них parse_glyph моментально возвращает None).
#[derive(Debug, Clone)]
pub struct Gvar<'a> {
    pub axis_count: u16,
    /// Общие tuples, на которые могут ссылаться tuple variation headers
    /// через `TUPLE_INDEX_MASK` (когда `EMBEDDED_PEAK_TUPLE` не выставлен).
    /// Размер каждого вектора = `axis_count`.
    pub shared_tuples: Vec<Vec<f32>>,
    pub glyph_count: u16,
    /// Bit-mask из spec: bit 0 = LONG_OFFSETS. Сохраняется для
    /// прозрачности (parser сам декодирует с учётом флага).
    pub flags: u16,
    glyph_data: &'a [u8],
    /// Абсолютные byte-offsets от начала `glyph_data` для каждого
    /// glyph-а. Длина = `glyph_count + 1`; per-glyph slice =
    /// `glyph_data[offsets[i] .. offsets[i+1]]`. Equal offsets означают
    /// empty (нет вариаций для этого glyph-а).
    glyph_offsets: Vec<u32>,
}

impl<'a> Gvar<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
        if major != 1 {
            return Err(FontError::InvalidTable(GVAR));
        }
        let axis_count = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
        let shared_tuple_count = r.read_u16().ok_or(FontError::InvalidTable(GVAR))? as usize;
        let shared_tuples_offset = r.read_u32().ok_or(FontError::InvalidTable(GVAR))? as usize;
        let glyph_count = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
        let flags = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
        let glyph_data_array_offset =
            r.read_u32().ok_or(FontError::InvalidTable(GVAR))? as usize;

        let long_offsets = flags & FLAG_LONG_OFFSETS != 0;
        let mut glyph_offsets = Vec::with_capacity(glyph_count as usize + 1);
        for _ in 0..=glyph_count {
            let off = if long_offsets {
                r.read_u32().ok_or(FontError::InvalidTable(GVAR))?
            } else {
                // Short format: stored value × 2 = actual byte offset.
                (r.read_u16().ok_or(FontError::InvalidTable(GVAR))? as u32) * 2
            };
            glyph_offsets.push(off);
        }

        // Shared tuples: shared_tuple_count × axis_count × F2DOT14.
        let mut shared_tuples = Vec::with_capacity(shared_tuple_count);
        if shared_tuple_count > 0 {
            if shared_tuples_offset >= data.len() {
                return Err(FontError::InvalidTable(GVAR));
            }
            let mut s = BinaryReader::new(&data[shared_tuples_offset..]);
            for _ in 0..shared_tuple_count {
                let mut tuple = Vec::with_capacity(axis_count as usize);
                for _ in 0..axis_count {
                    tuple.push(read_f2dot14(&mut s).ok_or(FontError::InvalidTable(GVAR))?);
                }
                shared_tuples.push(tuple);
            }
        }

        // glyph_data — slice от glyph_data_array_offset до конца. Если
        // offset == 0 или совпадает с длиной таблицы (font без gvar-данных
        // для всех glyph-ов), даём пустой slice — все per-glyph offsets
        // должны быть == 0.
        let glyph_data = if glyph_data_array_offset == 0 {
            &data[..0]
        } else if glyph_data_array_offset >= data.len() {
            // Допустимо, если все offsets == 0 (нет данных). Иначе ошибка.
            if glyph_offsets.iter().any(|&o| o != 0) {
                return Err(FontError::InvalidTable(GVAR));
            }
            &data[..0]
        } else {
            &data[glyph_data_array_offset..]
        };

        Ok(Self {
            axis_count,
            shared_tuples,
            glyph_count,
            flags,
            glyph_data,
            glyph_offsets,
        })
    }

    /// Сырой byte-slice glyph-variation-data для одного glyph-а. `None`,
    /// если glyph не имеет вариаций (offsets[i] == offsets[i+1]). Caller
    /// обычно использует `parse_glyph` для типизированного разбора.
    pub fn glyph_variation_data(&self, glyph_id: u16) -> Option<&'a [u8]> {
        let i = glyph_id as usize;
        if i + 1 >= self.glyph_offsets.len() {
            return None;
        }
        let start = self.glyph_offsets[i] as usize;
        let end = self.glyph_offsets[i + 1] as usize;
        if start == end {
            return None;
        }
        if end > self.glyph_data.len() || start > end {
            return None;
        }
        Some(&self.glyph_data[start..end])
    }

    /// Декодирует `GlyphVariationData` для glyph-а. `None` если у glyph-а
    /// нет вариаций. `Err`, если данные обрезаны / битые.
    pub fn parse_glyph(
        &self,
        glyph_id: u16,
    ) -> Result<Option<GlyphVariationData>, FontError> {
        let Some(data) = self.glyph_variation_data(glyph_id) else {
            return Ok(None);
        };
        parse_glyph_variation_data(data, self.axis_count, &self.shared_tuples).map(Some)
    }
}

/// Парсит один `GlyphVariationData` block для одного glyph-а.
fn parse_glyph_variation_data(
    data: &[u8],
    axis_count: u16,
    shared_tuples: &[Vec<f32>],
) -> Result<GlyphVariationData, FontError> {
    let mut r = BinaryReader::new(data);
    let tuple_variation_count = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
    let data_offset = r.read_u16().ok_or(FontError::InvalidTable(GVAR))? as usize;
    let has_shared_points = tuple_variation_count & SHARED_POINT_NUMBERS != 0;
    let count = (tuple_variation_count & TUPLE_COUNT_MASK) as usize;

    // 1. Сначала читаем все TupleVariationHeader-ы подряд.
    struct Header {
        variation_data_size: u16,
        peak: Vec<f32>,
        intermediate: Option<(Vec<f32>, Vec<f32>)>,
        private_point_numbers: bool,
    }

    let mut headers = Vec::with_capacity(count);
    for _ in 0..count {
        let variation_data_size = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;
        let tuple_index = r.read_u16().ok_or(FontError::InvalidTable(GVAR))?;

        let peak = if tuple_index & EMBEDDED_PEAK_TUPLE != 0 {
            // Embedded peak: axis_count × F2DOT14.
            let mut t = Vec::with_capacity(axis_count as usize);
            for _ in 0..axis_count {
                t.push(read_f2dot14(&mut r).ok_or(FontError::InvalidTable(GVAR))?);
            }
            t
        } else {
            // Lookup в shared_tuples по TUPLE_INDEX_MASK.
            let idx = (tuple_index & TUPLE_INDEX_MASK) as usize;
            shared_tuples
                .get(idx)
                .cloned()
                .ok_or(FontError::InvalidTable(GVAR))?
        };

        let intermediate = if tuple_index & INTERMEDIATE_REGION != 0 {
            let mut start = Vec::with_capacity(axis_count as usize);
            for _ in 0..axis_count {
                start.push(read_f2dot14(&mut r).ok_or(FontError::InvalidTable(GVAR))?);
            }
            let mut end = Vec::with_capacity(axis_count as usize);
            for _ in 0..axis_count {
                end.push(read_f2dot14(&mut r).ok_or(FontError::InvalidTable(GVAR))?);
            }
            Some((start, end))
        } else {
            None
        };

        headers.push(Header {
            variation_data_size,
            peak,
            intermediate,
            private_point_numbers: tuple_index & PRIVATE_POINT_NUMBERS != 0,
        });
    }

    // 2. Данные (shared point numbers + per-tuple data) начинаются с
    //    `data_offset` от начала GlyphVariationData. Headers могут заканчиваться
    //    раньше — gap игнорируется.
    if data_offset > data.len() {
        return Err(FontError::InvalidTable(GVAR));
    }
    let serialized = &data[data_offset..];
    let mut s = BinaryReader::new(serialized);

    // 3. Shared packed point numbers, если флаг выставлен.
    let shared_points = if has_shared_points {
        Some(read_packed_point_numbers(&mut s).ok_or(FontError::InvalidTable(GVAR))?)
    } else {
        None
    };

    // 4. Для каждой tuple-variation читаем variation_data_size байт:
    //    [private point numbers (если PRIVATE_POINT_NUMBERS)] + packed deltas (x, y).
    let mut tuple_variations = Vec::with_capacity(count);
    for h in headers {
        let size = h.variation_data_size as usize;
        let pos = s.position();
        let end = pos
            .checked_add(size)
            .ok_or(FontError::InvalidTable(GVAR))?;
        if end > serialized.len() {
            return Err(FontError::InvalidTable(GVAR));
        }
        let tuple_data = &serialized[pos..end];
        // Двигаем основной reader.
        s.skip(size).ok_or(FontError::InvalidTable(GVAR))?;

        let mut t = BinaryReader::new(tuple_data);
        let points = if h.private_point_numbers {
            read_packed_point_numbers(&mut t).ok_or(FontError::InvalidTable(GVAR))?
        } else {
            // Берём shared, если есть; иначе spec: «if neither private
            // nor shared, all points are referenced» → All.
            match &shared_points {
                Some(p) => p.clone(),
                None => PointNumbers::All,
            }
        };

        // Сколько deltas ожидать на каждую координату:
        // - Explicit: len() = points.len()
        // - All: неизвестно на parser-уровне; читаем до конца budget-а и
        //   делим пополам.
        let (x_deltas, y_deltas) = match &points {
            PointNumbers::Explicit(p) => {
                let n = p.len();
                let x = read_packed_deltas_count(&mut t, n)
                    .ok_or(FontError::InvalidTable(GVAR))?;
                let y = read_packed_deltas_count(&mut t, n)
                    .ok_or(FontError::InvalidTable(GVAR))?;
                (x, y)
            }
            PointNumbers::All => {
                let all = read_packed_deltas_until_end(&mut t)
                    .ok_or(FontError::InvalidTable(GVAR))?;
                if all.len() % 2 != 0 {
                    return Err(FontError::InvalidTable(GVAR));
                }
                let mid = all.len() / 2;
                let y = all[mid..].to_vec();
                let mut x = all;
                x.truncate(mid);
                (x, y)
            }
        };

        tuple_variations.push(TupleVariation {
            peak: h.peak,
            intermediate: h.intermediate,
            points,
            x_deltas,
            y_deltas,
        });
    }

    Ok(GlyphVariationData { tuple_variations })
}

/// Decode packed point numbers per OpenType spec
/// (<https://learn.microsoft.com/en-us/typography/opentype/spec/otvarcommonformats#packed-point-numbers>):
/// первый байт — count (или 0 = «all points»); если high bit count-byte
/// выставлен, count 16-bit: `((b0 & 0x7F) << 8) | b1`. Дальше идут runs,
/// каждый со своим control-байтом: `bit 7` — 16-bit deltas, low 7 бит + 1 =
/// длина run-а. Numbers stored как cumulative deltas (first is absolute).
fn read_packed_point_numbers(r: &mut BinaryReader<'_>) -> Option<PointNumbers> {
    let first = r.read_u8()?;
    let count: u16 = if first & 0x80 != 0 {
        let lo = r.read_u8()?;
        ((u16::from(first & 0x7F)) << 8) | u16::from(lo)
    } else {
        u16::from(first)
    };
    if count == 0 {
        return Some(PointNumbers::All);
    }

    let mut points = Vec::with_capacity(count as usize);
    let mut last: u16 = 0;
    let mut remaining = count;
    while remaining > 0 {
        let control = r.read_u8()?;
        let words = control & 0x80 != 0;
        let run_len = u16::from(control & 0x7F) + 1;
        let to_read = run_len.min(remaining);
        for _ in 0..to_read {
            let delta = if words {
                r.read_u16()?
            } else {
                u16::from(r.read_u8()?)
            };
            // wrapping_add — defensive: spec ожидает monotonic, но битый
            // шрифт не должен паниковать parser.
            last = last.wrapping_add(delta);
            points.push(last);
        }
        remaining -= to_read;
    }
    Some(PointNumbers::Explicit(points))
}

/// Decode packed deltas (`expected` штук) per OpenType spec
/// (<https://learn.microsoft.com/en-us/typography/opentype/spec/otvarcommonformats#packed-deltas>):
/// каждый run начинается control-байтом: bit 7 (0x80) DELTAS_ARE_ZERO,
/// bit 6 (0x40) DELTAS_ARE_WORDS, low 6 бит + 1 = длина run-а.
fn read_packed_deltas_count(r: &mut BinaryReader<'_>, expected: usize) -> Option<Vec<i16>> {
    let mut out = Vec::with_capacity(expected);
    while out.len() < expected {
        if !read_one_delta_run(r, expected - out.len(), &mut out)? {
            return None;
        }
    }
    Some(out)
}

/// Decode packed deltas пока в reader-е остаются байты. Используется для
/// PointNumbers::All — общее количество deltas заранее не известно.
fn read_packed_deltas_until_end(r: &mut BinaryReader<'_>) -> Option<Vec<i16>> {
    let mut out = Vec::new();
    while r.remaining() > 0 {
        // Лимит для одного run-а — 64; передаём заведомо большое число
        // (read_one_delta_run возьмёт min).
        if !read_one_delta_run(r, usize::MAX, &mut out)? {
            return None;
        }
    }
    Some(out)
}

/// Читает один run packed deltas и append-ит в `out`. Возвращает `Some(true)`
/// на успех, `Some(false)` если run-длина усечена до `cap_remaining` (для
/// fixed-count чтения; обычно false не встречается, т.к. spec не разбивает
/// run между x и y).
fn read_one_delta_run(
    r: &mut BinaryReader<'_>,
    cap_remaining: usize,
    out: &mut Vec<i16>,
) -> Option<bool> {
    let control = r.read_u8()?;
    let zero = control & 0x80 != 0;
    let words = control & 0x40 != 0;
    let run_len = usize::from(control & 0x3F) + 1;
    let to_read = run_len.min(cap_remaining);
    if zero {
        for _ in 0..to_read {
            out.push(0);
        }
    } else if words {
        for _ in 0..to_read {
            out.push(r.read_i16()?);
        }
    } else {
        for _ in 0..to_read {
            out.push(i16::from(r.read_u8()? as i8));
        }
    }
    Some(true)
}

/// `F2DOT14` (signed 2.14 fixed-point) → f32. OpenType `Tuple` element-ы
/// хранятся в этом формате, диапазон [−2.0, 2.0).
fn read_f2dot14(r: &mut BinaryReader<'_>) -> Option<f32> {
    Some(f32::from(r.read_i16()?) / 16384.0)
}

/// Per-axis scalar tent-функции для одной оси tuple-variation.
/// Аналогичен `RegionAxisCoordinates::scalar` из `item_variation`, но
/// gvar хранит peak отдельно от start/end (intermediate region). Если
/// intermediate отсутствует — start/end по spec вычисляются из peak:
/// peak > 0: start=0, end=peak; peak < 0: start=peak, end=0; peak=0: scalar=1.
pub fn tuple_axis_scalar(
    coord: f32,
    peak: f32,
    intermediate: Option<(f32, f32)>,
) -> f32 {
    // peak == 0 → axis нейтральная.
    if peak == 0.0 {
        return 1.0;
    }
    // Polarity mismatch.
    if (peak > 0.0 && coord < 0.0) || (peak < 0.0 && coord > 0.0) {
        return 0.0;
    }
    let (start, end) = match intermediate {
        Some((s, e)) => (s, e),
        None => {
            if peak > 0.0 {
                (0.0, peak)
            } else {
                (peak, 0.0)
            }
        }
    };
    if coord == peak {
        return 1.0;
    }
    if coord < start || coord > end {
        return 0.0;
    }
    if coord < peak {
        let denom = peak - start;
        if denom == 0.0 {
            return 0.0;
        }
        (coord - start) / denom
    } else {
        let denom = end - peak;
        if denom == 0.0 {
            return 0.0;
        }
        (end - coord) / denom
    }
}

/// Региональный scalar для всех осей tuple-variation: произведение per-axis
/// scalars. Если все axis-scalars положительны — региональный > 0 (variation
/// активна), иначе 0.
pub fn tuple_scalar(coords: &[f32], variation: &TupleVariation) -> f32 {
    let mut acc = 1.0_f32;
    for (i, &peak) in variation.peak.iter().enumerate() {
        let coord = coords.get(i).copied().unwrap_or(0.0);
        let inter = variation
            .intermediate
            .as_ref()
            .map(|(s, e)| (s[i], e[i]));
        acc *= tuple_axis_scalar(coord, peak, inter);
        if acc == 0.0 {
            return 0.0;
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_f2dot14(v: f32) -> [u8; 2] {
        let raw = (v * 16384.0).round() as i16;
        raw.to_be_bytes()
    }

    // ───────── packed point numbers ─────────

    #[test]
    fn packed_points_zero_count_means_all() {
        let data = [0u8];
        let mut r = BinaryReader::new(&data);
        assert_eq!(read_packed_point_numbers(&mut r), Some(PointNumbers::All));
    }

    #[test]
    fn packed_points_single_byte_count_single_run() {
        // count=3, control=0x02 (run_len=3, byte deltas), deltas 1, 2, 3.
        // Cumulative: 1, 3, 6.
        let data = [3u8, 0x02, 1, 2, 3];
        let mut r = BinaryReader::new(&data);
        assert_eq!(
            read_packed_point_numbers(&mut r),
            Some(PointNumbers::Explicit(vec![1, 3, 6]))
        );
    }

    #[test]
    fn packed_points_two_byte_count() {
        // count = 0x8001 = первый байт 0x80, второй 0x01 → count = 1.
        // (0x80 & 0x7F) = 0; (0 << 8) | 1 = 1.
        let data = [0x80, 0x01, 0x00, 7];
        let mut r = BinaryReader::new(&data);
        assert_eq!(
            read_packed_point_numbers(&mut r),
            Some(PointNumbers::Explicit(vec![7]))
        );
    }

    #[test]
    fn packed_points_word_run() {
        // count=2, control=0x81 (high bit = words, len=2), deltas 0x0100, 0x0001.
        // Cumulative: 0x0100=256, 256+1=257.
        let data = [2u8, 0x81, 0x01, 0x00, 0x00, 0x01];
        let mut r = BinaryReader::new(&data);
        assert_eq!(
            read_packed_point_numbers(&mut r),
            Some(PointNumbers::Explicit(vec![256, 257]))
        );
    }

    #[test]
    fn packed_points_multiple_runs() {
        // count=4. Run 1: control=0x01 (byte, len=2), deltas 5, 5 → 5, 10.
        // Run 2: control=0x81 (word, len=2), deltas 100, 100 → 110, 210.
        let data = [4u8, 0x01, 5, 5, 0x81, 0x00, 100, 0x00, 100];
        let mut r = BinaryReader::new(&data);
        assert_eq!(
            read_packed_point_numbers(&mut r),
            Some(PointNumbers::Explicit(vec![5, 10, 110, 210]))
        );
    }

    #[test]
    fn packed_points_run_longer_than_count() {
        // count=2, control=0x04 (run_len=5, byte). Должны прочитать только 2.
        let data = [2u8, 0x04, 1, 2, 3, 4, 5];
        let mut r = BinaryReader::new(&data);
        assert_eq!(
            read_packed_point_numbers(&mut r),
            Some(PointNumbers::Explicit(vec![1, 3]))
        );
        // Должны остановиться после первых 2 deltas, остальные байты не съедены.
        assert_eq!(r.remaining(), 3);
    }

    // ───────── packed deltas ─────────

    #[test]
    fn packed_deltas_byte_run() {
        // control=0x02 (byte, len=3), deltas -1, 2, -3.
        let data = [0x02u8, 0xFFu8, 0x02, 0xFD];
        let mut r = BinaryReader::new(&data);
        let out = read_packed_deltas_count(&mut r, 3).unwrap();
        assert_eq!(out, vec![-1i16, 2, -3]);
    }

    #[test]
    fn packed_deltas_word_run() {
        // control=0x41 (word, len=2), deltas 0x0100, 0xFFFF=-1.
        let data = [0x41u8, 0x01, 0x00, 0xFF, 0xFF];
        let mut r = BinaryReader::new(&data);
        let out = read_packed_deltas_count(&mut r, 2).unwrap();
        assert_eq!(out, vec![256i16, -1]);
    }

    #[test]
    fn packed_deltas_zero_run() {
        // control=0x84 (zero, len=5). Без следующих байт.
        let data = [0x84u8];
        let mut r = BinaryReader::new(&data);
        let out = read_packed_deltas_count(&mut r, 5).unwrap();
        assert_eq!(out, vec![0, 0, 0, 0, 0]);
    }

    #[test]
    fn packed_deltas_mixed_runs() {
        // 2 zeros, then 1 byte=10, then 1 word=300.
        // Control 0x81 (zero, len=2), control 0x00 (byte, len=1), data 10,
        // control 0x40 (word, len=1), data 0x012C=300.
        let data = [0x81u8, 0x00, 10, 0x40, 0x01, 0x2C];
        let mut r = BinaryReader::new(&data);
        let out = read_packed_deltas_count(&mut r, 4).unwrap();
        assert_eq!(out, vec![0i16, 0, 10, 300]);
    }

    #[test]
    fn packed_deltas_until_end_reads_all() {
        // 4 байтовых deltas с control 0x03 (byte, len=4): 10, -20, 30, -40.
        let data = [0x03u8, 10, 0xEC, 30, 0xD8];
        let mut r = BinaryReader::new(&data);
        let out = read_packed_deltas_until_end(&mut r).unwrap();
        assert_eq!(out, vec![10i16, -20, 30, -40]);
    }

    // ───────── tuple_scalar ─────────

    #[test]
    fn tuple_axis_scalar_no_intermediate_positive_peak() {
        // peak=1.0; default region = (0, 1).
        assert!((tuple_axis_scalar(0.0, 1.0, None) - 0.0).abs() < 1e-5);
        assert!((tuple_axis_scalar(0.5, 1.0, None) - 0.5).abs() < 1e-5);
        assert!((tuple_axis_scalar(1.0, 1.0, None) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn tuple_axis_scalar_no_intermediate_negative_peak() {
        // peak=-1.0; default region = (-1, 0).
        assert!((tuple_axis_scalar(-1.0, -1.0, None) - 1.0).abs() < 1e-5);
        assert!((tuple_axis_scalar(-0.5, -1.0, None) - 0.5).abs() < 1e-5);
        // polarity mismatch.
        assert!(tuple_axis_scalar(0.5, -1.0, None).abs() < 1e-5);
    }

    #[test]
    fn tuple_axis_scalar_intermediate_region() {
        // peak=0.5, intermediate (0.0, 1.0). coord=0.25 → (0.25-0)/(0.5-0)=0.5.
        assert!((tuple_axis_scalar(0.25, 0.5, Some((0.0, 1.0))) - 0.5).abs() < 1e-5);
        // coord=0.75 → (1.0-0.75)/(1.0-0.5)=0.5.
        assert!((tuple_axis_scalar(0.75, 0.5, Some((0.0, 1.0))) - 0.5).abs() < 1e-5);
        // Вне диапазона.
        assert!(tuple_axis_scalar(-0.1, 0.5, Some((0.0, 1.0))).abs() < 1e-5);
        assert!(tuple_axis_scalar(1.5, 0.5, Some((0.0, 1.0))).abs() < 1e-5);
    }

    #[test]
    fn tuple_axis_scalar_peak_zero_neutral() {
        // peak == 0 → всегда 1.0.
        assert!((tuple_axis_scalar(0.5, 0.0, None) - 1.0).abs() < 1e-5);
        assert!((tuple_axis_scalar(-1.0, 0.0, None) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn tuple_scalar_two_axes() {
        let v = TupleVariation {
            peak: vec![1.0, 0.5],
            intermediate: Some((vec![0.0, 0.0], vec![1.0, 1.0])),
            points: PointNumbers::All,
            x_deltas: vec![],
            y_deltas: vec![],
        };
        // coords=(1.0, 0.5) → 1.0 * 1.0 = 1.0.
        assert!((tuple_scalar(&[1.0, 0.5], &v) - 1.0).abs() < 1e-5);
        // coords=(0.5, 0.5) → 0.5 * 1.0 = 0.5.
        assert!((tuple_scalar(&[0.5, 0.5], &v) - 0.5).abs() < 1e-5);
        // coords=(1.0, 0.0) → 1.0 * 0.0 = 0.0 (на границе intermediate-region).
        assert!(tuple_scalar(&[1.0, 0.0], &v).abs() < 1e-5);
    }

    #[test]
    fn tuple_scalar_short_coords_treated_as_zero() {
        let v = TupleVariation {
            peak: vec![1.0, 1.0],
            intermediate: None,
            points: PointNumbers::All,
            x_deltas: vec![],
            y_deltas: vec![],
        };
        // coords длиной 1: первая axis даёт scalar(1.0) = 1.0; вторая —
        // отсутствует → coord=0 → axis-scalar=0.0 → итог 0.
        assert!(tuple_scalar(&[1.0], &v).abs() < 1e-5);
    }

    // ───────── full gvar table ─────────

    /// Минимальная gvar header без glyph-данных. axis_count=1,
    /// shared_tuple_count=0, glyph_count=2, short offsets, оба glyph-а пустые.
    fn build_minimal_gvar(glyph_count: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // major
        out.extend_from_slice(&0u16.to_be_bytes()); // minor
        out.extend_from_slice(&1u16.to_be_bytes()); // axis_count
        out.extend_from_slice(&0u16.to_be_bytes()); // shared_tuple_count
        out.extend_from_slice(&0u32.to_be_bytes()); // shared_tuples_offset
        out.extend_from_slice(&glyph_count.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // flags = short offsets
        // glyph_data_array_offset вычисляем: 20 header + (glyph_count+1)*2 offsets.
        let header_size = 20u32 + ((glyph_count as u32 + 1) * 2);
        out.extend_from_slice(&header_size.to_be_bytes());
        // All offsets = 0 → нет данных у glyph-ов.
        for _ in 0..=glyph_count {
            out.extend_from_slice(&0u16.to_be_bytes());
        }
        out
    }

    #[test]
    fn parses_empty_gvar() {
        let data = build_minimal_gvar(0);
        let gvar = Gvar::parse(&data).unwrap();
        assert_eq!(gvar.axis_count, 1);
        assert_eq!(gvar.glyph_count, 0);
        assert!(gvar.shared_tuples.is_empty());
    }

    #[test]
    fn parses_gvar_with_no_glyph_data() {
        let data = build_minimal_gvar(2);
        let gvar = Gvar::parse(&data).unwrap();
        assert_eq!(gvar.glyph_count, 2);
        assert!(gvar.glyph_variation_data(0).is_none());
        assert!(gvar.glyph_variation_data(1).is_none());
        assert!(gvar.parse_glyph(0).unwrap().is_none());
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_minimal_gvar(0);
        data[1] = 2; // major = 2.
        assert!(Gvar::parse(&data).is_err());
    }

    /// Собирает gvar с указанными shared_tuples и per-glyph data slices.
    /// Каждый glyph_blob — уже-собранный GlyphVariationData byte-stream.
    fn build_gvar(
        axis_count: u16,
        shared_tuples: &[Vec<f32>],
        long_offsets: bool,
        glyph_blobs: &[Vec<u8>],
    ) -> Vec<u8> {
        let glyph_count = glyph_blobs.len() as u16;
        let shared_tuple_count = shared_tuples.len() as u16;
        let offset_size = if long_offsets { 4 } else { 2 };
        let header_size = 20u32;
        let offsets_size = (glyph_count as u32 + 1) * offset_size;
        // Shared tuples идут сразу после offsets array.
        let shared_tuples_offset = header_size + offsets_size;
        let shared_tuples_bytes_len = shared_tuple_count as u32 * axis_count as u32 * 2;
        let glyph_data_array_offset = shared_tuples_offset + shared_tuples_bytes_len;

        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // major
        out.extend_from_slice(&0u16.to_be_bytes()); // minor
        out.extend_from_slice(&axis_count.to_be_bytes());
        out.extend_from_slice(&shared_tuple_count.to_be_bytes());
        out.extend_from_slice(&shared_tuples_offset.to_be_bytes());
        out.extend_from_slice(&glyph_count.to_be_bytes());
        let flags = if long_offsets { FLAG_LONG_OFFSETS } else { 0 };
        out.extend_from_slice(&flags.to_be_bytes());
        out.extend_from_slice(&glyph_data_array_offset.to_be_bytes());

        // Offsets: cumulative по blob-ам.
        let mut acc: u32 = 0;
        let push_offset = |out: &mut Vec<u8>, v: u32| {
            if long_offsets {
                out.extend_from_slice(&v.to_be_bytes());
            } else {
                // Short format хранит value/2; spec требует чтобы offsets
                // были выровнены на 2 байта.
                assert_eq!(v % 2, 0, "short offsets must be 2-byte aligned");
                out.extend_from_slice(&((v / 2) as u16).to_be_bytes());
            }
        };
        push_offset(&mut out, acc);
        for blob in glyph_blobs {
            acc += blob.len() as u32;
            push_offset(&mut out, acc);
        }

        // Shared tuples.
        for tuple in shared_tuples {
            assert_eq!(tuple.len(), axis_count as usize);
            for &v in tuple {
                out.extend_from_slice(&put_f2dot14(v));
            }
        }

        // Glyph data array.
        for blob in glyph_blobs {
            out.extend_from_slice(blob);
        }

        out
    }

    /// Собирает один GlyphVariationData блок с одним tuple-variation,
    /// embedded peak (без shared), without private/shared points, без
    /// intermediate. points = explicit list. deltas передаются как готовые
    /// runs (caller знает packing).
    fn build_simple_glyph_blob(
        peak: &[f32],
        point_numbers: &[u16],
        x_deltas: &[i16],
        y_deltas: &[i16],
    ) -> Vec<u8> {
        // Pack point numbers: count byte + один run всех байтов.
        let mut points_bytes = Vec::new();
        let count = point_numbers.len() as u8;
        points_bytes.push(count);
        if !point_numbers.is_empty() {
            // Run control: 0x00 + (count - 1) = (count-1) если меньше 128.
            // Bit 7 = 0 (byte deltas).
            assert!(count <= 128, "test helper supports ≤128 points");
            points_bytes.push(count - 1);
            let mut last: u16 = 0;
            for &p in point_numbers {
                let delta = p - last;
                assert!(delta < 256, "test helper supports byte-sized deltas");
                points_bytes.push(delta as u8);
                last = p;
            }
        }

        // Pack deltas как word-runs (надёжно для любых i16): для каждой
        // последовательности — control 0x40 | (len-1), затем len i16-be.
        let pack_word_run = |vals: &[i16]| -> Vec<u8> {
            let mut out = Vec::new();
            assert!(vals.len() <= 64, "test helper supports run ≤64");
            if !vals.is_empty() {
                out.push(0x40 | ((vals.len() as u8) - 1));
                for &v in vals {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            out
        };

        let x_packed = pack_word_run(x_deltas);
        let y_packed = pack_word_run(y_deltas);

        // Tuple variation header: variationDataSize, tupleIndex с
        // EMBEDDED_PEAK_TUPLE и PRIVATE_POINT_NUMBERS.
        let tuple_data_size =
            (points_bytes.len() + x_packed.len() + y_packed.len()) as u16;

        let mut header = Vec::new();
        header.extend_from_slice(&tuple_data_size.to_be_bytes());
        header.extend_from_slice(
            &(EMBEDDED_PEAK_TUPLE | PRIVATE_POINT_NUMBERS).to_be_bytes(),
        );
        for &p in peak {
            header.extend_from_slice(&put_f2dot14(p));
        }

        // GlyphVariationData header: tupleVariationCount, dataOffset.
        let mut blob = Vec::new();
        blob.extend_from_slice(&1u16.to_be_bytes()); // count = 1, без shared points
        // dataOffset = 4 (GVD header) + header.len()
        let data_offset = (4 + header.len()) as u16;
        blob.extend_from_slice(&data_offset.to_be_bytes());
        blob.extend_from_slice(&header);
        blob.extend_from_slice(&points_bytes);
        blob.extend_from_slice(&x_packed);
        blob.extend_from_slice(&y_packed);

        // Выравнивание до чётной длины (для short offsets).
        if blob.len() % 2 != 0 {
            blob.push(0);
        }
        blob
    }

    #[test]
    fn parses_single_glyph_one_tuple_explicit_points() {
        let blob = build_simple_glyph_blob(
            &[1.0],            // peak (1 axis)
            &[0, 1, 2],        // points
            &[10, -10, 20],    // x deltas
            &[5, -5, 0],       // y deltas
        );
        let data = build_gvar(1, &[], false, std::slice::from_ref(&blob));
        let gvar = Gvar::parse(&data).unwrap();
        assert_eq!(gvar.glyph_count, 1);
        let g = gvar.parse_glyph(0).unwrap().unwrap();
        assert_eq!(g.tuple_variations.len(), 1);
        let tv = &g.tuple_variations[0];
        assert!((tv.peak[0] - 1.0).abs() < 1e-3);
        assert!(tv.intermediate.is_none());
        match &tv.points {
            PointNumbers::Explicit(p) => assert_eq!(p, &[0u16, 1, 2]),
            _ => panic!("expected explicit"),
        }
        assert_eq!(tv.x_deltas, vec![10i16, -10, 20]);
        assert_eq!(tv.y_deltas, vec![5i16, -5, 0]);
    }

    #[test]
    fn parses_glyph_using_shared_tuple() {
        // shared_tuples = [[0.5]], glyph blob refers via TUPLE_INDEX 0.
        // Manually build minimal glyph blob с TUPLE_INDEX_MASK 0 (no
        // EMBEDDED_PEAK_TUPLE), PRIVATE_POINT_NUMBERS=1.
        let points_bytes: Vec<u8> = vec![1u8, 0x00, 5]; // 1 point, value=5
        let x_packed: Vec<u8> = vec![0x40, 0x00, 0x0A]; // word-run len=1, value=10
        let y_packed: Vec<u8> = vec![0x40, 0xFF, 0xF6]; // word-run len=1, value=-10
        let tuple_data_size =
            (points_bytes.len() + x_packed.len() + y_packed.len()) as u16;

        let mut blob = Vec::new();
        blob.extend_from_slice(&1u16.to_be_bytes()); // tupleVariationCount=1
        blob.extend_from_slice(&8u16.to_be_bytes()); // dataOffset = 4 + 4 = 8
        // tupleVariationHeader: variationDataSize + tupleIndex
        // tupleIndex = PRIVATE_POINT_NUMBERS | 0 (shared idx 0)
        blob.extend_from_slice(&tuple_data_size.to_be_bytes());
        blob.extend_from_slice(&PRIVATE_POINT_NUMBERS.to_be_bytes());
        // Embedded peak absent — shared tuple идёт через индекс.
        blob.extend_from_slice(&points_bytes);
        blob.extend_from_slice(&x_packed);
        blob.extend_from_slice(&y_packed);
        if blob.len() % 2 != 0 {
            blob.push(0);
        }

        let data = build_gvar(1, &[vec![0.5]], false, &[blob]);
        let gvar = Gvar::parse(&data).unwrap();
        let g = gvar.parse_glyph(0).unwrap().unwrap();
        let tv = &g.tuple_variations[0];
        assert!((tv.peak[0] - 0.5).abs() < 1e-3);
        match &tv.points {
            PointNumbers::Explicit(p) => assert_eq!(p, &[5u16]),
            _ => panic!("expected explicit"),
        }
        assert_eq!(tv.x_deltas, vec![10i16]);
        assert_eq!(tv.y_deltas, vec![-10i16]);
    }

    #[test]
    fn parses_glyph_all_points_mode() {
        // Один tuple с points=All: shared point numbers count=0 (means All)
        // флаг SHARED_POINT_NUMBERS выставлен в tupleVariationCount.
        // Без private points в самой tuple.

        // 4 deltas (x), 4 deltas (y) — пусть 8 точек в outline.
        let x: Vec<i16> = vec![1, 2, 3, 4];
        let y: Vec<i16> = vec![-1, -2, -3, -4];

        let pack_word_run = |vals: &[i16]| -> Vec<u8> {
            let mut out = Vec::new();
            if !vals.is_empty() {
                out.push(0x40 | ((vals.len() as u8) - 1));
                for &v in vals {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            out
        };
        let x_packed = pack_word_run(&x);
        let y_packed = pack_word_run(&y);
        let tuple_data_size = (x_packed.len() + y_packed.len()) as u16;

        // Shared points = [0u8] = «all».
        let shared_points_bytes: Vec<u8> = vec![0];

        let mut blob = Vec::new();
        // tupleVariationCount = 1 | SHARED_POINT_NUMBERS
        blob.extend_from_slice(&(1u16 | SHARED_POINT_NUMBERS).to_be_bytes());
        // dataOffset = 4 (GVD header) + 4 (single tuple header w/ embedded peak)+ 2 (peak)
        // = 4 + 8 = ... Compute below.
        // header: variationDataSize (2) + tupleIndex (2) + peak (1 axis × 2)
        // = 6 bytes.
        let header_size = 6u16;
        let data_offset = 4u16 + header_size;
        blob.extend_from_slice(&data_offset.to_be_bytes());
        // TupleVariationHeader: PRIVATE_POINT_NUMBERS НЕ set, чтобы tuple
        // использовала shared points.
        blob.extend_from_slice(&tuple_data_size.to_be_bytes());
        blob.extend_from_slice(&EMBEDDED_PEAK_TUPLE.to_be_bytes());
        blob.extend_from_slice(&put_f2dot14(1.0)); // peak[0]
        blob.extend_from_slice(&shared_points_bytes);
        blob.extend_from_slice(&x_packed);
        blob.extend_from_slice(&y_packed);
        if blob.len() % 2 != 0 {
            blob.push(0);
        }

        let data = build_gvar(1, &[], false, &[blob]);
        let gvar = Gvar::parse(&data).unwrap();
        let g = gvar.parse_glyph(0).unwrap().unwrap();
        let tv = &g.tuple_variations[0];
        assert!(matches!(tv.points, PointNumbers::All));
        assert_eq!(tv.x_deltas, x);
        assert_eq!(tv.y_deltas, y);
    }

    #[test]
    fn parses_glyph_intermediate_region() {
        // Tuple с INTERMEDIATE_REGION + EMBEDDED_PEAK_TUPLE.
        let points_bytes: Vec<u8> = vec![1u8, 0x00, 0]; // 1 point, value=0
        let x_packed: Vec<u8> = vec![0x40, 0x00, 0x05]; // word-run len=1, value=5
        let y_packed: Vec<u8> = vec![0x40, 0x00, 0x05];
        let tuple_data_size =
            (points_bytes.len() + x_packed.len() + y_packed.len()) as u16;

        let mut blob = Vec::new();
        blob.extend_from_slice(&1u16.to_be_bytes()); // count=1
        // dataOffset = 4 + (2+2+2+2+2) = 14. Header = 2+2+2 (peak) + 2 (start) + 2 (end) = 10.
        let data_offset = 4u16 + 10;
        blob.extend_from_slice(&data_offset.to_be_bytes());
        blob.extend_from_slice(&tuple_data_size.to_be_bytes());
        blob.extend_from_slice(
            &(EMBEDDED_PEAK_TUPLE | INTERMEDIATE_REGION | PRIVATE_POINT_NUMBERS).to_be_bytes(),
        );
        blob.extend_from_slice(&put_f2dot14(0.5)); // peak
        blob.extend_from_slice(&put_f2dot14(0.0)); // start
        blob.extend_from_slice(&put_f2dot14(1.0)); // end
        blob.extend_from_slice(&points_bytes);
        blob.extend_from_slice(&x_packed);
        blob.extend_from_slice(&y_packed);
        if blob.len() % 2 != 0 {
            blob.push(0);
        }

        let data = build_gvar(1, &[], false, &[blob]);
        let gvar = Gvar::parse(&data).unwrap();
        let g = gvar.parse_glyph(0).unwrap().unwrap();
        let tv = &g.tuple_variations[0];
        let (s, e) = tv.intermediate.as_ref().expect("intermediate present");
        assert!((s[0] - 0.0).abs() < 1e-3);
        assert!((e[0] - 1.0).abs() < 1e-3);
        assert!((tv.peak[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn parses_long_offsets_flag() {
        let blob = build_simple_glyph_blob(&[1.0], &[0], &[1], &[2]);
        let data = build_gvar(1, &[], true, &[blob]);
        let gvar = Gvar::parse(&data).unwrap();
        assert_eq!(gvar.flags & FLAG_LONG_OFFSETS, FLAG_LONG_OFFSETS);
        let g = gvar.parse_glyph(0).unwrap().unwrap();
        assert_eq!(g.tuple_variations.len(), 1);
    }

    #[test]
    fn parses_multiple_glyphs_with_mixed_data() {
        // Glyph 0: 1 tuple, 2 точки.
        let blob0 = build_simple_glyph_blob(&[1.0], &[0, 5], &[10, 20], &[1, 2]);
        // Glyph 1: пустой (нет вариаций).
        let blob1: Vec<u8> = vec![];
        // Glyph 2: 1 tuple, 1 точка.
        let blob2 = build_simple_glyph_blob(&[-1.0], &[3], &[-5], &[-7]);

        let data = build_gvar(1, &[], false, &[blob0, blob1, blob2]);
        let gvar = Gvar::parse(&data).unwrap();
        assert_eq!(gvar.glyph_count, 3);

        let g0 = gvar.parse_glyph(0).unwrap().unwrap();
        assert_eq!(g0.tuple_variations.len(), 1);
        assert_eq!(g0.tuple_variations[0].x_deltas.len(), 2);

        assert!(gvar.parse_glyph(1).unwrap().is_none());

        let g2 = gvar.parse_glyph(2).unwrap().unwrap();
        assert!((g2.tuple_variations[0].peak[0] - (-1.0)).abs() < 1e-3);
    }

    #[test]
    fn rejects_truncated_glyph_data() {
        let blob = build_simple_glyph_blob(&[1.0], &[0, 1], &[1, 2], &[3, 4]);
        let mut data = build_gvar(1, &[], false, &[blob]);
        // Отрезаем последние 4 байта — должны не суметь распарсить glyph.
        let truncated_len = data.len() - 4;
        data.truncate(truncated_len);
        let gvar = Gvar::parse(&data).unwrap();
        // Parser-уровень парсинг header-а проходит; падение происходит при
        // parse_glyph (т.к. slice короче декларированного variation_data_size).
        let res = gvar.parse_glyph(0);
        assert!(res.is_err() || res.unwrap().is_none());
    }

    #[test]
    fn glyph_id_out_of_range_returns_none() {
        let blob = build_simple_glyph_blob(&[1.0], &[0], &[1], &[2]);
        let data = build_gvar(1, &[], false, &[blob]);
        let gvar = Gvar::parse(&data).unwrap();
        assert!(gvar.glyph_variation_data(99).is_none());
        assert!(gvar.parse_glyph(99).unwrap().is_none());
    }

    #[test]
    fn rejects_shared_tuple_index_out_of_range() {
        // Создаём glyph blob с TUPLE_INDEX=5, но shared_tuples пуст.
        let points_bytes: Vec<u8> = vec![1u8, 0x00, 0];
        let x_packed: Vec<u8> = vec![0x40, 0x00, 0x01];
        let y_packed: Vec<u8> = vec![0x40, 0x00, 0x02];
        let tuple_data_size =
            (points_bytes.len() + x_packed.len() + y_packed.len()) as u16;
        let mut blob = Vec::new();
        blob.extend_from_slice(&1u16.to_be_bytes());
        blob.extend_from_slice(&8u16.to_be_bytes()); // dataOffset = 4 + 4
        blob.extend_from_slice(&tuple_data_size.to_be_bytes());
        blob.extend_from_slice(&(PRIVATE_POINT_NUMBERS | 5).to_be_bytes());
        blob.extend_from_slice(&points_bytes);
        blob.extend_from_slice(&x_packed);
        blob.extend_from_slice(&y_packed);
        if blob.len() % 2 != 0 {
            blob.push(0);
        }
        let data = build_gvar(1, &[], false, &[blob]);
        let gvar = Gvar::parse(&data).unwrap();
        assert!(gvar.parse_glyph(0).is_err());
    }
}
