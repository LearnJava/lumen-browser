//! `ItemVariationStore` — общий контейнер для variation deltas в OpenType
//! Variable Fonts (HVAR / MVAR / gvar / cvar / BASE-итд.). Хранит:
//! - список **variation regions** (peak/start/end на каждой оси —
//!   tent-функция: даёт 1.0 в peak-точке, 0.0 за пределами `[start, end]`);
//! - набор **item variation data blocks**, каждый из которых описывает
//!   per-item deltas (`i32`), привязанные к подмножеству регионов.
//!
//! При runtime caller вычисляет «scalar» каждого региона по текущим
//! нормализованным axis-coords, умножает на delta-значения соответствующих
//! items и суммирует. Phase 0 — только parser; `evaluate(coords)` для
//! tent-функции добавится вместе с первым consumer-ом (HVAR), где её
//! поведение можно валидировать на реальных variation deltas.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/otvarcommonformats>.
//!
//! Phase 0 ограничения:
//! - Только format 1 (`ItemVariationStore` v1). Format 2 (с long-formed
//!   delta sets) появился в HVAR v2 — добавим, когда консьюмер потребует.
//! - DeltaSetIndexMap (используется HVAR для glyph_id → (outer, inner)
//!   маппинга) — отдельный тип, добавится при подключении HVAR.

use crate::binary::BinaryReader;
use crate::face::FontError;

const IVS: [u8; 4] = *b"ivs?"; // используется только для FontError::InvalidTable

/// Один axis-сегмент региона: tent-функция со scalar = 1.0 в peak,
/// линейно убывающим до 0 в start и end. Координаты в F2DOT14
/// (i16/16384, диапазон −2.0..2.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionAxisCoordinates {
    pub start: f32,
    pub peak: f32,
    pub end: f32,
}

impl RegionAxisCoordinates {
    /// Per-axis scalar для tent-функции в `coord`. Возвращает значение
    /// в `[0.0, 1.0]`. Правила per OpenType spec
    /// (<https://learn.microsoft.com/en-us/typography/opentype/spec/otvarcommonformats#variation-regions>):
    /// - `peak == 0` → scalar = 1.0 (axis нейтральная, регион всегда
    ///   активен независимо от coord);
    /// - `coord == peak` → 1.0;
    /// - `coord < start || coord > end` → 0.0;
    /// - `start < 0 && peak > 0 && coord > 0`: clamp `start = 0` (axis-
    ///   side mismatch), и наоборот для negative peak / positive coord;
    /// - `coord < peak` → линейная интерполяция `(coord - start) /
    ///   (peak - start)`;
    /// - `coord > peak` → `(end - coord) / (end - peak)`.
    pub fn scalar(&self, coord: f32) -> f32 {
        // Spec corner case: peak == 0 — axis нейтральная, scalar = 1.0
        // для любой coord (регион применяется всегда).
        if self.peak == 0.0 {
            return 1.0;
        }
        // Точно в peak — scalar = 1.0. Это покрывает «start = peak = end»
        // degenerate case и обычный coord == peak.
        if coord == self.peak {
            return 1.0;
        }
        // Spec: peak > 0 + coord < 0 (или peak < 0 + coord > 0) → 0.
        // Polarity mismatch.
        if (self.peak > 0.0 && coord < 0.0) || (self.peak < 0.0 && coord > 0.0) {
            return 0.0;
        }
        // Outside [start, end] → 0.0. Используем strict inequality:
        // coord == start (или == end) — спорный edge case; spec говорит
        // 0 для «outside», т.е. для == границы — 0 (граница не включена).
        if coord < self.start || coord > self.end {
            return 0.0;
        }
        if coord < self.peak {
            let denom = self.peak - self.start;
            if denom == 0.0 {
                return 0.0;
            }
            (coord - self.start) / denom
        } else {
            // coord > peak.
            let denom = self.end - self.peak;
            if denom == 0.0 {
                return 0.0;
            }
            (self.end - coord) / denom
        }
    }
}

