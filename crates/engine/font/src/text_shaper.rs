//! `lumen_core::ext::TextShaper` implementation (LIB-1/LIB-3, ADR-027).
//!
//! [`RustybuzzShaper`] shapes through `rustybuzz`: full complex-script
//! support — mark attachment, Arabic joining, Indic reordering. It replaced
//! this crate's own `GSUB`/`GPOS` engine (Latin/Cyrillic ligatures + kerning
//! only) as the default at `LIB-2` (2026-08-31, which re-shot the graphic-tests
//! references, the CPU snapshot PNGs and the paint crate's textual
//! display-list snapshots to match) and the own engine was deleted outright
//! at `LIB-3` (2026-09-01) once that swap had held.

use lumen_core::ext::{ShapeDirection, ShapedGlyph, TextShaper};

/// `TextShaper` backed by `rustybuzz` — the LIB-1 replacement, sole
/// implementation since `LIB-3`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RustybuzzShaper;

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

/// The [`TextShaper`] implementation callers should use.
///
/// `rustybuzz`-backed, the sole implementation since `LIB-3` removed the
/// crate's own `GSUB`/`GPOS` engine.
pub fn active_text_shaper() -> &'static dyn TextShaper {
    &RustybuzzShaper
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

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

    #[test]
    fn rustybuzz_shaper_reports_byte_offset_clusters() {
        // "café" — the 'é' is 2 bytes in UTF-8, so its cluster must be 1
        // (byte offset), not "index 1" read as a char count.
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

    #[test]
    fn rustybuzz_shaper_kerns_known_pair() {
        // The bundled Inter face kerns "AV" — a negative x-advance
        // adjustment vs. the bare hmtx advance.
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
