//! Complex-script conformance for [`lumen_font::RustybuzzShaper`] (LIB-1).
//!
//! Marked `#[ignore]` — machine-dependent (needs real Windows system fonts
//! with Arabic/Devanagari coverage: `crate::shape::Shaper`'s own engine has
//! no complex-script support at all, so there is nothing to test it
//! against, and the bundled Inter face has neither script). Run manually:
//! `cargo test -p lumen-font --test all -- --ignored --nocapture rustybuzz_complex_scripts`.
//!
//! This is the direct-API counterpart to the LIB-0 conformance probe
//! (`docs/conformance/probes/text-shaping.html`): that probe cannot show
//! these checks at all today, because every headless/MCP screenshot path
//! renders through the bundled Inter face only (see CLAUDE.md's "No
//! headless or MCP screenshot path renders text through `SystemFontIndex`"
//! gotcha, 2026-08-31) — Inter has none of these glyphs. Testing
//! `RustybuzzShaper::shape` directly against a real system font sidesteps
//! that blind spot entirely.

use lumen_core::ext::{ShapeDirection, TextShaper};
use lumen_font::{OwnTextShaper, RustybuzzShaper};
use std::path::Path;

/// Reads a system font file, skipping the check (not failing it) if this
/// machine doesn't have it — same convention as
/// `cases::real_system_fonts`.
fn read_system_font(candidates: &[&str]) -> Option<Vec<u8>> {
    for name in candidates {
        let path = Path::new(r"C:\Windows\Fonts").join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            return Some(bytes);
        }
    }
    None
}

#[test]
#[ignore]
fn arabic_medial_form_differs_from_isolated_form() {
    let Some(font) = read_system_font(&["tahoma.ttf"]) else {
        eprintln!("skip: tahoma.ttf not found on this machine");
        return;
    };

    // Arabic BEH (U+0628) alone shapes to its isolated form; the same
    // letter sandwiched between two others (here ALEF, BEH, ALEF) must
    // shape to its MEDIAL form — a different glyph id. `crate::shape`'s own
    // engine has no GSUB 5/6 (contextual/chained) lookups, so it always
    // emits the isolated-form glyph regardless of context; this is exactly
    // what LIB-1 replaces.
    let isolated = RustybuzzShaper.shape(
        &font,
        "\u{0628}",
        ShapeDirection::RightToLeft,
        Some(*b"Arab"),
        &[],
        &[],
    );
    let in_word = RustybuzzShaper.shape(
        &font,
        "\u{0627}\u{0628}\u{0627}",
        ShapeDirection::RightToLeft,
        Some(*b"Arab"),
        &[],
        &[],
    );
    assert_eq!(isolated.len(), 1);
    assert_eq!(in_word.len(), 3, "three letters must not merge/drop glyphs");

    let isolated_beh_glyph = isolated[0].glyph_id;
    let medial_beh_glyph = in_word[1].glyph_id;
    assert_ne!(
        isolated_beh_glyph, medial_beh_glyph,
        "medial BEH must use a different glyph than isolated BEH — proves \
         contextual joining (GSUB 5/6) ran"
    );
}

