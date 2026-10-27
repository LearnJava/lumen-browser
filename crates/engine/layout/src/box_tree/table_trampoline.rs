use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};
use crate::style::Visibility;

/// One row's worth of loop-entry state — column placement resolved ahead of
/// time (CSS 2.1 §17.5 Steps 1–2: map each cell to its starting column,
/// skipping rowspan-occupied columns, and its border-box width) by
/// `table::build_table_init`'s per-row scan. Purely a function of static
/// `col_span`/`row_span` metadata and `TableInit::col_widths`, never of a
/// laid-out `.rect` — see that function's doc comment for why the whole
/// column-resolution pass (rowspan occupancy threaded across every row of the
/// table, in DOM order) can run natively before any cell is dispatched.
pub(super) struct RowInit {
    /// Indices into the row's `children` — non-`Skip` cells, source order.
    pub(super) cell_idxs: Vec<usize>,
    /// `(col_start, border-box width)`, parallel to `cell_idxs`.
    pub(super) cell_cols: Vec<(usize, f32)>,
}

/// One top-level child of the `<table>` box — either a direct `<tr>` or a
/// `<tbody>`/`<thead>`/`<tfoot>` row group with its own (possibly empty) row
/// list. `rows`' first element of each pair is the row's actual index into
/// the group's `children` (not a 0-based position — matches the removed
/// code's `r` loop variable, which skips non-`TableRow` children).
pub(super) enum TopEntry {
    Row { child_idx: usize, row: RowInit },
    Group { child_idx: usize, rows: Vec<(usize, RowInit)> },
}

/// Loop-entry state for the table dispatch arm's per-row/per-cell placement
/// pass (CSS 2.1 §17.5/§17.6.2) — everything the removed inline triple loop
/// (pre-LAYOUT-2-срез-6 `table.rs`'s `lay_out_table`/`lay_out_table_row`) read
/// or wrote across rows/cells, plus what `layout_dispatch.rs`'s table dispatch
/// arm used to do with `lay_out_table`'s return value once it came back
/// (container height). Captured by `table::build_table_init` before any cell
/// is placed — column widths, collapsed-border geometry, and every row's
/// column assignment (Steps 1–2) still run natively there; see its doc
/// comment for why (none of it ever calls `lay_out` on a cell).
pub(super) struct TableInit {
    pub(super) top_level: Vec<TopEntry>,
    pub(super) col_widths: Vec<f32>,
    pub(super) collapse: bool,
    pub(super) h_spacing: f32,
    pub(super) v_spacing: f32,
    /// CSS 2.1 §17.6.2 collapsed border model — absolute x of each column's
    /// cell border-box left edge. `None` in the separated border model.
    pub(super) collapse_col_x: Option<Vec<f32>>,
    pub(super) collapse_v_overlap: f32,
    pub(super) collapse_width: f32,
    pub(super) n_cols: usize,
    pub(super) content_x: f32,
    pub(super) content_y: f32,
    pub(super) content_width: f32,
    pub(super) children_pcb: Rect,
    // Phase-running state, mutated by `begin_row`/`finish_row`/`finish_entry`.
    pub(super) cur_y: f32,
    /// `(y, height)` per row, DOM order — read back by `finish_table`'s
    /// rowspan post-fix (Pass 2) and the collapse-mode height epilogue.
    pub(super) flat_row_rects: Vec<(f32, f32)>,
    /// CSS Tables 3 §«visibility: collapse» — per flat row: the row (or its row group) is
    /// collapsed, so it has zero height and contributes no `border-spacing` slot.
    pub(super) collapsed_rows: Vec<bool>,
    /// `(group_child_idx, row_field, cell_local_idx, start_flat_row, row_span)`
    /// — `row_field` is the row's own top-level `child_idx` when `group` is
    /// `None`, or its index within the group's `children` when `group` is
    /// `Some` (mirrors the removed code's `(group, row, child_idx, ...)`
    /// tuple exactly, including the two different meanings of `row`).
    pub(super) span_fixes: Vec<(Option<usize>, usize, usize, usize, u32)>,
    // Phase-epilogue inputs (`finish_table` only) — ride along unchanged from
    // `build_table_init`, same as `GridInit`'s equivalent fields.
    pub(super) s: Arc<ComputedStyle>,
    pub(super) em: f32,
    pub(super) available_height: Option<f32>,
    pub(super) padding_top: f32,
    pub(super) padding_bottom: f32,
    /// Indices (into the table's `children`) of `caption-side: bottom` captions, placed by
    /// `finish_table` once the rows' extent is known.
    pub(super) bottom_captions: Vec<usize>,
    /// Высота подписей `caption-side: top` (margin box), уже отложенная над сеткой.
    pub(super) top_caption_h: f32,
    /// Сдвиг содержимого ячеек `middle`/`bottom` по их `vertical-align` — ключ как у
    /// `span_fixes`; `table_height::distribute` снимает его, когда ячейка вырастает.
    pub(super) cell_dy: Vec<(Option<usize>, usize, usize, f32)>,
}

