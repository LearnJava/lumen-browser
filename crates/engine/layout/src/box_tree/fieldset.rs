//! HTML Rendering §15.3.13 — rendered legend у `<fieldset>`: legend лежит не в потоке, а на
//! верхней границе fieldset, содержимое начинается ниже `max(border-top, legend)`.

use super::*;
use super::intrinsic::{max_content_outer_height, max_content_outer_width, preferred_inline_block_width};
use crate::style::{AlignValue, Direction, WritingMode};
use crate::vertical::VerticalInit;

/// Куда по inline-оси content box встаёт legend: к началу, к концу или по центру.
#[derive(Clone, Copy, PartialEq)]
enum InlineAlign {
    Start,
    End,
    Center,
}

/// Смещение border box legend от ближнего (верхнего/левого) края content box вдоль inline-оси.
/// Так считает Chromium (сверено с Edge, ряд проб с полями): центр и конец берут свободное место
/// по border box без полей (`free`), а поля учитываются только у ближнего края (`center`) или у
/// обоих (`end`: `m_near + free − m_far`); `auto`-поле прижимает legend к противоположной стороне.
fn legend_inline_offset(
    align: InlineAlign,
    free: f32,
    (m_near, near_auto): (f32, bool),
    (m_far, far_auto): (f32, bool),
) -> f32 {
    let free = free.max(0.0);
    match (near_auto, far_auto) {
        (true, true) => free / 2.0,
        (true, false) => free - m_far,
        (false, true) => m_near,
        (false, false) => match align {
            InlineAlign::Start => m_near,
            InlineAlign::Center => m_near + free / 2.0,
            InlineAlign::End => m_near + free - m_far,
        },
    }
}

/// `justify-self` legend (его задаёт и `align`-атрибут) -> сторона content box. `start_at_far_edge` —
/// inline-начало лежит на дальней физической стороне (rtl; в вертикальных режимах — снизу).
/// `text-align` legend на место не влияет (HTML Rendering §15.3.13).
fn legend_inline_align(justify_self: AlignValue, start_at_far_edge: bool) -> InlineAlign {
    let (near, far) = (InlineAlign::Start, InlineAlign::End);
    let start = if start_at_far_edge { far } else { near };
    let end = if start_at_far_edge { near } else { far };
    match justify_self {
        AlignValue::Center => InlineAlign::Center,
        AlignValue::End => end,
        _ => start,
    }
}

/// В вертикальном режиме Edge не применяет `justify-self` legend (`end`/`center`/`align`-атрибут):
/// место по inline-оси задают только `direction` и `auto`-поля (проба 2026-10-28, `vertical-rl` и
/// `vertical-lr`). Поэтому вертикальные пути выравнивают как по `start`.
const VERTICAL_LEGEND_JUSTIFY: AlignValue = AlignValue::Start;

/// Legend блокифицируется для раскладки (HTML Rendering §15.3.13), а `getComputedStyle` читает
/// стиль бокса после неё и должен видеть автор-значение: на время раскладки ставим `block`, после
/// неё [`restore_display`] возвращает прежнее. Нужно там, где внутренние алгоритмы (интринсики,
/// `dispatch_box`) смотрят на `style.display`, а не на `BoxKind`.
fn blockify_for_layout(child: &mut LayoutBox) -> Display {
    let author = child.style.display;
    if blockified_legend_display(author) == Some(Display::Block) {
        Arc::make_mut(&mut child.style).display = Display::Block;
    }
    author
}

fn restore_display(child: &mut LayoutBox, author: Display) {
    if child.style.display != author {
        Arc::make_mut(&mut child.style).display = author;
    }
}

/// Индекс поставленного на границу legend у `b` (flex/grid-fieldset): контейнер не считает его
/// элементом, а его раскладка идёт ниже (`content_offset`).
pub(super) fn placed_legend_idx(b: &LayoutBox) -> Option<usize> {
    b.fieldset_legend.filter(|l| l.placed).map(|l| l.idx)
}

/// Индекс rendered legend у flex/grid-контейнера до раскладки (интринсики): горизонтальный
/// fieldset ставит legend на границу, и контейнер его элементом не считает.
pub(super) fn rendered_legend_idx_of(b: &LayoutBox) -> Option<usize> {
    b.fieldset_legend
        .filter(|_| b.style.writing_mode == WritingMode::HorizontalTb)
        .map(|l| l.idx)
}

