//! `box-decoration-break` on an inline box cut across lines (CSS Fragmentation L3 §5.1): `slice`
//! leaves the start edge on the first line fragment and the end edge on the last, `clone` repeats
//! both on every line and keeps room for them when breaking lines.

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, InlineFrag, LayoutBox};

struct Measurer;
impl crate::TextMeasurer for Measurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// Lines of the first inline run of `<div style="width:100px">body</div>` (font 20px → glyph 10px).
fn lines(body: &str) -> Vec<Vec<InlineFrag>> {
    let html = format!(
        "<html><body style=\"margin:0;font-size:20px\"><div style=\"width:100px\">{body}</div></body></html>"
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let hp = lumen_core::ext::NullHyphenationProvider;
    let (tree, _) = super::super::layout_measured_hyp_with_counters(
        &doc, &sheet, Size::new(800.0, 600.0), &Measurer, &hp, false,
    );
    fn find(b: &LayoutBox) -> Option<Vec<Vec<InlineFrag>>> {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            return Some(lines.clone());
        }
        b.children.iter().find_map(find)
    }
    find(&tree).expect("inline run")
}

fn texts(lines: &[Vec<InlineFrag>]) -> Vec<String> {
    lines.iter().map(|l| l.iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join("|")).collect()
}

const END_EDGE: &str = "border-right:40px solid blue";

#[test]
fn clone_reserves_the_trailing_edge_when_breaking_lines() {
    let l = lines(&format!("<span style=\"{END_EDGE};box-decoration-break:clone\">x x x x x</span>"));
    assert_eq!(texts(&l), ["x x x", "x|x"]);
    let first = l[0].last().unwrap();
    assert!(first.edge_end, "the cut line keeps the end edge");
    let last = l[1].last().unwrap();
    assert!(last.edge_end);
}

#[test]
fn slice_fits_more_and_keeps_the_end_edge_for_the_last_line_only() {
    let l = lines(&format!("<span style=\"{END_EDGE}\">x x x x x x</span>"));
    assert_eq!(texts(&l), ["x x x x x", "x"]);
    assert!(!l[0].last().unwrap().edge_end, "a sliced box has no end edge on the cut line");
    assert!(l[1].last().unwrap().edge_end);
}

#[test]
fn clone_repeats_the_start_edge_on_continuation_lines() {
    let style = "border-left:10px solid red;padding-left:5px;box-decoration-break:clone";
    let l = lines(&format!("<span style=\"{style}\">x x x x x x</span>"));
    assert!(l.len() >= 2, "{:?}", texts(&l));
    for line in &l {
        let f = &line[0];
        assert!(f.edge_start, "{:?}", texts(&l));
        assert_eq!(f.x, 15.0, "text starts after border + padding on every line");
        assert_eq!(f.padding_left, 5.0);
    }
}

#[test]
fn slice_continuation_line_has_no_start_edge() {
    let l = lines("<span style=\"border-left:10px solid red;padding-left:5px\">x x x x x x</span>");
    assert!(l.len() >= 2);
    assert!(l[0][0].edge_start);
    assert_eq!(l[0][0].x, 15.0);
    assert!(!l[1][0].edge_start);
    assert_eq!(l[1][0].x, 0.0);
}

#[test]
fn webkit_prefixed_alias_clones_too() {
    let l = lines(&format!("<span style=\"{END_EDGE};-webkit-box-decoration-break:clone\">x x x x x</span>"));
    assert_eq!(texts(&l), ["x x x", "x|x"]);
}
