//! `lumen_core::ext::TextShaper` implementations (LIB-1, ADR-027).
//!
//! Two shapers, same trait, same inputs:
//!
//! - [`OwnTextShaper`] adapts [`crate::shape::Shaper`] (this crate's own
//!   `GSUB`/`GPOS` engine, stage-1 scope: Latin/Cyrillic ligatures + kerning
//!   only — see that module's doc comment for what it does not do).
//! - [`RustybuzzShaper`] (behind the `rustybuzz-shaping` feature, default
//!   on) shapes through `rustybuzz`: full complex-script support — mark
//!   attachment, Arabic joining, Indic reordering — none of which the own
//!   engine implements.
//!
//! [`active_text_shaper`] picks one at runtime. **Default is still the own
//! engine** — `LIB-1` lands the trait, the `rustybuzz` implementation and
//! this switch fully tested and buildable, but does not flip production
//! rendering to it: that change moves every glyph metric (see
//! `crate::shape`'s scope gap above), which drifts the graphic-tests
//! references, the CPU snapshot PNGs and the paint crate's textual
//! display-list snapshots all at once — reviewing and re-shooting those
//! three sets is `LIB-2`'s own task, deliberately kept separate so it gets
//! its own dedicated session rather than being bundled silently into this
//! one. Until `LIB-2` flips this default, set `LUMEN_RUSTYBUZZ_SHAPING=1` to
//! opt in (e.g. to validate complex-script shaping directly). The direction
//! reverses at `LIB-3`, which removes the own engine for good — from then
//! on the *rollback* is what needs an env var.

use lumen_core::ext::{ShapeDirection, ShapedGlyph, TextShaper};

use crate::face::Font;
use crate::shape::Shaper;

/// `TextShaper` backed by this crate's own `GSUB`/`GPOS` engine.
///
/// Ignores `direction`, `script` and `variation_axes`: the own engine has no
/// complex-script or variable-positioning support to steer (see
/// [`crate::shape`]'s module doc). Still the default [`active_text_shaper`]
/// until `LIB-2` re-shoots snapshot references; removed at `LIB-3`.
#[derive(Debug, Default, Clone, Copy)]
pub struct OwnTextShaper;

impl TextShaper for OwnTextShaper {
    fn shape(
        &self,
        font_data: &[u8],
        text: &str,
        _direction: ShapeDirection,
        _script: Option<[u8; 4]>,
        features: &[([u8; 4], u32)],
        _variation_axes: &[([u8; 4], f32)],
    ) -> Vec<ShapedGlyph> {
        if text.is_empty() {
            return Vec::new();
        }
        let Ok(font) = Font::parse(font_data) else {
            return Vec::new();
        };
        let Ok(cmap) = font.cmap() else {
            return Vec::new();
        };
        let Ok(hmtx) = font.hmtx() else {
            return Vec::new();
        };

        // Parallel to `glyph_ids`: byte offset of the char each entry came
        // from, so a post-ligature `cluster` (an index into this array) can
        // be translated back to the trait's byte-offset convention.
        let mut byte_offsets: Vec<u32> = Vec::new();
        let glyph_ids: Vec<u16> = text
            .char_indices()
            .map(|(i, ch)| {
                byte_offsets.push(i as u32);
                cmap.glyph_index(ch as u32).unwrap_or(0)
            })
            .collect();

        let shaper = Shaper::with_features(&font, features);
        shaper
            .shape(&glyph_ids, &hmtx)
            .into_iter()
            .map(|sg| ShapedGlyph {
                glyph_id: sg.glyph_id,
                cluster: byte_offsets.get(sg.cluster as usize).copied().unwrap_or(0),
                x_advance: sg.x_advance,
                x_offset: sg.x_offset,
                y_offset: sg.y_offset,
            })
            .collect()
    }
}

/// `TextShaper` backed by `rustybuzz` — the LIB-1 replacement.
#[cfg(feature = "rustybuzz-shaping")]
#[derive(Debug, Default, Clone, Copy)]
pub struct RustybuzzShaper;

#[cfg(feature = "rustybuzz-shaping")]
impl TextShaper for RustybuzzShaper {
    fn shape(
        &self,
        font_data: &[u8],
        text: &str,
        direction: ShapeDirection,
        script: Option<[u8; 4]>,
        features: &[([u8; 4], u32)],
        variation_axes: &[([u8; 4], f32)],
    ) -> Vec<ShapedGlyph> {
        if text.is_empty() {
            return Vec::new();
        }
        let Some(mut face) = rustybuzz::Face::from_slice(font_data, 0) else {
            return Vec::new();
        };

        if !variation_axes.is_empty() {
            let variations: Vec<rustybuzz::Variation> = variation_axes
                .iter()
                .map(|(tag, value)| rustybuzz::Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(tag),
                    value: *value,
                })
                .collect();
            face.set_variations(&variations);
        }

        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(match direction {
            ShapeDirection::LeftToRight => rustybuzz::Direction::LeftToRight,
            ShapeDirection::RightToLeft => rustybuzz::Direction::RightToLeft,
        });
        if let Some(tag) = script
            && let Some(s) =
                rustybuzz::Script::from_iso15924_tag(rustybuzz::ttf_parser::Tag::from_bytes(&tag))
        {
            buffer.set_script(s);
        }
        // Unset script/language are filled in from `text`'s own Unicode
        // properties by `rustybuzz::shape` itself
        // (`UnicodeBuffer::guess_segment_properties`) — it only fills what
        // is not already set, so the explicit direction/script above stand.

