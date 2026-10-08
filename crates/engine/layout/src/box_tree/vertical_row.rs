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
/// `available_height` — inline-размер строки (физическая высота).
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
        extent: f32,
    }

    let orig_children = std::mem::take(&mut b.children);
    let mut children: Vec<LayoutBox> = Vec::with_capacity(orig_children.len());
    let mut columns: Vec<Column> = Vec::new();
    let mut cur = Column { idxs: Vec::new(), has_baseline: false, extent: 0.0 };

    for mut child in orig_children {
        if matches!(child.kind, BoxKind::InlineSpace) {
            cur.extent += space_h;
            children.push(child);
            continue;
        }
        let is_run = matches!(child.kind, BoxKind::InlineRun { .. });
        let c_em = child.style.font_size;
        let mt = child.style.margin_top.resolve_or_zero(c_em, available_width, viewport);
        let mb = child.style.margin_bottom.resolve_or_zero(c_em, available_width, viewport);
        let place_y = start_y + cur.extent;
        // Прогон переносится по оставшейся части колонки, атомарный бокс — по всей.
        let child_inline = if is_run {
            (content_inline - cur.extent).max(0.0)
        } else {
            content_inline
        };
        lay_out(
            &mut child, start_x, place_y, available_width, Some(child_inline),
            measurer, viewport, pcb, hp, false,
        );
        if matches!(child.kind, BoxKind::Skip) {
            children.push(child);
            continue;
        }
        let mut size = mt + child.rect.height + mb;
        if !is_run && cur.extent > 0.0 && cur.extent + size > content_inline {
            columns.push(std::mem::replace(
                &mut cur,
                Column { idxs: Vec::new(), has_baseline: false, extent: 0.0 },
            ));
            lay_out(
                &mut child, start_x, start_y, available_width, Some(content_inline),
                measurer, viewport, pcb, hp, false,
            );
            size = mt + child.rect.height + mb;
        }
        if is_run || matches!(inline_v_align(&child), VerticalAlign::Baseline) {
            cur.has_baseline = true;
        }
        cur.idxs.push(children.len());
        cur.extent += size;
        children.push(child);
    }
    if !cur.idxs.is_empty() {
        columns.push(cur);
    }

    // Block-размер колонки — самый широкий margin box участника; strut
    // (высота строки ряда) входит, когда в колонке есть участник по baseline.
    let margin_box_w = |c: &LayoutBox| -> (f32, f32, f32) {
        let em = c.style.font_size;
        let ml = c.style.margin_left.resolve_or_zero(em, available_width, viewport);
        let mr = c.style.margin_right.resolve_or_zero(em, available_width, viewport);
        (ml, mr, ml + c.rect.width + mr)
    };
    let col_sizes: Vec<f32> = columns
        .iter()
        .map(|col| {
            let widest = col.idxs.iter().map(|&i| margin_box_w(&children[i]).2).fold(0.0_f32, f32::max);
            if col.has_baseline { widest.max(b.used_line_height) } else { widest }
        })
        .collect();
    let total_block: f32 = col_sizes.iter().sum();

    let mut taken = 0.0_f32;
    for (col, &col_w) in columns.iter().zip(&col_sizes) {
        let col_left = if rl { start_x + total_block - taken - col_w } else { start_x + taken };
        taken += col_w;
        for &i in &col.idxs {
            let (ml, _mr, mbox_w) = margin_box_w(&children[i]);
            // Начало block-оси: правая кромка для `rl`, левая для `lr`.
            let slack = col_w - mbox_w;
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
    // Колонки идут одна под другой начиная с `start_y`: inline-координаты
    // детей уже абсолютные, сдвигать по `y` нечего.

    let longest = columns.iter().map(|c| c.extent).fold(0.0_f32, f32::max);
    b.children = children;
    b.rect.x = start_x;
    b.rect.y = start_y;
    b.rect.width = total_block;
    b.rect.height = if available_height.is_some() { content_inline } else { longest };
}
