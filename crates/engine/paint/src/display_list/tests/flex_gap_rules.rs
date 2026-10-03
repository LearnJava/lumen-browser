//! CSS Gap Decorations L1 в flex-контейнерах (BUG-553, срез 6): щели главной оси — между
//! элементами одной flex-строки, поперечной — между строками; `*-rule-break: intersection`
//! и `*-rule-inset-*` по §3.1.2–§3.3. Числа взяты из WPT `css/css-gaps/flex/`.

use super::text_and_images::build;
use super::*;

/// Линии правил: `(x, y, w, h)` отрезка, по возрастанию `(y, x)`; `vertical` — колоночные.
fn rules(dl: &DisplayList, vertical: bool) -> Vec<(f32, f32, f32, f32)> {
    let mut v: Vec<_> = dl
        .iter()
        .filter_map(|c| match c {
            DisplayCommand::DrawBorder { rect, widths: [0.0, w, 0.0, 0.0], .. } if vertical && *w > 0.0 => {
                Some((rect.x, rect.y, rect.width, rect.height))
            }
            DisplayCommand::DrawBorder { rect, widths: [0.0, 0.0, h, 0.0], .. } if !vertical && *h > 0.0 => {
                Some((rect.x, rect.y, rect.width, rect.height))
            }
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap());
    v
}

fn close(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    (a.0 - b.0).abs() < 0.6 && (a.1 - b.1).abs() < 0.6 && (a.2 - b.2).abs() < 0.6 && (a.3 - b.3).abs() < 0.6
}

fn assert_rules(got: &[(f32, f32, f32, f32)], want: &[(f32, f32, f32, f32)]) {
    assert_eq!(got.len(), want.len(), "got {got:?}, want {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(close(*g, *w), "got {got:?}, want {want:?}");
    }
}

/// flex-gap-decorations-030: junction-вставки режут отрезки у стыка, cap-вставка — у края.
#[test]
fn flex_intersection_with_junction_and_cap_insets() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;gap:20px;width:340px;
        row-rule:3px solid gray;row-rule-break:intersection;
        row-rule-inset-junction-start:0;row-rule-inset-junction-end:1px;
        column-rule:3px solid red;column-rule-break:intersection;
        column-rule-inset-junction-start:0;column-rule-inset-junction-end:-6px;
        column-rule-inset-cap-end:-8px">
        <div style="width:200px;height:100px"></div><div style="width:100px;height:100px"></div>
        <div style="width:100px;height:100px"></div><div style="width:100px;height:100px"></div>
        <div style="width:100px;height:100px"></div></div>"#;
    let dl = build(html, "");
    assert_rules(
        &rules(&dl, false),
        &[(0.0, 108.5, 99.0, 3.0), (120.0, 108.5, 79.0, 3.0), (240.0, 108.5, 100.0, 3.0)],
    );
    assert_rules(
        &rules(&dl, true),
        &[(208.5, 0.0, 3.0, 106.0), (108.5, 120.0, 3.0, 108.0), (228.5, 120.0, 3.0, 108.0)],
    );
}

/// flex-gap-decorations-034: стыки разных строк не совпадают, поперечная щель режется на каждом.
#[test]
fn flex_row_rule_cut_at_junctions_of_both_lines() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;width:600px;gap:90px;
        column-rule:5px solid blue;row-rule:5px solid red;
        column-rule-break:intersection;column-rule-inset:0;
        row-rule-break:intersection;row-rule-inset:0">
        <div style="width:60px;height:100px"></div><div style="width:80px;height:100px"></div>
        <div style="width:70px;height:100px"></div><div style="width:120px;height:100px"></div>
        <div style="width:240px;height:100px"></div><div style="width:270px;height:100px"></div></div>"#;
    let dl = build(html, "");
    assert_rules(
        &rules(&dl, false),
        &[(0.0, 142.5, 60.0, 5.0), (150.0, 142.5, 80.0, 5.0), (330.0, 142.5, 60.0, 5.0), (480.0, 142.5, 120.0, 5.0)],
    );
    assert_rules(
        &rules(&dl, true),
        &[(102.5, 0.0, 5.0, 100.0), (272.5, 0.0, 5.0, 100.0), (432.5, 0.0, 5.0, 100.0), (282.5, 190.0, 5.0, 100.0)],
    );
}

