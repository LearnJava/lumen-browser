//! CC-18 срез 3: перетаскивание плавающей панели за шапку и сброс позиции
//! дабл-кликом. Тесты держат три инварианта среза: drag двигает только
//! нарисованное, но не layout-бокс; дабл-клик возвращает панель к CSS-дефолту;
//! нажатие на собственный контрол панели разрешается в его действие, а не в
//! `drag-panel`.

use super::*;

use crate::chrome_float::{drag_offset, is_double_press, FloatingPanelPress};

/// Раскладывает настоящий chrome-документ и отцепляет `#demoBar` ровно так,
/// как это делает `relayout_chrome_host` — общая преамбула всех тестов ниже.
fn detached_demo_bar() -> (lumen_dom::Document, Rect, crate::chrome_float::FloatingPanelDetachment)
{
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let model = lumen_chrome::ChromeModel::default();
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout =
        lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let demo_bar = doc.find_by_id(lumen_chrome::ids::DEMO_BAR).expect("has #demoBar");
    let (rect, detached) =
        take_floating_panel(&mut layout, demo_bar, lumen_chrome::ids::DEMO_BAR)
            .expect("#demoBar must be detachable");
    (doc, rect, detached)
}

/// Повторяет `Lumen::chrome_action_at` без живой оболочки: ближайший предок
/// с распознанным `data-action` вдоль bubble-пути попадания.
fn action_at(doc: &lumen_dom::Document, hit: &lumen_paint::HitTestResult) -> Option<lumen_chrome::ChromeAction> {
    hit.path
        .iter()
        .find_map(|&nid| doc.get(nid).get_attr("data-action").and_then(lumen_chrome::ChromeAction::from_attr_value))
}

/// Есть ли в списке отрисовки собственная заливка бокса панели в точке `rect`.
fn fills_rect(dl: &lumen_paint::DisplayList, rect: Rect) -> bool {
    dl.iter().any(|cmd| {
        matches!(
            cmd,
            lumen_paint::DisplayCommand::FillRect { rect: r, .. }
            | lumen_paint::DisplayCommand::FillRoundedRect { rect: r, .. }
                if *r == rect
        )
    })
}

/// Главный инвариант среза: drag — это смещение времени отрисовки. Он обязан
/// двигать то, что нарисовано, и обязан НЕ трогать сам layout-бокс: именно
/// этот бокс BUG-1059 возвращает в инкрементальный базис следующего прохода
/// (`restore_floating_panel`), и смещение, просочившееся в базис, подсунуло бы
/// переиспользованию боксов геометрию, которой каскад не выдавал.
#[test]
fn cc18_drag_moves_the_painted_rect_but_not_the_layout_box() {
    let (_doc, base, detached) = detached_demo_bar();

    // Курсор взял панель в точке (+10, +5) от её угла и увёл на (+120, -60).
    let grab = (10.0, 5.0);
    let offset = drag_offset(
        base,
        grab,
        base.x + grab.0 + 120.0,
        base.y + grab.1 - 60.0,
        1920.0,
        1040.0,
    );
    assert!((offset.0 - 120.0).abs() < 0.01, "горизонтальное смещение = пройденный путь: {offset:?}");
    assert!((offset.1 + 60.0).abs() < 0.01, "вертикальное смещение = пройденный путь: {offset:?}");

    let mut moved = detached.removed.clone();
    lumen_layout::translate_subtree(&mut moved, offset.0, offset.1);
    let moved_rect = moved.rect;
    assert!((moved_rect.x - (base.x + offset.0)).abs() < 0.01);
    assert!((moved_rect.y - (base.y + offset.1)).abs() < 0.01);

    assert_eq!(
        detached.removed.rect, base,
        "исходный бокс обязан остаться в CSS-вычисленной позиции — его отдают обратно в базис"
    );
    assert!(
        fills_rect(&paint_ordered(&moved), moved_rect),
        "смещённая копия рисует свою заливку в новой точке"
    );
    assert!(
        fills_rect(&paint_ordered(&detached.removed), base),
        "несмещённый бокс рисует ровно то же, что до среза — путь без drag-а не тронут"
    );
}

/// Клампинг у края окна: панель нельзя утащить за пределы экрана — это тот же
/// `Math.max(4, Math.min(innerWidth - width - 4, …))`, что и в эталоне.
#[test]
fn cc18_drag_clamps_the_panel_inside_the_window() {
    let (_doc, base, _detached) = detached_demo_bar();
    let grab = (10.0, 5.0);

    let far_up_left = drag_offset(base, grab, -5000.0, -5000.0, 1920.0, 1040.0);
    assert!((base.x + far_up_left.0 - 4.0).abs() < 0.01, "левый край упирается в поле 4px");
    assert!((base.y + far_up_left.1 - 4.0).abs() < 0.01, "верхний край упирается в поле 4px");

    let far_down_right = drag_offset(base, grab, 5000.0, 5000.0, 1920.0, 1040.0);
    assert!(
        (base.x + far_down_right.0 - (1920.0 - base.width - 4.0)).abs() < 0.01,
        "правый край упирается в поле 4px"
    );
    assert!(
        (base.y + far_down_right.1 - (1040.0 - base.height - 4.0)).abs() < 0.01,
        "нижний край упирается в поле 4px"
    );
}

