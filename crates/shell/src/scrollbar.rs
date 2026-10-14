//! Vertical scrollbar в overlay-полосе display list-а: тонкая полоска у
//! правого края viewport-а, показывает текущую `scroll_y` относительно
//! `content_height`. `classify_track_click` — единая точка решения для
//! MouseDown: thumb / track-выше / track-ниже / мимо; `ScrollDrag` хранит
//! origin-снапшот и через `scroll_for` отдаёт unclamped `scroll_y` под
//! текущую позицию курсора (для drag-режима).
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

/// Thumb под курсором — темнее, чтобы было видно, что он «ловит» мышь.
const THUMB_COLOR_HOVER: Color = Color { r: 0, g: 0, b: 0, a: 170 };

/// Thumb зажат (идёт drag) — самый тёмный.
const THUMB_COLOR_ACTIVE: Color = Color { r: 0, g: 0, b: 0, a: 215 };

/// Состояние thumb-а для окраски: покой / курсор над ним / drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThumbState {
    #[default]
    Idle,
    Hover,
    Active,
}

impl ThumbState {
    fn color(self) -> Color {
        match self {
            ThumbState::Idle => THUMB_COLOR,
            ThumbState::Hover => THUMB_COLOR_HOVER,
            ThumbState::Active => THUMB_COLOR_ACTIVE,
        }
    }
}

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
    build_scrollbar_overlay_with_state(
        scroll_y,
        content_height,
        viewport_width,
        viewport_height,
        ThumbState::Idle,
    )
}

/// То же, что [`build_scrollbar_overlay`], но thumb окрашен по `state`.
pub fn build_scrollbar_overlay_with_state(
    scroll_y: f32,
    content_height: f32,
    viewport_width: f32,
    viewport_height: f32,
    state: ThumbState,
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
            color: state.color(),
        },
    ]
}

/// Длина горизонтальной дорожки: ширина viewport-а без уголка, который
/// занимает вертикальная полоса (если она есть).
fn hbar_track_len(viewport_width: f32, vertical_present: bool) -> f32 {
    if vertical_present {
        viewport_width - SCROLLBAR_WIDTH
    } else {
        viewport_width
    }
}

/// Горизонтальная полоса нужна: размеры конечны, контент шире viewport-а, а
/// сам viewport выше полосы. Возвращает длину дорожки.
fn hbar_track(
    scroll_x: f32,
    content_width: f32,
    viewport_width: f32,
    viewport_height: f32,
    vertical_present: bool,
) -> Option<f32> {
    if !scroll_x.is_finite()
        || !content_width.is_finite()
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
    {
        return None;
    }
    if viewport_height <= SCROLLBAR_WIDTH || viewport_width <= SCROLLBAR_WIDTH {
        return None;
    }
    if content_width <= viewport_width {
        return None;
    }
    let len = hbar_track_len(viewport_width, vertical_present);
    (len > 0.0).then_some(len)
}

/// Горизонтальная полоса прокрутки страницы: дорожка вдоль нижнего края
/// viewport-а, thumb пропорционален `viewport_width / content_width`.
/// `origin_x` — левый край области страницы в координатах overlay-а (ширина
/// левой док-панели); `vertical_present` оставляет уголок вертикальной
/// полосе. Пустой Vec, если контент по ширине помещается.
pub fn build_hscrollbar_overlay(
    scroll_x: f32,
    content_width: f32,
    viewport_width: f32,
    viewport_height: f32,
    origin_x: f32,
    vertical_present: bool,
    state: ThumbState,
) -> DisplayList {
    let Some(len) = hbar_track(scroll_x, content_width, viewport_width, viewport_height, vertical_present)
    else {
        return Vec::new();
    };
    let (thumb_left, thumb_w) = thumb_geometry_in_track(scroll_x, content_width, viewport_width, len);
    let track_y = viewport_height - SCROLLBAR_WIDTH;
    vec![
        DisplayCommand::FillRect {
            rect: Rect::new(origin_x, track_y, len, SCROLLBAR_WIDTH),
            color: TRACK_COLOR,
        },
        DisplayCommand::FillRect {
            rect: Rect::new(origin_x + thumb_left, track_y, thumb_w, SCROLLBAR_WIDTH),
            color: state.color(),
        },
    ]
}

/// Геометрия thumb-а, когда длина дорожки `track_len` отличается от длины
/// viewport-а `viewport_len` (уголок под вертикальную полосу).
fn thumb_geometry_in_track(
    scroll: f32,
    content_len: f32,
    viewport_len: f32,
    track_len: f32,
) -> (f32, f32) {
    let thumb = (track_len * viewport_len / content_len)
        .max(MIN_THUMB_HEIGHT)
        .min(track_len);
    let max_scroll = (content_len - viewport_len).max(0.0);
    let ratio = if max_scroll > 0.0 { (scroll / max_scroll).clamp(0.0, 1.0) } else { 0.0 };
    ((track_len - thumb).max(0.0) * ratio, thumb)
}

