//! Common OpenType Layout structures shared by `GSUB` and `GPOS`.
//!
//! Both tables begin with the same header (version + ScriptList /
//! FeatureList / LookupList offsets) and reuse the same building blocks:
//! Coverage tables, Class Definition tables and (for `GPOS`) ValueRecords.
//! This module parses those shared pieces and the script→langsys→feature→
//! lookup navigation that selects which lookups are active for a given set
//! of feature tags.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/chapter2>.
//!
//! Scope (Phase «Interactive», U-2 stage 1): enough of the common layer to
//! drive Latin/Cyrillic ligatures (`GSUB`) and kerning (`GPOS`). LookupFlag
//! mark filtering, FeatureVariations and the lookupOrder field are parsed
//! past but not honoured — our shaping targets simple scripts without
//! combining marks.

use crate::binary::BinaryReader;

/// Read a big-endian `u16` at an absolute byte offset, `None` if out of bounds.
#[inline]
fn u16_at(data: &[u8], pos: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(pos..pos + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

/// Read a big-endian `i16` at an absolute byte offset, `None` if out of bounds.
#[inline]
fn i16_at(data: &[u8], pos: usize) -> Option<i16> {
    let bytes: [u8; 2] = data.get(pos..pos + 2)?.try_into().ok()?;
    Some(i16::from_be_bytes(bytes))
}

/// Parsed header of a `GSUB`/`GPOS` table: byte offsets (relative to the
/// table start) of the three core lists. Version 1.1's `featureVariations`
/// offset is read past but not retained — variable-font feature variations
/// are out of scope for stage-1 shaping.
#[derive(Debug, Clone, Copy)]
pub struct LayoutHeader {
    /// Byte offset of the ScriptList, relative to the table start.
    pub script_list: usize,
    /// Byte offset of the FeatureList, relative to the table start.
    pub feature_list: usize,
    /// Byte offset of the LookupList, relative to the table start.
    pub lookup_list: usize,
}

impl LayoutHeader {
    /// Parse the 10-byte (v1.0) / 14-byte (v1.1) header at the start of a
    /// `GSUB`/`GPOS` table. Returns `None` if the data is too short or any
    /// list offset points past the end of the table.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut r = BinaryReader::new(data);
        let _major = r.read_u16()?;
        let _minor = r.read_u16()?;
        let script_list = r.read_u16()? as usize;
        let feature_list = r.read_u16()? as usize;
        let lookup_list = r.read_u16()? as usize;
        if script_list > data.len() || feature_list > data.len() || lookup_list > data.len() {
            return None;
        }
        Some(Self {
            script_list,
            feature_list,
            lookup_list,
        })
    }
}

/// A single lookup: its type, flags and the absolute byte offsets (within
/// the owning `GSUB`/`GPOS` table) of each of its subtables.
#[derive(Debug, Clone)]
pub struct Lookup {
    /// Lookup type as stored (1..=9). For an Extension lookup (GSUB 7 /
    /// GPOS 9) the *actual* type is resolved per-subtable by the caller.
    pub lookup_type: u16,
    /// LookupFlag bitfield (parsed; mark filtering not yet honoured).
    pub lookup_flag: u16,
    /// Absolute offsets of each subtable, relative to the table start.
    pub subtables: Vec<usize>,
}

/// Borrowed view over a `GSUB`/`GPOS` table providing lookup access and the
/// feature→lookup navigation used to pick active lookups by feature tag.
#[derive(Debug, Clone, Copy)]
pub struct LayoutTable<'a> {
    /// The full `GSUB`/`GPOS` table bytes; all offsets are relative to this.
    pub data: &'a [u8],
    /// Parsed list offsets.
    pub header: LayoutHeader,
}

