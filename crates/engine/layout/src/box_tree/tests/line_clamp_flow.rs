use lumen_core::geom::{Rect, Size};

// ── line-clamp по потоку контейнера (CSS Overflow L4 §line-clamp, LINE-CLAMP-BOX) ──
//
// Без метрик шрифта: строку задаёт пустой `inline-block` высотой 10 при
// `line-height:0` — ряд atomic inline равен 10.

fn span() -> &'static str {
    r#"<span style="display:inline-block;width:10px;height:10px"></span>"#
}

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}#c{{line-height:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

fn rows(n: usize) -> String {
    vec![span(); n].join("<br>")
}

#[test]
fn rows_of_atomic_inline_are_clamped_to_n_rows() {
    let html = format!(r#"<div id="c">{}</div><div id="after"></div>"#, rows(5));
    let r = rects(&html, "#c{line-clamp:3}", &["c", "after"]);
    assert_eq!((r[0].height, r[1].y), (30.0, 30.0), "{r:?}");
}

#[test]
fn clamp_not_exceeding_row_count_keeps_height() {
    let html = format!(r#"<div id="c">{}</div>"#, rows(3));
    let r = rects(&html, "#c{line-clamp:3}", &["c"]);
    assert_eq!(r[0].height, 30.0, "{r:?}");
}

#[test]
fn nested_blocks_count_toward_the_clamp() {
    let html = format!(r#"<div id="c"><div>{0}</div><div>{0}</div><div>{0}</div><div>{0}</div></div>"#, span());
    let r = rects(&html, "#c{line-clamp:2}", &["c"]);
    assert_eq!(r[0].height, 20.0, "{r:?}");
}

#[test]
fn clamp_cuts_inside_a_nested_block() {
    let html = format!(r#"<div id="c"><div>{}</div><div id="n">{}</div></div>"#, span(), rows(4));
    let r = rects(&html, "#c{line-clamp:3}", &["c", "n"]);
    assert_eq!(r[0].height, 30.0, "{r:?}");
}

#[test]
fn auto_clamps_to_the_rows_that_fit_the_height() {
    let html = format!(r#"<div id="c">{}</div><div id="after"></div>"#, rows(6));
    // max-height: 45 → помещаются четыре ряда по 10, пятый уже не влезает.
    let r = rects(&html, "#c{line-clamp:auto;max-height:45px}", &["c", "after"]);
    assert_eq!((r[0].height, r[1].y), (40.0, 40.0), "{r:?}");
}

#[test]
fn auto_keeps_the_specified_height() {
    let html = format!(r#"<div id="c">{}</div>"#, rows(6));
    let r = rects(&html, "#c{line-clamp:auto;height:45px}", &["c"]);
    assert_eq!(r[0].height, 45.0, "{r:?}");
}

#[test]
fn auto_without_a_definite_height_does_not_clamp() {
    let html = format!(r#"<div id="c">{}</div>"#, rows(6));
    let r = rects(&html, "#c{line-clamp:auto}", &["c"]);
    assert_eq!(r[0].height, 60.0, "{r:?}");
}

#[test]
fn nested_block_keeps_its_bottom_padding_under_the_clamp_line() {
    let html = format!(r#"<div id="c"><div id="n" style="padding-bottom:4px">{}</div></div>"#, rows(4));
    let r = rects(&html, "#c{line-clamp:2}", &["c", "n"]);
    assert_eq!((r[1].height, r[0].height), (24.0, 24.0), "{r:?}");
}

#[test]
fn block_without_lines_after_the_clamp_point_is_hidden() {
    let html = format!(r#"<div id="c">{}<div id="g" style="height:30px"></div></div>"#, rows(2));
    let r = rects(&html, "#c{line-clamp:2}", &["c"]);
    assert_eq!(r[0].height, 20.0, "{r:?}");
}

#[test]
fn clamp_not_reached_keeps_the_trailing_block() {
    let html = format!(r#"<div id="c">{}<div style="height:30px"></div></div>"#, rows(2));
    let r = rects(&html, "#c{line-clamp:3}", &["c"]);
    assert_eq!(r[0].height, 50.0, "{r:?}");
}

#[test]
fn legacy_prefixed_clamp_needs_a_vertical_webkit_box() {
    let html = format!(r#"<div id="c">{}</div>"#, rows(5));
    // Без `display:-webkit-box` и вертикальной оси `-webkit-line-clamp` не действует.
    let plain = rects(&html, "#c{-webkit-line-clamp:3}", &["c"]);
    assert_eq!(plain[0].height, 50.0, "{plain:?}");
    let horizontal = rects(&html, "#c{display:-webkit-box;-webkit-line-clamp:3}", &["c"]);
    assert_eq!(horizontal[0].height, 50.0, "{horizontal:?}");
    let boxed = rects(&html, "#c{display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:3}", &["c"]);
    assert_eq!(boxed[0].height, 30.0, "{boxed:?}");
}

#[test]
fn unprefixed_clamp_wins_over_an_earlier_prefixed_one() {
    let html = format!(r#"<div id="c">{}</div>"#, rows(5));
    let r = rects(&html, "#c{-webkit-line-clamp:3;line-clamp:2}", &["c"]);
    assert_eq!(r[0].height, 20.0, "{r:?}");
}

#[test]
fn float_below_the_clamp_line_is_hidden_with_its_line() {
    let html = format!(
        r#"<div id="c">{}<div id="f" style="float:left;width:10px;height:10px"></div>{}</div>"#,
        rows(2),
        rows(2)
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#c{line-height:0;line-clamp:2}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    assert!(super::find_by_id_all(&root, &doc, "f").is_none(), "поплавок скрытой строки остался");
}

#[test]
fn float_above_the_clamp_line_stays() {
    let html = format!(
        r#"<div id="c"><div id="f" style="float:left;width:10px;height:10px"></div>{}</div>"#,
        rows(4)
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#c{line-height:0;line-clamp:2}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    assert!(super::find_by_id_all(&root, &doc, "f").is_some(), "поплавок видимой строки пропал");
}

fn has(html: &str, css: &str, id: &str) -> bool {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}#c{{line-height:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    super::find_by_id_all(&root, &doc, id).is_some()
}

#[test]
fn wrapped_row_of_atomic_inline_counts_its_visual_lines() {
    // Width 25 holds two 10px items: five of them wrap into three lines of one `InlineBlockRow`.
    let item = |id: &str| format!(r#"<span id="{id}" style="display:inline-block;width:10px;height:10px"></span>"#);
    let html = format!(r#"<div id="c">{}{}{}{}{}</div><div id="after"></div>"#, item("i1"), item("i2"), item("i3"), item("i4"), item("i5"));
    let r = rects(&html, "#c{width:25px;line-clamp:2}", &["c", "after"]);
    assert_eq!((r[0].height, r[1].y), (20.0, 20.0), "{r:?}");
    assert!(has(&html, "#c{width:25px;line-clamp:2}", "i4"), "вторая строка пропала");
    assert!(!has(&html, "#c{width:25px;line-clamp:2}", "i5"), "третья строка осталась");
    assert!(has(&html, "#c{width:25px;line-clamp:3}", "i5"), "строка внутри лимита скрыта");
}

#[test]
fn count_and_auto_clamp_to_the_earlier_limit() {
    let row = |id: &str| format!(r#"<span id="{id}" style="display:inline-block;width:10px;height:10px"></span>"#);
    let html = format!(r#"<div id="c">{}<br>{}<br>{}<br>{}</div>"#, row("r1"), row("r2"), row("r3"), row("r4"));
    // The height holds two lines, the count three: the height wins …
    assert!(!has(&html, "#c{line-clamp:3 auto;max-height:25px}", "r3"));
    assert!(has(&html, "#c{line-clamp:3 auto;max-height:25px}", "r2"));
    // … the count is smaller than what the height holds, so the count wins; either order.
    assert!(!has(&html, "#c{line-clamp:auto 2;max-height:100px}", "r3"));
    assert!(has(&html, "#c{line-clamp:2 auto;max-height:100px}", "r2"));
    // No definite height — only the count limits.
    assert!(!has(&html, "#c{line-clamp:3 auto}", "r4"));
    assert!(has(&html, "#c{line-clamp:3 auto}", "r3"));
}
