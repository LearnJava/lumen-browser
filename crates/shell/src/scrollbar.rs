//! Vertical scrollbar в overlay-полосе display list-а: тонкая полоска у
//! правого края viewport-а, показывает текущую `scroll_y` относительно
//! `content_height`. Drag-to-scroll: `thumb_hit_test` определяет попадание
//! курсора по thumb-у, `ScrollDrag` хранит origin-снапшот и через
//! `scroll_for` отдаёт unclamped `scroll_y` под текущую позицию курсора.
//!
//! Render-сторона не меняется: scrollbar возвращается как `Vec<DisplayCommand>`,
//! который вызывающий конкатенирует в overlay-полосу. Overlay в `Renderer::render`
//! не сдвигается на `-scroll_y` — поэтому scrollbar остаётся viewport-locked
//! при любом scroll-position-е.
//!
//! Геометрия:
//! - `track` — фон вдоль правого края, всегда полный по высоте, тёмно-прозрачный;
//! - `thumb` — поверх track-а, высота пропорциональна `viewport/content`,
//!   позиция пропорциональна `scroll_y / max_scroll`;
//! - если контент помещается в viewport (`content_height <= viewport_height`),
//!   возвращается пустой Vec — scrollbar не рисуется (как в Chromium/Firefox
//!   c overlay-scrollbars-ом).
//!
//! Минимальная высота thumb-а — `MIN_THUMB_HEIGHT`: при очень длинных страницах
//! пропорциональная высота `viewport²/content` уходит к нулю и thumb становится
//! невидимым/некликабельным. Когда max применяется, scroll-mapping остаётся
//! линейным: `thumb_top = (viewport - thumb_h) * scroll_y / max_scroll`, и
//! thumb всё ещё корректно достигает `top=0` (scroll=0) и `top=viewport-thumb_h`
//! (scroll=max).

use lumen_core::geom::Rect;
use lumen_layout::Color;
use lumen_paint::{DisplayCommand, DisplayList};

/// Ширина scrollbar-а в CSS px. 8 px — компромисс между видимостью и
/// неинтрузивностью; примерно как у браузерных overlay-scrollbar-ов.
pub const SCROLLBAR_WIDTH: f32 = 8.0;

/// Минимальная высота thumb-а в CSS px. На очень длинных страницах
/// пропорциональная высота уходит к 1-2 px — клик/визуальный feedback
/// невозможен. 24 px — практический минимум.
pub const MIN_THUMB_HEIGHT: f32 = 24.0;

/// Track-фон: тёмный, низкий alpha — еле заметен, но даёт точку отсчёта.
const TRACK_COLOR: Color = Color { r: 0, g: 0, b: 0, a: 28 };

/// Thumb-цвет: тёмный, полупрозрачный — виден поверх и светлого, и тёмного
/// контента без лишнего контраста.
const THUMB_COLOR: Color = Color { r: 0, g: 0, b: 0, a: 120 };