/// flex-gap-decorations-009/010: главная щель занимает ровно свою flex-строку, щели соседних
/// строк не сливаются; строковая линия идёт на всю ширину.
#[test]
fn flex_main_gaps_stay_within_their_line() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;border:2px solid #000;width:170px;
        column-gap:10px;row-gap:10px;
        column-rule:10px solid red;row-rule:10px solid blue">
        ITEMS</div>"#
        .replace("ITEMS", ITEMS);
    let dl = build(&html, "");
    assert_rules(
        &rules(&dl, true),
        &[(52.0, 2.0, 10.0, 50.0), (112.0, 2.0, 10.0, 50.0), (102.0, 62.0, 10.0, 50.0), (52.0, 122.0, 10.0, 50.0), (112.0, 122.0, 10.0, 50.0)],
    );
    assert_rules(&rules(&dl, false), &[(2.0, 52.0, 170.0, 10.0), (2.0, 112.0, 170.0, 10.0)]);
}

/// §4.6: значения списка раздаются щелям сквозь все flex-строки и не перезапускаются на каждой.
#[test]
fn flex_rule_value_list_continues_across_lines() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;width:170px;column-gap:10px;row-gap:10px;
        column-rule-style:solid;column-rule-width:2px,4px,6px">ITEMS</div>"#
        .replace("ITEMS", ITEMS);
    let dl = build(&html, "");
    // Щели по порядку: строка 1 — 0, 1; строка 2 — 2; строка 3 — 3, 4 (цикл 2, 4, 6, 2, 4).
    let widths: Vec<f32> = rules(&dl, true).iter().map(|r| r.2).collect();
    assert_eq!(widths, vec![2.0, 4.0, 6.0, 2.0, 4.0], "{widths:?}");
}

/// `column`: главная ось вертикальная — щели между элементами рисует `row-rule`, а не `column-rule`.
#[test]
fn flex_column_direction_swaps_axes() {
    let html = r#"<div style="display:flex;flex-direction:column;width:120px;height:170px;
        row-gap:10px;row-rule:2px solid blue;column-rule:4px solid red">
        <div style="height:50px"></div><div style="height:50px"></div><div style="height:50px"></div></div>"#;
    let dl = build(html, "");
    assert_rules(&rules(&dl, false), &[(0.0, 54.0, 120.0, 2.0), (0.0, 114.0, 120.0, 2.0)]);
    assert!(rules(&dl, true).is_empty(), "column-rule не рисуется в одной колонке");
}

const ITEMS: &str = r#"<div style="width:50px;height:50px"></div><div style="width:50px;height:50px"></div>
    <div style="width:50px;height:50px"></div><div style="width:100px;height:50px"></div>
    <div style="width:50px;height:50px"></div><div style="width:50px;height:50px"></div>
    <div style="width:50px;height:50px"></div><div style="width:50px;height:50px"></div>"#;

/// flex-gap-decorations-065/066: cap-вставки отрицательной величины вытягивают линии за край
/// контейнера; стык с поперечной щелью остаётся стыком (junction), а не cap.
#[test]
fn flex_negative_cap_insets_extend_past_container_edges() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;gap:10px;width:320px;
        column-rule:5px solid blue;row-rule:5px solid red;
        row-rule-inset-cap-end:-100px;column-rule-inset-cap-end:-100px">
        ITEMS</div>"#
        .replace("ITEMS", &"<div style=\"width:100px;height:100px\"></div>".repeat(9));
    let dl = build(&html, "");
    assert_rules(
        &rules(&dl, true),
        &[
            (102.5, 0.0, 5.0, 100.0),
            (212.5, 0.0, 5.0, 100.0),
            (102.5, 110.0, 5.0, 100.0),
            (212.5, 110.0, 5.0, 100.0),
            (102.5, 220.0, 5.0, 200.0),
            (212.5, 220.0, 5.0, 200.0),
        ],
    );
    assert_rules(&rules(&dl, false), &[(0.0, 102.5, 420.0, 5.0), (0.0, 212.5, 420.0, 5.0)]);
}

/// `direction: rtl` зеркалит вставки строковой линии: «начало» — у правого края.
#[test]
fn flex_rtl_mirrors_row_rule_insets() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;gap:20px;width:340px;direction:rtl;
        row-rule:6px solid red;row-rule-break:intersection;
        row-rule-inset-start:10px;row-rule-inset-end:0">
        ITEMS</div>"#
        .replace("ITEMS", &"<div style=\"width:100px;height:100px\"></div>".repeat(6));
    let dl = build(&html, "");
    // Три элемента в строке справа налево: 240..340, 120..220, 0..100; отрезок строковой щели
    // под первым элементом строки укорочен слева-направо справа (start = right) на 10px.
    let rows = rules(&dl, false);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(close(rows[0], (0.0, 107.0, 90.0, 6.0)), "{rows:?}");
}
