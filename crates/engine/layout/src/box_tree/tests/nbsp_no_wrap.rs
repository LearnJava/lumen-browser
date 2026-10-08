//! BUG-1323 — U+00A0, U+202F, U+2007 (UAX #14 class GL) are not CSS document
//! white space (CSS Text L3 §4.1.1): the line must not break at them.

use lumen_core::geom::Size;

/// Every character is 10 px wide, so wrapping is exact and font-independent.
struct Fixed10;
impl crate::TextMeasurer for Fixed10 {
    fn char_width(&self, _: char, _: f32) -> f32 {
        10.0
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// Height of `#box` (`font: 20px`, `width: 200px`) holding `aaaaaaaaaaaaaaa<sep>bbbbbbbbbbbbbbb`
/// (15 + 15 letters, 31 cells — wider than the 20-cell box).
fn box_height(sep: &str, ws: &str) -> f32 {
    let html = format!(r#"<div id="box">aaaaaaaaaaaaaaa{sep}bbbbbbbbbbbbbbb</div>"#);
    let css = format!("body{{margin:0}}#box{{font:20px sans-serif;width:200px;white-space:{ws}}}");
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed10);
    super::find_by_id_all(&root, &doc, "box").expect("box not found").rect.height
}

#[test]
fn space_breaks_the_line() {
    let one_line = box_height("\u{a0}", "normal");
    assert!(box_height(" ", "normal") > one_line * 1.5, "a plain space must wrap");
}

#[test]
fn gl_spaces_do_not_break_the_line() {
    for sep in ["\u{a0}", "\u{202f}", "\u{2007}", "\u{2060}"] {
        for ws in ["normal", "pre-line", "pre-wrap"] {
            let spaced = box_height(" ", ws);
            let h = box_height(sep, ws);
            assert!(h < spaced * 0.75, "U+{:04X} under {ws}: {h} vs space {spaced}", sep.chars().next().unwrap() as u32);
        }
    }
}

#[test]
fn en_space_still_breaks() {
    assert!(box_height("\u{2002}", "normal") > box_height("\u{a0}", "normal") * 1.5);
}
