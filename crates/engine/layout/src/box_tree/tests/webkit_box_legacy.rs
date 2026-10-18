use lumen_core::geom::{Rect, Size};

// ── legacy flexbox: `-webkit-box-pack/-align/-flex/-direction/-ordinal-group`
// (WHATWG Compat §2.1, CSS-SPECS.md «legacy flexbox»). Items are fixed-size empty blocks. ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const TWO: &str = r#"<div id="c"><div id="a"></div><div id="b"></div></div>"#;
const BOX: &str = "#c{display:-webkit-box;width:300px;height:100px}\
                   #a,#b{width:50px;height:20px}";

fn xs(css: &str, html: &str) -> (f32, f32) {
    let r = rects(html, &format!("{BOX}{css}"), &["a", "b"]);
    (r[0].x, r[1].x)
}

#[test]
fn box_pack_maps_to_justify_content() {
    assert_eq!(xs("", TWO), (0.0, 50.0));
    assert_eq!(xs("#c{-webkit-box-pack:center}", TWO), (100.0, 150.0));
    assert_eq!(xs("#c{-webkit-box-pack:end}", TWO), (200.0, 250.0));
    assert_eq!(xs("#c{-webkit-box-pack:justify}", TWO), (0.0, 250.0));
}

#[test]
fn box_align_maps_to_align_items() {
    let ys = |css: &str| {
        let r = rects(TWO, &format!("{BOX}#a,#b{{height:auto}}#a{{height:20px}}{css}"), &["a", "b"]);
        (r[0].y, r[0].height, r[1].height)
    };
    // initial `stretch`: #b has no height, so it fills the box
    assert_eq!(ys(""), (0.0, 20.0, 100.0));
    assert_eq!(ys("#c{-webkit-box-align:start}"), (0.0, 20.0, 0.0));
    assert_eq!(ys("#c{-webkit-box-align:center}"), (40.0, 20.0, 0.0));
    assert_eq!(ys("#c{-webkit-box-align:end}"), (80.0, 20.0, 0.0));
}

#[test]
fn box_direction_reverse_flips_the_main_axis() {
    assert_eq!(xs("#c{-webkit-box-direction:reverse}", TWO), (250.0, 300.0 - 100.0));
    // reverse also flips what `start` means: `end` packs to the left
    assert_eq!(xs("#c{-webkit-box-direction:reverse;-webkit-box-pack:end}", TWO), (50.0, 0.0));
}

#[test]
fn box_orient_vertical_stacks_and_pack_works_on_the_vertical_axis() {
    let r = rects(TWO, &format!("{BOX}#c{{-webkit-box-orient:vertical;-webkit-box-pack:end}}"), &["a", "b"]);
    assert_eq!((r[0].y, r[1].y), (60.0, 80.0), "{r:?}");
}

#[test]
fn box_flex_distributes_free_space_and_default_does_not_shrink() {
    let html = r#"<div id="c"><div id="a" style="-webkit-box-flex:1"></div><div id="b" style="-webkit-box-flex:2"></div></div>"#;
    let r = rects(html, "#c{display:-webkit-box;width:300px}#a,#b{width:0;height:10px}", &["a", "b"]);
    assert_eq!((r[0].width, r[1].width), (100.0, 200.0), "{r:?}");
    // box-flex 0: wide items overflow instead of shrinking
    let r = rects(TWO, "#c{display:-webkit-box;width:60px}#a,#b{width:50px;height:10px}", &["a", "b"]);
    assert_eq!((r[0].width, r[1].width), (50.0, 50.0), "{r:?}");
}

#[test]
fn box_flex_overrides_flex_grow_in_a_legacy_box_only() {
    let html = r#"<div id="c"><div id="a" style="flex-grow:1"></div></div>"#;
    let legacy = rects(html, "#c{display:-webkit-box;width:300px}#a{height:10px}", &["a"]);
    assert_eq!(legacy[0].width, 0.0, "{legacy:?}");
    let plain = rects(html, "#c{display:flex;width:300px}#a{height:10px}", &["a"]);
    assert_eq!(plain[0].width, 300.0, "{plain:?}");
}

#[test]
fn box_ordinal_group_reorders_items() {
    let html = r#"<div id="c"><div id="a" style="-webkit-box-ordinal-group:2"></div><div id="b"></div></div>"#;
    assert_eq!(xs("", html), (50.0, 0.0));
}

#[test]
fn flex_wrap_is_ignored_in_a_legacy_box() {
    let r = rects(TWO, "#c{display:-webkit-box;flex-wrap:wrap;width:50px}#a,#b{width:50px;height:10px}", &["a", "b"]);
    assert_eq!((r[0].y, r[1].y), (0.0, 0.0), "{r:?}");
}
