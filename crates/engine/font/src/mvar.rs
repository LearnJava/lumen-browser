//! `MVAR` — Metrics Variations Table. Описывает variation deltas для
//! глобальных метрик шрифта (не per-glyph, а единых для всего face):
//! x-height, cap-height, underline position/thickness, strikeout
//! position/thickness, subscript/superscript offsets и др. Список
//! «известных» tag-ов задан spec-ом; шрифт может объявить любое
//! подмножество.
//!
//! При активном variation-instance runtime берёт base-метрики из
//! соответствующих таблиц (`OS/2`, `hhea`, `post`, ...) и прибавляет
//! delta, вычисленную через `ItemVariationStore.evaluate(coords)` для
//! записи с нужным tag-ом.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/mvar>.
//!
//! Phase 0 ограничения:
//! - Только v1.0.
//! - Без `evaluate(coords)` — Phase 0 не имеет axis-instance из CSS
//!   `font-variation-settings`. Caller получает `(outer, inner)` индекс
//!   для tag-а и сам комбинирует с base-метрикой.

use crate::binary::BinaryReader;
use crate::face::FontError;
use crate::item_variation::ItemVariationStore;

const MVAR: [u8; 4] = *b"MVAR";

/// Одна запись MVAR: tag метрики + (outer, inner) для lookup в IVS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueRecord {
    /// 4-байтовый tag метрики. Стандартные (OpenType spec table «MVAR
    /// value tags»): `b"xhgt"`, `b"cpht"`, `b"undo"`, `b"unds"`, `b"strs"`,
    /// `b"stro"`, `b"sbxs"`, `b"sbxy"`, `b"sbxo"`, `b"sbyo"`, `b"spxs"`,
    /// `b"spxy"`, `b"spxo"`, `b"spyo"`, `b"hasc"`, `b"hcla"`, `b"hcld"`,
    /// `b"hdsc"`, `b"hcrs"`, `b"hcrn"`, `b"hcof"`, `b"vasc"`, `b"vdsc"`,
    /// `b"vlgp"`, `b"vcrs"`, `b"vcrn"`, `b"vcof"`. Custom tag-и допустимы.
    pub tag: [u8; 4],
    pub delta_set_outer: u16,
    pub delta_set_inner: u16,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mvar {
    pub store: ItemVariationStore,
    /// Sorted by `tag` для бинарного поиска. Spec требует sort —
    /// проверяется через `is_sorted_by_tag`.
    pub records: Vec<ValueRecord>,
}

impl Mvar {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(MVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(MVAR))?;
        if major != 1 {
            return Err(FontError::InvalidTable(MVAR));
        }
        // reserved uint16
        r.skip(2).ok_or(FontError::InvalidTable(MVAR))?;
        let value_record_size = r.read_u16().ok_or(FontError::InvalidTable(MVAR))? as usize;
        let value_record_count = r.read_u16().ok_or(FontError::InvalidTable(MVAR))? as usize;
        let store_offset = r.read_u16().ok_or(FontError::InvalidTable(MVAR))? as usize;

        // Spec задаёт valueRecordSize = 8 (Tag + uint16 + uint16). Реальные
        // шрифты могут добавить trailing reserved bytes — поддержим этот
        // случай: парсим первые 8 байт каждой записи, остальные skip-аем.
        if value_record_size < 8 {
            return Err(FontError::InvalidTable(MVAR));
        }

        let mut records = Vec::with_capacity(value_record_count);
        for _ in 0..value_record_count {
            let tag = r.read_tag().ok_or(FontError::InvalidTable(MVAR))?;
            let outer = r.read_u16().ok_or(FontError::InvalidTable(MVAR))?;
            let inner = r.read_u16().ok_or(FontError::InvalidTable(MVAR))?;
            if value_record_size > 8 {
                r.skip(value_record_size - 8)
                    .ok_or(FontError::InvalidTable(MVAR))?;
            }
            records.push(ValueRecord {
                tag,
                delta_set_outer: outer,
                delta_set_inner: inner,
            });
        }

