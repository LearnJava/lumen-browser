//! Таблица в вертикальном `writing-mode` (TABLE-VWM).
//!
//! CSS Writing Modes L3 §7 / CSS Tables L3: строки таблицы идут по block-оси режима, столбцы —
//! по inline-оси. В `vertical-*` inline-ось — физическая `y` (сверху вниз; снизу вверх при
//! `direction: rtl` либо `sideways-lr`), block-ось — `x` (слева направо у `vertical-lr`,
//! справа налево у `vertical-rl`/`sideways-rl`). Толщина строки — физическая ширина ячеек,
//! ширина столбца — физическая высота.
//!
//! Горизонтальный путь (`table.rs` + `table_trampoline.rs`) считает в физических осях и здесь не
//! годится, поэтому вертикальная таблица раскладывается отдельно и целиком в одном вызове
//! (вложенные вертикальные таблицы идут обычной рекурсией `lay_out`): сетка ячеек →
//! ширины столбцов → ячейки на своих столбцах → толщины строк → итоговые прямоугольники.
//!
//! Правила: `thead` идёт первой, `tfoot` последней (CSS 2.1 §17.2); `border-spacing` физический
//! (первая длина — по `x`, вторая — по `y`, как в Chrome); `vertical-align` ячейки действует
//! по block-оси (`top` = начало, `baseline` считается как `top`).
//!
//! Не реализовано: `<caption>` (не раскладывается), ортогональные ячейки
//! (`writing-mode: horizontal-tb` внутри вертикальной таблицы), разрешение конфликтов рамок
//! `border-collapse: collapse` по элементам (рамки строк/групп) — рамки ячеек перекрываются на
//! большую из двух.

use super::*;
use super::block_flow_trampoline::DispatchOutcome;
use crate::style::{Direction, TableLayout, WritingMode};

/// Строка таблицы: прямой `<tr>` (`group == None`) или строка внутри группы.
#[derive(Clone, Copy)]
struct RowRef {
    group: Option<usize>,
    /// Индекс строки в `children` таблицы либо группы.
    row: usize,
}

/// Ячейка на сетке: `cell` — индекс в `children` строки.
struct Slot {
    row: usize,
    col: usize,
    rs: usize,
    cs: usize,
    cell: usize,
}

fn row_ref(b: &LayoutBox, r: RowRef) -> &LayoutBox {
    match r.group {
        Some(g) => &b.children[g].children[r.row],
        None => &b.children[r.row],
    }
}

fn row_mut(b: &mut LayoutBox, r: RowRef) -> &mut LayoutBox {
    match r.group {
        Some(g) => &mut b.children[g].children[r.row],
        None => &mut b.children[r.row],
    }
}

/// Строки в порядке раскладки: первая `thead` — сверху, первая `tfoot` — снизу (CSS 2.1 §17.2),
/// остальное — в порядке документа. Вторым значением — группы `(индекс в children, первая
/// строка, число строк)`.
fn collect_rows(b: &LayoutBox) -> (Vec<RowRef>, Vec<(usize, usize, usize)>) {
    let (mut header, mut footer) = (None, None);
    for (i, c) in b.children.iter().enumerate() {
        if !matches!(c.kind, BoxKind::TableRowGroup) {
            continue;
        }
        match c.style.display {
            Display::TableHeaderGroup if header.is_none() => header = Some(i),
            Display::TableFooterGroup if footer.is_none() => footer = Some(i),
            _ => {}
        }
    }
    let mut order: Vec<usize> = Vec::with_capacity(b.children.len());
    order.extend(header);
    for (i, c) in b.children.iter().enumerate() {
        if Some(i) != header && Some(i) != footer && matches!(c.kind, BoxKind::TableRow | BoxKind::TableRowGroup) {
            order.push(i);
        }
    }
    order.extend(footer);
    let mut rows = Vec::new();
    let mut groups = Vec::new();
    for i in order {
        if matches!(b.children[i].kind, BoxKind::TableRow) {
            rows.push(RowRef { group: None, row: i });
            continue;
        }
        let first = rows.len();
        for (r, rc) in b.children[i].children.iter().enumerate() {
            if matches!(rc.kind, BoxKind::TableRow) {
                rows.push(RowRef { group: Some(i), row: r });
            }
        }
        groups.push((i, first, rows.len() - first));
    }
    (rows, groups)
}

