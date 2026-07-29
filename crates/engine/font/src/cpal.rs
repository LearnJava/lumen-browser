//! `CPAL` table — Color Palette Table for COLR color fonts.
//!
//! Spec: <https://learn.microsoft.com/en-us/typography/opentype/spec/cpal>.
//!
//! `CPAL` stores one or more palettes; each palette is a run of
//! `num_palette_entries` BGRA color records taken from a shared
//! `colorRecords` array. A COLR layer references a palette *entry index*, and
//! the active palette decides the actual color — this is exactly the
//! indirection the CSS `font-palette` property (CSS Fonts L4 §11.3) selects
//! over: `normal` → palette 0, `light`/`dark` → the first palette flagged
//! with the matching `paletteType` bit (v1 only), `<dashed-ident>` →
//! `@font-palette-values` `base-palette` + `override-colors`.
//!
//! Version 0 has palettes only. Version 1 adds three optional arrays —
//! palette types (the light/dark flags), palette name IDs and palette entry
//! name IDs. Only the type array is parsed: the name IDs address the `name`
//! table and no Lumen path consumes them yet.

use crate::binary::BinaryReader;
use crate::face::FontError;

const CPAL: [u8; 4] = *b"CPAL";

/// One CPAL color record, converted from the on-disk BGRA order to RGBA.
///
/// Alpha is straight (non-premultiplied), per spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteColor {
    /// Red channel, 0–255.
    pub r: u8,
    /// Green channel, 0–255.
    pub g: u8,
    /// Blue channel, 0–255.
    pub b: u8,
    /// Straight (non-premultiplied) alpha, 0–255.
    pub a: u8,
}

/// Parsed `CPAL` table: palettes as flat runs over the color record array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpal {
    /// Table version (0 or 1). Higher versions are parsed as v1 — the v1
    /// header is a prefix of any future one.
    pub version: u16,
    /// Number of entries in every palette (all palettes share this length).
    pub num_palette_entries: u16,
    /// All color records of the table, in file order, converted to RGBA.
    pub color_records: Vec<PaletteColor>,
    /// Per-palette index of its first record inside [`Self::color_records`].
    pub palette_record_indices: Vec<u16>,
    /// `paletteType` flags, one per palette (v1 only; empty for v0 and for a
    /// v1 table whose type array offset is 0).
    pub palette_types: Vec<u32>,
}

impl Cpal {
    /// `paletteType` bit 0 — palette is usable with a light background.
    pub const USABLE_WITH_LIGHT_BACKGROUND: u32 = 0x0001;
    /// `paletteType` bit 1 — palette is usable with a dark background.
    pub const USABLE_WITH_DARK_BACKGROUND: u32 = 0x0002;

    /// Parses the `CPAL` table body.
    ///
    /// Returns `Err(InvalidTable)` for a truncated header, an out-of-range
    /// `offsetFirstColorRecord`, or a color record array that does not fit in
    /// the table. Individual palettes whose start index is out of range are
    /// *not* rejected here — [`Self::palette`] returns `None` for them, so a
    /// font with one bad palette still renders its good ones.
    pub fn parse(data: &[u8]) -> Result<Self, FontError> {
        let mut r = BinaryReader::new(data);
        let version = r.read_u16().ok_or(FontError::InvalidTable(CPAL))?;
        let num_palette_entries = r.read_u16().ok_or(FontError::InvalidTable(CPAL))?;
        let num_palettes = r.read_u16().ok_or(FontError::InvalidTable(CPAL))?;
        let num_color_records = r.read_u16().ok_or(FontError::InvalidTable(CPAL))?;
        let first_record_offset = r.read_u32().ok_or(FontError::InvalidTable(CPAL))?;

        let mut palette_record_indices = Vec::with_capacity(num_palettes as usize);
        for _ in 0..num_palettes {
            palette_record_indices.push(r.read_u16().ok_or(FontError::InvalidTable(CPAL))?);
        }

        // v1 tail: three u32 offsets, each relative to the table start and
        // `0` when the array is absent. Only the type array is read.
        let palette_types = if version >= 1 {
            let types_offset = r.read_u32().ok_or(FontError::InvalidTable(CPAL))?;
            // `offsetPaletteLabelArray` / `offsetPaletteEntryLabelArray`
            // follow; both address the `name` table and stay unparsed.
            if types_offset == 0 {
                Vec::new()
            } else {
                let mut tr = BinaryReader::new(data);
                tr.seek(types_offset as usize);
                let mut types = Vec::with_capacity(num_palettes as usize);
                for _ in 0..num_palettes {
                    types.push(tr.read_u32().ok_or(FontError::InvalidTable(CPAL))?);
                }
                types
            }
        } else {
            Vec::new()
        };

        let mut cr = BinaryReader::new(data);
        cr.seek(first_record_offset as usize);
        if cr.remaining() < num_color_records as usize * 4 {
            return Err(FontError::InvalidTable(CPAL));
        }
        let mut color_records = Vec::with_capacity(num_color_records as usize);
        for _ in 0..num_color_records {
            // On disk the order is blue, green, red, alpha.
            let b = cr.read_u8().ok_or(FontError::InvalidTable(CPAL))?;
            let g = cr.read_u8().ok_or(FontError::InvalidTable(CPAL))?;
            let red = cr.read_u8().ok_or(FontError::InvalidTable(CPAL))?;
            let a = cr.read_u8().ok_or(FontError::InvalidTable(CPAL))?;
            color_records.push(PaletteColor { r: red, g, b, a });
        }

        Ok(Self {
            version,
            num_palette_entries,
            color_records,
            palette_record_indices,
            palette_types,
        })
    }

