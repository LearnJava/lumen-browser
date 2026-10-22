//! BUG-1485: an anchor with a `transform` is the bounding box of its transformed border box —
//! the rect `getBoundingClientRect` reports — not its layout rect.

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

/// Lays out an anchor `left:50;top:50;40x30` with `transform_css` and a target pinned to its
/// right-bottom corner; returns the target's position and the anchor's `getBoundingClientRect`.
fn corner(transform_css: &str) -> ((f32, f32), [f32; 4]) {
    let html = r#"<div style="position:relative;width:400px;height:300px"><div id="a"></div><div id="t"></div></div>"#;
    let css = format!(
        "body{{margin:0}}#a{{position:absolute;left:50px;top:50px;width:40px;height:30px;\
         anchor-name:--a;transform-origin:0 0;{transform_css}}}\
         #t{{position:absolute;width:5px;height:5px;position-anchor:--a;\
         left:anchor(right);top:anchor(bottom)}}"
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root: LayoutBox = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Measurer);
    let rects = crate::collect_layout_rects(&root, &doc);
    let a = doc.find_by_id("a").expect("нет #a");
    let t = super::find_by_id_all(&root, &doc, "t").expect("нет #t").rect;
    ((t.x, t.y), rects[&(a.index() as u32)])
}

#[test]
fn untransformed_anchor_unchanged() {
    assert_eq!(corner("").0, (90.0, 80.0));
}

#[test]
fn translated_anchor_moves_the_target() {
    let ((x, y), g) = corner("transform:translateX(100px)");
    assert_eq!(g, [150.0, 50.0, 40.0, 30.0]);
    assert_eq!((x, y), (190.0, 80.0));
}

#[test]
fn scaled_anchor_grows_the_target_offset() {
    let ((x, y), _) = corner("transform:scale(2)");
    assert_eq!((x, y), (130.0, 110.0));
}

#[test]
fn rotated_anchor_follows_its_bounding_box() {
    let ((x, y), g) = corner("transform:rotate(45deg)");
    assert!((x - (g[0] + g[2])).abs() < 0.01 && (y - (g[1] + g[3])).abs() < 0.01, "{x} {y} {g:?}");
}
