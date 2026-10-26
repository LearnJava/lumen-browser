//! BUG-1528: `justify-self` / `align-self` of an abspos child of a block container whose
//! insets are `auto` — the static position rectangle is the content box along x and a point
//! (the in-flow position) along y.

use lumen_core::geom::Size;

/// `(x, y)` of `#a` for a 50×50 abspos box inside a block with a 75×50 content box at (6, 6).
fn pos_of(block_css: &str, abs_css: &str) -> (f32, f32) {
    let doc = lumen_html_parser::parse(r#"<div id="c"><div id="b"><div id="a"></div></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#c{{position:relative;border:1px solid;width:100px;height:100px}}\
         #b{{width:75px;height:50px;border:5px solid;{block_css}}}\
         #a{{position:absolute;width:50px;height:50px;{abs_css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let r = super::find_by_id_all(&root, &doc, "a").expect("нет #a").rect;
    (r.x, r.y)
}

#[test]
fn start_keeps_the_static_position() {
    assert_eq!(pos_of("", "justify-self:start;align-self:start"), (6.0, 6.0));
    assert_eq!(pos_of("", ""), (6.0, 6.0));
}

#[test]
fn end_aligns_to_the_inline_end_and_above_the_static_point() {
    assert_eq!(pos_of("", "justify-self:end;align-self:end"), (31.0, -44.0));
}

#[test]
fn center_splits_the_free_space() {
    assert_eq!(pos_of("", "justify-self:center;align-self:center"), (18.5, -19.0));
}

#[test]
fn only_the_auto_inset_axis_is_aligned() {
    // `left: 0` + `right: auto` is not a static position along x.
    assert_eq!(pos_of("", "left:0;justify-self:end;align-self:end").1, -44.0);
    assert_eq!(pos_of("", "top:0;justify-self:end;align-self:end").0, 31.0);
}

#[test]
fn justify_items_of_a_block_applies_but_align_items_does_not() {
    assert_eq!(pos_of("justify-items:center;align-items:center", ""), (18.5, 6.0));
}

#[test]
fn rtl_inline_end_is_the_left_edge() {
    let (x, _) = pos_of("direction:rtl", "justify-self:end");
    assert_eq!(x, 6.0);
}

#[test]
fn insets_on_the_other_axis_align_inside_the_containing_block() {
    // `top: 0; bottom: 0` + `align-self: end` aligns inside the positioned ancestor (a second
    // placement through the static wrapper); the inline axis stays a static position.
    assert_eq!(pos_of("", "top:0;bottom:0;align-self:end;justify-self:end"), (31.0, 51.0));
    assert_eq!(pos_of("", "top:0;bottom:0;align-self:center;justify-self:center"), (18.5, 26.0));
}
