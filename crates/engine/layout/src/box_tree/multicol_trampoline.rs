use super::*;
use super::layout_cache::finalize_block_height;
use super::layout_dispatch::{dispatch_box, finish_after_match};
use super::block_flow_trampoline::{self, DispatchOutcome};
use super::multicol_fragmentation::{balanced_height, forced_breaks, item_lines, pack, ItemLines};

/// One segment of flow children between `column-span: all` boundaries — CSS
/// Multicol §3.4. `sliceable` is decided up front by `multicol_abspos::
/// build_multicol_init` (a pure function of each item's `style`/`kind`, see
/// `box_is_column_sliceable`), so [`run`] never needs to re-derive it from a
/// laid-out `.rect`.
pub(super) struct SegmentInit {
    /// Indices into `MulticolInit::work` — regular (non-span) children of
    /// this segment, source order.
    pub(super) item_idxs: Vec<usize>,
    /// Index into `MulticolInit::work` of the trailing `column-span: all`
    /// element that closes this segment, if any.
    pub(super) span_idx: Option<usize>,
    /// Whether every item in `item_idxs` can be geometrically sliced across
    /// columns (CSS Multicol §3.4) instead of placed atomically.
    pub(super) sliceable: bool,
    /// The segment is one plain grid container that is cut across the columns as a grid
    /// (`multicol_abspos::is_fragmentable_grid`), see [`emit_grid_fragments`].
    pub(super) grid_frag: bool,
}

/// CSS Multicol L2 §4.2 / §4.4 — rows of column boxes: a non-`auto` `column-height` with
/// `column-wrap` other than `nowrap`. Content past `n_cols` columns of `col_h` opens a new
/// row in the block direction (`row_gap` below the previous one) instead of an overflow
/// column in the inline direction, and a row keeps `col_h` even when only partly filled.
#[derive(Clone, Copy)]
pub(super) struct ColRows {
    pub(super) col_h: f32,
    pub(super) row_gap: f32,
}

impl ColRows {
    /// Distance between the tops of two consecutive rows.
    fn pitch(self) -> f32 {
        self.col_h + self.row_gap
    }

    /// Block extent of `rows` rows, `row_gap` between them.
    fn extent(self, rows: usize) -> f32 {
        rows as f32 * self.col_h + rows.saturating_sub(1) as f32 * self.row_gap
    }
}

/// Loop-entry state for the multicol dispatch arm's per-segment placement
/// pass (CSS Multicol §3.4) — everything the removed inline function
/// (pre-LAYOUT-2-срез-7 `multicol_abspos.rs`'s `lay_out_multicol_children`)
/// read or wrote across segments/items, plus what `layout_dispatch.rs`'s
/// multicol branch used to do with its return value once it came back
/// (container height, `finish_after_match`). Captured by `multicol_abspos::
/// build_multicol_init` before any item is dispatched — column count/width
/// and the segment split (including each segment's slice-vs-atomic decision)
/// still run natively there; see its doc comment for why (none of it ever
/// calls `lay_out` on a child).
///
/// Unlike `FlexInit`'s Step 1 probe, EVERY segment's measure pass below
/// (`Phase::Measure`) unconditionally dispatches every item — there is no
/// skippable-probe shortcut here — so it is captured into this state machine
/// the same as the real-placement pass, not left native.
pub(super) struct MulticolInit {
    pub(super) content_x: f32,
    pub(super) content_y: f32,
    pub(super) content_width: f32,
    pub(super) col_gap: f32,
    pub(super) n_cols: u32,
    pub(super) col_w: f32,
    pub(super) balance: bool,
    pub(super) container_h: Option<f32>,
    /// `container_h` is the container's block size (not a `column-height`): every segment gets
    /// only what the segments and spanners above it left of it.
    pub(super) limit_shared: bool,
    /// `Some` when overflow columns wrap into rows (see [`ColRows`]).
    pub(super) col_rows: Option<ColRows>,
    pub(super) segments: Vec<SegmentInit>,
    pub(super) children_pcb: Rect,
    // Phase-epilogue inputs (`finish_frame` only) — ride along unchanged from
    // `build_multicol_init`, same as `GridInit`'s equivalent fields.
    pub(super) s: Arc<ComputedStyle>,
    pub(super) em: f32,
    /// `dispatch_box`'s own `available_width` parameter — distinct from
    /// `content_width` above (the content-box width column sizing resolves
    /// against). Threaded through only for `finish_after_match`'s `cb` arg.
    pub(super) cb: f32,
    pub(super) is_positioned: bool,
    /// `dispatch_box`'s own `pcb` parameter — this container's positioned
    /// containing block, distinct from `children_pcb` (the CB its own
    /// children resolve against). Used only by `finish_after_match`.
    pub(super) own_pcb: Rect,
    pub(super) padding_top: f32,
    pub(super) padding_bottom: f32,
    pub(super) size_contained: bool,
    pub(super) field_intrinsic: Option<(f32, f32)>,
    pub(super) available_height: Option<f32>,
    // Running state, mutated by `run`'s driver and the `post_*`/`finish_*` helpers.
    pub(super) work: Vec<LayoutBox>,
    pub(super) consumed: Vec<bool>,
    pub(super) out: Vec<LayoutBox>,
    pub(super) cur_y: f32,
    /// With [`ColRows`]: block extent already used in the current row (a balanced segment plus
    /// the `column-span: all` elements after it) — the next segment's first row only has the rest
    /// of `column-height` (Multicol L2 §4.4: a spanner splits the row, the columns after it fill
    /// what is left, and a spanner that uses the whole row starts a new one).
    pub(super) row_used: Option<f32>,
}