/// Размещение ячеек по сетке с учётом `rowspan` (CSS 2.1 §17.5): ячейка занимает первый
/// столбец, не занятый ячейками верхних строк. Возвращает ячейки и число столбцов.
fn place_slots(b: &LayoutBox, rows: &[RowRef]) -> (Vec<Slot>, usize) {
    let mut slots = Vec::new();
    let mut rem: Vec<usize> = Vec::new();
    let mut n_cols = 0;
    for (ri, &rr) in rows.iter().enumerate() {
        let row = row_ref(b, rr);
        let first_slot = slots.len();
        let mut col = 0;
        for (ci, cell) in row.children.iter().enumerate() {
            if matches!(cell.kind, BoxKind::Skip) {
                continue;
            }
            while col < rem.len() && rem[col] > 0 {
                col += 1;
            }
            let cs = cell.col_span.max(1) as usize;
            let rs = (cell.row_span.max(1) as usize).min(rows.len() - ri);
            slots.push(Slot { row: ri, col, rs, cs, cell: ci });
            n_cols = n_cols.max(col + cs);
            col += cs;
        }
        for sl in &slots[first_slot..] {
            if sl.rs > 1 {
                if rem.len() < sl.col + sl.cs {
                    rem.resize(sl.col + sl.cs, 0);
                }
                for v in &mut rem[sl.col..sl.col + sl.cs] {
                    *v = (*v).max(sl.rs);
                }
            }
        }
        for v in &mut rem {
            *v = v.saturating_sub(1);
        }
    }
    (slots, n_cols)
}

/// Рамка и padding ячейки по inline-оси (физические top + bottom).
fn frame_vert_of(st: &ComputedStyle, viewport: Size) -> f32 {
    let em = st.font_size;
    st.padding_top.resolve_or_zero(em, 0.0, viewport)
        + st.padding_bottom.resolve_or_zero(em, 0.0, viewport)
        + st.border_top_width
        + st.border_bottom_width
}

/// min-content border-box **высота** бокса в вертикальном режиме — его inline-размер: самое
/// длинное неразрывное слово текста; блок — самый «длинный» из детей. Зеркало
/// [`max_content_outer_height`]: явная `height` побеждает.
fn min_content_outer_height(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>, viewport: Size) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let frame = frame_vert_of(s, viewport);
    if let Some(h_len) = &s.height
        && !h_len.is_intrinsic()
        && let Some(h) = h_len.resolve(em, None, viewport)
    {
        return match s.box_sizing {
            BoxSizing::ContentBox => h + frame,
            BoxSizing::BorderBox => h.max(frame),
        }
        .max(0.0);
    }
    let content = match &b.kind {
        BoxKind::InlineRun { segments, .. } => measurer.map_or(0.0, |m| {
            let mut best = 0.0_f32;
            let mut run = 0.0_f32;
            for seg in segments {
                let ts = super::inline_wrap::TabStops::of(&seg.style, m).unit;
                let piece = |t: &str| measure_text_w_styled(t, &seg.style, ts, m);
                if seg.style.white_space.is_nowrap() || seg.style.text_wrap_mode == TextWrapMode::Nowrap {
                    run += piece(&seg.text);
                    best = best.max(run);
                } else {
                    run = 0.0;
                    for word in split_css_whitespace(&seg.text) {
                        best = best.max(piece(word));
                    }
                }
            }
            best
        }),
        _ => b
            .children
            .iter()
            .filter(|c| {
                !matches!(c.kind, BoxKind::Skip)
                    && !matches!(c.style.position, Position::Absolute | Position::Fixed)
            })
            .map(|c| {
                let cem = c.style.font_size;
                let m = c.style.margin_top.resolve_or_zero(cem, 0.0, viewport)
                    + c.style.margin_bottom.resolve_or_zero(cem, 0.0, viewport);
                let ch = if matches!(c.style.writing_mode, WritingMode::HorizontalTb)
                    && !matches!(c.kind, BoxKind::InlineRun { .. })
                {
                    c.style.height.as_ref().and_then(|h| h.resolve(cem, None, viewport)).unwrap_or(0.0)
                } else {
                    min_content_outer_height(c, measurer, viewport)
                };
                ch + m
            })
            .fold(0.0_f32, f32::max),
    };
    content + frame
}

