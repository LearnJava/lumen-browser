//! BUG-1326 — a tab runs to the next tab stop, a multiple of `tab-size`
//! (CSS Text L3 §4.2); before the fix it added a fixed `N × 8 px`.

use lumen_core::geom::Size;

/// Every character, the space and `0` included, is 10 px wide.
struct Fixed10;
impl crate::TextMeasurer for Fixed10 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        10.0
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// `x` (inside its line) of the fragment `b` that follows `before`, under
/// `white-space: <ws>` and `style` on the container.
fn b_x(ws: &str, style: &str, before: &str) -> f32 {
    let html = format!(r#"<div id="box">{before}<i>b</i></div>"#);
    let css = format!("body{{margin:0}}#box{{font:20px sans-serif;white-space:{ws};{style}}}");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed10);
    fn find(b: &super::super::LayoutBox) -> Option<f32> {
        if let super::super::BoxKind::InlineRun { lines, .. } = &b.kind
            && let Some(f) = lines.iter().flatten().find(|f| f.text == "b")
        {
            return Some(f.x);
        }
        b.children.iter().find_map(find)
    }
    find(&root).expect("no fragment `b`")
}

fn pre(style: &str, before: &str) -> f32 {
    b_x("pre", style, before)
}

#[test]
fn tab_after_short_text_reaches_the_stop() {
    assert_eq!(pre("tab-size:4", "a\t"), 40.0);
    assert_eq!(pre("tab-size:8", "a\t"), 80.0);
}

#[test]
fn tab_on_a_stop_goes_to_the_next_one() {
    assert_eq!(pre("tab-size:8", "abcdefgh\t"), 160.0);
    assert_eq!(pre("tab-size:2", "ab\t"), 40.0);
}

#[test]
fn consecutive_tabs_advance_one_stop_each() {
    assert_eq!(pre("tab-size:8", "abcdefgh\t\t"), 240.0);
    assert_eq!(pre("tab-size:4", "a\t\t"), 80.0);
}

#[test]
fn tab_size_length_is_independent_of_the_space_width() {
    assert_eq!(pre("tab-size:40px", "a\t"), 40.0);
    assert_eq!(pre("tab-size:80px", "a\t"), 80.0);
}

#[test]
fn integer_tab_size_scales_with_the_font() {
    // The space is 10 px whatever the size: the measurer is fixed.
    assert_eq!(pre("tab-size:4;font-size:40px", "a\t"), 40.0);
}

#[test]
fn tab_closer_than_half_a_space_skips_a_stop() {
    // `a` + 28 px letter-spacing puts the pen at 38 px, stops every 40: 2 px
    // to the stop is under 0.5 × 10 px, so the next stop (80) is used.
    assert_eq!(pre("tab-size:40px;letter-spacing:28px", "a\t"), 80.0);
}

#[test]
fn letter_spacing_counts_in_the_stop_unit() {
    // One space = 10 + 2 (letter-spacing); tab-size 4 → stops every 48 px.
    // `a` takes 10 + 2, the tab reaches 48.
    assert_eq!(pre("tab-size:4;letter-spacing:2px", "a\t"), 48.0);
}

#[test]
fn pre_wrap_tabs_follow_the_same_stops() {
    assert_eq!(b_x("pre-wrap", "tab-size:4", "a\t"), 40.0);
    assert_eq!(b_x("pre-wrap", "tab-size:8", "abcdefgh\t"), 160.0);
}

#[test]
fn tab_stops_are_measured_from_the_line_start() {
    // After a newline the pen restarts at the container edge.
    assert_eq!(pre("tab-size:4", "abc\nx\t"), 40.0);
}
