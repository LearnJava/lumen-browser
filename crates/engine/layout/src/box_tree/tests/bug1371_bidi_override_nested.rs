//! BUG-1371 — `unicode-bidi: bidi-override` / `<bdo>` must reach nested inline
//! elements (CSS Writing Modes L4 §2.4.2).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// `(text, bidi_level)` of every non-blank fragment of the laid-out `body`.
fn levels(body: &str, css: &str) -> Vec<(String, u8)> {
    let html = format!("<html><body style=\"margin:0\">{body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(css);
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn walk(b: &LayoutBox, out: &mut Vec<(String, u8)>) {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            for f in lines.iter().flatten() {
                if !f.text.trim().is_empty() {
                    out.push((f.text.trim().to_string(), f.bidi_level));
                }
            }
        }
        b.children.iter().for_each(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(&tree, &mut out);
    out
}

const OVERRIDE: &str = ".o{direction:rtl;unicode-bidi:bidi-override}";

#[test]
fn block_override_reaches_nested_span() {
    let l = levels("<p class=o><span>abc</span> def</p>", OVERRIDE);
    assert!(l.iter().all(|(_, lv)| lv % 2 == 1), "{l:?}");
}

#[test]
fn block_override_reaches_nested_bold() {
    let l = levels("<p class=o><b>abc</b> def</p>", OVERRIDE);
    assert!(l.iter().all(|(_, lv)| lv % 2 == 1), "{l:?}");
}

#[test]
fn bdo_rtl_reverses_latin() {
    let l = levels("<p><bdo dir=rtl>abc def</bdo></p>", "");
    assert!(!l.is_empty() && l.iter().all(|(_, lv)| lv % 2 == 1), "{l:?}");
}

#[test]
fn bdo_rtl_reaches_nested_element() {
    let l = levels("<p><bdo dir=rtl><i>abc</i> def</bdo></p>", "");
    assert!(!l.is_empty() && l.iter().all(|(_, lv)| lv % 2 == 1), "{l:?}");
}

#[test]
fn plain_nested_span_is_not_overridden() {
    let l = levels("<p><span>abc</span> def</p>", "");
    assert!(l.iter().all(|(_, lv)| *lv == 0), "{l:?}");
}
