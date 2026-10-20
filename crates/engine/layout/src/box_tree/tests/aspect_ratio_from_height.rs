use lumen_core::geom::{Rect, Size};

// ── `aspect-ratio` при `width: auto` и определённой высоте (BUG-1385) ──
//
// Каждый случай — разметка WPT `css/css-sizing/aspect-ratio`, где ширина не выводилась из высоты.

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn size(html: &str, css: &str, id: &str) -> (f32, f32) {
    let r = rects(html, css, &[id])[0];
    (r.width, r.height)
}

const ONE: &str = r#"<div id="o"><div id="a"></div></div>"#;

#[test]
fn block_takes_width_from_height() {
    assert_eq!(size(ONE, "#a{height:100px;aspect-ratio:1/1}", "a"), (100.0, 100.0));
}

#[test]
fn block_content_box_adds_padding_to_ratio_width() {
    assert_eq!(size(ONE, "#a{height:100px;aspect-ratio:2/1;padding:0 10px}", "a"), (220.0, 100.0));
}

#[test]
fn block_with_height_and_max_width_is_clamped() {
    let (w, _) = size(ONE, "#a{height:100px;aspect-ratio:1/1;max-width:60px}", "a");
    assert_eq!(w, 60.0);
}

#[test]
fn block_with_both_sizes_ignores_ratio() {
    assert_eq!(size(ONE, "#a{height:100px;width:30px;aspect-ratio:1/1}", "a"), (30.0, 100.0));
}

#[test]
fn block_with_indefinite_height_is_stretched() {
    // `height: 50%` of an auto-height parent is not definite: the width stays `auto`.
    assert_eq!(size(ONE, "#a{height:50%;aspect-ratio:1/1}", "a").0, 800.0);
}

#[test]
fn block_wider_contents_raise_the_automatic_minimum_width() {
    let html = r#"<div id="o"><div id="a"><div style="width:100px;height:1px"></div></div></div>"#;
    assert_eq!(size(html, "#a{height:100px;aspect-ratio:1/2}", "a"), (100.0, 100.0));
}

#[test]
fn scroll_container_has_no_automatic_minimum_width() {
    let html = r#"<div id="o"><div id="a"><div style="width:100px;height:1px"></div></div></div>"#;
    assert_eq!(size(html, "#a{height:100px;aspect-ratio:1/2;overflow:hidden}", "a"), (50.0, 100.0));
}

#[test]
fn float_in_zero_wide_container_takes_ratio_width() {
    let html = r#"<div style="width:0"><div id="a"></div></div>"#;
    assert_eq!(size(html, "#a{float:left;height:100px;aspect-ratio:1/1}", "a"), (100.0, 100.0));
}

#[test]
fn grid_item_takes_ratio_width_and_stretch_fills_the_area() {
    let html = r#"<div id="g"><div id="a"></div></div>"#;
    assert_eq!(size(html, "#g{display:grid}#a{height:100px;aspect-ratio:1/1}", "a"), (100.0, 100.0));
    let css = "#g{display:grid;grid-template:50px/100px}#a{height:50px;aspect-ratio:1/2;justify-self:stretch}";
    assert_eq!(size(html, css, "a"), (100.0, 50.0));
}

#[test]
fn abspos_with_one_vertical_stretch_takes_ratio_width() {
    let html = r#"<div id="o"><div id="a"></div></div>"#;
    let css = "#o{position:relative;width:300px;height:50px}#a{position:absolute;left:0;top:0;bottom:0;aspect-ratio:2/1}";
    assert_eq!(size(html, css, "a"), (100.0, 50.0));
}

#[test]
fn abspos_stretched_on_both_axes_keeps_the_width_and_follows_the_ratio() {
    let html = r#"<div id="o"><div id="a"></div></div>"#;
    let css = "#o{position:relative;width:100px;height:500px}#a{position:absolute;inset:0;aspect-ratio:1/1}";
    assert_eq!(size(html, css, "a"), (100.0, 100.0));
}

#[test]
fn auto_ratio_form_works_like_ratio_on_a_box_without_natural_ratio() {
    assert_eq!(size(ONE, "#a{width:100px;aspect-ratio:auto 4/1}", "a"), (100.0, 25.0));
    assert_eq!(size(ONE, "#a{width:100px;aspect-ratio:4/1 auto}", "a"), (100.0, 25.0));
    assert_eq!(size(ONE, "#a{width:100px;aspect-ratio:auto}", "a").1, 0.0);
}

#[test]
fn svg_with_view_box_derives_the_auto_axis_from_the_ratio() {
    let html = r#"<div><svg id="a" height="40" viewBox="0 0 4 1"></svg><svg id="b" width="40" viewBox="0 0 4 1"></svg></div>"#;
    let r = rects(html, "svg{display:block}", &["a", "b"]);
    assert_eq!((r[0].width, r[0].height), (160.0, 40.0));
    assert_eq!((r[1].width, r[1].height), (40.0, 10.0));
}

#[test]
fn grid_item_stretched_on_both_axes_ignores_the_ratio() {
    let html = r#"<div id="g"><div id="a"></div></div>"#;
    let css = "#g{display:grid;grid-template:100px/100px}#a{aspect-ratio:2/1;align-self:stretch;justify-self:stretch}";
    assert_eq!(size(html, css, "a"), (100.0, 100.0));
}

#[test]
fn fit_content_parent_grows_to_the_contents_minimum_of_a_ratio_child() {
    let html = r#"<div id="p"><div id="a"><div style="width:100px;height:1px"></div></div></div>"#;
    let css = "#p{width:fit-content}#a{aspect-ratio:1/2;height:100px}";
    assert_eq!(size(html, css, "p").0, 100.0);
}

#[test]
fn abspos_with_authored_width_takes_height_from_the_ratio_between_block_insets() {
    let html = r#"<div id="o"><div id="a"></div></div>"#;
    let css = "#o{position:relative;width:100px;height:500px}#a{position:absolute;inset:0;width:100px;aspect-ratio:1/1}";
    assert_eq!(size(html, css, "a"), (100.0, 100.0));
}

#[test]
fn abspos_with_authored_height_takes_width_from_the_ratio_between_inline_insets() {
    let html = r#"<div id="o"><div id="a"></div></div>"#;
    let css = "#o{position:relative;width:500px;height:100px}#a{position:absolute;inset:0;height:100px;aspect-ratio:1/1}";
    assert_eq!(size(html, css, "a"), (100.0, 100.0));
}
