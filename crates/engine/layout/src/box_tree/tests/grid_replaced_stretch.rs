use lumen_core::geom::{Rect, Size};

// ── BUG-1311: `stretch` у replaced-item'а в grid-области ──

fn rect_of(item_style: &str) -> Rect {
    let html = format!(
        r#"<div id="g"><canvas id="c" width="10" height="10" style="{item_style}"></canvas></div>"#
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#g{display:grid;grid-template:100px/200px;width:200px;height:100px}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn align_stretch_fills_block_axis_and_keeps_ratio() {
    let r = rect_of("align-self:stretch");
    assert_eq!((r.width, r.height), (100.0, 100.0), "{r:?}");
}

#[test]
fn justify_stretch_fills_inline_axis_and_derives_height() {
    let r = rect_of("justify-self:stretch");
    assert_eq!((r.width, r.height), (200.0, 200.0), "{r:?}");
}

#[test]
fn both_stretch_fill_the_area() {
    let r = rect_of("justify-self:stretch;align-self:stretch");
    assert_eq!((r.width, r.height), (200.0, 100.0), "{r:?}");
}

#[test]
fn normal_keeps_intrinsic_size() {
    let r = rect_of("");
    assert_eq!((r.width, r.height), (10.0, 10.0), "{r:?}");
}
