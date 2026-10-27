//! BUG-1547 — `<ol>` numbering: UA `decimal`, `start`, `reversed`, `<li value>`, `type`, nesting
//! bullets; the marker number is the implicit `list-item` counter.

use lumen_core::geom::Size;

use crate::box_tree::BoxKind;

/// Marker texts of every `<li>` in the document, in tree order.
fn markers(html: &str, css: &str) -> Vec<String> {
    let root = super::super::layout(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(css),
        Size::new(800.0, 600.0),
    );
    let mut found = Vec::new();
    super::find_markers(&root, &mut found);
    found
        .iter()
        .map(|m| match &m.kind {
            BoxKind::Marker { text, .. } => text.trim().to_owned(),
            _ => unreachable!(),
        })
        .collect()
}

fn texts(html: &str) -> Vec<String> {
    markers(html, "")
}

#[test]
fn ol_defaults_to_decimal() {
    assert_eq!(texts("<ol><li>a<li>b<li>c</ol>"), ["1.", "2.", "3."]);
}

#[test]
fn ol_start_sets_first_number() {
    assert_eq!(texts("<ol start=5><li>a<li>b</ol>"), ["5.", "6."]);
    assert_eq!(texts("<ol start=-1><li>a<li>b</ol>"), ["-1.", "0."]);
}

#[test]
fn ol_reversed_counts_down_from_item_count() {
    assert_eq!(texts("<ol reversed><li>a<li>b<li>c</ol>"), ["3.", "2.", "1."]);
    assert_eq!(texts("<ol reversed start=10><li>a<li>b</ol>"), ["10.", "9."]);
}

#[test]
fn li_value_restarts_the_sequence() {
    assert_eq!(texts("<ol><li value=7>a<li>b<li value=2>c<li>d</ol>"), ["7.", "8.", "2.", "3."]);
    assert_eq!(texts("<ol reversed><li>a<li value=9>b<li>c</ol>"), ["3.", "9.", "8."]);
}

#[test]
fn type_attribute_picks_the_marker_style() {
    assert_eq!(texts("<ol type=a><li>x<li>y</ol>"), ["a.", "b."]);
    assert_eq!(texts("<ol type=I><li>x<li>y</ol>"), ["I.", "II."]);
    assert_eq!(texts("<ol><li type=i>x<li>y</ol>"), ["i.", "2."]);
}

#[test]
fn nested_lists_keep_independent_counters() {
    assert_eq!(
        texts("<ol><li>a<ol start=3><li>b<li>c</ol><li>d</ol>"),
        ["1.", "3.", "4.", "2."]
    );
}

#[test]
fn ul_nesting_uses_disc_circle_square() {
    let root = super::super::layout(
        &lumen_html_parser::parse("<ul><li>a<ul><li>b<ul><li>c</ul></ul></ul><ol><li><ul><li>d</ul></ol>"),
        &lumen_css_parser::parse(""),
        Size::new(800.0, 600.0),
    );
    let mut found = Vec::new();
    super::find_markers(&root, &mut found);
    let kinds: Vec<_> = found
        .iter()
        .map(|m| match &m.kind {
            BoxKind::Marker { list_style_type, .. } => list_style_type.clone(),
            _ => unreachable!(),
        })
        .collect();
    use crate::style::ListStyleType::{Circle, Decimal, Disc, Square};
    assert_eq!(kinds, [Disc, Circle, Square, Decimal, Circle]);
}

#[test]
fn author_list_style_type_beats_the_ua_default() {
    assert_eq!(markers("<ol><li>a<li>b</ol>", "ol { list-style-type: upper-roman }"), ["I.", "II."]);
}

#[test]
fn display_list_item_on_a_div_numbers_itself() {
    assert_eq!(
        markers(
            "<div><p>a</p><p>b</p></div>",
            "div { counter-reset: list-item } p { display: list-item; list-style-type: decimal }"
        ),
        ["1.", "2."]
    );
}