/// Один variation region — кортеж `RegionAxisCoordinates` на каждую ось.
/// Количество элементов = `VariationRegionList::axis_count`.
#[derive(Debug, Clone, PartialEq)]
pub struct VariationRegion {
    pub axes: Vec<RegionAxisCoordinates>,
}

impl VariationRegion {
    /// Региональный scalar — произведение per-axis scalars. Region
    /// активен (scalar > 0) только если все axes активны одновременно.
    ///
    /// `coords` — нормализованные координаты (после `avar`) длиной
    /// `axes.len()`. Если короче — недостающие axes считаются равными
    /// 0.0 (default position, нейтральные). Если длиннее — лишние
    /// игнорируются.
    pub fn scalar(&self, coords: &[f32]) -> f32 {
        let mut acc = 1.0_f32;
        for (i, axis) in self.axes.iter().enumerate() {
            let c = coords.get(i).copied().unwrap_or(0.0);
            acc *= axis.scalar(c);
            if acc == 0.0 {
                return 0.0;
            }
        }
        acc
    }
}

/// Список всех регионов, на которые могут ссылаться item-variation-data
/// blocks по `region_index`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VariationRegionList {
    pub axis_count: u16,
    pub regions: Vec<VariationRegion>,
}

/// Блок per-item delta-наборов: для `item_count` items, каждый item
/// содержит `region_indexes.len()` deltas, привязанных к регионам через
/// `region_indexes`.
///
/// Spec format: первые `word_delta_count` deltas на item — `i16`,
/// остальные — `i8`. Парсер преобразует всё в `i32` для единого
/// runtime-формата (compute scalar * delta даёт f32; integer storage
/// устойчив к overflow при суммировании).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemVariationData {
    /// Индексы регионов из `VariationRegionList`, на которые ссылаются
    /// deltas.
    pub region_indexes: Vec<u16>,
    /// `delta_sets[item_index][delta_index_within_item]`. Длина
    /// внешнего vector = `item_count`, длина внутреннего =
    /// `region_indexes.len()`.
    pub delta_sets: Vec<Vec<i32>>,
}

/// Root variation store. `format == 1` для всех современных шрифтов.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemVariationStore {
    pub format: u16,
    pub region_list: VariationRegionList,
    pub data_blocks: Vec<ItemVariationData>,
}

impl ItemVariationStore {
    /// Parses an `ItemVariationStore` starting at the beginning of `data`.
    /// Phase 0: only format == 1.
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let format = r.read_u16().ok_or(FontError::InvalidTable(IVS))?;
        if format != 1 {
            return Err(FontError::InvalidTable(IVS));
        }
        let region_list_offset = r.read_u32().ok_or(FontError::InvalidTable(IVS))? as usize;
        let item_variation_data_count =
            r.read_u16().ok_or(FontError::InvalidTable(IVS))? as usize;
        // Читаем item-variation-data offsets массива; offsets от начала
        // ItemVariationStore (= base data slice).
        let mut data_offsets = Vec::with_capacity(item_variation_data_count);
        for _ in 0..item_variation_data_count {
            data_offsets.push(r.read_u32().ok_or(FontError::InvalidTable(IVS))? as usize);
        }

        // VariationRegionList @ region_list_offset.
        let region_list = parse_region_list(data, region_list_offset)?;

        // ItemVariationData blocks @ data_offsets[i].
        let mut data_blocks = Vec::with_capacity(item_variation_data_count);
        for &off in &data_offsets {
            data_blocks.push(parse_item_variation_data(data, off)?);
        }

