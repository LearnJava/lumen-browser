//! CSS Multicol L1 §7.1 / Fragmentation L3 §3.1 — a segment that mixes a leaf cut by the
//! column windows with a leaf that carries a forced break: the forced break opens a column, the
//! leaf behind it is still cut where it does not fit (WPT `css-multicol/column-height-024`).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn leaves(b: &LayoutBox, out: &mut Vec<(f32, f32, f32, f32)>) {
    if matches!(b.kind, BoxKind::Block) && b.children.iter().all(|c| matches!(c.kind, BoxKind::Skip)) && b.rect.height > 0.0 {
        out.push((b.rect.x, b.rect.y, b.rect.width, b.rect.height));
    }
    for c in &b.children {
        leaves(c, out);
    }
}

/// `(x, y, width, height)` of every non-empty leaf block, document order.
fn leaf_boxes(html: &str, css: &str) -> Vec<(f32, f32, f32, f32)> {
    let root = lay(html, css);
    let mut v = Vec::new();
    leaves(&root, &mut v);
    v
}

const CSS: &str = "body{margin:0} #c{columns:2;column-gap:0;width:200px;column-fill:auto;height:100px} \
                   .a{height:30px;background:green} .b{break-before:column;height:250px;background:blue}";

#[test]
fn a_leaf_behind_a_forced_break_is_still_cut_by_the_columns() {
    let v = leaf_boxes("<div id=c><div class=a></div><div class=b></div></div>", CSS);
    // `.b` opens the second column and spills over into two overflow columns.
    assert_eq!(
        v,
        [(0.0, 0.0, 100.0, 30.0), (100.0, 0.0, 100.0, 100.0), (200.0, 0.0, 100.0, 100.0), (300.0, 0.0, 100.0, 50.0)],
        "{v:?}"
    );
}

#[test]
fn a_forced_break_inside_the_column_does_not_waste_the_rest_of_it() {
    // Fits the column: the forced break only moves the leaf to the next column.
    let v = leaf_boxes(
        "<div id=c><div class=a></div><div class=a style=\"break-before:column\"></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 100.0, 30.0), (100.0, 0.0, 100.0, 30.0)], "{v:?}");
}

#[test]
fn balanced_columns_keep_the_forced_break() {
    let v = leaf_boxes(
        "<div id=c style=\"column-fill:balance;height:auto\"><div class=a></div>\
         <div class=a style=\"break-before:column\"></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 100.0, 30.0), (100.0, 0.0, 100.0, 30.0)], "{v:?}");
}

#[test]
fn column_height_rows_cut_the_leaf_behind_the_last_break() {
    // column-height-024: 1px leaves one per column, then a 40px leaf that needs two columns.
    let mut html = String::from("<div id=c><div>");
    for _ in 0..3 {
        html.push_str("<div class=a style=\"break-before:column\"></div>");
    }
    html.push_str("<div class=b style=\"break-before:column\"></div></div></div>");
    let css = "body{margin:0} #c{columns:2;column-gap:0;width:200px;column-height:20px;column-wrap:wrap;row-gap:5px} \
               .a{height:1px;background:green} .b{height:40px;background:white}";
    let v = leaf_boxes(&html, css);
    // Rows of 20px, 5px apart: (a a) (a b-top) (b-bottom).
    assert_eq!(v.len(), 5, "{v:?}");
    assert_eq!(v[3], (100.0, 25.0, 100.0, 20.0), "{v:?}");
    assert_eq!(v[4], (0.0, 50.0, 100.0, 20.0), "{v:?}");
}
