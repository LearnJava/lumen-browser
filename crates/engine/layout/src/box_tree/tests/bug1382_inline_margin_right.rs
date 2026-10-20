//! BUG-1382 — the trailing `margin-right` of an inline box without background, border or padding
//! must keep the next word out of the box's fragment (CSS 2.1 §8.3, §10.3.1).

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

/// `(text, x)` of every non-blank fragment of the laid-out `body` (font 20px → glyph 10px).
fn frags(body: &str) -> Vec<(String, f32)> {
    let html = format!("<html><body style=\"margin:0;font-size:20px\">{body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn walk(b: &LayoutBox, out: &mut Vec<(String, f32)>) {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            for f in lines.iter().flatten() {
                if !f.text.trim().is_empty() {
                    out.push((f.text.trim().to_string(), f.x));
                }
            }
        }
        b.children.iter().for_each(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(&tree, &mut out);
    out
}

#[test]
fn margin_right_separates_following_word() {
    let l = frags("<span style=\"margin-right:40px\">x</span>y");
    assert_eq!(l.len(), 2, "{l:?}");
    assert_eq!(l[0], ("x".to_string(), 0.0));
    assert_eq!(l[1], ("y".to_string(), 50.0));
}

#[test]
fn margin_right_after_preceding_text() {
    let l = frags("a<span style=\"margin-right:40px\">x</span>y");
    let y = l.last().expect("fragments");
    assert_eq!(y.1, 10.0 + 10.0 + 40.0, "{l:?}");
}

#[test]
fn margin_left_still_separates() {
    let l = frags("a<span style=\"margin-left:40px\">x</span>y");
    assert_eq!(l.iter().find(|f| f.0.starts_with('x')).map(|f| f.1), Some(50.0), "{l:?}");
}