        Ok(Self {
            format,
            region_list,
            data_blocks,
        })
    }

    /// Вычисляет суммарный delta для item `(outer, inner)` при текущих
    /// нормализованных axis-coordinates. Алгоритм per OpenType spec:
    /// sum_i (region_scalar[region_indexes[i]] * delta_sets[inner][i]).
    ///
    /// `coords` — нормализованные axis values (после `avar`), длина
    /// должна совпадать с `region_list.axis_count`. Короткий vec
    /// доколачивается нулями (per `VariationRegion::scalar`).
    ///
    /// Возвращает `None`, если `outer` / `inner` вне диапазона —
    /// caller должен использовать base-метрику без variation (типичный
    /// fallback для битых HVAR-мапов).
    pub fn evaluate(&self, outer: u16, inner: u16, coords: &[f32]) -> Option<f32> {
        let block = self.data_blocks.get(outer as usize)?;
        let delta_set = block.delta_sets.get(inner as usize)?;
        let mut sum = 0.0_f32;
        for (i, &region_index) in block.region_indexes.iter().enumerate() {
            let region = self.region_list.regions.get(region_index as usize)?;
            let scalar = region.scalar(coords);
            if scalar == 0.0 {
                continue;
            }
            // delta хранится как i32 (i16 word или i8 byte после parse),
            // scalar в [0, 1]; результат — f32.
            let delta = *delta_set.get(i)? as f32;
            sum += scalar * delta;
        }
        Some(sum)
    }

    /// `true`, если store не содержит ни регионов, ни data blocks —
    /// эквивалентно «нет вариаций» (для HVAR/MVAR такой store означает
    /// «использовать base values из main table без variations»).
    pub fn is_empty(&self) -> bool {
        self.region_list.regions.is_empty() && self.data_blocks.is_empty()
    }
}

fn parse_region_list(data: &[u8], offset: usize) -> Result<VariationRegionList, FontError> {
    if offset >= data.len() {
        return Err(FontError::InvalidTable(IVS));
    }
    let mut r = BinaryReader::new(&data[offset..]);
    let axis_count = r.read_u16().ok_or(FontError::InvalidTable(IVS))?;
    let region_count = r.read_u16().ok_or(FontError::InvalidTable(IVS))? as usize;
    let mut regions = Vec::with_capacity(region_count);
    for _ in 0..region_count {
        let mut axes = Vec::with_capacity(axis_count as usize);
        for _ in 0..axis_count {
            let start = read_f2dot14(&mut r).ok_or(FontError::InvalidTable(IVS))?;
            let peak = read_f2dot14(&mut r).ok_or(FontError::InvalidTable(IVS))?;
            let end = read_f2dot14(&mut r).ok_or(FontError::InvalidTable(IVS))?;
            axes.push(RegionAxisCoordinates { start, peak, end });
        }
        regions.push(VariationRegion { axes });
    }
    Ok(VariationRegionList {
        axis_count,
        regions,
    })
}

