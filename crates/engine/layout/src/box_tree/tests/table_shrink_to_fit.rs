use lumen_core::geom::{Rect, Size};

// ── таблица без `width` сжимается по max-content колонок (CSS 2.1 §17.5.2, BUG-1335) ──

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn rect_of(html: &str, id: &str) -> Rect {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}table{border-spacing:0}td{padding:0}");
    let root = super::super::layout_measured(&doc, &sheet, Size::new(1024.0, 600.0), &Measurer);
    super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect
}

#[test]
fn table_with_text_cell_shrinks_to_the_text() {
    let r = rect_of(r#"<table id="t"><tr><td>ab</td></tr></table>"#, "t");
    assert!(r.width > 0.0 && r.width < 100.0, "{r:?}");
}

#[test]
fn display_table_with_block_of_fixed_width_takes_that_width() {
    let html = r#"<div id="t" style="display:table"><div style="display:table-row"><div style="display:table-cell"><div style="width:50px;height:10px"></div></div></div></div>"#;
    assert_eq!(rect_of(html, "t").width, 50.0);
}

#[test]
fn explicit_and_auto_columns_add_up() {
    let html = r#"<table id="t"><tr><td style="width:40px"></td><td><div style="width:30px;height:5px"></div></td></tr></table>"#;
    assert_eq!(rect_of(html, "t").width, 70.0);
}

// BUG-1341: inline-block с пустыми блочными детьми сжимается до своих рамок.
#[test]
fn inline_block_with_empty_block_child_shrinks_to_border() {
    let r = rect_of(
        "<div id=a style='display:inline-block;border-right:3px solid green'><div style='height:20px'></div></div>",
        "a",
    );
    assert!((r.width - 3.0).abs() < 0.01, "ширина {}", r.width);
}

#[test]
fn inline_block_with_fixed_width_block_child_unchanged() {
    let r = rect_of(
        "<div id=a style='display:inline-block;border-right:3px solid green'><div style='height:20px;width:30px'></div></div>",
        "a",
    );
    assert!((r.width - 33.0).abs() < 0.01, "ширина {}", r.width);
}
