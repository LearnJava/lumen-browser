//! `GSUB` — Glyph Substitution table.
//!
//! Stage-1 scope (Latin/Cyrillic): Lookup Type 1 (Single substitution) and
//! Lookup Type 4 (Ligature substitution), plus Type 7 (Extension) which
//! merely wraps another lookup. These cover the common `liga`/`clig`
//! ligatures (fi, fl, ffi, …) and one-to-one localized/stylistic forms.
//!
//! Lookup Types 2/3/5/6/8 (multiple, alternate, contextual, chained,
//! reverse-chained) are recognised but skipped — they drive complex-script
//! and contextual features outside the stage-1 goal.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/gsub>.

use crate::otlayout::{Coverage, LayoutTable, resolve_extension};
use crate::shape::ShapedGlyph;

/// Feature tags enabled by default for substitution: standard ligatures
/// (`liga`), contextual ligatures (`clig`), contextual alternates (`calt`),
/// required ligatures (`rlig`) and glyph composition (`ccmp`) — matching a
/// browser's `font-variant-ligatures: normal` plus always-on features.
///
/// `calt` is included because some fonts (e.g. the bundled Inter) ship their
/// common f-ligatures as `calt` type-4 ligature lookups rather than `liga`.
/// Discretionary/historical ligatures (`dlig`/`hlig`) stay off by default.
pub const GSUB_FEATURES: [[u8; 4]; 5] =
    [*b"liga", *b"clig", *b"calt", *b"rlig", *b"ccmp"];

#[inline]
fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(pos..pos + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

#[inline]
fn i16_at(data: &[u8], pos: usize) -> Option<i16> {
    let bytes: [u8; 2] = data.get(pos..pos + 2)?.try_into().ok()?;
    Some(i16::from_be_bytes(bytes))
}

/// Parsed `GSUB` table plus the lookup indices activated by the enabled
/// substitution features, in application order.
#[derive(Debug, Clone)]
pub struct Gsub<'a> {
    table: LayoutTable<'a>,
    lookups: Vec<u16>,
}