/// Ширины столбцов (border box по inline-оси) при заданной сумме `available` — как
/// `compute_table_col_widths` горизонтального пути (CSS 2.1 §17.5.2): столбцы с явным размером
/// его сохраняют, остальные получают min-content и делят остаток пропорционально max-content.
/// `stretch` — размер таблицы задан, и при отсутствии auto-столбцов излишек делится между явными.
fn distribute_columns(explicit: &[Option<f32>], min: &[f32], max: &[f32], available: f32, stretch: bool) -> Vec<f32> {
    let n = explicit.len();
    let sizes: Vec<Option<f32>> = (0..n).map(|i| explicit[i].map(|e| e.max(min[i]))).collect();
    let fixed_sum: f32 = sizes.iter().flatten().sum();
    let auto: Vec<usize> = (0..n).filter(|&i| sizes[i].is_none()).collect();
    if auto.is_empty() {
        let mut out: Vec<f32> = sizes.iter().map(|s| s.unwrap_or(0.0)).collect();
        if stretch && fixed_sum > 0.0 && available > fixed_sum {
            let k = available / fixed_sum;
            out.iter_mut().for_each(|v| *v *= k);
        } else if stretch && fixed_sum <= 0.0 && n > 0 {
            out.iter_mut().for_each(|v| *v = available / n as f32);
        }
        return out;
    }
    let room = (available - fixed_sum).max(0.0);
    let min_total: f32 = auto.iter().map(|&i| min[i]).sum();
    let weight_total: f32 = auto.iter().map(|&i| max[i].max(min[i])).sum();
    (0..n)
        .map(|i| {
            sizes[i].unwrap_or_else(|| {
                if min_total >= room {
                    if min_total > 0.0 { room * min[i] / min_total } else { room / auto.len() as f32 }
                } else {
                    let extra = room - min_total;
                    let w = max[i].max(min[i]);
                    min[i] + if weight_total > 0.0 { extra * w / weight_total } else { extra / auto.len() as f32 }
                }
            })
        })
        .collect()
}

/// Перекрытие рамок соседних ячеек на линиях сетки `border-collapse: collapse` (CSS 2.1
/// §17.6.2): на линии `k` между дорожками `k-1` и `k` — большая из конечной рамки ячеек,
/// заканчивающихся на ней, и начальной рамки ячеек, с неё начинающихся. Крайние линии — 0
/// (внешние рамки ячеек лежат на рамке таблицы).
fn collapse_overlaps(
    slots: &[Slot],
    cells: &[&LayoutBox],
    n: usize,
    along_cols: bool,
    rl: bool,
    rev: bool,
) -> Vec<f32> {
    let mut ov = vec![0.0_f32; n + 1];
    for (sl, cell) in slots.iter().zip(cells) {
        let st = &cell.style;
        let (start, end, a, len) = if along_cols {
            let (t, bt) = (st.border_top_width, st.border_bottom_width);
            let (s0, e0) = if rev { (bt, t) } else { (t, bt) };
            (s0, e0, sl.col, sl.cs)
        } else {
            let (l, r) = (st.border_left_width, st.border_right_width);
            let (s0, e0) = if rl { (r, l) } else { (l, r) };
            (s0, e0, sl.row, sl.rs)
        };
        if a > 0 {
            ov[a] = ov[a].max(start);
        }
        if a + len < n {
            ov[a + len] = ov[a + len].max(end);
        }
    }
    ov
}