    /// Number of palettes in the table.
    pub fn num_palettes(&self) -> u16 {
        self.palette_record_indices.len() as u16
    }

    /// Returns the `num_palette_entries` colors of palette `index`.
    ///
    /// `None` when `index` is out of range, or when the palette's recorded
    /// start index plus its length runs past the color record array (a
    /// malformed font — the palette is skipped rather than clamped, so a
    /// caller cannot silently paint with a half-wrong palette).
    pub fn palette(&self, index: u16) -> Option<&[PaletteColor]> {
        let start = *self.palette_record_indices.get(index as usize)? as usize;
        let end = start.checked_add(self.num_palette_entries as usize)?;
        self.color_records.get(start..end)
    }

    /// Index of the first palette flagged `USABLE_WITH_LIGHT_BACKGROUND`,
    /// used by `font-palette: light`.
    ///
    /// `None` when the table is v0, has no palette type array, or no palette
    /// carries the flag — the caller then falls back to palette 0 per CSS
    /// Fonts L4 §11.3 (`light`/`dark` behave as `normal` if the font has no
    /// matching palette).
    pub fn first_light_palette(&self) -> Option<u16> {
        self.first_palette_with_flag(Self::USABLE_WITH_LIGHT_BACKGROUND)
    }

    /// Index of the first palette flagged `USABLE_WITH_DARK_BACKGROUND`,
    /// used by `font-palette: dark`. Same fallback rules as
    /// [`Self::first_light_palette`].
    pub fn first_dark_palette(&self) -> Option<u16> {
        self.first_palette_with_flag(Self::USABLE_WITH_DARK_BACKGROUND)
    }

