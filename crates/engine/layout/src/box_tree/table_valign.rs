//! Вертикальное выравнивание содержимого ячеек таблицы (CSS 2.1 §17.5.3) и
//! базовая линия таблицы (CSS Align L3 §9.1, CSS Tables L3 §3.7) — всё, что
//! зависит от `vertical-align` ячейки.
//!
//! Ячейка раскладывается с содержимым у верхней кромки; когда высота строки
//! известна, [`shift_cell_content`] сдвигает детей ячейки на добавленный зазор.
//! Базовая линия считается по уже разложенному дереву (как и вся модель в
//! [`super::baseline`]), поэтому сдвиг содержимого учитывается сам собой.

use super::baseline::{box_baseline, BaselineSide};
use super::*;

/// Участвует ли ячейка в выравнивании по базовой линии строки: CSS 2.1 §17.5.3
/// — любое значение, кроме `top`/`middle`/`bottom`, ведёт себя как `baseline`;
/// ячейка с `rowspan > 1` в общую базовую линию не входит.
pub(super) fn is_baseline_aligned(cell: &LayoutBox) -> bool {
    cell.row_span == 1
        && !matches!(cell.style.vertical_align, VerticalAlign::Top | VerticalAlign::Middle | VerticalAlign::Bottom)
}

/// Сдвиг содержимого ячейки по вертикали при выравнивании `middle`/`bottom` в
/// ячейке высотой `target_h` (border box): свободное место — высота за вычетом
/// border/padding и фактической высоты содержимого (последний in-flow бокс
/// вместе с нижним margin), а не `rect.height`, в которую уже входит заданная
/// `height` ячейки.
pub(super) fn free_space_shift(cell: &LayoutBox, target_h: f32, align: VerticalAlign) -> f32 {
    let st = &cell.style;
    let em = st.font_size;
    let top = st.border_top_width + st.padding_top.resolve_or_zero(em, 0.0, Size::ZERO);
    let bottom = st.border_bottom_width + st.padding_bottom.resolve_or_zero(em, 0.0, Size::ZERO);
    let content_top = cell.rect.y + top;
    let content_bottom = cell
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|c| {
            let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, 0.0, Size::ZERO);
            c.rect.y + c.rect.height + mb
        })
        .fold(content_top, f32::max);
    let free = (target_h - top - bottom - (content_bottom - content_top)).max(0.0);
    match align {
        VerticalAlign::Middle => free / 2.0,
        VerticalAlign::Bottom => free,
        _ => 0.0,
    }
}

/// Расстояние от верхней кромки border box ячейки до нижней кромки её content
/// box — базовая линия ячейки без строк (CSS 2.1 §17.5.3).
fn content_bottom(cell: &LayoutBox) -> f32 {
    let st = &cell.style;
    let pb = st.padding_bottom.resolve_or_zero(st.font_size, 0.0, Size::ZERO);
    (cell.rect.height - pb - st.border_bottom_width).max(0.0)
}

/// Базовая линия ячейки стороны `side` от верхней кромки её border box.
pub(super) fn cell_baseline(cell: &LayoutBox, side: BaselineSide, measurer: Option<&dyn TextMeasurer>) -> f32 {
    box_baseline(cell, side, measurer).unwrap_or_else(|| content_bottom(cell))
}

/// Сдвигает содержимое ячейки по вертикали. Абсолютно позиционированные дети
/// привязаны к containing block и от выравнивания не зависят.
pub(super) fn shift_cell_content(cell: &mut LayoutBox, dy: f32) {
    if dy.abs() < 1e-4 {
        return;
    }
    for c in &mut cell.children {
        if matches!(c.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        crate::incremental::translate_subtree(c, 0.0, dy);
    }
}

/// Базовая линия строки таблицы (от верхней кромки строки): общая (первая)
/// линия ячеек, выровненных по базовой линии — и для первой, и для последней
/// строки таблицы; без таких ячеек — линия крайней ячейки (первой для `First`,
/// последней для `Last`). `None` у строки без ячеек.
fn row_baseline(row: &LayoutBox, side: BaselineSide, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    let cells = || row.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip));
    if let Some(c) = cells().find(|c| is_baseline_aligned(c)) {
        return Some(c.rect.y - row.rect.y + cell_baseline(c, BaselineSide::First, measurer));
    }
    let c = match side {
        BaselineSide::First => cells().next()?,
        BaselineSide::Last => cells().next_back()?,
    };
    Some(c.rect.y - row.rect.y + cell_baseline(c, side, measurer))
}

/// Первая/последняя базовая линия таблицы — линия первой/последней строки;
/// `<caption>` в расчёте не участвует (CSS Tables L3 §3.7). Отсчёт — от верхней
/// кромки border box таблицы.
pub(super) fn table_baseline(
    table: &LayoutBox,
    side: BaselineSide,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<f32> {
    let mut rows = table.children.iter().flat_map(|c| -> Vec<&LayoutBox> {
        match c.kind {
            BoxKind::TableRow => vec![c],
            BoxKind::TableRowGroup => c.children.iter().filter(|r| matches!(r.kind, BoxKind::TableRow)).collect(),
            _ => Vec::new(),
        }
    });
    let row = match side {
        BaselineSide::First => rows.next()?,
        BaselineSide::Last => rows.last()?,
    };
    Some(row.rect.y - table.rect.y + row_baseline(row, side, measurer)?)
}