/// На сколько legend сдвинул содержимое fieldset вниз (0 без поставленного legend). В вертикальном
/// режиме сдвиг идёт по block-оси (ширина) и в высоту контейнера не входит.
pub(super) fn placed_content_offset(b: &LayoutBox) -> f32 {
    if b.style.writing_mode != WritingMode::HorizontalTb {
        return 0.0;
    }
    b.fieldset_legend.filter(|l| l.placed).map_or(0.0, |l| l.content_offset)
}

/// Block-начало вертикального режима лежит справа (`vertical-rl`, `sideways-rl`).
fn block_start_is_right(wm: WritingMode) -> bool {
    matches!(wm, WritingMode::VerticalRl | WritingMode::SidewaysRl)
}

/// Контейнер пересобрал `children` (multicol режет и переставляет детей): находит legend по узлу и
/// обновляет `idx`, по которому его читают baseline и рамка. Legend не режется и не клонируется.
pub(super) fn resync_legend_idx(b: &mut LayoutBox) {
    let Some(fl) = b.fieldset_legend else { return };
    if let Some(idx) = b.children.iter().position(|c| c.node == fl.node) {
        b.fieldset_legend = Some(FieldsetLegend { idx, ..fl });
    }
}

/// Раскладывает rendered legend `b` на верхней границе и возвращает, на сколько содержимое
/// fieldset опускается ниже `border-top + padding-top` (`max(legend − border-top, 0)`).
///
/// Legend сжимается по содержимому внутри content box fieldset, встаёт по своему
/// `justify-self` (его задаёт и `align`-атрибут legend) и `auto`-полям, по вертикали его margin
/// box центруется на `border-top` (выше рамки — прижимается к верху, а рамка уходит вниз на
/// половину разницы: `border_inset`). Ничего не делает, если у `b` нет rendered legend.
#[allow(clippy::too_many_arguments)]
pub(super) fn place_rendered_legend(
    b: &mut LayoutBox,
    content_x: f32,
    content_width: f32,
    children_available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    children_pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let Some(fl) = b.fieldset_legend else { return 0.0 };
    let border_top = b.style.border_top_width;
    let top_y = b.rect.y;
    let Some(child) = b.children.get_mut(fl.idx).filter(|c| c.node == fl.node) else { return 0.0 };

    let author_display = blockify_for_layout(child);
    let cem = child.style.font_size;
    let (ml_auto, mr_auto) = (child.style.margin_left.is_auto(), child.style.margin_right.is_auto());
    let ml = child.style.margin_left.resolve_or_zero(cem, content_width, viewport);
    let mr = child.style.margin_right.resolve_or_zero(cem, content_width, viewport);
    let mt = child.style.margin_top.resolve_or_zero(cem, content_width, viewport);
    let mb = child.style.margin_bottom.resolve_or_zero(cem, content_width, viewport);

    // Сжатие по содержимому (CSS 2.1 §10.3.5), как у float: явная `width` решает сама.
    let probe_w = if child.style.width.is_some() || matches!(child.kind, BoxKind::Table) {
        content_width
    } else {
        preferred_inline_block_width(child, measurer, viewport)
            .or_else(|| {
                let w = max_content_outer_width(child, measurer, viewport);
                (w > 0.0).then_some(w)
            })
            .map(|w| (w + ml + mr).min(content_width))
            .unwrap_or(content_width)
    };
    lay_out(child, content_x, top_y, probe_w, children_available_height, measurer, viewport,
            children_pcb, hp, false);

    let (lw, lh) = (child.rect.width, child.rect.height);
    let margin_box_h = mt + lh + mb;
    let align = legend_inline_align(child.style.justify_self, child.style.direction == Direction::Rtl);
    let x_off = legend_inline_offset(align, content_width - lw, (ml, ml_auto), (mr, mr_auto));
    let margin_top_edge = if margin_box_h >= border_top { 0.0 } else { (border_top - margin_box_h) / 2.0 };
    let final_x = content_x + x_off;
    let final_y = top_y + margin_top_edge + mt;
    let (dx, dy) = (final_x - child.rect.x, final_y - child.rect.y);
    shift_tree(child, dx, dy);
    restore_display(child, author_display);

    let inset = ((margin_box_h - border_top) / 2.0).max(0.0);
    let extra = (margin_box_h - border_top).max(0.0);
    b.fieldset_legend = Some(FieldsetLegend { placed: true, border_inset: inset, content_offset: extra, ..fl });
    extra
}

