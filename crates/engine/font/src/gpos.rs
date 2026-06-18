//! `GPOS` — Glyph Positioning table.
//!
//! Stage-1 scope (Latin/Cyrillic kerning): Lookup Type 1 (Single
//! adjustment) and Lookup Type 2 (Pair adjustment, both the glyph-pair
//! format 1 and the class-pair format 2), plus Type 9 (Extension) which
//! wraps another lookup. Pair adjustment is the table that carries the bulk
//! of a font's kerning in modern OpenType fonts.
//!
//! Lookup Types 3/4/5/6/7/8 (cursive, mark-to-base/ligature/mark,
//! contextual, chained) are recognised but skipped — they target
//! complex-script positioning beyond the stage-1 goal.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/gpos>.

use crate::otlayout::{
    ClassDef, Coverage, LayoutTable, read_value_record, resolve_extension, value_record_size,
};
use crate::shape::ShapedGlyph;

/// Feature tag enabled by default for positioning: horizontal kerning.
pub const GPOS_FEATURES: [[u8; 4]; 1] = [*b"kern"];

#[inline]
fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(pos..pos + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

/// Parsed `GPOS` table plus the lookup indices activated by the enabled
/// positioning features, in application order.
#[derive(Debug, Clone)]
pub struct Gpos<'a> {
    table: LayoutTable<'a>,
    lookups: Vec<u16>,
}

