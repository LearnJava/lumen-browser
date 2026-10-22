//! BUG-1477: an `anchor-name` on a plain inline element (no `LayoutBox` of its own) anchors to
//! the union of its line fragments, as `getBoundingClientRect` reports it.

use lumen_core::geom::Size;

use super::super::{layout, LayoutBox};

fn build(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    (root, doc)
}

fn rect_of(root: &LayoutBox, doc: &lumen_dom::Document, id: &str) -> lumen_core::geom::Rect {
    super::find_by_id_all(root, doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect
}

const TARGET: &str = "#t{position:absolute;width:5px;height:5px;position-anchor:--a;left:anchor(left);top:anchor(bottom)}";

#[test]
fn probe_span_anchor() {
    let (root, doc) = build(
        r#"<div style="padding-left:30px">xx <span id="a" style="anchor-name:--a">YYY</span></div><div id="t"></div>"#,
        TARGET,
    );
    let t = rect_of(&root, &doc, "t");
    eprintln!("PROBE t={t:?}");
    let rects = crate::collect_layout_rects(&root, &doc);
    let a = doc.find_by_id("a").unwrap();
    eprintln!("PROBE a={:?}", rects.get(&(a.index() as u32)));
    panic!();
}