/// Flex/grid-fieldset в вертикальном режиме: раскладывает legend и возвращает, на сколько
/// содержимое сдвинуто по block-оси (`max(margin box legend − border блока-начала, 0)`, физическая
/// ширина). Где legend стоит, зависит от итоговой ширины и высоты fieldset, поэтому контейнер
/// зовёт [`settle_legend_vertical`], когда они известны. `room_inline` — inline-размер content box
/// (физическая высота), если он определён; `room_block` — доступная ширина.
pub(super) fn lay_out_legend_vertical(
    b: &mut LayoutBox,
    room_inline: f32,
    room_block: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let Some(fl) = b.fieldset_legend else { return 0.0 };
    let border_start = if block_start_is_right(b.style.writing_mode) {
        b.style.border_right_width
    } else {
        b.style.border_left_width
    };
    let (x, y) = (b.rect.x, b.rect.y);
    let Some(child) = b.children.get_mut(fl.idx).filter(|c| c.node == fl.node) else { return 0.0 };

    let author_display = blockify_for_layout(child);
    let cem = child.style.font_size;
    let [m_left, m_right, m_top, m_bottom] = [
        &child.style.margin_left, &child.style.margin_right, &child.style.margin_top, &child.style.margin_bottom,
    ]
    .map(|m| m.resolve_or_zero(cem, room_inline, viewport));
    let inline_room = if child.style.height.is_some() {
        room_inline
    } else {
        room_inline.min(max_content_outer_height(child, measurer, viewport) + m_top + m_bottom)
    };
    lay_out(child, x, y, room_block, Some(inline_room), measurer, viewport, pcb, hp, false);
    let margin_box_block = m_left + child.rect.width + m_right;
    restore_display(child, author_display);

    let extra = (margin_box_block - border_start).max(0.0);
    let inset = ((margin_box_block - border_start) / 2.0).max(0.0);
    b.fieldset_legend = Some(FieldsetLegend { placed: true, border_inset: inset, content_offset: extra, ..fl });
    extra
}

/// Ставит legend, разложенный [`lay_out_legend_vertical`], на границу блока-начала итогового
/// border box `b` (его `rect` уже окончательный): по block-оси margin box центруется на границе,
/// по inline-оси — в content box по `justify-self` и `auto`-полям. `frame_top`/`frame_vert` —
/// `border + padding` сверху и сверху+снизу.
pub(super) fn settle_legend_vertical(b: &mut LayoutBox, frame_top: f32, frame_vert: f32, viewport: Size) {
    let Some(fl) = b.fieldset_legend.filter(|l| l.placed) else { return };
    let st = b.style.clone();
    let rl = block_start_is_right(st.writing_mode);
    let border_start = if rl { st.border_right_width } else { st.border_left_width };
    let content_inline = (b.rect.height - frame_vert).max(0.0);
    let (box_x, box_w, content_y) = (b.rect.x, b.rect.width, b.rect.y + frame_top);
    let Some(child) = b.children.get_mut(fl.idx).filter(|c| c.node == fl.node) else { return };

    let cem = child.style.font_size;
    let (mt_auto, mb_auto) = (child.style.margin_top.is_auto(), child.style.margin_bottom.is_auto());
    let [m_left, m_right, m_top, m_bottom] = [
        &child.style.margin_left, &child.style.margin_right, &child.style.margin_top, &child.style.margin_bottom,
    ]
    .map(|m| m.resolve_or_zero(cem, content_inline, viewport));
    let (m_start, m_end) = if rl { (m_right, m_left) } else { (m_left, m_right) };
    let (lw, lh) = (child.rect.width, child.rect.height);
    let margin_box_block = m_start + lw + m_end;
    let off = if margin_box_block >= border_start { 0.0 } else { (border_start - margin_box_block) / 2.0 };
    let final_x = if rl { box_x + box_w - off - m_start - lw } else { box_x + off + m_start };
    let start_at_far = (child.style.direction == Direction::Rtl) != (st.writing_mode == WritingMode::SidewaysLr);
    let align = legend_inline_align(VERTICAL_LEGEND_JUSTIFY, start_at_far);
    let y_off = legend_inline_offset(align, content_inline - lh, (m_top, mt_auto), (m_bottom, mb_auto));
    let (dx, dy) = (final_x - child.rect.x, content_y + y_off - child.rect.y);
    shift_tree(child, dx, dy);
}

