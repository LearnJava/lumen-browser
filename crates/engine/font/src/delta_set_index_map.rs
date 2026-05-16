//! `DeltaSetIndexMap` — таблица отображения 16-/32-битного входного индекса
//! (обычно glyph_id) в пару `(outerIndex, innerIndex)` для lookup в
//! `ItemVariationStore`. Используется в `HVAR` / `VVAR` (один map для
//! advance, опционально другой для LSB/TSB) и `MVAR` (отдельные maps на
//! каждый metrics tag).
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/otvarcommonformats#delta-set-index-map>.
//!
//! Формат entry packed в `entry_size` байт (1..4):
//! - выровненное BE-целое число;
//! - младшие `inner_bit_count` бит → `inner`;
//! - старшие → `outer`.
//!
//! `entryFormat` byte:
//! - bits 0-3 (`INNER_INDEX_BIT_COUNT_MASK`): inner bit count − 1 (1..16).
//! - bits 4-5 (`MAP_ENTRY_SIZE_MASK`): entry size in bytes − 1 (1..4).
//! - bits 6-7: reserved (должны быть 0).
//!
//! Phase 0: format 0 (16-bit map_count) и format 1 (32-bit map_count).
//! Поведение при out-of-range glyph_id per spec: использовать **последнюю**
//! запись (не error, не identity).

use crate::binary::BinaryReader;
use crate::face::FontError;

const DSIM: [u8; 4] = *b"dsim";

/// Распакованный entry: пара индексов для lookup в `ItemVariationStore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeltaSetIndex {
    pub outer: u16,
    pub inner: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeltaSetIndexMap {
    /// Format 0 или 1; влияет только на размер map_count в заголовке.
    pub format: u8,
    /// Все entries, распакованные при парсинге для O(1) lookup.
    pub entries: Vec<DeltaSetIndex>,
}

impl DeltaSetIndexMap {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let format = r.read_u8().ok_or(FontError::InvalidTable(DSIM))?;
        let entry_format = r.read_u8().ok_or(FontError::InvalidTable(DSIM))?;
        if format > 1 {
            return Err(FontError::InvalidTable(DSIM));
        }
        // Reserved bits 6-7 в spec «должны быть 0»; реальные шрифты
        // соблюдают, но не валидируем строго — fail-open.
        let inner_bit_count = ((entry_format & 0x0F) + 1) as u32;
        let entry_size = (((entry_format >> 4) & 0x03) + 1) as usize;
        if !(1..=16).contains(&inner_bit_count) {
            return Err(FontError::InvalidTable(DSIM));
        }
        // entry_size 1..=4 уже гарантирован: 2-битное поле + 1.
        let map_count = if format == 0 {
            r.read_u16().ok_or(FontError::InvalidTable(DSIM))? as usize
        } else {
            r.read_u32().ok_or(FontError::InvalidTable(DSIM))? as usize
        };

        let inner_mask: u32 = if inner_bit_count == 32 {
            u32::MAX
        } else {
            (1u32 << inner_bit_count) - 1
        };

        let mut entries = Vec::with_capacity(map_count);
        for _ in 0..map_count {
            let raw = read_be_unsigned(&mut r, entry_size).ok_or(FontError::InvalidTable(DSIM))?;
            let inner = (raw & inner_mask) as u16;
            let outer_raw = raw >> inner_bit_count;
            // outer теоретически может превышать u16 если inner_bit_count
            // мал и entry_size = 4. Клампим к u16::MAX (это значит «битый
            // шрифт» — реальные HVAR ставят outer ≤ 65535 разумно).
            let outer = u16::try_from(outer_raw).unwrap_or(u16::MAX);
            entries.push(DeltaSetIndex { outer, inner });
        }

        Ok(Self { format, entries })
    }

    /// Возвращает `(outer, inner)` для glyph_id (или другого входного
    /// индекса). Per spec: для glyph_id ≥ len() используется **последняя**
    /// запись. Пустой map возвращает `DeltaSetIndex::default()` (0, 0) —
    /// валидно, поскольку ItemVariationStore без entries тоже отдаёт пусто.
    pub fn get(&self, index: u32) -> DeltaSetIndex {
        if self.entries.is_empty() {
            return DeltaSetIndex::default();
        }
        let idx = (index as usize).min(self.entries.len() - 1);
        self.entries[idx]
    }
}

