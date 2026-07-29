//! `COLR` table — layered color glyph definitions.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/colr>.
//!
//! Version 0 describes a color glyph as an ordered list of *layers*: each
//! layer is an ordinary monochrome glyph plus a palette entry index into the
//! [`crate::cpal::Cpal`] table. Painting a color glyph means rasterizing each
//! layer glyph in order (back to front) with its palette color. The special
//! palette index `0xFFFF` means «use the text foreground color», which is how
//! a color font mixes in `currentColor`.
//!
//! Version 1 adds a paint graph (gradients, transforms, compositing) in
//! separate arrays that follow the v0 header. This parser reads the v0
//! records only — a v1 font still carries them for backwards compatibility,
//! and a v1-only glyph simply has no v0 base record, so
//! [`Colr::layers_for`] returns `None` and the caller falls back to the plain
//! monochrome outline. The v1 paint graph is deferred.

use crate::binary::BinaryReader;
use crate::face::FontError;

const COLR: [u8; 4] = *b"COLR";

/// Palette index meaning «use the text foreground color» instead of a CPAL
/// entry (spec: `0xFFFF`).
pub const PALETTE_INDEX_FOREGROUND: u16 = 0xFFFF;

/// One layer of a color glyph: which glyph to draw and in which color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    /// Glyph id of the monochrome outline forming this layer.
    pub glyph_id: u16,
    /// CPAL palette entry index, or [`PALETTE_INDEX_FOREGROUND`] for the
    /// text color.
    pub palette_index: u16,
}

/// A v0 base glyph record: the layer range belonging to one color glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseGlyph {
    /// Glyph id the color definition applies to.
    pub glyph_id: u16,
    /// Index of the first layer in [`Colr::layers`].
    pub first_layer_index: u16,
    /// Number of consecutive layers.
    pub num_layers: u16,
}

/// Parsed `COLR` table (version 0 records).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colr {
    /// Table version (0, 1, …). Only the v0 records are parsed.
    pub version: u16,
    /// Base glyph records, sorted by `glyph_id` (spec requirement — the
    /// lookup in [`Self::layers_for`] binary-searches them).
    pub base_glyphs: Vec<BaseGlyph>,
    /// Flat layer array shared by all base glyph records.
    pub layers: Vec<Layer>,
}

impl Colr {
    /// Parses the `COLR` table body.
    ///
    /// Returns `Err(InvalidTable)` when the header is truncated or either
    /// record array runs past the end of the table.
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let version = r.read_u16().ok_or(FontError::InvalidTable(COLR))?;
        let num_base_glyphs = r.read_u16().ok_or(FontError::InvalidTable(COLR))?;
        let base_glyphs_offset = r.read_u32().ok_or(FontError::InvalidTable(COLR))?;
        let layers_offset = r.read_u32().ok_or(FontError::InvalidTable(COLR))?;
        let num_layers = r.read_u16().ok_or(FontError::InvalidTable(COLR))?;

        let mut br = BinaryReader::new(data);
        br.seek(base_glyphs_offset as usize);
        if br.remaining() < num_base_glyphs as usize * 6 {
            return Err(FontError::InvalidTable(COLR));
        }
        let mut base_glyphs = Vec::with_capacity(num_base_glyphs as usize);
        for _ in 0..num_base_glyphs {
            base_glyphs.push(BaseGlyph {
                glyph_id: br.read_u16().ok_or(FontError::InvalidTable(COLR))?,
                first_layer_index: br.read_u16().ok_or(FontError::InvalidTable(COLR))?,
                num_layers: br.read_u16().ok_or(FontError::InvalidTable(COLR))?,
            });
        }

        let mut lr = BinaryReader::new(data);
        lr.seek(layers_offset as usize);
        if lr.remaining() < num_layers as usize * 4 {
            return Err(FontError::InvalidTable(COLR));
        }
        let mut layers = Vec::with_capacity(num_layers as usize);
        for _ in 0..num_layers {
            layers.push(Layer {
                glyph_id: lr.read_u16().ok_or(FontError::InvalidTable(COLR))?,
                palette_index: lr.read_u16().ok_or(FontError::InvalidTable(COLR))?,
            });
        }