/// То же для вертикального режима записи: legend встаёт на border блока-начала (`border-right`
/// у `vertical-rl`/`sideways-rl`, `border-left` у `vertical-lr`/`sideways-lr`), по inline-оси
/// (физический y) выравнивается в content box, а содержимое сдвигается на `cursor_block_consumed`.
///
/// У `rl` правая граница ещё не финальна: ширина fieldset известна только после раскладки
/// детей, поэтому legend ставится от провизорной правой кромки, и `vertical_trampoline::
/// finish_frame` сдвигает его вместе с остальными детьми.
pub(super) fn place_rendered_legend_vertical(
    b: &mut LayoutBox,
    init: &mut VerticalInit,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let Some(fl) = b.fieldset_legend else { return };
    let rl = init.is_rtl;
    let (content_x_left, content_y, content_inline) = (init.content_x_left, init.content_y, init.content_inline);
    let st = b.style.clone();
    let em = st.font_size;
    let (border_start, padding_start) = if rl {
        (st.border_right_width, st.padding_right.resolve_or_zero(em, content_inline, viewport))
    } else {
        (st.border_left_width, st.padding_left.resolve_or_zero(em, content_inline, viewport))
    };
    let box_x = b.rect.x;
    let sideways_lr = st.writing_mode == WritingMode::SidewaysLr;
    let Some(child) = b.children.get_mut(fl.idx).filter(|c| c.node == fl.node) else { return };

    let author_display = blockify_for_layout(child);
    let cem = child.style.font_size;
    let (mt_auto, mb_auto) = (child.style.margin_top.is_auto(), child.style.margin_bottom.is_auto());
    let m_left = child.style.margin_left.resolve_or_zero(cem, content_inline, viewport);
    let m_right = child.style.margin_right.resolve_or_zero(cem, content_inline, viewport);
    let m_top = child.style.margin_top.resolve_or_zero(cem, content_inline, viewport);
    let m_bottom = child.style.margin_bottom.resolve_or_zero(cem, content_inline, viewport);
    let (m_start, m_end) = if rl { (m_right, m_left) } else { (m_left, m_right) };

    // Сжатие по содержимому вдоль inline-оси (физическая высота): не больше, чем нужно тексту.
    let inline_room = if child.style.height.is_some() {
        content_inline
    } else {
        content_inline.min(max_content_outer_height(child, measurer, viewport) + m_top + m_bottom)
    };
    let room_block = (init.content_block_avail - init.cursor_block_consumed).max(0.0);
    lay_out(child, content_x_left, content_y, room_block, Some(inline_room), measurer, viewport,
            init.pcb, hp, false);

    let (lw, lh) = (child.rect.width, child.rect.height);
    let margin_box_block = m_start + lw + m_end;
    let off = if margin_box_block >= border_start { 0.0 } else { (border_start - margin_box_block) / 2.0 };
    let final_x = if rl {
        let right = content_x_left + init.content_block_avail + padding_start + border_start;
        right - off - m_start - lw
    } else {
        box_x + off + m_start
    };
    // Inline-начало — сверху, кроме rtl (снизу) и `sideways-lr` (там строка идёт снизу вверх).
    let start_at_far = (child.style.direction == Direction::Rtl) != sideways_lr;
    let align = legend_inline_align(VERTICAL_LEGEND_JUSTIFY, start_at_far);
    let y_off = legend_inline_offset(align, content_inline - lh, (m_top, mt_auto), (m_bottom, mb_auto));
    let final_y = content_y + y_off;
    let (dx, dy) = (final_x - child.rect.x, final_y - child.rect.y);
    shift_tree(child, dx, dy);
    restore_display(child, author_display);

    let extra = (margin_box_block - border_start).max(0.0);
    init.cursor_block_consumed += extra;
    let inset = ((margin_box_block - border_start) / 2.0).max(0.0);
    b.fieldset_legend = Some(FieldsetLegend { placed: true, border_inset: inset, content_offset: extra, ..fl });
}
