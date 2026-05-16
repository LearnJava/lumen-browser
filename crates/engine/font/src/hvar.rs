//! `HVAR` — Horizontal Metrics Variations Table. Описывает per-glyph
//! variation deltas для горизонтальных метрик: advance width, left
//! sidebearing (LSB), right sidebearing (RSB). При активном
//! variation-instance шрифта runtime берёт base-метрики из `hmtx`,
//! ищет (outer, inner)-индекс через соответствующий `DeltaSetIndexMap`
//! (или identity fallback), вычисляет delta через
//! `ItemVariationStore::evaluate(coords)` и прибавляет к base.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/hvar>.
//!
//! Phase 0 ограничения:
//! - Только v1.0. Будущие версии (если появятся) — отдельная задача.
//! - Парсер; `evaluate(coords)` для tent-функции на регионах добавится
//!   вместе с реальным consumer-ом в rasterizer-е (нужен текущий axis-
//!   instance, который сейчас не существует — нет CSS `font-variation-
//!   settings` cascade).

use crate::binary::BinaryReader;
use crate::delta_set_index_map::{DeltaSetIndex, DeltaSetIndexMap};
use crate::face::FontError;
use crate::item_variation::ItemVariationStore;

const HVAR: [u8; 4] = *b"HVAR";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hvar {
    pub store: ItemVariationStore,
    /// Маппер glyph_id → (outer, inner) для advance width variations.
    /// `None` означает identity (per spec: outer=0, inner=glyph_id).
    pub advance_width_map: Option<DeltaSetIndexMap>,
    /// LSB (left sidebearing) variations. `None` — нет вариаций LSB.
    pub lsb_map: Option<DeltaSetIndexMap>,
    /// RSB (right sidebearing) variations.
    pub rsb_map: Option<DeltaSetIndexMap>,
}

impl Hvar {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(HVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(HVAR))?;
        if major != 1 {
            return Err(FontError::InvalidTable(HVAR));
        }
        let store_offset = r.read_u32().ok_or(FontError::InvalidTable(HVAR))? as usize;
        let aw_map_offset = r.read_u32().ok_or(FontError::InvalidTable(HVAR))? as usize;
        let lsb_map_offset = r.read_u32().ok_or(FontError::InvalidTable(HVAR))? as usize;
        let rsb_map_offset = r.read_u32().ok_or(FontError::InvalidTable(HVAR))? as usize;

        // store_offset обязателен по spec.
        if store_offset == 0 || store_offset >= data.len() {
            return Err(FontError::InvalidTable(HVAR));
        }
        let store = ItemVariationStore::parse(&data[store_offset..])?;

        // Опциональные maps (offset == 0 ⇒ map отсутствует).
        let advance_width_map = parse_optional_map(data, aw_map_offset)?;
        let lsb_map = parse_optional_map(data, lsb_map_offset)?;
        let rsb_map = parse_optional_map(data, rsb_map_offset)?;

        Ok(Self {
            store,
            advance_width_map,
            lsb_map,
            rsb_map,
        })
    }

    /// `(outer, inner)`-индекс для advance width variations glyph_id.
    /// Если `advance_width_map` отсутствует — identity-fallback per spec
    /// (`outer=0, inner=glyph_id`).
    pub fn advance_width_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.advance_width_map.as_ref(), glyph_id)
    }

    /// Аналогично для LSB. `None`-map → identity-fallback. Caller обычно
    /// проверяет `has_lsb_variations()` если нужно различать «нет
    /// вариаций» от «identity-fallback» (для performance shortcut).
    pub fn lsb_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.lsb_map.as_ref(), glyph_id)
    }

    pub fn rsb_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.rsb_map.as_ref(), glyph_id)
    }

    /// `true`, если HVAR содержит хоть один map для LSB (т.е. шрифт
    /// планирует варьировать sidebearings, а не только advance).
    pub fn has_lsb_variations(&self) -> bool {
        self.lsb_map.is_some()
    }

    pub fn has_rsb_variations(&self) -> bool {
        self.rsb_map.is_some()
    }
}