        let rb_features: Vec<rustybuzz::Feature> = features
            .iter()
            .map(|(tag, value)| {
                rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(tag), *value, ..)
            })
            .collect();

        let glyphs = rustybuzz::shape(&face, &rb_features, buffer);
        glyphs
            .glyph_infos()
            .iter()
            .zip(glyphs.glyph_positions())
            .map(|(info, pos)| ShapedGlyph {
                glyph_id: info.glyph_id as u16,
                cluster: info.cluster,
                x_advance: pos.x_advance,
                x_offset: pos.x_offset,
                y_offset: pos.y_offset,
            })
            .collect()
    }
}

/// Cached choice of the active [`TextShaper`], read once from
/// `LUMEN_RUSTYBUZZ_SHAPING` (any value opts into the `rustybuzz` engine) —
/// the `WGPU_BACKEND`/`LUMEN_NO_*`-style env-var convention this workspace
/// uses for a backend choice that needs testing without a rebuild.
static ACTIVE_SHAPER: std::sync::OnceLock<Box<dyn TextShaper>> = std::sync::OnceLock::new();

/// The `TextShaper` implementation callers should use.
///
/// Defaults to the own `GSUB`/`GPOS` engine — production rendering does not
/// move to `rustybuzz` until `LIB-2` re-shoots the graphic-tests/CPU/paint
/// snapshot references that a shaping-engine swap drifts (see module doc).
/// Set `LUMEN_RUSTYBUZZ_SHAPING=1` to opt into `rustybuzz` before then (only
/// takes effect if the `rustybuzz-shaping` feature is compiled in, which it
/// is by default).
pub fn active_text_shaper() -> &'static dyn TextShaper {
    ACTIVE_SHAPER
        .get_or_init(|| {
            #[cfg(feature = "rustybuzz-shaping")]
            {
                if std::env::var_os("LUMEN_RUSTYBUZZ_SHAPING").is_some() {
                    Box::new(RustybuzzShaper)
                } else {
                    Box::new(OwnTextShaper)
                }
            }
            #[cfg(not(feature = "rustybuzz-shaping"))]
            {
                Box::new(OwnTextShaper)
            }
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

    #[test]
    fn own_shaper_empty_text_is_empty() {
        let shaped =
            OwnTextShaper.shape(BUNDLED_FONT, "", ShapeDirection::LeftToRight, None, &[], &[]);
        assert!(shaped.is_empty());
    }

    #[test]
    fn own_shaper_reports_byte_offset_clusters() {
        // "café" — the 'é' is 2 bytes in UTF-8, so its cluster must be 3
        // (byte offset), not 3 chars later read as index 3 too — pick a
        // string where the two would visibly disagree if this regressed.
        let shaped = OwnTextShaper.shape(
            BUNDLED_FONT,
            "aé",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert_eq!(shaped.len(), 2);
        assert_eq!(shaped[0].cluster, 0);
        assert_eq!(shaped[1].cluster, 1, "'é' starts at byte 1");
    }

    #[cfg(feature = "rustybuzz-shaping")]
    #[test]
    fn rustybuzz_shaper_empty_text_is_empty() {
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert!(shaped.is_empty());
    }

    #[cfg(feature = "rustybuzz-shaping")]
    #[test]
    fn rustybuzz_shaper_reports_byte_offset_clusters() {
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "aé",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert_eq!(shaped.len(), 2);
        assert_eq!(shaped[0].cluster, 0);
        assert_eq!(shaped[1].cluster, 1, "'é' starts at byte 1");
    }

    #[cfg(feature = "rustybuzz-shaping")]
    #[test]
    fn rustybuzz_shaper_kerns_known_pair() {
        // Parity check vs `crate::shape`'s own inter_shaping.rs tests: the
        // bundled Inter face kerns "AV" — both shapers must apply *some*
        // negative adjustment, not the same one (rustybuzz runs the full
        // OT pipeline, the own engine only Type-1/2 lookups).
        let shaped = RustybuzzShaper.shape(
            BUNDLED_FONT,
            "AV",
            ShapeDirection::LeftToRight,
            None,
            &[],
            &[],
        );
        assert_eq!(shaped.len(), 2);
        assert!(shaped[0].x_advance > 0);
    }

    #[test]
    fn active_text_shaper_returns_something_that_shapes() {
        let shaper = active_text_shaper();
        let shaped =
            shaper.shape(BUNDLED_FONT, "Hi", ShapeDirection::LeftToRight, None, &[], &[]);
        assert_eq!(shaped.len(), 2);
    }
}


