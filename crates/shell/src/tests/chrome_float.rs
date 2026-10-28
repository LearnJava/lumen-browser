//! CC-18 срез 3: перетаскивание плавающей панели за шапку и сброс позиции
//! дабл-кликом. Тесты держат три инварианта среза: drag двигает только
//! нарисованное, но не layout-бокс; дабл-клик возвращает панель к CSS-дефолту;
//! нажатие на собственный контрол панели разрешается в его действие, а не в
//! `drag-panel`.

use super::*;

use crate::chrome_float::{drag_offset, is_double_press, FloatingPanelPress};
use crate::platform::os_params::OsParams;

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
/// Заливка привязана к целым пикселям (`background_clip_rect`), поэтому каждый край
/// может отличаться от дробного края бокса не более чем на полпикселя.
fn fills_rect(dl: &lumen_paint::DisplayList, rect: Rect) -> bool {
    let near = |a: f32, b: f32| (a - b).abs() <= 0.5;
    dl.iter().any(|cmd| {
        matches!(
            cmd,
            lumen_paint::DisplayCommand::FillRect { rect: r, .. }
            | lumen_paint::DisplayCommand::FillRoundedRect { rect: r, .. }
                if near(r.x, rect.x)
                    && near(r.y, rect.y)
                    && near(r.x + r.width, rect.x + rect.width)
                    && near(r.y + r.height, rect.y + rect.height)
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
    let os = OsParams::DEFAULT;
    assert!(
        !is_double_press(&os, None, lumen_chrome::ids::DEMO_BAR, 1_100.0, 100.0, 200.0),
        "первое нажатие в сессии дабл-кликом быть не может"
    );
    assert!(is_double_press(&os, Some(&prev), lumen_chrome::ids::DEMO_BAR, 1_200.0, 102.0, 199.0));
    assert!(
        !is_double_press(&os, Some(&prev), lumen_chrome::ids::DEMO_BAR, 2_500.0, 100.0, 200.0),
        "пауза больше порога — это два отдельных нажатия"
    );
    assert!(
        !is_double_press(&os, Some(&prev), lumen_chrome::ids::DEMO_BAR, 1_200.0, 140.0, 200.0),
        "курсор уехал — это не дабл-клик, а новое перетаскивание"
    );
    assert!(
        !is_double_press(&os, Some(&prev), lumen_chrome::ids::INFO_PANEL, 1_200.0, 100.0, 200.0),
        "нажатия по шапкам разных панелей не складываются в дабл-клик"
    );
}

/// UX-OSPARAMS: время и допуск дабл-клика берутся из ОС, а не из констант.
#[test]
fn osparams_double_press_follows_os_time_and_slop() {
    let prev = FloatingPanelPress {
        panel_id: lumen_chrome::ids::DEMO_BAR,
        at_ms: 1_000.0,
        pos: (100.0, 200.0),
    };
    let slow = OsParams { double_click_ms: 2_000, double_click_slop: (20.0, 20.0), ..OsParams::DEFAULT };
    // 1,5 с и 8 px: для настроек по умолчанию это два нажатия, для замедленных — дабл-клик.
    assert!(!is_double_press(&OsParams::DEFAULT, Some(&prev), lumen_chrome::ids::DEMO_BAR, 2_500.0, 108.0, 200.0));
    assert!(is_double_press(&slow, Some(&prev), lumen_chrome::ids::DEMO_BAR, 2_500.0, 108.0, 200.0));
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

/// UPD-9: `#updateBar` идёт тем же путём, что `#demoBar`: он лежит внутри
/// `chrome_page_host_rect`, поэтому без отцепления 4-полосный клип выбросил
/// бы его целиком. Тест держит три вещи: закрытый бар не даёт бокса (нечего
/// отцеплять), открытый стоит в правом верхнем углу по CSS эталона и
/// рисуется, а нажатие на «Скачать» разрешается в `DownloadUpdate`.
#[test]
fn upd9_update_bar_detaches_paints_and_routes_its_button() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let bar = doc.find_by_id(lumen_chrome::ids::UPDATE_BAR).expect("has #updateBar");

    let _ = lumen_chrome::bind_model_tracked(&mut doc, &lumen_chrome::ChromeModel::default());
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    assert!(
        take_floating_panel(&mut layout, bar, lumen_chrome::ids::UPDATE_BAR).is_none(),
        "closed #updateBar is display:none — no box to detach"
    );

    let model = lumen_chrome::ChromeModel {
        update: lumen_chrome::ChromeUpdateModel {
            bar_open: true,
            title: "Доступна версия 9.9.9".to_owned(),
            meta: "Установлена 0.5.0 · 2.0 MB".to_owned(),
            action: lumen_chrome::ChromeUpdateAction::Download,
            ..lumen_chrome::ChromeUpdateModel::default()
        },
        ..lumen_chrome::ChromeModel::default()
    };
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let (rect, detached) = take_floating_panel(&mut layout, bar, lumen_chrome::ids::UPDATE_BAR)
        .expect("open #updateBar must be detachable");
    assert!((rect.width - 340.0).abs() < 0.5, "width {rect:?}");
    assert!((rect.right() - (1920.0 - 18.0)).abs() < 0.5, "right edge {rect:?}");
    assert!((rect.y - 92.0).abs() < 0.5, "top {rect:?}");
    assert!(rect.height > 40.0, "title row + action row, got {rect:?}");
    assert!(fills_rect(&paint_ordered(&detached.removed), rect), "bar background must paint");

    let btn = doc.find_by_id(lumen_chrome::ids::UPD_DOWNLOAD_BTN).expect("has #updDownloadBtn");
    let btn_rect = lumen_layout::find_box_by_node(&detached.removed, btn).expect("visible button has a box").rect;
    let hit = hit_test(
        Point::new(btn_rect.x + btn_rect.width / 2.0, btn_rect.y + btn_rect.height / 2.0),
        &detached.removed,
    )
    .expect("press on the button hits the bar");
    assert_eq!(action_at(&doc, &hit), Some(lumen_chrome::ChromeAction::DownloadUpdate));
    let restart = doc.find_by_id(lumen_chrome::ids::UPD_RESTART_BTN).expect("has #updRestartBtn");
    assert!(
        lumen_layout::find_box_by_node(&detached.removed, restart).is_none(),
        "restart button is hidden until the archive is staged"
    );
}

/// UX-PASSWORDS: `#loginBar` отцепляется как `#updateBar` и не налезает на него
/// по горизонтали, а кнопка «Сохранить» разрешается в `SaveLogin`.
#[test]
fn login_bar_detaches_and_routes_its_buttons() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let bar = doc.find_by_id(lumen_chrome::ids::LOGIN_BAR).expect("has #loginBar");

    let model = lumen_chrome::ChromeModel {
        login_offer: lumen_chrome::ChromeLoginOfferModel {
            open: true,
            title: "Сохранить пароль?".to_owned(),
            meta: "example.com · anna".to_owned(),
            save_label: "Сохранить".to_owned(),
            fill: false,
            generate: false,
            never_label: String::new(),
        },
        update: lumen_chrome::ChromeUpdateModel { bar_open: true, ..Default::default() },
        ..lumen_chrome::ChromeModel::default()
    };
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let (rect, detached) = take_floating_panel(&mut layout, bar, lumen_chrome::ids::LOGIN_BAR)
        .expect("open #loginBar must be detachable");
    assert!(rect.right() < 1920.0 - 18.0 - 340.0 + 0.5, "не должен перекрывать #updateBar: {rect:?}");
    assert!(fills_rect(&paint_ordered(&detached.removed), rect), "bar background must paint");

    let press = |id| {
        let node = doc.find_by_id(id).expect("button id");
        let r = lumen_layout::find_box_by_node(&detached.removed, node).expect("button box").rect;
        let hit = hit_test(Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0), &detached.removed)
            .expect("press hits the bar");
        action_at(&doc, &hit)
    };
    assert_eq!(press(lumen_chrome::ids::LOGIN_SAVE_BTN), Some(lumen_chrome::ChromeAction::SaveLogin));
}

/// COMPAT-MEDIA-OSDEC-WIN-BAR: `#mediaBar` отцепляется, не налезает на
/// `#updateBar`/`#loginBar`, обе кнопки разрешаются в свои действия.
#[test]
fn media_bar_detaches_and_routes_its_buttons() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let bar = doc.find_by_id(lumen_chrome::ids::MEDIA_BAR).expect("has #mediaBar");

    let closed = lumen_chrome::ChromeModel::default();
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &closed);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    assert!(
        take_floating_panel(&mut layout, bar, lumen_chrome::ids::MEDIA_BAR).is_none(),
        "закрытая панель не отцепляется"
    );

    let mut state = crate::media_bar::MediaBar::default();
    assert!(state.note_missing(lumen_media_native::MissingOsCodec::MediaFeaturePack));
    let model = lumen_chrome::ChromeModel {
        media_bar: state.model(),
        login_offer: lumen_chrome::ChromeLoginOfferModel {
            open: true,
            title: "Сохранить пароль?".to_owned(),
            meta: "example.com · anna".to_owned(),
            save_label: "Сохранить".to_owned(),
            ..Default::default()
        },
        update: lumen_chrome::ChromeUpdateModel { bar_open: true, ..Default::default() },
        ..lumen_chrome::ChromeModel::default()
    };
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let login = doc.find_by_id(lumen_chrome::ids::LOGIN_BAR).expect("has #loginBar");
    let (login_rect, _) = take_floating_panel(&mut layout, login, lumen_chrome::ids::LOGIN_BAR)
        .expect("open #loginBar must be detachable");
    let (rect, detached) = take_floating_panel(&mut layout, bar, lumen_chrome::ids::MEDIA_BAR)
        .expect("open #mediaBar must be detachable");
    assert!(rect.right() < 1920.0 - 18.0 - 340.0 + 0.5, "не должен перекрывать #updateBar: {rect:?}");
    assert!(rect.y >= login_rect.bottom() - 0.5, "не должен перекрывать #loginBar: {rect:?} / {login_rect:?}");
    assert!(fills_rect(&paint_ordered(&detached.removed), rect), "bar background must paint");

    let press = |id| {
        let node = doc.find_by_id(id).expect("button id");
        let r = lumen_layout::find_box_by_node(&detached.removed, node).expect("button box").rect;
        let hit = hit_test(Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0), &detached.removed)
            .expect("press hits the bar");
        action_at(&doc, &hit)
    };
    assert_eq!(press(lumen_chrome::ids::MEDIA_INSTALL_BTN), Some(lumen_chrome::ChromeAction::OpenMediaInstall));
    assert_eq!(press(lumen_chrome::ids::MEDIA_HIDE_BTN), Some(lumen_chrome::ChromeAction::DismissMediaBar));
}