/// Which per-segment sub-loop a [`Frame`] is currently driving. `Frame::k`
/// carries the position (Measure/Place: index into the current segment's
/// `item_idxs`; Span: the `work` index of the span element directly — Span
/// never iterates a list, so repurposing `k` for the element's own index
/// avoids an `Option::unwrap` at every read site).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// First pass, always run for every non-empty segment (CSS Multicol
    /// §3.4) — dispatch each item at `(0, 0)` to measure its outer height.
    Measure,
    /// Second pass, atomic segments only — dispatch each item again at its
    /// resolved column position (skipped entirely for sliceable segments,
    /// which fragment geometrically from the Measure pass's results instead).
    Place,
    /// The segment's trailing `column-span: all` element, if any.
    Span,
}

/// One level of the explicit stack `run` maintains in place of the native
/// call stack — the multicol-container analogue of `grid_trampoline::Frame`.
/// `b` is owned (taken via `block_flow_trampoline::take_box`) for the
/// duration of this container's own segment/item placement loop.
struct Frame {
    b: LayoutBox,
    init: Box<MulticolInit>,
    seg_i: usize,
    phase: Phase,
    k: usize,
    /// Outer (margin-box) height of each item measured so far in the current
    /// segment's Measure pass, parallel to `segments[seg_i].item_idxs`.
    outer_hs: Vec<f32>,
    /// Atomic-path scratch, populated once Measure completes for a
    /// non-sliceable segment — column index per position in `item_idxs`.
    col_assignment: Vec<usize>,
    /// Atomic-path scratch — running content-bottom per column, indexed by
    /// column number (not by item).
    col_y: Vec<f32>,
}

enum StepOutcome {
    /// This item/element is fully placed (synchronously, or via the block-
    /// flow/flex/grid/table trampoline) — move on to the next.
    Advance,
    /// This item is itself a multicol container with its own segments to
    /// place — push the current frame and continue processing this one.
    Descend(Box<MulticolInit>),
}

fn new_frame(b: LayoutBox, init: Box<MulticolInit>) -> Frame {
    let mut frame = Frame {
        b,
        init,
        seg_i: 0,
        phase: Phase::Span,
        k: 0,
        outer_hs: Vec::new(),
        col_assignment: Vec::new(),
        col_y: Vec::new(),
    };
    enter_segment(&mut frame);
    frame
}

/// Resets per-segment scratch and picks the first phase for
/// `frame.init.segments[frame.seg_i]` — `Phase::Measure` for a segment with
/// regular items, `Phase::Span` (skipping straight past an empty item list)
/// for a segment that is only a `column-span: all` element, or nothing left
/// to do (`run`'s top-of-loop check handles `seg_i >= segments.len()`).
fn enter_segment(frame: &mut Frame) {
    frame.outer_hs = Vec::new();
    frame.col_assignment = Vec::new();
    frame.col_y = Vec::new();
    frame.k = 0;
    if frame.seg_i >= frame.init.segments.len() {
        return;
    }
    if frame.init.segments[frame.seg_i].item_idxs.is_empty() {
        enter_span_phase(frame);
    } else {
        frame.phase = Phase::Measure;
    }
}

fn enter_span_phase(frame: &mut Frame) {
    frame.phase = Phase::Span;
    frame.k = frame.init.segments[frame.seg_i].span_idx.unwrap_or(0);
}

