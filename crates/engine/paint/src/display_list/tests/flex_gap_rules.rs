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

/// BUG-553 срез 11: анимированные `*-rule-width` / `*-rule-color` (перекрытие от
/// планировщика) перекрашивают и утолщают линии щелей без повторной раскладки — и в
/// упорядоченном (живом) пути, и в `walk_with_anim`.
#[test]
fn animated_gap_rule_override_changes_width_and_colour() {
    let html = r#"<div style="display:flex;gap:20px;width:300px;column-rule:2px solid red;
        row-rule:2px solid red"><div style="width:100px;height:50px"></div>
        <div style="width:100px;height:50px"></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_flex(b: &lumen_layout::LayoutBox) -> Option<lumen_dom::NodeId> {
        if b.style.display == lumen_layout::Display::Flex {
            return Some(b.node);
        }
        b.children.iter().find_map(find_flex)
    }
    let node = find_flex(&tree).expect("flex container");

    let mut gap_rules = lumen_layout::GapRuleOverride::default();
    assert!(gap_rules.set("column-rule-width", "6px"));
    assert!(gap_rules.set("column-rule-color", "rgb(0, 0, 255)"));
    let mut overrides = HashMap::new();
    overrides.insert(
        node,
        CompositorOverride { gap_rules: Some(gap_rules), ..Default::default() },
    );
    let frame = CompositorAnimFrame { overrides, has_active: true };

    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let check = |dl: &DisplayList, what: &str| {
        let cols = rules(dl, true);
        assert_eq!(cols.len(), 1, "{what}: {cols:?}");
        assert!((cols[0].2 - 6.0).abs() < 0.1, "{what}: width {cols:?}");
        assert!(
            dl.iter().any(|c| matches!(c,
                DisplayCommand::DrawBorder { colors, widths: [0.0, w, 0.0, 0.0], .. }
                if *w > 0.0 && colors[1] == blue)),
            "{what}: colour"
        );
    };

    let stacking_tree = lumen_layout::StackingTree::build(&tree);
    let order = lumen_layout::PaintOrder::from_tree(&stacking_tree);
    let base = build_display_list_ordered(&tree, &stacking_tree, &order).0;
    let base_cols = rules(&base, true);
    assert!((base_cols[0].2 - 2.0).abs() < 0.1, "base {base_cols:?}");
    let ordered = build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));
    check(&ordered, "ordered");
    let walked = build_display_list_with_anim(&tree, Some(&frame));
    check(&walked, "walk_with_anim");
}

// Нумерация и геометрия щелей grid при коллапсе `repeat(auto-fit, …)` (BUG-553, срез 17;
// WPT `css/css-gaps/grid/grid-gap-decorations-collapsed-*`). Пустые auto-fit-дорожки
// схлопываются в 0, а щели по обе стороны сливаются в одну — она берёт ровно одно значение
// списка (CSS Gap Decorations L1 §4.6).

/// collapsed-middle-spanner: схлопнутая средняя дорожка не оставляет щели; единственный
/// видимый зазор между верхним и нижним рядом блокирует элемент, растянутый на два ряда.
#[test]
fn grid_auto_fit_collapsed_middle_rows_keep_one_gap() {
    let html = r#"<div style="display:grid;grid-template-columns:100px 100px 100px;
        grid-template-rows:100px repeat(auto-fit,100px) 100px 100px;gap:10px;width:320px;height:540px;
        align-content:start;row-rule:6px solid red;rule-inset:0px">
        <div style="grid-column:1;grid-row:-3/-1;width:100px;height:100px"></div>
        <div style="grid-column:2;grid-row:1/2;width:100px;height:100px"></div>
        <div style="grid-column:2;grid-row:-3/-2;width:100px;height:100px"></div>
        <div style="grid-column:2;grid-row:-2/-1;width:100px;height:100px"></div>
        <div style="grid-column:3;grid-row:1/2;width:100px;height:100px"></div>
        <div style="grid-column:3;grid-row:-3/-2;width:100px;height:100px"></div>
        <div style="grid-column:3;grid-row:-2/-1;width:100px;height:100px"></div></div>"#;
    let dl = build(html, "");
    // Ряды после коллапса: 0..100, 110..210, 220..320 — две щели (y = 102 и 212) на всю
    // ширину; схлопнутые дорожки не оставляют третьей.
    assert_rules(&rules(&dl, false), &[(0.0, 102.0, 320.0, 6.0), (0.0, 212.0, 320.0, 6.0)]);
}

