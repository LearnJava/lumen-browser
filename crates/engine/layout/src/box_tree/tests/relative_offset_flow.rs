use lumen_core::geom::{Rect, Size};

// ── BUG-1240: position: relative не двигает соседей и не растит родителя ──

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn relative_top_does_not_shift_following_siblings() {
    let r = rects(
        r#"<div id="a" style="height:50px"></div><div id="b" style="height:20px;position:relative;top:7px"></div><div id="c" style="height:30px"></div>"#,
        "",
        &["a", "b", "c"],
    );
    assert_eq!((r[0].y, r[1].y, r[2].y), (0.0, 57.0, 70.0), "{r:?}");
}

#[test]
fn relative_child_does_not_grow_parent() {
    let r = rects(
        r#"<div id="p" style="position:relative"><div style="position:relative;top:50px;height:30px"></div></div>"#,
        "",
        &["p"],
    );
    assert_eq!(r[0].height, 30.0, "{r:?}");
}

// ── BUG-1422: top/bottom в процентах — от высоты содержащего блока ──

const PCT_BOX: &str = r#"<div id="c"><div id="a"></div></div>"#;

fn pct_y(container: &str, child: &str) -> f32 {
    let css = format!("#c{{width:300px;{container}}}#a{{position:relative;height:5px;width:5px;{child}}}");
    rects(PCT_BOX, &css, &["a"])[0].y
}

#[test]
fn relative_top_percent_uses_container_height() {
    assert_eq!(pct_y("height:100px", "top:10%"), 10.0);
}

#[test]
fn relative_bottom_percent_uses_container_height() {
    assert_eq!(pct_y("height:100px", "bottom:10%"), -10.0);
}

#[test]
fn relative_top_percent_with_auto_height_is_auto() {
    assert_eq!(pct_y("", "top:10%"), 0.0);
}

#[test]
fn relative_top_percent_with_auto_height_falls_back_to_bottom() {
    assert_eq!(pct_y("", "top:10%;bottom:7px"), -7.0);
}

#[test]
fn relative_top_calc_percent_uses_container_height() {
    assert_eq!(pct_y("height:100px", "top:calc(10px + 10%)"), 20.0);
}

#[test]
fn relative_left_percent_still_uses_container_width() {
    let css = "#c{width:300px;height:100px}#a{position:relative;height:5px;width:5px;left:10%}";
    assert_eq!(rects(PCT_BOX, css, &["a"])[0].x, 30.0);
}
