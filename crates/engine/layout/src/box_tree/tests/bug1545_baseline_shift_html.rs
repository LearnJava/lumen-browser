//! BUG-1545 — `baseline-shift` shifts the text of an HTML inline box like the matching
//! `vertical-align` (CSS Inline L3 §5.2), not only SVG `<text>`.

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// `(block height, y_offset)` of the fragment holding "HH" for a `<span style=...>` in a
/// 20px/40px line: a raised box can grow the line, so the offset alone hides the shift.
fn span_y(style: &str) -> (f32, f32) {
    let html = format!(
        "<html><body style=\"margin:0\"><div style=\"font:20px/2 sans-serif\">a<span style=\"{style}\">HH</span></div></body></html>"
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn find(b: &LayoutBox) -> Option<(f32, f32)> {
        if let BoxKind::InlineRun { lines, .. } = &b.kind
            && let Some(f) = lines.iter().flatten().find(|f| f.text.contains("HH"))
        {
            return Some((b.rect.height, f.y_offset));
        }
        b.children.iter().find_map(find)
    }
    find(&tree).expect("fragment with HH")
}

#[test]
fn length_matches_vertical_align_and_moves_the_text() {
    let plain = span_y("");
    assert_ne!(span_y("baseline-shift:10px"), plain);
    assert_eq!(span_y("baseline-shift:10px"), span_y("vertical-align:10px"));
    assert_eq!(span_y("baseline-shift:-5px"), span_y("vertical-align:-5px"));
}

#[test]
fn percentage_matches_vertical_align() {
    assert_ne!(span_y("baseline-shift:50%"), span_y(""));
    assert_eq!(span_y("baseline-shift:50%"), span_y("vertical-align:50%"));
    assert_eq!(span_y("baseline-shift:-100%"), span_y("vertical-align:-100%"));
}

#[test]
fn keywords_match_vertical_align() {
    for (bs, va) in [("sub", "sub"), ("super", "super"), ("top", "top"), ("bottom", "bottom"), ("center", "middle")] {
        assert_eq!(span_y(&format!("baseline-shift:{bs}")), span_y(&format!("vertical-align:{va}")), "{bs}");
    }
    assert_ne!(span_y("baseline-shift:super"), span_y(""));
}

#[test]
fn explicit_vertical_align_wins() {
    assert_eq!(span_y("baseline-shift:10px;vertical-align:top"), span_y("vertical-align:top"));
}

#[test]
fn zero_and_baseline_do_not_shift() {
    let plain = span_y("");
    assert_eq!(span_y("baseline-shift:0"), plain);
    assert_eq!(span_y("baseline-shift:baseline"), plain);
}