/// COMPAT-HANG: `#hangBar` отцепляется, три кнопки разрешаются в свои действия.
#[test]
fn hang_bar_detaches_and_routes_its_buttons() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let bar = doc.find_by_id(lumen_chrome::ids::HANG_BAR).expect("has #hangBar");

    let closed = lumen_chrome::ChromeModel::default();
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &closed);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    assert!(
        take_floating_panel(&mut layout, bar, lumen_chrome::ids::HANG_BAR).is_none(),
        "закрытая панель не отцепляется"
    );

    let model = lumen_chrome::ChromeModel {
        hang_bar: lumen_chrome::ChromeHangBarModel { open: true, meta: "Скрипт не отвечает 7 с.".to_owned() },
        ..lumen_chrome::ChromeModel::default()
    };
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let (rect, detached) = take_floating_panel(&mut layout, bar, lumen_chrome::ids::HANG_BAR)
        .expect("open #hangBar must be detachable");
    assert!(fills_rect(&paint_ordered(&detached.removed), rect), "bar background must paint");

    let press = |id| {
        let node = doc.find_by_id(id).expect("button id");
        let r = lumen_layout::find_box_by_node(&detached.removed, node).expect("button box").rect;
        let hit = hit_test(Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0), &detached.removed)
            .expect("press hits the bar");
        action_at(&doc, &hit)
    };
    assert_eq!(press(lumen_chrome::ids::HANG_WAIT_BTN), Some(lumen_chrome::ChromeAction::HangWait));
    assert_eq!(press(lumen_chrome::ids::HANG_RELOAD_BTN), Some(lumen_chrome::ChromeAction::HangReload));
    assert_eq!(press(lumen_chrome::ids::HANG_STOP_BTN), Some(lumen_chrome::ChromeAction::HangStop));
}

