//! BUG-1521 — `text-emphasis` marks take room in the line box when the half-leading is too small
//! for them (CSS Text Decoration L3 §4.2); a roomy `line-height` is left alone.

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

/// `(height of the <div>, y_offset of its first fragment)` for a 40px div with `style`.
fn laid(style: &str) -> (f32, f32) {
    let html = format!(
        "<html><body style=\"margin:0\"><div style=\"font-size:40px;{style}\">XXXX</div></body></html>"
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn run(b: &LayoutBox) -> Option<(f32, f32)> {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            let f = lines.iter().flatten().next()?;
            return Some((b.rect.height, f.y_offset));
        }
        b.children.iter().find_map(run)
    }
    run(&tree).expect("inline run")
}

#[test]
fn no_marks_no_growth() {
    assert_eq!(laid("line-height:1"), (40.0, 0.0));
}

#[test]
fn over_marks_grow_the_line_on_top() {
    assert_eq!(laid("line-height:1;text-emphasis:circle"), (60.0, 20.0));
}

#[test]
fn under_marks_grow_the_line_at_the_bottom() {
    assert_eq!(laid("line-height:1;text-emphasis:circle;text-emphasis-position:under right"), (60.0, 0.0));
}

#[test]
fn roomy_line_height_absorbs_the_marks() {
    assert_eq!(laid("line-height:2;text-emphasis:circle"), (80.0, 20.0));
}

#[test]
fn half_leading_is_credited() {
    // Half-leading 5px of 20px marks: 15px are missing.
    assert_eq!(laid("line-height:1.25;text-emphasis:circle"), (65.0, 20.0));
}
