//! BUG-1322 — `white-space: pre-wrap` / `break-spaces` wrap at the available
//! width (CSS Text L3 §4.1.3). Before the fix both behaved like `pre`: one
//! unbreakable line wider than the container.

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

fn lay_out(html: &str, css: &str) -> (lumen_dom::Document, super::super::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed10);
    (doc, root)
}

const TEXT: &str = "aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk";

/// Height of `#box` (`font: 20px`, `width: 200px`) for `body_html` under
/// `white-space: <ws>`.
fn box_height(ws: &str, extra_css: &str, body_html: &str) -> f32 {
    let html = format!(r#"<div id="box">{body_html}</div>"#);
    let css = format!(
        "body{{margin:0}}#box{{font:20px sans-serif;width:200px;white-space:{ws};{extra_css}}}"
    );
    let (doc, root) = lay_out(&html, &css);
    super::find_by_id_all(&root, &doc, "box").expect("box not found").rect.height
}

fn h(ws: &str) -> f32 {
    box_height(ws, "", TEXT)
}

#[test]
fn pre_wrap_wraps_like_normal() {
    let normal = h("normal");
    assert!(normal > h("pre") * 1.5, "probe must wrap under normal");
    assert!((h("pre-wrap") - normal).abs() < 0.5, "pre-wrap {} vs normal {}", h("pre-wrap"), normal);
}

#[test]
fn break_spaces_wraps_like_normal() {
    assert!((h("break-spaces") - h("normal")).abs() < 0.5);
}

#[test]
fn pre_still_does_not_wrap() {
    assert_eq!(h("pre"), box_height("pre", "", "aaa"), "pre stays a single line");
}

#[test]
fn pre_wrap_forced_newline_still_breaks() {
    let one = box_height("pre-wrap", "", "aaa");
    let two = box_height("pre-wrap", "", "aaa\nbbb");
    assert!(two > one * 1.9, "{two} vs {one}");
}

#[test]
fn pre_wrap_trailing_spaces_hang_instead_of_wrapping() {
    // A long run of spaces after the last word must not push a second line.
    let one = box_height("pre-wrap", "", "aaa");
    let hung = box_height("pre-wrap", "", "aaa                                        ");
    assert_eq!(hung, one, "trailing spaces hang under pre-wrap");
}

#[test]
fn break_spaces_trailing_spaces_take_room_and_wrap() {
    let one = box_height("break-spaces", "", "aaa");
    let spaced = box_height("break-spaces", "", "aaa                                        ");
    assert!(spaced > one * 1.5, "{spaced} vs {one}");
}

#[test]
fn pre_wrap_preserves_runs_of_spaces() {
    // Two spaces between words are kept, so they are wider than one.
    let w = |s: &str| {
        let html = format!(r#"<span id="s" style="font:20px sans-serif;white-space:pre-wrap">{s}</span>"#);
        let (doc, root) = lay_out(&html, "body{margin:0}");
        super::find_by_id_all(&root, &doc, "s").map(|b| b.rect.width)
    };
    if let (Some(a), Some(b)) = (w("a b"), w("a    b")) {
        assert!(b > a + 1.0, "{b} vs {a}");
    }
}

#[test]
fn pre_wrap_wraps_across_inline_elements() {
    // Spaces sit inside the elements: a whitespace-only text node between two
    // inline siblings is dropped before layout in the preserved modes
    // (separate defect, see BUGS.md).
    let spans = "<i>aaaaaa </i><i>bbbbbb </i><i>cc </i><i>dddddd </i><i>eeeeee</i>";
    let normal = box_height("normal", "", spans);
    assert!(normal > box_height("normal", "", "<i>aaaaaa</i>") * 1.5);
    assert!((box_height("pre-wrap", "", spans) - normal).abs() < 0.5, "{} vs {normal}", box_height("pre-wrap", "", spans));
}

#[test]
fn pre_wrap_overflow_wrap_anywhere_breaks_long_word() {
    let word = "x".repeat(60);
    let plain = box_height("pre-wrap", "", &word);
    let broken = box_height("pre-wrap", "overflow-wrap:anywhere;", &word);
    assert!(broken > plain * 1.5, "{broken} vs {plain}");
}

#[test]
fn pre_wrap_cjk_wraps() {
    let cjk = "日本語".repeat(12);
    let normal = box_height("normal", "", &cjk);
    assert!(normal > box_height("normal", "", "日") * 1.5);
    assert!((box_height("pre-wrap", "", &cjk) - normal).abs() < 0.5);
}
