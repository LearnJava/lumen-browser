use super::*;
use super::layout_cache::finalize_block_height;
use super::line_clamp_flow;
use super::layout_dispatch::{dispatch_box, finish_after_match, FontContext};
use super::shape_resolve::{resolve_shape, FloatBoxes};

/// LAYOUT-2 срез 1 — see `dispatch_box`'s doc comment (`layout_dispatch.rs`)
/// for why this split exists. `Done` matches every dispatch arm that already
/// fully laid out `b` (leaf/replaced boxes, flex/grid/svg/vertical/multicol/
/// table/InlineBlockRow containers) — behaviorally identical to the
/// pre-LAYOUT-2 function returning normally. `NeedsBlockFlowLoop` is the one
/// case that used to recurse non-tail-call into `lay_out_inner` for each
/// normal-flow child (CSS 2.1 §9.5/§8.3.1's block-flow branch) — [`run`]
/// drives it (and every further plain-block descendant) iteratively instead.
pub(super) enum DispatchOutcome {
    Done,
    // Boxed: `BlockFlowInit` carries a `FloatContext` + several `Rect`s, ~300
    // bytes — large enough that leaving it inline would size every `Done` the
    // same, on the hot path every dispatch call returns through.
    NeedsBlockFlowLoop(Box<BlockFlowInit>),
    // LAYOUT-2 срез 3: the flex dispatch arm's item-placement loop (final
    // placement pass, CSS Flexbox L1 §9.5/§9.6) — reads each item's `.rect`
    // back for cross-axis alignment (column) or leaves it for the separate
    // cross-align sub-pass (row), so it is non-tail-recursive the same way the
    // block-flow branch is. `super::flex_trampoline::run` drives it (and every
    // further flex-container descendant it meets) on an explicit heap stack.
    NeedsFlexLoop(Box<super::flex_trampoline::FlexInit>),
    // LAYOUT-2 срез 4: the grid dispatch arm's probe + final-placement passes
    // (CSS Grid L1 §12.3/§11.2) — read each item's `.rect` back for row-height
    // growth and cell alignment, so it is non-tail-recursive the same way the
    // flex/block-flow branches are. `super::grid_trampoline::run` drives it
    // (and every further grid-container descendant it meets, incl. subgrid)
    // on an explicit heap stack.
    NeedsGridLoop(Box<super::grid_trampoline::GridInit>),
    // LAYOUT-2 срез 6: the table dispatch arm's per-cell placement (CSS 2.1
    // §17.5) — reads each cell's `.rect` back for row-height normalisation,
    // rowspan bookkeeping and border-collapse offset, so it is non-tail-
    // recursive the same way the other three branches are.
    // `super::table_trampoline::run` drives it (and every further table
    // descendant a cell meets, e.g. `<td><table>…`) on an explicit heap stack.
    NeedsTableLoop(Box<super::table_trampoline::TableInit>),
    // LAYOUT-2 срез 7: the multicol dispatch arm's per-segment placement pass
    // (CSS Multicol §3.4) — unconditionally reads each item's `.rect` back
    // (Measure pass, every segment) and, for atomic segments, dispatches
    // again at the resolved column position and reads it a second time
    // (Place pass) — non-tail-recursive the same way the other four branches
    // are. `super::multicol_trampoline::run` drives it (and every further
    // multicol-container descendant it meets) on an explicit heap stack.
    NeedsMulticolLoop(Box<super::multicol_trampoline::MulticolInit>),
    // LAYOUT-2 срез 8: the vertical-writing-mode dispatch arm's per-child
    // block-axis stacking loop (CSS Writing Modes L3 §3) — reads each
    // child's `.rect` back to reposition it to its true physical x and to
    // advance the block-axis cursor, so it is non-tail-recursive the same
    // way the other five branches are. `super::vertical_trampoline::run`
    // drives it (and every further vertical-writing-mode descendant it
    // meets) on an explicit heap stack.
    NeedsVerticalLoop(Box<crate::vertical::VerticalInit>),
}

/// Loop-entry state for the plain block-flow branch, captured by `dispatch_box`
/// before any child is processed — everything the removed inline loop
/// (pre-LAYOUT-2 `layout_dispatch.rs`) read or wrote across iterations or in
/// its post-loop epilogue. `fc`/`child_y`/`prev_block_mb`/`seen_inflow_child`/
/// `inside_marker_w`/`abs_deferred` mutate per child inside `run`; the rest are
/// read-only invariants for this box's whole loop.
#[derive(Clone)]
pub(super) struct BlockFlowInit {
    pub(super) fc: FloatContext,
    /// CSS 2.1 §9.5/§10.6.3: a non-BFC in-flow block shares its parent's formatting
    /// context — it does not enclose its floats, its parent takes them over.
    pub(super) shares_floats: bool,
    pub(super) container_right: f32,
    pub(super) child_y: f32,
    pub(super) prev_block_mb: f32,
    pub(super) b_collapses_top: bool,
    pub(super) b_collapses_bottom: bool,
    pub(super) seen_inflow_child: bool,
    pub(super) inside_marker_w: f32,
    /// Line height of the pending inside marker — consumed together with
    /// `inside_marker_w` when the first in-flow child is block-level.
    pub(super) inside_marker_h: f32,
    /// The pending `inside` text marker as an inline segment, and the index of its
    /// `BoxKind::Marker` box — consumed with `inside_marker_w` when the first in-flow
    /// child is an inline run, which then lays the marker out as its first fragment.
    pub(super) inside_marker_seg: Option<(usize, InlineSegment)>,
    pub(super) abs_deferred: Vec<(usize, f32, f32)>,
    pub(super) s: Arc<ComputedStyle>,
    pub(super) em: f32,
    pub(super) cb: f32,
    pub(super) content_x: f32,
    pub(super) content_y: f32,
    pub(super) content_width: f32,
    pub(super) children_pcb: Rect,
    pub(super) children_available_height: Option<f32>,
    pub(super) is_positioned: bool,
    pub(super) pcb: Rect,
    pub(super) padding_top: f32,
    pub(super) padding_bottom: f32,
    pub(super) size_contained: bool,
    pub(super) field_intrinsic: Option<(f32, f32)>,
    pub(super) available_height: Option<f32>,
}

/// One level of the explicit stack `run` maintains in place of the native call
/// stack. `b` is owned — swapped out of its parent's `children[idx]` via
/// `take_box` (or, for the root, out of the caller's `&mut LayoutBox`) for the
/// duration of this box's own loop, and swapped back once it finishes. Without
/// this move there would be two live `&mut LayoutBox` into the same tree at
/// once (the parent's slot and this frame's box) — exactly what native
/// recursion safely nests via the call stack, and what an explicit `Vec<Frame>`
/// cannot borrow-check its way to without owning each level outright.
struct Frame {
    b: LayoutBox,
    init: Box<BlockFlowInit>,
    next_child_idx: usize,
    // Pre-recursion bookkeeping for the child at `next_child_idx`, computed in
    // `step_child` right before the recursive step and consumed by
    // `post_child_bookkeeping` once that child (whether resolved synchronously
    // or via a resumed descent) is fully laid out.
    pending_is_block: bool,
    pending_collapsed_mt: f32,
}

