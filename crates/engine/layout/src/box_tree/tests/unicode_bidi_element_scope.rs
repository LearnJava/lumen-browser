//! `unicode-bidi` applies per inline *element*, not per text segment (CSS Writing
//! Modes L4 §2.2): a `normal` child inside an `embed`/`isolate` span stays inside
//! the span's embedding.

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
fn levels(body: &str) -> Vec<(String, u8)> {
    let html = format!("<html><body style=\"margin:0\">{body}</body></html>");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
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

fn level_of(l: &[(String, u8)], text: &str) -> u8 {
    l.iter().find(|(t, _)| t == text).unwrap_or_else(|| panic!("no fragment {text:?} in {l:?}")).1
}

#[test]
fn embed_spans_a_normal_child() {
    // One RLE around `1 <b>2</b> 3`: the digit in `<b>` is an EN inside an RTL
    // embedding (level 2), as are its neighbours. Split per segment it sat
    // between two embeddings, at the paragraph's level 0.
    let l = levels("<p>x <span style=\"unicode-bidi:embed;direction:rtl\">1 <b>2</b> 3</span></p>");
    assert_eq!(level_of(&l, "2"), 2, "{l:?}");
    assert_eq!(level_of(&l, "1"), 2, "{l:?}");
    assert_eq!(level_of(&l, "3"), 2, "{l:?}");
}

#[test]
fn isolate_spans_a_normal_child() {
    let l = levels("<p>x <span style=\"unicode-bidi:isolate;direction:rtl\">1 <b>2</b> 3</span></p>");
    assert_eq!(level_of(&l, "2"), 2, "{l:?}");
}

#[test]
fn override_on_an_inline_reaches_a_normal_child_and_stops_at_its_end() {
    let l = levels("<p><span style=\"unicode-bidi:bidi-override;direction:rtl\">ab <i>cd</i> ef</span> gh</p>");
    for w in ["ab", "cd", "ef"] {
        assert_eq!(level_of(&l, w) % 2, 1, "{w}: {l:?}");
    }
    assert_eq!(level_of(&l, "gh"), 0, "{l:?}");
}

#[test]
fn nested_scopes_nest() {
    // An isolate inside an override: the isolate's content is not overridden.
    let l = levels(
        "<p><span style=\"unicode-bidi:bidi-override;direction:rtl\">ab \
         <span style=\"unicode-bidi:isolate;direction:ltr\">cd</span> ef</span></p>",
    );
    assert_eq!(level_of(&l, "cd") % 2, 0, "{l:?}");
    assert_eq!(level_of(&l, "ab") % 2, 1, "{l:?}");
    assert_eq!(level_of(&l, "ef") % 2, 1, "{l:?}");
}
