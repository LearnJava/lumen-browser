//! BUG-1470 — `position:absolute`/`fixed` inside an inline context must not break
//! the line or take space in it, and its static position is the inline cursor
//! (the point right after the preceding inline content), not the container's
//! content-box corner.

use lumen_core::geom::Size;

/// 10 px per glyph, ascent 0.8 / descent 0.2 / line-gap 0.2 (`normal` = 1.2 × кегль).
struct Fixed10;

impl super::super::super::TextMeasurer for Fixed10 {
    fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    fn line_gap_px(&self, font_size_px: f32) -> f32 { font_size_px * 0.2 }
}

/// Lays out a 400px `<div>` holding `B` (inline-block 50×10), the given `middle`,
/// then `r` (inline-block 50×10), and returns `(a.rect, r.rect)`.
///
/// Markup is minified (no inter-element whitespace) so the probe isolates the
/// out-of-flow behaviour from white-space collapsing, like the bug's `--mcp` probe.
fn layout_probe(middle: &str) -> (Rect, Rect, super::super::LayoutBox, lumen_dom::Document) {
    let html = format!(
        r#"<div id="c" style="width:400px"><span id="B" style="display:inline-block;width:50px;height:10px;vertical-align:top"></span>{middle}<span id="r" style="display:inline-block;width:50px;height:10px;vertical-align:top"></span></div>"#
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("*{margin:0}");
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed10);
    let a = super::find_by_id_all(&root, &doc, "a").map(|b| b.rect).unwrap_or(Rect::ZERO);
    let r = super::find_by_id_all(&root, &doc, "r").expect("#r").rect;
    (a, r, root, doc)
}

use super::super::Rect;

/// Case 1/4 of the probe: an absolute box as a DIRECT child between two atomic
/// inlines must not break the line, and its static position is the inline cursor.
#[test]
fn direct_child_absolute_does_not_break_the_line() {
    let (a, r, root, doc) = layout_probe(
        r#"<span id="a" style="position:absolute;width:30px;height:10px"></span>"#,
    );
    assert_eq!((a.x, a.y, a.width, a.height), (50.0, 0.0, 30.0, 10.0), "#a static position");
    assert_eq!((r.x, r.y), (50.0, 0.0), "#r stays on the first line at x=50");
    // Both in-flow boxes share one InlineBlockRow — the absolute box did not split it.
    let rows = count_rows(&root, &doc, "c");
    assert_eq!(rows, 1, "the div must hold a single InlineBlockRow, got {rows}");
}

/// Case 3: explicit `left`/`top` still win over the static position.
#[test]
fn explicit_insets_override_the_static_position() {
    let (a, r, _, _) = layout_probe(
        r#"<span id="a" style="position:absolute;left:5px;top:5px;width:30px;height:10px"></span>"#,
    );
    assert_eq!((a.x, a.y, a.width, a.height), (5.0, 5.0, 30.0, 10.0), "#a at left/top");
    assert_eq!((r.x, r.y), (50.0, 0.0), "#r stays on the first line at x=50");
}

/// Case 5: an absolute box wrapped in a `<span>` (flattened by the escape
/// mechanism) keeps its own box instead of collapsing to a zero-size rect.
#[test]
fn wrapped_absolute_keeps_its_own_box() {
    let (a, r, _, _) = layout_probe(
        r#"<span><span id="a" style="position:absolute;width:30px;height:10px"></span></span>"#,
    );
    assert_eq!((a.x, a.y, a.width, a.height), (50.0, 0.0, 30.0, 10.0), "#a box and static position");
    assert_eq!((r.x, r.y), (50.0, 0.0), "#r stays on the first line at x=50");
}

/// Case 6: an absolute box with text content inside a `<span>` must not take
/// space in the line — `#r` is not shifted by `#a`'s width.
#[test]
fn wrapped_absolute_with_text_takes_no_space() {
    let (a, r, _, _) = layout_probe(
        r#"<span><span id="a" style="position:absolute">CDE</span></span>"#,
    );
    assert_eq!((a.x, a.y), (50.0, 0.0), "#a static position");
    assert!((a.width - 30.0).abs() < 0.01, "#a is sized to its text, got {}", a.width);
    assert_eq!((r.x, r.y), (50.0, 0.0), "#r is not shifted by #a's width");
}

/// The `fixed` half of the bug title: a `position: fixed` box between atomic
/// inlines is likewise transparent to the line and takes its static position
/// from the inline cursor (against the viewport here — no positioned ancestor).
#[test]
fn fixed_child_is_transparent_to_the_line() {
    let (a, r, _, _) = layout_probe(
        r#"<span id="a" style="position:fixed;width:30px;height:10px"></span>"#,
    );
    assert_eq!((a.x, a.y, a.width, a.height), (50.0, 0.0, 30.0, 10.0), "#a static position");
    assert_eq!((r.x, r.y), (50.0, 0.0), "#r stays on the first line at x=50");
}

/// Counts the direct `InlineBlockRow` children of the block `#id`.
fn count_rows(root: &super::super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> usize {
    let target = super::find_by_id_all(root, doc, id).expect("target");
    target
        .children
        .iter()
        .filter(|c| matches!(c.kind, super::super::BoxKind::InlineBlockRow))
        .count()
}