/// Куда попал клик в горизонтальную дорожку: `Above` = левее thumb-а,
/// `Below` = правее (имена общие с вертикальной классификацией).
/// Координаты те же, что у [`build_hscrollbar_overlay`].
#[allow(clippy::too_many_arguments)]
pub fn classify_hscroll_click(
    point_x: f32,
    point_y: f32,
    scroll_x: f32,
    content_width: f32,
    viewport_width: f32,
    viewport_height: f32,
    origin_x: f32,
    vertical_present: bool,
) -> TrackClick {
    if !point_x.is_finite() || !point_y.is_finite() {
        return TrackClick::None;
    }
    let Some(len) = hbar_track(scroll_x, content_width, viewport_width, viewport_height, vertical_present)
    else {
        return TrackClick::None;
    };
    let track_y = viewport_height - SCROLLBAR_WIDTH;
    let x = point_x - origin_x;
    if point_y < track_y || point_y >= viewport_height || x < 0.0 || x >= len {
        return TrackClick::None;
    }
    let (left, w) = thumb_geometry_in_track(scroll_x, content_width, viewport_width, len);
    if x < left {
        TrackClick::Above
    } else if x >= left + w {
        TrackClick::Below
    } else {
        TrackClick::Thumb
    }
}

/// Целевой `scroll_x` при drag-е горизонтального thumb-а: курсор сдвинулся
/// с `drag.start_mouse_y` (здесь это X) на `current_mouse_x`. Без clamp.
pub fn hscroll_for(
    drag: &ScrollDrag,
    current_mouse_x: f32,
    content_width: f32,
    viewport_width: f32,
    vertical_present: bool,
) -> f32 {
    if !current_mouse_x.is_finite()
        || !content_width.is_finite()
        || !viewport_width.is_finite()
        || viewport_width <= 0.0
        || content_width <= viewport_width
    {
        return drag.start_scroll_y;
    }
    let len = hbar_track_len(viewport_width, vertical_present);
    let (_, thumb) = thumb_geometry_in_track(drag.start_scroll_y, content_width, viewport_width, len);
    let range = len - thumb;
    if range <= 0.0 {
        return drag.start_scroll_y;
    }
    let per_px = (content_width - viewport_width) / range;
    drag.start_scroll_y + (current_mouse_x - drag.start_mouse_y) * per_px
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

/// Результат классификации точки клика по scrollbar-у. `Thumb` — стартуем
/// drag, `Above` / `Below` — делаем page-jump (scroll на ±page_step), `None`
/// — клик мимо scrollbar-а, обычная обработка дальше.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackClick {
    None,
    Thumb,
    Above,
    Below,
}

