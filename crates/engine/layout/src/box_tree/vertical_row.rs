//! `InlineBlockRow` в блоке с вертикальным `writing-mode` (BUG-1263).
//!
//! Горизонтальный ряд (`layout_dispatch`, ветка `BoxKind::InlineBlockRow`)
//! течёт слева направо и переносится по ширине. В вертикальном режиме оси
//! меняются местами (CSS Writing Modes L3 §3): inline-ось — физический `y`,
//! строки (колонки) укладываются вдоль block-оси — физического `x`
//! (справа налево для `vertical-rl`, слева направо для `vertical-lr`).
//!
//! Упрощения относительно горизонтального ряда: нет `::first-line`-разбиения
//! прогона, общего с атомарным соседом (IFC-4), и центральная базовая линия
//! заменена выравниванием по центру колонки — для атомарных боксов без текста
//! она совпадает с серединой margin box.

use super::*;

/// Раскладывает `InlineBlockRow` `b`, у которого вертикальный `writing-mode`.
///
/// `available_width` — остаток по block-оси (физическая ширина),
/// `available_height` — inline-размер строки (физическая высота), `outer_floats` —
/// float-ы контейнера в системе координат ряда (его первая колонка — block `0`).
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_out_vertical_inline_block_row(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    outer_floats: Option<&FloatContext>,
) {
    let s = b.style.clone();
    let rl = matches!(
        s.writing_mode,
        crate::style::WritingMode::VerticalRl | crate::style::WritingMode::SidewaysRl
    );
    let content_inline = available_height.unwrap_or(viewport.height).max(0.0);
    let space_h = measurer.map_or(0.0, |m| m.char_width(' ', s.font_size));

    /// Колонка: индексы детей, inline-протяжённость и block-размер.
    struct Column {
        idxs: Vec<usize>,
        has_baseline: bool,
        /// В колонке есть прогон текста (его не зеркалим при `direction: rtl`).
        has_run: bool,
        /// Занятая inline-длина, считая от `inline_start`.
        extent: f32,
        /// Block-позиция начала колонки (от начала block-оси ряда) и её block-размер.
        start: f32,
        width: f32,
        /// Левый край полосы между float-ами, с которого колонка начинается по inline-оси.
        inline_start: f32,
    }
    let new_column = |start: f32| Column {
        idxs: Vec::new(),
        has_baseline: false,
        has_run: false,
        extent: 0.0,
        start,
        width: 0.0,
        inline_start: 0.0,
    };

    // CSS 2.1 §9.5: float-ы контейнера (в системе координат ряда) сужают колонку —
    // её inline-полоса берётся по `line_band` на всю block-ширину колонки.
    let floats = outer_floats.filter(|fc| !fc.is_empty());
    let band = |start: f32, width: f32| match floats {
        Some(fc) => fc.line_band(start, start + width.max(0.01), 0.0, content_inline),
        None => (0.0, content_inline),
    };
    let strut = b.used_line_height;
    let margin_box_w = |c: &LayoutBox| -> (f32, f32, f32) {
        let em = c.style.font_size;
        let ml = c.style.margin_left.resolve_or_zero(em, available_width, viewport);
        let mr = c.style.margin_right.resolve_or_zero(em, available_width, viewport);
        (ml, mr, ml + c.rect.width + mr)
    };

    let orig_children = std::mem::take(&mut b.children);
    let mut children: Vec<LayoutBox> = Vec::with_capacity(orig_children.len());
    let mut columns: Vec<Column> = Vec::new();
    let mut cur = new_column(0.0);

    for mut child in orig_children {
        if matches!(child.kind, BoxKind::InlineSpace) {
            // Пробел в начале колонки рядом с float-ом схлопывается, как в начале строки.
            if !(floats.is_some() && cur.idxs.is_empty()) {
                cur.extent += space_h;
            }
            children.push(child);
            continue;
        }
        let is_run = matches!(child.kind, BoxKind::InlineRun { .. });
        let c_em = child.style.font_size;
        let mt = child.style.margin_top.resolve_or_zero(c_em, available_width, viewport);
        let mb = child.style.margin_bottom.resolve_or_zero(c_em, available_width, viewport);
        let baseline = is_run || matches!(inline_v_align(&child), VerticalAlign::Baseline);
        let laid_extent = cur.extent;
        if is_run {
            // Прогон переносится по оставшейся части колонки (между float-ами).
            let (l, r) = band(cur.start, cur.width.max(strut));
            let l = if cur.idxs.is_empty() { l } else { cur.inline_start };
            lay_out(
                &mut child, start_x, start_y + l + cur.extent, available_width,
                Some((r - l - cur.extent).max(0.0)), measurer, viewport, pcb, hp, false,
            );
            if matches!(child.kind, BoxKind::Skip) {
                children.push(child);
                continue;
            }
            if cur.idxs.is_empty() {
                cur.inline_start = l;
            }
            cur.has_baseline = true;
            cur.has_run = true;
            cur.width = cur.width.max(margin_box_w(&child).2).max(strut);
            cur.idxs.push(children.len());
            cur.extent += mt + child.rect.height + mb;
            children.push(child);
            continue;
        }
        // Атомарный бокс — по всей длине колонки.
        lay_out(
            &mut child, start_x, start_y + laid_extent, available_width, Some(content_inline),
            measurer, viewport, pcb, hp, false,
        );
        if matches!(child.kind, BoxKind::Skip) {
            children.push(child);
            continue;
        }
        let size = mt + child.rect.height + mb;
        let bs = margin_box_w(&child).2;
        let l = loop {
            let width = cur.width.max(bs).max(if cur.has_baseline || baseline { strut } else { 0.0 });
            let (l, r) = band(cur.start, width);
            if cur.extent + size <= r - l {
                break l;
            }
            if cur.extent > 0.0 {
                // Не влез в колонку — новая колонка сразу за этой.
                let next = cur.start + cur.width;
                columns.push(std::mem::replace(&mut cur, new_column(next)));
                continue;
            }
            // Пустая колонка, а бокс не влез рядом с float-ами: ниже по block-оси, где
            // полоса шире (CSS 2.1 §9.5.1 п.8); если полоса уже полная — берём как есть.
            let narrowed = l > 0.0 || r < content_inline;
            match floats.filter(|_| narrowed && cur.idxs.is_empty()).and_then(|fc| fc.next_float_bottom(cur.start)) {
                Some(nb) => {
                    // The first block position (1px steps, as `line_band` samples) where the
                    // band is wide enough: a contour releases the band before its margin box ends.
                    let mut y = cur.start + 1.0;
                    while y < nb {
                        let (l2, r2) = band(y, width);
                        if size <= r2 - l2 {
                            break;
                        }
                        y += 1.0;
                    }
                    cur.start = y.min(nb);
                }
                None => break l,
            }
        };
        if baseline {
            cur.has_baseline = true;
        }
        cur.width = cur.width.max(bs).max(if cur.has_baseline { strut } else { 0.0 });
        if cur.idxs.is_empty() {
            cur.inline_start = l;
        }
        let dy = cur.inline_start + cur.extent - laid_extent;
        crate::box_tree::shift_tree(&mut child, 0.0, dy);
        cur.idxs.push(children.len());
        cur.extent += size;
        children.push(child);
    }
    if !cur.idxs.is_empty() {
        columns.push(cur);
    }

    // Block-размер колонки — самый широкий margin box участника; strut
    // (высота строки ряда) входит, когда в колонке есть участник по baseline.
    // Колонки идут вдоль block-оси; после float-а колонка может начаться дальше
    // конца предыдущей, поэтому ряд кончается там, где кончается последняя.
    let total_block = columns.last().map_or(0.0, |c| c.start + c.width);
    for col in &columns {
        let col_left = if rl { start_x + total_block - col.start - col.width } else { start_x + col.start };
        for &i in &col.idxs {
            let (ml, _mr, mbox_w) = margin_box_w(&children[i]);
            // Начало block-оси: правая кромка для `rl`, левая для `lr`.
            let slack = col.width - mbox_w;
            let offset = match inline_v_align(&children[i]) {
                VerticalAlign::Top | VerticalAlign::TextTop => if rl { slack } else { 0.0 },
                VerticalAlign::Bottom | VerticalAlign::TextBottom => if rl { 0.0 } else { slack },
                _ => (slack / 2.0).floor(),
            };
            let dx = col_left + offset + ml - children[i].rect.x;
            if dx != 0.0 {
                crate::vertical::shift_subtree_x(&mut children[i], dx);
            }
        }
    }
    // Inline-координаты детей уже абсолютные (колонка начинается на `start_y` плюс
    // левый край её полосы между float-ами). Под `direction: rtl` (и в `sideways-lr`) inline-старт —
    // нижний край полосы: колонка из одних атомарных боксов зеркалится к нему
    // (UAX #9, как горизонтальный ряд в `layout_dispatch`).
    let inline_reversed = (b.style.direction == Direction::Rtl)
        != (s.writing_mode == crate::style::WritingMode::SidewaysLr);
    if inline_reversed {
        for col in columns.iter().filter(|c| !c.has_run) {
            let (_, r) = band(col.start, col.width);
            for &i in &col.idxs {
                let c = &mut children[i];
                let c_em = c.style.font_size;
                let mt = c.style.margin_top.resolve_or_zero(c_em, available_width, viewport);
                let mb = c.style.margin_bottom.resolve_or_zero(c_em, available_width, viewport);
                let top = c.rect.y - mt;
                let offset = top - start_y - col.inline_start;
                let dy = (start_y + r - offset - (mt + c.rect.height + mb)) - top;
                shift_tree(c, 0.0, dy);
            }
        }
    }

    let longest = columns.iter().map(|c| c.inline_start + c.extent).fold(0.0_f32, f32::max);
    b.children = children;
    b.rect.x = start_x;
    b.rect.y = start_y;
    b.rect.width = total_block;
    b.rect.height = if available_height.is_some() { content_inline } else { longest };
}
