use super::*;
use super::layout_cache::finalize_block_height;
use super::layout_dispatch::{dispatch_box, finish_after_match};
use super::block_flow_trampoline::{self, DispatchOutcome};
use super::multicol_flow::{self, Window};
use super::multicol_nested::{cut_nested, cut_nested_rows, NestedFlow, NestedSeg};
use super::multicol_fragmentation::{balanced_height, forced_breaks, item_lines, pack, ItemLines};

mod grid_fragments;
use grid_fragments::emit_grid_fragments;

/// One segment of flow children between `column-span: all` boundaries — CSS
/// Multicol §3.4. `sliceable` is decided up front by `multicol_abspos::
/// build_multicol_init` (a pure function of the container's style, see
/// `segment_is_sliceable`), so [`run`] never needs to re-derive it from a
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
    /// `column-wrap: wrap` with an `auto` `column-height` (Multicol L2 §4.4): the row-gap. The
    /// container has no column height to wrap at, so a row opens only where forced column
    /// breaks need more than `column-count` columns; its height is its tallest column.
    pub(super) wrap_auto: Option<f32>,
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
    /// Indices into `out` of the absolutely positioned boxes lifted out of the items and cut
    /// across the columns on their own (`multicol_flow::take_oof`): placed already, so
    /// `finish_frame` does not hand them to `lay_out_abs_children` as the container's own.
    pub(super) oof_out: Vec<usize>,
    /// Column windows of the last segment cut by [`emit_sliced_fragments`], for the container's own
    /// absolutely positioned children (cut in `finish_frame`).
    pub(super) frag_cols: Option<FragCols>,
    /// How many segments were cut by [`emit_sliced_fragments`]; `frag_cols` stands for the
    /// whole container only when it is one.
    pub(super) frag_segments: u32,
    /// Set by the parent multicol container that can cut this one across its own columns
    /// ([`multicol_nested`](super::multicol_nested)): keep the flow of the only segment.
    pub(super) keep_flow: bool,
    /// The flow kept for the parent (`keep_flow`), handed to it when this frame is done.
    pub(super) nested_out: Option<Box<NestedFlow>>,
    /// `(index in work, flow)` of the nested multicol items that kept their flow.
    pub(super) nested: Vec<(usize, Box<NestedFlow>)>,
}

