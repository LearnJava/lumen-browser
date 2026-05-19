//! Vertical scrollbar в overlay-полосе display list-а: тонкая полоска у
//! правого края viewport-а, показывает текущую `scroll_y` относительно
//! `content_height`. Phase 0 — только visual indicator, без drag-interaction
//! (Mouse-down на thumb → seek будет отдельной задачей).
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
}
