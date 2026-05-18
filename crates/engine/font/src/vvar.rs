//! `VVAR` — Vertical Metrics Variations Table. Зеркало `HVAR` для
//! вертикальных метрик: advance height, top sidebearing (TSB), bottom
//! sidebearing (BSB), Y-координата vertical origin (vOrg). При активном
//! variation-instance шрифта runtime берёт base-метрики из `vmtx` (и
//! `VORG` для vOrg), ищет (outer, inner)-индекс через соответствующий
//! `DeltaSetIndexMap` (или identity fallback для advance height),
//! вычисляет delta через `ItemVariationStore::evaluate(coords)` и
//! прибавляет к base.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/vvar>.
//!
//! Phase 0 ограничения:
//! - Только v1.0. Будущие версии (если появятся) — отдельная задача.
//! - Парсер; `evaluate(coords)` для tent-функции на регионах добавится
//!   вместе с реальным consumer-ом в rasterizer-е (нужен текущий axis-
//!   instance, который сейчас не существует — нет CSS `font-variation-
//!   settings` cascade).
//! - Identity fallback per spec — только для advance height. Для TSB /
//!   BSB / vOrg отсутствующая map = «нет вариаций» (`has_*_variations()
//!   ` вернёт `false`); `*_index` всё равно возвращает identity для
//!   симметрии с HVAR (caller проверяет `has_*_variations()`).

use crate::binary::BinaryReader;
use crate::delta_set_index_map::{DeltaSetIndex, DeltaSetIndexMap};
use crate::face::FontError;
use crate::item_variation::ItemVariationStore;

const VVAR: [u8; 4] = *b"VVAR";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Vvar {
    pub store: ItemVariationStore,
    /// Маппер glyph_id → (outer, inner) для advance height variations.
    /// `None` означает identity (per spec: outer=0, inner=glyph_id).
    pub advance_height_map: Option<DeltaSetIndexMap>,
    /// TSB (top sidebearing) variations. `None` — нет вариаций TSB.
    pub tsb_map: Option<DeltaSetIndexMap>,
    /// BSB (bottom sidebearing) variations. `None` — нет вариаций BSB.
    pub bsb_map: Option<DeltaSetIndexMap>,
    /// vOrg (vertical origin Y) variations. `None` — нет вариаций vOrg.
    pub v_org_map: Option<DeltaSetIndexMap>,
}

impl Vvar {
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let major = r.read_u16().ok_or(FontError::InvalidTable(VVAR))?;
        let _minor = r.read_u16().ok_or(FontError::InvalidTable(VVAR))?;
        if major != 1 {
            return Err(FontError::InvalidTable(VVAR));
        }
        let store_offset = r.read_u32().ok_or(FontError::InvalidTable(VVAR))? as usize;
        let ah_map_offset = r.read_u32().ok_or(FontError::InvalidTable(VVAR))? as usize;
        let tsb_map_offset = r.read_u32().ok_or(FontError::InvalidTable(VVAR))? as usize;
        let bsb_map_offset = r.read_u32().ok_or(FontError::InvalidTable(VVAR))? as usize;
        let v_org_map_offset = r.read_u32().ok_or(FontError::InvalidTable(VVAR))? as usize;

        if store_offset == 0 || store_offset >= data.len() {
            return Err(FontError::InvalidTable(VVAR));
        }
        let store = ItemVariationStore::parse(&data[store_offset..])?;

        let advance_height_map = parse_optional_map(data, ah_map_offset)?;
        let tsb_map = parse_optional_map(data, tsb_map_offset)?;
        let bsb_map = parse_optional_map(data, bsb_map_offset)?;
        let v_org_map = parse_optional_map(data, v_org_map_offset)?;

        Ok(Self {
            store,
            advance_height_map,
            tsb_map,
            bsb_map,
            v_org_map,
        })
    }

    /// `(outer, inner)`-индекс для advance height variations glyph_id.
    /// Если `advance_height_map` отсутствует — identity-fallback per spec
    /// (`outer=0, inner=glyph_id`).
    pub fn advance_height_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.advance_height_map.as_ref(), glyph_id)
    }

    /// Аналогично для TSB. `None`-map → identity-fallback. Caller обычно
    /// проверяет `has_tsb_variations()` если нужно различать «нет
    /// вариаций» от «identity-fallback» (для performance shortcut).
    pub fn tsb_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.tsb_map.as_ref(), glyph_id)
    }

    pub fn bsb_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.bsb_map.as_ref(), glyph_id)
    }

    pub fn v_org_index(&self, glyph_id: u16) -> DeltaSetIndex {
        index_or_identity(self.v_org_map.as_ref(), glyph_id)
    }

    pub fn has_tsb_variations(&self) -> bool {
        self.tsb_map.is_some()
    }

    pub fn has_bsb_variations(&self) -> bool {
        self.bsb_map.is_some()
    }

    pub fn has_v_org_variations(&self) -> bool {
        self.v_org_map.is_some()
    }
}