impl<'a> Gsub<'a> {
    /// Parse the `GSUB` table bytes and pre-select the lookups for the
    /// default substitution features. Returns `None` if the table is
    /// malformed; an empty lookup set is valid (no substitutions apply).
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let table = LayoutTable::parse(data)?;
        let lookups = table.enabled_lookups(&GSUB_FEATURES);
        Some(Self { table, lookups })
    }

    /// Whether any substitution lookups are active.
    pub fn has_lookups(&self) -> bool {
        !self.lookups.is_empty()
    }

    /// Apply all enabled substitution lookups to `glyphs` in order.
    pub fn apply(&self, glyphs: &mut Vec<ShapedGlyph>) {
        for &li in &self.lookups {
            let Some(lookup) = self.table.lookup(li) else {
                continue;
            };
            self.apply_lookup(lookup.lookup_type, &lookup.subtables, glyphs);
        }
    }

    /// Apply one lookup (all its subtables) across the whole buffer.
    fn apply_lookup(&self, lookup_type: u16, subtables: &[usize], glyphs: &mut Vec<ShapedGlyph>) {
        for &sub in subtables {
            let (real_type, real_off) = if lookup_type == 7 {
                match resolve_extension(self.table.data, sub) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                (lookup_type, sub)
            };
            match real_type {
                1 => self.apply_single(real_off, glyphs),
                4 => self.apply_ligature(real_off, glyphs),
                _ => {}
            }
        }
    }

    /// GSUB Lookup Type 1 — Single Substitution (formats 1 and 2).
    /// Replaces each covered glyph one-for-one in place.
    fn apply_single(&self, off: usize, glyphs: &mut [ShapedGlyph]) {
        let data = self.table.data;
        let Some(format) = u16_at(data, off) else {
            return;
        };
        let Some(cov_off) = u16_at(data, off + 2) else {
            return;
        };
        let Some(cov) = Coverage::parse(data, off + cov_off as usize) else {
            return;
        };
        for g in glyphs.iter_mut() {
            let Some(idx) = cov.index_of(g.glyph_id) else {
                continue;
            };
            match format {
                1 => {
                    if let Some(delta) = i16_at(data, off + 4) {
                        g.glyph_id = (g.glyph_id as i32 + delta as i32) as u16;
                    }
                }
                2 => {
                    // glyphCount at off+4, substituteGlyphIDs[] at off+6.
                    if let Some(sub) = u16_at(data, off + 6 + idx as usize * 2) {
                        g.glyph_id = sub;
                    }
                }
                _ => {}
            }
        }
    }

    /// GSUB Lookup Type 4 — Ligature Substitution (format 1). Greedily
    /// replaces a run of glyphs matching a ligature's components with the
    /// single ligature glyph; the merged glyph inherits the smallest source
    /// cluster of its components.
    fn apply_ligature(&self, off: usize, glyphs: &mut Vec<ShapedGlyph>) {
        let data = self.table.data;
        if u16_at(data, off) != Some(1) {
            return;
        }
        let Some(cov_off) = u16_at(data, off + 2) else {
            return;
        };
        let Some(cov) = Coverage::parse(data, off + cov_off as usize) else {
            return;
        };
        let Some(set_count) = u16_at(data, off + 4) else {
            return;
        };

        let mut i = 0;
        while i < glyphs.len() {
            let first = glyphs[i].glyph_id;
            if let Some(cov_idx) = cov.index_of(first)
                && cov_idx < set_count
                && let Some((lig_glyph, comp_count)) =
                    self.match_ligature_set(off, cov_idx, glyphs, i)
            {
                let cluster = glyphs[i..i + comp_count]
                    .iter()
                    .map(|g| g.cluster)
                    .min()
                    .unwrap_or(glyphs[i].cluster);
                glyphs[i] = ShapedGlyph {
                    glyph_id: lig_glyph,
                    cluster,
                    x_advance: 0,
                    x_offset: 0,
                    y_offset: 0,
                };
                glyphs.drain(i + 1..i + comp_count);
            }
            i += 1;
        }
    }

    /// Try every ligature in the LigatureSet at `cov_idx`, returning the
    /// `(ligatureGlyph, componentCount)` of the first whose trailing
    /// components match the glyphs starting at `start`.
    fn match_ligature_set(
        &self,
        sub_off: usize,
        cov_idx: u16,
        glyphs: &[ShapedGlyph],
        start: usize,
    ) -> Option<(u16, usize)> {
        let data = self.table.data;
        // ligatureSetOffsets[] start at sub_off + 6, relative to sub_off.
        let set_off = sub_off + u16_at(data, sub_off + 6 + cov_idx as usize * 2)? as usize;
        let lig_count = u16_at(data, set_off)?;
        for li in 0..lig_count as usize {
            let lig_off = set_off + u16_at(data, set_off + 2 + li * 2)? as usize;
            let lig_glyph = u16_at(data, lig_off)?;
            let comp_count = u16_at(data, lig_off + 2)? as usize;
            if comp_count == 0 || start + comp_count > glyphs.len() {
                continue;
            }
            // componentGlyphIDs[] holds comp_count-1 entries (first is the
            // coverage glyph), at lig_off + 4.
            let mut matched = true;
            for c in 1..comp_count {
                let want = match u16_at(data, lig_off + 4 + (c - 1) * 2) {
                    Some(v) => v,
                    None => {
                        matched = false;
                        break;
                    }
                };
                if glyphs[start + c].glyph_id != want {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Some((lig_glyph, comp_count));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(ids: &[u16]) -> Vec<ShapedGlyph> {
        ids.iter()
            .enumerate()
            .map(|(i, &g)| ShapedGlyph {
                glyph_id: g,
                cluster: i as u32,
                x_advance: 0,
                x_offset: 0,
                y_offset: 0,
            })
            .collect()
    }

    /// Hand-built GSUB with one Type-4 ligature subtable: glyphs (10, 11) ->
    /// 99, reached through a single LookupList lookup wired into a `liga`
    /// feature under the `DFLT` script. Layout is laid out so every offset is
    /// computed explicitly.
    fn synthetic_gsub_ligature() -> Vec<u8> {
        // Sequential, non-overlapping layout. Offsets in the header are
        // absolute (from table start); offsets inside each structure are
        // relative to that structure's start.
        let mut d: Vec<u8> = Vec::new();
        // Header @0..10: version 1.0, scriptList@10, featureList@30,
        // lookupList@44.
        d.extend([0, 1, 0, 0]); // version 1.0
        d.extend([0, 10]); // scriptListOffset
        d.extend([0, 30]); // featureListOffset
        d.extend([0, 44]); // lookupListOffset

        // ScriptList @10..18: count=1, record(DFLT, scriptOffset=8 -> @18)
        d.extend([0, 1]); // scriptCount
        d.extend(*b"DFLT");
        d.extend([0, 8]); // scriptOffset (relative to @10) -> @18
        // Script @18..22: defaultLangSysOffset=4 (-> @22), langSysCount=0
        assert_eq!(d.len(), 18);
        d.extend([0, 4]); // defaultLangSysOffset (relative to @18) -> @22
        d.extend([0, 0]); // langSysCount
        // LangSys @22..30: lookupOrder, requiredFeatureIndex=NONE,
        // featureIndexCount=1, featureIndices=[0]
        assert_eq!(d.len(), 22);
        d.extend([0, 0]); // lookupOrder
        d.extend([0xFF, 0xFF]); // requiredFeatureIndex
        d.extend([0, 1]); // featureIndexCount
        d.extend([0, 0]); // featureIndices[0]

        // FeatureList @30..38: count=1, record(liga, featureOffset=8 -> @38)
        assert_eq!(d.len(), 30);
        d.extend([0, 1]); // featureCount
        d.extend(*b"liga");
        d.extend([0, 8]); // featureOffset (relative to @30) -> @38
        // Feature @38..44: featureParams=0, lookupIndexCount=1, indices=[0]
        assert_eq!(d.len(), 38);
        d.extend([0, 0]); // featureParams
        d.extend([0, 1]); // lookupIndexCount
        d.extend([0, 0]); // lookupListIndices[0]

        // LookupList @44..48: count=1, lookupOffset=4 (-> @48)
        assert_eq!(d.len(), 44);
        d.extend([0, 1]); // lookupCount
        d.extend([0, 4]); // lookupOffsets[0] (relative to @44) -> @48
        // Lookup @48..56: type=4, flag=0, subTableCount=1, subOffset=8 -> @56
        assert_eq!(d.len(), 48);
        d.extend([0, 4]); // lookupType
        d.extend([0, 0]); // lookupFlag
        d.extend([0, 1]); // subTableCount
        d.extend([0, 8]); // subtableOffsets[0] (relative to @48) -> @56

        // LigatureSubst subtable @56..64.
        assert_eq!(d.len(), 56);
        d.extend([0, 1]); // substFormat
        d.extend([0, 8]); // coverageOffset (relative to @56) -> @64
        d.extend([0, 1]); // ligatureSetCount
        d.extend([0, 14]); // ligatureSetOffsets[0] (relative to @56) -> @70
        // Coverage @64..70: format=1, glyphCount=1, glyphArray=[10]
        assert_eq!(d.len(), 64);
        d.extend([0, 1]); // coverageFormat
        d.extend([0, 1]); // glyphCount
        d.extend([0, 10]); // glyph 10
        // LigatureSet @70..74: ligatureCount=1, ligatureOffsets[0]=4 (-> @74)
        assert_eq!(d.len(), 70);
        d.extend([0, 1]); // ligatureCount
        d.extend([0, 4]); // ligatureOffsets[0] (relative to @70) -> @74
        // Ligature @74..80: ligatureGlyph=99, componentCount=2, comps=[11]
        assert_eq!(d.len(), 74);
        d.extend([0, 99]); // ligatureGlyph
        d.extend([0, 2]); // componentCount
        d.extend([0, 11]); // componentGlyphIDs[0]
        d
    }

    #[test]
    fn ligature_merges_two_glyphs() {
        let data = synthetic_gsub_ligature();
        let gsub = Gsub::parse(&data).expect("parse synthetic GSUB");
        assert!(gsub.has_lookups());
        let mut buf = glyphs(&[5, 10, 11, 7]);
        gsub.apply(&mut buf);
        let ids: Vec<u16> = buf.iter().map(|g| g.glyph_id).collect();
        assert_eq!(ids, vec![5, 99, 7], "10+11 should merge into 99");
        // The ligature inherits the smallest source cluster of its parts.
        assert_eq!(buf[1].cluster, 1);
        assert_eq!(buf[2].cluster, 3);
    }

    #[test]
    fn ligature_leaves_non_matching_runs_alone() {
        let data = synthetic_gsub_ligature();
        let gsub = Gsub::parse(&data).expect("parse synthetic GSUB");
        let mut buf = glyphs(&[10, 12, 11]); // 10 not followed by 11
        gsub.apply(&mut buf);
        let ids: Vec<u16> = buf.iter().map(|g| g.glyph_id).collect();
        assert_eq!(ids, vec![10, 12, 11], "no ligature without the component");
    }
}
