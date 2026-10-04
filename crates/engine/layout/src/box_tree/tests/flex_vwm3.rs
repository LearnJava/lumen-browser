use lumen_core::geom::{Rect, Size};

// ── FLEX-VWM-3: ортогональные потоки во flex и общие дефекты, найденные на WPT ──

fn layout_of(html: &str, css: &str) -> (super::super::LayoutBox, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    (root, doc)
}

fn rects(html: &str, css: &str, ids: &[&str]) -> Vec<Rect> {
    let (root, doc) = layout_of(html, css);
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

/// Текст float:right лежит внутри своего бокса, а не у левого края контейнера.
#[test]
fn right_float_carries_its_text() {
    let (root, doc) = layout_of(
        r#"<div id="c"><div id="f">hello</div></div>"#,
        "#c{width:300px}#f{float:right;width:50px}",
    );
    let f = super::find_by_id_all(&root, &doc, "f").unwrap();
    assert!(f.rect.x > 200.0, "float прижат вправо: {:?}", f.rect);
    let run = &f.children[0];
    assert_eq!(run.rect.x, f.rect.x, "строка текста идёт за боксом: {:?} / {:?}", run.rect, f.rect);
}

/// `position: relative` у float-а сдвигает и бокс, и его содержимое.
#[test]
fn relative_float_moves_box_and_content_together() {
    let (root, doc) = layout_of(
        r#"<div id="c"><div id="f">hello</div></div>"#,
        "#c{width:300px}#f{float:left;position:relative;left:4px;margin-left:7px}",
    );
    let f = super::find_by_id_all(&root, &doc, "f").unwrap();
    assert_eq!(f.rect.x, 11.0, "{:?}", f.rect);
    assert_eq!(f.children[0].rect.x, 11.0, "{:?}", f.children[0].rect);
}

/// Ширина float-а «по содержимому» отдаётся вместе с его горизонтальными
/// margin: border-box равен содержимому, а не содержимому минус margin.
#[test]
fn shrink_to_fit_float_keeps_its_horizontal_margins_out_of_the_box() {
    let r = rects(
        r#"<div id="f"><div id="i"></div></div>"#,
        "#f{float:left;margin:0 3px}#i{width:10px;height:10px;border:1px solid}",
        &["f", "i"],
    );
    assert_eq!(r[0].width, 12.0, "{:?}", r[0]);
    assert_eq!(r[1].width, 12.0, "{:?}", r[1]);
}

/// CSS 2.1 §10.3.3: при `direction: rtl` у родителя блок уже содержащего блока
/// прижимается к правому краю.
#[test]
fn rtl_parent_aligns_overconstrained_block_to_the_right() {
    let r = rects(
        r#"<div id="p"><div id="c"></div></div>"#,
        "#p{width:300px;direction:rtl}#c{width:50px;height:5px;margin:0 13px 0 7px}",
        &["c"],
    );
    assert_eq!(r[0].x, 300.0 - 13.0 - 50.0, "{:?}", r[0]);
}

/// Колонка вертикальных элементов в горизонтальном flex-контейнере: block-размер
/// (ширина) auto растягивается на поперечную ось.
#[test]
fn column_flex_stretches_orthogonal_item_width() {
    let r = rects(
        r#"<div id="k"><div id="i"><span>ab</span></div></div>"#,
        "#k{display:flex;flex-direction:column;width:150px;height:200px}\
         #i{writing-mode:vertical-lr;margin:0 13px 0 7px}",
        &["i"],
    );
    assert_eq!(r[0].x, 7.0, "{:?}", r[0]);
    assert_eq!(r[0].width, 150.0 - 7.0 - 13.0, "{:?}", r[0]);
}

/// Content-box `width` элемента колонки не теряет рамку при повторной раскладке
/// (сброшенный вниз float раскладывает flex-контейнер второй раз).
#[test]
fn dropped_float_column_flex_keeps_item_border_box() {
    let r = rects(
        r#"<div id="w"><div class="c"><div class="i"></div></div><div class="c"><div class="i" id="i2"></div></div></div>"#,
        "#w{width:300px}.c{display:flex;flex-direction:column;float:left;width:200px;height:50px}\
         .i{width:6px;border:2px solid}",
        &["i2"],
    );
    assert_eq!(r[0].width, 10.0, "{:?}", r[0]);
}

/// Вертикальный поток: margin по block-оси (физические left/right) занимают место
/// между соседями, а смежные margin схлопываются.
#[test]
fn vertical_lr_block_axis_margins_collapse_between_siblings() {
    let r = rects(
        r#"<div id="k"><div id="a"></div><div id="b"></div></div>"#,
        "#k{writing-mode:vertical-lr;height:100px;width:200px}\
         #a,#b{width:10px;height:20px;margin:0 13px 0 7px}",
        &["a", "b"],
    );
    assert_eq!(r[0].x, 7.0, "{:?}", r[0]);
    // 7 + 10 + max(13, 7) = 30
    assert_eq!(r[1].x, 30.0, "{:?}", r[1]);
}

/// `vertical-rl`: block-start — правая кромка, поэтому первым считается
/// margin-right.
#[test]
fn vertical_rl_block_axis_margins_start_at_the_right() {
    let r = rects(
        r#"<div id="k"><div id="a"></div></div>"#,
        "#k{writing-mode:vertical-rl;height:100px;width:200px}\
         #a{width:10px;height:20px;margin:0 13px 0 7px}",
        &["a"],
    );
    assert_eq!(r[0].x, 200.0 - 13.0 - 10.0, "{:?}", r[0]);
}

/// `direction: rtl` в вертикальном потоке: inline-start — низ, over-constrained
/// блок прижимается вниз.
#[test]
fn vertical_rtl_aligns_overconstrained_block_to_the_bottom() {
    let r = rects(
        r#"<div id="k"><div id="a"></div></div>"#,
        "#k{writing-mode:vertical-lr;direction:rtl;height:100px;width:200px}\
         #a{width:10px;height:20px;margin:11px 0 17px 0}",
        &["a"],
    );
    assert_eq!(r[0].y, 100.0 - 17.0 - 20.0, "{:?}", r[0]);
}

/// Процентный padding вертикального блока считается от inline-размера
/// содержащего блока (высоты), а не от ширины.
#[test]
fn vertical_box_percent_padding_resolves_against_inline_size() {
    let r = rects(
        r#"<div id="k"><div id="a"></div></div>"#,
        "#k{writing-mode:vertical-lr;height:100px;width:400px}\
         #a{width:30px;padding-right:70%}",
        &["a"],
    );
    assert_eq!(r[0].width, 30.0 + 70.0, "{:?}", r[0]);
}

/// Float в вертикальном режиме с `height:auto` обтягивает содержимое по
/// inline-оси (высоте), а не занимает всё место.
#[test]
fn vertical_float_shrink_wraps_its_inline_size() {
    let r = rects(
        r#"<div id="f"><div id="i"></div></div>"#,
        "#f{float:left;writing-mode:vertical-lr}#i{width:5px;height:30px}",
        &["f"],
    );
    assert_eq!(r[0].height, 30.0, "{:?}", r[0]);
}

/// Поля детей вертикального блока не схлопываются через его верхний край:
/// их y-краёв нет у соседей по block-оси.
#[test]
fn vertical_box_does_not_collapse_child_margin_through_its_top() {
    let r = rects(
        r#"<div id="k"><div id="a"></div></div>"#,
        "#k{writing-mode:vertical-lr;height:100px;width:200px}#a{width:10px;height:20px;margin:11px 0 0 0}",
        &["k", "a"],
    );
    assert_eq!(r[0].y, 0.0, "{:?}", r[0]);
    assert_eq!(r[1].y, 11.0, "{:?}", r[1]);
}
