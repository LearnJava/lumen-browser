//! BUG-935 срез 71 — the generic child pass must not run for an `<svg>` root.
//!
//! `build_box_inner` replaces an `SvgRoot`'s children with the SVG shape tree
//! (`build_svg_children`); building them first as ordinary HTML boxes only to drop them
//! made every inline `<svg>` cost a full extra pass over its subtree (on `lenta.ru` a
//! 340-element icon sprite = 4 ms of every same-tick flush). The census below counts
//! `build_box_inner` calls: they must not grow with the number of shapes.

use lumen_core::geom::Size;

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn sprite(shapes: usize) -> String {
    let mut svg = String::from(r#"<svg id="s" width="100" height="100" viewBox="0 0 100 100">"#);
    for i in 0..shapes {
        svg.push_str(&format!(
            r#"<symbol id="sym{i}"><path d="M0 0L10 10"/><circle cx="5" cy="5" r="2"/></symbol>"#
        ));
    }
    svg.push_str(r#"<rect x="1" y="1" width="5" height="5"/></svg>"#);
    format!("<html><body>{svg}<p>after</p></body></html>")
}

fn build_count(shapes: usize) -> (u32, usize) {
    let doc = lumen_html_parser::parse(&sprite(shapes));
    let sheet = lumen_css_parser::parse("");
    let vp = Size::new(800.0, 600.0);
    let hp = lumen_core::ext::NullHyphenationProvider;
    let _ = super::super::take_box_build_stats();
    let (tree, _) = super::super::layout_measured_hyp_with_counters(&doc, &sheet, vp, &Measurer, &hp, false);
    let built = super::super::take_box_build_stats().built;
    let svg = doc.find_by_id("s").expect("fixture svg");
    fn find(b: &crate::box_tree::LayoutBox, id: lumen_dom::NodeId) -> Option<&crate::box_tree::LayoutBox> {
        if b.node == id && matches!(b.kind, crate::box_tree::BoxKind::SvgRoot { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(|c| find(c, id))
    }
    let root = find(&tree, svg).expect("svg root box");
    (built, root.children.len())
}

#[test]
fn svg_children_are_not_built_twice() {
    let (small_built, small_children) = build_count(2);
    let (large_built, large_children) = build_count(60);
    assert_eq!(
        small_built, large_built,
        "boxes built for the page must not depend on how many shapes the <svg> holds"
    );
    // The shape tree is still built: the `<rect>` is painted, `<symbol>` content is not.
    assert_eq!((small_children, large_children), (1, 1));
}