/// Дабл-клик по шапке: второе нажатие рядом и вовремя — сброс; далеко или
/// поздно — обычное начало нового перетаскивания.
#[test]
fn cc18_double_click_detection_needs_same_panel_time_and_place() {
    let prev = FloatingPanelPress {
        panel_id: lumen_chrome::ids::DEMO_BAR,
        at_ms: 1_000.0,
        pos: (100.0, 200.0),
    };
    assert!(
        !is_double_press(None, lumen_chrome::ids::DEMO_BAR, 1_100.0, 100.0, 200.0),
        "первое нажатие в сессии дабл-кликом быть не может"
    );
    assert!(is_double_press(Some(&prev), lumen_chrome::ids::DEMO_BAR, 1_200.0, 102.0, 199.0));
    assert!(
        !is_double_press(Some(&prev), lumen_chrome::ids::DEMO_BAR, 2_500.0, 100.0, 200.0),
        "пауза больше порога — это два отдельных нажатия"
    );
    assert!(
        !is_double_press(Some(&prev), lumen_chrome::ids::DEMO_BAR, 1_200.0, 140.0, 200.0),
        "курсор уехал — это не дабл-клик, а новое перетаскивание"
    );
    assert!(
        !is_double_press(Some(&prev), lumen_chrome::ids::INFO_PANEL, 1_200.0, 100.0, 200.0),
        "нажатия по шапкам разных панелей не складываются в дабл-клик"
    );
}

/// Сам сброс: смещение снимается, и панель снова рисуется ровно в
/// CSS-вычисленной позиции своей формы — layout для этого пересчитывать не
/// нужно, дефолтная геометрия уже лежит в отцепленном боксе.
#[test]
fn cc18_reset_repaints_the_panel_at_its_css_default() {
    let (_doc, base, detached) = detached_demo_bar();
    let mut dragged = detached.removed.clone();
    lumen_layout::translate_subtree(&mut dragged, 120.0, -60.0);
    assert!(!fills_rect(&paint_ordered(&dragged), base), "пока панель утащена, её нет в дефолтной точке");

    // Сброс = забыть смещение; источником отрисовки снова становится сам
    // отцепленный бокс.
    assert!(
        fills_rect(&paint_ordered(&detached.removed), base),
        "после сброса панель рисуется в дефолтной для своей формы позиции"
    );
}

/// Нажатие на контрол внутри панели не должно начинать перетаскивание: у
/// кнопок есть собственный `data-action`, а поиск идёт по ближайшему предку —
/// это и есть эквивалент эталонного `e.target.closest('.demo-switch,
/// .demo-info-btn, .demo-expand')`, только декларативный.
#[test]
fn cc18_press_on_a_panel_control_resolves_its_own_action_not_the_drag_handle() {
    let (doc, _base, detached) = detached_demo_bar();

    let switch_id = doc.find_by_id("demoSwitch").expect("панель имеет #demoSwitch");
    let switch_box =
        lumen_layout::find_box_by_node(&detached.removed, switch_id).expect("у #demoSwitch есть бокс");
    let button = switch_box.children.first().expect("в переключателе форм есть кнопки");
    let p = Point::new(button.rect.x + button.rect.width / 2.0, button.rect.y + button.rect.height / 2.0);
    let hit = hit_test(p, &detached.removed).expect("точка внутри панели обязана попасть");
    assert_eq!(
        action_at(&doc, &hit),
        Some(lumen_chrome::ChromeAction::SetDemoVariant),
        "кнопка формы обязана остаться кнопкой формы, а не ручкой перетаскивания"
    );

    let header_id = doc.find_by_id("demoHeader").expect("панель имеет #demoHeader");
    let header_box =
        lumen_layout::find_box_by_node(&detached.removed, header_id).expect("у #demoHeader есть бокс");
    let grip = Point::new(header_box.rect.x + 2.0, header_box.rect.y + header_box.rect.height / 2.0);
    let hit = hit_test(grip, &detached.removed).expect("точка на шапке обязана попасть");
    assert_eq!(
        action_at(&doc, &hit),
        Some(lumen_chrome::ChromeAction::DragPanel),
        "свободное место шапки — ручка перетаскивания"
    );

    // Кнопка `ⓘ` живёт ВНУТРИ шапки: здесь bubble-поиск обязан остановиться
    // на ней, не дойдя до `drag-panel` родителя.
    let info_btn = header_box
        .children
        .iter()
        .find(|c| {
            doc.get(c.node).get_attr("data-action") == Some("toggle-demo-info")
        })
        .expect("в шапке есть кнопка ⓘ");
    let p = Point::new(
        info_btn.rect.x + info_btn.rect.width / 2.0,
        info_btn.rect.y + info_btn.rect.height / 2.0,
    );
    let hit = hit_test(p, &detached.removed).expect("точка на кнопке ⓘ обязана попасть");
    assert_eq!(
        action_at(&doc, &hit),
        Some(lumen_chrome::ChromeAction::ToggleDemoInfo),
        "кнопка внутри шапки перехватывает нажатие у перетаскивания"
    );
}
