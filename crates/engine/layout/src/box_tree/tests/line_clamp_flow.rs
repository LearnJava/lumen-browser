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
        r#"<div id="c">{}<br><div id="f" style="float:left;width:10px;height:10px"></div>{}</div>"#,
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

fn table_cell(id: &str) -> String {
    format!(
        r#"<table id="{id}" style="display:inline-table;border-spacing:0"><tr><td style="padding:0;width:10px;height:10px"></td></tr></table>"#
    )
}

#[test]
fn inline_table_is_an_atomic_inline_in_the_row() {
    // `display: inline-table` не рвёт ряд, как блочная `Table`: три таблицы через
    // `<br>` — три ряда, line-clamp: 2 оставляет два.
    let html = format!(
        r#"<div id="c">{}<br>{}<br>{}</div><div id="after"></div>"#,
        table_cell("t1"),
        table_cell("t2"),
        table_cell("t3")
    );
    let r = rects(&html, "#c{line-clamp:2}", &["c", "after"]);
    assert_eq!((r[0].height, r[1].y), (20.0, 20.0), "{r:?}");
    assert!(!has(&html, "#c{line-clamp:2}", "t3"));
    assert!(has(&html, "#c{line-clamp:2}", "t2"));
}

#[test]
fn inline_table_shares_a_line_with_its_siblings() {
    let html = format!(r#"<div id="c">{}{}</div>"#, table_cell("t1"), table_cell("t2"));
    let r = rects(&html, "", &["t1", "t2"]);
    assert_eq!((r[0].y, r[1].y), (0.0, 0.0), "{r:?}");
    assert_eq!(r[0].width, 10.0, "{r:?}");
    assert!(r[1].x >= r[0].x + 10.0, "{r:?}");
}

// ── text-wrap: balance после отсечки (LINE-CLAMP-BOX-2e) ──

/// Ширина символа — половина кегля (8 px при 16 px).
struct Fixed8;
impl crate::TextMeasurer for Fixed8 {
    fn char_width(&self, _: char, font_size_px: f32) -> f32 {
        font_size_px / 2.0
    }
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }
}

/// Тексты строк прогона внутри элемента `id` после раскладки.
fn run_lines(html: &str, css: &str, id: &str) -> Vec<String> {
    fn runs(b: &super::super::LayoutBox, out: &mut Vec<Vec<String>>) {
        if let super::super::BoxKind::InlineRun { lines, .. } = &b.kind {
            out.push(lines.iter().map(|l| l.iter().map(|f| f.text.as_str()).collect::<String>()).collect());
        }
        b.children.iter().for_each(|c| runs(c, out));
    }
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}div{{font-size:16px;line-height:20px}}{css}"));
    let root = super::super::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    let node = super::find_by_id_all(&root, &doc, id).expect("нет элемента");
    let mut out = Vec::new();
    runs(node, &mut out);
    out.concat().into_iter().map(|l| l.trim().to_string()).collect()
}

#[test]
fn nested_balance_works_on_the_visible_lines_only() {
    // Ширина 8 символов. Жадно блок даёт «1 2 3 4», «5 6 7 8», «9»; до отсечки на
    // третьей строке контейнера ему остаются две — их и балансируем, а не три.
    let html = r#"<div id="c">0<div id="b">1 2 3 4 5 6 7 8 9</div>10</div>"#;
    let css = "#c{line-clamp:3;width:65px}#b{text-wrap:balance}";
    assert_eq!(run_lines(html, css, "b"), ["1 2 3 4", "5 6 7 8…"]);
    // Без `line-clamp` те же девять чисел балансируются по всем трём строкам.
    let css = "#c{width:65px}#b{text-wrap:balance}";
    assert_eq!(run_lines(html, css, "b"), ["1 2 3", "4 5 6", "7 8 9"]);
}

#[test]
fn auto_clamp_balances_after_clamping_by_height() {
    // Высота 40 при строке 20 — помещаются две строки из трёх. Жадно: «1 2 3 4», «5 6 7 8»,
    // «9»; балансировка по всем трём дала бы «1 2 3», «4 5 6», «7 8 9».
    let html = r#"<div id="c">1 2 3 4 5 6 7 8 9</div>"#;
    let css = "#c{line-clamp:auto;width:65px;max-height:40px;text-wrap:balance}";
    assert_eq!(run_lines(html, css, "c"), ["1 2 3 4", "5 6 7 8…"]);
}

#[test]
fn float_past_the_content_edge_is_clipped_there() {
    // Contents of #c: a float 30 high at the top, then 4 rows of 10; clamp 2 → content edge y=20.
    let html = format!(
        r#"<div id="c"><div id="f" style="float:left;width:10px;height:30px"></div>{}</div>"#,
        rows(4)
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#c{line-height:0;line-clamp:2}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let f = super::find_by_id_all(&root, &doc, "f").expect("поплавок видимой строки пропал");
    let clip = f.clamp_clip.expect("поплавок, вылезающий за content edge, не обрезан");
    assert_eq!(clip.y + clip.height, 20.0, "{clip:?}");
}

#[test]
fn float_inside_the_content_edge_is_not_clipped() {
    let html = format!(
        r#"<div id="c"><div id="f" style="float:left;width:10px;height:10px"></div>{}</div>"#,
        rows(4)
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#c{line-height:0;line-clamp:2}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let f = super::find_by_id_all(&root, &doc, "f").expect("поплавок видимой строки пропал");
    assert!(f.clamp_clip.is_none());
}

#[test]
fn overflow_clip_container_does_not_enclose_its_float() {
    // `overflow: clip` is no BFC root (CSS Overflow L3 §3.1): the float sticks out of #c.
    let html = r#"<div id="c"><div style="float:left;width:10px;height:50px"></div><span style="display:inline-block;width:10px;height:10px"></span></div>"#;
    let clip = rects(html, "#c{overflow:clip}", &["c"]);
    let hidden = rects(html, "#c{overflow:hidden}", &["c"]);
    assert_eq!((clip[0].height, hidden[0].height), (10.0, 50.0), "{clip:?} {hidden:?}");
}

#[test]
fn clipped_part_of_a_float_is_not_scrollable_overflow() {
    let html = format!(
        r#"<div id="s" style="overflow:hidden;height:20px"><div id="c"><div style="float:left;width:10px;height:30px"></div>{}</div></div>"#,
        rows(4)
    );
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("body{margin:0}#c{line-height:0;line-clamp:2}");
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let s = super::find_by_id_all(&root, &doc, "s").expect("нет #s");
    assert_eq!(crate::content_height(s), 20.0);
}
