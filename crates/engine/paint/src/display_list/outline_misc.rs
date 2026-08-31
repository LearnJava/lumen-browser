//! P1/SPLIT-DL11: outline (`emit_outline`), resize-grip (`emit_resize_grip`/
//! `point_on_resize_grip`), multi-column `column-rule` separators
//! (`emit_column_rules`) и paint-visibility helpers (`is_paint_visible`/
//! `is_opacity_subtree_painted`). Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-11).

use super::*;

pub(crate) fn emit_outline(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if !s.outline_style.is_visible() || s.outline_width <= 0.0 {
        return;
    }
    let color = match s.outline_color {
        OutlineColor::Color(c) => c,
        OutlineColor::Auto | OutlineColor::CurrentColor => s.color,
    };
    out.push(DisplayCommand::DrawOutline {
        rect: b.rect,
        width: s.outline_width,
        style: s.outline_style,
        color,
        offset: s.outline_offset.px(),
    });
}

/// Рисует grip для resize property на overflow≠visible элементах.
/// 12px grip в углу как FillRoundedRect. // CSS: resize
pub(crate) fn emit_resize_grip(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;

    // resize свойство должно быть не None и overflow не Visible
    if s.resize == Resize::None {
        return;
    }

    // Проверяем, что overflow != Visible (есть прокрутка или обрезание)
    let overflow_x_hidden = matches!(s.overflow_x, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll);
    let overflow_y_hidden = matches!(s.overflow_y, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll);

    if !overflow_x_hidden && !overflow_y_hidden {
        return;
    }

    // 12px grip в углу (bottom-right по умолчанию)
    let grip_size = 12.0;
    let grip_x = b.rect.x + b.rect.width - grip_size;
    let grip_y = b.rect.y + b.rect.height - grip_size;

    // Рисуем grip как белый закруглённый квадрат (Phase 0)
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect { x: grip_x, y: grip_y, width: grip_size, height: grip_size },
        color: Color { r: 200, g: 200, b: 200, a: 255 },
        radii: CornerRadii { tl: 2.0, tl_y: 2.0, tr: 2.0, tr_y: 2.0, br: 2.0, br_y: 2.0, bl: 2.0, bl_y: 2.0 },
    });
}

/// Возвращает `true`, если точка (`px`, `py`) попадает в resize-grip элемента.
///
/// Grip — это 12×12 px область в правом нижнем углу `b.rect`. Присутствует
/// только когда `resize != None` и хотя бы одна ось `overflow` ≠ Visible.
pub fn point_on_resize_grip(b: &LayoutBox, px: f32, py: f32) -> bool {
    let s = &b.style;
    if s.resize == Resize::None {
        return false;
    }
    let overflow_hidden = matches!(s.overflow_x, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll)
        || matches!(s.overflow_y, Overflow::Hidden | Overflow::Clip | Overflow::Auto | Overflow::Scroll);
    if !overflow_hidden {
        return false;
    }
    let grip_size = 12.0_f32;
    let grip_x = b.rect.x + b.rect.width - grip_size;
    let grip_y = b.rect.y + b.rect.height - grip_size;
    px >= grip_x && px < grip_x + grip_size && py >= grip_y && py < grip_y + grip_size
}

/// CSS Multi-column Layout L1 §3.3 — рисует разделители колонок
/// (`column-rule`) между каждой парой соседних колонок.
///
/// Разделитель центрируется в gap между колонками. Геометрия колонок
/// вычисляется заново по тем же формулам, что и в `lay_out_multicol_children`,
/// поскольку после layout она не сохраняется в LayoutBox.
///
/// Реализует только Solid / Dashed / Dotted через существующий `DrawBorder`
/// (правая сторона rect = rule rect); Double и прочие — как Solid (Phase 0).
/// Порядок рисования: после фона и бордера контейнера, перед children
/// (CSS Multi-column L1 §3.3: «above the border of the multi-column element»).
pub(crate) fn emit_column_rules(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    if s.column_count.is_none() && s.column_width.is_none() {
        return;
    }
    if !s.column_rule_style.is_visible() || s.column_rule_width <= 0.0 {
        return;
    }

    // Content box — mirrors lay_out_multicol_children content_x/y/w/h.
    let em = s.font_size;
    let content_x = b.rect.x + s.border_left_width + s.padding_left.px();
    let content_y = b.rect.y + s.border_top_width + s.padding_top.px();
    let content_w = (b.rect.width
        - s.border_left_width
        - s.border_right_width
        - s.padding_left.px()
        - s.padding_right.px())
    .max(0.0);
    let content_h = (b.rect.height
        - s.border_top_width
        - s.border_bottom_width
        - s.padding_top.px()
        - s.padding_bottom.px())
    .max(0.0);
    if content_w <= 0.0 || content_h <= 0.0 {
        return;
    }

    // Sentinel viewport for length resolution (good enough for px/em/%).
    let vp = Size::new(content_w, content_h);
    let col_gap = s.column_gap.resolve_or_zero(em, content_w, vp).max(0.0);

    // Mirror column count computation from lay_out_multicol_children.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_w), vp)
                && w > 0.0
            {
                let n_from_w = ((content_w + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(content_w), vp)
                && w > 0.0
            {
                ((content_w + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }
    .max(1);

    if n_cols <= 1 || col_gap <= 0.0 {
        return;
    }

    let col_w = ((content_w - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);
    let rule_w = s.column_rule_width;
    let rule_color = s.column_rule_color.resolve(s.color);

    for i in 0..(n_cols - 1) {
        // Left edge of gap after column i.
        let gap_left = content_x + (i + 1) as f32 * col_w + i as f32 * col_gap;
        // Rule centered in the gap.
        let sep_x = gap_left + (col_gap - rule_w) * 0.5;

        // Reuse DrawBorder: emit as right-side only with rect.width = rule_w.
        // Renderer draws right side at: rect.x + rect.width - wr = sep_x ✓.
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(sep_x, content_y, rule_w, content_h),
            widths: [0.0, rule_w, 0.0, 0.0],
            colors: [Color::TRANSPARENT, rule_color, Color::TRANSPARENT, Color::TRANSPARENT],
            styles: [
                BorderStyle::None,
                s.column_rule_style,
                BorderStyle::None,
                BorderStyle::None,
            ],
            radii: CornerRadii::default(),
        });
    }
}

/// CSS Display L3 §4 — `visibility: hidden` (и `collapse` для не-table
/// per spec) делает box-self **не-рисуемым** (background, border,
/// outline, box-shadow, content), но layout остаётся (`Skip` иной
/// семантики). Children по-прежнему обходятся: visibility наследуется,
/// но child может явно вернуть себя через `visibility: visible`.
pub(crate) fn is_paint_visible(b: &LayoutBox) -> bool {
    matches!(b.style.visibility, Visibility::Visible)
}

/// CSS Color L3 §3.2 — `opacity: 0` создаёт stacking context, и после
/// off-screen compositor pass весь subtree даёт fully-transparent
/// результат. Phase 0 без compositor-pass-ов: pure-pixel skip всего
/// subtree (children тоже не рисуются — это отличие от visibility:
/// hidden, где children могут override через `:visible`). Сравнение
/// `<= 0.0` страхует от sub-normal значений, попавших в opacity
/// через клипанг — layout cascade clamp-ит в `[0.0, 1.0]`, но
/// defensive check дешёвый. opacity > 0 && < 1 Phase 0 не обрабатывается
/// (требует off-screen pass с per-pixel alpha multiply — P2 п.4+).
pub(crate) fn is_opacity_subtree_painted(b: &LayoutBox) -> bool {
    b.style.opacity > 0.0
}