        Ok(Self { version, base_glyphs, layers })
    }

    /// `true` when the table defines no v0 color glyph at all (a v1-only
    /// font, or an empty table). Callers use it to skip the color path
    /// entirely.
    pub fn is_empty(&self) -> bool {
        self.base_glyphs.is_empty()
    }

    /// Layers of the color glyph `glyph_id`, back to front.
    ///
    /// `None` when the glyph has no v0 color definition (draw the plain
    /// outline instead), or when the record's layer range is out of bounds in
    /// a malformed font.
    pub fn layers_for(&self, glyph_id: u16) -> Option<&[Layer]> {
        let idx = self
            .base_glyphs
            .binary_search_by_key(&glyph_id, |b| b.glyph_id)
            .ok()?;
        let rec = self.base_glyphs[idx];
        let start = rec.first_layer_index as usize;
        let end = start.checked_add(rec.num_layers as usize)?;
        let slice = self.layers.get(start..end)?;
        if slice.is_empty() { None } else { Some(slice) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a COLR v0 table from base glyph records `(glyph, first, count)`
    /// and layer records `(glyph, palette_index)`.
    fn build_colr(
        version: u16,
        bases: &[(u16, u16, u16)],
        layers: &[(u16, u16)],
    ) -> Vec<u8> {
        let header_len = 14u32;
        let base_offset = header_len;
        let layer_offset = base_offset + bases.len() as u32 * 6;
        let mut out = Vec::new();
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&(bases.len() as u16).to_be_bytes());
        out.extend_from_slice(&base_offset.to_be_bytes());
        out.extend_from_slice(&layer_offset.to_be_bytes());
        out.extend_from_slice(&(layers.len() as u16).to_be_bytes());
        for &(g, first, count) in bases {
            out.extend_from_slice(&g.to_be_bytes());
            out.extend_from_slice(&first.to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
        }
        for &(g, p) in layers {
            out.extend_from_slice(&g.to_be_bytes());
            out.extend_from_slice(&p.to_be_bytes());
        }
        out
    }

    #[test]
    fn parses_v0_records() {
        let data = build_colr(0, &[(5, 0, 2)], &[(100, 0), (101, 1)]);
        let colr = Colr::parse(&data).unwrap();
        assert_eq!(colr.version, 0);
        assert_eq!(colr.base_glyphs.len(), 1);
        assert_eq!(colr.layers.len(), 2);
        let layers = colr.layers_for(5).unwrap();
        assert_eq!(layers[0], Layer { glyph_id: 100, palette_index: 0 });
        assert_eq!(layers[1], Layer { glyph_id: 101, palette_index: 1 });
    }

    #[test]
    fn lookup_binary_searches_sorted_records() {
        let data = build_colr(
            0,
            &[(2, 0, 1), (7, 1, 2), (9, 3, 1)],
            &[(10, 0), (11, 0), (12, 1), (13, 2)],
        );
        let colr = Colr::parse(&data).unwrap();
        assert_eq!(colr.layers_for(2).unwrap().len(), 1);
        assert_eq!(colr.layers_for(7).unwrap().len(), 2);
        assert_eq!(colr.layers_for(7).unwrap()[1].glyph_id, 12);
        assert_eq!(colr.layers_for(9).unwrap()[0].glyph_id, 13);
    }

    #[test]
    fn glyph_without_color_definition_is_none() {
        let data = build_colr(0, &[(5, 0, 1)], &[(100, 0)]);
        let colr = Colr::parse(&data).unwrap();
        assert!(colr.layers_for(4).is_none());
        assert!(colr.layers_for(6).is_none());
    }

    #[test]
    fn zero_layer_record_is_none() {
        let data = build_colr(0, &[(5, 0, 0)], &[(100, 0)]);
        let colr = Colr::parse(&data).unwrap();
        assert!(colr.layers_for(5).is_none());
    }

    #[test]
    fn layer_range_past_array_is_none() {
        // Record claims 3 layers starting at 1, but only 2 layers exist.
        let data = build_colr(0, &[(5, 1, 3)], &[(100, 0), (101, 1)]);
        let colr = Colr::parse(&data).unwrap();
        assert!(colr.layers_for(5).is_none());
    }

    #[test]
    fn foreground_palette_index_preserved() {
        let data = build_colr(0, &[(5, 0, 1)], &[(100, PALETTE_INDEX_FOREGROUND)]);
        let colr = Colr::parse(&data).unwrap();
        assert_eq!(colr.layers_for(5).unwrap()[0].palette_index, PALETTE_INDEX_FOREGROUND);
    }

    #[test]
    fn v1_table_without_v0_records_is_empty() {
        let data = build_colr(1, &[], &[]);
        let colr = Colr::parse(&data).unwrap();
        assert_eq!(colr.version, 1);
        assert!(colr.is_empty());
        assert!(colr.layers_for(0).is_none());
    }

    #[test]
    fn truncated_header_is_rejected() {
        assert!(matches!(Colr::parse(&[0, 0, 0, 1]), Err(FontError::InvalidTable(_))));
    }

    #[test]
    fn records_past_table_end_are_rejected() {
        let mut data = build_colr(0, &[(5, 0, 2)], &[(100, 0), (101, 1)]);
        data.truncate(data.len() - 4);
        assert!(matches!(Colr::parse(&data), Err(FontError::InvalidTable(_))));
    }
}