impl<'a> LayoutTable<'a> {
    /// Parse the table header; returns `None` for malformed/empty data.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        Some(Self {
            data,
            header: LayoutHeader::parse(data)?,
        })
    }

    /// Total number of lookups in the LookupList.
    pub fn lookup_count(&self) -> u16 {
        u16_at(self.data, self.header.lookup_list).unwrap_or(0)
    }

    /// Resolve a lookup by its LookupList index: returns its type, flags and
    /// the absolute offsets of its subtables. `None` if the index is out of
    /// range or the lookup is structurally invalid.
    pub fn lookup(&self, index: u16) -> Option<Lookup> {
        let base = self.header.lookup_list;
        let count = u16_at(self.data, base)?;
        if index >= count {
            return None;
        }
        // lookupOffsets[] start right after the count (2 bytes).
        let off_pos = base + 2 + index as usize * 2;
        let lookup_off = base + u16_at(self.data, off_pos)? as usize;
        let lookup_type = u16_at(self.data, lookup_off)?;
        let lookup_flag = u16_at(self.data, lookup_off + 2)?;
        let sub_count = u16_at(self.data, lookup_off + 4)?;
        let mut subtables = Vec::with_capacity(sub_count as usize);
        for i in 0..sub_count as usize {
            let sub_off = u16_at(self.data, lookup_off + 6 + i * 2)? as usize;
            subtables.push(lookup_off + sub_off);
        }
        Some(Lookup {
            lookup_type,
            lookup_flag,
            subtables,
        })
    }

    /// Collect the LookupList indices activated by any of the `wanted`
    /// feature tags, under a default script/language selection.
    ///
    /// Selection policy (simple-script shaping): prefer the `DFLT` script,
    /// else `latn`, else `cyrl`, else the first script; within it the
    /// default LangSys (else the first LangSys). The required feature, if
    /// present, is always included. Returned indices are sorted ascending
    /// and de-duplicated — lookups must be applied in LookupList order.
    pub fn enabled_lookups(&self, wanted: &[[u8; 4]]) -> Vec<u16> {
        let mut out = Vec::new();
        let Some(langsys) = self.select_langsys() else {
            return out;
        };
        let feature_indices = self.langsys_features(langsys);
        for fi in feature_indices {
            let Some((tag, lookups)) = self.feature(fi) else {
                continue;
            };
            if wanted.contains(&tag) {
                out.extend(lookups);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Pick a LangSys table offset (relative to table start) following the
    /// default-script policy described on [`Self::enabled_lookups`].
    fn select_langsys(&self) -> Option<usize> {
        let sl = self.header.script_list;
        let count = u16_at(self.data, sl)?;
        let mut scripts: Vec<([u8; 4], usize)> = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let rec = sl + 2 + i * 6;
            let tag: [u8; 4] = self.data.get(rec..rec + 4)?.try_into().ok()?;
            let off = sl + u16_at(self.data, rec + 4)? as usize;
            scripts.push((tag, off));
        }
        // Preference order for the script.
        let pick = [*b"DFLT", *b"latn", *b"cyrl"];
        let script_off = pick
            .iter()
            .find_map(|t| scripts.iter().find(|(tag, _)| tag == t).map(|(_, o)| *o))
            .or_else(|| scripts.first().map(|(_, o)| *o))?;

        // Script table: defaultLangSysOffset (may be NULL), langSysCount, records.
        let default_off = u16_at(self.data, script_off)? as usize;
        if default_off != 0 {
            return Some(script_off + default_off);
        }
        // No default LangSys: take the first explicit one.
        let lang_count = u16_at(self.data, script_off + 2)?;
        if lang_count == 0 {
            return None;
        }
        let rec = script_off + 4; // first LangSysRecord
        let off = u16_at(self.data, rec + 4)? as usize;
        Some(script_off + off)
    }

    /// Feature indices referenced by a LangSys table (required feature first
    /// if set, then the explicit feature list).
    fn langsys_features(&self, langsys: usize) -> Vec<u16> {
        let mut out = Vec::new();
        // LangSys: lookupOrder(Offset16, reserved), requiredFeatureIndex,
        // featureIndexCount, featureIndices[].
        let Some(required) = u16_at(self.data, langsys + 2) else {
            return out;
        };
        if required != 0xFFFF {
            out.push(required);
        }
        let Some(fcount) = u16_at(self.data, langsys + 4) else {
            return out;
        };
        for i in 0..fcount as usize {
            if let Some(fi) = u16_at(self.data, langsys + 6 + i * 2) {
                out.push(fi);
            }
        }
        out
    }

    /// Resolve a FeatureList index to its 4-byte tag and the LookupList
    /// indices it activates.
    fn feature(&self, index: u16) -> Option<([u8; 4], Vec<u16>)> {
        let fl = self.header.feature_list;
        let count = u16_at(self.data, fl)?;
        if index >= count {
            return None;
        }
        let rec = fl + 2 + index as usize * 6;
        let tag: [u8; 4] = self.data.get(rec..rec + 4)?.try_into().ok()?;
        let feature_off = fl + u16_at(self.data, rec + 4)? as usize;
        // Feature: featureParamsOffset, lookupIndexCount, lookupListIndices[].
        let lookup_count = u16_at(self.data, feature_off + 2)?;
        let mut lookups = Vec::with_capacity(lookup_count as usize);
        for i in 0..lookup_count as usize {
            if let Some(li) = u16_at(self.data, feature_off + 4 + i * 2) {
                lookups.push(li);
            }
        }
        Some((tag, lookups))
    }
}

/// A Coverage table: maps a glyph id to a *coverage index* (its ordinal
/// position within the table) or `None` if the glyph is not covered.
///
/// Coverage indices are the key used to look up parallel arrays in most
/// GSUB/GPOS subtables.
#[derive(Debug, Clone)]
pub enum Coverage {
    /// Format 1: an explicit sorted list of glyph ids; coverage index is
    /// the position in the list.
    List(Vec<u16>),
    /// Format 2: sorted, non-overlapping glyph ranges, each carrying the
    /// coverage index of its first glyph.
    Ranges(Vec<CoverageRange>),
}

/// One range record of a format-2 Coverage table.
#[derive(Debug, Clone, Copy)]
pub struct CoverageRange {
    /// First glyph id in the range (inclusive).
    pub start: u16,
    /// Last glyph id in the range (inclusive).
    pub end: u16,
    /// Coverage index assigned to `start`; subsequent glyphs increment it.
    pub start_index: u16,
}

impl Coverage {
    /// Parse a Coverage table located at absolute `offset` within `data`.
    pub fn parse(data: &[u8], offset: usize) -> Option<Self> {
        let format = u16_at(data, offset)?;
        match format {
            1 => {
                let count = u16_at(data, offset + 2)? as usize;
                let mut glyphs = Vec::with_capacity(count);
                for i in 0..count {
                    glyphs.push(u16_at(data, offset + 4 + i * 2)?);
                }
                Some(Coverage::List(glyphs))
            }
            2 => {
                let count = u16_at(data, offset + 2)? as usize;
                let mut ranges = Vec::with_capacity(count);
                for i in 0..count {
                    let rec = offset + 4 + i * 6;
                    ranges.push(CoverageRange {
                        start: u16_at(data, rec)?,
                        end: u16_at(data, rec + 2)?,
                        start_index: u16_at(data, rec + 4)?,
                    });
                }
                Some(Coverage::Ranges(ranges))
            }
            _ => None,
        }
    }

    /// Return the coverage index of `glyph`, or `None` if not covered.
    pub fn index_of(&self, glyph: u16) -> Option<u16> {
        match self {
            Coverage::List(glyphs) => glyphs
                .binary_search(&glyph)
                .ok()
                .map(|pos| pos as u16),
            Coverage::Ranges(ranges) => {
                let pos = ranges
                    .binary_search_by(|r| {
                        if glyph < r.start {
                            std::cmp::Ordering::Greater
                        } else if glyph > r.end {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .ok()?;
                let r = ranges[pos];
                Some(r.start_index + (glyph - r.start))
            }
        }
    }
}

/// A Class Definition table: maps a glyph id to a class number (0 for any
/// glyph not explicitly listed). Used by class-based pair kerning (GPOS
/// PairPos format 2) and contextual lookups.
#[derive(Debug, Clone)]
pub enum ClassDef {
    /// Format 1: classes for a contiguous run of glyph ids starting at
    /// `start_glyph`.
    Range {
        /// First glyph id covered by `classes`.
        start_glyph: u16,
        /// Class value for `start_glyph + i`.
        classes: Vec<u16>,
    },
    /// Format 2: explicit glyph ranges each mapped to a class.
    Ranges(Vec<ClassRange>),
}

/// One range record of a format-2 ClassDef table.
#[derive(Debug, Clone, Copy)]
pub struct ClassRange {
    /// First glyph id in the range (inclusive).
    pub start: u16,
    /// Last glyph id in the range (inclusive).
    pub end: u16,
    /// Class assigned to every glyph in the range.
    pub class: u16,
}

impl ClassDef {
    /// Parse a ClassDef table at absolute `offset`. A NULL (`0`) offset has
    /// no table; callers treat that as "every glyph is class 0".
    pub fn parse(data: &[u8], offset: usize) -> Option<Self> {
        let format = u16_at(data, offset)?;
        match format {
            1 => {
                let start_glyph = u16_at(data, offset + 2)?;
                let count = u16_at(data, offset + 4)? as usize;
                let mut classes = Vec::with_capacity(count);
                for i in 0..count {
                    classes.push(u16_at(data, offset + 6 + i * 2)?);
                }
                Some(ClassDef::Range {
                    start_glyph,
                    classes,
                })
            }
            2 => {
                let count = u16_at(data, offset + 2)? as usize;
                let mut ranges = Vec::with_capacity(count);
                for i in 0..count {
                    let rec = offset + 4 + i * 6;
                    ranges.push(ClassRange {
                        start: u16_at(data, rec)?,
                        end: u16_at(data, rec + 2)?,
                        class: u16_at(data, rec + 4)?,
                    });
                }
                Some(ClassDef::Ranges(ranges))
            }
            _ => None,
        }
    }

    /// Return the class of `glyph` (0 when not explicitly assigned).
    pub fn class_of(&self, glyph: u16) -> u16 {
        match self {
            ClassDef::Range {
                start_glyph,
                classes,
            } => {
                if glyph < *start_glyph {
                    return 0;
                }
                let idx = (glyph - start_glyph) as usize;
                classes.get(idx).copied().unwrap_or(0)
            }
            ClassDef::Ranges(ranges) => ranges
                .binary_search_by(|r| {
                    if glyph < r.start {
                        std::cmp::Ordering::Greater
                    } else if glyph > r.end {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .ok()
                .map(|pos| ranges[pos].class)
                .unwrap_or(0),
        }
    }
}

// ValueRecord format flag bits (GPOS).
const X_PLACEMENT: u16 = 0x0001;
const Y_PLACEMENT: u16 = 0x0002;
const X_ADVANCE: u16 = 0x0004;
const Y_ADVANCE: u16 = 0x0008;
const X_PLACEMENT_DEVICE: u16 = 0x0010;
const Y_PLACEMENT_DEVICE: u16 = 0x0020;
const X_ADVANCE_DEVICE: u16 = 0x0040;
const Y_ADVANCE_DEVICE: u16 = 0x0080;

/// A GPOS ValueRecord: positional adjustments in font design units. Fields
/// absent from the owning subtable's valueFormat stay `0`. Device-table
/// offsets are consumed but ignored (no hinting at our render sizes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValueRecord {
    /// Horizontal placement adjustment (glyph x offset), font units.
    pub x_placement: i16,
    /// Vertical placement adjustment (glyph y offset), font units.
    pub y_placement: i16,
    /// Horizontal advance adjustment, font units.
    pub x_advance: i16,
    /// Vertical advance adjustment, font units (unused for horizontal text).
    pub y_advance: i16,
}

/// Number of bytes a ValueRecord with `format` occupies (2 per set bit).
pub fn value_record_size(format: u16) -> usize {
    (format.count_ones() as usize) * 2
}

/// Read a ValueRecord of the given `format` at absolute `offset`, returning
/// the record and the number of bytes consumed. Present fields are read in
/// the canonical bit order; device-table offsets are skipped over.
pub fn read_value_record(data: &[u8], offset: usize, format: u16) -> Option<ValueRecord> {
    let mut pos = offset;
    let mut v = ValueRecord::default();
    if format & X_PLACEMENT != 0 {
        v.x_placement = i16_at(data, pos)?;
        pos += 2;
    }
    if format & Y_PLACEMENT != 0 {
        v.y_placement = i16_at(data, pos)?;
        pos += 2;
    }
    if format & X_ADVANCE != 0 {
        v.x_advance = i16_at(data, pos)?;
        pos += 2;
    }
    if format & Y_ADVANCE != 0 {
        v.y_advance = i16_at(data, pos)?;
        pos += 2;
    }
    // Device/VariationIndex offsets — 2 bytes each, ignored.
    for bit in [
        X_PLACEMENT_DEVICE,
        Y_PLACEMENT_DEVICE,
        X_ADVANCE_DEVICE,
        Y_ADVANCE_DEVICE,
    ] {
        if format & bit != 0 {
            pos += 2;
        }
    }
    let _ = pos;
    Some(v)
}

/// Resolve an Extension subtable (GSUB Lookup Type 7 / GPOS Lookup Type 9):
/// returns the real lookup type and the absolute offset of the wrapped
/// subtable. `None` if the extension format is unsupported.
pub fn resolve_extension(data: &[u8], offset: usize) -> Option<(u16, usize)> {
    let format = u16_at(data, offset)?;
    if format != 1 {
        return None;
    }
    let ext_type = u16_at(data, offset + 2)?;
    // Offset32 to the wrapped subtable, relative to the extension subtable.
    let ext_off_bytes: [u8; 4] = data.get(offset + 4..offset + 8)?.try_into().ok()?;
    let ext_off = u32::from_be_bytes(ext_off_bytes) as usize;
    Some((ext_type, offset + ext_off))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_format1_index() {
        // format=1, count=3, glyphs [5,9,20]
        let data = [0, 1, 0, 3, 0, 5, 0, 9, 0, 20];
        let cov = Coverage::parse(&data, 0).unwrap();
        assert_eq!(cov.index_of(5), Some(0));
        assert_eq!(cov.index_of(9), Some(1));
        assert_eq!(cov.index_of(20), Some(2));
        assert_eq!(cov.index_of(6), None);
    }

    #[test]
    fn coverage_format2_index() {
        // format=2, count=1, range start=10 end=13 startIndex=4
        let data = [0, 2, 0, 1, 0, 10, 0, 13, 0, 4];
        let cov = Coverage::parse(&data, 0).unwrap();
        assert_eq!(cov.index_of(10), Some(4));
        assert_eq!(cov.index_of(12), Some(6));
        assert_eq!(cov.index_of(13), Some(7));
        assert_eq!(cov.index_of(14), None);
        assert_eq!(cov.index_of(9), None);
    }

    #[test]
    fn classdef_format1() {
        // format=1, start=7, count=3, classes [1,2,1]
        let data = [0, 1, 0, 7, 0, 3, 0, 1, 0, 2, 0, 1];
        let cd = ClassDef::parse(&data, 0).unwrap();
        assert_eq!(cd.class_of(6), 0);
        assert_eq!(cd.class_of(7), 1);
        assert_eq!(cd.class_of(8), 2);
        assert_eq!(cd.class_of(9), 1);
        assert_eq!(cd.class_of(10), 0);
    }

    #[test]
    fn classdef_format2() {
        // format=2, count=2, ranges (3..5 -> 1), (10..10 -> 2)
        let data = [0, 2, 0, 2, 0, 3, 0, 5, 0, 1, 0, 10, 0, 10, 0, 2];
        let cd = ClassDef::parse(&data, 0).unwrap();
        assert_eq!(cd.class_of(3), 1);
        assert_eq!(cd.class_of(5), 1);
        assert_eq!(cd.class_of(6), 0);
        assert_eq!(cd.class_of(10), 2);
    }

    #[test]
    fn value_record_reads_only_present_fields() {
        // format with X_PLACEMENT | X_ADVANCE = 0x0005
        let data = [0xFF, 0xFB, 0x00, 0x10]; // xPlacement=-5, xAdvance=16
        let v = read_value_record(&data, 0, 0x0005).unwrap();
        assert_eq!(v.x_placement, -5);
        assert_eq!(v.x_advance, 16);
        assert_eq!(v.y_placement, 0);
        assert_eq!(value_record_size(0x0005), 4);
    }
}