/// UX-SECURITY-UI: `#securityBox` отцепляется как плавающая панель, обе кнопки
/// разрешаются в свои действия.
#[test]
fn security_box_detaches_and_routes_its_buttons() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let bx = doc.find_by_id(lumen_chrome::ids::SECURITY_BOX).expect("has #securityBox");

    let model = lumen_chrome::ChromeModel {
        security: lumen_chrome::ChromeSecurityModel {
            open: true,
            title: "Ваше подключение не защищено".to_owned(),
            message: "bad.example".to_owned(),
            detail: "certificate expired".to_owned(),
            proceed_label: String::new(),
            other_browser: true,
            proceed_blocked: false,
            time_settings: true,
        },
        ..lumen_chrome::ChromeModel::default()
    };
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let (rect, detached) = take_floating_panel(&mut layout, bx, lumen_chrome::ids::SECURITY_BOX)
        .expect("open #securityBox must be detachable");
    assert!(fills_rect(&paint_ordered(&detached.removed), rect), "box background must paint");

    let press = |id| {
        let node = doc.find_by_id(id).expect("button id");
        let r = lumen_layout::find_box_by_node(&detached.removed, node).expect("button box").rect;
        let hit = hit_test(Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0), &detached.removed)
            .expect("press hits the box");
        action_at(&doc, &hit)
    };
    assert_eq!(press(lumen_chrome::ids::SECURITY_PROCEED_BTN), Some(lumen_chrome::ChromeAction::SecurityProceed));
    assert_eq!(press(lumen_chrome::ids::SECURITY_BACK_BTN), Some(lumen_chrome::ChromeAction::SecurityBack));
    assert_eq!(
        press(lumen_chrome::ids::SECURITY_OTHER_BTN),
        Some(lumen_chrome::ChromeAction::SecurityOtherBrowser)
    );
    assert_eq!(
        press(lumen_chrome::ids::SECURITY_OTHER_ANY_BTN),
        Some(lumen_chrome::ChromeAction::SecurityOtherBrowserAny)
    );
    assert_eq!(
        press(lumen_chrome::ids::SECURITY_TIME_BTN),
        Some(lumen_chrome::ChromeAction::SecurityTimeSettings)
    );
}

/// UX-CHROME-PANELS срез 21: `#profileFrame` в анонимном профиле отцепляется,
/// покрывает окно целиком и рисует обводку; вне анонимного — не рисуется.
#[test]
fn profile_frame_covers_window_only_for_anonymous() {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let probe = |slug: &str| {
        let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
        let model = lumen_chrome::ChromeModel {
            profile_slug: Some(slug.to_owned()),
            ..lumen_chrome::ChromeModel::default()
        };
        let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
        let node = doc.find_by_id(lumen_chrome::ids::PROFILE_FRAME).expect("has #profileFrame");
        let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
        take_floating_panel(&mut layout, node, lumen_chrome::ids::PROFILE_FRAME)
            .map(|(rect, d)| (rect, paint_ordered(&d.removed).len()))
    };
    let (rect, cmds) = probe("anonymous").expect("anonymous: рамка есть в дереве");
    assert_eq!((rect.x, rect.y, rect.width, rect.height), (0.0, 0.0, 1920.0, 1040.0));
    assert!(cmds > 0, "обводка должна рисоваться");
    let off = probe("personal").map_or(0, |(_, n)| n);
    assert_eq!(off, 0, "вне анонимного профиля рамка не рисуется");
}
