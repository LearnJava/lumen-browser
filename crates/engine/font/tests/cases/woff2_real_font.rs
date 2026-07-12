//! Integration test for the WOFF2 decoder on a real font (BUG-160).
//!
//! Regression guard: before the fix, `decode_woff2` corrupted the transformed
//! `glyf`/`loca` reconstruction (coordinates read from the wrong substream,
//! instruction length read out of order, `loca` format not patched into
//! `head`), so every real WOFF2 font failed with "unexpected end of font data".
//!
//! Fixture: Fira Mono Regular (SIL OFL 1.1) — a TrueType (`glyf`) font in WOFF2
//! form with the glyf transform applied. See `tests/fonts/ATTRIBUTION.md`.

use std::path::PathBuf;

use lumen_font::Font;
use lumen_font::glyf::Outline;

fn fixture() -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fonts/FiraMono-Regular.woff2");
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn decodes_real_woff2_to_parsable_sfnt() {
    let raw = fixture();
    assert!(lumen_font::is_woff2(&raw), "fixture must be WOFF2");

    let sfnt = lumen_font::decode_woff2(&raw).expect("WOFF2 decode must succeed");
    // sfnt magic for TrueType is 0x00010000.
    assert_eq!(u32::from_be_bytes([sfnt[0], sfnt[1], sfnt[2], sfnt[3]]), 0x0001_0000);

    let font = Font::parse(&sfnt).expect("reconstructed sfnt must parse");

    // After reconstruction loca is long-form; head must agree.
    assert_eq!(
        font.head().unwrap().index_to_loc_format,
        lumen_font::head::IndexToLocFormat::Long,
        "head.indexToLocFormat must be patched to Long for the synthesised loca"
    );
}

#[test]
fn reconstructed_glyphs_have_sane_outlines() {
    let raw = fixture();
    let sfnt = lumen_font::decode_woff2(&raw).unwrap();
    let font = Font::parse(&sfnt).unwrap();
    let head = font.head().unwrap();
    let cmap = font.cmap().unwrap();

    // Every tested letter must reconstruct to a non-degenerate outline that
    // sits inside the font bounding box — proves triplet coords are correct,
    // not merely that the container parses.
    let mut simple_seen = false;
    for ch in "ABCDEFGabcdefg0123".chars() {
        let gid = cmap.glyph_index(ch as u32).expect("glyph in cmap");
        let glyph = font
            .glyph(gid)
            .expect("glyph parse")
            .expect("glyph has outline");
        let b = glyph.bbox;
        assert!(b.x_max > b.x_min && b.y_max > b.y_min, "'{ch}': degenerate bbox {b:?}");
        assert!(
            b.x_min >= head.x_min - 50
                && b.x_max <= head.x_max + 50
                && b.y_min >= head.y_min - 50
                && b.y_max <= head.y_max + 50,
            "'{ch}': bbox {b:?} escapes font bbox"
        );
        if let Outline::Simple(contours) = &glyph.outline
            && !contours.is_empty()
        {
            simple_seen = true;
        }
    }
    assert!(simple_seen, "expected at least one reconstructed simple-glyph outline");
}
