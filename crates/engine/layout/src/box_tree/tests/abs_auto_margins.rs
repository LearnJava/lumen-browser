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
    aligned_rect_in("", css)
}

/// [`aligned_rect_of`] with extra declarations on the containing block, whose direction and
/// writing mode define the alignment axes (CSS Box Alignment L3 §5.3).
fn aligned_rect_in(container_css: &str, css: &str) -> Rect {
    let doc = lumen_html_parser::parse(r#"<div id="p"><div id="c"></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{position:relative;width:200px;height:100px;{container_css}}}#c{{position:absolute;inset:0;width:50px;height:50px;{css}}}"
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
    let r = aligned_rect_in("direction:rtl", "justify-self:end;margin-right:10px");
    // rtl containing block: the inline `end` edge is the left one.
    assert_eq!(r.x, 0.0, "{r:?}");
    // The box's own direction does not turn the containing block's axis round.
    let r = aligned_rect_of("justify-self:end;margin-right:10px;direction:rtl");
    assert_eq!(r.x, 140.0, "{r:?}");
    let r = aligned_rect_of("justify-self:self-end;margin-right:10px;direction:rtl");
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

// ── BUG-1526: auto size between insets is `fit-content` once the box is aligned ──

fn auto_sized_rect_of(container_css: &str, css: &str, inner_css: &str) -> Rect {
    let doc = lumen_html_parser::parse(r#"<div id="p"><div id="c"><div id="i"></div></div></div>"#);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{position:relative;width:200px;height:100px;{container_css}}}         #c{{position:absolute;inset:0;{css}}}#i{{width:30px;height:20px;{inner_css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn aligned_auto_size_is_fit_content_and_normal_stretches() {
    let r = auto_sized_rect_of("", "justify-self:center;align-self:end", "");
    assert_eq!((r.x, r.y, r.width, r.height), (85.0, 80.0, 30.0, 20.0), "{r:?}");
    let r = auto_sized_rect_of("", "justify-self:normal;align-self:stretch", "");
    assert_eq!((r.x, r.y, r.width, r.height), (0.0, 0.0, 200.0, 100.0), "{r:?}");
}

#[test]
fn aligned_axes_come_from_the_containing_block_not_the_box() {
    // vertical-lr container: `justify-self` runs down (y), `align-self` across (x).
    let r = auto_sized_rect_of("writing-mode:vertical-lr", "justify-self:end;align-self:center", "");
    assert_eq!((r.x, r.y), (85.0, 80.0), "{r:?}");
}

#[test]
fn default_overflow_keeps_the_box_in_the_containing_block_but_unsafe_does_not() {
    // Insets 20/10 leave 70px; the 95px box centred in them would start at 7.5 and end past the
    // 100px block: it is pulled back to y = 5. `unsafe` keeps the centred 7.5, `safe` starts at 20.
    let css = "top:20px;bottom:10px;left:0;right:0;";
    let inner = "height:95px;";
    let r = auto_sized_rect_of("", &format!("{css}align-self:center"), inner);
    assert_eq!(r.y, 5.0, "{r:?}");
    let r = auto_sized_rect_of("", &format!("{css}align-self:unsafe center"), inner);
    assert_eq!(r.y, 7.5, "{r:?}");
    let r = auto_sized_rect_of("", &format!("{css}align-self:safe center"), inner);
    assert_eq!(r.y, 20.0, "{r:?}");
}

// ── BUG-1482: «semi-replaced» form controls stretch between two insets (Position L3 §5.3) ──

fn control_rect(markup: &str, css: &str) -> Rect {
    let html = format!(r#"<div id="p">{markup}</div>"#);
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&format!(
        "body{{margin:0}}#p{{position:relative;width:200px;height:100px}}#c{{position:absolute;margin:0;{css}}}"
    ));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, "c").expect("нет #c").rect
}

#[test]
fn abs_form_controls_stretch_between_insets() {
    for markup in [
        r#"<button id="c">x</button>"#,
        r#"<input id="c" type="text">"#,
        r#"<input id="c" type="button" value="go">"#,
        r#"<select id="c"><option>a</option></select>"#,
        r#"<textarea id="c">t</textarea>"#,
    ] {
        let r = control_rect(markup, "inset:0;box-sizing:border-box");
        assert_eq!((r.x, r.y, r.width, r.height), (0.0, 0.0, 200.0, 100.0), "{markup}: {r:?}");
        let r = control_rect(markup, "top:3px;right:3px;bottom:3px;left:3px;box-sizing:border-box");
        assert_eq!((r.x, r.y, r.width, r.height), (3.0, 3.0, 194.0, 94.0), "{markup}: {r:?}");
    }
}

#[test]
fn abs_form_control_with_one_inset_keeps_content_size() {
    let r = control_rect(r#"<button id="c">x</button>"#, "left:0;top:0");
    assert!(r.width < 50.0 && r.height < 50.0, "{r:?}");
}
