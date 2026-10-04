use lumen_core::geom::{Rect, Size};

// ── FLEX-VWM: физические оси flex-контейнера (CSS Flexbox §5.1 + Writing Modes §6.1) ──
//
// Пустые `div` с явными размерами: числа считаются руками, шрифт не нужен.

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

const ITEMS: &str = r#"<div id="a"></div><div id="b"></div><div id="c"></div>"#;
const ITEM_CSS: &str = "#a,#b,#c{width:50px;height:20px}";

fn run(container_css: &str) -> Vec<Rect> {
    let html = format!(r#"<div id="k">{ITEMS}</div>"#);
    rects(&html, &format!("#k{{display:flex;{container_css}}}{ITEM_CSS}"), &["a", "b", "c"])
}

fn xs(r: &[Rect]) -> Vec<f32> {
    r.iter().map(|r| r.x).collect()
}
fn ys(r: &[Rect]) -> Vec<f32> {
    r.iter().map(|r| r.y).collect()
}

#[test]
fn row_reverse_packs_at_the_right_edge() {
    // main-start у row-reverse — правый край: при свободном месте элементы
    // прижимаются вправо, первый в DOM — самый правый.
    let r = run("flex-direction:row-reverse;width:300px");
    assert_eq!(xs(&r), vec![250.0, 200.0, 150.0]);
}

#[test]
fn row_reverse_flex_end_goes_to_the_left_edge() {
    let r = run("flex-direction:row-reverse;justify-content:flex-end;width:300px");
    assert_eq!(xs(&r), vec![100.0, 50.0, 0.0]);
}

#[test]
fn rtl_row_starts_at_the_right() {
    let r = run("direction:rtl;width:300px");
    assert_eq!(xs(&r), vec![250.0, 200.0, 150.0]);
}

#[test]
fn rtl_with_row_reverse_cancels_out() {
    let r = run("direction:rtl;flex-direction:row-reverse;width:300px");
    assert_eq!(xs(&r), vec![0.0, 50.0, 100.0]);
}

#[test]
fn row_reverse_keeps_physical_margins() {
    // margin-left:10px остаётся левым полем при зеркалировании: рамка элемента
    // стоит на 10px правее левой кромки своего margin box.
    let html = r#"<div id="k"><div id="a"></div><div id="b"></div></div>"#;
    let r = rects(
        html,
        "#k{display:flex;flex-direction:row-reverse;width:300px}#a,#b{width:50px;height:20px;margin-left:10px}",
        &["a", "b"],
    );
    // margin box 60px: a — [240,300), b — [180,240).
    assert_eq!(r[0].x, 250.0, "{:?}", r[0]);
    assert_eq!(r[1].x, 190.0, "{:?}", r[1]);
}

#[test]
fn column_reverse_with_definite_height_packs_at_the_bottom() {
    let r = run("flex-direction:column-reverse;height:200px;width:100px");
    assert_eq!(ys(&r), vec![180.0, 160.0, 140.0]);
}

#[test]
fn column_reverse_with_auto_height_swaps_places() {
    let r = run("flex-direction:column-reverse;width:100px");
    assert_eq!(ys(&r), vec![40.0, 20.0, 0.0]);
}

#[test]
fn wrap_reverse_puts_the_first_line_at_the_bottom() {
    // Контейнер 100×100: линия 1 — a (60px не влезает рядом с b), линия 2 — b.
    let html = r#"<div id="k"><div id="a"></div><div id="b"></div></div>"#;
    let r = rects(
        html,
        "#k{display:flex;flex-wrap:wrap-reverse;align-content:flex-start;width:100px;height:100px}\
         #a,#b{width:60px;height:20px}",
        &["a", "b"],
    );
    assert_eq!(r[0].y, 80.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 60.0, "{:?}", r[1]);
}

#[test]
fn wrap_reverse_flex_start_alignment_hugs_the_bottom() {
    // cross-start при wrap-reverse — низ линии: `align-items:flex-start` ставит
    // элемент к нижней кромке своей линии.
    let html = r#"<div id="k"><div id="a"></div></div>"#;
    let r = rects(
        html,
        "#k{display:flex;flex-wrap:wrap-reverse;align-items:flex-start;width:100px;height:100px}\
         #a{width:60px;height:20px}",
        &["a"],
    );
    assert_eq!(r[0].y, 80.0, "{:?}", r[0]);
}

#[test]
fn rtl_column_aligns_to_the_right() {
    // У column cross-ось — inline: при rtl cross-start справа.
    let r = run("direction:rtl;flex-direction:column;width:200px;align-items:flex-start");
    assert_eq!(xs(&r), vec![150.0, 150.0, 150.0]);
    assert_eq!(ys(&r), vec![0.0, 20.0, 40.0]);
}

// ── вертикальные writing-mode ──

const VITEM_CSS: &str = "#a,#b,#c{width:20px;height:50px}";

fn run_v(container_css: &str) -> Vec<Rect> {
    let html = format!(r#"<div id="k">{ITEMS}</div>"#);
    rects(&html, &format!("#k{{display:flex;{container_css}}}{VITEM_CSS}"), &["a", "b", "c"])
}

#[test]
fn vertical_lr_row_runs_top_to_bottom() {
    let r = run_v("writing-mode:vertical-lr;width:100px;height:200px");
    assert_eq!(ys(&r), vec![0.0, 50.0, 100.0]);
    assert_eq!(xs(&r), vec![0.0, 0.0, 0.0]);
}

#[test]
fn vertical_rl_row_runs_top_to_bottom_from_the_right_edge() {
    // cross-start у vertical-rl — правый край.
    let r = run_v("writing-mode:vertical-rl;width:100px;height:200px");
    assert_eq!(ys(&r), vec![0.0, 50.0, 100.0]);
    assert_eq!(xs(&r), vec![80.0, 80.0, 80.0]);
}

#[test]
fn vertical_rl_row_reverse_runs_bottom_up() {
    let r = run_v("writing-mode:vertical-rl;flex-direction:row-reverse;width:100px;height:200px");
    assert_eq!(ys(&r), vec![150.0, 100.0, 50.0]);
}

#[test]
fn vertical_rl_rtl_row_starts_at_the_bottom() {
    let r = run_v("writing-mode:vertical-rl;direction:rtl;width:100px;height:200px");
    assert_eq!(ys(&r), vec![150.0, 100.0, 50.0]);
}

#[test]
fn sideways_lr_row_runs_bottom_to_top() {
    let r = run_v("writing-mode:sideways-lr;width:100px;height:200px");
    assert_eq!(ys(&r), vec![150.0, 100.0, 50.0]);
    assert_eq!(xs(&r), vec![0.0, 0.0, 0.0]);
}

#[test]
fn vertical_rl_column_runs_right_to_left() {
    // column в vertical-rl — по блочной оси: от правого края к левому.
    let r = run_v("writing-mode:vertical-rl;flex-direction:column;width:100px;height:200px");
    assert_eq!(xs(&r), vec![80.0, 60.0, 40.0]);
    assert_eq!(ys(&r), vec![0.0, 0.0, 0.0]);
}

#[test]
fn vertical_lr_column_runs_left_to_right() {
    let r = run_v("writing-mode:vertical-lr;flex-direction:column;width:100px;height:200px");
    assert_eq!(xs(&r), vec![0.0, 20.0, 40.0]);
}

#[test]
fn vertical_column_justify_end_packs_at_the_main_end() {
    // vertical-lr + column: main-end — правый край контейнера (100px).
    let r = run_v("writing-mode:vertical-lr;flex-direction:column;justify-content:flex-end;width:100px;height:200px");
    assert_eq!(xs(&r), vec![40.0, 60.0, 80.0]);
}

#[test]
fn vertical_row_align_items_center_centers_across_the_width() {
    let r = run_v("writing-mode:vertical-lr;align-items:center;width:100px;height:200px");
    assert_eq!(xs(&r), vec![40.0, 40.0, 40.0]);
}

#[test]
fn vertical_auto_width_wraps_the_single_line() {
    // width:auto — контейнер сжимается по содержимому: ширина линии 20px.
    let html = format!(r#"<div id="k">{ITEMS}</div>"#);
    let r = rects(
        &html,
        &format!("#k{{display:flex;writing-mode:vertical-lr;height:200px}}{VITEM_CSS}"),
        &["k", "a"],
    );
    assert_eq!(r[0].width, 20.0, "{:?}", r[0]);
    assert_eq!(r[0].height, 200.0, "{:?}", r[0]);
    assert_eq!(r[1].x, 0.0);
}

#[test]
fn vertical_column_auto_width_is_the_sum_of_items() {
    let html = format!(r#"<div id="k">{ITEMS}</div>"#);
    let r = rects(
        &html,
        &format!("#k{{display:flex;writing-mode:vertical-lr;flex-direction:column;height:200px}}{VITEM_CSS}"),
        &["k"],
    );
    assert_eq!(r[0].width, 60.0, "{:?}", r[0]);
}

#[test]
fn vertical_row_wraps_by_the_container_height() {
    // 3 × 50px в 120px-й колонке: два элемента на линию → вторая линия правее.
    let r = run_v("writing-mode:vertical-lr;flex-wrap:wrap;width:100px;height:120px;align-content:flex-start");
    assert_eq!(ys(&r), vec![0.0, 50.0, 0.0]);
    assert_eq!(xs(&r), vec![0.0, 0.0, 20.0]);
}

#[test]
fn vertical_rl_wrap_puts_the_second_line_to_the_left() {
    let r = run_v("writing-mode:vertical-rl;flex-wrap:wrap;width:100px;height:120px;align-content:flex-start");
    assert_eq!(ys(&r), vec![0.0, 50.0, 0.0]);
    assert_eq!(xs(&r), vec![80.0, 80.0, 60.0]);
}

// ── `safe` и `left`/`right` у justify-content/align-content ──

fn one_item(container_css: &str) -> Rect {
    let html = r#"<div id="k"><div id="a"></div></div>"#;
    rects(
        html,
        &format!("#k{{display:flex;width:90px;height:90px;{container_css}}}#a{{flex:0 0 100px;width:100px;height:100px}}"),
        &["a"],
    )[0]
}

#[test]
fn safe_start_in_wrap_reverse_falls_back_to_the_top() {
    // Без `safe` элемент прижат к cross-start (низу) и торчит вверх; с `safe`
    // при переполнении — к верху (writing-mode start).
    let r = one_item("flex-wrap:wrap-reverse;align-content:safe flex-start");
    assert_eq!(r.y, 0.0, "{r:?}");
    let r = one_item("flex-wrap:wrap-reverse;align-content:flex-start");
    assert_eq!(r.y, -10.0, "{r:?}");
}

#[test]
fn safe_start_in_row_reverse_falls_back_to_the_left() {
    let r = one_item("flex-direction:row-reverse;justify-content:safe flex-start");
    assert_eq!(r.x, 0.0, "{r:?}");
    let r = one_item("flex-direction:row-reverse;justify-content:flex-start");
    assert_eq!(r.x, -10.0, "{r:?}");
}

#[test]
fn safe_start_in_column_reverse_falls_back_to_the_top() {
    let r = one_item("flex-direction:column-reverse;justify-content:safe flex-start");
    assert_eq!(r.y, 0.0, "{r:?}");
}

#[test]
fn safe_does_not_change_alignment_without_overflow() {
    let r = run("flex-direction:row-reverse;justify-content:safe flex-start;width:300px");
    assert_eq!(xs(&r), vec![250.0, 200.0, 150.0]);
}

#[test]
fn justify_left_in_a_column_is_the_writing_mode_start() {
    // Вертикальная главная ось: `left` ведёт себя как `start` — верх, даже у
    // column-reverse (где flex-start — низ).
    let r = run("flex-direction:column-reverse;justify-content:left;height:200px;width:100px");
    assert_eq!(ys(&r), vec![40.0, 20.0, 0.0]);
}

#[test]
fn justify_left_and_right_are_physical_along_a_horizontal_axis() {
    let r = run("flex-direction:row-reverse;justify-content:left;width:300px");
    assert_eq!(xs(&r), vec![100.0, 50.0, 0.0]);
    let r = run("justify-content:right;width:300px");
    assert_eq!(xs(&r), vec![150.0, 200.0, 250.0]);
    let r = run("direction:rtl;justify-content:left;width:300px");
    assert_eq!(xs(&r), vec![100.0, 50.0, 0.0]);
}

// ── статическая позиция abspos-ребёнка (CSS Flexbox §4.1) ──

fn abs_child(container_css: &str, child_css: &str) -> Rect {
    let html = r#"<div id="k"><div id="a"></div></div>"#;
    rects(
        html,
        &format!(
            "#k{{display:flex;position:relative;width:100px;height:60px;{container_css}}}\
             #a{{position:absolute;width:10px;height:10px;{child_css}}}"
        ),
        &["a"],
    )[0]
}

#[test]
fn abspos_static_position_follows_justify_content_and_align_items() {
    let r = abs_child("justify-content:center;align-items:center", "");
    assert_eq!((r.x, r.y), (45.0, 25.0), "{r:?}");
    let r = abs_child("justify-content:flex-end;align-items:flex-end", "");
    assert_eq!((r.x, r.y), (90.0, 50.0), "{r:?}");
    let r = abs_child("justify-content:space-around", "");
    assert_eq!((r.x, r.y), (45.0, 0.0), "{r:?}");
}

#[test]
fn abspos_static_position_is_overridden_by_its_own_align_self_and_insets() {
    let r = abs_child("align-items:flex-end", "align-self:center");
    assert_eq!(r.y, 25.0, "{r:?}");
    // `left` задан — статическая позиция по x не используется.
    let r = abs_child("justify-content:flex-end", "left:5px");
    assert_eq!(r.x, 5.0, "{r:?}");
}

#[test]
fn abspos_static_position_uses_the_physical_axes() {
    // vertical-lr + row: главная ось — вертикаль, поперечная — горизонталь.
    let r = abs_child("writing-mode:vertical-lr;justify-content:flex-end;align-items:center", "");
    assert_eq!((r.x, r.y), (45.0, 50.0), "{r:?}");
    // row-reverse: flex-start — правый край.
    let r = abs_child("flex-direction:row-reverse", "");
    assert_eq!((r.x, r.y), (90.0, 0.0), "{r:?}");
    // Переполнение: unsafe end выступает за start-кромку, safe — прижимается к ней.
    let r = abs_child("justify-content:flex-end;width:5px", "");
    assert_eq!(r.x, -5.0, "{r:?}");
    let r = abs_child("justify-content:safe flex-end;width:5px", "");
    assert_eq!(r.x, 0.0, "{r:?}");
}

#[test]
fn justify_left_right_are_physical_along_the_inline_axis_and_start_along_the_block_axis() {
    // column (блочная ось): и `left`, и `right` — writing-mode start.
    let r = run("flex-direction:column;justify-content:right;height:200px;width:100px");
    assert_eq!(ys(&r), vec![0.0, 20.0, 40.0]);
    // row в вертикальном режиме (inline-ось вертикальна): left — верх, right — низ.
    let r = run_v("writing-mode:vertical-rl;justify-content:right;width:100px;height:200px");
    assert_eq!(ys(&r), vec![50.0, 100.0, 150.0]);
    let r = run_v("writing-mode:vertical-rl;justify-content:left;width:100px;height:200px");
    assert_eq!(ys(&r), vec![0.0, 50.0, 100.0]);
}

// ── `safe` в grid (общий разбор `AlignValue::parse_with_overflow`) ──

#[test]
fn grid_safe_alignment_falls_back_to_start_only_on_overflow() {
    let html = r#"<div id="g"><div id="big"></div><div id="small"></div></div>"#;
    let css = "#g{display:grid;grid-template:50px 50px / 50px;width:50px}\
               #big{width:100px;height:20px;justify-self:safe center}\
               #small{width:20px;height:20px;justify-self:safe center}";
    let r = rects(html, css, &["big", "small"]);
    assert_eq!(r[0].x, 0.0, "переполнение: к start-кромке, {:?}", r[0]);
    assert_eq!(r[1].x, 15.0, "влезает: центр, {:?}", r[1]);
}

// ── BUG-1265: self-start/self-end, safe у items, place-* ──

fn lone_item(container_css: &str, item_css: &str) -> Rect {
    let html = r#"<div id="k"><div id="a"></div></div>"#;
    rects(html, &format!("#k{{display:flex;{container_css}}}#a{{{item_css}}}"), &["a"])[0]
}

#[test]
fn start_follows_the_container_but_self_start_follows_the_item() {
    // rtl-колонка: cross-start — правая кромка. Элемент с direction:ltr
    // свою сторону start видит слева.
    let c = "flex-direction:column;direction:rtl;width:100px;height:50px";
    let i = "width:20px;height:10px;direction:ltr;";
    assert_eq!(lone_item(c, &format!("{i}align-self:start")).x, 80.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:self-start")).x, 0.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:self-end")).x, 80.0);
}

#[test]
fn self_start_of_a_vertical_item_in_a_row_looks_at_its_own_block_axis() {
    // У vertical-rl элемента block-start по горизонтали — правая кромка; по
    // вертикали его inline-start — верх.
    let c = "flex-direction:column;width:100px;height:50px";
    let i = "width:20px;height:10px;writing-mode:vertical-rl;";
    assert_eq!(lone_item(c, &format!("{i}align-self:self-start")).x, 80.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:self-end")).x, 0.0);
}

#[test]
fn safe_center_does_not_push_an_overflowing_row_item_past_the_start() {
    let c = "width:100px;height:20px";
    let i = "width:20px;height:40px;";
    assert_eq!(lone_item(c, &format!("{i}align-self:center")).y, -10.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:safe center")).y, 0.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:safe end")).y, 0.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:unsafe end")).y, -20.0);
}

#[test]
fn safe_center_does_not_push_an_overflowing_column_item_past_the_start() {
    let c = "flex-direction:column;width:20px;height:100px";
    let i = "width:40px;height:10px;";
    assert_eq!(lone_item(c, &format!("{i}align-self:center")).x, -10.0);
    assert_eq!(lone_item(c, &format!("{i}align-self:safe center")).x, 0.0);
}

#[test]
fn place_shorthands_carry_safe() {
    let c = "width:100px;height:20px";
    let i = "width:20px;height:40px;";
    assert_eq!(lone_item(c, &format!("{i}place-self:safe center")).y, 0.0);
    assert_eq!(lone_item(c, &format!("{i}place-self:center")).y, -10.0);
    let c2 = "width:100px;height:20px;place-items:safe center";
    assert_eq!(lone_item(c2, i).y, 0.0);
}

#[test]
fn place_content_takes_left_and_right_for_the_inline_axis() {
    // place-content: <align-content> <justify-content>; `right` — физическая сторона.
    let c = "width:100px;height:50px;place-content:start right";
    assert_eq!(lone_item(c, "width:20px;height:10px").x, 80.0);
}
