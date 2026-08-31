//! P1/SPLIT-DL11: scrollbar geometry + rendering (`scrollbar_rects`/
//! `emit_scrollbars`) and the overflow-container scroll-layer patch fast
//! path (`scroll_layer_geometry`/`patch_scroll_layer`). Вынесено из
//! `display_list.rs` (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL,
//! батч DL-11).

use super::*;

/// Default scrollbar gutter width for `scrollbar-width: auto` in CSS px.
const SCROLLBAR_WIDTH: f32 = 12.0;
/// Scrollbar gutter width for `scrollbar-width: thin` in CSS px.
pub(crate) const SCROLLBAR_WIDTH_THIN: f32 = 6.0;
/// Minimum thumb length in CSS px so it stays clickable at large scroll ranges.
const SCROLLBAR_MIN_THUMB: f32 = 20.0;
/// Default track color: very light translucent grey.
pub(crate) const SCROLLBAR_TRACK_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.08];
/// Default thumb color: semi-transparent dark pill.
pub(crate) const SCROLLBAR_THUMB_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.38];

/// Convert a CSS `Color` (u8 sRGB) to a linear `[f32; 4]` array for the renderer.
fn color_u8_to_f32(c: Color) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}

/// Input geometry for `scrollbar_rects`.
struct ScrollbarInput {
    /// Padding-box origin and size in document-space CSS px.
    pub clip_x: f32,
    pub clip_y: f32,
    pub clip_w: f32,
    pub clip_h: f32,
    /// Current scroll offset in CSS px.
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Total content width / height in CSS px.
    pub content_w: f32,
    pub content_h: f32,
    /// Emit vertical scrollbar when content_h > clip_h.
    pub need_v: bool,
    /// Emit horizontal scrollbar when content_w > clip_w.
    pub need_h: bool,
    /// Scrollbar gutter width/height in CSS px. From `scrollbar-width`: auto=12, thin=6.
    pub gutter_px: f32,
}

/// One axis result: `(track_rect, thumb_rect)` in document-space CSS px.
type ScrollbarAxis = Option<(Rect, Rect)>;

/// Compute track and thumb rects for the vertical and horizontal scrollbar axes.
///
/// Returns `(vertical, horizontal)` where each is `Some((track, thumb))` if the
/// axis overflows, or `None` if the content fits within the clip rect for that axis.
fn scrollbar_rects(i: &ScrollbarInput) -> (ScrollbarAxis, ScrollbarAxis) {
    let g = i.gutter_px;
    // Minimum thumb length scales with gutter so thin scrollbars stay clickable.
    let min_thumb = SCROLLBAR_MIN_THUMB.min(g * 2.0).max(g);
    // Inset from track edge — 2px for auto, 1px for thin.
    let inset = if g >= 10.0 { 2.0 } else { 1.0 };

    let v = if i.need_v && i.content_h > i.clip_h {
        let track = Rect::new(
            i.clip_x + i.clip_w - g,
            i.clip_y,
            g,
            i.clip_h,
        );
        let thumb_h = ((i.clip_h / i.content_h) * i.clip_h).max(min_thumb).min(i.clip_h);
        let max_scroll = (i.content_h - i.clip_h).max(0.0);
        let thumb_y = if max_scroll > 0.0 {
            i.clip_y + (i.scroll_y / max_scroll) * (i.clip_h - thumb_h)
        } else {
            i.clip_y
        };
        let thumb = Rect::new(
            track.x + inset,
            thumb_y.clamp(i.clip_y, i.clip_y + i.clip_h - thumb_h),
            g - inset * 2.0,
            thumb_h,
        );
        Some((track, thumb))
    } else {
        None
    };

    let h = if i.need_h && i.content_w > i.clip_w {
        let track = Rect::new(
            i.clip_x,
            i.clip_y + i.clip_h - g,
            i.clip_w,
            g,
        );
        let thumb_w = ((i.clip_w / i.content_w) * i.clip_w).max(min_thumb).min(i.clip_w);
        let max_scroll = (i.content_w - i.clip_w).max(0.0);
        let thumb_x = if max_scroll > 0.0 {
            i.clip_x + (i.scroll_x / max_scroll) * (i.clip_w - thumb_w)
        } else {
            i.clip_x
        };
        let thumb = Rect::new(
            thumb_x.clamp(i.clip_x, i.clip_x + i.clip_w - thumb_w),
            track.y + inset,
            thumb_w,
            g - inset * 2.0,
        );
        Some((track, thumb))
    } else {
        None
    };

    (v, h)
}

