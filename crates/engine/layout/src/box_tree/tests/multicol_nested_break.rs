//! CSS Fragmentation L3 §4.1 — a forced column break inside a child block of a multicol
//! container cuts that block (WPT `css-multicol/column-height-024`, `spanner-fragmentation-*`).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn leaves(b: &LayoutBox, out: &mut Vec<(f32, f32, f32)>) {
    if matches!(b.kind, BoxKind::Block) && b.children.iter().all(|c| matches!(c.kind, BoxKind::Skip)) && b.rect.height > 0.0 {
        out.push((b.rect.x, b.rect.y, b.rect.height));
    }
    for c in &b.children {
        leaves(c, out);
    }
}

/// `(x, y, height)` of every non-empty leaf block, document order.
fn leaf_boxes(html: &str, css: &str) -> Vec<(f32, f32, f32)> {
    let root = lay(html, css);
    let mut v = Vec::new();
    leaves(&root, &mut v);
    v
}

const CSS: &str = "body{margin:0} #c{columns:3;column-gap:0;width:300px;column-fill:auto;height:100px} \
                   .l{height:20px;background:green}";

#[test]
fn break_before_on_a_grandchild_opens_a_new_column() {
    let v = leaf_boxes(
        "<div id=c><div><div class=l></div><div class=l style=\"break-before:column\"></div></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 20.0), (100.0, 0.0, 20.0)], "{v:?}");
}

#[test]
fn break_after_on_a_grandchild_opens_a_new_column() {
    let v = leaf_boxes(
        "<div id=c><div><div class=l style=\"break-after:column\"></div><div class=l></div></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 20.0), (100.0, 0.0, 20.0)], "{v:?}");
}

#[test]
fn breaks_two_levels_down_cut_every_wrapper() {
    let v = leaf_boxes(
        "<div id=c><div><div><div class=l></div><div class=l style=\"break-before:column\"></div></div>\
         <div><div class=l style=\"break-before:column\"></div></div></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 20.0), (100.0, 0.0, 20.0), (200.0, 0.0, 20.0)], "{v:?}");
}

#[test]
fn a_break_before_the_first_child_adds_no_empty_column() {
    let v = leaf_boxes(
        "<div id=c><div><div class=l style=\"break-before:column\"></div><div class=l></div></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 20.0), (0.0, 20.0, 20.0)], "{v:?}");
}

#[test]
fn a_break_between_wrappers_is_taken_from_the_edge_children() {
    // `break-before` of the second wrapper's first child is a break before the wrapper.
    let v = leaf_boxes(
        "<div id=c><div><div class=l></div></div><div><div class=l style=\"break-before:column\"></div></div></div>",
        CSS,
    );
    assert_eq!(v, [(0.0, 0.0, 20.0), (100.0, 0.0, 20.0)], "{v:?}");
}

#[test]
fn fragments_follow_box_decoration_break_slice() {
    // The wrapper's 5px top padding stays on the first fragment, the bottom one on the last:
    // 5 + 20 + 20 + 5 stacks across two columns as 25 and 25, not 30 and 30.
    let root = lay(
        "<div id=c><div style=\"padding:5px 0\"><div class=l></div>\
         <div class=l style=\"break-before:column\"></div></div></div>",
        CSS,
    );
    let mut v = Vec::new();
    leaves(&root, &mut v);
    assert_eq!(v, [(0.0, 5.0, 20.0), (100.0, 0.0, 20.0)], "{v:?}");
}

const NESTED_CSS: &str = "body{margin:0} #o{columns:3;column-gap:10px;column-fill:auto;width:620px;height:100px} \
                          #n{columns:2;column-gap:10px;padding:10px} .t{height:250px;background:hotpink}";

#[test]
fn nested_multicol_continues_in_the_next_outer_column_with_box_decoration_clone() {
    // WPT `css-gaps/multicol/multicol-gap-decorations-006`: 100px outer columns, a nested container
    // of 250px of content. Fragment 1 keeps its padding on both edges, so its two inner columns
    // are 100 - 10 - 10 = 80px; the remaining 90px are balanced in fragment 2 (45 + 45).
    let v = leaf_boxes(
        "<div id=o><div id=n style=\"box-decoration-break:clone\"><div class=t></div></div></div>",
        NESTED_CSS,
    );
    assert_eq!(v, [(10.0, 10.0, 80.0), (105.0, 10.0, 80.0), (220.0, 10.0, 45.0), (315.0, 10.0, 45.0)], "{v:?}");
}

#[test]
fn nested_multicol_slices_the_edges_a_break_cuts() {
    // `box-decoration-break: slice` (the default): fragment 1 has no bottom padding (90px columns),
    // fragment 2 no top padding; the last 70px are balanced (35 + 35).
    let v = leaf_boxes("<div id=o><div id=n><div class=t></div></div></div>", NESTED_CSS);
    assert_eq!(v, [(10.0, 10.0, 90.0), (105.0, 10.0, 90.0), (220.0, 0.0, 35.0), (315.0, 0.0, 35.0)], "{v:?}");
}

#[test]
fn nested_multicol_that_fits_the_outer_column_stays_whole() {
    let v = leaf_boxes(
        "<div id=o><div id=n><div class=t style=\"height:60px\"></div></div></div>",
        NESTED_CSS,
    );
    assert_eq!(v, [(10.0, 10.0, 30.0), (105.0, 10.0, 30.0)], "{v:?}");
}