        // ItemVariationStore @ store_offset, или 0 если нет вариаций.
        let store = if store_offset == 0 {
            ItemVariationStore::default()
        } else {
            if store_offset >= data.len() {
                return Err(FontError::InvalidTable(MVAR));
            }
            ItemVariationStore::parse(&data[store_offset..])?
        };

        Ok(Self { store, records })
    }

    /// Lookup `(outer, inner)` для метрики по tag-у. `None`, если запись
    /// не объявлена шрифтом (caller использует base-метрику без variation).
    /// Бинарный поиск по sorted-records (per spec).
    pub fn lookup(&self, tag: &[u8; 4]) -> Option<&ValueRecord> {
        // Spec гарантирует sort; используем binary_search_by для O(log n).
        match self.records.binary_search_by(|r| r.tag.cmp(tag)) {
            Ok(idx) => Some(&self.records[idx]),
            Err(_) => None,
        }
    }

    /// Проверяет, что records отсортированы по tag — инвариант OpenType
    /// spec. Используется в тестах; в продакшен-коде `lookup` полагается
    /// на сортировку и даст неверный результат для несортированных шрифтов
    /// (что само по себе нарушение spec, но реальные шрифты сортируются).
    pub fn is_sorted_by_tag(&self) -> bool {
        self.records.windows(2).all(|w| w[0].tag <= w[1].tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимальный синтетический ItemVariationStore: format=1, 0 регионов,
    /// 0 data blocks. Возвращает байты длиной 12.
    fn build_minimal_ivs() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // format
        out.extend_from_slice(&8u32.to_be_bytes()); // region_list_offset
        out.extend_from_slice(&0u16.to_be_bytes()); // itemVariationDataCount
        // VariationRegionList @ offset 8: axisCount=0, regionCount=0.
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out
    }

    /// Строит синтетический MVAR с указанным набором records (tag, outer,
    /// inner). records должны быть sorted-by-tag.
    fn build_mvar(records: &[(&[u8; 4], u16, u16)]) -> Vec<u8> {
        let header_size = 12u16;
        let value_record_size = 8u16;
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // major
        out.extend_from_slice(&0u16.to_be_bytes()); // minor
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        out.extend_from_slice(&value_record_size.to_be_bytes());
        out.extend_from_slice(&(records.len() as u16).to_be_bytes());
        let ivs = build_minimal_ivs();
        let store_offset = header_size + (records.len() as u16) * value_record_size;
        out.extend_from_slice(&store_offset.to_be_bytes());
        // Value records:
        for (tag, outer, inner) in records {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&outer.to_be_bytes());
            out.extend_from_slice(&inner.to_be_bytes());
        }
        // ItemVariationStore @ store_offset:
        out.extend_from_slice(&ivs);
        out
    }

    #[test]
    fn parses_empty_mvar() {
        let data = build_mvar(&[]);
        let mvar = Mvar::parse(&data).unwrap();
        assert_eq!(mvar.records.len(), 0);
        assert!(mvar.store.is_empty());
    }

    #[test]
    fn parses_single_record() {
        let data = build_mvar(&[(b"xhgt", 0, 5)]);
        let mvar = Mvar::parse(&data).unwrap();
        assert_eq!(mvar.records.len(), 1);
        assert_eq!(
            mvar.records[0],
            ValueRecord {
                tag: *b"xhgt",
                delta_set_outer: 0,
                delta_set_inner: 5,
            }
        );
    }

    #[test]
    fn lookup_finds_existing_tag() {
        // Sorted by tag: cpht < undo < unds < xhgt в byte-order.
        let data = build_mvar(&[
            (b"cpht", 0, 1),
            (b"undo", 0, 2),
            (b"unds", 0, 3),
            (b"xhgt", 0, 4),
        ]);
        let mvar = Mvar::parse(&data).unwrap();
        assert!(mvar.is_sorted_by_tag());
        let v = mvar.lookup(b"undo").unwrap();
        assert_eq!(v.delta_set_inner, 2);
        let v = mvar.lookup(b"xhgt").unwrap();
        assert_eq!(v.delta_set_inner, 4);
    }

    #[test]
    fn lookup_returns_none_for_missing_tag() {
        let data = build_mvar(&[(b"xhgt", 0, 5)]);
        let mvar = Mvar::parse(&data).unwrap();
        assert!(mvar.lookup(b"cpht").is_none());
        assert!(mvar.lookup(b"GRAD").is_none());
    }

    #[test]
    fn lookup_binary_search_works_at_boundaries() {
        // Проверяем что bin-search корректно находит первую и последнюю
        // записи (типовые off-by-one ошибки).
        let data = build_mvar(&[
            (b"aaaa", 1, 1),
            (b"bbbb", 1, 2),
            (b"cccc", 1, 3),
            (b"dddd", 1, 4),
            (b"eeee", 1, 5),
        ]);
        let mvar = Mvar::parse(&data).unwrap();
        assert_eq!(mvar.lookup(b"aaaa").unwrap().delta_set_inner, 1);
        assert_eq!(mvar.lookup(b"eeee").unwrap().delta_set_inner, 5);
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_mvar(&[(b"xhgt", 0, 1)]);
        data[1] = 2; // major = 2
        assert!(Mvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_value_record_size_too_small() {
        let mut data = build_mvar(&[(b"xhgt", 0, 1)]);
        // value_record_size на offset 6..8; устанавливаем 4 (< 8).
        data[6] = 0;
        data[7] = 4;
        assert!(Mvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_store_offset_out_of_bounds() {
        let mut data = build_mvar(&[]);
        // store_offset on offset 10..12. Установим за пределы файла.
        data[10] = 0xFF;
        data[11] = 0xFF;
        assert!(Mvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let data = build_mvar(&[]);
        let truncated = &data[..8]; // header требует 12 байт
        assert!(Mvar::parse(truncated).is_err());
    }

    #[test]
    fn rejects_truncated_records() {
        let data = build_mvar(&[(b"xhgt", 0, 1), (b"cpht", 0, 2)]);
        // Обрезаем 4 байта внутри второй записи.
        let truncated = &data[..data.len() - 12 - 4]; // strip ivs + 4 bytes record
        assert!(Mvar::parse(truncated).is_err());
    }

    #[test]
    fn parses_multiple_records_preserves_order() {
        let data = build_mvar(&[
            (b"cpht", 1, 10),
            (b"strs", 1, 11),
            (b"undo", 1, 12),
            (b"xhgt", 1, 13),
        ]);
        let mvar = Mvar::parse(&data).unwrap();
        assert_eq!(mvar.records.len(), 4);
        assert_eq!(&mvar.records[0].tag, b"cpht");
        assert_eq!(&mvar.records[1].tag, b"strs");
        assert_eq!(&mvar.records[2].tag, b"undo");
        assert_eq!(&mvar.records[3].tag, b"xhgt");
    }

    #[test]
    fn handles_zero_store_offset() {
        // Spec позволяет MVAR без variation store (только records без
        // вариаций — необычно, но валидно). Тестируем graceful handling.
        let mut data = build_mvar(&[(b"xhgt", 0, 0)]);
        // store_offset байты 10..12. Обнуляем.
        data[10] = 0;
        data[11] = 0;
        let mvar = Mvar::parse(&data).unwrap();
        assert!(mvar.store.is_empty());
        assert_eq!(mvar.records.len(), 1);
    }

    #[test]
    fn handles_larger_value_record_size() {
        // Spec позволяет valueRecordSize > 8 для расширений (reserved
        // padding). Парсер должен skip-нуть лишние байты.
        let header_size = 12u16;
        let value_record_size = 10u16; // 8 + 2 reserved
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&value_record_size.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // 1 record
        let store_offset = header_size + value_record_size;
        out.extend_from_slice(&store_offset.to_be_bytes());
        // Record (10 bytes):
        out.extend_from_slice(b"xhgt"); // tag
        out.extend_from_slice(&7u16.to_be_bytes()); // outer
        out.extend_from_slice(&9u16.to_be_bytes()); // inner
        out.extend_from_slice(&[0xDE, 0xAD]); // reserved padding
        out.extend_from_slice(&build_minimal_ivs());

        let mvar = Mvar::parse(&out).unwrap();
        assert_eq!(mvar.records.len(), 1);
        assert_eq!(mvar.records[0].delta_set_outer, 7);
        assert_eq!(mvar.records[0].delta_set_inner, 9);
    }
}