fn parse_optional_map(data: &[u8], offset: usize) -> Result<Option<DeltaSetIndexMap>, FontError> {
    if offset == 0 {
        return Ok(None);
    }
    if offset >= data.len() {
        return Err(FontError::InvalidTable(VVAR));
    }
    Ok(Some(DeltaSetIndexMap::parse(&data[offset..])?))
}

/// Identity fallback per OpenType VVAR spec: «If no advance height
/// mapping subtable is provided, then a default mapping is used: glyph
/// indices are used directly as implicit delta-set indices, with outer
/// index of zero and inner indices that match the glyph indices». Для
/// TSB/BSB/vOrg формально fallback нет (no map = no variations), но
/// возвращаем identity для симметрии с HVAR — caller различает по
/// `has_*_variations()`.
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
    /// 0 регионов, 0 data blocks.
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

    /// Строит синтетический VVAR с указанными map-offsets (`None` = no map).
    /// Возвращает байты, в которые включены IVS и встроенные maps в порядке
    /// `ah, tsb, bsb, v_org`.
    fn build_vvar(
        ah_map: Option<&[(u16, u16)]>,
        tsb_map: Option<&[(u16, u16)]>,
        bsb_map: Option<&[(u16, u16)]>,
        v_org_map: Option<&[(u16, u16)]>,
    ) -> Vec<u8> {
        let header_size = 24u32; // 2*u16 + 5*u32
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes()); // major
        out.extend_from_slice(&0u16.to_be_bytes()); // minor

        let ivs = build_minimal_ivs();
        let store_offset = header_size;
        out.extend_from_slice(&store_offset.to_be_bytes());

        let ah_bytes = ah_map.map(build_dsim);
        let tsb_bytes = tsb_map.map(build_dsim);
        let bsb_bytes = bsb_map.map(build_dsim);
        let v_org_bytes = v_org_map.map(build_dsim);

        let mut next_offset = header_size + ivs.len() as u32;
        let mut alloc = |bytes: &Option<Vec<u8>>| -> u32 {
            if let Some(b) = bytes {
                let o = next_offset;
                next_offset += b.len() as u32;
                o
            } else {
                0
            }
        };
        let ah_offset = alloc(&ah_bytes);
        let tsb_offset = alloc(&tsb_bytes);
        let bsb_offset = alloc(&bsb_bytes);
        let v_org_offset = alloc(&v_org_bytes);
        let _ = next_offset;

        out.extend_from_slice(&ah_offset.to_be_bytes());
        out.extend_from_slice(&tsb_offset.to_be_bytes());
        out.extend_from_slice(&bsb_offset.to_be_bytes());
        out.extend_from_slice(&v_org_offset.to_be_bytes());

        out.extend_from_slice(&ivs);
        for b in [&ah_bytes, &tsb_bytes, &bsb_bytes, &v_org_bytes]
            .into_iter()
            .flatten()
        {
            out.extend_from_slice(b);
        }
        out
    }

    #[test]
    fn parses_minimal_vvar_no_maps() {
        let data = build_vvar(None, None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert!(vvar.store.is_empty());
        assert!(vvar.advance_height_map.is_none());
        assert!(vvar.tsb_map.is_none());
        assert!(vvar.bsb_map.is_none());
        assert!(vvar.v_org_map.is_none());
        assert!(!vvar.has_tsb_variations());
        assert!(!vvar.has_bsb_variations());
        assert!(!vvar.has_v_org_variations());
    }

    #[test]
    fn parses_vvar_with_advance_height_map() {
        let pairs = [(0, 0), (0, 1), (0, 2)];
        let data = build_vvar(Some(&pairs), None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        let m = vvar.advance_height_map.as_ref().expect("ah map present");
        assert_eq!(m.entries.len(), 3);
        assert!(vvar.tsb_map.is_none());
    }

    #[test]
    fn advance_height_index_uses_map_when_present() {
        let pairs = [(5, 100), (5, 101), (5, 102)];
        let data = build_vvar(Some(&pairs), None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert_eq!(
            vvar.advance_height_index(0),
            DeltaSetIndex { outer: 5, inner: 100 }
        );
        assert_eq!(
            vvar.advance_height_index(1),
            DeltaSetIndex { outer: 5, inner: 101 }
        );
        assert_eq!(
            vvar.advance_height_index(2),
            DeltaSetIndex { outer: 5, inner: 102 }
        );
    }

    #[test]
    fn advance_height_index_falls_back_to_identity_when_no_map() {
        let data = build_vvar(None, None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        // Per spec: identity outer=0, inner=glyph_id.
        assert_eq!(
            vvar.advance_height_index(42),
            DeltaSetIndex { outer: 0, inner: 42 }
        );
        assert_eq!(
            vvar.advance_height_index(65535),
            DeltaSetIndex {
                outer: 0,
                inner: 65535,
            }
        );
    }

    #[test]
    fn advance_height_index_clamps_out_of_range_via_map() {
        // Map с 2 entries; для glyph_id ≥ 2 — последняя entry per spec.
        let pairs = [(1, 10), (1, 20)];
        let data = build_vvar(Some(&pairs), None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert_eq!(
            vvar.advance_height_index(100),
            DeltaSetIndex { outer: 1, inner: 20 }
        );
    }

    #[test]
    fn tsb_variations_recognised() {
        let ah = [(0, 0), (0, 1)];
        let tsb = [(2, 0), (2, 1)];
        let data = build_vvar(Some(&ah), Some(&tsb), None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert!(vvar.has_tsb_variations());
        assert!(!vvar.has_bsb_variations());
        assert!(!vvar.has_v_org_variations());
        assert_eq!(
            vvar.tsb_index(0),
            DeltaSetIndex { outer: 2, inner: 0 }
        );
        assert_eq!(
            vvar.tsb_index(1),
            DeltaSetIndex { outer: 2, inner: 1 }
        );
    }

    #[test]
    fn bsb_variations_recognised() {
        let bsb = [(3, 7), (3, 8)];
        let data = build_vvar(None, None, Some(&bsb), None);
        let vvar = Vvar::parse(&data).unwrap();
        assert!(!vvar.has_tsb_variations());
        assert!(vvar.has_bsb_variations());
        assert_eq!(
            vvar.bsb_index(0),
            DeltaSetIndex { outer: 3, inner: 7 }
        );
        assert_eq!(
            vvar.bsb_index(1),
            DeltaSetIndex { outer: 3, inner: 8 }
        );
    }

    #[test]
    fn v_org_variations_recognised() {
        let vorg = [(4, 12), (4, 34)];
        let data = build_vvar(None, None, None, Some(&vorg));
        let vvar = Vvar::parse(&data).unwrap();
        assert!(vvar.has_v_org_variations());
        assert_eq!(
            vvar.v_org_index(0),
            DeltaSetIndex { outer: 4, inner: 12 }
        );
        assert_eq!(
            vvar.v_org_index(1),
            DeltaSetIndex { outer: 4, inner: 34 }
        );
    }

    #[test]
    fn all_four_maps_present_simultaneously() {
        let ah = [(0, 0)];
        let tsb = [(1, 0)];
        let bsb = [(2, 0)];
        let vorg = [(3, 0)];
        let data = build_vvar(Some(&ah), Some(&tsb), Some(&bsb), Some(&vorg));
        let vvar = Vvar::parse(&data).unwrap();
        assert!(vvar.advance_height_map.is_some());
        assert!(vvar.has_tsb_variations());
        assert!(vvar.has_bsb_variations());
        assert!(vvar.has_v_org_variations());
        assert_eq!(
            vvar.advance_height_index(0),
            DeltaSetIndex { outer: 0, inner: 0 }
        );
        assert_eq!(
            vvar.tsb_index(0),
            DeltaSetIndex { outer: 1, inner: 0 }
        );
        assert_eq!(
            vvar.bsb_index(0),
            DeltaSetIndex { outer: 2, inner: 0 }
        );
        assert_eq!(
            vvar.v_org_index(0),
            DeltaSetIndex { outer: 3, inner: 0 }
        );
    }

    #[test]
    fn tsb_index_falls_back_to_identity_when_no_map() {
        let data = build_vvar(None, None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert_eq!(
            vvar.tsb_index(7),
            DeltaSetIndex { outer: 0, inner: 7 }
        );
    }

    #[test]
    fn bsb_index_falls_back_to_identity_when_no_map() {
        let data = build_vvar(None, None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert_eq!(
            vvar.bsb_index(123),
            DeltaSetIndex {
                outer: 0,
                inner: 123,
            }
        );
    }

    #[test]
    fn v_org_index_falls_back_to_identity_when_no_map() {
        let data = build_vvar(None, None, None, None);
        let vvar = Vvar::parse(&data).unwrap();
        assert_eq!(
            vvar.v_org_index(99),
            DeltaSetIndex { outer: 0, inner: 99 }
        );
    }

    #[test]
    fn rejects_unsupported_major_version() {
        let mut data = build_vvar(None, None, None, None);
        data[1] = 2; // major = 2
        assert!(Vvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_missing_store_offset() {
        let mut data = build_vvar(None, None, None, None);
        // store_offset on bytes 4..8. Zero it out.
        data[4] = 0;
        data[5] = 0;
        data[6] = 0;
        data[7] = 0;
        assert!(Vvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_store_offset_out_of_bounds() {
        let mut data = build_vvar(None, None, None, None);
        data[4] = 0xFF;
        data[5] = 0xFF;
        data[6] = 0xFF;
        data[7] = 0xFF;
        assert!(Vvar::parse(&data).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let data = build_vvar(None, None, None, None);
        let truncated = &data[..16]; // header требует 24 байта
        assert!(Vvar::parse(truncated).is_err());
    }
}