impl<'a> Gpos<'a> {
    /// Parse the `GPOS` table bytes and pre-select the lookups for the
    /// default positioning features (`kern`). Returns `None` if malformed.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let table = LayoutTable::parse(data)?;
        let lookups = table.enabled_lookups(&GPOS_FEATURES);
        Some(Self { table, lookups })
    }

    /// Whether any positioning lookups are active.
    pub fn has_lookups(&self) -> bool {
        !self.lookups.is_empty()
    }

    /// Apply all enabled positioning lookups to `glyphs` in order. Advances
    /// must already be seeded with each glyph's base `hmtx` advance.
    pub fn apply(&self, glyphs: &mut [ShapedGlyph]) {
        for &li in &self.lookups {
            let Some(lookup) = self.table.lookup(li) else {
                continue;
            };
            self.apply_lookup(lookup.lookup_type, &lookup.subtables, glyphs);
        }
    }

    fn apply_lookup(&self, lookup_type: u16, subtables: &[usize], glyphs: &mut [ShapedGlyph]) {
        for &sub in subtables {
            let (real_type, real_off) = if lookup_type == 9 {
                match resolve_extension(self.table.data, sub) {
                    Some(v) => v,
                    None => continue,
                }
            } else {
                (lookup_type, sub)
            };
            match real_type {
                1 => self.apply_single(real_off, glyphs),
                2 => self.apply_pair(real_off, glyphs),
                _ => {}
            }
        }
    }

    /// GPOS Lookup Type 1 — Single Adjustment (formats 1 and 2). Adds a
    /// fixed (format 1) or per-glyph (format 2) ValueRecord to each covered
    /// glyph.
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
        let Some(value_format) = u16_at(data, off + 4) else {
            return;
        };
        for g in glyphs.iter_mut() {
            let Some(idx) = cov.index_of(g.glyph_id) else {
                continue;
            };
            let value = match format {
                1 => read_value_record(data, off + 6, value_format),
                2 => {
                    let rec_size = value_record_size(value_format);
                    read_value_record(data, off + 8 + idx as usize * rec_size, value_format)
                }
                _ => None,
            };
            if let Some(v) = value {
                g.x_advance += v.x_advance as i32;
                g.x_offset += v.x_placement as i32;
                g.y_offset += v.y_placement as i32;
            }
        }
    }

    /// GPOS Lookup Type 2 — Pair Adjustment. The first glyph of an adjacent
    /// pair carries the adjustment (value1); value2 on the second glyph is
    /// applied when present. Handles format 1 (explicit glyph pairs) and
    /// format 2 (class-based pairs — how most large kerning tables are
    /// stored).
    fn apply_pair(&self, off: usize, glyphs: &mut [ShapedGlyph]) {
        let data = self.table.data;
        let Some(format) = u16_at(data, off) else {
            return;
        };
        match format {
            1 => self.apply_pair_format1(off, glyphs),
            2 => self.apply_pair_format2(off, glyphs),
            _ => {}
        }
    }

    fn apply_pair_format1(&self, off: usize, glyphs: &mut [ShapedGlyph]) {
        let data = self.table.data;
        let Some(cov_off) = u16_at(data, off + 2) else {
            return;
        };
        let Some(cov) = Coverage::parse(data, off + cov_off as usize) else {
            return;
        };
        let (Some(vf1), Some(vf2), Some(pair_set_count)) = (
            u16_at(data, off + 4),
            u16_at(data, off + 6),
            u16_at(data, off + 8),
        ) else {
            return;
        };
        let vf1_size = value_record_size(vf1);
        let vf2_size = value_record_size(vf2);
        let pair_rec_size = 2 + vf1_size + vf2_size; // secondGlyph + value1 + value2

        for i in 0..glyphs.len().saturating_sub(1) {
            let first = glyphs[i].glyph_id;
            let second = glyphs[i + 1].glyph_id;
            let Some(cov_idx) = cov.index_of(first) else {
                continue;
            };
            if cov_idx >= pair_set_count {
                continue;
            }
            // pairSetOffsets[] at off+10, relative to off.
            let Some(set_rel) = u16_at(data, off + 10 + cov_idx as usize * 2) else {
                continue;
            };
            let set_off = off + set_rel as usize;
            let Some(pair_value_count) = u16_at(data, set_off) else {
                continue;
            };
            // PairValueRecords start at set_off+2; binary structure is flat,
            // scan for the matching secondGlyph.
            for p in 0..pair_value_count as usize {
                let rec = set_off + 2 + p * pair_rec_size;
                let Some(sg) = u16_at(data, rec) else { break };
                if sg != second {
                    continue;
                }
                if let Some(v1) = read_value_record(data, rec + 2, vf1) {
                    glyphs[i].x_advance += v1.x_advance as i32;
                    glyphs[i].x_offset += v1.x_placement as i32;
                    glyphs[i].y_offset += v1.y_placement as i32;
                }
                if vf2 != 0
                    && let Some(v2) = read_value_record(data, rec + 2 + vf1_size, vf2)
                {
                    glyphs[i + 1].x_advance += v2.x_advance as i32;
                    glyphs[i + 1].x_offset += v2.x_placement as i32;
                    glyphs[i + 1].y_offset += v2.y_placement as i32;
                }
                break;
            }
        }
    }

    fn apply_pair_format2(&self, off: usize, glyphs: &mut [ShapedGlyph]) {
        let data = self.table.data;
        let Some(cov_off) = u16_at(data, off + 2) else {
            return;
        };
        let Some(cov) = Coverage::parse(data, off + cov_off as usize) else {
            return;
        };
        let (Some(vf1), Some(vf2)) = (u16_at(data, off + 4), u16_at(data, off + 6)) else {
            return;
        };
        let (Some(cd1_off), Some(cd2_off)) = (u16_at(data, off + 8), u16_at(data, off + 10)) else {
            return;
        };
        let (Some(class1_count), Some(class2_count)) =
            (u16_at(data, off + 12), u16_at(data, off + 14))
        else {
            return;
        };
        let cd1 = ClassDef::parse(data, off + cd1_off as usize);
        let cd2 = ClassDef::parse(data, off + cd2_off as usize);
        let (Some(cd1), Some(cd2)) = (cd1, cd2) else {
            return;
        };

        let vf1_size = value_record_size(vf1);
        let vf2_size = value_record_size(vf2);
        let class2_rec_size = vf1_size + vf2_size;
        let class1_rec_size = class2_count as usize * class2_rec_size;
        // Class1Records start at off+16.
        let records_base = off + 16;

        for i in 0..glyphs.len().saturating_sub(1) {
            let first = glyphs[i].glyph_id;
            // First glyph must be covered for the subtable to apply.
            if cov.index_of(first).is_none() {
                continue;
            }
            let c1 = cd1.class_of(first);
            let c2 = cd2.class_of(glyphs[i + 1].glyph_id);
            if c1 >= class1_count || c2 >= class2_count {
                continue;
            }
            let rec = records_base
                + c1 as usize * class1_rec_size
                + c2 as usize * class2_rec_size;
            if let Some(v1) = read_value_record(data, rec, vf1) {
                glyphs[i].x_advance += v1.x_advance as i32;
                glyphs[i].x_offset += v1.x_placement as i32;
                glyphs[i].y_offset += v1.y_placement as i32;
            }
            if vf2 != 0
                && let Some(v2) = read_value_record(data, rec + vf1_size, vf2)
            {
                glyphs[i + 1].x_advance += v2.x_advance as i32;
                glyphs[i + 1].x_offset += v2.x_placement as i32;
                glyphs[i + 1].y_offset += v2.y_placement as i32;
            }
        }
    }
}