/// Свободное место в ячейке по block-оси (физическая `x`) при толщине `target_w`: толщина за
/// вычетом рамки/padding и фактического размера содержимого (а не `rect.width`, куда входит
/// заданная `width` ячейки). Ячейка уже разложена в своей естественной ширине.
fn cell_block_free(cell: &LayoutBox, target_w: f32, rl: bool, viewport: Size) -> f32 {
    let st = &cell.style;
    let em = st.font_size;
    let left = st.border_left_width + st.padding_left.resolve_or_zero(em, 0.0, viewport);
    let right = st.border_right_width + st.padding_right.resolve_or_zero(em, 0.0, viewport);
    let kids = cell.children.iter().filter(|c| {
        !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed)
    });
    let content = if rl {
        let start = cell.rect.x + cell.rect.width - right;
        let end = kids
            .map(|c| c.rect.x - c.style.margin_left.resolve_or_zero(c.style.font_size, 0.0, viewport))
            .fold(start, f32::min);
        start - end
    } else {
        let start = cell.rect.x + left;
        let end = kids
            .map(|c| {
                c.rect.x + c.rect.width + c.style.margin_right.resolve_or_zero(c.style.font_size, 0.0, viewport)
            })
            .fold(start, f32::max);
        end - start
    };
    (target_w - left - right - content).max(0.0)
}

