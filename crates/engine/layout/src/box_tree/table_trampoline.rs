use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};

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
    let mut current = Frame { b: block_flow_trampoline::take_box(b), init, t: 0, r: 0, cell: 0, group_row_y: 0.0 };
    let mut stack: Vec<Frame> = Vec::new();

    loop {
        if current.t >= current.init.top_level.len() {
            finish_table(&mut current, viewport);
            match stack.pop() {
                None => {
                    *b = current.b;
                    return;
                }
                Some(mut parent) => {
                    let cell_local_idx = {
                        let ri = row_init_ref(&parent.init.top_level[parent.t], parent.r);
                        ri.cell_idxs[parent.cell]
                    };
                    {
                        let row = row_box_mut(&mut parent.b, &parent.init.top_level, parent.t, parent.r);
                        row.children[cell_local_idx] = current.b;
                    }
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
            finish_row(&mut current, viewport);
            current.r += 1;
            current.cell = 0;
            continue;
        }

        match step_cell(&mut current, measurer, viewport, hp) {
            StepOutcome::Advance => {
                current.cell += 1;
            }
            StepOutcome::Descend(child_init) => {
                let cell_local_idx = {
                    let ri = row_init_ref(&current.init.top_level[current.t], current.r);
                    ri.cell_idxs[current.cell]
                };
                let child_box = {
                    let row = row_box_mut(&mut current.b, &current.init.top_level, current.t, current.r);
                    block_flow_trampoline::take_box(&mut row.children[cell_local_idx])
                };
                let child_frame =
                    Frame { b: child_box, init: child_init, t: 0, r: 0, cell: 0, group_row_y: 0.0 };
                stack.push(current);
                current = child_frame;
            }
        }
    }
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
    let cell_box = &mut row.children[cell_local_idx];
    let saved_width = Arc::make_mut(&mut cell_box.style).width.take();
    let outcome = dispatch_box(
        cell_box, cell_x, row_content_y, avail, None, measurer, viewport, pcb, hp,
        false, None, AlignValue::Auto, None,
    );
    Arc::make_mut(&mut cell_box.style).width = saved_width;
    match outcome {
        DispatchOutcome::Done => StepOutcome::Advance,
        DispatchOutcome::NeedsBlockFlowLoop(ci) => {
            block_flow_trampoline::run(cell_box, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsFlexLoop(ci) => {
            super::flex_trampoline::run(cell_box, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsGridLoop(ci) => {
            super::grid_trampoline::run(cell_box, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsTableLoop(ci) => StepOutcome::Descend(ci),
        // LAYOUT-2 срез 7: a cell whose content is itself a multicol
        // container — same shape as the flex/grid arms above.
        DispatchOutcome::NeedsMulticolLoop(ci) => {
            super::multicol_trampoline::run(cell_box, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
    }
}

/// Runs once every cell of the current row is placed — CSS 2.1 §17.5 Step 4
/// (non-rowspan cells normalised to the row's max height), the row's own
/// border-box height (`s.height` or content-derived), `flat_row_rects`/
/// `span_fixes` bookkeeping, and the row-cursor advance (`TableInit::cur_y`
/// for a direct row, `frame.group_row_y` for a grouped one) — copied from the
/// removed code's per-row epilogue (`lay_out_table_row`'s Step 4 plus
/// `lay_out_table`'s per-row wrapper tail).
fn finish_row(frame: &mut Frame, viewport: Size) {
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

    let row = row_box_mut(&mut frame.b, &frame.init.top_level, t, r);
    let row_h = cell_idxs
        .iter()
        .filter(|&&i| row.children[i].row_span == 1)
        .map(|&i| row.children[i].rect.height)
        .fold(0.0_f32, f32::max);
    for &i in &cell_idxs {
        if row.children[i].row_span == 1 {
            row.children[i].rect.height = row_h;
        }
    }

    let em = row.style.font_size;
    let row_style_h = if let Some(h_len) = &row.style.height
        && let Some(h) = h_len.resolve(em, None, viewport)
    {
        let pt = row.style.padding_top.resolve_or_zero(em, content_width, viewport);
        let pb = row.style.padding_bottom.resolve_or_zero(em, content_width, viewport);
        match row.style.box_sizing {
            BoxSizing::ContentBox => {
                (h + pt + pb + row.style.border_top_width + row.style.border_bottom_width).max(0.0)
            }
            BoxSizing::BorderBox => h.max(pt + pb + row.style.border_top_width + row.style.border_bottom_width),
        }
    } else {
        let pt = row.style.padding_top.resolve_or_zero(em, content_width, viewport);
        let pb = row.style.padding_bottom.resolve_or_zero(em, content_width, viewport);
        row_h + pt + pb + row.style.border_top_width + row.style.border_bottom_width
    };
    row.rect.height = row_style_h;
    let row_rect_y = row.rect.y;

    for &i in &cell_idxs {
        if row.children[i].row_span > 1 {
            frame.init.span_fixes.push((group_opt, row_field, i, flat_idx, row.children[i].row_span));
        }
    }

    let c_mb = row.style.margin_bottom.resolve_or_zero(em, content_width, viewport);
    let next_y = row_rect_y + row_style_h + c_mb + v_spacing - collapse_v_overlap;
    frame.init.flat_row_rects.push((row_rect_y, row_style_h));

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

/// Runs once every top-level entry is placed — CSS 2.1 §17.5 Pass 2 (extend
/// each rowspan cell's height to its last spanned row's bottom edge), then
/// (moved in from `layout_dispatch.rs`'s former post-`lay_out_table` code)
/// the table's own border-box width/height: collapse-mode geometry first
/// (matching the removed `lay_out_table`'s tail), then the explicit-`height`
/// override (either mode), then non-collapse content-derived auto height.
/// Copied from the removed code's tail plus the dispatch arm.
fn finish_table(frame: &mut Frame, viewport: Size) {
    for &(group, row, child_idx, start_flat, span) in &frame.init.span_fixes {
        let end_flat = (start_flat + span as usize).min(frame.init.flat_row_rects.len());
        if end_flat == 0 {
            continue;
        }
        let (last_y, last_h) = frame.init.flat_row_rects[end_flat - 1];
        let target_bottom = last_y + last_h;
        let cell = match group {
            None => &mut frame.b.children[row].children[child_idx],
            Some(g) => &mut frame.b.children[g].children[row].children[child_idx],
        };
        cell.rect.height = (target_bottom - cell.rect.y).max(cell.rect.height);
    }

    let s = Arc::clone(&frame.init.s);

    if frame.init.collapse {
        if s.width.is_none() && frame.init.n_cols > 0 {
            frame.b.rect.width = frame.init.collapse_width;
        }
        if s.height.is_none() {
            frame.b.rect.height = frame
                .init
                .flat_row_rects
                .last()
                .map(|&(y, h)| (y + h - frame.b.rect.y).max(0.0))
                .unwrap_or(0.0);
        }
    }

    if let Some(h_len) = &s.height
        && let Some(h) = resolve_block_size(h_len, frame.init.em, frame.init.available_height, viewport)
    {
        frame.b.rect.height = match s.box_sizing {
            BoxSizing::ContentBox => (h + frame.init.padding_top + frame.init.padding_bottom
                + s.border_top_width + s.border_bottom_width)
                .max(0.0),
            BoxSizing::BorderBox => h.max(
                frame.init.padding_top + frame.init.padding_bottom
                    + s.border_top_width + s.border_bottom_width,
            ),
        };
    } else if !frame.init.collapse {
        let content_height = (frame.init.cur_y - frame.init.content_y).max(0.0);
        frame.b.rect.height = content_height
            + frame.init.padding_top + frame.init.padding_bottom
            + s.border_top_width + s.border_bottom_width;
    }
}