/// Column geometry of a segment cut by [`emit_sliced_fragments`] without rows of columns.
pub(super) struct FragCols {
    /// `(lo, hi)` of every column in the segment's virtual coordinates.
    windows: Vec<(f32, f32)>,
    first_h: f32,
    later_h: f32,
    /// Page `y` of the segment's top edge.
    origin: f32,
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
    /// With [`MulticolInit::wrap_auto`] and more forced columns than `column-count`: the
    /// content-box `y` just past the last row, set by `compute_col_assignment`.
    wrap_end: Option<f32>,
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
        wrap_end: None,
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
    frame.wrap_end = None;
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
                    if let Some(flow) = current.init.nested_out.take()
                        && nested_flow_complete(&flow, &current.init)
                    {
                        parent.init.nested.push((i, flow));
                    }
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
                    StepOutcome::Descend(mut child_init) => {
                        child_init.keep_flow = wants_nested_flow(&current);
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
                let col_in_row = if current.init.col_rows.is_some() || current.wrap_end.is_some() {
                    col % current.init.n_cols as usize
                } else {
                    col
                };
                let col_x = current.init.content_x + col_in_row as f32 * (col_w + current.init.col_gap);
                let col_y = current.col_y[col];
                let pcb = current.init.children_pcb;
                match dispatch_child(&mut current.init.work[i], col_x, col_y, col_w, pcb, measurer, viewport, hp) {
                    StepOutcome::Advance => {
                        post_place_item(&mut current, i, viewport);
                        advance(&mut current);
                    }
                    StepOutcome::Descend(mut child_init) => {
                        child_init.keep_flow = wants_nested_flow(&current);
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

/// Can the segment `frame` is measuring cut a nested multicol item across its own columns
/// ([`emit_nested_fragments`])? Then the item keeps the flow it cuts for it.
fn wants_nested_flow(frame: &Frame) -> bool {
    let init = &frame.init;
    let seg = &init.segments[frame.seg_i];
    frame.phase == Phase::Measure
        && init.container_h.is_some()
        && !init.balance
        && init.col_rows.is_none()
        && seg.sliceable
        && seg.item_idxs.len() == 1
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
    let seg = &frame.init.segments[frame.seg_i];
    let (grid_frag, sliceable) = (seg.grid_frag, seg.sliceable);
    if (grid_frag && emit_grid_fragments(frame))
        || (sliceable && !forced_rows_wanted(frame) && emit_sliced_fragments(frame, viewport))
    {
        enter_span_phase(frame);
    } else {
        compute_col_assignment(frame);
        frame.phase = Phase::Place;
        frame.k = 0;
    }
}

/// `column-wrap: wrap` without a `column-height` rows up the runs between forced breaks when
/// they ask for more than `column-count` columns (`assign_forced_rows`) — a path of the atomic
/// placement, so a segment of that kind is not sliced.
fn forced_rows_wanted(frame: &Frame) -> bool {
    if frame.init.wrap_auto.is_none() {
        return false;
    }
    let items: Vec<&LayoutBox> =
        frame.init.segments[frame.seg_i].item_idxs.iter().map(|&i| &frame.init.work[i]).collect();
    forced_breaks(&items).iter().filter(|f| **f).count() + 1 > frame.init.n_cols as usize
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

/// Column windows `(lo, hi)` for out-of-flow pieces (Fragmentation L3 §4.4): the content windows
/// with the last one stretched to its column's full block size, then empty columns of that size
/// up to `bottom` (virtual coordinates). Columns of unbounded size (`column-fill: auto` without
/// a height) are one and the same, so nothing is added.
fn oof_windows(windows: &[(f32, f32)], h_of: &dyn Fn(usize) -> f32, n: usize, bottom: f32) -> Vec<(f32, f32)> {
    let mut out = windows.to_vec();
    let k = out.len() - 1;
    let h = h_of(k);
    if !h.is_finite() {
        return out;
    }
    out[k].1 = out[k].1.max(out[k].0 + h);
    while out.last().is_some_and(|w| w.1 < bottom - 0.01) && out.len() < n.max(1) + 200 {
        let lo = out.last().map_or(0.0, |w| w.1);
        out.push((lo, lo + h_of(out.len())));
    }
    out
}

/// CSS Multicol §3.4 / Fragmentation L3 — cuts a segment across its columns at the break points
/// of the box tree ([`multicol_flow`]): between the children of a block on any depth, between
/// the lines of a text run, at forced breaks; boxes that cannot be cut go whole into one
/// column. A pure post-processing of the Measure pass's results — nothing is dispatched again.
///
/// Absolutely positioned boxes whose containing block is inside the cut part are lifted out of it
/// and cut on their own against the column windows, plus empty columns up to their bottom edge
/// (Fragmentation L3 §4.4: their offset is measured in the stack of the fragmentainers).
///
/// Returns `false`, before anything is consumed, when an item holds an out-of-flow box that the
/// virtual stack cannot move (the caller takes the atomic placement then).
fn emit_sliced_fragments(frame: &mut Frame, viewport: Size) -> bool {
    use multicol_flow::{scan_oof, take_oof, Flow, OofScan};
    let seg_i = frame.seg_i;
    let item_idxs = frame.init.segments[seg_i].item_idxs.clone();
    let scan = {
        let items: Vec<&LayoutBox> = item_idxs.iter().map(|&i| &frame.init.work[i]).collect();
        scan_oof(&items)
    };
    if scan == OofScan::Unsupported || (scan == OofScan::Inside && frame.init.col_rows.is_some()) {
        return false;
    }
    if scan == OofScan::None && emit_nested_fragments(frame, &item_idxs, viewport) {
        return true;
    }
    if scan == OofScan::None && frame.init.keep_flow {
        keep_nested_flow(frame, &item_idxs, viewport);
    }
    // Absolute boxes of the cut part: their containing block is a fragmented box, so they are cut
    // across the columns on their own, below (`(index of the item, box)`).
    let mut lifted: Vec<(usize, LayoutBox)> = Vec::new();
    if scan == OofScan::Inside {
        for (k, &i) in item_idxs.iter().enumerate() {
            let mut taken = Vec::new();
            take_oof(&mut frame.init.work[i], 0, &mut taken);
            lifted.extend(taken.into_iter().map(|b| (k, b)));
        }
    }
    let n = frame.init.n_cols as usize;
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
    let followed_by_span = frame.init.segments[seg_i].span_idx.is_some();

    let mut frags: Vec<LayoutBox> = Vec::new();
    let mut seg_extent = 0.0f32;
    let first_h: f32;
    let later_h: f32;
    let mut rows_used = 1usize;
    let oof_first: usize;
    {
        let items: Vec<multicol_flow::Item> = item_idxs
            .iter()
            .map(|&i| {
                let c = &frame.init.work[i];
                let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
                let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
                (c, mt, mb)
            })
            .collect();
        let flow = Flow::new(&items, viewport);
        let model = &flow.model;
        // Height of the first row of the segment and of the later ones. A segment that continues a
        // row cut by a spanner starts with the rest of that row; a segment closed by a spanner whose
        // content fits one row is balanced (Multicol L1 §7: columns before a spanner are balanced).
        match rows_cfg {
            Some(r) => {
                let avail = row_rest.map_or(r.col_h, |rest| rest.min(r.col_h)).max(1.0);
                later_h = r.col_h.max(1.0);
                let fits = model.paginate(&|_| avail, false).is_some_and(|w| w.len() <= n);
                first_h = if followed_by_span && fits { model.balanced(n, Some(avail)).clamp(1.0, avail) } else { avail };
            }
            None => {
                first_h = if balance {
                    model.balanced(n, container_h)
                } else {
                    container_h.unwrap_or(f32::INFINITY).max(1.0)
                };
                later_h = first_h;
            }
        }
        let h_of = |k: usize| if k < n { first_h } else { later_h };
        let pages = model.paginate_ex(&h_of, false).unwrap_or_else(|| vec![multicol_flow::Pg { lo: 0.0, hi: model.end, lead: 0.0, tail: 0.0 }]);
        let windows: Vec<(f32, f32)> = pages.iter().map(|p| (p.lo, p.hi)).collect();
        if rows_cfg.is_some() {
            rows_used = windows.len().max(1).div_ceil(n);
        }
        for (c, pg) in pages.iter().enumerate() {
            let (lo, hi) = (pg.lo, pg.hi);
            let (col_in_row, row) = match rows_cfg {
                Some(_) => (c % n, c / n),
                None => (c, 0),
            };
            let row_y = match rows_cfg {
                Some(r) if row > 0 => first_h + r.row_gap + (row - 1) as f32 * r.pitch(),
                _ => 0.0,
            };
            let win = Window {
                lo,
                hi,
                lead: pg.lead,
                tail: pg.tail,
                last: c + 1 == windows.len(),
                extent: h_of(c),
                col_top: cur_y + row_y,
                dx: content_x + col_in_row as f32 * (col_w + col_gap),
            };
            for &(b, off) in &flow.items {
                if let Some(mut frag) = multicol_flow::cut(b, off, &win, 0) {
                    if matches!(frag.kind, BoxKind::InlineRun { .. }) {
                        frag.rect.width = col_w;
                    }
                    frags.push(frag);
                }
            }
            seg_extent = seg_extent.max(hi - lo + pg.lead + pg.tail);
        }
        let ow = oof_windows(&windows, &h_of, n, lifted.iter().map(|(k, b)| b.rect.y + flow.items[*k].1 + b.rect.height).fold(0.0, f32::max));
        oof_first = frags.len();
        if rows_cfg.is_none() {
            for (c, &(lo, hi)) in ow.iter().enumerate() {
                let win = Window {
                    lo,
                    hi,
                    lead: 0.0,
                    tail: 0.0,
                    last: c + 1 == ow.len(),
                    extent: h_of(c),
                    col_top: cur_y,
                    dx: content_x + c as f32 * (col_w + col_gap),
                };
                for (k, b) in &lifted {
                    if let Some(piece) = multicol_flow::cut_oof(b, flow.items[*k].1, &win) {
                        frags.push(piece);
                    }
                }
            }
            frame.init.frag_cols = Some(FragCols { windows, first_h, later_h, origin: cur_y });
            frame.init.frag_segments += 1;
        }
    }
    let base = frame.init.out.len();
    frame.init.oof_out.extend(base + oof_first..base + frags.len());
    frame.init.out.extend(frags);
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
    true
}

/// May this multicol container keep its flow for a parent that cuts it across its columns
/// ([`MulticolInit::keep_flow`])? Not with a block size of its own or out of the flow; a container
/// of one plain segment, or one with rows of columns (and spanners between the segments).
fn nested_flow_eligible(init: &MulticolInit) -> bool {
    (init.container_h.is_none() || (init.col_rows.is_some() && !init.limit_shared))
        && init.s.position == Position::Static
        && init.wrap_auto.is_none()
        && (init.col_rows.is_some() || (init.segments.len() == 1 && init.segments[0].span_idx.is_none()))
}

/// The flow being kept for the parent, started on first use.
fn nested_flow_mut(init: &mut MulticolInit) -> &mut NestedFlow {
    let flow = NestedFlow {
        segs: Vec::new(),
        rows: init.col_rows.map(|r| (r.col_h, r.row_gap)),
        n: init.n_cols as usize,
        col_w: init.col_w,
        col_gap: init.col_gap,
        balance: init.balance,
        content_x: init.content_x,
        edge_top: init.s.border_top_width + init.padding_top,
        edge_bottom: init.s.border_bottom_width + init.padding_bottom,
    };
    init.nested_out.get_or_insert_with(|| Box::new(flow))
}

/// Did every segment of columns and every spanner of the container reach its kept flow? A segment
/// that was placed whole (atomic, a grid, ...) leaves a hole, and the flow cannot be cut.
fn nested_flow_complete(flow: &NestedFlow, init: &MulticolInit) -> bool {
    let wanted: usize = init
        .segments
        .iter()
        .map(|s| usize::from(!s.item_idxs.is_empty()) + usize::from(s.span_idx.is_some()))
        .sum();
    flow.segs.len() == wanted
}

/// The flow of this multicol container, kept for the parent that may cut it across its columns
/// ([`MulticolInit::keep_flow`]): the items of the segment being measured.
fn keep_nested_flow(frame: &mut Frame, item_idxs: &[usize], viewport: Size) {
    if !nested_flow_eligible(&frame.init) || item_idxs.is_empty() {
        return;
    }
    let col_w = frame.init.col_w;
    let items = item_idxs
        .iter()
        .map(|&i| {
            let c = &frame.init.work[i];
            let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
            let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
            (c.clone(), mt, mb)
        })
        .collect();
    nested_flow_mut(&mut frame.init).segs.push(NestedSeg::Cols(items));
}

/// Fragmentation L3 §4 — the segment is one nested multicol container (with the flow it kept) that
/// is taller than the column height of this `column-fill: auto` container: its columns continue
/// in the next column of this one instead of overflowing the first ([`cut_nested`]). `false`
/// (nothing consumed) when the segment is anything else, or when the nested box fits.
fn emit_nested_fragments(frame: &mut Frame, item_idxs: &[usize], viewport: Size) -> bool {
    let [i] = *item_idxs else { return false };
    let init = &frame.init;
    let Some(pos) = init.nested.iter().position(|(k, _)| *k == i) else { return false };
    let Some(h) = segment_limit(init) else { return false };
    let b = &init.work[i];
    if init.balance || init.col_rows.is_some() || !multicol_flow::is_cuttable_multicol(b) {
        return false;
    }
    let col_w = init.col_w;
    let mt = b.style.margin_top.resolve_or_zero(b.style.font_size, col_w, viewport);
    if mt + b.rect.height <= h + multicol_flow::EPS {
        return false;
    }
    let (content_x, pitch, cur_y) = (init.content_x, col_w + init.col_gap, init.cur_y);
    let col_dx = |m: usize| content_x + m as f32 * pitch;
    let nf = &init.nested[pos].1;
    let cut = if nf.rows.is_some() { cut_nested_rows(nf, b, h, mt, cur_y, &col_dx, viewport) } else { cut_nested(nf, b, h, mt, cur_y, &col_dx, viewport) };
    let Some(cut) = cut else { return false };
    // Out-of-flow boxes of the container itself are cut against one window per fragment.
    let windows: Vec<(f32, f32)> = (0..cut.frags.len()).map(|m| (m as f32 * h, (m + 1) as f32 * h)).collect();
    frame.init.frag_cols = Some(FragCols { windows, first_h: h, later_h: h, origin: cur_y });
    frame.init.frag_segments += 1;
    frame.init.out.extend(cut.frags);
    frame.init.consumed[i] = true;
    frame.init.cur_y += cut.extent.max(0.0);
    true
}

/// `true` when a flow item of the grid runs across the whole row gap `[gap_lo, gap_hi)` (its block
/// extent starts before the gap and ends after it), measured from the grid's top edge `gy`. A row
/// subgrid does not bridge the gap by itself when it already is cut: it starts at the top of the
/// window `from`, or an earlier window of `done` ended inside it. For the subgrid the gap is then
/// one of its own row gaps, and Chrome breaks there like in the parent unless an item of the
/// subgrid (or of a subgrid nested in it) runs through it (`subgrid/fragmentation/013`). The first
/// cut through a subgrid that began in the middle of a window keeps the gap (`012`).
fn gap_is_bridged(grid: &LayoutBox, gy: f32, from: f32, done: &[(f32, f32, bool)], gap_lo: f32, gap_hi: f32) -> bool {
    fn bridges(c: &LayoutBox, gy: f32, cut: bool, gap_lo: f32, gap_hi: f32) -> bool {
        if matches!(c.kind, BoxKind::Skip)
            || matches!(c.style.position, Position::Absolute | Position::Fixed)
            || c.rect.y - gy >= gap_lo - 0.01
            || c.rect.y - gy + c.rect.height <= gap_hi + 0.01
        {
            return false;
        }
        !(cut && super::grid_row_subgrid::is_row_subgrid(c)) || c.children.iter().any(|k| bridges(k, gy, true, gap_lo, gap_hi))
    }
    grid.children.iter().any(|c| {
        let top = c.rect.y - gy;
        let cut = (top - from).abs() < 0.01 || done.iter().any(|w| w.1 > top + 0.01 && w.1 < gap_lo - 0.01);
        bridges(c, gy, cut, gap_lo, gap_hi)
    })
}

/// Block offsets (from the grid's top edge) of the row tracks that a forced column break
/// (`break-before`/`break-after: column|always` of an in-flow item) makes the first of a column:
/// the start of the track an item begins in for `break-before`, of the track after the one it ends
/// in for `break-after`. Sorted, without the very top.
fn forced_row_starts(grid: &LayoutBox, gy: f32, rows: &[(f32, f32)]) -> Vec<f32> {
    use crate::style::BreakValue;
    let forced = |v: BreakValue| matches!(v, BreakValue::Column | BreakValue::Always);
    let mut starts: Vec<f32> = Vec::new();
    for c in &grid.children {
        if matches!(c.kind, BoxKind::Skip) || matches!(c.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let (top, bot) = (c.rect.y - gy, c.rect.y - gy + c.rect.height);
        if forced(c.style.break_before)
            && let Some(k) = rows.iter().rposition(|r| r.0 <= top + 0.01)
        {
            starts.push(rows[k].0);
        }
        if forced(c.style.break_after)
            && let Some(k) = rows.iter().rposition(|r| r.0 < bot - 0.01)
            && let Some(next) = rows.get(k + 1)
        {
            starts.push(next.0);
        }
    }
    starts.retain(|&y| y > 0.01);
    starts.sort_by(f32::total_cmp);
    starts.dedup_by(|a, b| (*a - *b).abs() <= 0.01);
    starts
}

/// The windows `[start, end)` (in the grid's own unfragmented block coordinates) a grid of height
/// `total_h` with row tracks `rows` is cut into by columns `limit` tall, each with a flag that it
/// ends in a forced break (the column keeps its full height). A break inside a row gap drops the
/// rest of the gap (Fragmentation L3 §5.1: a gap adjoining a break is truncated); so does one at
/// the very start edge of the gap, unless an item runs through it (Chrome keeps such a gap:
/// `grid-gap-decorations-fragmentation-029`). Inside a track the track continues. A forced break
/// (`forced`, see [`forced_row_starts`]) that falls inside the column ends the window at the track
/// before it and starts the next one at the forced track. `mono` — the `(top, bottom)` extents of
/// the monolithic items, only in a balanced container: a column that holds the top edge of one
/// reaches to its bottom edge (the next column starts there, `flex-gap-decorations-fragmentation-021`);
/// with `column-fill: auto` the overflow does not move the next column (`012`) and `mono` is empty.
/// In a wrapped column flex container a break at the leading edge of a gap between
/// items does not drop the gap, the next column starts at the break (`flex/fragmentation/014`:
/// the gap is drawn at the top of the column).
fn grid_windows(
    grid: &LayoutBox,
    gy: f32,
    rows: &[(f32, f32)],
    forced: &[f32],
    mono: &[(f32, f32)],
    total_h: f32,
    limit: f32,
) -> Vec<(f32, f32, bool)> {
    let keep_gaps = is_column_flex(grid);
    // The block extents of the children of the items (line boxes, nested blocks): a break never
    // cuts one of them. Only a wrapped column flex container is snapped this way; a grid with a
    // child straddling the break stays atomic (`emit_grid_fragments`).
    let kid_edges: Vec<(f32, f32)> = grid
        .children
        .iter()
        .filter(|c| {
            !matches!(c.kind, BoxKind::Skip)
                && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                && !is_monolithic(c)
        })
        .flat_map(|c| c.children.iter())
        .filter(|k| keep_gaps && !matches!(k.kind, BoxKind::Skip))
        .map(|k| (k.rect.y - gy, k.rect.y - gy + k.rect.height))
        .collect();
    let mut windows: Vec<(f32, f32, bool)> = Vec::new();
    // The block size the container still has: a column that ends above its nominal bottom (a break
    // moved up to the top of a child) consumed the whole column height of it.
    let mut total_h = total_h;
    let mut start = 0.0f32;
    loop {
        let mut end = start + limit;
        if let Some(&(_, bot)) = mono.iter().find(|m| m.0 >= start - 0.01 && m.0 < end - 0.01 && m.1 > end + 0.01) {
            end = bot;
        }
        if let Some(&fs) = forced.iter().find(|&&y| y > start + 0.01 && y <= end + 0.01) {
            let prev_end = rows.iter().rev().find(|r| r.0 < fs - 0.01).map_or(fs, |r| r.1.min(fs));
            windows.push((start, prev_end.max(start), true));
            start = fs;
            continue;
        }
        if end >= total_h - 0.01 {
            windows.push((start, total_h, false));
            break;
        }
        // A break that falls inside a line box (or a nested block) of an item cannot cut it: the
        // content that does not fit moves to the next column, so the break goes up to the top of
        // the first child it would have cut (Fragmentation L3 §4.4) and the column keeps its full
        // height (`flex/fragmentation/009`).
        let nominal_end = end;
        let mut snapped = false;
        while let Some(top) = kid_edges
            .iter()
            .filter(|k| k.0 < end - 0.01 && k.1 > end + 0.01)
            .map(|k| k.0)
            .fold(None, |m: Option<f32>, t| Some(m.map_or(t, |m| m.min(t))))
        {
            if top <= start + 0.01 {
                break;
            }
            end = top;
            snapped = true;
        }
        windows.push((start, end, snapped));
        let in_gap = rows.windows(2).find(|w| {
            let inside = end > w[0].1 + 0.01 && end < w[1].0 - 0.01;
            let at_start = (end - w[0].1).abs() <= 0.01 && end < w[1].0 - 0.01;
            inside || (at_start && !gap_is_bridged(grid, gy, start, &windows, w[0].1, w[1].0))
        });
        // A wrapped column flex container keeps a gap whose leading edge the break falls on
        // (`flex/fragmentation/014`); a break inside the gap still drops the rest of it (`006`).
        let at_gap_start = keep_gaps && rows.windows(2).any(|w| (end - w[0].1).abs() <= 0.01 && end < w[1].0 - 0.01);
        start = if at_gap_start { end } else { in_gap.map_or(end, |w| w[1].0) };
        if snapped {
            total_h -= (nominal_end - start).max(0.0);
        }
        if start >= total_h - 0.01 {
            break;
        }
    }
    windows
}

/// A main-axis gap of a flex line plus the block extent `(top, bottom)` (from the container's top
/// edge) of the item that follows it and whether that item is monolithic: the gap belongs to the
/// fragments that hold this item.
struct LineGap {
    start: f32,
    end: f32,
    next_top: f32,
    next_bot: f32,
    next_mono: bool,
}

/// A box a column break never cuts (`contain: size`, Fragmentation L3 §3: monolithic): it is placed
/// whole in the fragment its top edge falls in and overflows that column.
fn is_monolithic(b: &LayoutBox) -> bool {
    b.style.contain.0 & crate::style::ContainFlags::SIZE.0 != 0
}

/// A block with nothing inside it (only whitespace placeholders) and no border: a column break
/// can cut it by its box alone, each fragment keeping its part.
fn is_clippable_leaf(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block)
        && b.children.iter().all(|c| matches!(c.kind, BoxKind::Skip))
        && [b.style.border_top_width, b.style.border_bottom_width, b.style.border_left_width, b.style.border_right_width]
            .iter()
            .all(|w| *w == 0.0)
}

/// A wrapped `flex-direction: column` container (its lines are columns of the multicol's column).
fn is_column_flex(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::Flex)
        && matches!(b.style.flex_direction, crate::style::FlexDirection::Column)
}

/// The block-axis tracks of a wrapped column flex container, `(top, bottom)` from its border-box
/// top: the margin-box extents of the in-flow items merged across the lines. Two extents closer
/// than the `row-gap` belong to one track, so what stays between two tracks is a row gap that a
/// column break can drop (Fragmentation L3 §5.1).
fn column_flex_tracks(flex: &LayoutBox, gy0: f32) -> Vec<(f32, f32)> {
    let s = &flex.style;
    let cw = flex.rect.width;
    let vp = Size::new(cw, flex.rect.height);
    let gap = s.row_gap.resolve_or_zero(s.font_size, cw, vp);
    let mut spans: Vec<(f32, f32)> = flex
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|c| {
            let cs = &c.style;
            let mt = cs.margin_top.resolve_or_zero(cs.font_size, cw, vp);
            let mb = cs.margin_bottom.resolve_or_zero(cs.font_size, cw, vp);
            (c.rect.y - gy0 - mt, c.rect.y - gy0 + c.rect.height + mb)
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut tracks: Vec<(f32, f32)> = Vec::new();
    for sp in spans {
        match tracks.last_mut() {
            Some(t) if sp.0 < t.1 + (gap - 0.5).max(0.01) => t.1 = t.1.max(sp.1),
            _ => tracks.push(sp),
        }
    }
    tracks
}

/// The main-axis gaps of every flex line of a wrapped row flex container, in the container's own
/// coordinates (`rows` — the lines as `(top, bottom)` from the border-box top, `gy0` — its top).
/// A gap is the free space between two neighbouring items of one line that is at least the
/// `column-gap` wide (`gap: 0` — any seam); the margin boxes are used. `None` for a grid, whose
/// tracks the painter reads itself.
fn flex_line_gaps(flex: &LayoutBox, gy0: f32, rows: &[(f32, f32)]) -> Option<Vec<Vec<LineGap>>> {
    if !matches!(flex.style.display, Display::Flex) || is_column_flex(flex) {
        return None;
    }
    let s = &flex.style;
    let em = s.font_size;
    let cw = flex.rect.width;
    let vp = Size::new(cw, flex.rect.height);
    let main_gap = s.column_gap.resolve_or_zero(em, cw, vp);
    // Per line: the item's margin-box span along the main axis, its block extent, monolithic.
    type Item = (f32, f32, f32, f32, bool);
    let mut lines: Vec<Vec<Item>> = vec![Vec::new(); rows.len()];
    for c in &flex.children {
        if matches!(c.kind, BoxKind::Skip) || matches!(c.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        let cs = &c.style;
        let top = c.rect.y - gy0 - cs.margin_top.resolve_or_zero(cs.font_size, cw, vp);
        let k = rows.iter().rposition(|r| r.0 <= top + 0.5).unwrap_or(0);
        let l = c.rect.x - cs.margin_left.resolve_or_zero(cs.font_size, cw, vp);
        let r = c.rect.x + c.rect.width + cs.margin_right.resolve_or_zero(cs.font_size, cw, vp);
        lines[k].push((l.min(r), r, c.rect.y - gy0, c.rect.y - gy0 + c.rect.height, is_monolithic(c)));
    }
    Some(
        lines
            .into_iter()
            .map(|mut items| {
                items.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut gaps = Vec::new();
                let Some(mut reach) = items.first().map(|i| i.1) else { return gaps };
                for it in &items[1..] {
                    let dist = it.0 - reach;
                    let is_gap = if main_gap > 0.5 { dist > 0.5 && dist >= main_gap - 0.5 } else { dist >= -0.5 };
                    if is_gap {
                        gaps.push(LineGap { start: reach, end: it.0, next_top: it.2, next_bot: it.3, next_mono: it.4 });
                    }
                    reach = reach.max(it.1);
                }
                gaps
            })
            .collect(),
    )
}

/// One flex line of a wrapped column flex container: its extent across the main axis, `(left,
/// right)` from the container's left edge, and the main-axis gaps along the block axis (`start`/
/// `end`/`next_*` from the container's top edge).
struct ColumnLine {
    lo: f32,
    hi: f32,
    gaps: Vec<LineGap>,
    /// Indices (into `flex.children`) of the items of the line.
    members: Vec<usize>,
}

/// The flex lines (columns) of a wrapped column flex container laid out in full, in `x` order, with
/// the gaps between the items of each. The items are grouped as the painter groups them (two items
/// share a line when their margin boxes overlap across the main axis), the gap is the free space
/// between two neighbouring items of one line that is at least the `row-gap` wide (`gap: 0` — any
/// seam). A fragment cut by a column break keeps every line, even one with no item in it: the
/// column rule between two lines runs through it all the same (`flex-gap-decorations-fragmentation-020`),
/// and the gap next to an item that stayed in an earlier column belongs to the fragment that holds
/// the next item. `None` for anything but a column flex container in a horizontal writing mode.
fn column_flex_lines(flex: &LayoutBox, gy0: f32) -> Option<Vec<ColumnLine>> {
    if !is_column_flex(flex) || flex.style.writing_mode != crate::style::WritingMode::HorizontalTb {
        return None;
    }
    let s = &flex.style;
    let em = s.font_size;
    let cw = flex.rect.width;
    let vp = Size::new(cw, flex.rect.height);
    let main_gap = s.row_gap.resolve_or_zero(em, cw, vp);
    let wrap = s.flex_wrap != crate::style::FlexWrap::Nowrap;
    // Per item: the margin-box span across the main axis (`x`), along it (`y`), its block extent,
    // monolithic, its index among the children.
    type Item = ((f32, f32), (f32, f32), f32, f32, bool, usize);
    let mut items: Vec<Item> = flex
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(idx, c)| {
            let cs = &c.style;
            let m = |l: &LengthOrAuto| l.resolve_or_zero(cs.font_size, cw, vp);
            let ordered = |a: f32, b: f32| (a.min(b), b);
            let x = ordered(c.rect.x - m(&cs.margin_left), c.rect.x + c.rect.width + m(&cs.margin_right));
            let y = ordered(c.rect.y - gy0 - m(&cs.margin_top), c.rect.y - gy0 + c.rect.height + m(&cs.margin_bottom));
            (x, y, c.rect.y - gy0, c.rect.y - gy0 + c.rect.height, is_monolithic(c), idx)
        })
        .collect();
    items.sort_by(|a, b| a.0 .0.total_cmp(&b.0 .0));
    let mut groups: Vec<Vec<Item>> = Vec::new();
    let (mut cur_hi, mut cur_lo) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for it in items {
        match groups.last_mut() {
            Some(g) if !wrap || it.0 .0 < cur_hi - 0.5 || it.0 .0 <= cur_lo + 0.5 => {
                g.push(it);
                cur_hi = cur_hi.max(it.0 .1);
                cur_lo = cur_lo.max(it.0 .0);
            }
            _ => {
                groups.push(vec![it]);
                (cur_lo, cur_hi) = (it.0 .0, it.0 .1);
            }
        }
    }
    Some(
        groups
            .into_iter()
            .map(|mut g| {
                g.sort_by(|a, b| a.1 .0.total_cmp(&b.1 .0));
                let lo = g.iter().map(|i| i.0 .0).fold(f32::INFINITY, f32::min);
                let hi = g.iter().map(|i| i.0 .1).fold(f32::NEG_INFINITY, f32::max);
                let mut gaps = Vec::new();
                let mut reach = g[0].1 .1;
                for it in &g[1..] {
                    let dist = it.1 .0 - reach;
                    let is_gap = if main_gap > 0.5 { dist > 0.5 && dist >= main_gap - 0.5 } else { dist >= -0.5 };
                    if is_gap {
                        gaps.push(LineGap { start: reach, end: it.1 .0, next_top: it.2, next_bot: it.3, next_mono: it.4 });
                    }
                    reach = reach.max(it.1 .1);
                }
                ColumnLine { lo, hi, gaps, members: g.iter().map(|i| i.5).collect() }
            })
            .collect(),
    )
}

/// A wrapped row flex container whose items carry one forced column break (`break-before: column`
/// of an item that begins a later line) is not cut at the break the way a grid is: Chrome cuts the
/// box exactly at the column heights and the items of the lines from the break on sit lower than
/// the stretched line by `limit - c`, where `c` is the end of the item lines before the break plus
/// the row gap (Edge, `flex/fragmentation/008`: the 174px box gives 90 + 84, the third item sits
/// at y = 50 of the second column). Returns the container with those items moved and the lines as
/// the extents of their items (the rules run along the items, not along the stretched line);
/// `None` when the container does not qualify.
fn row_flex_forced_pull(
    grid: &LayoutBox,
    gy0: f32,
    rows: &[(f32, f32)],
    forced: &[f32],
    total_h: f32,
    limit: f32,
) -> Option<(LayoutBox, Vec<(f32, f32)>)> {
    // A break in front of the first line is no break.
    let first = rows.first()?.0;
    let later: Vec<f32> = forced.iter().copied().filter(|&y| y > first + 0.01).collect();
    if !matches!(grid.style.display, Display::Flex) || is_column_flex(grid) || later.len() != 1 || total_h <= limit {
        return None;
    }
    let fs = later[0];
    let s = &grid.style;
    let cw = grid.rect.width;
    let vp = Size::new(cw, grid.rect.height);
    let row_gap = s.row_gap.resolve_or_zero(s.font_size, cw, vp);
    let flow = |c: &LayoutBox| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed);
    let before = grid
        .children
        .iter()
        .filter(|c| flow(c) && c.rect.y - gy0 < fs - 0.01)
        .map(|c| c.rect.y - gy0 + c.rect.height + c.style.margin_bottom.resolve_or_zero(c.style.font_size, cw, vp))
        .fold(None, |m: Option<f32>, b| Some(m.map_or(b, |m| m.max(b))))?;
    let c = before + row_gap;
    if c >= limit - 0.01 {
        return None;
    }
    let delta = limit - c;
    let mut moved = grid.clone();
    let mut natural: Vec<Option<(f32, f32)>> = vec![None; rows.len()];
    for ch in moved.children.iter_mut().filter(|c| flow(c)) {
        let top = ch.rect.y - gy0;
        let k = rows.iter().rposition(|r| r.0 <= top + 0.5).unwrap_or(0);
        if top >= fs - 0.01 {
            super::shift_tree(ch, 0.0, delta);
        }
        let (t, b) = (ch.rect.y - gy0, ch.rect.y - gy0 + ch.rect.height);
        natural[k] = Some(natural[k].map_or((t, b), |n| (n.0.min(t), n.1.max(b))));
    }
    let natural = natural.into_iter().zip(rows).map(|(n, &r)| n.unwrap_or(r)).collect();
    Some((moved, natural))
}

/// The leftover of a row subgrid item `c` that holds a nested row subgrid: Chrome fragments such an
/// item over its whole block size, and the row gaps that fall between the windows from the nested
/// subgrid's top edge on (the nested subgrid drops them itself, the item does not know) stay
/// unconsumed, so the item gets one more fragment of their total size after its last window
/// (`subgrid/fragmentation/013`: 20 + 20 + 20 + 10px; a gap above the nested subgrid is dropped
/// by the item's own break). `(index of the item's last window, leftover)`.
fn subgrid_tail(c: &LayoutBox, gy: f32, windows: &[(f32, f32, bool)]) -> Option<(usize, f32)> {
    use super::grid_row_subgrid::is_row_subgrid;
    if matches!(c.kind, BoxKind::Skip) || !is_row_subgrid(c) {
        return None;
    }
    let (top, bot) = (c.rect.y - gy, c.rect.y - gy + c.rect.height);
    let nested_top = c
        .children
        .iter()
        .filter(|k| !matches!(k.kind, BoxKind::Skip) && is_row_subgrid(k))
        .map(|k| k.rect.y - gy)
        .fold(f32::INFINITY, f32::min);
    let last = windows.iter().rposition(|w| w.1 > top + 0.01 && w.0 < bot - 0.01)?;
    let tail: f32 = windows
        .windows(2)
        .map(|w| (w[0].1, w[1].0))
        .filter(|&(lo, hi)| hi > lo + 0.01 && lo >= nested_top - 0.01 && hi <= bot + 0.01)
        .map(|(lo, hi)| hi - lo)
        .sum();
    (tail > 0.01).then_some((last, tail))
}

/// A subgrid inside a subgrid item that the container's windows cut like the item itself: its rows
/// are tracks of the outer grid, so every window edge that runs through it falls between two of
/// them (no gap of its own is dropped) and none of its children straddles an edge
/// (`subgrid/fragmentation/013`: two nested subgrids, every row gap suppressed).
fn nested_cuttable(g: &LayoutBox, gy: f32, windows: &[(f32, f32, bool)]) -> bool {
    let Some(t) = g.subgrid_tracks.as_ref().filter(|t| t.rows.is_some()) else { return false };
    if matches!(g.kind, BoxKind::Skip) || is_monolithic(g) {
        return false;
    }
    let (top, bot) = (g.rect.y - gy, g.rect.y - gy + g.rect.height);
    let spans: Vec<(f32, f32)> = g
        .children
        .iter()
        .filter(|h| !matches!(h.kind, BoxKind::Skip) && !is_clippable_leaf(h))
        .map(|h| (h.rect.y - gy, h.rect.y - gy + h.rect.height))
        .collect();
    windows.iter().all(|&(ws, we, _)| {
        let (lo, hi) = (top.max(ws), bot.min(we));
        if hi <= lo + 0.01 {
            return true;
        }
        let whole = spans.iter().all(|&(a, b)| !(a < we - 0.01 && b > we + 0.01) && !(a < ws - 0.01 && b > ws + 0.01));
        whole && item_fragment_tracks(t, top + g.style.border_top_width, lo, hi, &spans, None).is_some_and(|r| r.1 == 0.0 && r.2.is_none())
    })
}

/// The part `[ws, we)` of a nested subgrid (see [`nested_cuttable`]) as its fragment: it keeps the
/// children that start in the window (a childless cell is cut) and the row tracks clipped to it. `dx`/`dy` move it into the
/// fragment, as for the other children of the item.
fn nested_fragment(g: &LayoutBox, gy: f32, ws: f32, we: f32, dx: f32, dy: f32) -> Option<LayoutBox> {
    let (t, b) = (g.rect.y - gy, g.rect.y - gy + g.rect.height);
    let (lo, hi) = (t.max(ws), b.min(we));
    if hi <= lo {
        return None;
    }
    let spans: Vec<(f32, f32)> = g
        .children
        .iter()
        .filter(|h| !matches!(h.kind, BoxKind::Skip))
        .map(|h| (h.rect.y - gy, h.rect.y - gy + h.rect.height))
        .collect();
    let mut k = g.clone();
    k.rect.x += dx;
    k.rect.y += dy + (lo - t);
    k.rect.height = hi - lo;
    if let Some((tracks, _, _)) = g
        .subgrid_tracks
        .as_ref()
        .and_then(|tr| item_fragment_tracks(tr, t + g.style.border_top_width, lo, hi, &spans, None))
    {
        k.subgrid_tracks = Some(tracks);
    }
    k.children = g
        .children
        .iter()
        .filter_map(|h| {
            let (ht, hb) = (h.rect.y - gy, h.rect.y - gy + h.rect.height);
            let mut h = h.clone();
            if matches!(h.kind, BoxKind::Skip) {
                super::shift_tree(&mut h, dx, dy);
                return Some(h);
            }
            // A childless cell that the window edge runs through keeps its part in each window.
            if is_clippable_leaf(&h) && (ht < lo - 0.01 || hb > hi + 0.01) {
                let (hlo, hhi) = (ht.max(lo), hb.min(hi));
                return (hhi > hlo).then(|| {
                    super::shift_tree(&mut h, dx, dy);
                    h.rect.y += hlo - ht;
                    h.rect.height = hhi - hlo;
                    h
                });
            }
            (ht >= lo - 0.01 && ht < hi - 0.01).then(|| {
                super::shift_tree(&mut h, dx, dy);
                h
            })
        })
        .collect();
    Some(k)
}

/// The block size of the row gap of a grid item (a subgrid) that a window starting at `ws` drops at
/// its top edge, container coordinates from `gy`: the item's content moves up by this much. 0 for
/// an item that starts in the window or has no row tracks of its own.
fn drop_at(c: &LayoutBox, gy: f32, ws: f32) -> f32 {
    let top = c.rect.y - gy;
    if top >= ws - 0.01 || matches!(c.kind, BoxKind::Skip) {
        return 0.0;
    }
    let spans: Vec<(f32, f32)> = c
        .children
        .iter()
        .filter(|g| !matches!(g.kind, BoxKind::Skip))
        .map(|g| (g.rect.y - gy, g.rect.y - gy + g.rect.height))
        .collect();
    c.subgrid_tracks
        .as_ref()
        .and_then(|t| item_fragment_tracks(t, top + c.style.border_top_width, ws, top + c.rect.height, &spans, None))
        .map_or(0.0, |r| r.1)
}

/// How far below the end `we` of a window `[ws, we)` the item `c` reaches: the item's content moved
/// up by the gap it dropped at the top (`drop_at`), so a child that crosses `we` and ends within
/// that distance still fits the window (`subgrid-gap-decorations-fragmentation-012`: the nested
/// subgrid ends 10px below the break). 0 when no child needs it.
fn room_at(c: &LayoutBox, gy: f32, ws: f32, we: f32) -> f32 {
    let drop = drop_at(c, gy, ws);
    let needed = drop > 0.0
        && c.children.iter().any(|g| {
            !matches!(g.kind, BoxKind::Skip) && g.rect.y - gy < we + drop - 0.01 && g.rect.y - gy + g.rect.height > we + 0.01
        });
    if needed { drop } else { 0.0 }
}

/// The start (container coordinates) of the first row of the subgrid item `c`, other than its
/// first, that holds a child with a forced column break. Such a break is not a cut of its own (the
/// container's windows come from its direct children), but Chrome lays the item's gap decorations
/// out as if the column ended there (`subgrid/fragmentation/011`).
fn nested_forced_row(c: &LayoutBox, gy: f32) -> Option<f32> {
    use crate::style::BreakValue;
    let rows = c.subgrid_tracks.as_ref()?.rows.as_ref()?;
    let top = c.rect.y - gy + c.style.border_top_width;
    let first = rows.first()?.0 + top;
    c.children
        .iter()
        .filter(|g| !matches!(g.kind, BoxKind::Skip) && !matches!(g.style.position, Position::Absolute | Position::Fixed))
        .filter(|g| matches!(g.style.break_before, BreakValue::Column | BreakValue::Always))
        .filter_map(|g| rows.iter().rposition(|r| r.0 + top <= g.rect.y - gy + 0.01).map(|k| rows[k].0 + top))
        .filter(|&r| r > first + 0.01)
        .min_by(f32::total_cmp)
}

/// The row tracks of a grid item cut by a column window, as the fragment of that item: `t.rows`
/// run from the item's content top (`content_top`, container coordinates), the item keeps the part
/// `lo..hi` of itself. `None` when the item has no row tracks to clip (the item keeps its own).
/// The second value is the block size of the item's own row gap that the window starts in (a
/// window edge on the leading edge of the gap or inside it): that gap is dropped, the first
/// visible track moves up to the fragment's top edge by this amount. When a child of the item
/// (`spans`, container coordinates) crosses that gap the gap is kept inside the item instead: the
/// second value is 0 and the third is `(top of the first track, gap size)`, the track grows
/// upwards over the gap. `forced` — the start of a row with a forced break (`nested_forced_row`)
/// inside the window: the tracks from it on are left out.
#[allow(clippy::type_complexity)]
fn item_fragment_tracks(
    t: &crate::subgrid::SubgridTracks,
    content_top: f32,
    lo: f32,
    hi: f32,
    spans: &[(f32, f32)],
    forced: Option<f32>,
) -> Option<(Box<crate::subgrid::SubgridTracks>, f32, Option<(f32, f32)>)> {
    let rows = t.rows.as_ref()?;
    let visible: Vec<(usize, (f32, f32))> = rows
        .iter()
        .enumerate()
        .filter_map(|(k, &(a, b))| {
            // The tracks from the forced break's row on belong to the next fragment: the gap
            // before that row is not drawn here.
            if forced.is_some_and(|r| a + content_top > r - 0.01) {
                return None;
            }
            let (a, b) = ((a + content_top).max(lo), (b + content_top).min(hi));
            (b > a).then_some((k, (a - lo, b - lo)))
        })
        .collect();
    // A first visible track that is not the item's first and starts below the window edge: the
    // edge is in the gap before it.
    let gap = visible.first().filter(|v| v.0 > 0 && v.1.0 > 0.01).map_or(0.0, |v| v.1.0);
    let bridged = gap > 0.0 && spans.iter().any(|&(a, b)| a <= lo + 0.01 && b >= lo + gap - 0.01);
    let (dropped, grown) = if bridged { (0.0, Some((lo + gap, gap))) } else { (gap, None) };
    Some((
        Box::new(crate::subgrid::SubgridTracks {
            cols: t.cols.clone(),
            rows: Some(
                visible
                    .iter()
                    .enumerate()
                    .map(|(n, v)| if bridged { (if n == 0 { 0.0 } else { v.1.0 }, v.1.1) } else { (v.1.0 - dropped, v.1.1 - dropped) })
                    .collect(),
            ),
            fragment: true,
            row_gap_base: visible.first().map(|v| (v.0, rows.len().saturating_sub(1))),
            line_gaps: None,
            rule_shift: 0.0,
        }),
        dropped,
        grown,
    ))
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
    if let Some(row_gap) = frame.init.wrap_auto {
        let chunks = forced.iter().filter(|f| **f).count() + 1;
        if chunks > n_cols {
            assign_forced_rows(frame, &forced, row_gap);
            return;
        }
    }
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

/// `column-wrap: wrap` without a `column-height` (Multicol L2 §4.4), forced column breaks
/// asking for more than `column-count` columns: every run of items between two forced breaks is
/// one column, `column-count` columns make a row, and a row is as tall as its tallest column.
/// Fills `col_assignment` (a column number counted across rows), `col_y` (each column's row top)
/// and `wrap_end`.
fn assign_forced_rows(frame: &mut Frame, forced: &[bool], row_gap: f32) {
    let n_cols = frame.init.n_cols as usize;
    let mut col = 0usize;
    let mut assignment = Vec::with_capacity(forced.len());
    let mut heights: Vec<f32> = vec![0.0];
    for (j, f) in forced.iter().enumerate() {
        if *f {
            col += 1;
            heights.push(0.0);
        }
        assignment.push(col);
        heights[col] += frame.outer_hs[j];
    }
    let rows = heights.len().div_ceil(n_cols);
    let mut y = frame.init.cur_y;
    let mut col_y = Vec::with_capacity(rows * n_cols);
    for r in 0..rows {
        let row_h = heights[r * n_cols..((r + 1) * n_cols).min(heights.len())].iter().copied().fold(0.0, f32::max);
        col_y.extend(std::iter::repeat_n(y, n_cols));
        y += row_h + if r + 1 < rows { row_gap } else { 0.0 };
    }
    col_y.truncate(heights.len().max(n_cols));
    frame.col_assignment = assignment;
    frame.col_y = col_y;
    frame.wrap_end = Some(y);
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
        None => frame.wrap_end.unwrap_or_else(|| frame.col_y.iter().copied().fold(frame.init.cur_y, f32::max)),
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
    if frame.init.keep_flow && nested_flow_eligible(&frame.init) {
        let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, content_width, viewport);
        nested_flow_mut(&mut frame.init).segs.push(NestedSeg::Span(Box::new(placed.clone()), mt, mb));
    }
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
    let own_start = out.len();
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
        .filter(|(i, c)| {
            matches!(c.style.position, Position::Absolute | Position::Fixed) && !frame.init.oof_out.contains(i)
        })
        .map(|(i, _)| (i, frame.init.content_x, frame.init.content_y))
        .collect();
    finish_after_match(
        &mut frame.b, &frame.init.s, frame.init.em, frame.init.cb, frame.init.available_height, frame.init.is_positioned,
        frame.init.own_pcb, &abs_deferred, measurer, viewport, hp,
    );
    cut_own_abspos(frame, own_start, viewport);
}

/// Fragmentation L3 §4.4 — the container's own absolutely positioned children (placed against its
/// padding box by `finish_after_match`) are cut across its columns: their `top` is an offset in the
/// stack of the columns, so a box at `top: 150px` in a 100px high container starts 50px down the
/// second column. Only when the container is one cut segment (no spanners, no rows of columns).
fn cut_own_abspos(frame: &mut Frame, own_start: usize, viewport: Size) {
    let Some(fc) = frame.init.frag_cols.take() else { return };
    if frame.init.segments.len() != 1 || frame.init.frag_segments != 1 || frame.init.col_rows.is_some() {
        return;
    }
    let s = &frame.init.s;
    let (ox, oy) = super::layout_dispatch::relative_offset(s, frame.init.em, frame.init.cb, frame.init.available_height, viewport);
    let n = frame.init.n_cols as usize;
    let (first_h, later_h) = (fc.first_h, fc.later_h);
    let h_of = |k: usize| if k < n { first_h } else { later_h };
    let origin = fc.origin + oy;
    let is_own = |i: usize, c: &LayoutBox| {
        i >= own_start && !matches!(c.kind, BoxKind::Skip) && c.style.position == Position::Absolute
    };
    let bottom = frame
        .b
        .children
        .iter()
        .enumerate()
        .filter(|(i, c)| is_own(*i, c))
        .map(|(_, c)| c.rect.y - origin + c.rect.height)
        .fold(0.0, f32::max);
    let ow = oof_windows(&fc.windows, &h_of, n, bottom);
    let pitch = frame.init.col_w + frame.init.col_gap;
    let children = std::mem::take(&mut frame.b.children);
    for (i, c) in children.into_iter().enumerate() {
        if !is_own(i, &c) {
            frame.b.children.push(c);
            continue;
        }
        let mut pieces = Vec::new();
        for (k, &(lo, hi)) in ow.iter().enumerate() {
            let win = Window { lo, hi, lead: 0.0, tail: 0.0, last: k + 1 == ow.len(), extent: h_of(k), col_top: origin, dx: ox + k as f32 * pitch };
            pieces.extend(multicol_flow::cut_oof(&c, -origin, &win));
        }
        if pieces.is_empty() {
            frame.b.children.push(c);
        } else {
            frame.b.children.extend(pieces);
        }
    }
}
