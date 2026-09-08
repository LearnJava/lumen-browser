use super::*;
use super::layout_dispatch::{dispatch_box, finalize_block_height, finish_after_match};
use super::block_flow_trampoline::{self, DispatchOutcome};
use super::multicol_abspos::balanced_column_height;

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
                let col_x = current.init.content_x + col as f32 * (col_w + current.init.col_gap);
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
    if frame.init.segments[frame.seg_i].sliceable {
        emit_sliced_fragments(frame, viewport);
        enter_span_phase(frame);
    } else {
        compute_col_assignment(frame);
        frame.phase = Phase::Place;
        frame.k = 0;
    }
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
    let container_h = frame.init.container_h;
    let cur_y = frame.init.cur_y;
    let item_idxs = frame.init.segments[seg_i].item_idxs.clone();
    let outer_hs = frame.outer_hs.clone();

    let total_h: f32 = outer_hs.iter().sum();
    let col_h = if balance {
        (total_h / n_cols as f32).ceil().max(1.0)
    } else {
        container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
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

    // Emit one clipped fragment per (column, box) overlap.
    let mut seg_extent = 0.0f32;
    for c in 0..n_cols as usize {
        let col_lo = c as f32 * col_h;
        let col_hi = col_lo + col_h;
        let col_x = content_x + c as f32 * (col_w + col_gap);
        for &(i, bt, bh) in &stack {
            let bb = bt + bh;
            let ov_lo = bt.max(col_lo);
            let ov_hi = bb.min(col_hi);
            if ov_hi > ov_lo {
                let mut frag = frame.init.work[i].clone();
                frag.rect.x = col_x;
                frag.rect.y = cur_y + (ov_lo - col_lo);
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
    frame.init.cur_y += seg_extent.max(0.0);
}

/// CSS Multicol §3.4 atomic fallback — greedy column assignment by height,
/// copied verbatim from the removed code's `else` branch (balanced target
/// height in balance mode, container height in fill:auto mode). Pure
/// function of the Measure pass's `outer_hs` — seeds `Phase::Place`'s
/// per-column running cursor (`col_y`) at the segment's current `cur_y`.
fn compute_col_assignment(frame: &mut Frame) {
    let n_cols = frame.init.n_cols as usize;
    let balance = frame.init.balance;
    let container_h = frame.init.container_h;
    let cur_y = frame.init.cur_y;
    let outer_hs = &frame.outer_hs;
    let total_h: f32 = outer_hs.iter().sum();
    let target_h = if balance {
        balanced_column_height(outer_hs, n_cols)
    } else {
        container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
    };

    let mut col_assignment = vec![0usize; outer_hs.len()];
    let mut col_fill = vec![0.0f32; n_cols];
    let mut cur_col = 0usize;
    for (j, &oh) in outer_hs.iter().enumerate() {
        let height_overflow = col_fill[cur_col] + oh > target_h && oh > 0.0;
        // Never advance past an empty column: a column must hold at least one item
        // before overflowing to the next, otherwise an item taller than target_h
        // would skip column 0 and leave it blank (CSS Multicol §3.4 — every column
        // box is filled in order, starting from the first).
        let col_nonempty = col_fill[cur_col] > 0.0;
        if cur_col + 1 < n_cols && col_nonempty && height_overflow {
            cur_col += 1;
        }
        col_assignment[j] = cur_col;
        col_fill[cur_col] += oh;
    }
    frame.col_assignment = col_assignment;
    frame.col_y = vec![cur_y; n_cols];
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
    frame.init.cur_y = frame.col_y.iter().copied().fold(frame.init.cur_y, f32::max);
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
    frame.init.cur_y = new_cur_y;
    frame.init.out.push(placed);
    frame.init.consumed[span_i] = true;
}

/// Runs once every segment is placed — (moved in from `layout_dispatch.rs`'s
/// former post-`lay_out_multicol_children` code) rebuilds `b.children` from
/// the placed/fragmented boxes plus any never-consumed non-flow boxes
/// (absolute/fixed, `Skip` placeholders, copied unchanged), then the
/// container's own height (`finalize_block_height`) and the same
/// `finish_after_match` tail (`position: relative` offset; `abs_deferred` is
/// always empty here — a multicol container's own absolutely-positioned
/// children were never collected into it, a pre-existing gap this slice
/// reproduces unchanged, not introduces; see `build_multicol_init`'s
/// `flow_idxs` filter) that the removed code fell through to.
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
        frame.init.field_intrinsic, content_height,
    );
    let empty_abs_deferred: Vec<(usize, f32, f32)> = Vec::new();
    finish_after_match(
        &mut frame.b, &frame.init.s, frame.init.em, frame.init.cb, frame.init.is_positioned,
        frame.init.own_pcb, &empty_abs_deferred, measurer, viewport, hp,
    );
}