fn parse_optional_map(data: &[u8], offset: usize) -> Result<Option<DeltaSetIndexMap>, FontError> {
    if offset == 0 {
        return Ok(None);
    }
    if offset >= data.len() {
        return Err(FontError::InvalidTable(HVAR));
    }
    Ok(Some(DeltaSetIndexMap::parse(&data[offset..])?))
}

/// Identity fallback per OpenType HVAR spec: «If no advance width
/// variations map is defined, then the glyph_id is used as the inner
/// index, with outer index 0».
fn index_or_identity(map: Option<&DeltaSetIndexMap>, glyph_id: u16) -> DeltaSetIndex {
    match map {
        Some(m) => m.get(u32::from(glyph_id)),
        None => DeltaSetIndex {
            outer: 0,
            inner: glyph_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимальный синтетический ItemVariationStore: format=1,
    /// 0 регионов, 0 data blocks. Возвращает байты + длину.
    fn build_minimal_ivs() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // format
        let region_list_offset: u32 = 8;
        out.extend_from_slice(&region_list_offset.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // itemVariationDataCount
        // VariationRegionList @ offset 8: axisCount=0, regionCount=0.
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out
    }

    /// Минимальный DeltaSetIndexMap format 0 с указанным набором пар.
    fn build_dsim(pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(0u8); // format 0
        // entry_format: inner=8 bits, entry_size=2.
        out.push(((8 - 1) & 0x0F) | (((2 - 1) & 0x03) << 4));
        out.extend_from_slice(&(pairs.len() as u16).to_be_bytes());
        for &(outer, inner) in pairs {
            let raw = (u32::from(outer) << 8) | u32::from(inner);
            out.extend_from_slice(&(raw as u16).to_be_bytes());
        }
        out
    }

    /// Строит синтетический HVAR с указанными map-offsets (0 = no map).
    /// Возвращает байты, в которые включены IVS и встроенные maps.
    fn build_hvar(aw_map: Option<&[(u16, u16)]>, lsb_map: Option<&[(u16, u16)]>) -> Vec<u8> {
        let header_size = 20u32;
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // major
        out.extend_from_slice(&0u16.to_be_bytes()); // minor

        let ivs = build_minimal_ivs();
        let store_offset = header_size;
        out.extend_from_slice(&store_offset.to_be_bytes());

        let aw_bytes = aw_map.map(build_dsim);
        let lsb_bytes = lsb_map.map(build_dsim);

        let mut next_offset = header_size + ivs.len() as u32;
        let aw_offset = if let Some(b) = &aw_bytes {
            let o = next_offset;
            next_offset += b.len() as u32;
            o
        } else {
            0
        };
        let lsb_offset = if let Some(b) = &lsb_bytes {
            let o = next_offset;
            next_offset += b.len() as u32;
            o
        } else {
            0
        };
        let rsb_offset = 0u32; // не тестируем RSB здесь
        let _ = next_offset;

        out.extend_from_slice(&aw_offset.to_be_bytes());
        out.extend_from_slice(&lsb_offset.to_be_bytes());
        out.extend_from_slice(&rsb_offset.to_be_bytes());

        out.extend_from_slice(&ivs);
        if let Some(b) = aw_bytes {
            out.extend_from_slice(&b);
        }
        if let Some(b) = lsb_bytes {
            out.extend_from_slice(&b);
        }
        out
    }

    #[test]
    fn parses_minimal_hvar_no_maps() {
        let data = build_hvar(None, None);
        let hvar = Hvar::parse(&data).unwrap();
        assert!(hvar.store.is_empty());
        assert!(hvar.advance_width_map.is_none());
        assert!(hvar.lsb_map.is_none());
        assert!(hvar.rsb_map.is_none());
        assert!(!hvar.has_lsb_variations());
        assert!(!hvar.has_rsb_variations());
    }

    #[test]
    fn parses_hvar_with_advance_width_map() {
        let pairs = [(0, 0), (0, 1), (0, 2)];
        let data = build_hvar(Some(&pairs), None);
        let hvar = Hvar::parse(&data).unwrap();
        let m = hvar.advance_width_map.as_ref().expect("aw map present");
        assert_eq!(m.entries.len(), 3);
        assert!(hvar.lsb_map.is_none());
    }

    #[test]
    fn advance_width_index_uses_map_when_present() {
        let pairs = [(5, 100), (5, 101), (5, 102)];
        let data = build_hvar(Some(&pairs), None);
        let hvar = Hvar::parse(&data).unwrap();
        assert_eq!(
            hvar.advance_width_index(0),
            DeltaSetIndex { outer: 5, inner: 100 }
        );
        assert_eq!(
            hvar.advance_width_index(1),
            DeltaSetIndex { outer: 5, inner: 101 }
        );
        assert_eq!(
            hvar.advance_width_index(2),
            DeltaSetIndex { outer: 5, inner: 102 }
        );
    }

    #[test]
    fn advance_width_index_falls_back_to_identity_when_no_map() {
        let data = build_hvar(None, None);
        let hvar = Hvar::parse(&data).unwrap();
        // Per spec: identity outer=0, inner=glyph_id.
        assert_eq!(
            hvar.advance_width_index(42),
            DeltaSetIndex { outer: 0, inner: 42 }
        );
        assert_eq!(
            hvar.advance_width_index(65535),
            DeltaSetIndex {
                outer: 0,
                inner: 65535
            }
        );
    }

    #[test]
    fn advance_width_index_clamps_out_of_range_via_map() {
        // Map с 2 entries; для glyph_id ≥ 2 — последняя entry per spec.
        let pairs = [(1, 10), (1, 20)];
        let data = build_hvar(Some(&pairs), None);
        let hvar = Hvar::parse(&data).unwrap();
        assert_eq!(
            hvar.advance_width_index(100),
            DeltaSetIndex { outer: 1, inner: 20 }
        );
    }

    #[test]
    fn lsb_variations_recognised() {
        let aw = [(0, 0), (0, 1)];
        let lsb = [(1, 0), (1, 1)];
        let data = build_hvar(Some(&aw), Some(&lsb));
        let hvar = Hvar::parse(&data).unwrap();
        assert!(hvar.has_lsb_variations());
        assert!(!hvar.has_rsb_variations());
        assert_eq!(
            hvar.lsb_index(0),
            DeltaSetIndex { outer: 1, inner: 0 }
        );
        assert_eq!(
            hvar.lsb_index(1),
            DeltaSetIndex { outer: 1, inner: 1 }
        );
    }

    #[test]
    fn lsb_index_falls_back_to_identity_when_no_map() {
        let data = build_hvar(None, None);
        let hvar = Hvar::parse(&data).unwrap();
        assert_eq!(
            hvar.lsb_index(7),
            DeltaSetIndex { outer: 0, inner: 7 }
        );
    }

    #[test]
    fn rsb_index_falls_back_to_identity_when_no_map() {
        let data = build_hvar(None, None);
        let hvar = Hvar::parse(&data).unwrap();
        assert_eq!(
            hvar.rsb_index(123),
            DeltaSetIndex {
                outer: 0,
                inner: 123
            }
        );
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_hvar(None, None);
        data[1] = 2; // major = 2
        assert!(Hvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_missing_store_offset() {
        let mut data = build_hvar(None, None);
        // store_offset on bytes 4..8. Zero it out.
        data[4] = 0;
        data[5] = 0;
        data[6] = 0;
        data[7] = 0;
        assert!(Hvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_store_offset_out_of_bounds() {
        let mut data = build_hvar(None, None);
        // store_offset гигантский.
        data[4] = 0xFF;
        data[5] = 0xFF;
        data[6] = 0xFF;
        data[7] = 0xFF;
        assert!(Hvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let data = build_hvar(None, None);
        let truncated = &data[..10]; // header требует 20 байт
        assert!(Hvar::parse(truncated).is_err());
    }
}