/// Emit `DrawScrollbar` track+thumb commands for a scroll container's padding box.
///
/// Shared by the legacy `walk` path and the ordered (stacking-context)
/// `box_layer_ops` path (BUG-220) so both render identical scrollbars. The
/// caller MUST emit these AFTER `PopScrollLayer`, so the bars stay at a fixed
/// position instead of translating with the scrolled content.
///
/// `padding_box` is `(px, py, pw, ph)` — padding-box origin and size in
/// document-space CSS px (border excluded). Content extent is measured relative
/// to the padding-box origin and floored at the padding-box size, so a border
/// does not inflate `content_w`/`content_h` past the clip and spawn a phantom
/// scrollbar.
///
/// No-op when `scrollbar-width: none` (gutter collapses to 0) — the container
/// still scrolls via keyboard/JS, only the visual bar is suppressed.
pub(crate) fn emit_scrollbars(
    b: &LayoutBox,
    padding_box: (f32, f32, f32, f32),
    is_scroll_x: bool,
    is_scroll_y: bool,
    out: &mut Vec<DisplayCommand>,
) {
    let (px, py, pw, ph) = padding_box;
    let gutter_px = match b.style.scrollbar_width {
        ScrollbarWidth::Auto => SCROLLBAR_WIDTH,
        ScrollbarWidth::Thin => SCROLLBAR_WIDTH_THIN,
        ScrollbarWidth::None => 0.0,
    };
    // Only emit when the scrollbar is visible (gutter_px > 0).
    if gutter_px <= 0.0 {
        return;
    }
    let (thumb_color, track_color) = match b.style.scrollbar_color {
        Some((thumb, track)) => (color_u8_to_f32(thumb), color_u8_to_f32(track)),
        None => (SCROLLBAR_THUMB_COLOR, SCROLLBAR_TRACK_COLOR),
    };
    // Content extent relative to padding-box origin, floored at padding-box size
    // (not border-box): a border must not make content_w exceed clip_w and fake
    // a horizontal scrollbar.
    let content_w = b
        .children
        .iter()
        .fold(pw, |acc, c| acc.max(c.rect.x + c.rect.width - px));
    let content_h = b
        .children
        .iter()
        .fold(ph, |acc, c| acc.max(c.rect.y + c.rect.height - py));
    let (v_bars, h_bars) = scrollbar_rects(&ScrollbarInput {
        clip_x: px,
        clip_y: py,
        clip_w: pw,
        clip_h: ph,
        scroll_x: b.scroll_x,
        scroll_y: b.scroll_y,
        content_w,
        content_h,
        need_v: is_scroll_y,
        need_h: is_scroll_x,
        gutter_px,
    });
    if let Some((track, thumb)) = v_bars {
        out.push(DisplayCommand::DrawScrollbar {
            track_rect: track,
            thumb_rect: thumb,
            vertical: true,
            thumb_color,
            track_color,
        });
    }
    if let Some((track, thumb)) = h_bars {
        out.push(DisplayCommand::DrawScrollbar {
            track_rect: track,
            thumb_rect: thumb,
            vertical: false,
            thumb_color,
            track_color,
        });
    }
}

/// Геометрия scroll-слоя overflow-контейнера — зеркало вычислений
/// `box_layer_ops`, которыми заполняются `PushScrollLayer` и `emit_scrollbars`
/// на ordered-пути. Дрейф с `box_layer_ops` ловят equivalence-тесты
/// `patch_scroll_layer_*` (патч против полной пересборки).
struct ScrollLayerGeometry {
    /// Значение `PushScrollLayer.clip_rect` (может содержать BIG-сентинели).
    clip_rect: Rect,
    /// Padding-box `(px, py, pw, ph)` — вход `emit_scrollbars`.
    padding_box: (f32, f32, f32, f32),
    /// `overflow-x` ∈ {scroll, auto}.
    is_scroll_x: bool,
    /// `overflow-y` ∈ {scroll, auto}.
    is_scroll_y: bool,
}

/// `None`, если бокс не открывает scroll-слой (не скроллится, `contain: paint`,
/// анонимный бокс).
fn scroll_layer_geometry(b: &LayoutBox) -> Option<ScrollLayerGeometry> {
    if !box_can_own_stacking_context(b) {
        return None;
    }
    let s = &b.style;
    let paint_contain = s.contain.0 & ContainFlags::PAINT.0 != 0;
    let clip_x = overflow_clips(s.overflow_x) || paint_contain;
    let clip_y = overflow_clips(s.overflow_y) || paint_contain;
    if !(clip_x || clip_y) {
        return None;
    }
    const BIG: f32 = 1_000_000.0;
    let px = b.rect.x + s.border_left_width;
    let py = b.rect.y + s.border_top_width;
    let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
    let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
    let cr = Rect::new(
        if clip_x { px } else { -BIG },
        if clip_y { py } else { -BIG },
        if clip_x { pw } else { 2.0 * BIG },
        if clip_y { ph } else { 2.0 * BIG },
    );
    let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
    let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
    if (is_scroll_x || is_scroll_y) && !paint_contain {
        Some(ScrollLayerGeometry {
            clip_rect: cr,
            padding_box: (px, py, pw, ph),
            is_scroll_x,
            is_scroll_y,
        })
    } else {
        None
    }
}

