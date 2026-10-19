//! BUG-1287 — `<length-percentage>` geometry attributes of SVG shapes (`width="100%"`,
//! `cx="50%"`, `5em`) used to fail `f32::parse` and collapse to 0.

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox, SvgShapeKind};

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn shapes(svg_open: &str, body: &str) -> Vec<SvgShapeKind> {
    let html = format!("<html><body>{svg_open}{body}</svg></body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn walk(b: &LayoutBox, out: &mut Vec<SvgShapeKind>) {
        if let BoxKind::SvgShape { shape, .. } = &b.kind {
            out.push(shape.clone());
        }
        b.children.iter().for_each(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(&tree, &mut out);
    out
}

const SVG: &str = r#"<svg style="width:200px;height:100px">"#;

#[test]
fn rect_percent_size_resolves_against_svg_viewport() {
    let s = shapes(SVG, r#"<rect width="100%" height="100%"/>"#);
    assert!(matches!(s[0], SvgShapeKind::Rect { x, y, width, height, .. }
        if (x, y, width, height) == (0.0, 0.0, 200.0, 100.0)), "{:?}", s[0]);
}

#[test]
fn rect_mixed_units() {
    let s = shapes(SVG, r#"<rect x="10%" y="10%" width="50" height="50%"/>"#);
    assert!(matches!(s[0], SvgShapeKind::Rect { x, y, width, height, .. }
        if (x, y, width, height) == (20.0, 10.0, 50.0, 50.0)), "{:?}", s[0]);
}

#[test]
fn circle_center_and_radius_percent() {
    let s = shapes(SVG, r#"<circle cx="50%" cy="50%" r="10%"/>"#);
    let SvgShapeKind::Circle { cx, cy, r } = s[0] else { panic!("{:?}", s[0]) };
    let expected_r = ((200.0f32 * 200.0 + 100.0 * 100.0) / 2.0).sqrt() * 0.1;
    assert_eq!((cx, cy), (100.0, 50.0));
    assert!((r - expected_r).abs() < 1e-3, "r = {r}");
}

#[test]
fn em_length_uses_element_font_size() {
    let s = shapes(r#"<svg style="width:200px;height:100px;font-size:10px">"#, r#"<rect width="5em" height="2em"/>"#);
    assert!(matches!(s[0], SvgShapeKind::Rect { width, height, .. } if (width, height) == (50.0, 20.0)), "{:?}", s[0]);
}

#[test]
fn percent_uses_view_box_when_present() {
    let s = shapes(r#"<svg style="width:200px;height:100px" viewBox="0 0 50 40">"#, r#"<rect width="100%" height="50%"/>"#);
    assert!(matches!(s[0], SvgShapeKind::Rect { width, height, .. } if (width, height) == (50.0, 20.0)), "{:?}", s[0]);
}

#[test]
fn line_percent_endpoints() {
    let s = shapes(SVG, r#"<line x1="0" y1="0" x2="100%" y2="100%"/>"#);
    assert!(matches!(s[0], SvgShapeKind::Line { x2, y2, .. } if (x2, y2) == (200.0, 100.0)), "{:?}", s[0]);
}
