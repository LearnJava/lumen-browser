//! BUG-1327 — a whitespace-only text node between inline elements must survive
//! when `white-space` preserves it (CSS Text L3 §4.1.1) and must never be
//! dropped when it holds characters outside the collapsible set (U+3000…).

use lumen_core::geom::Size;

/// Every character, the space included, is 10 px wide.
struct Fixed10;
impl crate::TextMeasurer for Fixed10 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        10.0
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

fn laid_out(ws: &str, between: &str) -> super::super::LayoutBox {
    let html = format!(r#"<div id="box"><b>a</b>{between}<b>b</b></div>"#);
    let css = format!("body{{margin:0}}#box{{font:20px sans-serif;white-space:{ws}}}");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed10)
}

fn laid_out_texts(ws: &str, between: &str) -> Vec<String> {
    texts(&laid_out(ws, between)).into_iter().map(|(t, _)| t).collect()
}

/// `(x, line index)` of the fragment `b` in `<div id=box><b>a</b>{between}<b>b</b></div>`.
fn b_pos(ws: &str, between: &str) -> (f32, usize) {
    let root = laid_out(ws, between);
    fn find(b: &super::super::LayoutBox) -> Option<(f32, usize)> {
        if let super::super::BoxKind::InlineRun { lines, .. } = &b.kind
            && let Some((i, f)) = lines
                .iter()
                .enumerate()
                .find_map(|(i, l)| l.iter().find(|f| f.text == "b").map(|f| (i, f)))
        {
            return Some((f.x, i));
        }
        b.children.iter().find_map(find)
    }
    find(&root).unwrap_or_else(|| panic!("no fragment `b` under {ws}: {:?}", texts(&root)))
}

fn texts(b: &super::super::LayoutBox) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    if let super::super::BoxKind::InlineRun { lines, .. } = &b.kind {
        out.extend(lines.iter().flatten().map(|f| (f.text.clone(), f.x)));
    }
    for c in &b.children {
        out.extend(texts(c));
    }
    out
}

#[test]
fn spaces_between_elements_survive_in_preserving_modes() {
    for ws in ["pre", "pre-wrap", "break-spaces"] {
        assert_eq!(b_pos(ws, "    ").0, 50.0, "{ws}");
    }
}

#[test]
fn spaces_between_elements_still_collapse_in_normal_modes() {
    for ws in ["normal", "nowrap", "pre-line"] {
        // Same-style neighbours coalesce into one fragment holding one space.
        assert_eq!(laid_out_texts(ws, "    "), ["a b"], "{ws}");
    }
}

#[test]
fn newline_between_elements_breaks_the_line_in_pre_and_pre_line() {
    for ws in ["pre", "pre-wrap", "pre-line"] {
        assert_eq!(b_pos(ws, "\n"), (0.0, 1), "{ws}");
    }
}

#[test]
fn ideographic_spaces_are_not_collapsible() {
    // CSS Text L3 §4.1.1: only U+0020, U+0009, U+000A, U+000C, U+000D collapse.
    assert_eq!(b_pos("pre", "\u{3000}\u{3000}").0, 30.0);
}
