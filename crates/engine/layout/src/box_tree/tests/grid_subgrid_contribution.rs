//! CSS Grid L1 §11 / L2 §9 (BUG-1318): `auto` columns are sized from their items,
//! and a subgrid's own items contribute to the parent's tracks, each at its own
//! span in the parent's coordinates — the same content directly in the parent
//! gives the reference result.

use lumen_core::geom::Size;

use super::super::{layout, LayoutBox};
use crate::style::Display;

fn find_grid(b: &LayoutBox, display: Display) -> Option<&LayoutBox> {
    if b.style.display == display {
        return Some(b);
    }
    b.children.iter().find_map(|c| find_grid(c, display))
}

fn root(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    layout(&doc, &sheet, Size::new(800.0, 600.0))
}

/// `x` of the non-skipped children of `b`.
fn xs(b: &LayoutBox) -> Vec<f32> {
    b.children.iter().map(|c| c.rect.x).collect()
}

const CSS: &str = ".o{display:inline-grid;grid-template-columns:auto auto} \
    .s{grid-column:1/3;display:grid;grid-template-columns:subgrid} \
    .a,.b{width:30px;height:10px} \
    .p .a,.p .b{padding:0 20px}";

#[test]
fn control_items_directly_in_parent() {
    let r = root("<div class=o><div class=a></div><div class=b></div></div>", CSS);
    let g = find_grid(&r, Display::InlineGrid).unwrap();
    assert_eq!(g.rect.width, 60.0);
}

#[test]
fn subgrid_items_widen_the_parents_auto_tracks() {
    let r = root("<div class=o><div class=s><div class=a></div><div class=b></div></div></div>", CSS);
    let g = find_grid(&r, Display::InlineGrid).unwrap();
    assert_eq!(g.rect.width, 60.0);
    let sub = &g.children[0];
    assert_eq!(xs(sub), vec![g.rect.x, g.rect.x + 30.0]);
}

#[test]
fn subgrid_item_padding_counts_in_the_parents_track() {
    let r = root("<div class='o p'><div class=s><div class=a></div><div class=b></div></div></div>", CSS);
    let g = find_grid(&r, Display::InlineGrid).unwrap();
    assert_eq!(g.rect.width, 140.0);
    let sub = &g.children[0];
    assert_eq!(xs(sub), vec![g.rect.x, g.rect.x + 70.0]);
}

#[test]
fn subgrid_own_padding_is_added_to_its_edge_items() {
    // The subgrid's padding (10 + 6) sits inside the parent's first and last track.
    let r = root(
        "<div class=o><div class=s style='padding:0 10px 0 6px'><div class=a></div><div class=b></div></div></div>",
        CSS,
    );
    let g = find_grid(&r, Display::InlineGrid).unwrap();
    // first track: 6 + 30, second: 30 + 10
    assert_eq!(g.rect.width, 76.0);
}

#[test]
fn unequal_auto_columns_follow_their_content() {
    // 300px, items 30 and 100: both reach their max-content, the rest (170) is shared equally.
    let r = root(
        "<div class=g><div style='width:30px;height:10px'></div><div style='width:100px;height:10px'></div></div>",
        ".g{display:grid;width:300px;grid-template-columns:auto auto}",
    );
    let g = find_grid(&r, Display::Grid).unwrap();
    assert_eq!(xs(g), vec![g.rect.x, g.rect.x + 115.0]);
}

#[test]
fn subgrid_unequal_children_size_the_parent_like_direct_children() {
    let css = ".o{display:inline-grid;grid-template-columns:auto auto} \
        .s{grid-column:1/3;display:grid;grid-template-columns:subgrid} \
        .a{width:30px;height:10px}.b{width:100px;height:10px}";
    let r = root("<div class=o><div class=s><div class=a></div><div class=b></div></div></div>", css);
    let g = find_grid(&r, Display::InlineGrid).unwrap();
    assert_eq!(g.rect.width, 130.0);
    assert_eq!(xs(&g.children[0]), vec![g.rect.x, g.rect.x + 30.0]);
}

#[test]
fn spanning_item_gets_what_the_tracks_lack() {
    // A 100px item over two auto tracks that hold 30 and 20: 50 more, split equally.
    let css = ".o{display:inline-grid;grid-template-columns:auto auto} \
        .w{grid-column:1/3;width:100px;height:10px} \
        .a{width:30px;height:10px}.b{width:20px;height:10px}";
    let r = root("<div class=o><div class=w></div><div class=a></div><div class=b></div></div>", css);
    let g = find_grid(&r, Display::InlineGrid).unwrap();
    assert_eq!(g.rect.width, 100.0);
    // second row: a in track 0 (30 + 25), b in track 1 starts after it.
    assert_eq!(g.children[2].rect.x - g.rect.x, 55.0);
}