/// grid-gap-decorations-029 (`grid-gap` — устаревший алиас `gap`): дорожки 3×100px + 2×10px
/// шире контейнера `120px`, но линии идут на всю протяжённость сетки (320px), а не на 120px.
#[test]
fn grid_rules_span_tracks_overflowing_the_container() {
    let html = r#"<div style="display:grid;grid-gap:10px;grid-template-columns:100px 100px 100px;
        grid-template-rows:100px 100px 100px;width:120px;height:120px;
        column-rule:5px solid blue;row-rule:5px solid red">
        <div></div><div></div><div></div><div></div><div></div><div></div><div></div><div></div><div></div></div>"#;
    let dl = build(html, "");
    assert_rules(&rules(&dl, false), &[(0.0, 103.0, 320.0, 5.0), (0.0, 213.0, 320.0, 5.0)]);
    assert_rules(&rules(&dl, true), &[(103.0, 0.0, 5.0, 320.0), (213.0, 0.0, 5.0, 320.0)]);
}

/// collapsed-trailing-auto-fit: пустые дорожки в хвосте не создают щелей.
#[test]
fn grid_auto_fit_collapsed_trailing_rows_have_no_gap() {
    let html = r#"<div style="display:grid;grid-template-columns:100px 100px 100px;
        grid-template-rows:100px 100px repeat(auto-fit,100px);gap:10px;width:320px;height:430px;
        align-content:start;row-rule:6px solid red;row-rule-visibility-items:around;rule-inset:0px">
        <div style="grid-column:1/4;grid-row:1/2;height:100px"></div>
        <div style="grid-column:1/4;grid-row:2/3;height:100px"></div></div>"#;
    let dl = build(html, "");
    // Один зазор между двумя рядами: y = 100 + (10 − 6) / 2 = 102, на всю ширину 320.
    assert_rules(&rules(&dl, false), &[(0.0, 102.0, 320.0, 6.0)]);
}

/// flex-gap-decorations-027: отрицательное поле шире самого элемента переворачивает его
/// margin-box; щель после такого элемента всё равно рисуется (в потоке он занимает нулевую
/// протяжённость на своём конце, по эталону щели стоят на x = 102, 162, 222, 282, 342 при
/// контейнере на x = 200).
#[test]
fn flex_column_rule_after_item_with_oversized_negative_margin() {
    let html = r#"<div style="display:flex;border:2px solid #000;column-gap:10px;
        column-rule:10px solid red;width:200px;flex-wrap:nowrap">
        <div style="width:50px;height:50px;flex-shrink:0;margin-left:-150px"></div>
        <div style="width:50px;height:50px;flex-shrink:0"></div>
        <div style="width:50px;height:50px;flex-shrink:0"></div>
        <div style="width:50px;height:50px;flex-shrink:0"></div>
        <div style="width:50px;height:50px;flex-shrink:0"></div>
        <div style="width:50px;height:50px;flex-shrink:0"></div></div>"#;
    let dl = build(html, "");
    let got = rules(&dl, true);
    let want: Vec<_> = [-98.0, -38.0, 22.0, 82.0, 142.0].iter().map(|&x| (x, 2.0, 10.0, 50.0)).collect();
    assert_rules(&got, &want);
}

/// Срез 32: заливка `background-color` привязана к целым пикселям, поэтому соседние элементы
/// на дробных дорожках `1fr` (100px − 2·10px = 26.67px) делят шов без полосы фона контейнера.
#[test]
fn box_backgrounds_snap_to_device_pixels() {
    let html = r#"<div class="g"><div class="i"></div><div class="i"></div><div class="i"></div></div>"#;
    let css = "*{margin:0}.g{display:grid;grid-template-columns:repeat(3,1fr);column-gap:10px;width:100px;\
               height:20px;background:red}.i{background:green}";
    let dl = build(html, css);
    let green: Vec<(f32, f32)> = dl
        .iter()
        .filter_map(|c| match c {
            DisplayCommand::FillRect { rect, color } if color.g > 0 && color.r == 0 => Some((rect.x, rect.x + rect.width)),
            _ => None,
        })
        .collect();
    assert_eq!(green.len(), 3, "три заливки элементов: {green:?}");
    for (x0, x1) in &green {
        assert_eq!(x0.fract(), 0.0, "левый край на целом пикселе: {green:?}");
        assert_eq!(x1.fract(), 0.0, "правый край на целом пикселе: {green:?}");
    }
    // 0–26.67, 36.67–63.33, 73.33–100 → 0–27, 37–63, 73–100.
    assert_eq!(green, vec![(0.0, 27.0), (37.0, 63.0), (73.0, 100.0)]);
}

