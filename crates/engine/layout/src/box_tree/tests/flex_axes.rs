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
