//! CSS Grid L2 §9: the items of a row `subgrid` ask the parent's row tracks for their own height
//! (plus the extra margin of the subgrid's gap), not the whole subgrid for its first row —
//! `css-gaps/grid/subgrid/fragmentation/subgrid-gap-decorations-fragmentation-020`.

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

/// `(y, height)` of the children of `b`.
fn ys(b: &LayoutBox) -> Vec<(f32, f32)> {
    b.children.iter().map(|c| (c.rect.y, c.rect.height)).collect()
}

const FR: &str = "body{margin:0} \
    .g{display:grid;grid-template-columns:100px;grid-template-rows:repeat(5,1fr);gap:10px;height:140px;width:100px} \
    .s{display:grid;grid-template-columns:subgrid;grid-template-rows:subgrid;grid-row:2/-2} \
    .i{background:purple}";

#[test]
fn a_larger_subgrid_gap_takes_its_extra_margin_out_of_the_fr_rows() {
    let r = root(
        "<div class=g><div class=s><div class=i></div><div class=i></div><div class=i></div></div></div>",
        &format!("{FR} .s{{row-gap:20px}}"),
    );
    let g = find_grid(&r, Display::Grid).unwrap();
    let sub = &g.children[0];
    // Rows stay 20px: the items shrink by the 5/10/5px of extra margin, the grid keeps 140px.
    assert_eq!(g.rect.height, 140.0);
    assert_eq!(ys(sub), vec![(30.0, 15.0), (65.0, 10.0), (95.0, 15.0)]);
}

#[test]
fn a_subgrid_gap_equal_to_the_parents_changes_nothing() {
    let r = root(
        "<div class=g><div class=s><div class=i></div><div class=i></div><div class=i></div></div></div>",
        &format!("{FR} .s{{row-gap:10px}}"),
    );
    let g = find_grid(&r, Display::Grid).unwrap();
    assert_eq!(ys(&g.children[0]), vec![(30.0, 20.0), (60.0, 20.0), (90.0, 20.0)]);
}

#[test]
fn each_auto_row_takes_the_height_of_its_own_subgrid_item() {
    let r = root(
        "<div class=g><div class=s><div class=i style='height:10px'></div><div class=i style='height:30px'></div></div></div>",
        "body{margin:0} .g{display:grid;grid-template-columns:100px;grid-template-rows:auto auto auto} \
         .s{display:grid;grid-template-columns:subgrid;grid-template-rows:subgrid;grid-row:1/3}",
    );
    let g = find_grid(&r, Display::Grid).unwrap();
    assert_eq!(ys(&g.children[0]), vec![(0.0, 10.0), (10.0, 30.0)]);
    assert_eq!(g.rect.height, 40.0);
}

#[test]
fn a_subgrid_gap_adds_to_the_auto_rows_around_it() {
    let r = root(
        "<div class=g><div class=s><div class=i style='height:10px'></div><div class=i style='height:30px'></div></div></div>",
        "body{margin:0} .g{display:grid;grid-template-columns:100px;grid-template-rows:auto auto} \
         .s{display:grid;grid-template-columns:subgrid;grid-template-rows:subgrid;grid-row:1/3;row-gap:20px}",
    );
    let g = find_grid(&r, Display::Grid).unwrap();
    // Each item carries 10px of extra margin next to the 20px gutter.
    assert_eq!(ys(&g.children[0]), vec![(0.0, 10.0), (30.0, 30.0)]);
    assert_eq!(g.rect.height, 60.0);
}
