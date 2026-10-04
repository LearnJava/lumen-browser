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
