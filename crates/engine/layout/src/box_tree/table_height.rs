//! Высота таблицы по строкам (CSS 2.1 §17.5.3, CSS Tables L3 §3.4): `height`
//! (и `min-height`) таблицы — минимум, а излишек над высотой строк делится между
//! строками, а не остаётся пустым хвостом под последней из них.
//!
//! Строки сначала раскладываются в естественную высоту ([`super::table_trampoline`]),
//! поэтому делёж идёт постфактум: каждая строка (и всё, что под ней) сдвигается на
//! накопленную долю излишка, ячейки обычной строки вырастают на её долю, а их
//! содержимое выравнивается заново по `vertical-align`.

use super::table_trampoline::{TableInit, TopEntry};
use super::*;
use std::collections::HashMap;

/// Ключ ячейки в [`TableInit::cell_dy`]: `(группа, строка, ячейка)` — те же
/// индексы, что у `TableInit::span_fixes` (строка — индекс верхнего уровня у
/// прямой строки и индекс внутри группы у сгруппированной).
pub(super) type CellKey = (Option<usize>, usize, usize);

/// Высота таблицы, к которой тянутся строки: заданные `height`/`min-height`, но не
/// меньше естественной `natural` (border box). `None` — делить нечего.
pub(super) fn target_height(init: &TableInit, natural: f32, viewport: Size) -> f32 {
    let s = &init.s;
    let chrome = init.padding_top + init.padding_bottom + s.border_top_width + s.border_bottom_width;
    let outer = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => (v + chrome).max(0.0),
        BoxSizing::BorderBox => v.max(chrome),
    };
    let resolve = |len: &Option<Length>| len.as_ref().and_then(|l| resolve_block_size(l, init.em, init.available_height, viewport));
    // Таблица без столбцов «пуста» (CSS Tables L3 §3.1 шаг 3B): её строки в высоту не входят,
    // заданная `height` берётся как есть, а не как минимум над строками.
    let mut target = match resolve(&s.height) {
        Some(h) if init.n_cols == 0 => outer(h),
        _ => natural.max(resolve(&s.height).map_or(0.0, outer)),
    };
    if let Some(h) = resolve(&s.min_height) {
        target = target.max(outer(h));
    }
    target
}

