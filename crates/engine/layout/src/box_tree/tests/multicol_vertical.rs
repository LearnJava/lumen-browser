//! CSS Multicol L1 in a vertical writing mode (MULTICOL-VERTICAL, `box_tree::multicol_vertical`):
//! columns run down the inline axis, each is as long as the container is wide, and the content of
//! the first column starts at the block-start edge (right for `vertical-rl`).
//!
//! Boxes with explicit sizes only: the numbers are computed by hand, no font is needed.

use lumen_core::geom::{Rect, Size};

fn lay(html: &str, css: &str) -> (lumen_dom::Document, crate::box_tree::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (doc, root)
}

fn rect_of(doc: &lumen_dom::Document, root: &crate::box_tree::LayoutBox, id: &str) -> Rect {
    super::find_by_id_all(root, doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect
}

/// Rects of the children of `#c` (the fragments), in tree order.
fn kids(doc: &lumen_dom::Document, root: &crate::box_tree::LayoutBox) -> Vec<Rect> {
    super::find_by_id_all(root, doc, "c").expect("нет #c").children.iter().map(|c| c.rect).collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

fn assert_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) {
    assert!(near(r.x, x) && near(r.y, y) && near(r.width, w) && near(r.height, h), "{r:?} вместо ({x}, {y}, {w}, {h})");
}

const RL: &str = "#c{writing-mode:vertical-rl;width:60px;height:100px;columns:2;column-gap:0;column-fill:auto}\
                  .b{width:30px}";

/// Two blocks fill the first column along the block axis (right to left), the third opens the
/// second column below it.
#[test]
fn vertical_rl_columns_fill_from_the_right_and_open_downwards() {
    let (doc, root) = lay(
        "<div id='c'><div class='b' id='a'></div><div class='b' id='b'></div><div class='b' id='d'></div></div>",
        RL,
    );
    let k = kids(&doc, &root);
    assert_eq!(k.len(), 3, "{k:?}");
    assert_rect(rect_of(&doc, &root, "c"), 0.0, 0.0, 60.0, 100.0);
    assert_rect(k[0], 30.0, 0.0, 30.0, 50.0);
    assert_rect(k[1], 0.0, 0.0, 30.0, 50.0);
    assert_rect(k[2], 30.0, 50.0, 30.0, 50.0);
}

#[test]
fn vertical_lr_columns_fill_from_the_left() {
    let css = RL.replace("vertical-rl", "vertical-lr");
    let (doc, root) = lay(
        "<div id='c'><div class='b' id='a'></div><div class='b' id='b'></div><div class='b' id='d'></div></div>",
        &css,
    );
    let k = kids(&doc, &root);
    assert_eq!(k.len(), 3, "{k:?}");
    assert_rect(k[0], 0.0, 0.0, 30.0, 50.0);
    assert_rect(k[1], 30.0, 0.0, 30.0, 50.0);
    assert_rect(k[2], 0.0, 50.0, 30.0, 50.0);
}

/// The gap sits between the columns along the inline axis.
#[test]
fn the_column_gap_is_measured_along_the_inline_axis() {
    let css = RL.replace("column-gap:0", "column-gap:20px");
    let (doc, root) = lay(
        "<div id='c'><div class='b' id='a'></div><div class='b' id='b'></div><div class='b' id='d'></div></div>",
        &css,
    );
    let k = kids(&doc, &root);
    assert_eq!(k.len(), 3, "{k:?}");
    // Columns are (100 - 20) / 2 = 40px long.
    assert_rect(k[0], 30.0, 0.0, 30.0, 40.0);
    assert_rect(k[2], 30.0, 60.0, 30.0, 40.0);
}

/// `column-fill: balance` with an auto block size: the container is as wide as the balanced
/// columns need — four 20px blocks over two columns make it 40px.
#[test]
fn an_auto_block_size_container_wraps_balanced_columns() {
    let (doc, root) = lay(
        "<div id='c'><div class='b' id='a'></div><div class='b' id='b'></div><div class='b' id='d'></div>\
         <div class='b' id='e'></div></div>",
        "#c{writing-mode:vertical-rl;height:100px;columns:2;column-gap:0}.b{width:20px}",
    );
    assert_rect(rect_of(&doc, &root, "c"), 0.0, 0.0, 40.0, 100.0);
    let k = kids(&doc, &root);
    assert_eq!(k.len(), 4, "{k:?}");
    assert_rect(k[0], 20.0, 0.0, 20.0, 50.0);
    assert_rect(k[1], 0.0, 0.0, 20.0, 50.0);
    assert_rect(k[2], 20.0, 50.0, 20.0, 50.0);
    assert_rect(k[3], 0.0, 50.0, 20.0, 50.0);
}

/// Fragmentation L3 §3.1: `break-before: column` starts a column; with `column-fill: auto` and no
/// block size the container is as wide as its widest column (WPT `flex-align-baseline-multicol-002`).
#[test]
fn a_forced_break_starts_a_column_and_the_widest_one_sizes_the_container() {
    let (doc, root) = lay(
        "<div id='c'><div id='a' style='width:10px'></div><div id='b' style='width:30px;break-before:column'></div>\
         <div id='d' style='width:20px;break-before:column'></div></div>",
        "#c{writing-mode:vertical-rl;height:90px;columns:3;column-gap:0;column-fill:auto}",
    );
    assert_rect(rect_of(&doc, &root, "c"), 0.0, 0.0, 30.0, 90.0);
    assert_rect(rect_of(&doc, &root, "a"), 20.0, 0.0, 10.0, 30.0);
    assert_rect(rect_of(&doc, &root, "b"), 0.0, 30.0, 30.0, 30.0);
    assert_rect(rect_of(&doc, &root, "d"), 10.0, 60.0, 20.0, 30.0);
}

/// Multicol L1 §6.1: a `column-span: all` block is as long as the whole container (WPT
/// `multicol-span-auto-size-in-vertical-writing-mode-001`) and takes its place in the block axis.
#[test]
fn a_column_spanner_spans_the_inline_axis() {
    let (doc, root) = lay(
        "<div id='c'><div id='s'></div><div id='f'></div></div>",
        "#c{writing-mode:vertical-rl;width:50px;height:100px;columns:2;column-gap:0;column-fill:auto}\
         #s{column-span:all;block-size:50px}#f{width:10px;height:10px}",
    );
    assert_rect(rect_of(&doc, &root, "c"), 0.0, 0.0, 50.0, 100.0);
    assert_rect(rect_of(&doc, &root, "s"), 0.0, 0.0, 50.0, 100.0);
}

/// The same with the spanner inside a wrapper (`-002`): the wrapper is split around it.
#[test]
fn a_spanner_in_a_wrapper_still_spans() {
    let (doc, root) = lay(
        "<div id='c'><div><div id='s'></div><div id='f'></div></div></div>",
        "#c{writing-mode:vertical-lr;width:50px;height:100px;columns:2;column-gap:0;column-fill:auto}\
         #s{column-span:all;block-size:50px}#f{width:10px;height:10px}",
    );
    assert_rect(rect_of(&doc, &root, "s"), 0.0, 0.0, 50.0, 100.0);
}

/// Content after a spanner starts below (to the left of) it, in fresh columns.
#[test]
fn columns_after_a_spanner_start_past_its_block_extent() {
    let (doc, root) = lay(
        "<div id='c'><div id='s' style='width:20px'></div><div id='a' style='width:10px'></div></div>",
        "#c{writing-mode:vertical-lr;width:50px;height:100px;columns:2;column-gap:0;column-fill:auto}\
         #s{column-span:all}",
    );
    assert_rect(rect_of(&doc, &root, "s"), 0.0, 0.0, 20.0, 100.0);
    assert_rect(rect_of(&doc, &root, "a"), 20.0, 0.0, 10.0, 50.0);
}

/// Padding and border of the container surround the columns on all four physical sides.
#[test]
fn the_container_frame_surrounds_the_columns() {
    let (doc, root) = lay(
        "<div id='c'><div class='b' id='a'></div><div class='b' id='b'></div><div class='b' id='d'></div></div>",
        "#c{writing-mode:vertical-rl;width:60px;height:100px;box-sizing:content-box;padding:5px;border:3px solid;\
         columns:2;column-gap:0;column-fill:auto}.b{width:30px}",
    );
    assert_rect(rect_of(&doc, &root, "c"), 0.0, 0.0, 76.0, 116.0);
    let k = kids(&doc, &root);
    assert_eq!(k.len(), 3, "{k:?}");
    // content box: x 8..68, y 8..108; first column is y 8..58.
    assert_rect(k[0], 38.0, 8.0, 30.0, 50.0);
    assert_rect(k[1], 8.0, 8.0, 30.0, 50.0);
    assert_rect(k[2], 38.0, 58.0, 30.0, 50.0);
}
