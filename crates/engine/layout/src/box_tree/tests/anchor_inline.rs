//! BUG-1477: an `anchor-name` on a plain inline element (no `LayoutBox` of its own) anchors to
//! the union of its line fragments, as `getBoundingClientRect` reports it.

use lumen_core::geom::{Rect, Size};

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

/// Lays `html` out; returns the box tree and the `getBoundingClientRect` map.
fn build(html: &str, css: &str) -> (LayoutBox, std::collections::HashMap<u32, [f32; 4]>, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Measurer);
    let rects = crate::collect_layout_rects(&root, &doc);
    (root, rects, doc)
}

fn gbcr(rects: &std::collections::HashMap<u32, [f32; 4]>, doc: &lumen_dom::Document, id: &str) -> [f32; 4] {
    let node = doc.find_by_id(id).unwrap_or_else(|| panic!("нет #{id}"));
    *rects.get(&(node.index() as u32)).unwrap_or_else(|| panic!("нет rect #{id}"))
}

fn target(root: &LayoutBox, doc: &lumen_dom::Document) -> Rect {
    super::find_by_id_all(root, doc, "t").expect("нет #t").rect
}

const TARGET: &str = "#t{position:absolute;width:5px;height:5px;position-anchor:--a;left:anchor(left);top:anchor(bottom)}";

#[test]
fn span_anchor_uses_its_fragment_rect() {
    let (root, rects, doc) = build(
        r#"<div style="padding-left:30px">xx <span id="a" style="anchor-name:--a">YYY</span></div><div id="t"></div>"#,
        TARGET,
    );
    let a = gbcr(&rects, &doc, "a");
    assert!(a[0] > 30.0, "якорь стоит после текста: {a:?}");
    let t = target(&root, &doc);
    assert_eq!((t.x, t.y), (a[0], a[1] + a[3]), "цель у левого нижнего угла якоря, а не (0, …)");
}

#[test]
fn display_inline_div_anchor_matches_span() {
    let (root, rects, doc) = build(
        r#"<div style="padding-left:30px">xx <div id="a" style="display:inline;anchor-name:--a">YYY</div></div><div id="t"></div>"#,
        TARGET,
    );
    let a = gbcr(&rects, &doc, "a");
    let t = target(&root, &doc);
    assert_eq!((t.x, t.y), (a[0], a[1] + a[3]));
}

#[test]
fn wrapped_span_anchor_is_the_union_of_its_lines() {
    // 8 px per glyph, 60 px wide: "aaaa bbbb" breaks after the first word.
    let (root, rects, doc) = build(
        r#"<div style="width:60px">zz <span id="a" style="anchor-name:--a">aaaa bbbb</span> cc</div><div id="t"></div>"#,
        "#t{position:absolute;width:5px;height:5px;position-anchor:--a;left:anchor(right);top:anchor(bottom)}",
    );
    let a = gbcr(&rects, &doc, "a");
    assert!(a[3] > 25.0, "два строковых фрагмента: {a:?}");
    let t = target(&root, &doc);
    assert_eq!((t.x, t.y), (a[0] + a[2], a[1] + a[3]));
}

#[test]
fn text_in_a_nested_inline_still_belongs_to_the_anchor() {
    let (root, rects, doc) = build(
        r#"<div style="padding-left:30px">xx <span id="a" style="anchor-name:--a"><b>YYY</b></span></div><div id="t"></div>"#,
        TARGET,
    );
    let a = gbcr(&rects, &doc, "a");
    let t = target(&root, &doc);
    assert_eq!((t.x, t.y), (a[0], a[1] + a[3]));
}

#[test]
fn inline_block_anchor_keeps_its_box_rect() {
    let (root, _, doc) = build(
        r#"<div style="padding-left:30px">xx <span id="a" style="anchor-name:--a;display:inline-block;width:20px;height:10px"></span></div><div id="t"></div>"#,
        TARGET,
    );
    let a = super::find_by_id_all(&root, &doc, "a").expect("нет #a").rect;
    let t = target(&root, &doc);
    assert_eq!((t.x, t.y), (a.x, a.y + a.height));
}

/// BUG-1774: the space between an `inline-block` and the next inline element keeps its width.
#[test]
fn space_after_inline_block_survives_before_inline_element() {
    let block = r#"<span style="display:inline-block;width:100px;height:20px"></span>"#;
    let (_, rects, doc) = build(&format!(r#"<div>{block} <b id="b">foo</b></div>"#), "");
    let (_, bare, bare_doc) = build(&format!(r#"<div>{block} <span id="b">foo</span></div>"#), "");
    let b = gbcr(&rects, &doc, "b");
    assert_eq!(b[0], 100.0 + 8.0, "пробел шириной в `char_width(' ')` между блоком и <b>");
    assert_eq!(b[0], gbcr(&bare, &bare_doc, "b")[0]);
}