/// Advances past the item/element `frame` just finished — copied from
/// immediately after the removed loop's recursive calls. Shared by the
/// synchronous dispatch paths and `run`'s resume-after-descend path.
fn advance(frame: &mut Frame) {
    match frame.phase {
        Phase::Measure | Phase::Place => frame.k += 1,
        Phase::Span => {
            frame.seg_i += 1;
            enter_segment(frame);
        }
    }
}

/// The `work` index this frame is currently dispatching (or, right after a
/// resumed descent, just finished) — `item_idxs[k]` for Measure/Place,
/// `k` itself for Span (see [`Phase`]'s doc comment).
fn pending_index(frame: &Frame) -> usize {
    match frame.phase {
        Phase::Measure | Phase::Place => frame.init.segments[frame.seg_i].item_idxs[frame.k],
        Phase::Span => frame.k,
    }
}

/// Dispatches one child at `(x, y)`/`w` — copied from the removed code's
/// `lay_out(&mut work[i], x, y, w, None, measurer, viewport, pcb, hp, false)`
/// call (all three call sites: Measure pass, atomic Place pass, `column-span:
/// all` element), except it calls `dispatch_box` directly instead of the
/// `lay_out` wrapper — the same trade-off every other LAYOUT-2 trampoline
/// makes (bypassing `lay_out_cache_checked`'s BUG-341 in-place/layout-result
/// caches for per-item dispatch; see `grid_trampoline::step_probe_item` for
/// the precedent). A same-kind (multicol) child hands back its `MulticolInit`
/// for `run` to push and descend into; every other kind resolves via its own
/// trampoline synchronously (one native frame per kind transition, the same
/// composition every LAYOUT-2 slice has settled for).
#[allow(clippy::too_many_arguments)]
fn dispatch_child(
    child: &mut LayoutBox,
    x: f32,
    y: f32,
    w: f32,
    pcb: Rect,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> StepOutcome {
    match dispatch_box(
        child, x, y, w, None, measurer, viewport, pcb, hp, false, None, AlignValue::Auto, None,
    ) {
        DispatchOutcome::Done => StepOutcome::Advance,
        DispatchOutcome::NeedsBlockFlowLoop(ci) => {
            block_flow_trampoline::run(child, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsFlexLoop(ci) => {
            super::flex_trampoline::run(child, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsGridLoop(ci) => {
            super::grid_trampoline::run(child, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsTableLoop(ci) => {
            super::table_trampoline::run(child, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsMulticolLoop(ci) => StepOutcome::Descend(ci),
        // LAYOUT-2 срез 8: same shape, for a child that is itself a vertical
        // writing-mode container.
        DispatchOutcome::NeedsVerticalLoop(ci) => {
            super::vertical_trampoline::run(child, ci, measurer, viewport, hp);
            StepOutcome::Advance
        }
    }
}

/// Drives `init`'s per-segment Measure/Place/Span passes (and every further
/// multicol-container descendant either meets) on an explicit heap stack, so
/// a chain of nested multicol containers no longer grows the native call
/// stack one frame per level (LAYOUT-2's acceptance criterion, applied to
/// item (5) of its ROADMAP entry). `b` is the box `dispatch_box`'s multicol
/// arm was originally called on.
pub(super) fn run(
    b: &mut LayoutBox,
    init: Box<MulticolInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let mut current = new_frame(block_flow_trampoline::take_box(b), init);
    let mut stack: Vec<Frame> = Vec::new();

    loop {
        if current.seg_i >= current.init.segments.len() {
            finish_frame(&mut current, measurer, viewport, hp);
            match stack.pop() {
                None => {
                    *b = current.b;
                    return;
                }
                Some(mut parent) => {
                    let i = pending_index(&parent);
                    parent.init.work[i] = current.b;
                    match parent.phase {
                        Phase::Measure => post_measure_item(&mut parent, i, viewport),
                        Phase::Place => post_place_item(&mut parent, i, viewport),
                        Phase::Span => post_span_item(&mut parent, i, viewport),
                    }
                    advance(&mut parent);
                    current = parent;
                }
            }
            continue;
        }

        match current.phase {
            Phase::Measure => {
                let seg_len = current.init.segments[current.seg_i].item_idxs.len();
                if current.k >= seg_len {
                    finish_measure_phase(&mut current, viewport);
                    continue;
                }
                let i = pending_index(&current);
                let pcb = current.init.children_pcb;
                let col_w = current.init.col_w;
                match dispatch_child(&mut current.init.work[i], 0.0, 0.0, col_w, pcb, measurer, viewport, hp) {
                    StepOutcome::Advance => {
                        post_measure_item(&mut current, i, viewport);
                        advance(&mut current);
                    }
                    StepOutcome::Descend(child_init) => {
                        let child_box = block_flow_trampoline::take_box(&mut current.init.work[i]);
                        stack.push(current);
                        current = new_frame(child_box, child_init);
                    }
                }
            }
            Phase::Place => {
                let seg_len = current.init.segments[current.seg_i].item_idxs.len();
                if current.k >= seg_len {
                    finish_place_phase(&mut current);
                    continue;
                }
                let i = pending_index(&current);
                let col = current.col_assignment[current.k];
                let col_w = current.init.col_w;
                // With rows the fragment number counts across rows; its column is the remainder.
                let col_in_row = if current.init.col_rows.is_some() { col % current.init.n_cols as usize } else { col };
                let col_x = current.init.content_x + col_in_row as f32 * (col_w + current.init.col_gap);
                let col_y = current.col_y[col];
                let pcb = current.init.children_pcb;
                match dispatch_child(&mut current.init.work[i], col_x, col_y, col_w, pcb, measurer, viewport, hp) {
                    StepOutcome::Advance => {
                        post_place_item(&mut current, i, viewport);
                        advance(&mut current);
                    }
                    StepOutcome::Descend(child_init) => {
                        let child_box = block_flow_trampoline::take_box(&mut current.init.work[i]);
                        stack.push(current);
                        current = new_frame(child_box, child_init);
                    }
                }
            }
            Phase::Span => {
                match current.init.segments[current.seg_i].span_idx {
                    None => advance(&mut current),
                    Some(span_i) => {
                        let content_x = current.init.content_x;
                        let content_width = current.init.content_width;
                        let cur_y = current.init.cur_y;
                        let pcb = current.init.children_pcb;
                        match dispatch_child(
                            &mut current.init.work[span_i], content_x, cur_y, content_width, pcb,
                            measurer, viewport, hp,
                        ) {
                            StepOutcome::Advance => {
                                post_span_item(&mut current, span_i, viewport);
                                advance(&mut current);
                            }
                            StepOutcome::Descend(child_init) => {
                                let child_box = block_flow_trampoline::take_box(&mut current.init.work[span_i]);
                                stack.push(current);
                                current = new_frame(child_box, child_init);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Runs right after an item finishes its Measure-phase dispatch — outer
/// (margin-box) height bookkeeping, copied from the removed code's
/// `outer_hs` collection. Shared by the synchronous path and `run`'s
/// resume-after-descend path.
fn post_measure_item(frame: &mut Frame, i: usize, viewport: Size) {
    let col_w = frame.init.col_w;
    let c = &frame.init.work[i];
    let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
    let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
    frame.outer_hs.push(mt + c.rect.height + mb);
}

/// Runs once every item of a segment has been measured — CSS Multicol §3.4:
/// a sliceable segment fragments geometrically from the measured heights
/// (no further dispatch), an atomic segment computes its column assignment
/// and enters the real-placement `Phase::Place`. Copied from the removed
/// code's `if all_sliceable { .. } else { .. }` split.
fn finish_measure_phase(frame: &mut Frame, viewport: Size) {
    if frame.init.segments[frame.seg_i].grid_frag && emit_grid_fragments(frame) {
        enter_span_phase(frame);
    } else if frame.init.segments[frame.seg_i].sliceable {
        emit_sliced_fragments(frame, viewport);
        enter_span_phase(frame);
    } else {
        compute_col_assignment(frame);
        frame.phase = Phase::Place;
        frame.k = 0;
    }
}

/// Column height limit of the current segment (see [`MulticolInit::limit_shared`]).
fn segment_limit(init: &MulticolInit) -> Option<f32> {
    let l = init.container_h?;
    if !init.limit_shared {
        return Some(l);
    }
    // A segment below a spanner has the rest of the container's height; once that is used up
    // the full limit stays (a column of a few pixels would only split the content into
    // hundreds of columns).
    let rest = l - (init.cur_y - init.content_y);
    Some(if rest >= 1.0 { rest } else { l })
}

/// CSS Multicol L1 §7 — used column height for a sliceable segment.
/// `limit` is the column height limit (definite `height`, else `max-height`).
/// `balance` spreads `total_h` over `n_cols` and never exceeds the limit;
/// `column-fill: auto` fills up to the limit, or — with no limit — keeps the
/// whole segment in one column.
fn column_height(balance: bool, limit: Option<f32>, total_h: f32, n_cols: usize) -> f32 {
    let h = if balance {
        let even = (total_h / n_cols as f32).ceil();
        limit.map_or(even, |l| even.min(l))
    } else {
        limit.unwrap_or(total_h)
    };
    h.max(1.0)
}

/// Number of columns needed for `total_h` of content at column height
/// `col_h` — never fewer than `column-count`; more only when the height limit
/// forces overflow columns.
fn overflow_columns(total_h: f32, col_h: f32, n_cols: usize) -> usize {
    let needed = ((total_h - 0.01) / col_h).ceil().max(1.0) as usize;
    needed.max(n_cols)
}

/// CSS Multicol §3.4 — geometric column slicing for a sliceable segment,
/// copied verbatim from the removed code's `all_sliceable` branch. Pure
/// post-processing of the Measure pass's results — every item here is a
/// leaf (`box_is_column_sliceable` requires `children.is_empty()`), so none
/// of this ever dispatches again.
fn emit_sliced_fragments(frame: &mut Frame, viewport: Size) {
    let seg_i = frame.seg_i;
    let n_cols = frame.init.n_cols;
    let col_gap = frame.init.col_gap;
    let col_w = frame.init.col_w;
    let content_x = frame.init.content_x;
    let balance = frame.init.balance;
    let container_h = segment_limit(&frame.init);
    let rows_cfg = frame.init.col_rows;
    // The rest of a row cut by a spanner; a row that is used up opens the next one.
    let mut row_rest = None;
    if let (Some(r), Some(used)) = (rows_cfg, frame.init.row_used.take()) {
        let rest = r.col_h - used;
        if rest < 1.0 {
            frame.init.cur_y += r.row_gap;
        } else {
            row_rest = Some(rest);
        }
    }
    let cur_y = frame.init.cur_y;
    let item_idxs = frame.init.segments[seg_i].item_idxs.clone();
    let outer_hs = frame.outer_hs.clone();

    let total_h: f32 = outer_hs.iter().sum();
    let n = n_cols as usize;
    let followed_by_span = frame.init.segments[seg_i].span_idx.is_some();
    // Height of the first row of the segment and of the later ones. A segment that continues a
    // row cut by a spanner starts with the rest of that row; a segment closed by a spanner whose
    // content fits one row is balanced (Multicol L1 §7: columns before a spanner are balanced).
    let (first_h, later_h) = match rows_cfg {
        Some(r) => {
            let avail = row_rest.map_or(r.col_h, |rest| rest.min(r.col_h)).max(1.0);
            if followed_by_span && total_h <= n as f32 * avail + 0.01 {
                (((total_h / n as f32).ceil()).clamp(1.0, avail), r.col_h.max(1.0))
            } else {
                (avail, r.col_h.max(1.0))
            }
        }
        None => {
            let h = column_height(balance, container_h, total_h, n);
            (h, h)
        }
    };

    // Virtual single-column stack: each box's border-box occupies
    // [virtual_top, virtual_top + height), with margins as gaps.
    let mut stack: Vec<(usize, f32, f32)> = Vec::with_capacity(item_idxs.len());
    let mut v = 0.0f32;
    for (&i, &oh) in item_idxs.iter().zip(outer_hs.iter()) {
        let c = &frame.init.work[i];
        let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
        stack.push((i, v + mt, c.rect.height));
        v += oh;
    }

    // Emit one clipped fragment per (column, box) overlap. Content that does
    // not fit into `n_cols` columns of `col_h` flows into overflow columns
    // laid out further along the inline axis (CSS Multicol L1 §7.1).
    // With rows (L2 §4.4) the fragments are numbered across rows: fragment `f` is column
    // `f % n_cols` of row `f / n_cols`, and the last row is completed to `n_cols` columns.
    let (used_cols, rows_used) = match rows_cfg {
        Some(_) => {
            let extra = ((total_h - n as f32 * first_h - 0.01) / later_h).ceil().max(0.0) as usize;
            let rows = 1 + extra.div_ceil(n);
            (rows * n, rows)
        }
        None => (overflow_columns(total_h, first_h, n), 1),
    };
    let mut seg_extent = 0.0f32;
    for c in 0..used_cols {
        // Virtual extent `[col_lo, col_hi)` of fragment `c`.
        let (col_lo, col_h_c) = if c < n {
            (c as f32 * first_h, first_h)
        } else if rows_cfg.is_some() {
            (n as f32 * first_h + (c - n) as f32 * later_h, later_h)
        } else {
            (c as f32 * first_h, first_h)
        };
        let col_hi = col_lo + col_h_c;
        let (col_in_row, row) = match rows_cfg {
            Some(_) => (c % n, c / n),
            None => (c, 0),
        };
        let col_x = content_x + col_in_row as f32 * (col_w + col_gap);
        let row_y = match rows_cfg {
            Some(r) if row > 0 => first_h + r.row_gap + (row - 1) as f32 * r.pitch(),
            _ => 0.0,
        };
        for &(i, bt, bh) in &stack {
            let bb = bt + bh;
            let ov_lo = bt.max(col_lo);
            let ov_hi = bb.min(col_hi);
            if ov_hi > ov_lo {
                let mut frag = frame.init.work[i].clone();
                frag.rect.x = col_x;
                frag.rect.y = cur_y + row_y + (ov_lo - col_lo);
                frag.rect.width = col_w;
                frag.rect.height = ov_hi - ov_lo;
                seg_extent = seg_extent.max(ov_hi - col_lo);
                frame.init.out.push(frag);
            }
        }
    }
    for &i in &item_idxs {
        frame.init.consumed[i] = true;
    }
    if let Some(r) = rows_cfg {
        seg_extent = first_h + (rows_used - 1) as f32 * (r.row_gap + later_h);
        // A balanced row followed by a spanner leaves the rest of its `column-height` to the
        // columns after the spanner.
        if rows_used == 1 && followed_by_span {
            frame.init.row_used = Some(r.col_h - row_rest.unwrap_or(r.col_h) + first_h);
        }
    }
    frame.init.cur_y += seg_extent.max(0.0);
}

/// `true` when a flow item of the grid runs across the whole row gap `[gap_lo, gap_hi)` (its block
/// extent starts before the gap and ends after it), measured from the grid's top edge `gy`.
fn gap_is_bridged(grid: &LayoutBox, gy: f32, gap_lo: f32, gap_hi: f32) -> bool {
    grid.children.iter().any(|c| {
        !matches!(c.kind, BoxKind::Skip)
            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
            && c.rect.y - gy < gap_lo - 0.01
            && c.rect.y - gy + c.rect.height > gap_hi + 0.01
    })
}

/// CSS Fragmentation L3 §5 / CSS Gap Decorations L1 §6.2 — a grid container laid out in full
/// (the Measure pass) is cut into one fragment per column of the multicol container
/// (`column-fill: auto`, a definite column height). A break that falls inside a row gap drops
/// the rest of that gap: the next fragment starts at the following track; a break inside a track
/// splits it (the track continues at the top of the next fragment). Each fragment keeps the parts
/// of the children that fall in its window (`box_tree` leaves — cutting one repeats nothing) and
/// the row tracks clipped to the window (`SubgridTracks::fragment`), so the painter draws a row
/// gap only between two tracks of the same fragment (a gap split by the break, or the last
/// content before it, is suppressed) and runs the column gaps over the fragment's height.
/// Returns `false` (nothing changed) when the box has no row tracks to cut by, and the segment
/// falls back to the atomic path.
fn emit_grid_fragments(frame: &mut Frame) -> bool {
    let seg_i = frame.seg_i;
    let i = frame.init.segments[seg_i].item_idxs[0];
    let Some(limit) = segment_limit(&frame.init) else { return false };
    let grid = &frame.init.work[i];
    let Some(rows) = grid.subgrid_tracks.as_ref().and_then(|t| t.rows.clone()) else { return false };
    if rows.len() < 2 || (frame.outer_hs[0] - grid.rect.height).abs() > 0.01 || limit < 1.0 {
        return false;
    }
    let total_h = grid.rect.height;
    // `column-fill: balance` shrinks the columns below the height limit when the content is
    // shorter than `limit × columns`; only a container that fills them all is cut at the limit.
    if frame.init.balance && total_h < limit * frame.init.n_cols as f32 - 0.01 {
        return false;
    }
    // Windows `[start, end)` in the grid's own (unfragmented) block coordinates.
    let mut windows: Vec<(f32, f32)> = Vec::new();
    let mut start = 0.0f32;
    loop {
        let end = start + limit;
        if end >= total_h - 0.01 {
            windows.push((start, total_h));
            break;
        }
        windows.push((start, end));
        // A break inside a row gap drops the rest of the gap (Fragmentation L3 §5.1: a gap adjoining
        // a break is truncated); so does one at the very start edge of the gap, unless an item runs
        // through it (Chrome keeps such a gap: `grid-gap-decorations-fragmentation-029`). Inside a
        // track the track continues.
        let in_gap = rows.windows(2).find(|w| {
            let inside = end > w[0].1 + 0.01 && end < w[1].0 - 0.01;
            let at_start = (end - w[0].1).abs() <= 0.01 && end < w[1].0 - 0.01;
            inside || (at_start && !gap_is_bridged(grid, grid.rect.y, w[0].1, w[1].0))
        });
        start = in_gap.map_or(end, |w| w[1].0);
        if start >= total_h - 0.01 {
            break;
        }
    }
    let (col_w, col_gap, content_x, cur_y) =
        (frame.init.col_w, frame.init.col_gap, frame.init.content_x, frame.init.cur_y);
    let (gx, gy) = (grid.rect.x, grid.rect.y);
    let mut seg_extent = 0.0f32;
    let mut out = Vec::with_capacity(windows.len());
    for (f, &(ws, we)) in windows.iter().enumerate() {
        let col_x = content_x + f as f32 * (col_w + col_gap);
        let mut frag = grid.clone();
        frag.rect.x = col_x;
        frag.rect.y = cur_y;
        frag.rect.width = col_w;
        frag.rect.height = we - ws;
        frag.children = grid
            .children
            .iter()
            .filter_map(|c| {
                let flow = !matches!(c.kind, BoxKind::Skip)
                    && !matches!(c.style.position, Position::Absolute | Position::Fixed);
                if !flow {
                    return (f == 0).then(|| {
                        let mut k = c.clone();
                        super::shift_tree(&mut k, col_x - gx, cur_y - gy);
                        k
                    });
                }
                let (top, bot) = (c.rect.y - gy, c.rect.y - gy + c.rect.height);
                let (lo, hi) = (top.max(ws), bot.min(we));
                // A zero-height item at the very start of a window belongs to it too.
                let inside = hi > lo || (c.rect.height == 0.0 && top >= ws && top < we);
                inside.then(|| {
                    let mut k = c.clone();
                    k.rect.x = col_x + (c.rect.x - gx);
                    k.rect.y = cur_y + (lo - ws);
                    k.rect.height = (hi - lo).max(0.0);
                    k
                })
            })
            .collect();
        let clipped: Vec<(f32, f32)> = rows
            .iter()
            .filter_map(|&(a, b)| {
                let (lo, hi) = (a.max(ws), b.min(we));
                (hi > lo).then_some((lo - ws, hi - ws))
            })
            .collect();
        let cols = grid.subgrid_tracks.as_ref().and_then(|t| t.cols.clone());
        frag.subgrid_tracks =
            Some(Box::new(crate::subgrid::SubgridTracks { cols, rows: Some(clipped), fragment: true }));
        seg_extent = seg_extent.max(we - ws);
        out.push(frag);
    }
    frame.init.out.extend(out);
    frame.init.consumed[i] = true;
    frame.init.cur_y += seg_extent;
    true
}

/// CSS Multicol §3.4 atomic fallback — greedy column assignment by height,
/// copied verbatim from the removed code's `else` branch (balanced target
/// height in balance mode, container height in fill:auto mode). Pure
/// function of the Measure pass's `outer_hs` — seeds `Phase::Place`'s
/// per-column running cursor (`col_y`) at the segment's current `cur_y`.
fn compute_col_assignment(frame: &mut Frame) {
    let n_cols = frame.init.n_cols as usize;
    let balance = frame.init.balance;
    let container_h = segment_limit(&frame.init);
    let cur_y = frame.init.cur_y;
    let outer_hs = &frame.outer_hs;
    let total_h: f32 = outer_hs.iter().sum();
    // CSS Fragmentation L3 §3.3: breaks between line boxes honour `orphans`/`widows`.
    let kinds: Vec<ItemLines> = frame.init.segments[frame.seg_i]
        .item_idxs
        .iter()
        .map(|&i| item_lines(&frame.init.work[i]))
        .collect();
    // CSS Fragmentation L3 §3.1: `break-before`/`break-after: column|always` open a new column.
    let items: Vec<&LayoutBox> = frame.init.segments[frame.seg_i]
        .item_idxs
        .iter()
        .map(|&i| &frame.init.work[i])
        .collect();
    let forced = forced_breaks(&items);
    let (orphans, widows) = (frame.init.s.orphans, frame.init.s.widows);
    let target_h = if let Some(r) = frame.init.col_rows {
        r.col_h.max(1.0)
    } else if balance {
        let balanced = balanced_height(outer_hs, &kinds, &forced, n_cols, orphans, widows);
        container_h.map_or(balanced, |limit| balanced.min(limit.max(1.0)))
    } else {
        column_height(false, container_h, total_h, n_cols)
    };

    // A column holds at least one item before it overflows to the next (CSS Multicol §3.4 —
    // every column box is filled in order, starting from the first); columns past
    // `column-count` are overflow columns (CSS Multicol L1 §7.1).
    let col_assignment = pack(outer_hs, &kinds, &forced, target_h, orphans, widows, false)
        .unwrap_or_else(|| vec![0; outer_hs.len()]);
    let col_count = col_assignment.iter().copied().max().map_or(0, |m| m + 1).max(n_cols);
    frame.col_y = match frame.init.col_rows {
        // A fragment of row `f / n_cols` starts that row's distance below the segment top.
        Some(r) => (0..col_count).map(|f| cur_y + (f / n_cols) as f32 * r.pitch()).collect(),
        None => vec![cur_y; col_count],
    };
    frame.col_assignment = col_assignment;
}

/// Runs right after an item finishes its Place-phase dispatch — per-column
/// running cursor advance, copied from the removed code's atomic-placement
/// loop tail (`col_y[col] = work[i].rect.y + work[i].rect.height + mb`).
fn post_place_item(frame: &mut Frame, i: usize, viewport: Size) {
    let k = frame.k;
    let col = frame.col_assignment[k];
    let col_w = frame.init.col_w;
    let c = &frame.init.work[i];
    let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
    let new_col_y = c.rect.y + c.rect.height + mb;
    let placed = c.clone();
    frame.col_y[col] = new_col_y;
    frame.init.out.push(placed);
    frame.init.consumed[i] = true;
}

/// Runs once every item of an atomic segment has been placed — folds the
/// per-column cursors into `cur_y`, copied verbatim from the removed code's
/// `cur_y = col_y.into_iter().fold(cur_y, f32::max)`.
fn finish_place_phase(frame: &mut Frame) {
    frame.init.cur_y = match frame.init.col_rows {
        // A row keeps `column-height` however little of it the content fills.
        Some(r) => {
            let rows = frame.col_y.len().div_ceil(frame.init.n_cols as usize).max(1);
            frame.init.cur_y + r.extent(rows)
        }
        None => frame.col_y.iter().copied().fold(frame.init.cur_y, f32::max),
    };
    enter_span_phase(frame);
}

/// Runs right after a segment's `column-span: all` element finishes its
/// dispatch — advances `cur_y` past it, copied from the removed code's span
/// handling tail.
fn post_span_item(frame: &mut Frame, span_i: usize, viewport: Size) {
    let content_width = frame.init.content_width;
    let c = &frame.init.work[span_i];
    let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, content_width, viewport);
    let new_cur_y = c.rect.y + c.rect.height + mb;
    let placed = c.clone();
    // A spanner after a row of columns eats into that row's `column-height`.
    if let Some(used) = frame.init.row_used {
        frame.init.row_used = Some(used + (new_cur_y - frame.init.cur_y));
    }
    frame.init.cur_y = new_cur_y;
    frame.init.out.push(placed);
    frame.init.consumed[span_i] = true;
}

/// Runs once every segment is placed — (moved in from `layout_dispatch.rs`'s
/// former post-`lay_out_multicol_children` code) rebuilds `b.children` from
/// the placed/fragmented boxes plus any never-consumed non-flow boxes
/// (absolute/fixed, `Skip` placeholders, copied unchanged), then the
/// container's own height (`finalize_block_height`) and the same
/// `finish_after_match` tail (`position: relative` offset, and the container's
/// own absolutely-positioned children, collected into `abs_deferred` here
/// because `build_multicol_init`'s `flow_idxs` filter leaves them out of the
/// column segments) that the removed code fell through to.
fn finish_frame(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let work = std::mem::take(&mut frame.init.work);
    let mut out = std::mem::take(&mut frame.init.out);
    for (i, b) in work.into_iter().enumerate() {
        if !frame.init.consumed[i] {
            out.push(b);
        }
    }
    frame.b.children = out;

    let content_height = frame.init.cur_y - frame.init.content_y;
    finalize_block_height(
        &mut frame.b, &frame.init.s, frame.init.em, frame.init.available_height, viewport,
        frame.init.padding_top, frame.init.padding_bottom, frame.init.size_contained,
        frame.init.field_intrinsic, content_height, frame.init.cb,
    );
    // CSS Positioned Layout L3 §4: the container's own abspos children (never part of a column
    // segment) are placed once its height is final; the static position is the content-box origin.
    let abs_deferred: Vec<(usize, f32, f32)> = frame
        .b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| (i, frame.init.content_x, frame.init.content_y))
        .collect();
    finish_after_match(
        &mut frame.b, &frame.init.s, frame.init.em, frame.init.cb, frame.init.is_positioned,
        frame.init.own_pcb, &abs_deferred, measurer, viewport, hp,
    );
}
