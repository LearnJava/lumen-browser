use lumen_core::geom::Rect;
use lumen_core::geom::Size;

// BUG-1669: HTML Rendering §15.3.3 — `<center>` (`text-align: -webkit-center`) центрит и
// блочных потомков с ограниченной шириной, чего plain `text-align: center` не делает.

fn rects(html: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const TABLE: &str = r#"<table id="t" style="width:400px;height:10px"><tr><td>x</td></tr></table>"#;

#[test]
fn center_element_centers_block_child() {
    let html = format!("<center>{TABLE}</center>");
    assert_eq!(rects(&html, &["t"])[0].x, 200.0);
}

#[test]
fn div_align_center_centers_block_child() {
    let html = format!(r#"<div align="center">{TABLE}</div>"#);
    assert_eq!(rects(&html, &["t"])[0].x, 200.0);
}

#[test]
fn plain_text_align_center_leaves_block_at_start() {
    let html = format!(r#"<div style="text-align:center">{TABLE}</div>"#);
    assert_eq!(rects(&html, &["t"])[0].x, 0.0);
}

#[test]
fn inherited_webkit_center_reaches_nested_blocks() {
    let html = format!(r#"<center><div style="width:600px;margin-left:100px">{TABLE}</div></center>"#);
    // Внутренний блок — margin box 700px центрится в 800px (сдвиг 50px), поэтому его
    // border-start на 150px; таблица центрится уже в его 600px: 150 + 100.
    assert_eq!(rects(&html, &["t"])[0].x, 250.0);
}

#[test]
fn rtl_center_element_centers_block_child() {
    let html = format!(r#"<center dir="rtl">{TABLE}</center>"#);
    assert_eq!(rects(&html, &["t"])[0].x, 200.0);
}

#[test]
fn explicit_justify_self_start_wins() {
    let html = format!(r#"<center>{TABLE}</center>"#)
        .replace(r#"style="width:400px"#, r#"style="justify-self:start;width:400px"#);
    assert_eq!(rects(&html, &["t"])[0].x, 0.0);
}
