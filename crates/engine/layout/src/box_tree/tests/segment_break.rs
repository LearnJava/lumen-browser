//! BUG-1330 — CSS Text L3 §4.1.2: a collapsible segment break between two East
//! Asian wide characters (or next to U+200B) is removed instead of becoming a space.

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

fn lines_of(b: &super::super::LayoutBox, out: &mut Vec<String>) {
    if let super::super::BoxKind::InlineRun { lines, .. } = &b.kind {
        out.extend(lines.iter().map(|l| l.iter().map(|f| f.text.as_str()).collect::<String>()));
    }
    for c in &b.children {
        lines_of(c, out);
    }
}

/// Text of every laid-out line of `<div id=box>{inner}</div>` under `white-space: ws`.
fn laid_out_lines(ws: &str, inner: &str) -> Vec<String> {
    let html = format!(r#"<div id="box">{inner}</div>"#);
    let css = format!("body{{margin:0}}#box{{font:20px sans-serif;white-space:{ws}}}");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed10);
    let mut out = Vec::new();
    lines_of(&root, &mut out);
    out
}

fn one_line(ws: &str, inner: &str) -> String {
    let lines = laid_out_lines(ws, inner);
    assert_eq!(lines.len(), 1, "{ws}: {lines:?}");
    lines.into_iter().next().unwrap()
}

#[test]
fn break_between_wide_characters_is_removed() {
    for ws in ["normal", "nowrap"] {
        assert_eq!(one_line(ws, "<span>日本語\n中国话</span>"), "日本語中国话", "{ws}");
        assert_eq!(one_line(ws, "<span>日本語   \n   中国话</span>"), "日本語中国话", "{ws}");
        assert_eq!(one_line(ws, "<span>日本語 \n \n  \n中国话</span>"), "日本語中国话", "{ws}");
        assert_eq!(one_line(ws, "<span>ＦＵＬＬ\nｶｸ</span>"), "ＦＵＬＬｶｸ", "{ws}");
    }
}

#[test]
fn break_next_to_zero_width_space_is_removed() {
    assert_eq!(one_line("normal", "ภาษา\u{200b}\nไทย"), "ภาษา\u{200b}ไทย");
    assert_eq!(one_line("normal", "aa\u{200b}  \n  bbb"), "aa\u{200b}bbb");
}

#[test]
fn break_stays_a_space_for_latin_hangul_and_mixed_neighbours() {
    assert_eq!(one_line("normal", "FULL\nWIDTH"), "FULL WIDTH");
    assert_eq!(one_line("normal", "한국어\n한국어"), "한국어 한국어");
    assert_eq!(one_line("normal", "日本語\nWIDTH"), "日本語 WIDTH");
    // No segment break — a plain space is never removed.
    assert_eq!(one_line("normal", "日本語 中国话"), "日本語 中国话");
}

#[test]
fn inline_box_boundaries_are_transparent() {
    assert_eq!(one_line("normal", "<b>日本語</b>\n<b>中国话</b>"), "日本語中国话");
    assert_eq!(one_line("normal", "<b>日本語</b>\n中国话"), "日本語中国话");
    assert_eq!(one_line("normal", "aa<span>\u{200b}</span>\nbbb"), "aa\u{200b}bbb");
    assert_eq!(one_line("normal", "<b>FULL</b>\n<b>WIDTH</b>"), "FULL WIDTH");
}

#[test]
fn pre_line_and_pre_keep_the_break() {
    assert_eq!(laid_out_lines("pre-line", "日本語\n中国话"), ["日本語", "中国话"]);
    assert_eq!(laid_out_lines("pre", "日本語\n中国话"), ["日本語", "中国话"]);
}
