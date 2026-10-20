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
        // BUG-1302: «the shadow is only drawn outside the border box» — the part under the
        // box is clipped away, otherwise it shows through a translucent background / the
        // transparent middle of a `double` border.
        let reach = if sigma > 0.0 { (3.0 * sigma).ceil() + 1.0 } else { 0.0 };
        let hole = if shadow_needs_cutout(b, shadow_rect, reach) {
            Some(b.rect)
        } else {
            None
        };
        if let Some(hole) = hole
            && sigma <= 0.0
            && base_radii.all_zero()
        {
            push_rect_minus_rect(shadow_rect, hole, color, out);
            continue;
        }
        if let Some(hole) = hole {
            let frame = Rect::new(
                shadow_rect.x - reach,
                shadow_rect.y - reach,
                shadow_rect.width + 2.0 * reach,
                shadow_rect.height + 2.0 * reach,
            );
            out.push(DisplayCommand::PushClipPath {
                shape: ResolvedClipShape::Polygon {
                    verts: frame_with_hole(frame, hole, &base_radii),
                    even_odd: true,
                },
            });
        }
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
        if hole.is_some() {
            out.push(DisplayCommand::PopClip);
        }
    }
}

/// Нужно ли вырезать border box из внешней тени (BUG-1302). Нет, если тень
/// (с размытием `reach`) не заходит под бокс, или если бокс закрывает её
/// непрозрачным фоном по border-box (фон рисуется поверх тени — вырез
/// невидим, а клип по контуру стоил бы offscreen-слоя).
fn shadow_needs_cutout(b: &LayoutBox, shadow_rect: Rect, reach: f32) -> bool {
    let r = b.rect;
    let overlaps = shadow_rect.x - reach < r.x + r.width
        && shadow_rect.x + shadow_rect.width + reach > r.x
        && shadow_rect.y - reach < r.y + r.height
        && shadow_rect.y + shadow_rect.height + reach > r.y;
    if !overlaps {
        return false;
    }
    let opaque_bg = b
        .style
        .background_color
        .and_then(|c| c.to_color_opt())
        .is_some_and(|c| c.a == 255)
        && background_color_clip(b) == BackgroundClip::BorderBox;
    !opaque_bg
}

/// `outer` без `hole` как до четырёх `FillRect` (оба — прямоугольники без скруглений).
fn push_rect_minus_rect(outer: Rect, hole: Rect, color: Color, out: &mut Vec<DisplayCommand>) {
    let ix0 = outer.x.max(hole.x);
    let iy0 = outer.y.max(hole.y);
    let ix1 = (outer.x + outer.width).min(hole.x + hole.width);
    let iy1 = (outer.y + outer.height).min(hole.y + hole.height);
    let mut fill = |x: f32, y: f32, w: f32, h: f32| {
        if w > 0.0 && h > 0.0 {
            out.push(DisplayCommand::FillRect { rect: Rect::new(x, y, w, h), color });
        }
    };
    if ix0 >= ix1 || iy0 >= iy1 {
        fill(outer.x, outer.y, outer.width, outer.height);
        return;
    }
    let (ox1, oy1) = (outer.x + outer.width, outer.y + outer.height);
    fill(outer.x, outer.y, outer.width, iy0 - outer.y);
    fill(outer.x, iy1, outer.width, oy1 - iy1);
    fill(outer.x, iy0, ix0 - outer.x, iy1 - iy0);
    fill(ix1, iy0, ox1 - ix1, iy1 - iy0);
}

/// Сегментов на четверть дуги скруглённого угла в контуре выреза.
const CUTOUT_ARC_STEPS: usize = 16;

/// Контур `rect` со скруглёнными углами по часовой стрелке (экранные координаты).
fn rounded_rect_outline(rect: Rect, radii: &CornerRadii) -> Vec<(f32, f32)> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let r = radii.clamped_to_box(rect.width, rect.height);
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    // (центр, rx, ry, начальный угол) для TL, TR, BR, BL.
    let corners = [
        ((x0 + r.tl, y0 + r.tl_y), r.tl, r.tl_y, PI),
        ((x1 - r.tr, y0 + r.tr_y), r.tr, r.tr_y, 1.5 * PI),
        ((x1 - r.br, y1 - r.br_y), r.br, r.br_y, 0.0),
        ((x0 + r.bl, y1 - r.bl_y), r.bl, r.bl_y, FRAC_PI_2),
    ];
    let mut pts = Vec::with_capacity(4 * (CUTOUT_ARC_STEPS + 1));
    for ((cx, cy), rx, ry, a0) in corners {
        if rx <= 0.0 || ry <= 0.0 {
            pts.push((cx, cy));
            continue;
        }
        for i in 0..=CUTOUT_ARC_STEPS {
            let t = a0 + FRAC_PI_2 * (i as f32 / CUTOUT_ARC_STEPS as f32);
            pts.push((cx + rx * t.cos(), cy + ry * t.sin()));
        }
    }
    pts
}

/// Один многоугольник «рамка минус вырез» для even-odd клипа: внешний
/// прямоугольник, мостик к контуру выреза и обратно по тем же точкам
/// (мостик нулевой площади).
fn frame_with_hole(frame: Rect, hole: Rect, radii: &CornerRadii) -> Vec<(f32, f32)> {
    let (fx0, fy0) = (frame.x, frame.y);
    let (fx1, fy1) = (frame.x + frame.width, frame.y + frame.height);
    let ring = rounded_rect_outline(hole, radii);
    let first = ring[0];
    let mut v = vec![(fx0, fy0), (fx1, fy0), (fx1, fy1), (fx0, fy1), (fx0, fy0), first];
    v.extend_from_slice(&ring[1..]);
    v.push(first);
    v.push((fx0, fy0));
    v
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