/// Делит `extra` между строками таблицы `b` и сдвигает всё, что лежит ниже них.
///
/// Доля строки пропорциональна её высоте; делят только строки с `height: auto`,
/// а если таких нет — все (как в Chrome). Если все веса нулевые, излишек делится
/// поровну.
pub(super) fn distribute(b: &mut LayoutBox, init: &TableInit, extra: f32) {
    // Строки в порядке документа: (группа, индекс строки в `children` группы/таблицы).
    let mut rows: Vec<(Option<usize>, usize)> = Vec::with_capacity(init.flat_row_rects.len());
    for entry in &init.top_level {
        match entry {
            TopEntry::Row { child_idx, .. } => rows.push((None, *child_idx)),
            TopEntry::Group { child_idx, rows: rs } => rows.extend(rs.iter().map(|(r, _)| (Some(*child_idx), *r))),
        }
    }
    let n = rows.len().min(init.flat_row_rects.len());
    if n == 0 {
        return;
    }
    let is_auto = |i: usize| {
        let (g, r) = rows[i];
        let row = match g {
            None => &b.children[r],
            Some(g) => &b.children[g].children[r],
        };
        row.style.height.is_none()
    };
    let auto: Vec<bool> = (0..n).map(is_auto).collect();
    let eligible: Vec<bool> = if auto.iter().any(|&a| a) { auto } else { vec![true; n] };
    let weight_sum: f32 = (0..n).filter(|&i| eligible[i]).map(|i| init.flat_row_rects[i].1).sum();
    let n_eligible = eligible.iter().filter(|&&e| e).count() as f32;
    let share = |i: usize| -> f32 {
        if !eligible[i] {
            0.0
        } else if weight_sum > 1e-3 {
            extra * init.flat_row_rects[i].1 / weight_sum
        } else {
            extra / n_eligible
        }
    };

    let old_dy: HashMap<CellKey, f32> = init.cell_dy.iter().copied().map(|(g, r, c, dy)| ((g, r, c), dy)).collect();
    let mut new_rects: Vec<(f32, f32)> = Vec::with_capacity(n);
    let mut cum = 0.0_f32;
    let mut flat = 0usize;
    for entry in &init.top_level {
        match entry {
            TopEntry::Row { child_idx, .. } => {
                if flat >= n {
                    break;
                }
                let sh = share(flat);
                let row = &mut b.children[*child_idx];
                grow_row(row, None, *child_idx, cum, sh, &old_dy);
                new_rects.push((row.rect.y, row.rect.height));
                cum += sh;
                flat += 1;
            }
            TopEntry::Group { child_idx, rows: rs } => {
                let group = &mut b.children[*child_idx];
                group.rect.y += cum;
                let cum_before = cum;
                for (r, _) in rs {
                    if flat >= n {
                        break;
                    }
                    let sh = share(flat);
                    let row = &mut group.children[*r];
                    grow_row(row, Some(*child_idx), *r, cum, sh, &old_dy);
                    new_rects.push((row.rect.y, row.rect.height));
                    cum += sh;
                    flat += 1;
                }
                group.rect.height += cum - cum_before;
            }
        }
    }

    // Ячейки с `rowspan` тянутся до нижней кромки последней строки диапазона.
    for &(group, row, cell_idx, start_flat, span) in &init.span_fixes {
        let end_flat = (start_flat + span as usize).min(new_rects.len());
        if end_flat == 0 {
            continue;
        }
        let (last_y, last_h) = new_rects[end_flat - 1];
        let row_box = match group {
            None => &mut b.children[row],
            Some(g) => &mut b.children[g].children[row],
        };
        let cell = &mut row_box.children[cell_idx];
        cell.rect.height = (last_y + last_h - cell.rect.y).max(cell.rect.height);
        let key = (group, row, cell_idx);
        realign(cell, old_dy.get(&key).copied().unwrap_or(0.0));
    }

    for &idx in &init.bottom_captions {
        crate::incremental::translate_subtree(&mut b.children[idx], 0.0, extra);
    }
}

/// Сдвигает строку вместе с содержимым на `cum` вниз и вытягивает её на `share`:
/// ячейки без `rowspan` занимают всю новую высоту.
fn grow_row(row: &mut LayoutBox, group: Option<usize>, row_field: usize, cum: f32, share: f32, old_dy: &HashMap<CellKey, f32>) {
    crate::incremental::translate_subtree(row, 0.0, cum);
    row.rect.height += share;
    if share <= 0.0 {
        return;
    }
    for (i, cell) in row.children.iter_mut().enumerate() {
        if cell.row_span != 1 || matches!(cell.kind, BoxKind::Skip) {
            continue;
        }
        cell.rect.height += share;
        realign(cell, old_dy.get(&(group, row_field, i)).copied().unwrap_or(0.0));
    }
}

/// Выравнивает содержимое ячейки заново в её новой высоте: снимает прежний сдвиг
/// `old_dy` и применяет сдвиг для `middle`/`bottom`; `top` и базовая линия остаются
/// у верха (их сдвиг от роста строки не зависит).
fn realign(cell: &mut LayoutBox, old_dy: f32) {
    let va = cell.style.vertical_align;
    if !matches!(va, VerticalAlign::Middle | VerticalAlign::Bottom) {
        return;
    }
    super::table_valign::shift_cell_content(cell, -old_dy);
    let dy = super::table_valign::free_space_shift(cell, cell.rect.height, va);
    super::table_valign::shift_cell_content(cell, dy);
}
