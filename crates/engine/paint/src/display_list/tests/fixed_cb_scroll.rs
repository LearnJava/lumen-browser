//! LAYOUT-FIXED-CB-2: `position: fixed` под предком, создающим containing block
//! (transform/filter/contain), в порядковой сборке display list — какие
//! scroll-слои предков он наследует.

use super::text_and_images::Fixed8;
use super::*;

/// Собирает порядковый display list, сдвинув единственный scroll-контейнер
/// с `id` в `(0, y)`, и возвращает `scroll_y` всех `PushScrollLayer`, открытых
/// в момент заливки `FillRect` цвета `red` (подвижный бокс теста).
fn scroll_layers_at_red(html: &str, css: &str, scrolled: &[(&str, f32)]) -> Vec<f32> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let mut tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    for (id, y) in scrolled {
        let node = doc.find_by_id(id).expect("id");
        assert!(lumen_layout::set_scroll_position(&mut tree, node, 0.0, *y), "#{id} не скроллится");
    }
    let stacking = lumen_layout::StackingTree::build(&tree);
    let order = lumen_layout::PaintOrder::from_tree(&stacking);
    let dl = build_display_list_ordered(&tree, &stacking, &order).0;
    let mut open: Vec<f32> = Vec::new();
    for c in &dl {
        match c {
            DisplayCommand::PushScrollLayer { scroll_y, .. } => open.push(*scroll_y),
            DisplayCommand::PopScrollLayer => {
                open.pop();
            }
            DisplayCommand::FillRect { color, .. } if *color == Color { r: 255, g: 0, b: 0, a: 255 } => {
                return open;
            }
            _ => {}
        }
    }
    panic!("нет красной заливки: {:?}", dl.iter().map(DisplayCommand::variant_name).collect::<Vec<_>>());
}

const F: &str = ".f{position:fixed;top:5px;left:5px;width:10px;height:10px;background:#f00}";

/// Опорная линия: CB — transform-предок, он сам SC и вкладывает ребёнка в свою скобку.
#[test]
fn fixed_under_transform_in_scroller_scrolls_with_it() {
    let layers = scroll_layers_at_red(
        r#"<div id="s"><div class="t"><div class="f"></div></div><div style="height:400px"></div></div>"#,
        &format!("#s{{overflow:auto;height:80px}}.t{{transform:scale(1);height:50px}}{F}"),
        &[("s", 30.0)],
    );
    assert_eq!(layers, vec![30.0]);
}

/// CB — `contain: layout` (не stacking context в нашей модели): scroll-слой предка
/// выше CB обязан дойти до `fixed`-ребёнка.
#[test]
fn fixed_under_contain_layout_in_scroller_scrolls_with_it() {
    let layers = scroll_layers_at_red(
        r#"<div id="s"><div class="t"><div class="f"></div></div><div style="height:400px"></div></div>"#,
        &format!("#s{{overflow:auto;height:80px}}.t{{contain:layout;height:50px}}{F}"),
        &[("s", 30.0)],
    );
    assert_eq!(layers, vec![30.0]);
}

/// Без предка-CB `fixed` по-прежнему не наследует scroll-слой (BUG-159).
#[test]
fn fixed_without_cb_ignores_ancestor_scroll() {
    let layers = scroll_layers_at_red(
        r#"<div id="s"><div class="t"><div class="f"></div></div><div style="height:400px"></div></div>"#,
        &format!("#s{{overflow:auto;height:80px}}.t{{height:50px}}{F}"),
        &[("s", 30.0)],
    );
    assert!(layers.is_empty(), "{layers:?}");
}

/// Контейнер прокрутки МЕЖДУ CB и `fixed`-ребёнком ребёнка не двигает — CB выше.
#[test]
fn fixed_escapes_scroller_below_its_cb() {
    let layers = scroll_layers_at_red(
        r#"<div id="s"><div class="t"><div id="s2"><div class="f"></div><div style="height:400px"></div></div></div><div style="height:400px"></div></div>"#,
        &format!("#s{{overflow:auto;height:80px}}.t{{contain:layout;height:50px}}#s2{{overflow:auto;height:40px}}{F}"),
        &[("s", 30.0), ("s2", 20.0)],
    );
    assert_eq!(layers, vec![30.0]);
}

/// CB сам прокручивается: его содержимое (в том числе `fixed`-потомок) едет с ним.
#[test]
fn fixed_in_scrolled_cb_scrolls_with_it() {
    let layers = scroll_layers_at_red(
        r#"<div id="t" class="t"><div class="f"></div><div style="height:400px"></div></div>"#,
        &format!(".t{{contain:layout;overflow:auto;height:80px}}{F}"),
        &[("t", 25.0)],
    );
    assert_eq!(layers, vec![25.0]);
}

fn legacy_has_fixed_marker(html: &str, css: &str) -> bool {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    build_display_list(&tree).iter().any(|c| matches!(c, DisplayCommand::BeginFixedLayer))
}

/// Скобка `BeginFixedLayer` — признак «приколот к вьюпорту» для композитора;
/// `fixed` под transform едет со страницей и её не получает.
#[test]
fn legacy_walk_fixed_under_cb_is_not_an_overlay() {
    let html = r#"<div class="t"><div class="f"></div></div>"#;
    assert!(!legacy_has_fixed_marker(html, &format!(".t{{transform:scale(1);height:50px}}{F}")));
    assert!(!legacy_has_fixed_marker(html, &format!(".t{{filter:blur(0px);height:50px}}{F}")));
    assert!(legacy_has_fixed_marker(html, &format!(".t{{height:50px}}{F}")));
}