/// Собрать display-command-ы scrollbar-а для подмешивания в overlay.
///
/// Возвращает пустой Vec, если scrollbar не нужен:
/// - контент помещается в viewport (`content_height <= viewport_height`);
/// - viewport вырожден (`width <= SCROLLBAR_WIDTH` или `height <= 0`).
///
/// `scroll_y` ожидается клампленым в `[0, content_height - viewport_height]`;
/// функция всё равно clamp-ит ratio в `[0, 1]` на случай float-погрешностей
/// в caller-е.
pub fn build_scrollbar_overlay(
    scroll_y: f32,
    content_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> DisplayList {
    if !content_height.is_finite()
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || !scroll_y.is_finite()
    {
        return Vec::new();
    }
    if viewport_height <= 0.0 || viewport_width <= SCROLLBAR_WIDTH {
        return Vec::new();
    }
    if content_height <= viewport_height {
        return Vec::new();
    }

    let (thumb_top, thumb_height) =
        thumb_geometry(scroll_y, content_height, viewport_height);

    let track_x = viewport_width - SCROLLBAR_WIDTH;
    vec![
        DisplayCommand::FillRect {
            rect: Rect::new(track_x, 0.0, SCROLLBAR_WIDTH, viewport_height),
            color: TRACK_COLOR,
        },
        DisplayCommand::FillRect {
            rect: Rect::new(track_x, thumb_top, SCROLLBAR_WIDTH, thumb_height),
            color: THUMB_COLOR,
        },
    ]
}

/// Pure-fn геометрия thumb-а — `(top, height)` в координатах overlay.
/// Вынесена отдельно для отдельного тестирования формул, без сборки
/// display-command-ов. Caller обязан сам проверить, что scrollbar вообще
/// нужен (см. `build_scrollbar_overlay`).
pub fn thumb_geometry(
    scroll_y: f32,
    content_height: f32,
    viewport_height: f32,
) -> (f32, f32) {
    let proportional = viewport_height * viewport_height / content_height;
    let thumb_h = proportional.max(MIN_THUMB_HEIGHT).min(viewport_height);

    let max_scroll = (content_height - viewport_height).max(0.0);
    let ratio = if max_scroll > 0.0 {
        (scroll_y / max_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let max_thumb_top = (viewport_height - thumb_h).max(0.0);
    (max_thumb_top * ratio, thumb_h)
}

/// Проверить, попадает ли точка `(point_x, point_y)` в текущий thumb-rect.
/// Используется на MouseDown для решения «начинать ли drag». Координаты в
/// тех же CSS px, что и `build_scrollbar_overlay`. Возвращает `false`, если
/// scrollbar вообще не отображается (контент короче viewport-а / вырожденный
/// viewport / NaN-Inf), либо если точка лежит вне track-полосы.
pub fn thumb_hit_test(
    point_x: f32,
    point_y: f32,
    scroll_y: f32,
    content_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> bool {
    if !point_x.is_finite() || !point_y.is_finite() {
        return false;
    }
    if !content_height.is_finite()
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || !scroll_y.is_finite()
    {
        return false;
    }
    if viewport_height <= 0.0 || viewport_width <= SCROLLBAR_WIDTH {
        return false;
    }
    if content_height <= viewport_height {
        return false;
    }

    let track_x = viewport_width - SCROLLBAR_WIDTH;
    if point_x < track_x || point_x >= viewport_width {
        return false;
    }

    let (thumb_top, thumb_h) = thumb_geometry(scroll_y, content_height, viewport_height);
    point_y >= thumb_top && point_y < thumb_top + thumb_h
}

/// Снапшот состояния на момент начала drag-а: scroll_y страницы и cursor_y
/// (оба в CSS px). При каждом MouseMove caller передаёт текущий cursor_y и
/// получает обратно желаемый `scroll_y` через `scroll_for` (без clamp — caller
/// уже умеет clamp-ить в `[0, max_scroll]`).
///
/// Drag-логика: сдвиг курсора на ΔY пикселей соответствует сдвигу scroll-а
/// на `ΔY × (max_scroll / track_range)`, где `track_range = vh − thumb_h`.
/// Это гарантирует, что под курсором всегда остаётся та же точка thumb-а,
/// в которую кликнули — стандартный paradigm scrollbar-а у всех браузеров.
#[derive(Debug, Clone, Copy)]
pub struct ScrollDrag {
    pub start_scroll_y: f32,
    pub start_mouse_y: f32,
}

impl ScrollDrag {
    pub fn new(start_scroll_y: f32, start_mouse_y: f32) -> Self {
        Self { start_scroll_y, start_mouse_y }
    }

    /// Желаемый `scroll_y` при текущей позиции курсора. Если scrollbar
    /// вырожден (content помещается в viewport, или viewport нулевой) —
    /// возвращает исходный `start_scroll_y` без сдвига. Caller отвечает
    /// за clamp в `[0, max_scroll]`.
    pub fn scroll_for(
        &self,
        current_mouse_y: f32,
        content_height: f32,
        viewport_height: f32,
    ) -> f32 {
        if !current_mouse_y.is_finite()
            || !content_height.is_finite()
            || !viewport_height.is_finite()
        {
            return self.start_scroll_y;
        }
        if viewport_height <= 0.0 || content_height <= viewport_height {
            return self.start_scroll_y;
        }

        let (_, thumb_h) = thumb_geometry(self.start_scroll_y, content_height, viewport_height);
        let track_range = viewport_height - thumb_h;
        if track_range <= 0.0 {
            return self.start_scroll_y;
        }

        let max_scroll = content_height - viewport_height;
        let scroll_per_pixel = max_scroll / track_range;
        let delta_mouse_y = current_mouse_y - self.start_mouse_y;
        self.start_scroll_y + delta_mouse_y * scroll_per_pixel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn empty_when_content_fits() {
        // Контент короче viewport-а — scrollbar не нужен.
        assert!(build_scrollbar_overlay(0.0, 500.0, 800.0, 600.0).is_empty());
        // Контент = viewport — тоже не нужен.
        assert!(build_scrollbar_overlay(0.0, 600.0, 800.0, 600.0).is_empty());
    }

    #[test]
    fn empty_when_viewport_degenerate() {
        assert!(build_scrollbar_overlay(0.0, 1000.0, 0.0, 600.0).is_empty());
        assert!(build_scrollbar_overlay(0.0, 1000.0, 800.0, 0.0).is_empty());
        // viewport_width <= SCROLLBAR_WIDTH — рисовать некуда.
        assert!(build_scrollbar_overlay(0.0, 1000.0, SCROLLBAR_WIDTH, 600.0).is_empty());
    }

    #[test]
    fn empty_on_nan_or_inf() {
        assert!(build_scrollbar_overlay(f32::NAN, 1000.0, 800.0, 600.0).is_empty());
        assert!(build_scrollbar_overlay(0.0, f32::INFINITY, 800.0, 600.0).is_empty());
        assert!(build_scrollbar_overlay(0.0, 1000.0, f32::NAN, 600.0).is_empty());
        assert!(build_scrollbar_overlay(0.0, 1000.0, 800.0, f32::NAN).is_empty());
    }

    #[test]
    fn emits_track_and_thumb() {
        let dl = build_scrollbar_overlay(0.0, 1200.0, 800.0, 600.0);
        assert_eq!(dl.len(), 2);
        // Track первый — он рисуется ПОД thumb-ом.
        match &dl[0] {
            DisplayCommand::FillRect { rect, color } => {
                assert!(approx_eq(rect.x, 800.0 - SCROLLBAR_WIDTH));
                assert!(approx_eq(rect.y, 0.0));
                assert!(approx_eq(rect.width, SCROLLBAR_WIDTH));
                assert!(approx_eq(rect.height, 600.0));
                assert_eq!(*color, TRACK_COLOR);
            }
            _ => panic!("expected track FillRect"),
        }
        match &dl[1] {
            DisplayCommand::FillRect { color, .. } => {
                assert_eq!(*color, THUMB_COLOR);
            }
            _ => panic!("expected thumb FillRect"),
        }
    }

    #[test]
    fn thumb_at_top_when_scroll_zero() {
        let (top, _h) = thumb_geometry(0.0, 1200.0, 600.0);
        assert!(approx_eq(top, 0.0));
    }

    #[test]
    fn thumb_at_bottom_when_scroll_max() {
        let max_scroll = 1200.0 - 600.0;
        let (top, h) = thumb_geometry(max_scroll, 1200.0, 600.0);
        // top + thumb_h должно достигать ровно viewport_height.
        assert!(approx_eq(top + h, 600.0));
    }

    #[test]
    fn thumb_height_proportional() {
        // viewport=600, content=1200 → proportional = 600²/1200 = 300.
        let (_top, h) = thumb_geometry(0.0, 1200.0, 600.0);
        assert!(approx_eq(h, 300.0));
    }

    #[test]
    fn thumb_height_clamped_to_minimum() {
        // viewport=600, content=600_000 → proportional = 0.6, clamp до MIN_THUMB_HEIGHT.
        let (_top, h) = thumb_geometry(0.0, 600_000.0, 600.0);
        assert!(approx_eq(h, MIN_THUMB_HEIGHT));
    }

    #[test]
    fn thumb_position_midway() {
        // На середине scroll-диапазона thumb должен быть на середине свободного
        // пробега `viewport - thumb_h`.
        let content = 1200.0;
        let viewport = 600.0;
        let max_scroll = content - viewport; // 600
        let (top, h) = thumb_geometry(max_scroll / 2.0, content, viewport);
        let max_thumb_top = viewport - h;
        assert!(approx_eq(top, max_thumb_top / 2.0));
    }

    #[test]
    fn thumb_position_clamped_for_overscroll() {
        // Если caller передал scroll_y > max_scroll (теоретически невозможно
        // после clamp_scroll, но защищаемся), thumb остаётся в нижней позиции.
        let (top, h) = thumb_geometry(99_999.0, 1200.0, 600.0);
        assert!(approx_eq(top + h, 600.0));
    }

    #[test]
    fn thumb_position_clamped_for_negative_scroll() {
        let (top, _h) = thumb_geometry(-50.0, 1200.0, 600.0);
        assert!(approx_eq(top, 0.0));
    }

    #[test]
    fn track_at_right_edge() {
        // viewport_width=1024 → track_x = 1016.
        let dl = build_scrollbar_overlay(0.0, 1200.0, 1024.0, 600.0);
        let DisplayCommand::FillRect { rect, .. } = &dl[0] else {
            panic!("expected FillRect");
        };
        assert!(approx_eq(rect.x, 1024.0 - SCROLLBAR_WIDTH));
        assert!(approx_eq(rect.width, SCROLLBAR_WIDTH));
    }

    #[test]
    fn thumb_min_height_still_reaches_endpoints() {
        // Длинная страница, thumb минимальной высоты — но top=0 на scroll=0
        // и top+h=viewport на scroll=max_scroll.
        let content = 600_000.0;
        let viewport = 600.0;
        let max_scroll = content - viewport;

        let (top0, h0) = thumb_geometry(0.0, content, viewport);
        assert!(approx_eq(top0, 0.0));
        assert!(approx_eq(h0, MIN_THUMB_HEIGHT));

        let (top_end, h_end) = thumb_geometry(max_scroll, content, viewport);
        assert!(approx_eq(top_end + h_end, viewport));
    }

    // ─── thumb_hit_test ───────────────────────────────────────────────────

    #[test]
    fn hit_test_inside_thumb() {
        // viewport 800×600, content 1200. Thumb-h=300, top=0 при scroll=0.
        // Track x в [792, 800). Точка (796, 100) внутри thumb-а.
        assert!(thumb_hit_test(796.0, 100.0, 0.0, 1200.0, 800.0, 600.0));
    }

    #[test]
    fn hit_test_outside_thumb_vertically() {
        // Та же конфигурация, точка (796, 400) ниже thumb-а (thumb ends at 300).
        assert!(!thumb_hit_test(796.0, 400.0, 0.0, 1200.0, 800.0, 600.0));
    }

    #[test]
    fn hit_test_outside_track_horizontally() {
        // viewport_width=800, track_x=792. Точка (700, 100) левее track-а.
        assert!(!thumb_hit_test(700.0, 100.0, 0.0, 1200.0, 800.0, 600.0));
        // Точка (800, 100) на правом краю — exclusive, не попадает.
        assert!(!thumb_hit_test(800.0, 100.0, 0.0, 1200.0, 800.0, 600.0));
    }

    #[test]
    fn hit_test_follows_thumb_position() {
        // Когда scroll прокручен на max, thumb внизу. Точка наверху больше
        // не попадает; точка внизу — попадает.
        let content = 1200.0;
        let viewport = 600.0;
        let max_scroll = content - viewport;
        assert!(!thumb_hit_test(796.0, 100.0, max_scroll, content, 800.0, viewport));
        assert!(thumb_hit_test(796.0, 500.0, max_scroll, content, 800.0, viewport));
    }

    #[test]
    fn hit_test_false_when_no_scrollbar() {
        // Контент помещается — scrollbar скрыт, hit-test всегда false.
        assert!(!thumb_hit_test(796.0, 100.0, 0.0, 500.0, 800.0, 600.0));
        assert!(!thumb_hit_test(796.0, 100.0, 0.0, 600.0, 800.0, 600.0));
    }

    #[test]
    fn hit_test_false_on_nan() {
        assert!(!thumb_hit_test(f32::NAN, 100.0, 0.0, 1200.0, 800.0, 600.0));
        assert!(!thumb_hit_test(796.0, f32::NAN, 0.0, 1200.0, 800.0, 600.0));
        assert!(!thumb_hit_test(796.0, 100.0, f32::NAN, 1200.0, 800.0, 600.0));
    }

    #[test]
    fn hit_test_false_on_degenerate_viewport() {
        assert!(!thumb_hit_test(796.0, 100.0, 0.0, 1200.0, 800.0, 0.0));
        assert!(!thumb_hit_test(796.0, 100.0, 0.0, 1200.0, SCROLLBAR_WIDTH, 600.0));
    }

    // ─── ScrollDrag::scroll_for ───────────────────────────────────────────

    #[test]
    fn drag_returns_start_scroll_when_no_movement() {
        // Cursor не двигался — scroll остаётся прежним.
        let drag = ScrollDrag::new(50.0, 150.0);
        let s = drag.scroll_for(150.0, 1200.0, 600.0);
        assert!(approx_eq(s, 50.0));
    }

    #[test]
    fn drag_proportional_to_cursor_delta() {
        // viewport=600, content=1200 → thumb_h=300, track_range=300,
        // max_scroll=600. scroll_per_pixel = 600/300 = 2. Δcursor=+50 →
        // Δscroll = +100.
        let drag = ScrollDrag::new(0.0, 0.0);
        let s = drag.scroll_for(50.0, 1200.0, 600.0);
        assert!(approx_eq(s, 100.0));
    }

    #[test]
    fn drag_negative_cursor_delta_goes_up() {
        // Тащим вверх — scroll уменьшается.
        let drag = ScrollDrag::new(200.0, 100.0);
        let s = drag.scroll_for(50.0, 1200.0, 600.0); // Δcursor = -50 → Δscroll = -100
        assert!(approx_eq(s, 100.0));
    }

    #[test]
    fn drag_from_anywhere_on_thumb_keeps_offset() {
        // Кликнули в середину thumb-а (start_mouse_y=150 при thumb_top=0,
        // thumb_h=300 — середина); сдвинули курсор на +100. scroll должен
        // увеличиться на 100×2 = 200, независимо от того, что клик был
        // не в верхушку thumb-а.
        let drag = ScrollDrag::new(0.0, 150.0);
        let s = drag.scroll_for(250.0, 1200.0, 600.0);
        assert!(approx_eq(s, 200.0));
    }

    #[test]
    fn drag_no_op_when_content_fits() {
        // Контент помещается — drag не должен менять scroll (max_scroll=0).
        let drag = ScrollDrag::new(0.0, 100.0);
        let s = drag.scroll_for(500.0, 500.0, 600.0);
        assert!(approx_eq(s, 0.0));
    }

    #[test]
    fn drag_no_op_when_viewport_degenerate() {
        let drag = ScrollDrag::new(10.0, 100.0);
        assert!(approx_eq(drag.scroll_for(500.0, 1200.0, 0.0), 10.0));
    }

    #[test]
    fn drag_unclamped_for_overscroll() {
        // Drag сам по себе не клампит — caller обязан clamp-нуть в
        // [0, max_scroll]. Тащим за пределы (Δcursor=+1000, scroll_per_pixel=2)
        // → возвращаем 2000, хотя max_scroll=600.
        let drag = ScrollDrag::new(0.0, 0.0);
        let s = drag.scroll_for(1000.0, 1200.0, 600.0);
        assert!(approx_eq(s, 2000.0));
    }

    #[test]
    fn drag_with_min_thumb_height() {
        // На очень длинной странице thumb-h=MIN_THUMB_HEIGHT=24,
        // track_range=576, max_scroll≈599_400. scroll_per_pixel ≈ 1040.6.
        // Δcursor=+1 → Δscroll ≈ +1040.
        let drag = ScrollDrag::new(0.0, 0.0);
        let s = drag.scroll_for(1.0, 600_000.0, 600.0);
        // Проверяем точную формулу: (600_000 - 600) / (600 - 24).
        let expected = (600_000.0 - 600.0) / (600.0 - MIN_THUMB_HEIGHT);
        assert!((s - expected).abs() < 0.1);
    }

    #[test]
    fn drag_nan_inputs_safe() {
        let drag = ScrollDrag::new(50.0, 100.0);
        assert!(approx_eq(drag.scroll_for(f32::NAN, 1200.0, 600.0), 50.0));
        assert!(approx_eq(drag.scroll_for(150.0, f32::NAN, 600.0), 50.0));
        assert!(approx_eq(drag.scroll_for(150.0, 1200.0, f32::NAN), 50.0));
    }
}