/// Swaps `slot` out for an inert placeholder and returns its previous value —
/// the LAYOUT-1 срез 2 `mem::take` idiom (see `LayoutBox`'s `Drop` impl in
/// `box_tree/types.rs`) applied to a single tree node instead of a whole
/// `children` vector, so the slot stays a valid `LayoutBox` for however long
/// the real value is owned by a `Frame` elsewhere. The placeholder is never
/// observed by any real layout logic — it is always overwritten by the true
/// (fully laid out) value before `run` looks at that slot again.
pub(super) fn take_box(slot: &mut LayoutBox) -> LayoutBox {
    let placeholder = LayoutBox {
        anchor_scroll: None, cv_hidden: Vec::new(),
        node: slot.node,
        rect: Rect::ZERO,
        style: Arc::clone(&slot.style),
        used_padding: None, used_line_height: 0.0,
        grid_baselines: None,
        fieldset_legend: None,
        table_caption_inset: [0.0; 2],
        clamp_clip: None,
        subgrid_tracks: None, used_grid_tracks: None,
        kind: BoxKind::Skip,
        children: Vec::new(),
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: crate::incremental::DirtyBits::default(),
        origin: slot.origin,
    };
    std::mem::replace(slot, placeholder)
}

/// Drives the plain block-flow branch (`init`) and every further plain-block
/// descendant it meets on an explicit heap stack, so a `<div>`×20000 chain no
/// longer grows the native call stack one frame per level (LAYOUT-2's
/// acceptance criterion for this item). `b` is the box `dispatch_box` was
/// originally called on. Float placement's probe/final `lay_out` calls, and
/// every non-block-flow dispatch arm a normal-flow child resolves to, still
/// recurse on the native stack — out of scope for this slice (see the other
/// five dispatchers LAYOUT-2's ROADMAP entry leaves for later slices).
pub(super) fn run(
    b: &mut LayoutBox,
    init: Box<BlockFlowInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> FloatContext {
    if !line_clamp_flow::wants_balance_pass(b) {
        return run_pass(b, init, measurer, viewport, hp);
    }
    run_balanced(b, init, measurer, viewport, hp)
}

/// `run` of a `line-clamp` container with balanced runs inside. Out of line so
/// the pristine copy it keeps stays off `run`'s frame, which every table/flex/
/// grid ↔ block-flow nesting level pays for.
#[inline(never)]
fn run_balanced(
    b: &mut LayoutBox,
    init: Box<BlockFlowInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> FloatContext {
    // `line-clamp` + `text-wrap: balance` inside: the visible lines of each balanced run are
    // known only after a layout without balancing, so the container is laid out twice.
    let (pristine, pristine_init) = (b.clone(), init.clone());
    let saved = line_clamp_flow::begin_greedy_pass();
    run_pass(b, init, measurer, viewport, hp);
    line_clamp_flow::begin_budget_pass(b);
    *b = pristine;
    let fc = run_pass(b, pristine_init, measurer, viewport, hp);
    line_clamp_flow::end_pass(saved);
    fc
}

fn run_pass(
    b: &mut LayoutBox,
    init: Box<BlockFlowInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> FloatContext {
    // Frames live on the heap (`Box<Frame>`): `run_pass` sits on the native
    // stack once per table/flex/grid <-> block-flow nesting level, so a frame
    // held by value (a whole `LayoutBox`) would be paid on every such level.
    let mut current = enter(b, init);
    let mut stack: Vec<Box<Frame>> = Vec::new();
    // BUG-1026: shared for the whole `run()` pass, not per-frame — a deep
    // single-child chain is exactly one first-child chain spanning every
    // frame this loop pushes, so the memo only pays off if levels share it.
    let mut top_cache = MarginCollapseCache::default();
    let mut bottom_cache = MarginCollapseCache::default();

    loop {
        if current.next_child_idx >= current.b.children.len() {
            finish_frame(&mut current, measurer, viewport, hp, &mut bottom_cache);
            match stack.pop() {
                None => return leave(b, current),
                Some(mut parent) => {
                    return_to_parent(&mut parent, current, viewport, &mut bottom_cache);
                    parent.next_child_idx += 1;
                    current = parent;
                }
            }
            continue;
        }

        let i = current.next_child_idx;
        match step_child(&mut current, i, measurer, viewport, hp, &mut top_cache, &mut bottom_cache) {
            StepOutcome::Advance => {
                current.next_child_idx += 1;
            }
            StepOutcome::Foreign(outcome) => {
                run_foreign_child(&mut current, i, outcome, measurer, viewport, hp, &mut bottom_cache);
                current.next_child_idx += 1;
            }
            StepOutcome::Descend(child_init) if line_clamp_flow::wants_balance_pass(&current.b.children[i]) => {
                // A clamp container with balanced runs is laid out twice: `run` does that for a
                // whole box, so it is driven recursively instead of as a frame of this stack.
                let shares = child_init.shares_floats;
                let child_fc = run(&mut current.b.children[i], child_init, measurer, viewport, hp);
                if shares {
                    current.init.fc.absorb_vertical(&child_fc, 0.0, 0.0);
                }
                post_child_bookkeeping(&mut current, i, viewport, &mut bottom_cache);
                current.next_child_idx += 1;
            }
            StepOutcome::Descend(child_init) => {
                let child_frame = enter(&mut current.b.children[i], child_init);
                stack.push(current);
                current = child_frame;
            }
        }
    }
}

/// A heap frame owning `slot`'s box (see `run_pass`). Out of line, like
/// [`leave`] and [`return_to_parent`]: the `LayoutBox` moves they make stay off
/// `run_pass`'s own frame.
#[inline(never)]
fn enter(slot: &mut LayoutBox, init: Box<BlockFlowInit>) -> Box<Frame> {
    Box::new(Frame { b: take_box(slot), init, next_child_idx: 0, pending_is_block: false, pending_collapsed_mt: 0.0 })
}

/// Puts the root frame's box back into `b` and returns its float context.
#[inline(never)]
fn leave(b: &mut LayoutBox, frame: Box<Frame>) -> FloatContext {
    let Frame { b: laid_out, init, .. } = *frame;
    *b = laid_out;
    init.fc
}

/// Puts a finished child frame's box back into its parent's slot and does the
/// parent's post-child bookkeeping.
#[inline(never)]
fn return_to_parent(parent: &mut Frame, child: Box<Frame>, viewport: Size, bottom_cache: &mut MarginCollapseCache) {
    let idx = parent.next_child_idx;
    let Frame { b: laid_out, init, .. } = *child;
    if init.shares_floats {
        parent.init.fc.absorb_vertical(&init.fc, 0.0, 0.0);
    }
    parent.b.children[idx] = laid_out;
    post_child_bookkeeping(parent, idx, viewport, bottom_cache);
}

enum StepOutcome {
    /// This child is fully handled (abs-deferred, `::marker`/float placed
    /// inline, or a normal-flow child whose own dispatch completed
    /// synchronously) — move on to the next index.
    Advance,
    /// This child is itself a plain block-flow box with its own children to
    /// process — push the current frame and continue processing this one.
    Descend(Box<BlockFlowInit>),
    /// This child is a flex/grid/table/multicol/vertical container: its own
    /// trampoline runs from `run_pass` (`run_foreign_child`), after
    /// `step_child`'s frame is gone — see `run_foreign_child`.
    Foreign(DispatchOutcome),
}

/// Drives a normal-flow child whose dispatch resolved to another kind's
/// trampoline, then does the post-child bookkeeping. Out of line and called
/// from `run_pass`, not from `step_child`: a kind transition (block → table →
/// block → …, a `<td>` holding a `<table>`) still costs native frames per
/// level, and `step_child`'s frame is by far the largest of them — keeping it
/// off that chain is what lets `tests::table_trampoline`'s 500-level chain fit
/// a test thread's 2 MiB stack.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_foreign_child(
    frame: &mut Frame,
    i: usize,
    outcome: DispatchOutcome,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    bottom_cache: &mut MarginCollapseCache,
) {
    let child = &mut frame.b.children[i];
    match outcome {
        DispatchOutcome::NeedsFlexLoop(ci) => super::flex_trampoline::run(child, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsGridLoop(ci) => super::grid_trampoline::run(child, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsTableLoop(ci) => super::table_trampoline::run(child, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsMulticolLoop(ci) => super::multicol_trampoline::run(child, ci, measurer, viewport, hp),
        DispatchOutcome::NeedsVerticalLoop(ci) => super::vertical_trampoline::run(child, ci, measurer, viewport, hp),
        DispatchOutcome::Done | DispatchOutcome::NeedsBlockFlowLoop(_) => {
            unreachable!("step_child handles Done/NeedsBlockFlowLoop itself")
        }
    }
    post_child_bookkeeping(frame, i, viewport, bottom_cache);
}

/// Handles exactly one child of `frame.b` at `i` — abs-positioned deferral,
/// `::marker` placement, float placement/clearance, or (falling through to
/// the CSS 2.1 §9.5/§8.3.1 normal-flow case) resolving the child's effective
/// position and recursing via `dispatch_box`. Copied verbatim from the removed
/// inline loop body (pre-LAYOUT-2 `layout_dispatch.rs`) except at that one
/// recursive call, which now either finishes synchronously (`Advance`, doing
/// the same post-child bookkeeping the old loop did right after the call) or
/// hands back the child's `BlockFlowInit` for `run` to push and descend into.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn step_child(
    frame: &mut Frame,
    i: usize,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    top_cache: &mut MarginCollapseCache,
    bottom_cache: &mut MarginCollapseCache,
) -> StepOutcome {
    let content_x = frame.init.content_x;
    let content_width = frame.init.content_width;
    let container_right = frame.init.container_right;
    let children_available_height = frame.init.children_available_height;
    let children_pcb = frame.init.children_pcb;

    // Rendered legend уже стоит на границе fieldset (`place_rendered_legend`).
    if frame.b.fieldset_legend.is_some_and(|l| l.placed && l.idx == i) {
        return StepOutcome::Advance;
    }

    if matches!(frame.b.children[i].style.position, Position::Absolute | Position::Fixed) {
        let child_y = frame.init.child_y;
        let static_x = if super::multicol_abspos::static_x_is_right_edge(&frame.b) { container_right } else { content_x };
        frame.init.abs_deferred.push((i, static_x, child_y));
        return StepOutcome::Advance;
    }

    // CSS Lists L3 §2.4 — position ::marker outside or inside principal block.
    if matches!(&frame.b.children[i].kind, BoxKind::Marker { .. }) {
        let child_y = frame.init.child_y;
        let child = &mut frame.b.children[i];
        let (position, em, marker_text, has_image) =
            if let BoxKind::Marker { position, text, in_run, image, .. } = &mut child.kind {
                // Re-decided on every layout pass, see `inside_marker_seg`.
                *in_run = false;
                (*position, child.style.font_size, text.clone(), image.is_some())
            } else { unreachable!() };
        let line_h = child.used_line_height;
        // CSS Lists L3 §2.4 — the outside marker occupies the area to the
        // left of the principal box. The default box is `em * 1.5`; a text
        // marker (counter glyph or `::marker { content }`) wider than that —
        // e.g. a custom `@counter-style` with a long prefix/suffix like
        // "#1: " — must grow the box leftward so its string right-aligns at
        // the content edge instead of overflowing into the first word
        // ("#1:One" instead of "#1: One" — BUG-185).
        let default_w = em * 1.5;
        let text_w = if marker_text.is_empty() {
            0.0
        } else {
            measurer.map_or(0.0, |m| {
                let fams = &child.style.font_family;
                let ts = super::inline_wrap::TabStops::of(&child.style, m).unit;
                measure_text_w_families(
                    &marker_text, em, child.style.letter_spacing, ts, fams, m,
                )
            })
        };
        // CSS Lists L3 §3.1: an `inside` marker is an ordinary inline box, so
        // the line continues right after its text; the `1.5em` floor is only a
        // gutter for `outside` markers and glyph (bullet) markers without text
        // (BUG-1548).
        let marker_w = if position == ListStylePosition::Inside && text_w > 0.0 {
            text_w
        } else {
            default_w.max(text_w) // CSS: list-style-type determines exact width
        };
        match position {
            ListStylePosition::Outside => {
                // Out of flow: does not advance child_y.
                // Snap to integer CSS pixels — em*1.5 is often fractional (BUG-083).
                child.rect = Rect::new(
                    (content_x - marker_w).round(),
                    child_y.round(),
                    marker_w.round(),
                    line_h.round(),
                );
            }
            ListStylePosition::Inside => {
                // CSS Lists L3 §2.4: inside marker shares the first line with
                // content. Place at content_x; record indent for the next child.
                // A text marker is laid out like the inline fragment it stands for:
                // same `x`, and the half-leading (CSS 2.1 §10.8.1) above its glyphs
                // that `apply_inline_vertical_align` gives the first line's text —
                // otherwise the marker sits a pixel above the baseline of the text
                // that follows it. Bullet markers keep the integer-snapped box.
                child.rect = if marker_text.is_empty() {
                    Rect::new(content_x.round(), child_y.round(), marker_w, line_h.round())
                } else {
                    let half_leading = (line_h - em) / 2.0;
                    Rect::new(content_x, child_y + half_leading, marker_w, line_h)
                };
                frame.init.inside_marker_w = marker_w;
                frame.init.inside_marker_h = child.rect.height;
                frame.init.inside_marker_seg = (!marker_text.is_empty() && !has_image)
                    .then(|| (i, inline_marker_segment(child, &marker_text)));
                // Do NOT advance child_y — marker is inline with content.
            }
        }
        return StepOutcome::Advance;
    }

    // CSS 2.1 §9.5.2: clear — advance child_y past relevant floats.
    // Clearance is inserted between the top margin and the top border, so the
    // final border edge ends up at max(natural-flow border, float bottom): the
    // top margin is *absorbed* by clearance, not stacked on top of the float
    // bottom. `clearance_pre` remembers the pre-clear flow position so the
    // start_y computation below can place the border at that maximum (fixes the
    // double-count where a cleared block dropped to float_bottom + margin_top).
    let clearance_pre = {
        let child = &frame.b.children[i];
        if !frame.init.fc.is_empty() && child.style.clear != ClearSide::None {
            let pre = frame.init.child_y;
            frame.init.child_y = frame.init.fc.clear_y(frame.init.child_y, child.style.clear);
            Some(pre)
        } else {
            None
        }
    };

    // CSS 2.1 §9.5.1: float box — placed out of normal flow. Still recurses on
    // the native stack via `lay_out` for the (at most two) trial layouts — out
    // of scope for this slice, see LAYOUT-2's ROADMAP entry.
    if frame.b.children[i].style.float_side != FloatSide::None {
        place_float(frame, i, measurer, viewport, hp);
        // `clear` moves only the float itself (CSS 2.1 §9.5.2); the in-flow
        // position of the following siblings stays where it was (BUG-1359).
        if let Some(pre_clear_y) = clearance_pre {
            frame.init.child_y = pre_clear_y;
        }
        return StepOutcome::Advance;
    }

    // CSS Lists L3 §2.4 — `list-style-position: inside`: the marker is an
    // inline box at the start of the principal box's first line.
    //  * first in-flow child is an inline run → the marker shares line 0:
    //    that line alone is inset by the marker width (`first_line_inset`,
    //    like `text-indent`), wrapped lines return to the content edge;
    //  * first in-flow child is block-level → the marker sits alone in an
    //    anonymous line box above it, so the block starts one marker line
    //    lower at the full content width (margins do not collapse through
    //    the marker's line box);
    //  * anything else (inline-block row, replaced) keeps the historical
    //    horizontal shift of the child.
    let inside_marker_w = std::mem::take(&mut frame.init.inside_marker_w);
    let inside_marker_h = std::mem::take(&mut frame.init.inside_marker_h);
    let inside_marker_seg = frame.init.inside_marker_seg.take();
    let mut marker_shift = 0.0;
    if inside_marker_w > 0.0 {
        let child = &mut frame.b.children[i];
        let mut marker_in_run = None;
        match &mut child.kind {
            BoxKind::InlineRun { segments, first_line_inset, .. } => match inside_marker_seg {
                // The marker's text is the run's first fragment: it is shaped,
                // split at bidi boundaries and positioned like the text after it.
                Some((marker_idx, seg)) => {
                    match segments.first_mut() {
                        Some(first) if first.pseudo_kind == PseudoKind::Marker => *first = seg,
                        _ => segments.insert(0, seg),
                    }
                    *first_line_inset = 0.0;
                    marker_in_run = Some(marker_idx);
                }
                None => *first_line_inset = inside_marker_w,
            },
            BoxKind::Block | BoxKind::FlowRoot | BoxKind::Table => {
                frame.init.child_y += inside_marker_h;
                frame.init.prev_block_mb = 0.0;
                frame.init.seen_inflow_child = true;
            }
            _ => marker_shift = inside_marker_w,
        }
        if let Some(idx) = marker_in_run
            && let BoxKind::Marker { in_run, .. } = &mut frame.b.children[idx].kind
        {
            *in_run = true;
        }
    }
    // CSS Text L3 §7.1: `text-indent` belongs to the container's first line; a run that
    // follows an in-flow block (`<br>`, nested `<div>`) continues the container.
    if frame.init.seen_inflow_child
        && matches!(frame.b.children[i].kind, BoxKind::InlineRun { .. })
        && !frame.b.children[i].style.text_extra.indent_continuation
    {
        Arc::make_mut(&mut frame.b.children[i].style).text_extra.indent_continuation = true;
    }
    let child_y = frame.init.child_y;
    // Normal flow: narrow x/width for active floats.
    let flow_left  = frame.init.fc.left_edge_in_flow(child_y, content_x);
    let flow_right = frame.init.fc.right_edge_in_flow(child_y, container_right);
    let (mut eff_left, mut eff_w) = {
        let l = flow_left + marker_shift;
        (l, (flow_right - l).max(0.0))
    };
    // CSS 2.1 §9.5: a block-level box in normal flow is NOT narrowed by
    // floats — its width and margins resolve against the full containing
    // block and only its line boxes are shortened.
    //
    // `outer_for_child` carries this block's float context down into an
    // in-flow non-BFC child so its (and its descendants') line boxes are
    // shortened by the active floats — instead of the box itself being
    // narrowed/clipped (the legacy approximation).
    let mut outer_for_child: Option<&FloatContext> = None;
    {
        let child = &frame.b.children[i];
        // CSS Shapes L1 §3: a paragraph beside a float with a `shape-outside`
        // spans the full containing block and wraps per line box against the
        // contour (`inline_shape_wrap`) instead of the float's margin rectangle.
        if matches!(child.kind, BoxKind::InlineRun { .. })
            && frame.init.fc.shapes_bottom() > child_y
        {
            eff_left = content_x + marker_shift;
            eff_w = (content_width - marker_shift).max(0.0);
            outer_for_child = Some(&frame.init.fc);
        } else if (flow_left > content_x || flow_right < container_right)
            && child.style.width.is_none()
            && matches!(child.kind, BoxKind::Block)
            && !establishes_bfc(child)
        {
            if has_in_flow_content(child) {
                // Auto-width non-BFC block with content beside a float: keep the
                // full containing-block width and propagate the float context so
                // the child's line boxes recede past the float (CSS 2.1 §9.5).
                eff_left = content_x;
                eff_w = content_width;
                outer_for_child = Some(&frame.init.fc);
            } else {
                // *Empty* auto-width block (no in-flow content to reflow): resolve
                // geometry against the full content width, then clip the result to
                // the non-float band. This keeps the visual identical when the box
                // would overlap a float (Lumen paints floats in source order, so the
                // clip stands in for float-over-block painting), while restoring a
                // margin'd box that fits in the gap between two floats — which the
                // naive narrowing collapsed to zero width.
                let cem = child.style.font_size;
                let ml = child.style.margin_left.resolve_or_zero(cem, content_width, viewport);
                let mr = child.style.margin_right.resolve_or_zero(cem, content_width, viewport);
                let bw = (content_width - ml - mr).max(0.0);
                let nat_x = content_x + ml;
                let vx = nat_x.max(flow_left);
                let vw = ((nat_x + bw).min(flow_right) - vx).max(0.0);
                // Reproduce the clipped border-box through lay_out's margin re-add:
                // it places x at eff_left + ml and width at eff_w − ml − mr.
                eff_left = vx - ml;
                eff_w = vw + ml + mr;
            }
        }
    }

    // CSS 2.1 §8.3.1: collapse adjacent sibling block margins.
    // Block/FlowRoot/Table participate; other kinds break the chain. A `Table`
    // box is block-level and its (wrapper) margins collapse with adjacent
    // sibling margins like a normal block, even though it establishes a BFC for
    // its own contents (so `collapsed_top_margin`/`collapsed_bottom_margin`
    // return its own margin without folding into its rows — see those fns).
    // `own_mt` is the child's own resolved top margin (what lay_out re-adds
    // internally); `collapsed_mt` additionally folds the child's own first-child
    // chain (§8.3.1). The base formula offsets start_y by (collapsed_mt − own_mt)
    // so that lay_out's internal "+own_mt" lands the child at its collapsed flow
    // position child_y + max(prev_block_mb, collapsed_mt).
    let (is_block, collapsed_mt, start_y, child_is_root_element) = {
        let child = &frame.b.children[i];
        let is_block = matches!(&child.kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Table);
        let is_first_inflow = !frame.init.seen_inflow_child;
        let own_mt = child.style.margin_top
            .resolve_or_zero(child.style.font_size, eff_w, viewport);
        // CSS 2.1 §8.3.1: the margins of the root element's box do not collapse.
        // When this container is the document box (`NodeId` index 0), its first
        // in-flow block child IS the root element, so the parent↔first-child collapse
        // chain must terminate there: a descendant's escaping top margin must not
        // shift the root element (and the propagated canvas background it backs) off
        // the viewport origin. Laying it out with `in_block_flow == false` also stops
        // it from flush-collapsing its own first child, so that child's collapsed
        // margin stays inside the root box (BUG-153 — restores the 1px magenta frame
        // top edge that BUG-151's collapse-through regressed).
        let child_is_root_element =
            frame.b.node.index() == 0 && is_first_inflow && is_block;
        let collapsed_mt = if child_is_root_element {
            own_mt
        } else {
            collapsed_top_margin(child, eff_w, viewport, top_cache)
        };
        let start_y = if let Some(pre_clear_y) = clearance_pre {
            // CSS 2.1 §9.5.2: a cleared block's border edge sits at the larger of
            // its natural flow position (margin included) and the cleared float
            // bottom (`child_y`, advanced by clear_y above). Clearance fills any
            // gap; the margin is not added a second time on top of the float
            // bottom. `natural_border` is the pre-clearance border-top.
            let natural_border = pre_clear_y
                - frame.init.prev_block_mb.min(collapsed_mt.max(0.0)) + collapsed_mt;
            natural_border.max(frame.init.child_y) - own_mt
        } else if is_block {
            if is_first_inflow
                && frame.init.b_collapses_top
                && matches!(child.kind, BoxKind::Block)
                && child.style.clear == ClearSide::None
            {
                // Parent↔first-child collapse: the margin escaped up into this box's
                // own (already-applied) top margin. Place the child flush at the
                // content top; lay_out re-adds own_mt, so pre-subtract it.
                frame.init.content_y - own_mt
            } else {
                frame.init.child_y - frame.init.prev_block_mb.min(collapsed_mt.max(0.0))
                    + collapsed_mt - own_mt
            }
        } else {
            frame.init.child_y
        };
        (is_block, collapsed_mt, start_y, child_is_root_element)
    };

    frame.pending_is_block = is_block;
    frame.pending_collapsed_mt = collapsed_mt;

    let justify_items = frame.init.s.justify_items;
    let child = &mut frame.b.children[i];
    // Writing Modes L3 §7.3.1 (FLEX-VWM-4): an in-flow orthogonal block whose
    // containing block has no definite height shrinks to its content along the
    // inline axis (physical height) instead of filling the initial containing
    // block. The root element is not one: its containing block is the ICB itself.
    // A vertical flex container sizes itself from its items (`flex_trampoline`).
    let children_available_height = if children_available_height.is_none() && !child_is_root_element && is_block {
        super::intrinsic::orthogonal_fit_content_height(child, eff_w, measurer, viewport, true)
    } else {
        children_available_height
    };
    match dispatch_box(
        child, eff_left, start_y, eff_w, children_available_height, measurer, viewport,
        children_pcb, hp, !child_is_root_element, outer_for_child, justify_items, None,
    ) {
        DispatchOutcome::Done => {
            post_child_bookkeeping(frame, i, viewport, bottom_cache);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsBlockFlowLoop(child_init) => StepOutcome::Descend(child_init),
        // LAYOUT-2 срез 3: a block-flow normal-flow child that is itself a
        // flex container. `StepOutcome::Descend` is typed for this loop's own
        // `BlockFlowInit`, so composing a flex chain onto the SAME stack is
        // out of scope here (a flex/block/flex chain still recurses one
        // native frame per flex↔block transition) — `flex_trampoline::run`
        // drives its own chain synchronously instead, exactly mirroring how
        // `flex_trampoline::step_item` calls back into `run` above for a
        // `NeedsBlockFlowLoop` child.
        outcome @ DispatchOutcome::NeedsFlexLoop(_) => StepOutcome::Foreign(outcome),
        // LAYOUT-2 срез 4: a block-flow normal-flow child that is itself a
        // grid container — same shape as the flex arm above.
        outcome @ DispatchOutcome::NeedsGridLoop(_) => StepOutcome::Foreign(outcome),
        // LAYOUT-2 срез 6: a block-flow normal-flow child that is itself a
        // table — same shape as the flex/grid arms above.
        outcome @ DispatchOutcome::NeedsTableLoop(_) => StepOutcome::Foreign(outcome),
        // LAYOUT-2 срез 7: a block-flow normal-flow child that is itself a
        // multicol container — same shape as the flex/grid/table arms above.
        outcome @ DispatchOutcome::NeedsMulticolLoop(_) => StepOutcome::Foreign(outcome),
        // LAYOUT-2 срез 8: a block-flow normal-flow child that is itself a
        // vertical-writing-mode container — same shape as the flex/grid/
        // table/multicol arms above.
        DispatchOutcome::NeedsVerticalLoop(mut child_init) => {
            // An orthogonal flow is an independent formatting context: its margins do
            // not collapse with its own children's, and it encloses its floats.
            child_init.collapses_start = false;
            child_init.collapses_end = false;
            child_init.encloses_floats = true;
            StepOutcome::Foreign(DispatchOutcome::NeedsVerticalLoop(child_init))
        }
    }
}

/// The text of an `inside` marker as the first inline segment of the list item's run
/// (CSS Lists L3 §3.1). The UA's `tabular-nums` only matters for Latin digits: without
/// any, the feature is dropped so the shaper keeps the plain glyph run — a marker such
/// as `א. ` or `১. ` then shapes exactly like the same text typed into the line (with
/// `tnum` on, the bundled fallback font moves the full stop).
fn inline_marker_segment(marker: &LayoutBox, text: &str) -> InlineSegment {
    let mut style = (*marker.style).clone();
    if !text.chars().any(|c| c.is_ascii_digit()) {
        style.font_variant_numeric.spacing = crate::style::NumericSpacing::Normal;
    }
    super::inline_build::make_content_text_segment(&style, marker.node, text.to_string(), PseudoKind::Marker)
}

/// CSS 2.1 §9.5.1 — float box placement, copied verbatim from the removed
/// inline loop body. Still recurses on the native stack (via `lay_out`) for
/// its own (at most two) trial layouts of `child` — a chain of nested floats
/// is not the acceptance criterion this slice targets, and is left for a
/// later LAYOUT-2 slice alongside flex/grid/table/multicol/vertical.
#[allow(clippy::too_many_arguments)]
fn place_float(
    frame: &mut Frame,
    i: usize,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let content_x = frame.init.content_x;
    let content_width = frame.init.content_width;
    let container_right = frame.init.container_right;
    let children_available_height = frame.init.children_available_height;
    let children_pcb = frame.init.children_pcb;
    let child_y = frame.init.child_y;

    let fc = &mut frame.init.fc;
    let child = &mut frame.b.children[i];

    let cem = child.style.font_size;
    // CSS 2.1 §10.3.5 shrink-to-fit applies to the float's *inline* size, which
    // runs along y in a vertical writing mode: offer no more height than the
    // box's max-content one (an auto-height vertical float used to fill it all).
    let children_available_height = if !matches!(child.style.writing_mode, crate::style::WritingMode::HorizontalTb)
        && child.style.height.is_none()
        && matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot)
    {
        let room = children_available_height.unwrap_or(viewport.height);
        Some(room.min(max_content_outer_height(child, measurer, viewport)))
    } else {
        children_available_height
    };
    // Shrink-to-fit width (CSS 2.1 §10.3.5): explicit CSS width wins;
    // otherwise preferred content width, falling back to max-content
    // measurement for text-only floats (e.g. the ::first-letter drop-cap box,
    // BB-2), clamped to available space. `probe_w` decides the float's box at
    // the *current* line; the outer width is then used to test whether the
    // float fits or must drop (rule 8 below).
    let probe_avail = {
        let l = fc.left_edge_at(child_y, content_x);
        let r = fc.right_edge_at(child_y, container_right);
        (r - l).max(0.0)
    };
    // CSS 2.1 §10.3.5 / §8.3: an explicit (incl. percentage) width and its
    // percentage margins/padding resolve against the float's containing block
    // — the same block the float would use if it weren't floated — not
    // against the (possibly float-narrowed) space at the current line. Using
    // the narrowed `probe_avail` here made `width:100%` collapse to near-zero
    // when squeezed next to prior floats, so it never dropped to a new line
    // under rule 8 below and poisoned every later `clear_y` computation that
    // depended on its true bottom edge (BUG-469).
    // `lay_out` treats its `available_width` as the room for the margin box, so
    // the shrink-to-fit border-box width has to be handed over with the
    // float's horizontal margins added back.
    // A bare text run (the `::first-letter` box) takes `available_width` as its
    // own width, margins not included.
    let probe_margins = if matches!(child.kind, BoxKind::InlineRun { .. }) {
        0.0
    } else {
        child.style.margin_left.resolve_or_zero(cem, probe_avail, viewport)
            + child.style.margin_right.resolve_or_zero(cem, probe_avail, viewport)
    };
    let probe_w = if child.style.width.is_some() {
        content_width
    } else {
        preferred_inline_block_width(child, measurer, viewport)
            .or_else(|| {
                let w = max_content_outer_width(child, measurer, viewport);
                (w > 0.0).then_some(w)
            })
            .map(|pw| (pw + probe_margins).min(probe_avail))
            .unwrap_or(probe_avail)
    };
    lay_out(child, fc.left_edge_at(child_y, content_x), child_y, probe_w,
            children_available_height, measurer, viewport, children_pcb, hp, false);

    // CSS 2.1 §9.5.1 rule 8: if the float's outer margin box does not fit in
    // the space beside existing floats, drop it below them until it fits (or
    // no float remains to clear). This wraps a row of left floats onto a new
    // line in a narrow container instead of overflowing past the edge.
    let same_line_top = same_line_float_top(&frame.b.children, i, child_y);
    let child = &mut frame.b.children[i];
    let fc = &mut frame.init.fc;
    let probe_ml = child.style.margin_left.resolve_or_zero(cem, probe_avail, viewport);
    let probe_mr = child.style.margin_right.resolve_or_zero(cem, probe_avail, viewport);
    let outer_w = probe_ml + child.rect.width + probe_mr;
    // CSS 2.1 §9.5.1 rules 6/8: a float that follows inline content on the same
    // line starts at the top of that line when it fits beside it (BUG-1338);
    // the loop below still drops it when it does not fit.
    let mut float_y = same_line_top.filter(|&t| t < child_y).unwrap_or(child_y);
    while !fc.is_empty() || float_y < child_y {
        let l = fc.left_edge_at(float_y, content_x);
        let r = fc.right_edge_at(float_y, container_right);
        if outer_w <= (r - l).max(0.0) {
            break;
        }
        match fc.next_float_bottom(float_y) {
            Some(ny) => float_y = ny.max(float_y),
            None => {
                if float_y < child_y {
                    float_y = child_y;
                }
                break;
            }
        }
    }
    let dropped = (float_y - child_y).abs() > f32::EPSILON;
    // Shadow child_y at the (possibly dropped) line for the placement below —
    // matches the removed loop's shadowing exactly: the *loop-level* child_y
    // (`frame.init.child_y`) is never written here, since a float does not
    // advance flow position.
    let child_y = float_y;
    let avail_left  = fc.left_edge_at(child_y, content_x);
    let avail_right = fc.right_edge_at(child_y, container_right);
    let avail_w = (avail_right - avail_left).max(0.0);
    // Re-lay-out at the dropped line: an auto-width float may grow into the
    // wider line, and the box's origin changed.
    if dropped {
        // Same containing-block basis as the probe layout above — an explicit
        // width must not be re-resolved against the new line's narrowed gap
        // either.
        let w = if child.style.width.is_some() {
            content_width
        } else {
            preferred_inline_block_width(child, measurer, viewport)
                .or_else(|| {
                    let w = max_content_outer_width(child, measurer, viewport);
                    (w > 0.0).then_some(w)
                })
                .map(|pw| (pw + probe_margins).min(avail_w))
                .unwrap_or(avail_w)
        };
        lay_out(child, avail_left, child_y, w,
                children_available_height, measurer, viewport, children_pcb, hp, false);
    }

    let fml = child.style.margin_left.resolve_or_zero(cem, avail_w, viewport);
    let fmr = child.style.margin_right.resolve_or_zero(cem, avail_w, viewport);
    let fmt = child.style.margin_top.resolve_or_zero(cem, avail_w, viewport);
    let fmb = child.style.margin_bottom.resolve_or_zero(cem, avail_w, viewport);
    let fw  = child.rect.width;
    let fh  = child.rect.height;

    // The trial layout put the float at the left edge of the line; its
    // descendants are absolute, so every move of the float box below has to
    // carry the subtree along (a right float otherwise left its text behind
    // at the container's left edge).
    let laid_out_at = (child.rect.x, child.rect.y);
    match child.style.float_side {
        FloatSide::Left => {
            let lx = fc.left_edge_at(child_y, content_x);
            child.rect.x = lx + fml;
            child.rect.y = child_y + fmt;
            let top_y  = child_y + fmt;
            let bot_y  = top_y + fh + fmb;
            let right_edge = lx + fml + fw + fmr;
            fc.add_left(child_y, bot_y, right_edge);
            wire_shape_outside(fc, child, true, ShapeBoxes {
                child_y, bot_y, box_left: lx, box_right: right_edge,
                margins: [fmt, fmr, fmb, fml],
            }, content_width, viewport);
        }
        FloatSide::Right => {
            let rx = fc.right_edge_at(child_y, container_right);
            child.rect.x = rx - fmr - fw;
            child.rect.y = child_y + fmt;
            let top_y  = child_y + fmt;
            let bot_y  = top_y + fh + fmb;
            let left_edge = rx - fmr - fw - fml;
            fc.add_right(child_y, bot_y, left_edge);
            wire_shape_outside(fc, child, false, ShapeBoxes {
                child_y, bot_y, box_left: left_edge, box_right: rx,
                margins: [fmt, fmr, fmb, fml],
            }, content_width, viewport);
        }
        FloatSide::None => unreachable!(),
    }
    // `position: relative` was already applied to the subtree by the trial
    // layout; the new margin-box position has to carry it too.
    let (rel_x, rel_y) = if matches!(child.style.position, Position::Relative) {
        let rel = |a: &LengthOrAuto, b: &LengthOrAuto| match (a, b) {
            (LengthOrAuto::Length(l), _) => l.resolve(cem, Some(content_width), viewport).unwrap_or(0.0),
            (LengthOrAuto::Auto, LengthOrAuto::Length(r)) => -r.resolve(cem, Some(content_width), viewport).unwrap_or(0.0),
            (LengthOrAuto::Auto, LengthOrAuto::Auto) => 0.0,
        };
        (rel(&child.style.left, &child.style.right), rel(&child.style.top, &child.style.bottom))
    } else {
        (0.0, 0.0)
    };
    let (final_x, final_y) = (child.rect.x + rel_x, child.rect.y + rel_y);
    child.rect.x = laid_out_at.0;
    child.rect.y = laid_out_at.1;
    shift_tree(child, final_x - laid_out_at.0, final_y - laid_out_at.1);
}

/// Top of the line the float at `children[i]` shares with the inline content
/// right before it (an `InlineRun` / single-row `InlineBlockRow` ending at
/// `child_y`), if any.
fn same_line_float_top(children: &[LayoutBox], i: usize, child_y: f32) -> Option<f32> {
    let prev = children[..i].iter().rev().find(|c| {
        c.style.float_side == FloatSide::None
            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
            && c.style.display != crate::style::Display::None
    })?;
    if (prev.rect.y + prev.rect.height - child_y).abs() > 0.5 || prev.rect.height <= 0.0 {
        return None;
    }
    match &prev.kind {
        BoxKind::InlineRun { lines, segments, .. } => {
            if lines.is_empty() || segments.last().is_none_or(|s| s.forced_break) {
                return None;
            }
            Some(prev.rect.y + prev.rect.height * (lines.len() - 1) as f32 / lines.len() as f32)
        }
        BoxKind::InlineBlockRow => {
            let y0 = prev.children.first()?.rect.y;
            prev.children.iter().all(|c| (c.rect.y - y0).abs() < 0.5).then_some(prev.rect.y)
        }
        _ => None,
    }
}

/// Margin-box geometry of a just-placed float (see [`wire_shape_outside`]).
struct ShapeBoxes {
    child_y: f32,
    bot_y: f32,
    box_left: f32,
    box_right: f32,
    /// Resolved margins `[top, right, bottom, left]`.
    margins: [f32; 4],
}

/// CSS Shapes L1 — record `child`'s `shape-outside` (grown by `shape-margin`,
/// a percentage resolving against the containing block's inline size
/// `cb_width`, §6.3) in the float context `fc`.
fn wire_shape_outside(
    fc: &mut FloatContext,
    child: &LayoutBox,
    is_left: bool,
    bx: ShapeBoxes,
    cb_width: f32,
    viewport: Size,
) {
    let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside else {
        return;
    };
    let margin = child.style.shape_margin
        .resolve_or_zero(child.style.font_size, cb_width, viewport)
        .max(0.0);
    let boxes = FloatBoxes::of(child, bx.margins, cb_width, viewport);
    if let Some(prims) = resolve_shape(sv, &boxes) {
        // The contour is physical px from the margin-box top-left, which is
        // the float context's frame once moved to where the float sits.
        let prims = prims
            .into_iter()
            .map(|mut p| {
                p.translate(bx.box_left, bx.child_y);
                p
            })
            .collect();
        fc.add_shape_prims(prims, is_left, (bx.child_y, bx.bot_y), (bx.box_left, bx.box_right), margin);
    }
}

/// Runs once `frame.b`'s children are all processed — the CSS 2.1 §8.3.1
/// parent↔last-child bottom-margin collapse and §9.5 float-enclosure epilogue
/// that used to run right after the removed inline loop, then the two shared
/// tail helpers (`finalize_block_height`/`finish_after_match`) every other
/// dispatch arm already calls directly. Copied verbatim except reading loop
/// state from `frame.init` instead of locals.
fn finish_frame(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    bottom_cache: &mut MarginCollapseCache,
) {
    // The block-size below may use `lh`/`ch`/`ex`; `dispatch_box`'s context for
    // this box was dropped when it handed the children loop over to `run`.
    let _font_ctx = FontContext::enter(&frame.b, measurer);
    // CSS 2.1 §8.3.1: parent↔last-child bottom margin collapse. When this box
    // collapses its bottom margin (auto height, no bottom padding/border, no
    // BFC) and the last in-flow child is a collapsible block, that child's
    // (collapsed) bottom margin escapes out of this box rather than enlarging
    // its content height — it becomes part of this box's own bottom margin
    // (reported to the parent loop via `collapsed_bottom_margin`). Only fold
    // it out when no float extends past the last child's flow bottom.
    let escaped_bottom = if frame.init.b_collapses_bottom {
        last_collapsible_child(&frame.b)
            .map(|c| collapsed_bottom_margin(c, frame.init.content_width, viewport, bottom_cache))
            .unwrap_or(0.0)
    } else {
        0.0
    };
    // CSS 2.1 §10.6.7: only a BFC root's height encloses its floats. A non-BFC in-flow block
    // shares its parent's formatting context (`shares_floats`) — the floats stick out of it
    // and the parent takes them over. `overflow: clip` is no BFC root either (CSS Overflow L3
    // §3.1).
    let encloses_floats = !frame.init.shares_floats
        && (!(matches!(frame.init.s.overflow_x, Overflow::Clip)
            || matches!(frame.init.s.overflow_y, Overflow::Clip))
            || establishes_bfc(&frame.b));
    let float_bottom = frame.init.fc.left.iter().chain(frame.init.fc.right.iter())
        .map(|(bot, _)| *bot)
        .filter(|_| encloses_floats)
        .fold(frame.init.child_y, f32::max);
    let base = (float_bottom - frame.init.content_y).max(0.0);
    // BUG-1365: a negative escaped margin was also added to `child_y`, so the
    // raw (unclamped) flow height is taken before it is removed again.
    let content_height = if escaped_bottom != 0.0 && (float_bottom - frame.init.child_y).abs() < 0.01 {
        (float_bottom - frame.init.content_y - escaped_bottom).max(0.0)
    } else {
        base
    };
    // CSS Overflow L4 §line-clamp: the content ends at the bottom of the n-th line of the flow.
    let auto_bound = line_clamp_flow::auto_bound(
        &frame.init.s, frame.init.em, frame.init.available_height, viewport,
        frame.init.padding_top, frame.init.padding_bottom, frame.init.content_y,
    );
    let clamp_cut = line_clamp_flow::find_cut(&frame.b, viewport, auto_bound);
    let content_height = clamp_cut.map_or(content_height, |cut| {
        (frame.b.rect.y + cut.line_bottom + cut.extra - frame.init.content_y).clamp(0.0, content_height)
    });

    finalize_block_height(
        &mut frame.b, &frame.init.s, frame.init.em, frame.init.available_height, viewport,
        frame.init.padding_top, frame.init.padding_bottom, frame.init.size_contained,
        frame.init.field_intrinsic, content_height, frame.init.cb,
    );
    finish_after_match(
        &mut frame.b, &frame.init.s, frame.init.em, frame.init.cb, frame.init.available_height, frame.init.is_positioned,
        frame.init.pcb, &frame.init.abs_deferred, measurer, viewport, hp,
    );
    if let Some(cut) = clamp_cut {
        line_clamp_flow::apply_cut(&mut frame.b, cut, viewport, measurer);
    }
}

/// CSS Box Alignment L3 §5.2 / §6.3 — `justify-self` (or, for `auto`, the parent's
/// `justify-items`) of a finished in-flow block-level child: the margin box moves inside the
/// containing block's content box to its inline-start / centre / inline-end. `start`/`end`
/// follow the *container's* direction, `self-start`/`self-end` the child's own writing mode,
/// `left`/`right` the physical side. Without an `unsafe`/`safe` keyword an overflowing box
/// still aligns by its own side (`unsafe`); `safe` falls back to `start`. Auto inline margins
/// take the free space first (CSS 2.1 §10.3.3), so such a box is left alone. An auto-sized
/// box was made `fit-content` wide by `dispatch_box` (`justify_fits_content`).
fn align_justify_self_child(frame: &mut Frame, idx: usize, viewport: Size) {
    use crate::style::Direction;
    let parent = &frame.init.s;
    let rev = parent.direction == Direction::Rtl;
    let content_x = frame.init.content_x;
    let content_width = frame.init.content_width;
    let child = &mut frame.b.children[idx];
    let cs = &child.style;
    if !matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot)
        || cs.margin_left.is_auto()
        || cs.margin_right.is_auto()
        || matches!(cs.float_side, FloatSide::Left | FloatSide::Right)
        || matches!(cs.position, Position::Absolute | Position::Fixed)
    {
        return;
    }
    let (value, own, side, safe) = if matches!(cs.justify_self, AlignValue::Auto) {
        let x = &parent.content_align_extra;
        (parent.justify_items, x.justify_items_own, x.justify_items_side, x.justify_items_safe)
    } else {
        let x = &cs.content_align_extra;
        (cs.justify_self, x.justify_self_own, x.justify_self_side, x.justify_self_safe)
    };
    let value = super::grid_vertical::resolve_own(value, own, cs, true, !rev);
    let mut align = super::grid_vertical::resolve_side(value, side, parent.writing_mode, rev);
    align = match align {
        AlignValue::Baseline => AlignValue::Start,
        AlignValue::LastBaseline => AlignValue::End,
        a => a,
    };
    if !matches!(align, AlignValue::Start | AlignValue::End | AlignValue::Center) {
        return;
    }
    let em = cs.font_size;
    let ml = cs.margin_left.resolve_or_zero(em, content_width, viewport);
    let mr = cs.margin_right.resolve_or_zero(em, content_width, viewport);
    let (m_s, m_e) = if rev { (mr, ml) } else { (ml, mr) };
    if safe && child.rect.width + m_s + m_e > content_width && matches!(align, AlignValue::End | AlignValue::Center) {
        align = AlignValue::Start;
    }
    let off = super::grid_vertical::axis_offset(align, content_width, child.rect.width, m_s, m_e, false, false);
    let (rel_dx, _) = super::layout_dispatch::relative_offset(cs, em, content_width, frame.init.children_available_height, viewport);
    let x = if rev { content_x + content_width - off - child.rect.width } else { content_x + off };
    let dx = x + rel_dx - child.rect.x;
    if dx.abs() > 0.01 {
        shift_tree(child, dx, 0.0);
    }
}

/// CSS 2.1 §10.3.3 — an over-constrained block-level box (a used width
/// narrower than the room, neither horizontal margin `auto`) ignores
/// `margin-left` when the *containing block's* `direction` is `rtl`, so the
/// box sits against the right edge instead of the left. `lay_out_inner`
/// places every such box at the left edge (it never sees the parent's
/// direction), so the parent shifts the finished child here.
fn align_rtl_overconstrained_child(frame: &mut Frame, idx: usize, viewport: Size) {
    if frame.b.style.direction != crate::style::Direction::Rtl {
        return;
    }
    let content_width = frame.init.content_width;
    let right = frame.init.container_right;
    let child = &mut frame.b.children[idx];
    let cs = &child.style;
    if !matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot)
        || cs.margin_left.is_auto()
        || cs.margin_right.is_auto()
        || matches!(cs.position, Position::Absolute | Position::Fixed)
        || !matches!(cs.justify_self, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch)
        || !matches!(frame.init.s.justify_items, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch)
    {
        return;
    }
    let mr = cs.margin_right.resolve_or_zero(cs.font_size, content_width, viewport);
    let dx = right - mr - child.rect.width - child.rect.x;
    if dx.abs() > 0.01 {
        shift_tree(child, dx, 0.0);
    }
}

/// Runs right after a normal-flow child finishes — CSS 2.1 §8.3.1 margin
/// collapsing and the `child_y` advance, copied verbatim from immediately
/// after the removed loop's recursive call. `frame.pending_is_block`/
/// `frame.pending_collapsed_mt` were set by `step_child` right before that
/// child was dispatched (synchronously or via a resumed descent — either way
/// this runs exactly once per child, same as the original `continue`-free
/// tail of the loop body).
fn post_child_bookkeeping(
    frame: &mut Frame,
    idx: usize,
    viewport: Size,
    bottom_cache: &mut MarginCollapseCache,
) {
    if matches!(frame.b.children[idx].kind, BoxKind::Skip) {
        // Zero-height; does not break the collapsing chain.
        return;
    }
    frame.init.seen_inflow_child = true;
    let content_width = frame.init.content_width;
    align_justify_self_child(frame, idx, viewport);
    align_rtl_overconstrained_child(frame, idx, viewport);
    // CSS 2.1 §8.3.1: the child's effective bottom margin is its own bottom
    // margin folded with any bottom margin escaping from its last-child chain
    // (collapse-through), mirroring `collapsed_mt` on the top edge. For
    // non-block kinds this is just the own margin.
    let child_mb =
        collapsed_bottom_margin(&frame.b.children[idx], content_width, viewport, bottom_cache);
    let is_block = frame.pending_is_block;
    let collapsed_mt = frame.pending_collapsed_mt;
    // CSS 2.1 §8.3.1 (self-collapsing empty box): an empty, non-BFC block with
    // no in-flow content and a used height of 0 has its own top and bottom
    // margins adjoining — they merge into ONE value with whatever already
    // collapsed into this position (`prev_block_mb`/`collapsed_mt`) instead of
    // stacking as two separate gaps around a box that occupies no vertical
    // space.
    let self_collapses = is_block
        && !establishes_bfc(&frame.b.children[idx])
        && frame.b.children[idx].style.clear == ClearSide::None
        && !has_in_flow_content(&frame.b.children[idx])
        && frame.b.children[idx].rect.height.abs() < 0.01;
    if self_collapses {
        let old_gap = frame.init.prev_block_mb.max(collapsed_mt);
        let merged = old_gap.max(child_mb);
        if merged > old_gap {
            frame.b.children[idx].rect.y += merged - old_gap;
        }
        // `child_y` already includes the previous sibling's bottom margin
        // (`prev_block_mb`); the merged gap replaces it rather than stacking.
        frame.init.child_y += merged - frame.init.prev_block_mb;
        frame.init.prev_block_mb = merged;
    } else {
        let child = &frame.b.children[idx];
        let (_, rel_dy) = super::layout_dispatch::relative_offset(
            &child.style, child.style.font_size, content_width,
            frame.init.children_available_height, viewport);
        frame.init.child_y = child.rect.y - rel_dy + child.rect.height + child_mb;
        frame.init.prev_block_mb = if is_block { child_mb.max(0.0) } else { 0.0 };
    }
    // CSS 2.1 §10.8 — inline-image line-box descent (the classic "image bottom
    // gap"), historically compensated here for `<video>`/`<canvas>`/`<iframe>`
    // (BUG-180, TEST-18): before IFC-3 they were inline-level replaced media
    // that Lumen still laid out as block-flow children (`default_display`
    // mapped them to Block), so the sub-baseline space of their line box was
    // dropped and every media-wrapping block came out ~descent px too short.
    //
    // `BoxKind::Image` was deliberately NOT in this list since IFC-2, and
    // IFC-3 removes `Video`/`Canvas`/`Iframe` from it the same way:
    // `default_display` now maps all four to `Inline`, so they get their
    // descent from the `InlineBlockRow` strut like `<img>` does. Reaching
    // block flow at all now means the author blockified the element
    // (`display: block`, a float, absolute positioning) — and a blockified
    // box has no line box and therefore no gap under it, exactly as for a
    // blockified `<img>`.
}