/// Вертикальная таблица `b` (`display: table`/`inline-table` в `vertical-*`/`sideways-*`).
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch(
    b: &mut LayoutBox,
    s: &Arc<ComputedStyle>,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> DispatchOutcome {
    let em = s.font_size;
    // Проценты отступов — от inline-размера содержащего блока, то есть его высоты.
    let cb_pct = available_height.unwrap_or(viewport.height).max(0.0);
    let margin_left = s.margin_left.resolve_or_zero(em, cb_pct, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb_pct, viewport);
    let margin_bottom = s.margin_bottom.resolve_or_zero(em, cb_pct, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb_pct, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb_pct, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb_pct, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb_pct, viewport);
    let frame_horiz = padding_left + padding_right + s.border_left_width + s.border_right_width;
    let frame_vert = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;

    let collapse = matches!(s.border_collapse, BorderCollapse::Collapse);
    // `border-spacing` физический: первая длина — по x (block-ось), вторая — по y (inline-ось).
    let (sp_b, sp_i) = if collapse { (0.0, 0.0) } else { (s.border_spacing_h, s.border_spacing_v) };
    let rl = matches!(s.writing_mode, WritingMode::VerticalRl | WritingMode::SidewaysRl);
    // `text-orientation: upright` принуждает используемое `direction` к `ltr` (Writing Modes L3 §5.1).
    let rtl = s.direction == Direction::Rtl && s.text_orientation != crate::style::TextOrientation::Upright;
    let rev = rtl != (s.writing_mode == WritingMode::SidewaysLr);

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;
    b.subgrid_tracks = None;
    b.table_caption_inset = [0.0; 2];

    let inline_avail = available_height.unwrap_or(viewport.height).max(0.0);
    let fill = (inline_avail - margin_top - margin_bottom).max(0.0);
    let explicit_inline =
        crate::vertical::resolve_axis_size(s.height.as_ref(), em, Some(inline_avail), viewport, s.box_sizing, frame_vert);
    let inline_limit = |len: Option<&Length>| -> Option<f32> {
        let raw = len.filter(|l| !l.is_intrinsic())?.resolve(em, Some(inline_avail), viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => raw + frame_vert,
            BoxSizing::BorderBox => raw.max(frame_vert),
        })
    };
    let max_inline = inline_limit(s.max_height.as_ref()).unwrap_or(f32::INFINITY);
    let min_inline = inline_limit(s.min_height.as_ref()).unwrap_or(0.0);
    let block_limit = |len: Option<&Length>| -> Option<f32> {
        let raw = len.filter(|l| !l.is_intrinsic())?.resolve(em, Some(available_width.max(0.0)), viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => raw + frame_horiz,
            BoxSizing::BorderBox => raw.max(frame_horiz),
        })
    };
    let max_block = block_limit(s.max_width.as_ref()).unwrap_or(f32::INFINITY);
    let min_block = block_limit(s.min_width.as_ref()).unwrap_or(0.0);
    let explicit_block = crate::vertical::resolve_axis_size(
        s.width.as_ref(), em, Some(available_width.max(0.0)), viewport, s.box_sizing, frame_horiz,
    )
    .map(|w| w.min(max_block).max(min_block));
    let block_avail = match explicit_block {
        Some(w) => (w - frame_horiz).max(0.0),
        None => (available_width - margin_left - frame_horiz).max(0.0),
    };

    let (rows, groups) = collect_rows(b);
    let (slots, n_cols) = place_slots(b, &rows);
    let n_rows = rows.len();
    let content_x0 = if collapse { b.rect.x } else { b.rect.x + s.border_left_width + padding_left };
    let content_y0 = if collapse { b.rect.y } else { b.rect.y + s.border_top_width + padding_top };

    if n_cols == 0 {
        b.rect.height = explicit_inline.unwrap_or(frame_vert).min(max_inline).max(min_inline);
        b.rect.width = explicit_block.unwrap_or(frame_horiz).min(max_block).max(min_block);
        return finish(b, s, em, available_width, pcb, content_x0, content_y0, measurer, viewport, hp);
    }

    // ── Размеры столбцов по inline-оси ──
    let cells: Vec<&LayoutBox> = slots.iter().map(|sl| &row_ref(b, rows[sl.row]).children[sl.cell]).collect();
    let (ov_i, ov_b) = if collapse {
        (
            collapse_overlaps(&slots, &cells, n_cols, true, rl, rev),
            collapse_overlaps(&slots, &cells, n_rows, false, rl, rev),
        )
    } else {
        (vec![0.0; n_cols + 1], vec![0.0; n_rows + 1])
    };
    let fixed_layout = s.table_layout == TableLayout::Fixed && explicit_inline.is_some();
    let mut col_exp: Vec<Option<f32>> = vec![None; n_cols];
    let mut col_min = vec![0.0_f32; n_cols];
    let mut col_max = vec![0.0_f32; n_cols];
    for (sl, cell) in slots.iter().zip(&cells) {
        let cem = cell.style.font_size;
        let frame = frame_vert_of(&cell.style, viewport);
        let ex = crate::vertical::resolve_axis_size(
            cell.style.height.as_ref(), cem, None, viewport, cell.style.box_sizing, frame,
        );
        let per = sl.cs as f32;
        if fixed_layout {
            // `table-layout: fixed`: размеры столбцов задаёт только первая строка.
            if sl.row == 0 && let Some(e) = ex {
                for slot in &mut col_exp[sl.col..sl.col + sl.cs] {
                    *slot = Some(slot.unwrap_or(0.0).max(e / per));
                }
            }
            continue;
        }
        match ex {
            Some(e) if sl.cs == 1 => col_exp[sl.col] = Some(col_exp[sl.col].unwrap_or(0.0).max(e)),
            _ => {}
        }
        let (mn, mx) = match ex {
            Some(e) => (e, e),
            None => (min_content_outer_height(cell, measurer, viewport), max_content_outer_height(cell, measurer, viewport)),
        };
        for c in sl.col..sl.col + sl.cs {
            col_min[c] = col_min[c].max(mn / per);
            col_max[c] = col_max[c].max(mx / per);
        }
    }
    let n_i = n_cols as f32;
    // Таблица по inline-оси = сумма столбцов + overhead (рамка/padding и щели; в collapse — минус перекрытия).
    let overhead_i = if collapse { -ov_i.iter().sum::<f32>() } else { frame_vert + (n_i + 1.0) * sp_i };
    let pref_cols: f32 = (0..n_cols).map(|c| col_exp[c].unwrap_or(col_max[c]).max(col_min[c])).sum();
    let inline_total = match explicit_inline {
        Some(h) => h,
        None => (pref_cols + overhead_i).min(fill),
    }
    .min(max_inline)
    .max(min_inline);
    let cols_sum = (inline_total - overhead_i).max(0.0);
    let cols = distribute_columns(&col_exp, &col_min, &col_max, cols_sum, explicit_inline.is_some());
    let inline_total = inline_total.max(cols.iter().sum::<f32>() + overhead_i);
    let grid_len_i = inline_total - if collapse { 0.0 } else { frame_vert };

    // Логические смещения столбцов от начала inline-оси сетки.
    let mut ipos = Vec::with_capacity(n_cols);
    let mut cur = if collapse { 0.0 } else { sp_i };
    for c in 0..n_cols {
        if c > 0 {
            cur -= ov_i[c];
        }
        ipos.push(cur);
        cur += cols[c] + sp_i;
    }
    // Физические `(y, длина)` полуинтервала столбцов `[a, e)`.
    let span_y = |a: usize, e: usize| -> (f32, f32) {
        let len = ipos[e - 1] + cols[e - 1] - ipos[a];
        let y = if rev { content_y0 + grid_len_i - (ipos[e - 1] + cols[e - 1]) } else { content_y0 + ipos[a] };
        (y, len)
    };

    // ── Проход 1: ячейки на столбцах, естественная толщина ──
    let mut thick = vec![0.0_f32; n_rows];
    let mut natural_w = vec![0.0_f32; slots.len()];
    for (k, sl) in slots.iter().enumerate() {
        let (y, inline) = span_y(sl.col, sl.col + sl.cs);
        let rr = rows[sl.row];
        let cell = &mut row_mut(b, rr).children[sl.cell];
        let saved_h = Arc::make_mut(&mut cell.style).height.take();
        lay_out(cell, 0.0, y, block_avail, Some(inline), measurer, viewport, pcb, hp, false);
        Arc::make_mut(&mut cell.style).height = saved_h;
        cell.rect.height = inline;
        natural_w[k] = cell.rect.width;
        if sl.rs == 1 {
            thick[sl.row] = thick[sl.row].max(cell.rect.width);
        }
    }
    // Явная `width` строки (её толщина).
    for (ri, &rr) in rows.iter().enumerate() {
        let r = row_ref(b, rr);
        let rem = r.style.font_size;
        if let Some(w) = crate::vertical::resolve_axis_size(
            r.style.width.as_ref(), rem, Some(block_avail), viewport, r.style.box_sizing, 0.0,
        ) {
            thick[ri] = thick[ri].max(w);
        }
    }
    // Ячейки на несколько строк: нехватку добавляем в последнюю из охваченных строк.
    for (k, sl) in slots.iter().enumerate() {
        if sl.rs > 1 {
            let covered: f32 = thick[sl.row..sl.row + sl.rs].iter().sum::<f32>() + (sl.rs - 1) as f32 * sp_b;
            if natural_w[k] > covered {
                thick[sl.row + sl.rs - 1] += natural_w[k] - covered;
            }
        }
    }

    // ── Толщина таблицы по block-оси ──
    let n_r = n_rows as f32;
    let overhead_b = if collapse { -ov_b.iter().sum::<f32>() } else { frame_horiz + (n_r + 1.0) * sp_b };
    let natural_b = thick.iter().sum::<f32>() + overhead_b;
    let block_total = explicit_block.unwrap_or(natural_b).max(natural_b);
    let extra = block_total - natural_b;
    if extra > 0.01 && n_rows > 0 {
        let sum: f32 = thick.iter().sum();
        for t in &mut thick {
            *t += if sum > 0.0 { extra * *t / sum } else { extra / n_r };
        }
    }
    let grid_len_b = block_total - if collapse { 0.0 } else { frame_horiz };
    let mut bpos = Vec::with_capacity(n_rows);
    let mut cur = if collapse { 0.0 } else { sp_b };
    for r in 0..n_rows {
        if r > 0 {
            cur -= ov_b[r];
        }
        bpos.push(cur);
        cur += thick[r] + sp_b;
    }
    let row_x = |r: usize, len: f32| -> f32 {
        if rl { content_x0 + grid_len_b - bpos[r] - len } else { content_x0 + bpos[r] }
    };

    b.rect.width = block_total;
    b.rect.height = inline_total;

    // ── Проход 2: ячейки занимают всю толщину охваченных строк, содержимое по `vertical-align` ──
    for (k, sl) in slots.iter().enumerate() {
        let last = sl.row + sl.rs - 1;
        let target_w = bpos[last] + thick[last] - bpos[sl.row];
        let x = if rl { content_x0 + grid_len_b - (bpos[last] + thick[last]) } else { content_x0 + bpos[sl.row] };
        let rr = rows[sl.row];
        let cell = &mut row_mut(b, rr).children[sl.cell];
        let nat = natural_w[k];
        let free = cell_block_free(cell, target_w, rl, viewport);
        // Содержимое лежит у начала block-оси естественной ширины; при `rl` начало — правый край.
        let base = if rl { target_w - nat } else { 0.0 };
        let toward_end = match super::table_valign::cell_block_align(&cell.style) {
            VerticalAlign::Middle => free / 2.0,
            VerticalAlign::Bottom => free,
            _ => 0.0,
        };
        let dx = base + if rl { -toward_end } else { toward_end };
        shift_tree(cell, x, 0.0);
        if dx.abs() > 1e-4 {
            for child in &mut cell.children {
                shift_tree(child, dx, 0.0);
            }
        }
        cell.rect.width = target_w;
    }

    // ── Строки, группы, колонки ──
    for (ri, &rr) in rows.iter().enumerate() {
        let r = row_mut(b, rr);
        r.rect = Rect::new(row_x(ri, thick[ri]), content_y0, thick[ri], grid_len_i);
    }
    for &(gi, first, count) in &groups {
        if count == 0 {
            b.children[gi].rect = Rect::new(content_x0, content_y0, 0.0, grid_len_i);
            continue;
        }
        let lo = (first..first + count).map(|r| row_x(r, thick[r])).fold(f32::INFINITY, f32::min);
        let hi = (first..first + count).map(|r| row_x(r, thick[r]) + thick[r]).fold(f32::NEG_INFINITY, f32::max);
        b.children[gi].rect = Rect::new(lo, content_y0, hi - lo, grid_len_i);
    }
    place_column_boxes(b, cols.len(), span_y, content_x0, grid_len_b);

    finish(b, s, em, available_width, pcb, content_x0, content_y0, measurer, viewport, hp)
}

