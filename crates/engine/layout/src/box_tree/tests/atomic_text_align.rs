use lumen_core::geom::{Rect, Size};

// BUG-1356: `text-align` сдвигает строку целиком, включая atomic inline.

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}#c{{width:500px}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn ib(id: &str, w: u32) -> String {
    format!(r#"<span id="{id}" style="display:inline-block;width:{w}px;height:10px"></span>"#)
}

#[test]
fn right_aligns_single_inline_block() {
    let html = format!(r#"<div id="c" style="text-align:right">{}</div>"#, ib("a", 40));
    assert_eq!(rects(&html, "", &["a"])[0].x, 460.0);
}

#[test]
fn right_aligns_run_of_inline_blocks_as_one_line() {
    let html = format!(r#"<div id="c" style="text-align:right">{}{}</div>"#, ib("a", 5), ib("b", 5));
    let r = rects(&html, "", &["a", "b"]);
    assert_eq!((r[0].x, r[1].x), (490.0, 495.0), "{r:?}");
}

#[test]
fn center_counts_inline_block_width() {
    let html = format!(r#"<div id="c" style="text-align:center">{}{}</div>"#, ib("a", 100), ib("b", 100));
    let r = rects(&html, "", &["a", "b"]);
    assert_eq!((r[0].x, r[1].x), (150.0, 250.0), "{r:?}");
}

#[test]
fn left_is_unchanged() {
    let html = format!(r#"<div id="c">{}</div>"#, ib("a", 40));
    assert_eq!(rects(&html, "", &["a"])[0].x, 0.0);
}
