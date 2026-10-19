//! BUG-1291 — `position: relative; top/bottom` did nothing on atomic inlines in an
//! `InlineBlockRow`: the row's vertical-align pass re-placed the already-shifted
//! child on its line and cancelled the offset (`left` survived, but leaked into the
//! flow of the following siblings).

use lumen_core::geom::{Rect, Size};

use crate::box_tree::LayoutBox;

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// Border-box rects of boxes whose width is one of `widths` (unique per test), in
/// document order.
fn rects(body: &str, widths: &[f32]) -> Vec<Rect> {
    let html = format!("<html><body style=\"margin:0\">{body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn walk(b: &LayoutBox, widths: &[f32], out: &mut Vec<Rect>) {
        if widths.iter().any(|w| (b.rect.width - w).abs() < 0.01) {
            out.push(b.rect);
        }
        b.children.iter().for_each(|c| walk(c, widths, out));
    }
    let mut out = Vec::new();
    walk(&tree, widths, &mut out);
    out
}

fn cell(w: u32, extra: &str) -> String {
    format!("<span style=\"display:inline-block;width:{w}px;height:20px;{extra}\"></span>")
}

#[test]
fn top_shifts_inline_block_vertically() {
    let plain = rects(&cell(61, ""), &[61.0]);
    let shifted = rects(&cell(61, "position:relative;top:50px;left:100px"), &[61.0]);
    assert_eq!(shifted[0].x, plain[0].x + 100.0);
    assert_eq!(shifted[0].y, plain[0].y + 50.0);
}

#[test]
fn bottom_shifts_inline_block_up() {
    let plain = rects(&cell(62, ""), &[62.0]);
    let shifted = rects(&cell(62, "position:relative;bottom:7px"), &[62.0]);
    assert_eq!(shifted[0].y, plain[0].y - 7.0);
}

#[test]
fn offset_does_not_move_neighbours() {
    let body = format!("{}{}", cell(63, "position:relative;top:30px;left:20px"), cell(64, ""));
    let plain = rects(&format!("{}{}", cell(63, ""), cell(64, "")), &[63.0, 64.0]);
    let r = rects(&body, &[63.0, 64.0]);
    assert_eq!(r[1], plain[1], "the following sibling must flow from the unshifted box");
    assert_eq!(r[0].y, plain[0].y + 30.0);
}

#[test]
fn offset_does_not_change_row_height() {
    let tall = rects(&format!("<div style=\"width:123px\">{}</div>", cell(65, "position:relative;top:40px")), &[123.0]);
    let flat = rects("<div style=\"width:123px\"><span style=\"display:inline-block;width:65px;height:20px\"></span></div>", &[123.0]);
    assert_eq!(tall[0].height, flat[0].height);
}
