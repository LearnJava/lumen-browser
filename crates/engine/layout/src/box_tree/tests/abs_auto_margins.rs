use lumen_core::geom::{Rect, Size};

// ── BUG-1282: auto-поля абсолютного бокса с двумя инсетами по оси ──

fn rect_of(css: &str) -> Rect {
    let doc = lumen_html_parser::parse(r#"<div id="p"><div id="c"></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{position:relative;width:100px;height:100px}}#c{{position:absolute;width:70px;height:70px;{css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn inset_zero_margin_auto_centers() {
    let r = rect_of("inset:0;margin:auto");
    assert_eq!((r.x, r.y), (15.0, 15.0), "{r:?}");
}

#[test]
fn horizontal_auto_margins_split_evenly() {
    let r = rect_of("left:0;right:0;margin:0 auto");
    assert_eq!((r.x, r.y), (15.0, 0.0), "{r:?}");
}

#[test]
fn single_auto_margin_takes_remainder() {
    let r = rect_of("left:0;right:0;margin-left:auto;margin-right:5px");
    assert_eq!(r.x, 25.0, "{r:?}");
}

#[test]
fn no_auto_margin_stays_at_start() {
    let r = rect_of("left:0;right:0;margin-left:3px");
    assert_eq!(r.x, 3.0, "{r:?}");
}

// ── BUG-1360: статическая позиция abspos при `direction: rtl` ──

fn rtl_rect_of(css: &str) -> Rect {
    let doc = lumen_html_parser::parse(r#"<div id="p"><div id="c"></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{direction:rtl;position:relative;width:200px;height:100px}}#c{{position:absolute;width:20px;height:20px;{css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn rtl_static_position_is_at_right_edge() {
    let r = rtl_rect_of("");
    assert_eq!((r.x, r.y), (180.0, 0.0), "{r:?}");
}

#[test]
fn rtl_static_position_respects_margin_right() {
    let r = rtl_rect_of("margin:0 5px 0 30px");
    assert_eq!(r.x, 175.0, "{r:?}");
}

#[test]
fn rtl_explicit_left_still_wins() {
    let r = rtl_rect_of("left:10px");
    assert_eq!(r.x, 10.0, "{r:?}");
}

// ── BUG-1481: `justify-self` / `align-self` / `place-self` выравнивают бокс между инсетами ──

fn aligned_rect_of(css: &str) -> Rect {
    let doc = lumen_html_parser::parse(r#"<div id="p"><div id="c"></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{position:relative;width:200px;height:100px}}#c{{position:absolute;inset:0;width:50px;height:50px;{css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn self_alignment_center_center() {
    let r = aligned_rect_of("justify-self:center;align-self:center");
    assert_eq!((r.x, r.y), (75.0, 25.0), "{r:?}");
}

#[test]
fn self_alignment_end_end() {
    let r = aligned_rect_of("justify-self:end;align-self:end");
    assert_eq!((r.x, r.y), (150.0, 50.0), "{r:?}");
}

#[test]
fn place_self_sets_both_axes() {
    let r = aligned_rect_of("place-self:end center");
    assert_eq!((r.x, r.y), (75.0, 50.0), "{r:?}");
}

#[test]
fn self_alignment_counts_margins_and_rtl_flips_start() {
    let r = aligned_rect_of("justify-self:end;margin-right:10px;direction:rtl");
    // rtl: the inline `end` edge is the left one.
    assert_eq!(r.x, 0.0, "{r:?}");
    let r = aligned_rect_of("justify-self:end;margin-right:10px");
    assert_eq!(r.x, 140.0, "{r:?}");
}

#[test]
fn self_alignment_yields_to_auto_margins_and_normal_stays_at_start() {
    let r = aligned_rect_of("justify-self:end;margin:auto");
    assert_eq!((r.x, r.y), (75.0, 25.0), "{r:?}");
    let r = aligned_rect_of("justify-self:normal;align-self:stretch");
    assert_eq!((r.x, r.y), (0.0, 0.0), "{r:?}");
}

#[test]
fn safe_center_overflow_falls_back_to_start() {
    let r = aligned_rect_of("width:300px;justify-self:safe center");
    assert_eq!(r.x, 0.0, "{r:?}");
}
