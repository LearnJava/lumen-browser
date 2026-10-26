use lumen_core::geom::Size;

use super::super::{BoxKind, InlineSegment, LayoutBox};

// ── BUG-1353: inline box cut by a block — `slice` decoration (CSS 2.1 §9.2.1.1) ──

fn collect(b: &LayoutBox, out: &mut Vec<InlineSegment>) {
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        out.extend(segments.iter().cloned());
    }
    for c in &b.children {
        collect(c, out);
    }
}

/// Segments of the element-box pieces, as `(text, border_left, border_right, pre, post)`.
fn pieces(body: &str) -> Vec<(String, f32, f32, f32, f32)> {
    let doc = lumen_html_parser::parse(&format!("<!doctype html><body>{body}"));
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let mut segs = Vec::new();
    collect(&root, &mut segs);
    segs.iter()
        .filter(|s| s.is_element_box)
        .map(|s| (s.text.clone(), s.style.border_left_width, s.style.border_right_width, s.pre_space, s.post_space))
        .collect()
}

#[test]
fn text_pieces_keep_only_outer_edges() {
    let p = pieces(r#"<span style="border:3px solid blue">a<div>One</div>b</span>"#);
    assert_eq!(p, vec![("a".into(), 3.0, 0.0, 3.0, 0.0), ("b".into(), 0.0, 3.0, 0.0, 3.0)]);
}

#[test]
fn leading_block_leaves_an_empty_start_piece() {
    let p = pieces(r#"<span style="border:3px solid blue"><div>One</div><span>Two</span></span>"#);
    assert_eq!(p.first(), Some(&(String::new(), 3.0, 0.0, 3.0, 0.0)), "{p:?}");
    assert_eq!(p.last(), Some(&(String::new(), 0.0, 3.0, 0.0, 3.0)), "{p:?}");
}

#[test]
fn uncut_inline_box_keeps_both_edges() {
    let p = pieces(r#"<span style="border:3px solid blue">a</span>"#);
    assert_eq!(p, vec![("a".into(), 3.0, 3.0, 3.0, 3.0)]);
}

// ── BUG-1530: empty inline box with horizontal edges keeps its width (CSS 2.1 §9.4.2) ──

#[test]
fn empty_inline_box_with_padding_keeps_an_edge_segment() {
    let p = pieces(r#"x<span style="padding:0 10px"></span>y"#);
    assert_eq!(p, vec![(String::new(), 0.0, 0.0, 10.0, 10.0)]);
}

#[test]
fn empty_inline_box_with_margin_left_keeps_an_edge_segment() {
    let p = pieces(r#"x<span style="margin-left:10px"></span>y"#);
    assert_eq!(p, vec![(String::new(), 0.0, 0.0, 10.0, 0.0)]);
}

#[test]
fn empty_inline_box_without_edges_stays_empty() {
    assert!(pieces(r#"x<span></span>y"#).is_empty());
}

#[test]
fn empty_inline_box_with_negative_or_cancelling_edges_keeps_an_edge_segment() {
    let p = pieces(r#"x<span style="margin-left:-1px"></span>y"#);
    assert_eq!(p, vec![(String::new(), 0.0, 0.0, -1.0, 0.0)]);
    let p = pieces(r#"x<span style="padding-left:1px;margin-left:-1px"></span>y"#);
    assert_eq!(p, vec![(String::new(), 0.0, 0.0, 0.0, 0.0)]);
}
