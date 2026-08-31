//! P1/SPLIT-DL12: box-shadow эмиссия — outset (`emit_box_shadows`) и
//! inset (`emit_inset_box_shadows`) тени, `spread_corner_radii`.
//! Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-12).

use super::*;

/// Эмитит outset box-shadow ПЕРЕД background (painter's order по CSS
/// Backgrounds L3 §4.6 — shadow «cast … behind the element», то есть
/// под background-color).
/// * `blur > 0`: shadow рисуется через `PushFilter { Blur(sigma) }` +
///   `FillRect` + `PopFilter`. Renderer применяет двухпроходный Gaussian
///   GPU-шейдер. sigma = blur / 2.0 (CSS Backgrounds L3 §4.6 — blur-radius
///   = standard deviation × 2, аналогично Edge/Chrome/Firefox).
/// * `blur == 0`: резкий `FillRect` напрямую (без offscreen pass).
/// * `inset` тени рисуются отдельно — `emit_inset_box_shadows` после
///   background и до border, по спеке §3.5.1 «inset shadows are drawn
///   inside the box, above the background and below the border».
/// * Multiple shadows: per spec «the first shadow is on top» —
///   эмитим в reverse iter (последняя в CSS-списке рисуется первой /
///   ниже всех, первая — последней-перед-background).
/// * `spread`: расширяет / сжимает rect ± по всем сторонам перед
///   смещением. Полностью схлопывающийся rect (w/h ≤ 0) — skip.
/// * Полностью прозрачная shadow (color.a == 0) — skip.
pub(crate) fn emit_box_shadows(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.box_shadow.is_empty() {
        return;
    }
    for shadow in s.box_shadow.iter().rev() {
        if shadow.inset {
            continue;
        }
        let color = shadow.color.unwrap_or(s.color);
        if color.a == 0 {
            continue;
        }
        // Snap shadow rect to integer CSS pixels — offset/spread are CSS lengths that can be
        // fractional; unsnapped values produce sub-pixel shadows vs Edge (BUG-084 partial).
        let x = (b.rect.x + shadow.offset_x - shadow.spread).round();
        let y = (b.rect.y + shadow.offset_y - shadow.spread).round();
        let w = (b.rect.width + 2.0 * shadow.spread).round();
        let h = (b.rect.height + 2.0 * shadow.spread).round();
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let sigma = shadow.blur / 2.0;
        let shadow_rect = Rect::new(x, y, w, h);
        // CSS Backgrounds L3 §7.1.1: the shadow shape is the border box expanded by
        // `spread`, and each corner with a non-zero border-radius is rounded with its
        // radius increased by the spread distance (square corners stay square). Without
        // this, a hard/blurred shadow on a rounded box renders as a square silhouette.
        let base_radii = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
        if sigma > 0.0 {
            out.push(DisplayCommand::PushFilter {
                filters: vec![FilterFn::Blur(sigma)],
                bounds: Some(shadow_rect),
            });
        }
        if base_radii.all_zero() {
            out.push(DisplayCommand::FillRect {
                rect: shadow_rect,
                color,
            });
        } else {
            out.push(DisplayCommand::FillRoundedRect {
                rect: shadow_rect,
                color,
                radii: spread_corner_radii(&base_radii, shadow.spread),
            });
        }
        if sigma > 0.0 {
            out.push(DisplayCommand::PopFilter);
        }
    }
}

/// Expands a box's resolved `CornerRadii` to the corner radii of its outer
/// box-shadow shape per CSS Backgrounds L3 §7.1.1: a corner with a non-zero
/// border-radius gets its radius increased by the spread distance (clamped at
/// zero for large negative spread); a square corner (radius 0) stays square.
fn spread_corner_radii(base: &CornerRadii, spread: f32) -> CornerRadii {
    let grow = |r: f32| if r > 0.0 { (r + spread).max(0.0) } else { 0.0 };
    CornerRadii {
        tl: grow(base.tl),
        tl_y: grow(base.tl_y),
        tr: grow(base.tr),
        tr_y: grow(base.tr_y),
        br: grow(base.br),
        br_y: grow(base.br_y),
        bl: grow(base.bl),
        bl_y: grow(base.bl_y),
    }
}