/// grid-gap-decorations-040/057: сетка 4×4 по 100px с двумя элементами — дорожки без
/// элементов (пустые строки/колонки) тоже дают щели, потому что шаблон из фиксированных
/// длин задаёт их геометрию независимо от детей. Щелей три на 4 дорожки; щель 1 обрывается
/// о широкий элемент (строки 1–2) и продолжается под ним до низа сетки (430px), остальные две
/// идут на всю протяжённость.
#[test]
fn grid_fixed_template_gives_gaps_between_empty_tracks() {
    let html = r#"<div style="display:grid;grid-template-columns:repeat(4,100px);grid-template-rows:repeat(4,100px);
        gap:10px;width:430px;height:430px;column-rule:5px solid blue;row-rule:5px solid red">
        <div style="grid-column:1/3;grid-row:1/3"></div><div style="grid-column:3/4;grid-row:1/3"></div></div>"#;
    let dl = build(html, "");
    let tops = |v: &[(f32, f32, f32, f32)], pick: fn(&(f32, f32, f32, f32)) -> f32| -> Vec<f32> {
        let mut t: Vec<f32> = v.iter().map(pick).collect();
        t.dedup_by(|a, b| (*a - *b).abs() < 0.6);
        t
    };
    let cols = rules(&dl, true);
    let rows = rules(&dl, false);
    // Колоночные щели — x = 102, 212, 322; строчные — y = 102, 212, 322 (центр зазора).
    // `rules` сортирует по (y, x): куски щели 1 под элементом идут после целых щелей.
    assert_eq!(tops(&cols, |r| r.0).len(), 3, "{cols:?}");
    assert_eq!(tops(&rows, |r| r.1).len(), 3, "{rows:?}");
    assert!(close(cols[2], (103.0, 220.0, 5.0, 210.0)), "{cols:?}");
}

/// collapsed-middle-auto-fit: ведущая фиксированная дорожка перед `repeat(auto-fit, …)`
/// даёт щель после себя, даже если в ней нет элементов (её граница известна из шаблона).
#[test]
fn grid_leading_track_before_auto_fit_keeps_its_gap() {
    let html = r#"<div style="display:grid;grid-template-columns:100px 100px 100px;
        grid-template-rows:100px repeat(auto-fit,100px) 100px 100px;gap:10px;width:320px;height:540px;
        align-content:start;row-rule:6px solid red;row-rule-visibility-items:around;rule-inset:0px">
        <div style="grid-column:1/4;grid-row:-3/-2"></div><div style="grid-column:1/4;grid-row:-2/-1"></div></div>"#;
    let dl = build(html, "");
    assert_rules(&rules(&dl, false), &[(0.0, 102.0, 320.0, 6.0), (0.0, 212.0, 320.0, 6.0)]);
}

/// grid-gap-decorations-033: контейнер без элементов в потоке (дети нет вовсе или только
/// `position: absolute`) всё равно рисует правила по щелям дорожек из шаблона, даже если
/// дорожки шире самого контейнера (`width: 50px` над тремя 50px-колонками).
#[test]
fn empty_grid_with_fixed_template_still_paints_gap_rules() {
    let html = r#"<div style="display:grid;grid-template-columns:repeat(3,50px);grid-template-rows:repeat(2,50px);
        gap:10px;width:50px;height:50px;column-rule:10px solid blue;row-rule:5px solid red"></div>"#;
    let dl = build(html, "");
    let cols = rules(&dl, true);
    let rows = rules(&dl, false);
    assert_eq!(cols.len(), 2, "{cols:?}");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(close(cols[0], (50.0, 0.0, 10.0, 110.0)), "{cols:?}");
}

/// flex-gap-decorations-064: `overlap-join` тянет куски поперечной линии к одному слитому стыку
/// с обеих сторон — соседние куски не должны перекрываться (полупрозрачная линия иначе
/// красится дважды), граница проходит посередине наложения.
#[test]
fn flex_overlap_join_pieces_of_cross_rule_do_not_overlap() {
    let html = r#"<div style="display:flex;flex-wrap:wrap;width:600px;gap:90px;
        column-rule:5px solid rgba(0,0,255,0.5);row-rule:5px solid rgba(255,0,0,0.5);
        column-rule-break:intersection;column-rule-inset:overlap-join;
        row-rule-break:intersection;row-rule-inset:overlap-join">
        <div style="width:100px;height:100px"></div><div style="width:200px;height:100px"></div>
        <div style="width:90px;height:100px"></div><div style="width:160px;height:100px"></div>
        <div style="width:110px;height:100px"></div><div style="width:120px;height:100px"></div></div>"#;
    let dl = build(html, "");
    let rows = rules(&dl, false);
    assert!(!rows.is_empty(), "{rows:?}");
    for pair in rows.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if (a.1 - b.1).abs() < 0.1 {
            assert!(a.0 + a.2 <= b.0 + 0.6, "куски наложились: {rows:?}");
        }
    }
    let reach = rows.iter().map(|r| r.0 + r.2).fold(0.0_f32, f32::max);
    assert!((reach - 600.0).abs() < 0.6, "{rows:?}");
}