fn parse_item_variation_data(data: &[u8], offset: usize) -> Result<ItemVariationData, FontError> {
    if offset >= data.len() {
        return Err(FontError::InvalidTable(IVS));
    }
    let mut r = BinaryReader::new(&data[offset..]);
    let item_count = r.read_u16().ok_or(FontError::InvalidTable(IVS))? as usize;
    let word_delta_count = r.read_u16().ok_or(FontError::InvalidTable(IVS))? as usize;
    let region_index_count = r.read_u16().ok_or(FontError::InvalidTable(IVS))? as usize;

    // Spec позволяет word_delta_count > region_index_count (LONG_WORDS bit
    // в format 2 — здесь не поддерживается); для format 1 предполагаем
    // word_delta_count <= region_index_count.
    if word_delta_count > region_index_count {
        return Err(FontError::InvalidTable(IVS));
    }

    let mut region_indexes = Vec::with_capacity(region_index_count);
    for _ in 0..region_index_count {
        region_indexes.push(r.read_u16().ok_or(FontError::InvalidTable(IVS))?);
    }

    let mut delta_sets = Vec::with_capacity(item_count);
    for _ in 0..item_count {
        let mut set = Vec::with_capacity(region_index_count);
        for i in 0..region_index_count {
            let v = if i < word_delta_count {
                i32::from(r.read_i16().ok_or(FontError::InvalidTable(IVS))?)
            } else {
                i32::from(r.read_u8().ok_or(FontError::InvalidTable(IVS))? as i8)
            };
            set.push(v);
        }
        delta_sets.push(set);
    }

    Ok(ItemVariationData {
        region_indexes,
        delta_sets,
    })
}

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

    /// Строит синтетический ItemVariationStore.
    /// regions: `&[Vec<(start, peak, end)>]` где внешняя длина = regionCount,
    /// внутренняя = axisCount (одинаковая у всех регионов).
    /// data_blocks: `&[(word_count, &[region_indexes], &[item_deltas])]`.
    /// item_deltas — flat `Vec<i32>` длиной `items * region_indexes.len()`.
    fn build_store(
        axis_count: u16,
        regions: &[Vec<(f32, f32, f32)>],
        data_blocks: &[(u16, Vec<u16>, Vec<i32>)],
    ) -> Vec<u8> {
        // Header (8 bytes) + variable-length data_offsets array.
        let header_size = 8 + 4 * data_blocks.len();
        let region_list_offset = header_size as u32;

        // Layout the regions блок.
        let mut region_list_bytes = Vec::new();
        region_list_bytes.extend_from_slice(&axis_count.to_be_bytes());
        region_list_bytes.extend_from_slice(&(regions.len() as u16).to_be_bytes());
        for region in regions {
            assert_eq!(region.len(), axis_count as usize);
            for &(start, peak, end) in region {
                region_list_bytes.extend_from_slice(&put_f2dot14(start));
                region_list_bytes.extend_from_slice(&put_f2dot14(peak));
                region_list_bytes.extend_from_slice(&put_f2dot14(end));
            }
        }

        // Variant data blocks — каждый со своим offset-ом от начала
        // ItemVariationStore. Кладём их подряд после region list.
        let mut blocks_bytes: Vec<Vec<u8>> = Vec::new();
        let mut next_offset = header_size + region_list_bytes.len();
        let mut data_offsets = Vec::new();
        for (word_count, region_indexes, deltas) in data_blocks {
            data_offsets.push(next_offset as u32);
            let mut b = Vec::new();
            let item_count = if region_indexes.is_empty() {
                0
            } else {
                deltas.len() / region_indexes.len()
            };
            b.extend_from_slice(&(item_count as u16).to_be_bytes());
            b.extend_from_slice(&word_count.to_be_bytes());
            b.extend_from_slice(&(region_indexes.len() as u16).to_be_bytes());
            for ri in region_indexes {
                b.extend_from_slice(&ri.to_be_bytes());
            }
            for (i, &v) in deltas.iter().enumerate() {
                let pos_in_item = i % region_indexes.len();
                if pos_in_item < *word_count as usize {
                    b.extend_from_slice(&(v as i16).to_be_bytes());
                } else {
                    b.push(v as i8 as u8);
                }
            }
            next_offset += b.len();
            blocks_bytes.push(b);
        }

        // Assemble: header + region_list + blocks.
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // format
        out.extend_from_slice(&region_list_offset.to_be_bytes());
        out.extend_from_slice(&(data_blocks.len() as u16).to_be_bytes());
        for o in &data_offsets {
            out.extend_from_slice(&o.to_be_bytes());
        }
        out.extend_from_slice(&region_list_bytes);
        for b in &blocks_bytes {
            out.extend_from_slice(b);
        }
        out
    }

    #[test]
    fn parses_empty_store() {
        // axisCount=0, regionCount=0, no data blocks.
        let data = build_store(0, &[], &[]);
        let store = ItemVariationStore::parse(&data).unwrap();
        assert_eq!(store.format, 1);
        assert_eq!(store.region_list.axis_count, 0);
        assert_eq!(store.region_list.regions.len(), 0);
        assert_eq!(store.data_blocks.len(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn parses_single_region_single_axis() {
        let data = build_store(1, &[vec![(-1.0, 1.0, 1.0)]], &[]);
        let store = ItemVariationStore::parse(&data).unwrap();
        assert_eq!(store.region_list.axis_count, 1);
        assert_eq!(store.region_list.regions.len(), 1);
        let r = &store.region_list.regions[0];
        assert_eq!(r.axes.len(), 1);
        assert!((r.axes[0].start - (-1.0)).abs() < 1e-3);
        assert!((r.axes[0].peak - 1.0).abs() < 1e-3);
        assert!((r.axes[0].end - 1.0).abs() < 1e-3);
    }

    #[test]
    fn parses_multi_axis_region() {
        // 2 axes (например, wght + wdth), один регион.
        let data = build_store(
            2,
            &[vec![(0.0, 1.0, 1.0), (-1.0, 0.0, 0.5)]],
            &[],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert_eq!(store.region_list.axis_count, 2);
        let r = &store.region_list.regions[0];
        assert_eq!(r.axes.len(), 2);
        assert!((r.axes[1].peak - 0.0).abs() < 1e-3);
    }

    #[test]
    fn parses_data_block_word_deltas() {
        // 1 axis / 1 region; data block: 2 items × 1 delta (word).
        // word_count = 1, region_indexes = [0], deltas = [100, -200].
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![100, -200])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert_eq!(store.data_blocks.len(), 1);
        let block = &store.data_blocks[0];
        assert_eq!(block.region_indexes, vec![0u16]);
        assert_eq!(block.delta_sets, vec![vec![100i32], vec![-200i32]]);
    }

    #[test]
    fn parses_data_block_mixed_word_and_byte_deltas() {
        // 1 region per index, region_indexes = [0, 1, 2], word_count=1.
        // Так первая delta i16 (word), остальные две — i8 (byte).
        // 2 items: item0 = [500, 7, -8]; item1 = [-300, 127, -128].
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)], vec![(0.0, 0.5, 1.0)], vec![(-1.0, -0.5, 0.0)]],
            &[(1, vec![0, 1, 2], vec![500, 7, -8, -300, 127, -128])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        let block = &store.data_blocks[0];
        assert_eq!(block.delta_sets[0], vec![500, 7, -8]);
        assert_eq!(block.delta_sets[1], vec![-300, 127, -128]);
    }

    #[test]
    fn parses_multiple_data_blocks() {
        // 1 region; два data blocks: первый — 1 item, второй — 2 items.
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[
                (1, vec![0], vec![42]),
                (1, vec![0], vec![-10, 20]),
            ],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert_eq!(store.data_blocks.len(), 2);
        assert_eq!(store.data_blocks[0].delta_sets, vec![vec![42]]);
        assert_eq!(store.data_blocks[1].delta_sets, vec![vec![-10], vec![20]]);
    }

    #[test]
    fn rejects_unsupported_format() {
        let mut data = build_store(0, &[], &[]);
        // format на offset 0; меняем 1 → 2 (long-formed deltas, не поддерж).
        data[1] = 2;
        assert!(ItemVariationStore::parse(&data).is_err());
    }

    #[test]
    fn rejects_word_count_exceeding_region_count() {
        // Создаём data block c word_count > region_index_count — это
        // невалид по format 1.
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(5, vec![0], vec![])], // word_count=5 > region_indexes.len()=1
        );
        assert!(ItemVariationStore::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_data() {
        let data = build_store(1, &[vec![(0.0, 1.0, 1.0)]], &[(1, vec![0], vec![100])]);
        let truncated = &data[..data.len() - 1];
        assert!(ItemVariationStore::parse(truncated).is_err());
    }

    #[test]
    fn is_empty_only_when_no_regions_and_no_data() {
        // Store с регионом, но без data — НЕ пустой (data blocks могут
        // быть в другой таблице; здесь регион уже non-trivial).
        let with_region = ItemVariationStore::parse(&build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[],
        ))
        .unwrap();
        assert!(!with_region.is_empty());

        let totally_empty = ItemVariationStore::parse(&build_store(0, &[], &[])).unwrap();
        assert!(totally_empty.is_empty());
    }

    #[test]
    fn region_axis_coordinates_clamp_range_to_2dot14() {
        // F2DOT14 имеет диапазон −2.0 ... 1.99999... — но реальные регионы
        // в OpenType держатся в −1..1. Просто проверяем round-trip
        // отрицательной end-точки.
        let data = build_store(1, &[vec![(-1.0, -0.5, 0.0)]], &[]);
        let store = ItemVariationStore::parse(&data).unwrap();
        let axis = &store.region_list.regions[0].axes[0];
        assert!((axis.start - (-1.0)).abs() < 1e-3);
        assert!((axis.peak - (-0.5)).abs() < 1e-3);
        assert!((axis.end - 0.0).abs() < 1e-3);
    }

    // ───── scalar (tent-function) ─────

    fn axis(start: f32, peak: f32, end: f32) -> RegionAxisCoordinates {
        RegionAxisCoordinates { start, peak, end }
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn scalar_peak_zero_always_one() {
        // peak == 0 → axis нейтральная: scalar = 1.0 для любого coord.
        let a = axis(-1.0, 0.0, 1.0);
        assert!(approx(a.scalar(-0.5), 1.0));
        assert!(approx(a.scalar(0.0), 1.0));
        assert!(approx(a.scalar(0.5), 1.0));
    }

    #[test]
    fn scalar_at_peak_is_one() {
        let a = axis(0.0, 1.0, 1.0);
        assert!(approx(a.scalar(1.0), 1.0));
    }

    #[test]
    fn scalar_below_start_is_zero() {
        let a = axis(0.0, 1.0, 1.0);
        assert!(approx(a.scalar(-0.1), 0.0));
        assert!(approx(a.scalar(0.0), 0.0));
    }

    #[test]
    fn scalar_above_end_is_zero() {
        let a = axis(0.0, 1.0, 1.0);
        // coord == end → scalar = 0 (exclusive per spec).
        assert!(approx(a.scalar(1.0), 1.0)); // на peak = end
        let a2 = axis(0.0, 0.5, 1.0);
        assert!(approx(a2.scalar(1.0), 0.0));
        assert!(approx(a2.scalar(1.5), 0.0));
    }

    #[test]
    fn scalar_linear_below_peak() {
        // start=0, peak=1, end=1. coord=0.5 → (0.5 - 0)/(1.0 - 0) = 0.5.
        let a = axis(0.0, 1.0, 1.0);
        assert!(approx(a.scalar(0.5), 0.5));
        assert!(approx(a.scalar(0.25), 0.25));
    }

    #[test]
    fn scalar_linear_above_peak() {
        // start=-1, peak=0.5, end=1. coord=0.75 → (1.0 - 0.75)/(1.0 - 0.5) = 0.5.
        let a = axis(-1.0, 0.5, 1.0);
        assert!(approx(a.scalar(0.75), 0.5));
    }

    #[test]
    fn scalar_polarity_mismatch_returns_zero() {
        // Spec: peak > 0, coord < 0 → 0.
        let a = axis(-1.0, 0.5, 1.0);
        assert!(approx(a.scalar(-0.3), 0.0));
        // И наоборот: peak < 0, coord > 0 → 0.
        let a2 = axis(-1.0, -0.5, 1.0);
        assert!(approx(a2.scalar(0.3), 0.0));
    }

    #[test]
    fn region_scalar_is_product_of_axis_scalars() {
        // Регион с двумя осями: scalar = axis0.scalar(c0) * axis1.scalar(c1).
        let r = VariationRegion {
            axes: vec![axis(0.0, 1.0, 1.0), axis(0.0, 0.5, 1.0)],
        };
        // c0=1.0 → 1.0; c1=0.25 → (0.25-0)/(0.5-0) = 0.5. итог 0.5.
        assert!(approx(r.scalar(&[1.0, 0.25]), 0.5));
        // c1=0.0 → 0.0; итог 0.0.
        assert!(approx(r.scalar(&[1.0, 0.0]), 0.0));
    }

    #[test]
    fn region_scalar_short_coords_treated_as_zero() {
        // Coords длиной 1 при axes длиной 2 — недостающая считается 0.0.
        // c1 = 0.0 → axis1.scalar(0.0) = 0.0 → региональный scalar = 0.0.
        let r = VariationRegion {
            axes: vec![axis(0.0, 1.0, 1.0), axis(0.0, 0.5, 1.0)],
        };
        assert!(approx(r.scalar(&[1.0]), 0.0));
    }

    // ───── ItemVariationStore::evaluate ─────

    #[test]
    fn evaluate_at_peak_returns_full_delta() {
        // 1 axis, 1 region peak at c=1; 1 data block с 1 item × 1 delta = 100.
        // coord = [1.0] → scalar = 1.0 → delta = 100.0.
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![100])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert!(approx(store.evaluate(0, 0, &[1.0]).unwrap(), 100.0));
    }

    #[test]
    fn evaluate_at_default_returns_zero() {
        // coord = [0.0] → axis на peak=1 даёт 0 → delta = 0.
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![100])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert!(approx(store.evaluate(0, 0, &[0.0]).unwrap(), 0.0));
    }

    #[test]
    fn evaluate_at_midpoint_interpolates() {
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![100])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        // c=0.5 → scalar=0.5 → delta = 50.
        assert!(approx(store.evaluate(0, 0, &[0.5]).unwrap(), 50.0));
    }

    #[test]
    fn evaluate_sums_multiple_regions() {
        // 1 axis, 2 регионов: peak=1.0 и peak=-1.0. Один item с двумя
        // deltas (100, 200), привязан к обоим регионам через region_indexes
        // = [0, 1]. coord=1.0 → region0.scalar=1.0, region1.scalar=0.0 →
        // итог = 100.
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)], vec![(-1.0, -1.0, 0.0)]],
            &[(2, vec![0, 1], vec![100, 200])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert!(approx(store.evaluate(0, 0, &[1.0]).unwrap(), 100.0));
        // coord=-1.0 → region0=0, region1=1 → итог = 200.
        assert!(approx(store.evaluate(0, 0, &[-1.0]).unwrap(), 200.0));
    }

    #[test]
    fn evaluate_multiple_items_independent() {
        // 1 region, 2 items: deltas [10] и [-30]. evaluate должна различать.
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![10, -30])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert!(approx(store.evaluate(0, 0, &[1.0]).unwrap(), 10.0));
        assert!(approx(store.evaluate(0, 1, &[1.0]).unwrap(), -30.0));
    }

    #[test]
    fn evaluate_out_of_range_returns_none() {
        let data = build_store(
            1,
            &[vec![(0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![100])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        // outer вне диапазона.
        assert!(store.evaluate(99, 0, &[1.0]).is_none());
        // inner вне диапазона.
        assert!(store.evaluate(0, 99, &[1.0]).is_none());
    }

    #[test]
    fn evaluate_two_axis_region() {
        // 2 axes; 1 регион (peak=(1, 1)); 1 delta = 100.
        // coord=(1, 1) → scalar 1.0 → 100; coord=(1, 0.5) → 1.0 * 0.5 = 0.5 → 50.
        let data = build_store(
            2,
            &[vec![(0.0, 1.0, 1.0), (0.0, 1.0, 1.0)]],
            &[(1, vec![0], vec![100])],
        );
        let store = ItemVariationStore::parse(&data).unwrap();
        assert!(approx(store.evaluate(0, 0, &[1.0, 1.0]).unwrap(), 100.0));
        assert!(approx(store.evaluate(0, 0, &[1.0, 0.5]).unwrap(), 50.0));
        // c0=0.0 → axis0.scalar=0 → итог 0.0.
        assert!(approx(store.evaluate(0, 0, &[0.0, 1.0]).unwrap(), 0.0));
    }
}
