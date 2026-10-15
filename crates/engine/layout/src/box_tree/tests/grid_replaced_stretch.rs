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

// ── BUG-1609: нативные контролы без пропорции растягиваются в grid-области ──

/// Область 80 × 50; `markup` — разметка контрола с `id="c"`.
fn control_rect(markup: &str) -> Rect {
    let html = format!(r#"<div id="g">{markup}</div>"#);
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#g{display:grid;grid-template:50px/80px;width:80px;height:50px}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn checkbox_normal_fills_area() {
    let r = control_rect(r#"<input id="c" type="checkbox" style="margin:0">"#);
    assert_eq!((r.width, r.height), (80.0, 50.0), "{r:?}");
}

#[test]
fn radio_and_range_fill_area() {
    let r = control_rect(r#"<input id="c" type="radio" style="margin:0">"#);
    assert_eq!((r.width, r.height), (80.0, 50.0), "radio {r:?}");
    let r = control_rect(r#"<input id="c" type="range" style="margin:0">"#);
    assert_eq!((r.width, r.height), (80.0, 50.0), "range {r:?}");
}

#[test]
fn button_and_text_input_fill_area() {
    let r = control_rect(r#"<button id="c" style="margin:0"></button>"#);
    assert_eq!((r.width, r.height), (80.0, 50.0), "button {r:?}");
    let r = control_rect(r#"<input id="c" style="margin:0">"#);
    assert_eq!((r.width, r.height), (80.0, 50.0), "text {r:?}");
}

#[test]
fn author_size_is_kept() {
    let r = control_rect(r#"<input id="c" type="checkbox" style="margin:0;width:20px;height:10px">"#);
    assert_eq!((r.width, r.height), (22.0, 12.0), "{r:?}");
}

#[test]
fn only_auto_axis_is_stretched() {
    let r = control_rect(r#"<input id="c" type="checkbox" style="margin:0;width:20px">"#);
    assert_eq!((r.width, r.height), (22.0, 50.0), "{r:?}");
}

#[test]
fn start_alignment_keeps_ua_size() {
    let r = control_rect(r#"<input id="c" type="checkbox" style="margin:0;align-self:start;justify-self:start">"#);
    assert_eq!((r.width, r.height), (15.0, 15.0), "{r:?}");
}