/// Эмитит inset box-shadow МЕЖДУ background и border (CSS Backgrounds
/// L3 §3.5.1: «inset shadows are drawn inside the padding edge of the
/// box, above the background but below the border and content»).
///
/// Геометрия per spec:
/// * **outer** = padding-box (border-rect минус border-widths) — это
///   область, в которой видна тень; тень клипается outer-ом.
/// * **inner** = `outer`, **смещённый** на `(offset_x, offset_y)` и
///   **сжатый** на `spread` (положительный spread → меньший inner →
///   шире кольцо тени; отрицательный spread → inner может выйти за
///   outer → тень коллапсирует к нулю).
///
/// Видимая тень = `outer \ (inner ∩ outer)` — кольцо/каёмка. Phase 0
/// без border-radius / blur разворачивается в 4 FillRect-а (top /
/// bottom / left / right), окаймляющие «дырку» внутри outer. Если
/// inner полностью НЕ пересекается с outer — заливаем весь outer
/// одним FillRect (тень закрывает всё). Если inner полностью покрывает
/// outer (отрицательный spread достаточной величины) — ничего не
/// эмитим.
///
/// Multiple inset shadows: тот же reverse-iter, что у outset — «first
/// shadow on top» (последняя в CSS-списке кладётся первой, первая —
/// последней; верхние перекрывают нижние). Несколько inset друг над
/// другом — нормальный паттерн под «двойную» обводку.
///
/// Phase 0 ограничения:
/// * `blur` игнорируется — inset blur требует clip-маски вокруг padding-box,
///   иначе размытие вытекает за границы элемента. Clip-маски будут реализованы
///   как часть stacking context (P1 п.2A). Outset blur реализован через
///   PushFilter/PopFilter без clip.
/// * Полностью прозрачная shadow (`color.a == 0`) — skip.
/// * `currentColor` для `color: None` берётся из `s.color`.
pub(crate) fn emit_inset_box_shadows(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.box_shadow.is_empty() {
        return;
    }
    let outer_x = b.rect.x + s.border_left_width;
    let outer_y = b.rect.y + s.border_top_width;
    let outer_w = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
    let outer_h = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
    if outer_w <= 0.0 || outer_h <= 0.0 {
        return;
    }
    let outer_right = outer_x + outer_w;
    let outer_bottom = outer_y + outer_h;
    for shadow in s.box_shadow.iter().rev() {
        if !shadow.inset {
            continue;
        }
        let color = shadow.color.unwrap_or(s.color);
        if color.a == 0 {
            continue;
        }
        // inner = outer, translated by offset, then inset by spread.
        let inner_x = outer_x + shadow.offset_x + shadow.spread;
        let inner_y = outer_y + shadow.offset_y + shadow.spread;
        let inner_right = outer_right + shadow.offset_x - shadow.spread;
        let inner_bottom = outer_bottom + shadow.offset_y - shadow.spread;
        // Inner полностью покрывает outer — кольцо нулевое, тени не видно.
        if inner_x <= outer_x
            && inner_y <= outer_y
            && inner_right >= outer_right
            && inner_bottom >= outer_bottom
        {
            continue;
        }
        // Inner не пересекает outer — тень покрывает весь outer.
        let no_overlap = inner_x >= outer_right
            || inner_y >= outer_bottom
            || inner_right <= outer_x
            || inner_bottom <= outer_y;
        if no_overlap {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, outer_y, outer_w, outer_h),
                color,
            });
            continue;
        }
        // Hole = inner clamped to outer.
        let hole_left = inner_x.max(outer_x);
        let hole_top = inner_y.max(outer_y);
        let hole_right = inner_right.min(outer_right);
        let hole_bottom = inner_bottom.min(outer_bottom);
        // Top frame.
        if hole_top > outer_y {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, outer_y, outer_w, hole_top - outer_y),
                color,
            });
        }
        // Bottom frame.
        if hole_bottom < outer_bottom {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, hole_bottom, outer_w, outer_bottom - hole_bottom),
                color,
            });
        }
        // Left frame.
        if hole_left > outer_x {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(outer_x, hole_top, hole_left - outer_x, hole_bottom - hole_top),
                color,
            });
        }
        // Right frame.
        if hole_right < outer_right {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(
                    hole_right,
                    hole_top,
                    outer_right - hole_right,
                    hole_bottom - hole_top,
                ),
                color,
            });
        }
    }
}