/// In-place патч скролл-позиции overflow-контейнера в готовом display list —
/// быстрый путь скролла без полной пересборки (`build_display_list_ordered`).
///
/// Полная пересборка после `lumen_layout::set_scroll_position` отличается от
/// старого списка ровно двумя вещами (layout детей не меняется — мутируются
/// только `scroll_x`/`scroll_y` контейнера): значениями скролла в
/// `PushScrollLayer` контейнера (включая BUG-159-переустановленные копии
/// вокруг дочерних stacking context'ов — у них тот же `clip_rect`) и
/// thumb-прямоугольниками его `DrawScrollbar`. Патч выполняет обе правки теми
/// же хелперами, что и построитель (`scroll_layer_geometry` /
/// `emit_scrollbars`), поэтому результат побайтно совпадает с пересборкой.
///
/// Возвращает `false`, если ожидания не сошлись (контейнер не найден,
/// найденные слои несут разные старые значения скролла, набор скроллбаров не
/// совпал по числу) — вызывающий обязан выполнить полную пересборку.
pub fn patch_scroll_layer(dl: &mut DisplayList, b: &LayoutBox) -> bool {
    let Some(g) = scroll_layer_geometry(b) else {
        return false;
    };
    let cr = g.clip_rect;
    let same_rect = |r: &Rect| {
        r.x.to_bits() == cr.x.to_bits()
            && r.y.to_bits() == cr.y.to_bits()
            && r.width.to_bits() == cr.width.to_bits()
            && r.height.to_bits() == cr.height.to_bits()
    };
    // Все PushScrollLayer контейнера: оригинал + переустановленные (BUG-159).
    // Они — клоны одной команды, поэтому старые значения скролла обязаны
    // совпадать; расхождение значит, что clip_rect делят разные контейнеры.
    let mut push_idxs: Vec<usize> = Vec::new();
    let mut old_scroll: Option<(u32, u32)> = None;
    for (i, cmd) in dl.iter().enumerate() {
        if let DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } = cmd
            && same_rect(clip_rect)
        {
            let sxy = (scroll_x.to_bits(), scroll_y.to_bits());
            match old_scroll {
                None => old_scroll = Some(sxy),
                Some(prev) if prev == sxy => {}
                Some(_) => return false,
            }
            push_idxs.push(i);
        }
    }
    let Some(&first_push) = push_idxs.first() else {
        return false;
    };
    // Балансирующий PopScrollLayer оригинального (первого) слоя.
    let mut depth = 0usize;
    let mut pop_idx = None;
    for (i, cmd) in dl.iter().enumerate().skip(first_push) {
        match cmd {
            DisplayCommand::PushScrollLayer { .. } => depth += 1,
            DisplayCommand::PopScrollLayer => {
                depth -= 1;
                if depth == 0 {
                    pop_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(pop_idx) = pop_idx else {
        return false;
    };
    // Скроллбары контейнера лежат подряд сразу после PopScrollLayer
    // (`box_layer_ops` кладёт их в overflow_post). Состав баров не зависит от
    // скролла (только от геометрии контента), поэтому пересобранный тем же
    // хелпером набор обязан совпасть по числу команд.
    let mut fresh: DisplayList = Vec::new();
    emit_scrollbars(b, g.padding_box, g.is_scroll_x, g.is_scroll_y, &mut fresh);
    let bars_start = pop_idx + 1;
    let mut bars_end = bars_start;
    while bars_end < dl.len() && matches!(dl[bars_end], DisplayCommand::DrawScrollbar { .. }) {
        bars_end += 1;
    }
    if bars_end - bars_start != fresh.len() {
        return false;
    }
    for (slot, new_cmd) in dl[bars_start..bars_end].iter_mut().zip(fresh) {
        *slot = new_cmd;
    }
    for &i in &push_idxs {
        if let DisplayCommand::PushScrollLayer { scroll_x, scroll_y, .. } = &mut dl[i] {
            *scroll_x = b.scroll_x;
            *scroll_y = b.scroll_y;
        }
    }
    true
}
