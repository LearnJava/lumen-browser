//! Integration tests for GSUB/GPOS shaping against the bundled
//! Inter-Regular face (U-2 stage 1).
//!
//! Synthetic tables verify the parsers in unit tests; this exercises the
//! real layout tables of a shipping font — the kind of coverage that caught
//! the historical `hhea` parser bug. Inter carries GPOS kerning and, via
//! its default-on `calt` feature, a `->` arrow ligature (type-4 ligature
//! substitution), so both shaping stages are observable here.

use std::path::PathBuf;

use lumen_font::{Font, Shaper};

fn font_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts")
        .join("Inter-Regular.ttf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Map a string to its cmap glyph ids (one per char, `.notdef` on miss).
fn glyph_ids(font: &Font, s: &str) -> Vec<u16> {
    let cmap = font.cmap().expect("cmap");
    s.chars()
        .map(|c| cmap.glyph_index(c as u32).unwrap_or(0))
        .collect()
}

#[test]
fn inter_has_layout_tables() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    assert!(font.table(b"GSUB").is_some(), "Inter should have GSUB");
    assert!(font.table(b"GPOS").is_some(), "Inter should have GPOS");
    let shaper = Shaper::new(&font);
    assert!(shaper.is_active(), "Inter shaper should have active lookups");
}

#[test]
fn gpos_kerning_tightens_av_pair() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    let hmtx = font.hmtx().expect("hmtx");
    let shaper = Shaper::new(&font);

    let ids = glyph_ids(&font, "AV");
    let base_a = hmtx.advance_width(ids[0]).unwrap() as i32;
    let shaped = shaper.shape(&ids, &hmtx);

    assert_eq!(shaped.len(), 2, "AV should not substitute");
    // The kerning adjustment lands on the *first* glyph of the pair.
    assert!(
        shaped[0].x_advance < base_a,
        "expected A in 'AV' to kern tighter than its base advance {base_a}, got {}",
        shaped[0].x_advance
    );
    // Second glyph keeps its base advance.
    let base_v = hmtx.advance_width(ids[1]).unwrap() as i32;
    assert_eq!(shaped[1].x_advance, base_v);
}

#[test]
fn gpos_leaves_unkerned_pair_at_base_advance() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    let hmtx = font.hmtx().expect("hmtx");
    let shaper = Shaper::new(&font);

    let ids = glyph_ids(&font, "ll");
    let base = hmtx.advance_width(ids[0]).unwrap() as i32;
    let shaped = shaper.shape(&ids, &hmtx);
    assert_eq!(shaped.len(), 2);
    assert_eq!(shaped[0].x_advance, base, "ll should not kern");
    assert_eq!(shaped[1].x_advance, base);
}

#[test]
fn gsub_ligature_merges_arrow() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    let hmtx = font.hmtx().expect("hmtx");
    let shaper = Shaper::new(&font);

    // Inter's calt feature ligates "->" into a single arrow glyph.
    let ids = glyph_ids(&font, "->");
    assert_eq!(ids.len(), 2);
    let shaped = shaper.shape(&ids, &hmtx);
    assert_eq!(shaped.len(), 1, "'->' should ligate to one glyph");
    assert_ne!(shaped[0].glyph_id, ids[0], "ligature glyph differs from '-'");
    assert_eq!(shaped[0].cluster, 0, "ligature inherits first component cluster");
    assert!(shaped[0].x_advance > 0);
}

#[test]
fn gsub_ligature_preserves_surrounding_clusters() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    let hmtx = font.hmtx().expect("hmtx");
    let shaper = Shaper::new(&font);

    // "x->y": x (cluster 0), arrow (cluster 1 from "-"), y (cluster 3).
    let ids = glyph_ids(&font, "x->y");
    let shaped = shaper.shape(&ids, &hmtx);
    assert_eq!(shaped.len(), 3, "only the arrow ligates");
    assert_eq!(shaped[0].cluster, 0);
    assert_eq!(shaped[1].cluster, 1);
    assert_eq!(shaped[2].cluster, 3, "trailing glyph keeps its source index");
}

#[test]
fn plain_text_is_not_corrupted() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    let hmtx = font.hmtx().expect("hmtx");
    let shaper = Shaper::new(&font);

    // Latin + Cyrillic with no ligatable sequences: glyph count is stable
    // and every glyph maps back to its source character one-to-one.
    let text = "Hello, мир 123";
    let ids = glyph_ids(&font, text);
    let shaped = shaper.shape(&ids, &hmtx);
    assert_eq!(shaped.len(), ids.len(), "no substitutions expected");
    for (i, g) in shaped.iter().enumerate() {
        assert_eq!(g.cluster, i as u32);
        assert_eq!(g.glyph_id, ids[i]);
        assert!(g.x_advance > 0);
    }
}

#[test]
fn empty_input_yields_empty_run() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter");
    let hmtx = font.hmtx().expect("hmtx");
    let shaper = Shaper::new(&font);
    assert!(shaper.shape(&[], &hmtx).is_empty());
}
