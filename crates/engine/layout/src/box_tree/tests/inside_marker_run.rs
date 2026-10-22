// ── `list-style-position: inside` text marker as the run's first segment ──

use lumen_core::geom::{Rect, Size};

fn marker_segments(tree: &super::super::LayoutBox) -> usize {
    let mut count = 0;
    let mut stack = vec![tree];
    while let Some(b) = stack.pop() {
        if let super::super::BoxKind::InlineRun { segments, .. } = &b.kind {
            count += segments
                .iter()
                .filter(|s| s.pseudo_kind == super::super::PseudoKind::Marker)
                .count();
        }
        stack.extend(b.children.iter());
    }
    count
}

#[test]
fn a_second_layout_pass_keeps_one_marker_segment() {
    let doc = lumen_html_parser::parse("<ul><li>x</ul>");
    let sheet = lumen_css_parser::parse(
        "ul { padding: 0; list-style-position: inside; list-style-type: hebrew; }",
    );
    let mut tree = super::super::layout_measured(
        &doc, &sheet, Size::new(400.0, 600.0), &crate::Fixed8,
    );
    assert_eq!(marker_segments(&tree), 1);
    // The same boxes laid out again at another width (a resize).
    let viewport = Size::new(380.0, 600.0);
    super::super::lay_out(
        &mut tree, 0.0, 0.0, 380.0, None, Some(&crate::Fixed8), viewport,
        Rect::new(0.0, 0.0, 380.0, 600.0), &lumen_core::ext::NullHyphenationProvider, false,
    );
    assert_eq!(marker_segments(&tree), 1, "relayout must replace, not stack, the marker segment");
}