/// One level of the explicit stack `run` maintains in place of the native
/// call stack — the table-container analogue of `grid_trampoline::Frame`.
/// `b` is owned (taken via `block_flow_trampoline::take_box`) for the
/// duration of this table's own row/cell placement loop. Table nests three
/// loops (top-level entry → row-within-group → cell-within-row) instead of
/// grid/flex's flat item list, so position is `(t, r, cell)` rather than a
/// single index — `group_row_y` is the one extra piece of running state a
/// `Group` entry needs across its own rows (mirrors the removed code's
/// group-local `row_y`, distinct from `TableInit::cur_y`'s table-level one).
struct Frame {
    b: LayoutBox,
    init: Box<TableInit>,
    /// Index into `init.top_level`.
    t: usize,
    /// Row-within-group position (`TopEntry::Group` only; unused/0 for a
    /// direct `Row` entry, which has exactly one implicit row).
    r: usize,
    /// Cell-within-current-row position.
    cell: usize,
    group_row_y: f32,
}

enum StepOutcome {
    /// This cell is fully placed (synchronously, or via the block-flow/flex/
    /// grid trampoline) — move on to the next.
    Advance,
    /// This cell's content is itself a table with its own rows to place —
    /// push the current frame and continue processing this one.
    Descend(Box<TableInit>),
    /// This cell's content resolved to another kind's trampoline (block-flow/
    /// flex/grid/multicol/vertical) — `run` drives it via `run_cell_content`.
    Foreign(DispatchOutcome),
}

