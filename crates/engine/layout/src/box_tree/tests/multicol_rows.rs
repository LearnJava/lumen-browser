//! CSS Multicol L2 §4.2 / §4.4 — rows of column boxes: `column-height` + `column-wrap: wrap`.
//! Numbers come from WPT `css/css-gaps/multicol/multicol-gap-decorations-004` (3 columns of
//! 60px, `column-height: 60px`, `row-gap: 10px`, six 60px items).

use lumen_core::geom::Size;

fn lay(html: &str, css: &str) -> (lumen_dom::Document, crate::box_tree::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (doc, root)
}

const SIX: &str = r#"<div id="c"><p id="a"></p><p id="b"></p><p id="d"></p><p id="e"></p><p id="f"></p><p id="g"></p></div>"#;
const BASE: &str = "body{margin:0} p{height:60px;margin:0} #c{width:200px;column-count:3;column-width:60px;column-gap:10px;row-gap:10px;column-fill:auto;";

fn rect(root: &crate::box_tree::LayoutBox, doc: &lumen_dom::Document, id: &str) -> lumen_core::geom::Rect {
    super::find_by_id_all(root, doc, id).unwrap_or_else(|| panic!("no #{id}")).rect
}

#[test]
fn overflow_columns_wrap_into_a_second_row() {
    let (doc, root) = lay(SIX, &format!("{BASE}column-height:60px;column-wrap:wrap}}"));
    // Row 1: a b d at x = 0, 70, 140; row 2 starts 60 + row-gap 10 below.
    assert_eq!((rect(&root, &doc, "a").x, rect(&root, &doc, "a").y), (0.0, 0.0));
    assert_eq!((rect(&root, &doc, "d").x, rect(&root, &doc, "d").y), (140.0, 0.0));
    assert_eq!((rect(&root, &doc, "e").x, rect(&root, &doc, "e").y), (0.0, 70.0));
    assert_eq!((rect(&root, &doc, "g").x, rect(&root, &doc, "g").y), (140.0, 70.0));
    // Two rows and one row-gap: 60 + 10 + 60.
    assert_eq!(rect(&root, &doc, "c").height, 130.0);
}

#[test]
fn nowrap_keeps_overflow_columns_in_the_inline_direction() {
    let (doc, root) = lay(SIX, &format!("{BASE}column-height:60px;column-wrap:nowrap}}"));
    assert_eq!((rect(&root, &doc, "e").x, rect(&root, &doc, "e").y), (210.0, 0.0));
}

#[test]
fn a_partly_filled_row_keeps_its_column_height() {
    // 4 items → row 2 holds one; the container still spans two full rows.
    let html = r#"<div id="c"><p id="a"></p><p id="b"></p><p id="d"></p><p id="e"></p></div>"#;
    let (doc, root) = lay(html, &format!("{BASE}column-height:60px}}"));
    assert_eq!(rect(&root, &doc, "e").y, 70.0);
    assert_eq!(rect(&root, &doc, "c").height, 130.0);
}

#[test]
fn without_a_column_height_nothing_wraps() {
    let (doc, root) = lay(SIX, &format!("{BASE}height:60px;column-wrap:wrap}}"));
    assert_eq!(rect(&root, &doc, "e").y, 0.0, "no column-height: overflow stays in the inline direction");
}

#[test]
fn a_spanner_splits_the_first_row_and_the_rest_fills_what_is_left() {
    // WPT multicol-gap-decorations-002: 3 columns × 60px, row-gap 10px, `p`×2 then an 18px
    // spanner then six more `p`. The columns before the spanner balance (2 × 60px over 3
    // columns → 40px), the spanner follows, the rest of the row (60 − 40 − 18 = 2px) holds the
    // start of the next block, then the second row begins one row-gap below.
    let html = r#"<div id="c"><p id="a"></p><p id="b"></p><h2 id="s"></h2><p id="d"></p><p id="e"></p><p id="f"></p><p id="g"></p><p id="h"></p><p id="i"></p></div>"#;
    let css = format!("{BASE}height:200px;column-height:60px;column-wrap:wrap}} h2{{column-span:all;height:18px;margin:0}}");
    let (doc, root) = lay(html, &css);
    assert_eq!((rect(&root, &doc, "a").x, rect(&root, &doc, "a").y), (0.0, 0.0));
    assert_eq!(rect(&root, &doc, "a").height, 40.0, "balanced to the 40px the columns need");
    // 120px of content over three 40px columns: `b` starts 20px into the second one.
    assert_eq!((rect(&root, &doc, "b").x, rect(&root, &doc, "b").y), (70.0, 20.0));
    let s = rect(&root, &doc, "s");
    assert_eq!((s.x, s.y, s.width, s.height), (0.0, 40.0, 200.0, 18.0));
}

#[test]
fn a_single_column_is_sliced_into_rows_by_its_column_height() {
    // WPT multicol-gap-decorations-021: `columns: 1 / 20px`, a 70px block. One column per row,
    // so the block is cut into four rows of 20px (the last one keeps the full column height) a
    // row-gap (10px) apart: 4 × 20 + 3 × 10.
    let html = r#"<div id="c"><p id="a"></p></div>"#;
    let css = "body{margin:0} p{height:70px;margin:0} #c{width:100px;columns:1/20px;gap:10px}";
    let (doc, root) = lay(html, css);
    assert_eq!(rect(&root, &doc, "c").height, 110.0);
}
