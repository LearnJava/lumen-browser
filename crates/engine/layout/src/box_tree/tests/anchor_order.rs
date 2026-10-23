//! BUG-1483: an anchor is visible to a positioned element only if it precedes the element in
//! tree order (CSS Anchor Positioning L1 §2.2); of several of one name, the last preceding wins.

use lumen_core::geom::Size;

use super::super::{layout_measured, LayoutBox};

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

const CSS: &str = ".a{anchor-name:--a;height:10px;width:20px}\
    .t{position:absolute;width:anchor-size(--a width);height:10px}\
    .r{position:relative}";

fn target_width(html: &str) -> f32 {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{CSS}"));
    let root: LayoutBox = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Measurer);
    super::find_by_id_all(&root, &doc, "t").expect("нет #t").rect.width
}

#[test]
fn anchor_after_target_is_invisible() {
    assert_eq!(target_width(r#"<div class=r><div id=t class=t></div><div class=a></div></div>"#), 0.0);
}

#[test]
fn anchor_in_later_relative_is_invisible() {
    assert_eq!(
        target_width(r#"<div class=r><div id=t class=t></div><div class=r><div class=a></div></div></div>"#),
        0.0
    );
}

#[test]
fn anchor_in_outer_block_after_nested_target_is_invisible() {
    assert_eq!(
        target_width(r#"<div class=r><div class=r><div id=t class=t></div></div><div class=a></div></div>"#),
        0.0
    );
}

#[test]
fn anchor_before_target_is_used() {
    assert_eq!(target_width(r#"<div class=r><div class=a></div><div id=t class=t></div></div>"#), 20.0);
}

#[test]
fn last_preceding_anchor_wins_over_a_later_one() {
    let html = r#"<div class=r><div class=a style="width:20px"></div><div class=a style="width:30px"></div>
        <div id=t class=t></div><div class=a style="width:50px"></div></div>"#;
    assert_eq!(target_width(html), 30.0);
}
