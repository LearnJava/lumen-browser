use lumen_core::geom::Size;

use super::super::{BoxKind, LayoutBox};

// ── BUG-1354: `<svg>` in flow is an inline-level replaced box (CSS 2.1 §10.3.2, §10.6.2) ──

/// Kind names of the first `<div>`'s direct children.
fn div_child_kinds(body: &str) -> Vec<&'static str> {
    let doc = lumen_html_parser::parse(&format!("<!doctype html><body>{body}"));
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find<'a>(b: &'a LayoutBox, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
        if matches!(&doc.get(b.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div") {
            return Some(b);
        }
        b.children.iter().find_map(|c| find(c, doc))
    }
    let div = find(&root, &doc).expect("div box");
    div.children
        .iter()
        .map(|c| match c.kind {
            BoxKind::InlineBlockRow => "row",
            BoxKind::SvgRoot { .. } => "svg",
            _ => "other",
        })
        .collect()
}

#[test]
fn svg_in_flow_goes_into_inline_row() {
    let k = div_child_kinds(r#"<div style="width:300px"><svg width="200" height="50"></svg></div>"#);
    assert_eq!(k, vec!["row"]);
}

#[test]
fn block_svg_stays_a_block_level_box() {
    let k = div_child_kinds(r#"<div style="width:300px"><svg style="display:block" width="200" height="50"></svg></div>"#);
    assert_eq!(k, vec!["svg"]);
}

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

#[test]
fn svg_row_has_baseline_gap_like_img() {
    fn div_height(inner: &str) -> f32 {
        let doc = lumen_html_parser::parse(&format!(
            "<!doctype html><body><div style=\"width:300px\">{inner}</div>"
        ));
        let sheet = lumen_css_parser::parse("body{margin:0}");
        let root = crate::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Measurer);
        fn find<'a>(b: &'a LayoutBox, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
            if matches!(&doc.get(b.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div") {
                return Some(b);
            }
            b.children.iter().find_map(|c| find(c, doc))
        }
        find(&root, &doc).expect("div box").rect.height
    }
    let svg = div_height(r#"<svg width="200" height="50"></svg>"#);
    let img = div_height(r#"<img width="200" height="50">"#);
    assert!(svg > 50.0, "svg row must reserve the baseline gap, got {svg}, img {img}");
    assert_eq!(svg, img);
}

/// `width`/`height` корневого `<svg>` принимают CSS-длины (`10px`, `2em`, `50%`), а не только число;
/// `auto` оставляет умолчание 300×150.
#[test]
fn svg_root_size_attributes_accept_units() {
    fn svg_size(attrs: &str) -> (f32, f32) {
        let doc = lumen_html_parser::parse(&format!(
            "<!doctype html><body><div style=\"width:400px;font-size:10px\"><svg id=s {attrs}></svg></div>"
        ));
        let sheet = lumen_css_parser::parse("body{margin:0}");
        let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find(b: &LayoutBox) -> Option<&LayoutBox> {
            if matches!(b.kind, BoxKind::SvgRoot { .. }) {
                return Some(b);
            }
            b.children.iter().find_map(find)
        }
        let b = find(&root).expect("svg box");
        (b.rect.width, b.rect.height)
    }
    assert_eq!(svg_size(r#"width="10px" height="5px""#), (10.0, 5.0));
    assert_eq!(svg_size(r#"width="2em" height="1em""#), (20.0, 10.0));
    assert_eq!(svg_size(r#"width="50%" height="30""#), (200.0, 30.0));
    assert_eq!(svg_size(r#"width="auto" height="auto""#), (300.0, 150.0));
}