/// `display: table-column(-group)` не раскладываются как блоки: их прямоугольник — «покрытые
/// столбцы × вся толщина сетки» (нужен для фона и рамки, рисуемых под строками и ячейками).
fn place_column_boxes(
    b: &mut LayoutBox,
    n: usize,
    span_y: impl Fn(usize, usize) -> (f32, f32),
    content_x0: f32,
    grid_len_b: f32,
) {
    let span_rect = |a: usize, e: usize| -> Option<Rect> {
        let (a, e) = (a.min(n), e.min(n));
        if a >= e {
            return None;
        }
        let (y, len) = span_y(a, e);
        Some(Rect::new(content_x0, y, grid_len_b, len))
    };
    let mut next = 0usize;
    for child in &mut b.children {
        match child.style.display {
            Display::TableColumn => {
                let end = next + child.col_span as usize;
                if let Some(r) = span_rect(next, end) {
                    child.rect = r;
                }
                next = end;
            }
            Display::TableColumnGroup => {
                let start = next;
                let has_cols = child.children.iter().any(|c| c.style.display == Display::TableColumn);
                if has_cols {
                    for col in &mut child.children {
                        if col.style.display != Display::TableColumn {
                            continue;
                        }
                        let end = next + col.col_span as usize;
                        if let Some(r) = span_rect(next, end) {
                            col.rect = r;
                        }
                        next = end;
                    }
                } else {
                    next += child.col_span as usize;
                }
                if let Some(r) = span_rect(start, next) {
                    child.rect = r;
                }
            }
            _ => {}
        }
    }
}

/// Хвост раскладки: абсолютные дети таблицы (CSS Position L3 §3 — не части таблицы, статическая
/// позиция — угол content box), `position: relative`.
#[allow(clippy::too_many_arguments)]
fn finish(
    b: &mut LayoutBox,
    s: &Arc<ComputedStyle>,
    em: f32,
    cb: f32,
    pcb: Rect,
    content_x0: f32,
    content_y0: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> DispatchOutcome {
    let abs: Vec<(usize, f32, f32)> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| (i, content_x0, content_y0))
        .collect();
    let is_positioned = super::multicol_abspos::establishes_abs_cb(s);
    super::layout_dispatch::finish_after_match(b, s, em, cb, Some(cb), is_positioned, pcb, &abs, measurer, viewport, hp);
    DispatchOutcome::Done
}
