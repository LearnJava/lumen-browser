//! TEXTQ-5 — the width of a word in layout comes from the measurer's shaper
//! ([`crate::TextMeasurer::shaped_width`]), so kerning moves the following
//! fragment; a measurer without a shaper keeps the per-character sum.

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};
use crate::ShapeParams;

/// Glyph = half the font size; the pair "AV" kerns by -4px. `letter-spacing`
/// follows every character like the real shaped measurer (trailing one included).
struct Kerning {
    shaped: bool,
}
impl crate::TextMeasurer for Kerning {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
    fn shaped_width(&self, text: &str, p: &ShapeParams<'_>) -> Option<f32> {
        if !self.shaped {
            return None;
        }
        let n = text.chars().count() as f32;
        let kern = text.matches("AV").count() as f32 * 4.0;
        Some(n * p.font_size_px * 0.5 - kern + n * p.letter_spacing)
    }
}

/// `(text, width)` of every non-blank fragment (font 20px → glyph 10px).
fn frags(m: &Kerning, body: &str) -> Vec<(String, f32)> {
    let html = format!("<html><body style=\"margin:0;font-size:20px\">{body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), m, &hp, false,
    );
    fn walk(b: &LayoutBox, out: &mut Vec<(String, f32)>) {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            for f in lines.iter().flatten() {
                if !f.text.trim().is_empty() {
                    out.push((f.text.trim().to_string(), f.width));
                }
            }
        }
        b.children.iter().for_each(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(&tree, &mut out);
    out
}

fn only_width(m: &Kerning, body: &str) -> f32 {
    let l = frags(m, body);
    assert_eq!(l.len(), 1, "{l:?}");
    l[0].1
}

#[test]
fn kerned_word_is_narrower_than_the_character_sum() {
    assert_eq!(only_width(&Kerning { shaped: true }, "AV"), 16.0);
    assert_eq!(only_width(&Kerning { shaped: false }, "AV"), 20.0);
}

#[test]
fn letter_spacing_goes_between_shaped_characters_not_after_the_last() {
    // 'A' 'V': 20 - 4 kern + 2 * 2px spacing, minus the trailing spacing = 18;
    // the character sum is 10 + 2 + 10 = 22.
    let body = "<span style=\"letter-spacing:2px\">AV</span>";
    assert_eq!(only_width(&Kerning { shaped: true }, body), 18.0);
    assert_eq!(only_width(&Kerning { shaped: false }, body), 22.0);
}

#[test]
fn tab_splits_the_run_so_kerning_does_not_cross_it() {
    let m = Kerning { shaped: true };
    let params = ShapeParams { font_size_px: 20.0, letter_spacing: 0.0, families: &[], features: &[], axes: &[] };
    use crate::TextMeasurer;
    assert_eq!(m.shaped_width("AV", &params), Some(16.0));
    // "A\tV" is never one "AV" pair: two characters plus a tab stop of 30px.
    assert_eq!(crate::box_tree::measure_text_w_families("A\tV", 20.0, 0.0, 30.0, &[], &m), 10.0 + 30.0 + 10.0);
    assert_eq!(crate::box_tree::measure_text_w_families("AV", 20.0, 0.0, 30.0, &[], &m), 16.0);
}
