//! CSS Fragmentation L3 §4 / CSS Grid L1 §pagination — a grid container the simple path
//! (`emit_grid_fragments`: a border-less grid of border-less leaves) does not take is cut by
//! `multicol_flow` between its row tracks and through its items.

use crate::{BoxKind, Display, LayoutBox, first_element_child, lay_measured};

const COLS: &str = "#c { width: 300px; column-count: 3; column-gap: 0px; column-fill: auto; height: 100px; } \
    #g { display: grid; } #g > div { background: green; } ";

fn lay(body: &str, css: &str) -> LayoutBox {
    let root = lay_measured(&format!("<div id='c'>{body}</div>"), &format!("{COLS}{css}"), 800.0);
    first_element_child(&root).clone()
}

/// The grid fragments of the multicol container: `(x, y, height)` and the number of visible row tracks.
fn fragments(c: &LayoutBox) -> Vec<(f32, f32, f32, Option<usize>)> {
    c.children
        .iter()
        .filter(|f| matches!(f.kind, BoxKind::Block) && f.style.display == Display::Grid)
        .map(|f| (f.rect.x, f.rect.y, f.rect.height, f.subgrid_tracks.as_ref().and_then(|t| t.rows.as_ref()).map(Vec::len)))
        .collect()
}

#[test]
fn bordered_items_split_between_rows() {
    let c = lay(
        "<div id='g' style='grid-template-rows: 100px 100px'>\
         <div style='border: 5px solid; height: 90px'></div><div style='border: 5px solid; height: 90px'></div></div>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "one grid fragment per column: {f:?}");
    assert!(f[0].0 < 1.0 && (f[1].0 - 100.0).abs() < 1.0, "x: {} {}", f[0].0, f[1].0);
    assert!((f[0].2 - 100.0).abs() < 0.5 && (f[1].2 - 100.0).abs() < 0.5, "heights: {} {}", f[0].2, f[1].2);
    assert_eq!(f[0].3, Some(1));
    assert_eq!(f[1].3, Some(1));
}

#[test]
fn a_padded_grid_keeps_padding_on_the_outer_edges() {
    let c = lay(
        "<div id='g' style='padding: 10px 0; grid-template-rows: 40px 40px 40px'>\
         <div style='break-inside: avoid'></div><div style='break-inside: avoid'></div><div style='break-inside: avoid'></div></div>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    // 10 + 40 + 40 = 90 fit; the third row (+ bottom padding) goes to the next column.
    assert!((f[0].2 - 90.0).abs() < 0.5, "first fragment drops the bottom padding: {}", f[0].2);
    assert!((f[1].2 - 50.0).abs() < 0.5, "second fragment drops the top padding: {}", f[1].2);
}

#[test]
fn row_gap_at_a_break_is_dropped() {
    let c = lay(
        "<div id='g' style='row-gap: 30px; grid-template-rows: 80px 80px 80px'>\
         <div style='border: 1px solid'></div><div style='border: 1px solid'></div><div style='border: 1px solid'></div></div>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 3, "{f:?}");
    for (k, fr) in f.iter().enumerate() {
        assert!((fr.2 - 80.0).abs() < 0.5, "fragment {k} holds exactly its track: {}", fr.2);
        assert_eq!(fr.3, Some(1));
    }
}

#[test]
fn break_before_on_an_item_starts_a_column() {
    let c = lay(
        "<div id='g' style='grid-template-rows: 20px 20px 20px'>\
         <div style='border: 1px solid'></div><div style='border: 1px solid; break-before: column'></div><div></div></div>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!(f[0].3, Some(1));
    assert_eq!(f[1].3, Some(2));
}

#[test]
fn an_item_spanning_two_rows_is_cut_inside_not_between_the_rows() {
    // The item covers both 60px rows (120px): the only legal break falls inside the item.
    let c = lay(
        "<div id='g' style='grid-template-rows: 60px 60px'>\
         <div style='grid-row: 1 / 3; border: 1px solid'></div></div>",
        "",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!((f[0].2 - 100.0).abs() < 0.5, "the first column is full: {}", f[0].2);
    assert!((f[1].2 - 20.0).abs() < 0.5, "the rest of the item: {}", f[1].2);
}

#[test]
fn a_balanced_grid_spreads_its_rows() {
    let c = lay(
        "<div id='g' style='grid-template-rows: 50px 50px'><div style='border: 1px solid'></div><div style='border: 1px solid'></div></div>",
        "#c { column-count: 2; column-fill: balance; }",
    );
    let f = fragments(&c);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!((f[0].2 - 50.0).abs() < 0.5 && (f[1].2 - 50.0).abs() < 0.5, "{f:?}");
}

#[test]
fn a_grid_that_fits_stays_whole() {
    let c = lay("<div id='g' style='grid-template-rows: 30px 30px'><div style='border: 1px solid'></div><div></div></div>", "");
    let f = fragments(&c);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].3, Some(2));
}