/// Куда попал клик в scrollbar-track: вне / в thumb / выше thumb / ниже thumb.
///
/// Единая точка решения для MouseDown: caller сразу знает, стартовать ли
/// drag (Thumb) или сделать page-jump (Above/Below). Координаты в тех же
/// CSS px, что и `build_scrollbar_overlay`. NaN-Inf / контент-помещается /
/// вырожденный viewport / point вне track → `None`.
pub fn classify_track_click(
    point_x: f32,
    point_y: f32,
    scroll_y: f32,
    content_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> TrackClick {
    if !point_x.is_finite() || !point_y.is_finite() {
        return TrackClick::None;
    }
    if !content_height.is_finite()
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || !scroll_y.is_finite()
    {
        return TrackClick::None;
    }
    if viewport_height <= 0.0 || viewport_width <= SCROLLBAR_WIDTH {
        return TrackClick::None;
    }
    if content_height <= viewport_height {
        return TrackClick::None;
    }

    let track_x = viewport_width - SCROLLBAR_WIDTH;
    if point_x < track_x || point_x >= viewport_width {
        return TrackClick::None;
    }
    if point_y < 0.0 || point_y >= viewport_height {
        return TrackClick::None;
    }

    let (thumb_top, thumb_h) = thumb_geometry(scroll_y, content_height, viewport_height);
    if point_y < thumb_top {
        TrackClick::Above
    } else if point_y >= thumb_top + thumb_h {
        TrackClick::Below
    } else {
        TrackClick::Thumb
    }
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

    // ─── classify_track_click ─────────────────────────────────────────────

    #[test]
    fn classify_thumb_hit() {
        // viewport 800×600, content 1200, scroll=0, thumb_top=0, thumb_h=300.
        // Точка (796, 100) в thumb-е.
        assert_eq!(
            classify_track_click(796.0, 100.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::Thumb
        );
    }

    #[test]
    fn classify_above_thumb() {
        // scroll прокручен — thumb внизу. Точка (796, 100) на track выше thumb-а.
        let content = 1200.0;
        let viewport = 600.0;
        let max_scroll = content - viewport;
        assert_eq!(
            classify_track_click(796.0, 100.0, max_scroll, content, 800.0, viewport),
            TrackClick::Above
        );
    }

    #[test]
    fn classify_below_thumb() {
        // scroll=0, thumb сверху (0..300). Точка (796, 500) на track ниже thumb-а.
        assert_eq!(
            classify_track_click(796.0, 500.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::Below
        );
    }

    #[test]
    fn classify_outside_track_horizontally() {
        assert_eq!(
            classify_track_click(700.0, 100.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::None
        );
        // Правый край — exclusive, не track.
        assert_eq!(
            classify_track_click(800.0, 100.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::None
        );
    }

    #[test]
    fn classify_outside_viewport_vertically() {
        // Над верхом и под низом viewport-а — не track.
        assert_eq!(
            classify_track_click(796.0, -10.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::None
        );
        assert_eq!(
            classify_track_click(796.0, 700.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::None
        );
    }

    #[test]
    fn classify_none_when_no_scrollbar() {
        // Контент помещается — scrollbar скрыт, любая точка → None.
        assert_eq!(
            classify_track_click(796.0, 100.0, 0.0, 500.0, 800.0, 600.0),
            TrackClick::None
        );
    }

    #[test]
    fn classify_none_on_nan() {
        assert_eq!(
            classify_track_click(f32::NAN, 100.0, 0.0, 1200.0, 800.0, 600.0),
            TrackClick::None
        );
        assert_eq!(
            classify_track_click(796.0, 100.0, f32::NAN, 1200.0, 800.0, 600.0),
            TrackClick::None
        );
    }

    #[test]
    fn classify_none_on_degenerate_viewport() {
        assert_eq!(
            classify_track_click(796.0, 100.0, 0.0, 1200.0, 800.0, 0.0),
            TrackClick::None
        );
        assert_eq!(
            classify_track_click(796.0, 100.0, 0.0, 1200.0, SCROLLBAR_WIDTH, 600.0),
            TrackClick::None
        );
    }

    #[test]
    fn drag_nan_inputs_safe() {
        let drag = ScrollDrag::new(50.0, 100.0);
        assert!(approx_eq(drag.scroll_for(f32::NAN, 1200.0, 600.0), 50.0));
        assert!(approx_eq(drag.scroll_for(150.0, f32::NAN, 600.0), 50.0));
        assert!(approx_eq(drag.scroll_for(150.0, 1200.0, f32::NAN), 50.0));
    }

    #[test]
    fn hbar_empty_when_content_fits() {
        assert!(build_hscrollbar_overlay(0.0, 800.0, 800.0, 600.0, 0.0, false, ThumbState::Idle).is_empty());
    }

    #[test]
    fn hbar_at_bottom_edge_with_corner() {
        let cmds = build_hscrollbar_overlay(0.0, 2000.0, 1000.0, 600.0, 0.0, true, ThumbState::Idle);
        assert_eq!(cmds.len(), 2);
        let DisplayCommand::FillRect { rect, .. } = &cmds[0] else { panic!() };
        assert!(approx_eq(rect.y, 600.0 - SCROLLBAR_WIDTH));
        assert!(approx_eq(rect.width, 1000.0 - SCROLLBAR_WIDTH));
    }

    #[test]
    fn hbar_thumb_reaches_end_at_max_scroll() {
        let cmds = build_hscrollbar_overlay(1000.0, 2000.0, 1000.0, 600.0, 10.0, true, ThumbState::Idle);
        let DisplayCommand::FillRect { rect, .. } = &cmds[1] else { panic!() };
        assert!(approx_eq(rect.x + rect.width, 10.0 + 1000.0 - SCROLLBAR_WIDTH));
    }

    #[test]
    fn hbar_classify_and_drag() {
        let (cw, vw, vh) = (2000.0, 1000.0, 600.0);
        let y = vh - 2.0;
        assert_eq!(classify_hscroll_click(5.0, y, 0.0, cw, vw, vh, 0.0, false), TrackClick::Thumb);
        assert_eq!(classify_hscroll_click(900.0, y, 0.0, cw, vw, vh, 0.0, false), TrackClick::Below);
        assert_eq!(classify_hscroll_click(5.0, y, 500.0, cw, vw, vh, 0.0, false), TrackClick::Above);
        assert_eq!(classify_hscroll_click(5.0, 10.0, 0.0, cw, vw, vh, 0.0, false), TrackClick::None);
        // Thumb 500 px из дорожки 1000: сдвиг на 250 px = 500 px контента... (1000/500 = 2x).
        let drag = ScrollDrag::new(0.0, 5.0);
        assert!(approx_eq(hscroll_for(&drag, 255.0, cw, vw, false), 500.0));
        assert!(approx_eq(hscroll_for(&drag, 5.0, cw, vw, false), 0.0));
    }

    #[test]
    fn thumb_state_darkens() {
        let a = |s: ThumbState| s.color().a;
        assert!(a(ThumbState::Idle) < a(ThumbState::Hover) && a(ThumbState::Hover) < a(ThumbState::Active));
        let cmds = build_scrollbar_overlay_with_state(0.0, 4000.0, 800.0, 600.0, ThumbState::Active);
        let DisplayCommand::FillRect { color, .. } = &cmds[1] else { panic!() };
        assert_eq!(*color, ThumbState::Active.color());
    }
}