/// Читает BE unsigned число длиной `len` байт (1..=4) → u32.
fn read_be_unsigned(r: &mut BinaryReader<'_>, len: usize) -> Option<u32> {
    let bytes = r.read_bytes(len)?;
    let mut v: u32 = 0;
    for &b in bytes {
        v = (v << 8) | u32::from(b);
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Кладёт entry заданной ширины в bytes (BE).
    fn put_entry(buf: &mut Vec<u8>, entry_size: usize, value: u32) {
        for i in (0..entry_size).rev() {
            buf.push(((value >> (8 * i)) & 0xFF) as u8);
        }
    }

    /// Собирает entry_format byte: inner_bit_count (1..=16) → bits 0-3,
    /// entry_size (1..=4) → bits 4-5.
    fn entry_format(inner_bit_count: u8, entry_size: u8) -> u8 {
        assert!((1..=16).contains(&inner_bit_count));
        assert!((1..=4).contains(&entry_size));
        ((inner_bit_count - 1) & 0x0F) | (((entry_size - 1) & 0x03) << 4)
    }

    /// Строит синтетический DeltaSetIndexMap.
    /// pairs — список (outer, inner) для каждой entry.
    fn build_map(
        format: u8,
        inner_bit_count: u8,
        entry_size: u8,
        pairs: &[(u16, u16)],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(format);
        out.push(entry_format(inner_bit_count, entry_size));
        let map_count = pairs.len();
        if format == 0 {
            out.extend_from_slice(&(map_count as u16).to_be_bytes());
        } else {
            out.extend_from_slice(&(map_count as u32).to_be_bytes());
        }
        let inner_mask = (1u32 << inner_bit_count) - 1;
        for &(outer, inner) in pairs {
            let raw = (u32::from(outer) << inner_bit_count) | (u32::from(inner) & inner_mask);
            put_entry(&mut out, entry_size as usize, raw);
        }
        out
    }

    #[test]
    fn parses_empty_format0() {
        let data = build_map(0, 8, 2, &[]);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.format, 0);
        assert_eq!(map.entries.len(), 0);
    }

    #[test]
    fn parses_single_entry_format0() {
        // inner_bit_count=8, entry_size=2 (16-bit entry): pair (3, 17).
        let data = build_map(0, 8, 2, &[(3, 17)]);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0], DeltaSetIndex { outer: 3, inner: 17 });
    }

    #[test]
    fn parses_multiple_entries() {
        let pairs = [(0, 0), (1, 5), (2, 100), (15, 200)];
        let data = build_map(0, 8, 2, &pairs);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        for (i, &(o, n)) in pairs.iter().enumerate() {
            assert_eq!(map.entries[i], DeltaSetIndex { outer: o, inner: n });
        }
    }

    #[test]
    fn parses_format1_with_large_map_count() {
        // format 1 с 5 entries.
        let pairs = [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)];
        let data = build_map(1, 4, 1, &pairs);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.format, 1);
        assert_eq!(map.entries.len(), 5);
        assert_eq!(map.entries[3], DeltaSetIndex { outer: 3, inner: 3 });
    }

    #[test]
    fn get_returns_entry_at_index() {
        let pairs = [(0, 10), (1, 20), (2, 30)];
        let data = build_map(0, 8, 2, &pairs);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.get(0), DeltaSetIndex { outer: 0, inner: 10 });
        assert_eq!(map.get(1), DeltaSetIndex { outer: 1, inner: 20 });
        assert_eq!(map.get(2), DeltaSetIndex { outer: 2, inner: 30 });
    }

    #[test]
    fn get_out_of_range_returns_last_entry() {
        // Per spec: glyph_id ≥ map_count → последняя entry.
        let pairs = [(0, 10), (1, 20)];
        let data = build_map(0, 8, 2, &pairs);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.get(5), DeltaSetIndex { outer: 1, inner: 20 });
        assert_eq!(map.get(1_000_000), DeltaSetIndex { outer: 1, inner: 20 });
    }

    #[test]
    fn get_on_empty_map_returns_default() {
        let data = build_map(0, 8, 2, &[]);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.get(0), DeltaSetIndex::default());
    }

    #[test]
    fn handles_4_byte_entry_size() {
        // entry_size=4 (max), inner_bit_count=16 → outer = upper 16 bits.
        let data = build_map(0, 16, 4, &[(40000, 25000)]);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(
            map.entries[0],
            DeltaSetIndex {
                outer: 40000,
                inner: 25000
            }
        );
    }

    #[test]
    fn handles_1_byte_entry_size() {
        // entry_size=1 (min), inner_bit_count=4 → outer 4 bits, inner 4 bits.
        let data = build_map(0, 4, 1, &[(7, 5), (15, 0)]);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.entries[0], DeltaSetIndex { outer: 7, inner: 5 });
        assert_eq!(map.entries[1], DeltaSetIndex { outer: 15, inner: 0 });
    }

    #[test]
    fn rejects_invalid_format() {
        let mut data = build_map(0, 8, 2, &[]);
        data[0] = 5; // format 5 — невалидно (только 0 и 1)
        assert!(DeltaSetIndexMap::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_data() {
        let data = build_map(0, 8, 2, &[(1, 2), (3, 4)]);
        // Обрезаем 1 байт (неполная вторая запись).
        let truncated = &data[..data.len() - 1];
        assert!(DeltaSetIndexMap::parse(truncated).is_err());
    }

    #[test]
    fn handles_min_inner_bit_count() {
        // inner_bit_count = 1 → outer берёт все остальные биты.
        // entry_size=2 (16 bit): inner 1 bit, outer 15 bits.
        let data = build_map(0, 1, 2, &[(100, 1), (200, 0)]);
        let map = DeltaSetIndexMap::parse(&data).unwrap();
        assert_eq!(map.entries[0], DeltaSetIndex { outer: 100, inner: 1 });
        assert_eq!(map.entries[1], DeltaSetIndex { outer: 200, inner: 0 });
    }

    #[test]
    fn format_field_preserved() {
        let f0 = DeltaSetIndexMap::parse(&build_map(0, 8, 2, &[])).unwrap();
        let f1 = DeltaSetIndexMap::parse(&build_map(1, 8, 2, &[])).unwrap();
        assert_eq!(f0.format, 0);
        assert_eq!(f1.format, 1);
    }
}
