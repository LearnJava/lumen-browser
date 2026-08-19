//! Metrics conformance test for the bundled Ahem.ttf (TEST-5).
//!
//! Ahem is the standard WPT/CSS-WG font for deterministic reftests: every
//! glyph is a solid square exactly one em wide, with a fixed 0.8em/0.2em
//! ascent/descent split. This test pins those numbers against our own
//! parser so a bad re-vendor (wrong file, re-hinted copy, etc.) fails loudly
//! instead of silently producing fuzzy reftest baselines. See
//! docs/tasks/p2-test-track.md#test-5-шрифт-ahem-s.

use std::path::PathBuf;

use lumen_font::Font;

fn font_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts")
        .join("Ahem.ttf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

#[test]
fn ahem_em_square_and_vertical_metrics() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Ahem.ttf");

    let head = font.head().expect("head table");
    assert_eq!(head.units_per_em, 1000, "Ahem em square is 1000 units");

    let hhea = font.hhea().expect("hhea table");
    // Ahem spec: ascent = 0.8em above baseline, descent = 0.2em below.
    assert_eq!(hhea.ascent, 800, "Ahem ascent is 0.8em");
    assert_eq!(hhea.descent, -200, "Ahem descent is 0.2em below baseline");
    assert_eq!(
        hhea.ascent - hhea.descent,
        head.units_per_em as i16,
        "ascent + descent (as a positive span) must cover exactly one em"
    );
}

#[test]
fn ahem_glyph_advance_equals_em_for_common_chars() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Ahem.ttf");

    let units_per_em = font.head().expect("head table").units_per_em;
    let cmap = font.cmap().expect("cmap table");
    let hmtx = font.hmtx().expect("hmtx table");

    // Letters, a digit, punctuation and space — Ahem gives every one of
    // these an advance of exactly 1em, unlike a normal font.
    for ch in ['A', 'X', 'a', 'z', '0', '!', '.', ' '] {
        let gid = cmap
            .glyph_index(ch as u32)
            .unwrap_or_else(|| panic!("'{ch}' not mapped in Ahem cmap"));
        let advance = hmtx
            .advance_width(gid)
            .unwrap_or_else(|| panic!("'{ch}' (glyph {gid}) missing hmtx entry"));
        assert_eq!(
            advance, units_per_em,
            "'{ch}' advance must equal the em square (1000), got {advance}"
        );
    }
}
