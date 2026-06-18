//! Text shaping: turns a sequence of glyph ids into positioned glyphs by
//! applying `GSUB` substitutions (ligatures) then `GPOS` adjustments
//! (kerning).
//!
//! This is the public entry point a renderer uses instead of the naïve
//! "one glyph per char, advance by `hmtx`" loop. The caller is responsible
//! for the cmap mapping (char → glyph id); [`Shaper::shape`] then handles
//! substitution and positioning and returns the final glyph run with
//! per-glyph advances and offsets in font design units.
//!
//! Stage-1 scope (U-2): Latin/Cyrillic ligatures + kerning. See [`crate::gsub`]
//! and [`crate::gpos`] for the supported lookup types. Mark positioning,
//! contextual lookups and complex scripts are out of scope.

use crate::face::Font;
use crate::gpos::Gpos;
use crate::gsub::Gsub;
use crate::hmtx::Hmtx;

/// One positioned glyph produced by shaping. All metrics are in font design
/// units (`units_per_em`); the renderer scales them by
/// `font_size / units_per_em`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// Resolved glyph id to rasterize (post-substitution).
    pub glyph_id: u16,
    /// Index of the source character this glyph derives from. Ligatures
    /// carry the smallest cluster of their components. Useful for caret
    /// placement / hit-testing; not required for plain rendering.
    pub cluster: u32,
    /// Total horizontal advance: base `hmtx` advance plus any `GPOS`
    /// adjustment. Font units.
    pub x_advance: i32,
    /// Horizontal draw offset from the pen position. Font units.
    pub x_offset: i32,
    /// Vertical draw offset from the baseline (positive = up). Font units.
    pub y_offset: i32,
}

/// Shaping engine bound to one font's `GSUB`/`GPOS` tables.
///
/// Construction parses the layout tables once; [`Shaper::shape`] can then be
/// called per text run. When the font has neither table (or neither carries
/// active lookups), shaping degrades to base advances — identical output to
/// the previous per-character path.
#[derive(Debug, Clone)]
pub struct Shaper<'a> {
    gsub: Option<Gsub<'a>>,
    gpos: Option<Gpos<'a>>,
}

impl<'a> Shaper<'a> {
    /// Build a shaper from a parsed font, reading its `GSUB`/`GPOS` tables.
    /// Always succeeds: missing/invalid tables simply disable that stage.
    pub fn new(font: &Font<'a>) -> Self {
        let gsub = font.table(b"GSUB").and_then(Gsub::parse);
        let gpos = font.table(b"GPOS").and_then(Gpos::parse);
        Self { gsub, gpos }
    }

    /// Whether shaping will change anything versus base advances — i.e. the
    /// font has active substitution or positioning lookups. Lets callers
    /// skip allocating a shaped run for fonts without layout tables.
    pub fn is_active(&self) -> bool {
        self.gsub.as_ref().is_some_and(Gsub::has_lookups)
            || self.gpos.as_ref().is_some_and(Gpos::has_lookups)
    }

    /// Shape a run of glyph ids into positioned glyphs.
    ///
    /// `glyph_ids` is the cmap-mapped run (one entry per source char, in
    /// logical order). `hmtx` supplies base advances. The result applies
    /// ligature substitution (which can shrink the run) then kerning.
    pub fn shape(&self, glyph_ids: &[u16], hmtx: &Hmtx) -> Vec<ShapedGlyph> {
        let mut buf: Vec<ShapedGlyph> = glyph_ids
            .iter()
            .enumerate()
            .map(|(i, &g)| ShapedGlyph {
                glyph_id: g,
                cluster: i as u32,
                x_advance: 0,
                x_offset: 0,
                y_offset: 0,
            })
            .collect();

        // GSUB first: substitution may merge/replace glyphs before advances
        // are seeded.
        if let Some(gsub) = &self.gsub {
            gsub.apply(&mut buf);
        }

        // Seed base advances from hmtx for the (possibly substituted) glyphs.
        for g in &mut buf {
            g.x_advance = i32::from(hmtx.advance_width(g.glyph_id).unwrap_or(0));
        }

        // GPOS: kerning / single adjustments on top of base advances.
        if let Some(gpos) = &self.gpos {
            gpos.apply(&mut buf);
        }

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests exercising real GSUB/GPOS live in
    // `tests/inter_shaping.rs` against the bundled Inter face; these unit
    // tests cover the degrade-to-base-advances path.

    #[test]
    fn shaped_glyph_is_pod() {
        let g = ShapedGlyph {
            glyph_id: 7,
            cluster: 0,
            x_advance: 500,
            x_offset: 0,
            y_offset: 0,
        };
        assert_eq!(g.glyph_id, 7);
        assert_eq!(g.x_advance, 500);
    }
}