#[test]
#[ignore]
fn combining_diacritic_attaches_via_mark_positioning() {
    let Some(font) = read_system_font(&["tahoma.ttf"]) else {
        eprintln!("skip: tahoma.ttf not found on this machine");
        return;
    };

    // ARABIC LETTER BEH (U+0628) + ARABIC FATHA (U+064E). Arabic tashkeel
    // (harakat) have no precomposed codepoint and are never pre-ligated —
    // there are far too many consonant+mark combinations for a font to
    // bother — so a font can only place them correctly via `GPOS` Lookup
    // Type 4 (mark-to-base), which `crate::shape::Shaper` does not
    // implement (LIB-1 task doc). Measured directly against Tahoma: the
    // own engine places the mark at a bare `(0, 0)` offset (floating next
    // to the base, at the pen position its own zero `hmtx` advance leaves
    // it — a lucky-looking but wrong placement, not attachment), while
    // rustybuzz resolves a real anchor offset.
    let base_alone = RustybuzzShaper.shape(
        &font,
        "\u{0628}",
        ShapeDirection::RightToLeft,
        Some(*b"Arab"),
        &[],
        &[],
    );
    assert_eq!(base_alone.len(), 1);
    let base_glyph = base_alone[0].glyph_id;

    let combo = RustybuzzShaper.shape(
        &font,
        "\u{0628}\u{064E}",
        ShapeDirection::RightToLeft,
        Some(*b"Arab"),
        &[],
        &[],
    );
    assert_eq!(combo.len(), 2, "base + mark, no merging expected here");
    let mark = combo
        .iter()
        .find(|g| g.glyph_id != base_glyph)
        .expect("combo must contain a glyph other than the base — the mark");
    assert_eq!(
        mark.x_advance, 0,
        "an attached mark must not advance the pen — got {}",
        mark.x_advance
    );
    assert!(
        mark.x_offset != 0 || mark.y_offset != 0,
        "rustybuzz must resolve a real GPOS anchor offset for the mark, \
         not leave it at (0, 0)"
    );

    // Contrast: `crate::shape::Shaper` cannot do this at all — its mark
    // lands at a bare (0, 0), proving the trait-anchor's own-engine path
    // still lacks GPOS 4, exactly as documented.
    let own_combo = OwnTextShaper.shape(
        &font,
        "\u{0628}\u{064E}",
        ShapeDirection::RightToLeft,
        None,
        &[],
        &[],
    );
    let own_mark = own_combo
        .iter()
        .find(|g| g.glyph_id != base_glyph)
        .expect("own engine must not drop the mark glyph either");
    assert_eq!(own_mark.x_offset, 0, "own engine has no mark anchor to apply");
    assert_eq!(own_mark.y_offset, 0, "own engine has no mark anchor to apply");
}

#[test]
#[ignore]
fn devanagari_vowel_sign_reorders_before_consonant() {
    let Some(font) = read_system_font(&["Nirmala.ttf", "mangal.ttf"]) else {
        eprintln!("skip: no Devanagari-capable font found on this machine");
        return;
    };

    // U+0915 (KA) + U+093F (dependent vowel sign I) — visually, the vowel
    // sign draws to the LEFT of the consonant it modifies, although it
    // follows it in logical/typed order. A shaper with no script-specific
    // reordering (this is not a `GSUB` lookup type at all — it needs a
    // dedicated Indic shaping engine) emits glyphs in logical order
    // unchanged; rustybuzz's Devanagari shaper must reorder them.
    //
    // `cluster` can't be used to observe the reorder directly: rustybuzz's
    // default cluster level (`MonotoneGraphemes`) deliberately MERGES
    // cluster values across a reordered group so they stay non-decreasing
    // — that's the point of the setting, not a bug. Compare glyph ids
    // against the consonant shaped alone instead. (The vowel sign shaped
    // alone is not a useful reference: with no consonant to attach to, the
    // Indic shaper inserts a dotted-circle placeholder glyph next to it,
    // which is correct behaviour but means "shape it alone" no longer
    // identifies a single glyph id.)
    let ka_alone = RustybuzzShaper.shape(
        &font,
        "\u{0915}",
        ShapeDirection::LeftToRight,
        Some(*b"Deva"),
        &[],
        &[],
    );
    assert_eq!(ka_alone.len(), 1);
    let ka_glyph = ka_alone[0].glyph_id;

    let shaped = RustybuzzShaper.shape(
        &font,
        "\u{0915}\u{093F}",
        ShapeDirection::LeftToRight,
        Some(*b"Deva"),
        &[],
        &[],
    );
    assert_eq!(shaped.len(), 2, "no merging expected for this pair");
    assert_eq!(
        shaped[1].glyph_id, ka_glyph,
        "the consonant must be emitted SECOND — reordered after the vowel \
         sign that logically follows it"
    );
    assert_ne!(
        shaped[0].glyph_id, ka_glyph,
        "the first glyph must be the reordered vowel sign, not the \
         consonant appearing in logical (typed) order"
    );
}