    fn first_palette_with_flag(&self, flag: u32) -> Option<u16> {
        self.palette_types
            .iter()
            .position(|t| t & flag != 0)
            .map(|i| i as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a CPAL table: `num_entries` entries per palette, `palettes`
    /// giving each palette's first-record index, `colors` the shared record
    /// array as RGBA tuples, `types` the optional v1 palette type flags.
    fn build_cpal(
        version: u16,
        num_entries: u16,
        palettes: &[u16],
        colors: &[(u8, u8, u8, u8)],
        types: Option<&[u32]>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&num_entries.to_be_bytes());
        out.extend_from_slice(&(palettes.len() as u16).to_be_bytes());
        out.extend_from_slice(&(colors.len() as u16).to_be_bytes());
        // Header (12) + palette indices + v1 tail (12 when present).
        let header_len = 12 + palettes.len() * 2 + if version >= 1 { 12 } else { 0 };
        let types_len = types.map_or(0, |t| t.len() * 4);
        let first_record_offset = (header_len + types_len) as u32;
        out.extend_from_slice(&first_record_offset.to_be_bytes());
        for p in palettes {
            out.extend_from_slice(&p.to_be_bytes());
        }
        if version >= 1 {
            let types_offset = if types.is_some() { header_len as u32 } else { 0 };
            out.extend_from_slice(&types_offset.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes()); // offsetPaletteLabelArray
            out.extend_from_slice(&0u32.to_be_bytes()); // offsetPaletteEntryLabelArray
        }
        if let Some(t) = types {
            for v in t {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        for &(r, g, b, a) in colors {
            // BGRA on disk.
            out.extend_from_slice(&[b, g, r, a]);
        }
        out
    }

    #[test]
    fn v0_single_palette_reads_rgba_from_bgra() {
        let data = build_cpal(0, 2, &[0], &[(10, 20, 30, 40), (50, 60, 70, 80)], None);
        let cpal = Cpal::parse(&data).unwrap();
        assert_eq!(cpal.version, 0);
        assert_eq!(cpal.num_palettes(), 1);
        let p = cpal.palette(0).unwrap();
        assert_eq!(p[0], PaletteColor { r: 10, g: 20, b: 30, a: 40 });
        assert_eq!(p[1], PaletteColor { r: 50, g: 60, b: 70, a: 80 });
    }

    #[test]
    fn two_palettes_share_the_record_array() {
        // 3 records, 2 entries per palette: palette 0 = [0,1], palette 1 = [1,2].
        let data = build_cpal(
            0,
            2,
            &[0, 1],
            &[(1, 1, 1, 255), (2, 2, 2, 255), (3, 3, 3, 255)],
            None,
        );
        let cpal = Cpal::parse(&data).unwrap();
        assert_eq!(cpal.palette(0).unwrap()[1].r, 2);
        assert_eq!(cpal.palette(1).unwrap()[0].r, 2);
        assert_eq!(cpal.palette(1).unwrap()[1].r, 3);
    }

    #[test]
    fn out_of_range_palette_index_is_none() {
        let data = build_cpal(0, 1, &[0], &[(1, 2, 3, 4)], None);
        let cpal = Cpal::parse(&data).unwrap();
        assert!(cpal.palette(1).is_none());
    }

    #[test]
    fn palette_running_past_records_is_none_not_clamped() {
        // Palette 1 starts at record 2 but needs 2 entries — only 3 exist.
        let data = build_cpal(
            0,
            2,
            &[0, 2],
            &[(1, 1, 1, 255), (2, 2, 2, 255), (3, 3, 3, 255)],
            None,
        );
        let cpal = Cpal::parse(&data).unwrap();
        assert!(cpal.palette(0).is_some());
        assert!(cpal.palette(1).is_none());
    }

    #[test]
    fn v1_palette_types_select_light_and_dark() {
        let data = build_cpal(
            1,
            1,
            &[0, 1, 2],
            &[(1, 1, 1, 255), (2, 2, 2, 255), (3, 3, 3, 255)],
            Some(&[0, Cpal::USABLE_WITH_DARK_BACKGROUND, Cpal::USABLE_WITH_LIGHT_BACKGROUND]),
        );
        let cpal = Cpal::parse(&data).unwrap();
        assert_eq!(cpal.first_dark_palette(), Some(1));
        assert_eq!(cpal.first_light_palette(), Some(2));
    }

    #[test]
    fn v0_has_no_light_or_dark_palette() {
        let data = build_cpal(0, 1, &[0], &[(1, 2, 3, 4)], None);
        let cpal = Cpal::parse(&data).unwrap();
        assert_eq!(cpal.first_light_palette(), None);
        assert_eq!(cpal.first_dark_palette(), None);
    }

    #[test]
    fn v1_without_type_array_has_no_light_or_dark() {
        let data = build_cpal(1, 1, &[0], &[(1, 2, 3, 4)], None);
        let cpal = Cpal::parse(&data).unwrap();
        assert!(cpal.palette_types.is_empty());
        assert_eq!(cpal.first_light_palette(), None);
    }

    #[test]
    fn truncated_header_is_rejected() {
        assert!(matches!(Cpal::parse(&[0, 0, 0]), Err(FontError::InvalidTable(_))));
    }

    #[test]
    fn color_records_past_table_end_are_rejected() {
        let mut data = build_cpal(0, 2, &[0], &[(1, 2, 3, 4), (5, 6, 7, 8)], None);
        data.truncate(data.len() - 4); // drop the last record
        assert!(matches!(Cpal::parse(&data), Err(FontError::InvalidTable(_))));
    }
}