/// Drives the current cell's own trampoline once `step_cell` has returned. Out
/// of line and called from `run`, not from `step_cell`: a table nested in a
/// cell re-enters `run` through `block_flow_trampoline::run`, one native frame
/// pair per nesting level, so every frame kept off that chain (here
/// `step_cell`'s) raises the depth a test thread's 2 MiB stack can hold.
#[inline(never)]
fn run_cell_content(
    frame: &mut Frame,
    outcome: DispatchOutcome,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let cell_box = current_cell_mut(frame);
    match outcome {
        DispatchOutcome::NeedsBlockFlowLoop(ci) => {
            block_flow_trampoline::run(cell_box, ci, measurer, viewport, hp);
        }
        DispatchOutcome::NeedsFlexLoop(ci) => super::flex_trampoline::run(cell_box, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsGridLoop(ci) => super::grid_trampoline::run(cell_box, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsMulticolLoop(ci) => super::multicol_trampoline::run(cell_box, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsVerticalLoop(ci) => super::vertical_trampoline::run(cell_box, ci, measurer, viewport, hp),
        DispatchOutcome::Done | DispatchOutcome::NeedsTableLoop(_) => {
            unreachable!("step_cell handles Done/NeedsTableLoop itself")
        }
    }
}

fn row_init_ref(entry: &TopEntry, r: usize) -> &RowInit {
    match entry {
        TopEntry::Row { row, .. } => row,
        TopEntry::Group { rows, .. } => &rows[r].1,
    }
}

fn row_count_of(entry: &TopEntry) -> usize {
    match entry {
        TopEntry::Row { .. } => 1,
        TopEntry::Group { rows, .. } => rows.len(),
    }
}

/// Locates the row `LayoutBox` for `(t, r)` — `b.children[child_idx]` for a
/// direct row, `b.children[group_idx].children[row_idx]` for a grouped one.
/// `top_level` is borrowed separately from `b` so callers can hold both a
/// `&mut` into `b` and (afterwards) a fresh borrow of `init` without
/// conflict — the two are disjoint fields of `Frame`.
fn row_box_mut<'a>(b: &'a mut LayoutBox, top_level: &[TopEntry], t: usize, r: usize) -> &'a mut LayoutBox {
    match &top_level[t] {
        TopEntry::Row { child_idx, .. } => &mut b.children[*child_idx],
        TopEntry::Group { child_idx, rows } => {
            let row_idx = rows[r].0;
            &mut b.children[*child_idx].children[row_idx]
        }
    }
}

/// Drives `init`'s row/cell placement pass (and every further table
/// descendant it meets — a `<td>` containing another `<table>`) on an
/// explicit heap stack, so a chain of nested tables no longer grows the
/// native call stack one frame per level (LAYOUT-2's acceptance criterion,
/// applied to item (4) of its ROADMAP entry). `b` is the box `dispatch_box`'s
/// table arm was originally called on.
///
/// A cell whose own content resolves to a *different* dispatcher (block-flow/
/// flex/grid) still calls that trampoline's `run` synchronously (one native
/// frame per kind transition) — the same composition every other LAYOUT-2
/// slice settled for; only same-kind (table-in-table) chains are pushed onto
/// this loop's own `stack` instead.
pub(super) fn run(
    b: &mut LayoutBox,
    init: Box<TableInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    // Heap frames (`Box<Frame>`), moved in and out by the out-of-line helpers
    // below: `run` sits on the native stack once per table-in-cell level (see
    // the doc comment above), so a by-value frame would be paid on every level.
    let mut current = enter(b, init);
    let mut stack: Vec<Box<Frame>> = Vec::new();

    loop {
        if current.t >= current.init.top_level.len() {
            finish_table(&mut current, measurer, viewport, hp);
            match stack.pop() {
                None => {
                    leave(b, current);
                    return;
                }
                Some(mut parent) => {
                    return_to_parent(&mut parent, current);
                    parent.cell += 1;
                    current = parent;
                }
            }
            continue;
        }

        // Freshly entering this top-level entry (before any of its rows) —
        // `Group`-only: place the group box and seed its own row cursor.
        if current.r == 0 && current.cell == 0 {
            begin_group(&mut current, viewport);
        }

        let row_count = row_count_of(&current.init.top_level[current.t]);
        if current.r >= row_count {
            finish_entry(&mut current, viewport);
            current.t += 1;
            current.r = 0;
            current.cell = 0;
            continue;
        }

        if current.cell == 0 {
            begin_row(&mut current, viewport);
        }

        let n_cells = row_init_ref(&current.init.top_level[current.t], current.r).cell_idxs.len();
        if current.cell >= n_cells {
            finish_row(&mut current, measurer, viewport);
            current.r += 1;
            current.cell = 0;
            continue;
        }

        match step_cell(&mut current, measurer, viewport, hp) {
            StepOutcome::Advance => {
                current.cell += 1;
            }
            StepOutcome::Foreign(outcome) => {
                run_cell_content(&mut current, outcome, measurer, viewport, hp);
                current.cell += 1;
            }
            StepOutcome::Descend(child_init) => {
                let child_frame = enter(current_cell_mut(&mut current), child_init);
                stack.push(current);
                current = child_frame;
            }
        }
    }
}

/// A heap frame owning `slot`'s box (see `run`). Out of line, like [`leave`]
/// and [`return_to_parent`]: their `LayoutBox` moves stay off `run`'s frame.
#[inline(never)]
fn enter(slot: &mut LayoutBox, init: Box<TableInit>) -> Box<Frame> {
    Box::new(Frame { b: block_flow_trampoline::take_box(slot), init, t: 0, r: 0, cell: 0, group_row_y: 0.0 })
}

/// Puts the root frame's box back into `b`.
#[inline(never)]
fn leave(b: &mut LayoutBox, frame: Box<Frame>) {
    *b = frame.b;
}

/// Puts a finished nested table back into the parent's current cell.
#[inline(never)]
fn return_to_parent(parent: &mut Frame, child: Box<Frame>) {
    *current_cell_mut(parent) = child.b;
}

/// The cell `frame`'s `(t, r, cell)` cursor points at.
fn current_cell_mut(frame: &mut Frame) -> &mut LayoutBox {
    let cell_local_idx = row_init_ref(&frame.init.top_level[frame.t], frame.r).cell_idxs[frame.cell];
    let row = row_box_mut(&mut frame.b, &frame.init.top_level, frame.t, frame.r);
    &mut row.children[cell_local_idx]
}

/// Places the row group's own box (`content_x`/`group_y`/`content_width`) and
/// seeds `frame.group_row_y` — copied from the removed code's `TableRowGroup`
/// arm, the part that ran once before that group's row loop. A no-op for a
/// direct `Row` entry (nothing to place — the row itself is positioned by
/// `begin_row` against `TableInit::cur_y`).
fn begin_group(frame: &mut Frame, viewport: Size) {
    let t = frame.t;
    let TopEntry::Group { child_idx, .. } = &frame.init.top_level[t] else { return };
    let child_idx = *child_idx;
    let content_x = frame.init.content_x;
    let content_width = frame.init.content_width;
    let cur_y = frame.init.cur_y;
    let group = &mut frame.b.children[child_idx];
    let g_mt = group.style.margin_top.resolve_or_zero(group.style.font_size, content_width, viewport);
    let group_y = cur_y + g_mt;
    group.rect.x = content_x;
    group.rect.y = group_y;
    group.rect.width = content_width;
    frame.group_row_y = group_y;
}

/// Places the row's own box (`content_x`/`row_y`/`content_width`) — copied
/// from the removed code's per-row prologue (identical shape for a direct row
/// against `cur_y` and a grouped row against the group-local `row_y`).
fn begin_row(frame: &mut Frame, viewport: Size) {
    let t = frame.t;
    let r = frame.r;
    let content_x = frame.init.content_x;
    let content_width = frame.init.content_width;
    let row_start_y = match &frame.init.top_level[t] {
        TopEntry::Row { .. } => frame.init.cur_y,
        TopEntry::Group { .. } => frame.group_row_y,
    };
    let row = row_box_mut(&mut frame.b, &frame.init.top_level, t, r);
    let c_mt = row.style.margin_top.resolve_or_zero(row.style.font_size, content_width, viewport);
    row.rect.x = content_x;
    row.rect.y = row_start_y + c_mt;
    row.rect.width = content_width;
}

/// Handles exactly one cell of the current row — CSS 2.1 §17.5 Step 3 (cell
/// x from precomputed `cell_cols`/collapse geometry, then dispatch at the
/// precomputed width), copied from the removed `lay_out_table_row`'s
/// `col_widths: Some` branch except at the one recursive call, which now
/// either finishes synchronously (`Advance`) or hands back the cell's
/// `TableInit` for `run` to push and descend into. The cell's CSS `width` is
/// cleared for the call and restored right after: `dispatch_box` resolves
/// `b.rect.width` from `s.width` once, synchronously, before returning any
/// outcome (including `NeedsTableLoop`), so restoring here — rather than
/// deferring past a possible descend — is safe and matches every other
/// trampoline's item-dispatch shape.
#[inline(never)]
fn step_cell(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> StepOutcome {
    let t = frame.t;
    let r = frame.r;
    let cell = frame.cell;
    let (cell_local_idx, col_start, avail) = {
        let ri = row_init_ref(&frame.init.top_level[t], r);
        let (col_start, avail) = ri.cell_cols[cell];
        (ri.cell_idxs[cell], col_start, avail)
    };
    let content_x = frame.init.content_x;
    let h_spacing = frame.init.h_spacing;
    let cell_x = if let Some(cx) = &frame.init.collapse_col_x {
        cx.get(col_start).copied().unwrap_or(content_x)
    } else {
        content_x
            + (col_start + 1) as f32 * h_spacing
            + frame.init.col_widths[..col_start.min(frame.init.col_widths.len())].iter().sum::<f32>()
    };
    let pcb = frame.init.children_pcb;

    let row = row_box_mut(&mut frame.b, &frame.init.top_level, t, r);
    let row_content_y = row.rect.y;
    let row_cell_h = super::table_height::row_cell_height(row, frame.init.content_width, viewport);
    let cell_box = &mut row.children[cell_local_idx];
    let saved_width = Arc::make_mut(&mut cell_box.style).width.take();
    // Высота строки определена → потомки ячейки без своей `height` считают `%` от неё
    // (CSS Tables L3 §3.4): ячейка получает её на время раскладки, как и ширину.
    let saved_height = row_cell_h
        .filter(|_| cell_box.row_span == 1 && cell_box.style.height.is_none())
        .filter(|_| matches!(cell_box.style.writing_mode, crate::style::WritingMode::HorizontalTb))
        .map(|h| {
            let st = Arc::make_mut(&mut cell_box.style);
            st.height = Some(Length::Px(h));
            std::mem::replace(&mut st.box_sizing, BoxSizing::BorderBox)
        });
    // Ячейка с вертикальным `writing-mode` (ортогональный поток в горизонтальной таблице): её
    // физическая ширина — размер по блочной оси, `auto` дал бы ширину по содержимому, а ячейка
    // обязана занять свой столбец целиком.
    if !matches!(cell_box.style.writing_mode, crate::style::WritingMode::HorizontalTb) {
        let st = Arc::make_mut(&mut cell_box.style);
        let em = st.font_size;
        let frame = st.padding_left.resolve_or_zero(em, avail, viewport)
            + st.padding_right.resolve_or_zero(em, avail, viewport)
            + st.border_left_width
            + st.border_right_width;
        st.width = Some(Length::Px(match st.box_sizing {
            BoxSizing::ContentBox => (avail - frame).max(0.0),
            BoxSizing::BorderBox => avail,
        }));
    }
    let outcome = dispatch_box(
        cell_box, cell_x, row_content_y, avail, None, measurer, viewport, pcb, hp,
        false, None, AlignValue::Auto, None,
    );
    Arc::make_mut(&mut cell_box.style).width = saved_width;
    if let Some(box_sizing) = saved_height {
        let st = Arc::make_mut(&mut cell_box.style);
        st.height = None;
        st.box_sizing = box_sizing;
    }
    match outcome {
        DispatchOutcome::Done => StepOutcome::Advance,
        DispatchOutcome::NeedsTableLoop(ci) => StepOutcome::Descend(ci),
        // Block-flow/flex/grid content, and (LAYOUT-2 срез 7/8) a multicol or
        // vertical writing-mode container — driven by `run_cell_content`.
        outcome => StepOutcome::Foreign(outcome),
    }
}

/// Runs once every cell of the current row is placed — CSS 2.1 §17.5 Step 4
/// (non-rowspan cells normalised to the row's max height), the row's own
/// border-box height (`s.height` or content-derived), `flat_row_rects`/
/// `span_fixes` bookkeeping, and the row-cursor advance (`TableInit::cur_y`
/// for a direct row, `frame.group_row_y` for a grouped one) — copied from the
/// removed code's per-row epilogue (`lay_out_table_row`'s Step 4 plus
/// `lay_out_table`'s per-row wrapper tail).
fn finish_row(frame: &mut Frame, measurer: Option<&dyn TextMeasurer>, viewport: Size) {
    let t = frame.t;
    let r = frame.r;
    let content_width = frame.init.content_width;
    let v_spacing = frame.init.v_spacing;
    let collapse_v_overlap = frame.init.collapse_v_overlap;
    let flat_idx = frame.init.flat_row_rects.len();
    let cell_idxs = row_init_ref(&frame.init.top_level[t], r).cell_idxs.clone();
    let (group_opt, row_field) = match &frame.init.top_level[t] {
        TopEntry::Row { child_idx, .. } => (None, *child_idx),
        TopEntry::Group { child_idx, rows } => (Some(*child_idx), rows[r].0),
    };

    // §17.5.5 / Tables 3: строка с `visibility: collapse` — или строка в такой группе, даже
    // с собственным `visibility: visible` — не занимает места: высота 0 и без слота
    // `border-spacing` вокруг неё.
    let group_collapsed = match &frame.init.top_level[t] {
        TopEntry::Group { child_idx, .. } => frame.b.children[*child_idx].style.visibility == Visibility::Collapse,
        TopEntry::Row { .. } => false,
    };
    let row = row_box_mut(&mut frame.b, &frame.init.top_level, t, r);
    let collapsed = group_collapsed || row.style.visibility == Visibility::Collapse;
    // CSS 2.1 §17.5.3: ячейки, выровненные по базовой линии, ставятся так, чтобы
    // их базовые линии совпали; высота строки — не меньше суммы наибольшего
    // подъёма и наибольшего спуска.
    let baselines: Vec<(usize, f32)> = cell_idxs
        .iter()
        .filter(|&&i| super::table_valign::is_baseline_aligned(&row.children[i]))
        .filter_map(|&i| {
            super::table_valign::cell_own_baseline(&row.children[i], BaselineSide::First, measurer).map(|bl| (i, bl))
        })
        .collect();
    let above = baselines.iter().map(|&(_, bl)| bl).fold(0.0_f32, f32::max);
    let below = baselines
        .iter()
        .map(|&(i, bl)| super::table_valign::natural_height(&row.children[i]) - bl)
        .fold(0.0_f32, f32::max);
    let mut row_h = cell_idxs
        .iter()
        .filter(|&&i| row.children[i].row_span == 1)
        .map(|&i| row.children[i].rect.height)
        .fold(0.0_f32, f32::max);
    if !baselines.is_empty() {
        row_h = row_h.max(above + below);
    }

    let em = row.style.font_size;
    let row_pad = {
        let pt = row.style.padding_top.resolve_or_zero(em, content_width, viewport);
        let pb = row.style.padding_bottom.resolve_or_zero(em, content_width, viewport);
        pt + pb + row.style.border_top_width + row.style.border_bottom_width
    };
    let row_style_h = if let Some(h_len) = &row.style.height
        && let Some(h) = h_len.resolve(em, None, viewport)
    {
        match row.style.box_sizing {
            BoxSizing::ContentBox => (h + row_pad).max(0.0),
            BoxSizing::BorderBox => h.max(row_pad),
        }
    } else {
        row_h + row_pad
    };
    // Ячейки занимают всю высоту строки (§17.5.3), в том числе заданную `height` строки.
    let row_h = row_h.max(row_style_h - row_pad);
    let (row_h, row_style_h) = if collapsed { (0.0, 0.0) } else { (row_h, row_style_h) };
    for &i in &cell_idxs {
        let cell = &mut row.children[i];
        if cell.row_span != 1 {
            continue;
        }
        let dy = match super::table_valign::cell_block_align(&cell.style) {
            VerticalAlign::Top => 0.0,
            va @ (VerticalAlign::Middle | VerticalAlign::Bottom) => {
                super::table_valign::free_space_shift(cell, row_h, va)
            }
            _ => baselines.iter().find(|&&(j, _)| j == i).map_or(0.0, |&(_, bl)| above - bl),
        };
        super::table_valign::shift_cell_content(cell, dy);
        if dy.abs() > 1e-4 && matches!(super::table_valign::cell_block_align(&cell.style), VerticalAlign::Middle | VerticalAlign::Bottom) {
            frame.init.cell_dy.push((group_opt, row_field, i, dy));
        }
        cell.rect.height = row_h;
    }
    let row_style_h = row_style_h.max(row_h + row_pad);
    row.rect.height = row_style_h;
    let row_rect_y = row.rect.y;

    for &i in &cell_idxs {
        if row.children[i].row_span > 1 {
            frame.init.span_fixes.push((group_opt, row_field, i, flat_idx, row.children[i].row_span));
        }
    }

    let c_mb = row.style.margin_bottom.resolve_or_zero(em, content_width, viewport);
    let next_y = if collapsed {
        row_rect_y
    } else {
        row_rect_y + row_style_h + c_mb + v_spacing - collapse_v_overlap
    };
    frame.init.flat_row_rects.push((row_rect_y, row_style_h));
    frame.init.collapsed_rows.push(collapsed);

    match &frame.init.top_level[t] {
        TopEntry::Row { .. } => frame.init.cur_y = next_y,
        TopEntry::Group { .. } => frame.group_row_y = next_y,
    }
}

/// Runs once every row of a `Group` entry is placed (including an empty
/// group, zero rows) — the group's own border-box height and
/// `TableInit::cur_y` advance past it, copied from the removed code's
/// `TableRowGroup` arm tail. A no-op for a direct `Row` entry — `finish_row`
/// already advanced `cur_y` for it.
fn finish_entry(frame: &mut Frame, viewport: Size) {
    let t = frame.t;
    let TopEntry::Group { child_idx, .. } = &frame.init.top_level[t] else { return };
    let child_idx = *child_idx;
    let content_width = frame.init.content_width;
    let group_row_y = frame.group_row_y;
    let group = &mut frame.b.children[child_idx];
    let em = group.style.font_size;
    let g_pt = group.style.padding_top.resolve_or_zero(em, content_width, viewport);
    let g_pb = group.style.padding_bottom.resolve_or_zero(em, content_width, viewport);
    let g_bor = group.style.border_top_width + group.style.border_bottom_width;
    let group_y = group.rect.y;
    group.rect.height = (group_row_y - group_y) + g_pt + g_pb + g_bor;
    let g_mb = group.style.margin_bottom.resolve_or_zero(em, content_width, viewport);
    frame.init.cur_y = group.rect.y + group.rect.height + g_mb;
}

/// CSS 2.1 §17.6.2 — в collapsed-режиме рамка строки/группы строк лежит на линии сетки, и
/// `width` таблицы задаёт ширину сетки, а не рамки: внешняя рамка выступает за неё. Здесь
/// строки и группы получают border-box `сетка + половина своих левой и правой рамок`, а
/// таблица — на наибольшую из таких надбавок шире. Рамки ячеек уже учтены привязкой крайних ячеек.
fn widen_for_outer_group_borders(frame: &mut Frame) {
    let mut extra = 0.0_f32;
    for child in &mut frame.b.children {
        if matches!(child.kind, BoxKind::TableRow | BoxKind::TableRowGroup) {
            let bw = (child.style.border_left_width + child.style.border_right_width) / 2.0;
            child.rect.width += bw;
            extra = extra.max(bw);
        }
    }
    frame.b.rect.width += extra;
}

/// Runs once every top-level entry is placed — CSS 2.1 §17.5 Pass 2 (extend
/// each rowspan cell's height to its last spanned row's bottom edge), then
/// (moved in from `layout_dispatch.rs`'s former post-`lay_out_table` code)
/// the table's own border-box width/height: collapse-mode geometry first
/// (matching the removed `lay_out_table`'s tail), then the explicit-`height`
/// override (either mode), then non-collapse content-derived auto height.
/// Copied from the removed code's tail plus the dispatch arm.
fn finish_table(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    for &(group, row, child_idx, start_flat, span) in &frame.init.span_fixes {
        let end_flat = (start_flat + span as usize).min(frame.init.flat_row_rects.len());
        if end_flat == 0 {
            continue;
        }
        // Ячейка тянется до нижнего края последней несхлопнутой строки охвата; схлопнутые
        // строки ей высоты не добавляют (если все схлопнуты — высота ячейки 0).
        let span_collapsed = frame.init.collapsed_rows[start_flat.min(end_flat)..end_flat].iter().any(|&c| c);
        let last_visible = (start_flat..end_flat).rev().find(|&k| !frame.init.collapsed_rows[k]);
        let target_bottom = match last_visible {
            Some(k) => frame.init.flat_row_rects[k].0 + frame.init.flat_row_rects[k].1,
            None => frame.init.flat_row_rects[start_flat.min(end_flat - 1)].0,
        };
        let cell = match group {
            None => &mut frame.b.children[row].children[child_idx],
            Some(g) => &mut frame.b.children[g].children[row].children[child_idx],
        };
        let grown = if span_collapsed {
            (target_bottom - cell.rect.y).max(0.0)
        } else {
            (target_bottom - cell.rect.y).max(cell.rect.height)
        };
        // §17.5.3: ячейка, растянутая на несколько строк, выравнивает содержимое по
        // всей высоте (`baseline` для неё равносильно `top`).
        let dy = super::table_valign::free_space_shift(cell, grown, super::table_valign::cell_block_align(&cell.style));
        super::table_valign::shift_cell_content(cell, dy);
        cell.rect.height = grown;
        if dy.abs() > 1e-4 && matches!(super::table_valign::cell_block_align(&cell.style), VerticalAlign::Middle | VerticalAlign::Bottom) {
            frame.init.cell_dy.push((group, row, child_idx, dy));
        }
    }

    let s = Arc::clone(&frame.init.s);

    if frame.init.collapse && s.width.is_none() && frame.init.n_cols > 0 {
        frame.b.rect.width = frame.init.collapse_width;
    }

    if frame.init.collapse && frame.init.n_cols > 0 {
        widen_for_outer_group_borders(frame);
    }

    // CSS 2.1 §17.4 — `caption-side: bottom` captions stack below the table's border box, i.e.
    // below the last row plus the table's bottom padding and border. Collapse mode ends at the
    // last row's bottom edge (the border is shared with the cells, padding does not apply);
    // separate mode at `cur_y` (which already includes the trailing `border-spacing` slot).
    let mut bottom_caption_h = 0.0_f32;
    if !frame.init.bottom_captions.is_empty() {
        let grid_bottom = if frame.init.collapse {
            frame.init.flat_row_rects.last().map_or(frame.init.cur_y, |&(y, h)| y + h)
        } else {
            frame.init.cur_y + frame.init.padding_bottom + s.border_bottom_width
        };
        let (x, w, pcb) = (frame.b.rect.x, frame.b.rect.width, frame.init.children_pcb);
        for k in 0..frame.init.bottom_captions.len() {
            let idx = frame.init.bottom_captions[k];
            let y = grid_bottom + bottom_caption_h;
            bottom_caption_h +=
                super::table::lay_out_caption(&mut frame.b.children[idx], x, y, w, measurer, viewport, pcb, hp);
        }
    }
    frame.b.table_caption_inset = [frame.init.top_caption_h, bottom_caption_h];

    place_column_boxes(frame);
    mirror_columns_rtl(frame);

    // Естественная высота (border box) — по строкам и подписям; `height`/`min-height` её
    // только поднимают (CSS 2.1 §17.5.3), а излишек делится между строками.
    let chrome = frame.init.padding_top + frame.init.padding_bottom + s.border_top_width + s.border_bottom_width;
    let natural = if frame.init.collapse {
        frame
            .init
            .flat_row_rects
            .last()
            .map(|&(y, h)| (y + h - frame.b.rect.y).max(0.0))
            .unwrap_or(frame.init.cur_y - frame.b.rect.y)
            + bottom_caption_h
    } else {
        (frame.init.cur_y - frame.init.content_y).max(0.0) + bottom_caption_h + chrome
    };
    let target = super::table_height::target_height(&frame.init, natural, viewport);
    frame.b.rect.height = target;
    if target - natural > 0.01 {
        super::table_height::distribute(&mut frame.b, &frame.init, target - natural);
    }

    // CSS 2.1 §17 / Position L3 §3: an absolutely positioned child of a table box is not a table
    // part — it is out of flow and placed against its containing block once the table's own
    // size is final (BUG-1357). Its static position is the table's content-box corner.
    let abs: Vec<(usize, f32, f32)> = frame
        .b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| (i, frame.init.content_x, frame.init.content_y))
        .collect();
    if !abs.is_empty() {
        let my_pcb = if super::multicol_abspos::establishes_abs_cb(&s) {
            Rect::new(
                frame.b.rect.x + s.border_left_width,
                frame.b.rect.y + s.border_top_width,
                (frame.b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (frame.b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            )
        } else {
            frame.init.children_pcb
        };
        super::multicol_abspos::lay_out_abs_children(&mut frame.b, &abs, measurer, viewport, my_pcb, hp);
    }
}

/// CSS 2.1 §17.5.1 / Writing Modes L3 §7: у горизонтальной таблицы с `direction: rtl` колонки идут
/// справа налево. Сетка колонок посчитана слева направо, поэтому готовые ячейки (вместе с
/// потомками) и боксы колонок зеркалятся внутри полосы столбцов; рамки ячеек остаются физическими.
fn mirror_columns_rtl(frame: &mut Frame) {
    if frame.init.s.direction != crate::style::Direction::Rtl
        || !matches!(frame.init.s.writing_mode, crate::style::WritingMode::HorizontalTb)
        || frame.init.n_cols == 0
    {
        return;
    }
    let init = &frame.init;
    let (lo, hi) = if init.collapse {
        let mut ext: Option<(f32, f32)> = None;
        for_each_cell(&frame.b, &mut |c| {
            let (l, r) = (c.rect.x, c.rect.x + c.rect.width);
            ext = Some(ext.map_or((l, r), |(a, b)| (a.min(l), b.max(r))));
        });
        match ext {
            Some(e) => e,
            None => return,
        }
    } else {
        let lo = init.content_x + init.h_spacing;
        let n = init.col_widths.len();
        (lo, lo + init.col_widths.iter().sum::<f32>() + n.saturating_sub(1) as f32 * init.h_spacing)
    };
    let mirror = |x: f32, w: f32| lo + hi - x - w;
    for_each_cell_mut(&mut frame.b, &mut |c| {
        let dx = mirror(c.rect.x, c.rect.width) - c.rect.x;
        super::shift_tree(c, dx, 0.0);
    });
    for child in &mut frame.b.children {
        if matches!(child.style.display, Display::TableColumn | Display::TableColumnGroup) {
            child.rect.x = mirror(child.rect.x, child.rect.width);
            for col in &mut child.children {
                col.rect.x = mirror(col.rect.x, col.rect.width);
            }
        }
    }
}

fn for_each_cell(b: &LayoutBox, f: &mut dyn FnMut(&LayoutBox)) {
    for child in &b.children {
        match child.kind {
            BoxKind::TableRow => child.children.iter().filter(|c| is_cell(c)).for_each(&mut *f),
            BoxKind::TableRowGroup => for_each_cell(child, f),
            _ => {}
        }
    }
}

fn for_each_cell_mut(b: &mut LayoutBox, f: &mut dyn FnMut(&mut LayoutBox)) {
    for child in &mut b.children {
        match child.kind {
            BoxKind::TableRow => child.children.iter_mut().filter(|c| is_cell(c)).for_each(&mut *f),
            BoxKind::TableRowGroup => for_each_cell_mut(child, f),
            _ => {}
        }
    }
}

fn is_cell(b: &LayoutBox) -> bool {
    b.style.display == Display::TableCell
}

/// CSS 2.1 §17.5.1 — `display: table-column(-group)` не раскладываются как блоки: их прямоугольник —
/// «покрытые колонки × высота всех строк». Нужен только для фона и рамки, которые рисуются под
/// группами строк и ячейками (колонки идут в дереве раньше строк).
fn place_column_boxes(frame: &mut Frame) {
    let n_cols = frame.init.col_widths.len();
    let (Some(top), Some(bottom)) = (
        frame.init.flat_row_rects.iter().map(|&(y, _)| y).reduce(f32::min),
        frame.init.flat_row_rects.iter().map(|&(y, h)| y + h).reduce(f32::max),
    ) else {
        return;
    };
    let init = &frame.init;
    let col_x = |k: usize| -> f32 {
        if let Some(cx) = &init.collapse_col_x {
            cx.get(k).copied().unwrap_or(init.content_x)
        } else {
            init.content_x + (k + 1) as f32 * init.h_spacing + init.col_widths[..k.min(n_cols)].iter().sum::<f32>()
        }
    };
    // Прямоугольник по полуинтервалу колонок [a, b).
    let span_rect = |a: usize, b: usize| -> Option<Rect> {
        let (a, b) = (a.min(n_cols), b.min(n_cols));
        if a >= b {
            return None;
        }
        let x = col_x(a);
        let right = col_x(b - 1) + init.col_widths[b - 1];
        Some(Rect { x, y: top, width: (right - x).max(0.0), height: (bottom - top).max(0.0) })
    };
    let mut next = 0usize;
    for child in &mut frame.b.children {
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
