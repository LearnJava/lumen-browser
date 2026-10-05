use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};
use super::baseline::{
    align_baseline_side, baseline_phys_side_in_axis, box_baseline_in_axis, resolved_align, PhysSide,
};

/// Per-line state `flex::build_flex_init` precomputes (CSS Flexbox L1 §9.3
/// line-breaking, §9.7 grow/shrink, §9.5 justify-content) before Phase A
/// (item placement, driven by [`run`]) begins. Immutable during Phase A —
/// grow/shrink and justify never depend on ANOTHER line's placement, only on
/// this line's own items, so all lines can be precomputed up front in
/// natural (not visiting) order.
pub(super) struct FlexLineInit {
    /// Keys into `FlexInit::item_idxs` — `lines[li]` from Step 2, source order.
    pub(super) line_keys: Vec<usize>,
    /// Positions into `line_keys` (0..line_keys.len()), in the order items are
    /// actually placed — always source order: `*-reverse` is a mirror applied
    /// after placement (`finish_frame`), so `justify-content: flex-start` packs
    /// at main-start whichever physical edge that is.
    pub(super) ordered_keys: Vec<usize>,
    /// Resolved outer (margin-box) main size per position in `line_keys`,
    /// after CSS Flexbox §9.7 grow/shrink.
    pub(super) hyp_mains: Vec<f32>,
    /// Per position in `line_keys`: whether the item's main-axis margin is
    /// `auto` — `(start-side, end-side)` in SOURCE order (reverse mapping is
    /// applied where consumed, same as the removed inline code did).
    pub(super) auto_main: Vec<(bool, bool)>,
    pub(super) jc_start: f32,
    pub(super) jc_gap: f32,
    pub(super) auto_main_share: f32,
}

/// Loop-entry state for `lay_out_flex`'s per-item final-placement pass
/// (CSS Flexbox L1 §9.5/§9.6) — everything the removed inline double loop
/// (pre-LAYOUT-2-срез-3 `flex.rs`) read or wrote across items/lines, plus
/// what `layout_dispatch.rs`'s flex arm used to do with `lay_out_flex`'s
/// return value once it came back (height, `flex_abs` children). Captured by
/// `flex::build_flex_init` before any item is placed — the Step 1 probe and
/// Step 2 line-breaking/Step 3 grow-shrink/justify precompute that produce
/// these fields still run natively in `build_flex_init`; see its doc comment
/// for why (a probe result is never suspended on, so it does not need to be).
pub(super) struct FlexInit {
    pub(super) item_idxs: Vec<usize>,
    /// Per line, in NATURAL order (0..n_lines) — indexed by `li`, not visiting
    /// position. `ordered_line_idxs` gives the visiting order separately.
    pub(super) line_inits: Vec<FlexLineInit>,
    pub(super) ordered_line_idxs: Vec<usize>,
    /// The main axis is physically vertical — see `flex::flex_axes`.
    pub(super) is_column: bool,
    /// Main-start is at the physical bottom/right: the auto-margin sides swap
    /// here, the items themselves are mirrored in `finish_frame`.
    pub(super) is_reverse: bool,
    /// Cross-start is at the physical bottom/right — mirrored in `finish_frame`.
    pub(super) cross_rev: bool,
    /// `flex-wrap: wrap-reverse` — what a `safe` align-content falls back across.
    pub(super) wrap_reverse: bool,
    /// `flex-direction: *-reverse` (the keyword, not the physical result).
    pub(super) reverse_kw: bool,
    pub(super) is_wrap: bool,
    pub(super) content_x: f32,
    pub(super) content_y: f32,
    pub(super) content_width: f32,
    pub(super) explicit_cross: Option<f32>,
    /// The container's definite content-box main size (`None` — content-sized).
    pub(super) main_definite: Option<f32>,
    /// The cross size (physical width of a vertical main axis) is content-sized:
    /// items align inside their line, not the container, and the container's
    /// width is the sum of its lines.
    pub(super) cross_indefinite: bool,
    /// `Some` for a flex container in a vertical `writing-mode`.
    pub(super) vertical: Option<super::flex::VerticalFlex>,
    pub(super) item_gap: f32,
    pub(super) cross_gap: f32,
    pub(super) s: Arc<ComputedStyle>,
    // BUG-341 S41 column probe/replay data, per k (index into `item_idxs`).
    pub(super) probe_cross: Vec<f32>,
    pub(super) column_probe: Vec<Option<f32>>,
    pub(super) probed_main: Vec<Option<f32>>,
    pub(super) probe_ran: Vec<Option<f32>>,
    // Phase A running state, mutated by `step_item`/`finish_line`.
    pub(super) main_cursor: f32,
    pub(super) cross_cursor: f32,
    /// Pushed in VISITING order (matches the removed code's `.push()` inside
    /// the `for li in &ordered_line_idxs` loop) — `align-content`'s
    /// `line_offsets[li]` below indexes it positionally the same way the
    /// removed code did, `wrap-reverse` quirk included. Not a behavior this
    /// slice is meant to change.
    pub(super) line_cross_sizes: Vec<f32>,
    // Phase D (container height + abs children) inputs — used only once, in
    // `finish_frame`, so they ride along unchanged from `build_flex_init`.
    pub(super) em: f32,
    pub(super) available_height: Option<f32>,
    pub(super) padding_top: f32,
    pub(super) padding_bottom: f32,
    pub(super) size_contained: bool,
    pub(super) is_positioned: bool,
    /// Positioned containing block for this container's OWN normal-flow
    /// children (Step 1 probe and item placement) — `dispatch_box`'s
    /// `children_pcb` local, distinct from `own_pcb` below.
    pub(super) children_pcb: Rect,
    /// This container's own positioned containing block (`dispatch_box`'s
    /// `pcb` parameter) — used only as the `flex_abs` fallback in
    /// `finish_frame` when the container itself is not positioned.
    pub(super) own_pcb: Rect,
}

/// One level of the explicit stack `run` maintains in place of the native
/// call stack — the flex-container analogue of `block_flow_trampoline::Frame`.
/// `b` is owned (taken via `block_flow_trampoline::take_box`) for the
/// duration of this container's own item-placement loop.
struct Frame {
    b: LayoutBox,
    init: Box<FlexInit>,
    /// Index into `init.ordered_line_idxs` — which line is being processed.
    li_pos: usize,
    /// Index into the current line's `ordered_keys` — which item within it.
    j_pos: usize,
}

enum StepOutcome {
    /// This item is fully placed (replayed, or its own dispatch completed
    /// synchronously/via the block-flow trampoline) — move on to the next.
    Advance,
    /// This item is itself a flex container with its own items to place —
    /// push the current frame and continue processing this one.
    Descend(Box<FlexInit>),
}

/// The item at `(li, j_pos)`, resolved once from `init`'s read-only per-line
/// tables — used identically by `step_item` (about to place it), `run`'s
/// descend branch (about to push a child frame for it) and `run`'s pop branch
/// (about to resume it) so all three agree on which item they mean without
/// threading extra fields through `Frame`.
struct ItemPos {
    k: usize,
    i: usize,
    outer_main: f32,
    auto_before: bool,
    auto_after: bool,
    auto_main_share: f32,
}

fn locate_item(init: &FlexInit, li: usize, j_pos: usize) -> ItemPos {
    let line = &init.line_inits[li];
    let line_j = line.ordered_keys[j_pos];
    let k = line.line_keys[line_j];
    let i = init.item_idxs[k];
    let outer_main = line.hyp_mains[line_j];
    let (a0, a1) = line.auto_main[line_j];
    let (auto_before, auto_after) = if init.is_reverse { (a1, a0) } else { (a0, a1) };
    ItemPos { k, i, outer_main, auto_before, auto_after, auto_main_share: line.auto_main_share }
}

/// Drives `init`'s item-placement pass (and every further flex-container
/// descendant it meets) on an explicit heap stack, so a chain of nested flex
/// containers no longer grows the native call stack one frame per level
/// (LAYOUT-2's acceptance criterion, applied to item (2) of its ROADMAP
/// entry). `b` is the box `dispatch_box`'s flex arm was originally called on.
///
/// Out of scope for this slice, same as float placement was for
/// `block_flow_trampoline` — both still recurse on the native stack:
/// the Step 1 probe (`build_flex_init`, column `flex-basis: auto`/`content`
/// items) and the cross-axis stretch re-layout for a column-flex child
/// (`finish_line`'s `relayout_column_flex` branch). Neither is the dominant,
/// unconditional-per-item recursion this slice targets — see LAYOUT-2's
/// ROADMAP entry for the follow-up.
pub(super) fn run(
    b: &mut LayoutBox,
    init: Box<FlexInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let mut current = Frame { b: block_flow_trampoline::take_box(b), init, li_pos: 0, j_pos: 0 };
    let mut stack: Vec<Frame> = Vec::new();

    loop {
        if current.li_pos >= current.init.ordered_line_idxs.len() {
            finish_frame(&mut current, measurer, viewport, hp);
            match stack.pop() {
                None => {
                    *b = current.b;
                    return;
                }
                Some(mut parent) => {
                    let li = parent.init.ordered_line_idxs[parent.li_pos];
                    let pos = locate_item(&parent.init, li, parent.j_pos);
                    parent.b.children[pos.i] = current.b;
                    post_item_place(&mut parent, li, &pos, viewport);
                    parent.j_pos += 1;
                    current = parent;
                }
            }
            continue;
        }

        let li = current.init.ordered_line_idxs[current.li_pos];
        let line_len = current.init.line_inits[li].ordered_keys.len();
        if current.j_pos >= line_len {
            finish_line(&mut current, li, measurer, viewport, hp);
            current.li_pos += 1;
            current.j_pos = 0;
            continue;
        }

        match step_item(&mut current, li, measurer, viewport, hp) {
            StepOutcome::Advance => {
                current.j_pos += 1;
            }
            StepOutcome::Descend(child_init) => {
                let pos = locate_item(&current.init, li, current.j_pos);
                let child_box = block_flow_trampoline::take_box(&mut current.b.children[pos.i]);
                let child_frame = Frame { b: child_box, init: child_init, li_pos: 0, j_pos: 0 };
                stack.push(current);
                current = child_frame;
            }
        }
    }
}

/// Handles exactly one item of the current line — CSS Flexbox §9.5/§9.6 final
/// placement, copied from the removed inline loop body (both the column and
/// row arms) except at the one recursive call in each, which now either
/// finishes synchronously (`Advance`, doing the same post-item bookkeeping
/// the removed loop did right after the call — see `post_item_place`) or
/// hands back the item's `FlexInit` for `run` to push and descend into.
fn step_item(
    frame: &mut Frame,
    li: usize,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) -> StepOutcome {
    if frame.j_pos == 0 {
        frame.init.main_cursor = frame.init.line_inits[li].jc_start;
    }
    let pos = locate_item(&frame.init, li, frame.j_pos);
    if pos.auto_before {
        frame.init.main_cursor += pos.auto_main_share;
    }
    let content_x = frame.init.content_x;
    let content_y = frame.init.content_y;
    let content_width = frame.init.content_width;
    let main_cursor = frame.init.main_cursor;
    let pcb = frame.init.children_pcb;

    if frame.init.is_column {
        let item_s = frame.b.children[pos.i].style.clone();
        let iem = item_s.font_size;
        let m_t = item_s.margin_top.resolve_or_zero(iem, content_width, viewport);
        let m_b = item_s.margin_bottom.resolve_or_zero(iem, content_width, viewport);
        // The item's resolved *border-box* main size — see the removed
        // code's comment on why `BorderBox` is forced below.
        let inner_main = (pos.outer_main - m_t - m_b).max(0.0);
        // BUG-341 S41: the margin-box cross width resolved before Step 1 —
        // see `column_item_avail_cross`'s doc comment.
        let item_avail_cross = frame.init.probe_cross[pos.k];
        // BUG-341 S41 — exact bit equality, not an epsilon: see the removed
        // code's comment on why an approximate match is unsafe here.
        // FLEX-VWM-3: the probe left a vertical item at its content-sized
        // width; stretching it to the cross size is a different layout.
        let block_axis_stretch = super::flex::column_item_stretches_block_axis(&frame.b.children[pos.i], &frame.b.style);
        let replayable = !block_axis_stretch
            && frame.init.column_probe[pos.k]
                .is_some_and(|probed| probed.to_bits() == inner_main.to_bits());

        if let Some(probed_at) = frame.init.probe_ran.get(pos.k).copied().flatten() {
            let cross_differs = probed_at.to_bits() != item_avail_cross.to_bits();
            let probed = frame.init.probed_main[pos.k];
            let column_probe_k = frame.init.column_probe[pos.k];
            note_flex_column(|c| {
                if replayable {
                    c.replayed += 1;
                    return;
                }
                c.double += 1;
                if column_probe_k.is_none() {
                    c.double_dirty += 1;
                } else if cross_differs {
                    c.double_cross += 1;
                } else {
                    c.double_size += 1;
                    if probed.is_some_and(|p| inner_main > p) {
                        c.double_size_grew += 1;
                    }
                }
            });
        }

        if replayable {
            // BUG-341 S41: reproduce the probe's own box-origin arithmetic
            // exactly rather than re-associating the sum — see the removed
            // code's comment (the one page of the graphic-test corpus where
            // an A/B caught the 0.01px difference).
            let dy = ((content_y + main_cursor) + m_t) - (content_y + m_t);
            // Wrapped column: the probe laid the item at the first line's x; later
            // lines are offset by the cross cursor (CSS Flexbox L1 §9.4).
            shift_tree(&mut frame.b.children[pos.i], frame.init.cross_cursor, dy);
            post_item_place(frame, li, &pos, viewport);
            return StepOutcome::Advance;
        }

        // BUG-736: a replaced element's intrinsic-hint `width` otherwise wins
        // over `item_avail_cross` the same way it did in the Step-1 probe
        // (`build_flex_init`) — force it to the resolved cross width here too,
        // for the (rarer) case this dispatch isn't served by the `replayable`
        // shortcut above (e.g. `flex-grow` changed the item's main size from
        // what Step 1 measured).
        let width_hinted = frame.b.children[pos.i].style.width_is_intrinsic_hint;
        let height_hinted = frame.b.children[pos.i].style.height_is_intrinsic_hint;
        match dispatch_box(
            &mut frame.b.children[pos.i], content_x + frame.init.cross_cursor, content_y + main_cursor, item_avail_cross,
            Some(inner_main), measurer, viewport, pcb, hp, false, None, AlignValue::Auto,
            Some(UsedSizeOverride {
                height: Some(inner_main),
                width: width_hinted.then_some(item_avail_cross).or_else(|| {
                    let m_l = item_s.margin_left.resolve_or_zero(iem, content_width, viewport);
                    let m_r = item_s.margin_right.resolve_or_zero(iem, content_width, viewport);
                    block_axis_stretch.then_some((item_avail_cross - m_l - m_r).max(0.0))
                }).or_else(|| {
                    // `BorderBox` is forced below for the main size, so an
                    // authored content-box width has to be handed over as the
                    // border-box width it stands for — otherwise a re-layout
                    // (probe memo hit, so no replay) loses its padding+border.
                    if item_s.box_sizing != BoxSizing::ContentBox {
                        return None;
                    }
                    let w = item_s.width.as_ref().filter(|w| !w.is_intrinsic())?;
                    let px = w.resolve(iem, Some(content_width), viewport)?;
                    let frame_h = item_s.padding_left.resolve_or_zero(iem, content_width, viewport)
                        + item_s.padding_right.resolve_or_zero(iem, content_width, viewport)
                        + item_s.border_left_width
                        + item_s.border_right_width;
                    Some(px + frame_h)
                }),
                box_sizing: Some(BoxSizing::BorderBox),
                clear_intrinsic_hint: width_hinted || height_hinted,
                percentage_base: None,
            }),
        ) {
            DispatchOutcome::Done => {
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsBlockFlowLoop(child_init) => {
                block_flow_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(child_init) => StepOutcome::Descend(child_init),
            // LAYOUT-2 срез 4: a column-flex item that is itself a grid
            // container — same shape as the block-flow arm above (a
            // different init type, so it runs synchronously via its own
            // trampoline rather than descending onto this stack).
            DispatchOutcome::NeedsGridLoop(child_init) => {
                super::grid_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 6: a column-flex item that is itself a table —
            // same shape as the grid arm above.
            DispatchOutcome::NeedsTableLoop(child_init) => {
                super::table_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 7: a column-flex item that is itself a multicol
            // container — same shape as the grid/table arms above.
            DispatchOutcome::NeedsMulticolLoop(child_init) => {
                super::multicol_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: a column-flex item that is itself a vertical
            // writing-mode container — same shape as the grid/table/multicol
            // arms above.
            DispatchOutcome::NeedsVerticalLoop(child_init) => {
                super::vertical_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
        }
    } else {
        let item_s = frame.b.children[pos.i].style.clone();
        let iem = item_s.font_size;
        let m_l = item_s.margin_left.resolve_or_zero(iem, content_width, viewport);
        let m_r = item_s.margin_right.resolve_or_zero(iem, content_width, viewport);
        // BUG-427: `inner_main` is a border-box main size — see the removed
        // code's comment for why row direction converts to a used *width*
        // here instead of forcing `box_sizing: BorderBox` the way column does.
        let inner_main = (pos.outer_main - m_l - m_r).max(0.0);
        let used_main = match item_s.box_sizing {
            BoxSizing::BorderBox => inner_main,
            BoxSizing::ContentBox => {
                let pl = item_s.padding_left.resolve_or_zero(iem, content_width, viewport);
                let pr = item_s.padding_right.resolve_or_zero(iem, content_width, viewport);
                (inner_main - pl - pr - item_s.border_left_width - item_s.border_right_width).max(0.0)
            }
        };
        let cross_cursor = frame.init.cross_cursor;
        let explicit_cross = frame.init.explicit_cross;
        // BUG-736: a replaced element's intrinsic-hint `height` (see
        // `ComputedStyle::height_is_intrinsic_hint`) is definite by itself —
        // it would otherwise win over the `aspect_ratio`-from-`width`
        // derivation `finalize_block_height` runs when `height` is `auto`,
        // pinning the item to its raw intrinsic height regardless of the
        // main-axis (width) size this override just resolved. Clearing is a
        // no-op for a style that was never hinted.
        //
        // BUG-974: `dispatch_box`'s `available_width` doubles as the free
        // space auto margins/auto-width distribute into — row items keep
        // `inner_main` (the space the flexbox algorithm assigned this item
        // in the line) here so `justify-content`/auto-margin centering on the
        // main axis is unaffected (see `flex_item_auto_main_margins_center`
        // and neighbors in `flow_modes.rs`). The item's own padding/margin/
        // max-width percentages resolve against a *different* base — the
        // container's content width, per CSS 2.1 §8.1/Flexbox §4 — so that
        // base goes through `UsedSizeOverride::percentage_base` instead of
        // `available_width`. Conflating the two (handing `available_width`
        // itself the container's content width) silently produced a
        // narrower box (114px instead of 140px for `width:100px;
        // padding-left:10%` in a 400px container) *and* broke every main-axis
        // auto-margin test, since those read free space off `available_width`.
        match dispatch_box(
            &mut frame.b.children[pos.i], content_x + main_cursor, content_y + cross_cursor, inner_main,
            explicit_cross, measurer, viewport, pcb, hp, false, None, AlignValue::Auto,
            Some(UsedSizeOverride {
                width: Some(used_main),
                clear_intrinsic_hint: true,
                percentage_base: Some(content_width),
                ..Default::default()
            }),
        ) {
            DispatchOutcome::Done => {
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsBlockFlowLoop(child_init) => {
                block_flow_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            DispatchOutcome::NeedsFlexLoop(child_init) => StepOutcome::Descend(child_init),
            // LAYOUT-2 срез 4: a row-flex item that is itself a grid
            // container — same shape as the column arm above.
            DispatchOutcome::NeedsGridLoop(child_init) => {
                super::grid_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 6: a row-flex item that is itself a table — same
            // shape as the grid arm above.
            DispatchOutcome::NeedsTableLoop(child_init) => {
                super::table_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 7: a row-flex item that is itself a multicol
            // container — same shape as the grid/table arms above.
            DispatchOutcome::NeedsMulticolLoop(child_init) => {
                super::multicol_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
            // LAYOUT-2 срез 8: a row-flex item that is itself a vertical
            // writing-mode container — same shape as the grid/table/multicol
            // arms above.
            DispatchOutcome::NeedsVerticalLoop(child_init) => {
                super::vertical_trampoline::run(&mut frame.b.children[pos.i], child_init, measurer, viewport, hp);
                post_item_place(frame, li, &pos, viewport);
                StepOutcome::Advance
            }
        }
    }
}

/// Runs right after an item finishes — CSS Flexbox §8.1 cross-axis auto-
/// margin/alignment for a COLUMN item (row's cross alignment is a separate
/// per-line pass, see `finish_line`) and the main-cursor advance, copied from
/// immediately after the removed loop's recursive call. Shared by
/// `step_item`'s synchronous paths and `run`'s resume-after-descend path —
/// both leave `frame.j_pos`/`li` pointing at this exact item, so re-deriving
/// `ItemPos` (via `locate_item`) gives the same answer either way; nothing
/// needs to be stashed across the suspension.
fn post_item_place(frame: &mut Frame, li: usize, pos: &ItemPos, viewport: Size) {
    // A wrapped column aligns across the *line* cross size, known only once the
    // line is complete — `finish_line` does it (`align_column_line`).
    if frame.init.is_column && !frame.init.is_wrap && !frame.init.cross_indefinite {
        let cross_shift =
            column_item_cross_shift(&frame.b.children[pos.i], frame.init.content_width, &frame.init.s, frame.init.cross_rev, frame.init.wrap_reverse, viewport);
        if cross_shift != 0.0 {
            shift_tree(&mut frame.b.children[pos.i], cross_shift, 0.0);
        }
    }
    let jc_gap = frame.init.line_inits[li].jc_gap;
    frame.init.main_cursor += pos.outer_main + frame.init.item_gap + jc_gap;
    if pos.auto_after {
        frame.init.main_cursor += pos.auto_main_share;
    }
}

/// The `align-self` (resolved against the container's `align-items`) of `item`
/// in the start-based frame: `start`/`end` follow the writing mode, so
/// `wrap-reverse` — which makes the frame's start the physical end — swaps them
/// (`flex-start`/`flex-end` are the frame already).
///
/// `self-start`/`self-end` are relative to the item's *own* writing mode and
/// direction (BUG-1265): they name a physical side of `cross_horizontal`'s axis,
/// which is then expressed in the frame (its start is the low edge unless
/// `cross_rev`).
fn frame_align(
    item: &ComputedStyle,
    container: &ComputedStyle,
    wrap_reverse: bool,
    cross_horizontal: bool,
    cross_rev: bool,
) -> AlignValue {
    let (value, wm, own) = if matches!(item.align_self, AlignValue::Auto) {
        let e = &container.content_align_extra;
        (container.align_items, e.items_wm, e.items_own)
    } else {
        let e = &item.content_align_extra;
        (item.align_self, e.self_wm, e.self_own)
    };
    if own && matches!(value, AlignValue::Start | AlignValue::End) {
        let low = own_start_is_low(item, cross_horizontal) == matches!(value, AlignValue::Start);
        return if low == !cross_rev { AlignValue::Start } else { AlignValue::End };
    }
    match value {
        AlignValue::Start if wm && wrap_reverse => AlignValue::End,
        AlignValue::End if wm && wrap_reverse => AlignValue::Start,
        other => other,
    }
}

/// `safe` (CSS Box Alignment L3 §4.4) on the item's resolved `align-self`: when
/// the item overflows its line, `center`/`end` fall back to the start edge.
fn safe_overflow_fallback(
    item: &ComputedStyle,
    container: &ComputedStyle,
    align: AlignValue,
    overflows: bool,
) -> AlignValue {
    let safe = if matches!(item.align_self, AlignValue::Auto) {
        container.content_align_extra.items_safe
    } else {
        item.content_align_extra.self_safe
    };
    if safe && overflows && matches!(align, AlignValue::Center | AlignValue::End) {
        AlignValue::Start
    } else {
        align
    }
}

/// Does the item's own start edge along a physical axis lie at the low side
/// (left / top)? Used by `self-start`/`self-end` (CSS Box Alignment L3 §4.2),
/// which follow the item's own `writing-mode` and `direction` — for the
/// horizontal axis the block start of a vertical box or the inline start of a
/// horizontal one, for the vertical axis the other way round.
pub(super) fn own_start_is_low(s: &ComputedStyle, horizontal_axis: bool) -> bool {
    use crate::style::{Direction, WritingMode as W};
    let rtl = s.direction == Direction::Rtl;
    match (s.writing_mode, horizontal_axis) {
        (W::HorizontalTb, true) => !rtl,
        (W::HorizontalTb, false) => true,
        (W::VerticalRl | W::SidewaysRl, true) => false,
        (W::VerticalLr | W::SidewaysLr, true) => true,
        // `sideways-lr` runs its inline axis bottom-to-top.
        (W::SidewaysLr, false) => rtl,
        (_, false) => !rtl,
    }
}

/// Участие item'а в baseline-выравнивании линии (CSS Flexbox §9.4 шаг 8, Align §9):
/// индекс группы (`0` — прижата к началу линии стартовой раскладки, `1` — к концу)
/// и расстояния от её начала до базовой линии и от базовой линии до конца margin
/// box item'а. `None` — item не участвует: другое значение `align-self` (либо `auto`
/// в поперечном поле, которое приоритетнее, CSS Flexbox §8.1) или поперечные поля
/// `auto`. Ортогональный item (ось строк которого не совпадает с осью базовой
/// линии) тоже участвует — с линией, синтезированной по краю border box.
///
/// `cross_vertical` — поперечная ось вертикальна (главная горизонтальна): тогда
/// базовая линия горизонтальна, иначе — вертикальна (положение по x). Группа
/// определяется краем, к которому тянется базовая линия
/// ([`baseline_phys_side_in_axis`]: `first` — начало блока самого item'а, поэтому
/// `first baseline` бокса `vertical-rl` и `last baseline` бокса `vertical-lr` делят
/// группу), с обращением при `wrap-reverse`.
///
/// Работает в стартовой раскладке (`finish_frame` зеркалит её для `cross_rev`):
/// там поперечное начало — нижний/правый край margin box, поэтому «подъём» —
/// расстояние от него до базовой линии (физический спуск), а группа переходит на
/// противоположный край, чтобы после зеркалирования базовые линии items остались
/// на одной прямой.
fn cross_item_baseline(
    item: &LayoutBox,
    init: &FlexInit,
    cross_vertical: bool,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
) -> Option<(usize, f32, f32)> {
    let (container, wrap_reverse, cross_rev, content_width) =
        (&*init.s, init.wrap_reverse, init.cross_rev, init.content_width);
    let is = &item.style;
    let (lo, hi, extent) = if cross_vertical {
        (&is.margin_top, &is.margin_bottom, item.rect.height)
    } else {
        (&is.margin_left, &is.margin_right, item.rect.width)
    };
    if matches!(lo, LengthOrAuto::Auto) || matches!(hi, LengthOrAuto::Auto) {
        return None;
    }
    let side = align_baseline_side(resolved_align(is, container))?;
    let iem = is.font_size;
    let (m_lo, m_hi) = (lo.resolve_or_zero(iem, content_width, viewport), hi.resolve_or_zero(iem, content_width, viewport));
    let outer = extent + m_lo + m_hi;
    // Линия измеряется по оси, перпендикулярной поперечной: вертикальная линия
    // (положение по x) — когда поперечная ось горизонтальна.
    let ascent = m_lo + box_baseline_in_axis(item, container, !cross_vertical, side, measurer);
    let descent = outer - ascent;
    let phys = baseline_phys_side_in_axis(item, container, !cross_vertical, side);
    let flips = usize::from(wrap_reverse) + usize::from(cross_rev);
    let max_side = (phys == PhysSide::Max) != (flips % 2 == 1);
    Some(if cross_rev { (usize::from(max_side), descent, ascent) } else { (usize::from(max_side), ascent, descent) })
}

/// CSS Flexbox §8.3/§8.1 — horizontal shift of a COLUMN item inside `cross_size`
/// (the container's content width, or the width of the item's own line when the
/// column wraps): auto margins first, then `align-self`/`align-items`.
///
/// Works in the start-based frame `finish_frame` mirrors afterwards for
/// `cross_rev`: there the cross-START margin is the physical *right* one, so the
/// two auto flags swap.
fn column_item_cross_shift(
    item: &LayoutBox,
    cross_size: f32,
    container: &ComputedStyle,
    cross_rev: bool,
    wrap_reverse: bool,
    viewport: Size,
) -> f32 {
    let is = &item.style;
    let iem = is.font_size;
    let m_l = is.margin_left.resolve_or_zero(iem, cross_size, viewport);
    let m_r = is.margin_right.resolve_or_zero(iem, cross_size, viewport);
    let avail_cross = (cross_size - m_l - m_r).max(0.0);
    let (auto_cross_l, auto_cross_r) = {
        let (l, r) = (matches!(is.margin_left, LengthOrAuto::Auto), matches!(is.margin_right, LengthOrAuto::Auto));
        if cross_rev { (r, l) } else { (l, r) }
    };
    let cross_align = frame_align(is, container, wrap_reverse, true, cross_rev);
    let cross_align = safe_overflow_fallback(is, container, cross_align, avail_cross < item.rect.width);
    let free_cross = (avail_cross - item.rect.width).max(0.0);
    if auto_cross_l && auto_cross_r {
        free_cross / 2.0
    } else if auto_cross_l {
        free_cross
    } else if auto_cross_r {
        0.0
    } else {
        // Unsafe alignment (the default) lets an overflowing item hang out of
        // the start side too, so the signed free space is used here.
        let signed_free = avail_cross - item.rect.width;
        match cross_align {
            AlignValue::Center => signed_free / 2.0,
            AlignValue::End | AlignValue::LastBaseline => signed_free,
            _ => 0.0,
        }
    }
}

/// Width of a wrapped column's line `li` — the widest margin box among its items
/// (CSS Flexbox §9.4 step 8: the line's cross size is its largest outer cross size).
fn column_line_cross_size(frame: &Frame, li: usize, viewport: Size) -> f32 {
    let cw = frame.init.content_width;
    frame.init.line_inits[li]
        .line_keys
        .iter()
        .map(|&k| {
            let item = &frame.b.children[frame.init.item_idxs[k]];
            let iem = item.style.font_size;
            let m_l = item.style.margin_left.resolve_or_zero(iem, cw, viewport);
            let m_r = item.style.margin_right.resolve_or_zero(iem, cw, viewport);
            item.rect.width + m_l + m_r
        })
        .fold(0.0_f32, f32::max)
}

/// Runs once all items of line `li` are placed — CSS Flexbox §9.5 cross-axis
/// alignment for a ROW line (column direction skips this entirely, matching
/// the removed code's `if !is_column` guard) plus the cross-cursor advance.
/// Copied from the removed loop's per-line tail. The `relayout_column_flex`
/// re-layout still recurses on the native stack — rare (only a column-flex
/// child being stretched by a definite parent cross size) and, like float
/// placement in `block_flow_trampoline`, out of scope for this slice.
fn finish_line(
    frame: &mut Frame,
    li: usize,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let is_column = frame.init.is_column;
    let n_items = frame.init.line_inits[li].line_keys.len();
    // CSS Flexbox L1 §9.4 шаг 7: размер линии по поперечной оси — наибольший
    // внешний (с полями) поперечный размер items; items, выровненные по базовой
    // линии, считаются группой: наибольшее расстояние от верхней кромки margin
    // box до базовой линии плюс наибольшее расстояние от неё до нижней.
    // Items, выровненные по базовой линии, считаются группами (по краю линии, к которому
    // тянется линия, см. `cross_item_baseline`): (наибольшее расстояние от начала
    // линии до базовой линии, наибольшее от базовой линии до конца).
    let cross_vertical = !is_column;
    let (line_cross, baseline_groups): (f32, [(f32, f32); 2]) = {
        let content_width = frame.init.content_width;
        let mut max_outer = 0.0_f32;
        let mut groups = [(0.0_f32, 0.0_f32); 2];
        for jx in 0..n_items {
            let k = frame.init.line_inits[li].line_keys[jx];
            let i = frame.init.item_idxs[k];
            let item = &frame.b.children[i];
            match cross_item_baseline(item, &frame.init, cross_vertical, viewport, measurer) {
                Some((idx, ascent, descent)) => {
                    let g = &mut groups[idx];
                    g.0 = g.0.max(ascent);
                    g.1 = g.1.max(descent);
                }
                None => {
                    let is = &item.style;
                    let iem = is.font_size;
                    max_outer = max_outer.max(if cross_vertical {
                        item.rect.height
                            + is.margin_top.resolve_or_zero(iem, content_width, viewport)
                            + is.margin_bottom.resolve_or_zero(iem, content_width, viewport)
                    } else {
                        item.rect.width
                            + is.margin_left.resolve_or_zero(iem, content_width, viewport)
                            + is.margin_right.resolve_or_zero(iem, content_width, viewport)
                    });
                }
            }
        }
        let grouped = max_outer.max(groups[0].0 + groups[0].1).max(groups[1].0 + groups[1].1);
        if is_column {
            // A wrapped column has one vertical line per wrap; a single-line column
            // keeps the zero (its cross cursor is never read).
            let cross = if frame.init.is_wrap || frame.init.cross_indefinite {
                column_line_cross_size(frame, li, viewport).max(grouped)
            } else {
                0.0
            };
            (cross, groups)
        } else {
            (grouped, groups)
        }
    };
    frame.init.line_cross_sizes.push(line_cross);

    if is_column && (frame.init.is_wrap || frame.init.cross_indefinite) {
        for jx in 0..n_items {
            let k = frame.init.line_inits[li].line_keys[jx];
            let i = frame.init.item_idxs[k];
            let shift = column_item_cross_shift(&frame.b.children[i], line_cross, &frame.init.s, frame.init.cross_rev, frame.init.wrap_reverse, viewport);
            if shift != 0.0 {
                shift_tree(&mut frame.b.children[i], shift, 0.0);
            }
        }
    }

    // Колонка (поперечная ось горизонтальна): выравнивание по базовой линии items
    // вертикального режима — их линия вертикальна, положение по x.
    if is_column {
        let content_width = frame.init.content_width;
        let line_left = frame.init.content_x + frame.init.cross_cursor;
        let effective_cross = if !frame.init.is_wrap && !frame.init.cross_indefinite { content_width } else { line_cross };
        for jx in 0..n_items {
            let k = frame.init.line_inits[li].line_keys[jx];
            let i = frame.init.item_idxs[k];
            let item = &frame.b.children[i];
            let Some((idx, ascent, _)) = cross_item_baseline(item, &frame.init, false, viewport, measurer) else {
                continue;
            };
            let iem = item.style.font_size;
            let m_l = item.style.margin_left.resolve_or_zero(iem, content_width, viewport);
            let margin_left_x = if idx == 0 {
                line_left + (baseline_groups[0].0 - ascent)
            } else {
                line_left + effective_cross - baseline_groups[1].1 - ascent
            };
            let dx = margin_left_x + m_l - item.rect.x;
            if dx != 0.0 {
                shift_tree(&mut frame.b.children[i], dx, 0.0);
            }
        }
    }

    if !is_column {
        let s = Arc::clone(&frame.init.s);
        let content_width = frame.init.content_width;
        let content_y = frame.init.content_y;
        let cross_cursor = frame.init.cross_cursor;
        let is_wrap = frame.init.is_wrap;
        // CSS Flexbox §9.5: for a single-line (non-wrapping) flex container the
        // line cross size equals the container's inner cross size (if definite).
        let effective_cross = if !is_wrap {
            frame.init.explicit_cross.unwrap_or(line_cross)
        } else {
            line_cross
        };
        for jx in 0..n_items {
            let k = frame.init.line_inits[li].line_keys[jx];
            let i = frame.init.item_idxs[k];
            let is = frame.b.children[i].style.clone();
            let iem = is.font_size;
            let m_t = is.margin_top.resolve_or_zero(iem, content_width, viewport);
            let m_b = is.margin_bottom.resolve_or_zero(iem, content_width, viewport);
            let align = frame_align(&is, &s, frame.init.wrap_reverse, false, frame.init.cross_rev);
            // In the start-based frame (mirrored afterwards for `cross_rev`) the
            // cross-start margin is the physical bottom one.
            let (auto_cross_start, auto_cross_end) = {
                let (t, b) = (
                    matches!(is.margin_top, LengthOrAuto::Auto),
                    matches!(is.margin_bottom, LengthOrAuto::Auto),
                );
                if frame.init.cross_rev { (b, t) } else { (t, b) }
            };
            let item_rect_height = frame.b.children[i].rect.height;
            let outer_cross = item_rect_height + m_t + m_b;
            let align = safe_overflow_fallback(&is, &s, align, outer_cross > effective_cross);
            if auto_cross_start || auto_cross_end {
                let free = (effective_cross - outer_cross).max(0.0);
                let shift = if auto_cross_start && auto_cross_end {
                    free / 2.0
                } else if auto_cross_start {
                    free
                } else {
                    0.0
                };
                let new_y = content_y + cross_cursor + m_t + shift;
                let item_y = frame.b.children[i].rect.y;
                shift_y_box(&mut frame.b.children[i], new_y - item_y);
                continue;
            }
            // §9.4 шаг 8 / §8.5: выравнивание по базовой линии. Группа прижата к
            // началу или концу линии стартовой раскладки (см. `cross_item_baseline`).
            if let Some((idx, ascent, _)) = cross_item_baseline(&frame.b.children[i], &frame.init, true, viewport, measurer) {
                let line_top = content_y + cross_cursor;
                let margin_top_y = if idx == 0 {
                    line_top + (baseline_groups[0].0 - ascent)
                } else {
                    line_top + effective_cross - baseline_groups[1].1 - ascent
                };
                let item_y = frame.b.children[i].rect.y;
                shift_y_box(&mut frame.b.children[i], margin_top_y + m_t - item_y);
                continue;
            }
            match align {
                AlignValue::End => {
                    let new_y = content_y + cross_cursor + effective_cross - outer_cross + m_t;
                    let item_y = frame.b.children[i].rect.y;
                    shift_y_box(&mut frame.b.children[i], new_y - item_y);
                }
                AlignValue::Center => {
                    let new_y = content_y + cross_cursor + m_t + (effective_cross - outer_cross) / 2.0;
                    let item_y = frame.b.children[i].rect.y;
                    shift_y_box(&mut frame.b.children[i], new_y - item_y);
                }
                AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                    let stretch_h = if is.height.is_none() {
                        (effective_cross - m_t - m_b).max(0.0)
                    } else {
                        item_rect_height
                    };
                    // BUG-104/BUG-209 — see the removed code's comment.
                    let old_h = frame.b.children[i].rect.height;
                    let grew = old_h < stretch_h;
                    // Вложенный flex-контейнер: растянутая высота определённая, и дети
                    // растягиваются уже по ней (Flexbox §9.4 step 11) — row-flex с пустыми
                    // `align-self: stretch` детьми иначе оставляет их высотой 0
                    // (`subgrid-gap-decorations-013`: полосы эталона `.row-gap-a`).
                    let relayout_column_flex = is.height.is_none()
                        && stretch_h > 0.0
                        && matches!(is.display, Display::Flex | Display::InlineFlex)
                        && if matches!(is.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse) {
                            frame.init.explicit_cross.is_some()
                        } else {
                            grew
                        };
                    if grew {
                        frame.b.children[i].rect.height = stretch_h;
                    }
                    frame.b.children[i].rect.y = content_y + cross_cursor + m_t;
                    // FLEX-VWM-4: a stretched item's height is definite, so a
                    // descendant's `height: <%>` resolves against it — which the
                    // first pass (item height still `auto`) could not do. Also when
                    // the first pass came out taller (§9.4 step 11: the stretched size
                    // is the line's, however large the content was).
                    if (grew || old_h > stretch_h + 0.01)
                        && !relayout_column_flex
                        && is.height.is_none()
                        && !matches!(is.position, Position::Relative | Position::Sticky)
                        && super::flex::subtree_has_percent_height(&frame.b.children[i])
                    {
                        relayout_stretched_row_item(frame, i, stretch_h, measurer, viewport, hp);
                    }
                    if relayout_column_flex {
                        let rx = frame.b.children[i].rect.x;
                        let ry = frame.b.children[i].rect.y;
                        let rw = frame.b.children[i].rect.width;
                        let pcb = frame.init.children_pcb;
                        lay_out_with_used_size(
                            &mut frame.b.children[i], rx, ry, rw, Some(stretch_h), measurer, viewport, pcb, hp, false,
                            UsedSizeOverride {
                                height: Some(stretch_h),
                                box_sizing: Some(BoxSizing::BorderBox),
                                ..Default::default()
                            },
                        );
                    }
                }
                _ => {
                    frame.b.children[i].rect.y = content_y + cross_cursor + m_t;
                }
            }
        }
    }

    frame.init.cross_cursor += line_cross + frame.init.cross_gap;
}

/// FLEX-VWM-4: lays the row item `i` out again with its stretched border-box
/// height `stretch_h` as an authored height, keeping its x and width.
fn relayout_stretched_row_item(
    frame: &mut Frame,
    i: usize,
    stretch_h: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let content_width = frame.init.content_width;
    let item = &frame.b.children[i];
    let is = Arc::clone(&item.style);
    let iem = is.font_size;
    let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
    let (rw, rx) = (item.rect.width, item.rect.x - m_l);
    let ry = frame.init.content_y + frame.init.cross_cursor;
    let pad_v = is.padding_top.resolve_or_zero(iem, content_width, viewport)
        + is.padding_bottom.resolve_or_zero(iem, content_width, viewport)
        + is.border_top_width
        + is.border_bottom_width;
    let pad_h = is.padding_left.resolve_or_zero(iem, content_width, viewport)
        + is.padding_right.resolve_or_zero(iem, content_width, viewport)
        + is.border_left_width
        + is.border_right_width;
    let (used_w, used_h) = match is.box_sizing {
        BoxSizing::BorderBox => (rw, stretch_h),
        BoxSizing::ContentBox => ((rw - pad_h).max(0.0), (stretch_h - pad_v).max(0.0)),
    };
    let pcb = frame.init.children_pcb;
    lay_out_with_used_size(
        &mut frame.b.children[i], rx, ry, rw, Some(stretch_h), measurer, viewport, pcb, hp, false,
        UsedSizeOverride {
            width: Some(used_w),
            height: Some(used_h),
            clear_intrinsic_hint: true,
            percentage_base: Some(content_width),
            ..Default::default()
        },
    );
}

/// FLEX-VWM: turns the start-based layout `step_item`/`finish_line` produced
/// into the physical one when main-start and/or cross-start sit at the
/// bottom/right edge (`row-reverse`, `direction: rtl`, `wrap-reverse`,
/// `vertical-rl` …, see `flex::flex_axes`).
///
/// Each item's margin box is reflected inside the container's content box
/// along the affected axis. Margins keep their physical sides — the item's own
/// offset inside its margin box is part of the reflected span — so a box is
/// moved by `extent - 2 * span_start - span_size`, a pure translation of its
/// whole subtree. The extent is the definite content size when there is one
/// and the occupied size otherwise (a content-sized container has no free
/// space, so its items just swap places).
fn mirror_reversed_axes(frame: &mut Frame, total_cross: f32, viewport: Size) {
    if !frame.init.is_reverse && !frame.init.cross_rev {
        return;
    }
    let (cx, cy, cw) = (frame.init.content_x, frame.init.content_y, frame.init.content_width);
    let is_column = frame.init.is_column;
    // Per item: [x_start, x_size, y_start, y_size] of the margin box relative to
    // the content-box origin.
    let spans: Vec<(usize, [f32; 4])> = frame
        .init
        .item_idxs
        .iter()
        .map(|&i| {
            let it = &frame.b.children[i];
            let iem = it.style.font_size;
            let m_l = it.style.margin_left.resolve_or_zero(iem, cw, viewport);
            let m_r = it.style.margin_right.resolve_or_zero(iem, cw, viewport);
            let m_t = it.style.margin_top.resolve_or_zero(iem, cw, viewport);
            let m_b = it.style.margin_bottom.resolve_or_zero(iem, cw, viewport);
            (
                i,
                [
                    it.rect.x - m_l - cx,
                    it.rect.width + m_l + m_r,
                    it.rect.y - m_t - cy,
                    it.rect.height + m_t + m_b,
                ],
            )
        })
        .collect();
    // Axis 0 = x, 1 = y; (start, size) index pairs into the arrays above.
    let (main_axis, cross_axis) = if is_column { (1, 0) } else { (0, 1) };
    let occupied = |axis: usize| spans.iter().map(|(_, s)| s[axis * 2] + s[axis * 2 + 1]).fold(0.0_f32, f32::max);
    let main_extent = frame.init.main_definite.unwrap_or_else(|| occupied(main_axis));
    let cross_extent = if is_column {
        if frame.init.cross_indefinite { total_cross } else { cw }
    } else {
        frame.init.explicit_cross.unwrap_or(total_cross)
    };
    for (i, sp) in spans {
        let delta = |axis: usize, extent: f32| extent - 2.0 * sp[axis * 2] - sp[axis * 2 + 1];
        let mut d = [0.0_f32; 2];
        if frame.init.is_reverse {
            d[main_axis] = delta(main_axis, main_extent);
        }
        if frame.init.cross_rev {
            d[cross_axis] = delta(cross_axis, cross_extent);
        }
        shift_tree(&mut frame.b.children[i], d[0], d[1]);
    }
}

/// Runs once every line is placed — CSS Flexbox §8.3 align-content across
/// lines, the container content-height/total-cross return value, and (moved
/// in from `layout_dispatch.rs`'s former post-`lay_out_flex` code) the
/// container's own `b.rect.height` and its `flex_abs` (absolutely-positioned)
/// children. Copied from the removed code's tail plus the dispatch arm.
fn finish_frame(
    frame: &mut Frame,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let is_column = frame.init.is_column;
    let n_lines = frame.init.line_inits.len();
    let cross_gap = frame.init.cross_gap;
    // Remove the trailing cross gap the loop accumulated — see the removed
    // code's comment (one surplus `cross_gap` after the last line, including
    // single-line containers).
    let mut total_cross = if n_lines > 0 {
        (frame.init.cross_cursor - cross_gap).max(0.0)
    } else {
        frame.init.cross_cursor
    };

    if frame.init.is_wrap {
        let line_gap_total = cross_gap * (n_lines.saturating_sub(1)) as f32;
        let used_cross: f32 = frame.init.line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        // Row: the container's definite height. Column: its content width — the
        // cross axis of a wrapped column is horizontal and always definite here.
        let cross_size = if is_column {
            (!frame.init.cross_indefinite).then_some(frame.init.content_width)
        } else {
            frame.init.explicit_cross
        };
        let free_cross = cross_size.map_or(0.0, |h| (h - used_cross).max(0.0));
        // `align-content: safe …` with the lines overflowing: align to the
        // writing-mode `start` edge — the logical start, or the logical end
        // under `wrap-reverse` (mirrored below like everything else).
        if frame.init.s.content_align_extra.align_safe
            && matches!(frame.init.s.align_content, AlignValue::Start | AlignValue::End | AlignValue::Center)
            && let Some(h) = cross_size
            && h < used_cross
            && frame.init.wrap_reverse
        {
            let shift = h - used_cross;
            for k in 0..frame.init.item_idxs.len() {
                let i = frame.init.item_idxs[k];
                if is_column {
                    shift_tree(&mut frame.b.children[i], shift, 0.0);
                } else {
                    shift_tree(&mut frame.b.children[i], 0.0, shift);
                }
            }
        }

        // Natural cross sizes of the lines, for the line tracks recorded below.
        let natural_sizes = frame.init.line_cross_sizes.clone();
        let mut line_offsets: Vec<f32> = vec![0.0; n_lines];
        if free_cross > 0.0 {
            let effective = match frame.init.s.align_content {
                AlignValue::Auto | AlignValue::Normal => AlignValue::Stretch,
                // `start`/`end` follow the writing mode: `wrap-reverse` swaps them
                // relative to this start-based frame (see `mirror_reversed_axes`).
                AlignValue::Start if frame.init.s.content_align_extra.align_wm && frame.init.wrap_reverse => {
                    AlignValue::End
                }
                AlignValue::End if frame.init.s.content_align_extra.align_wm && frame.init.wrap_reverse => {
                    AlignValue::Start
                }
                other => other,
            };

            match effective {
                AlignValue::End => {
                    line_offsets.fill(free_cross);
                }
                AlignValue::Center => {
                    line_offsets.fill(free_cross / 2.0);
                }
                AlignValue::SpaceBetween if n_lines > 1 => {
                    let gap_per = free_cross / (n_lines - 1) as f32;
                    for (idx, offset) in line_offsets.iter_mut().enumerate().skip(1) {
                        *offset = gap_per * idx as f32;
                    }
                }
                AlignValue::SpaceAround => {
                    let per = free_cross / n_lines as f32;
                    for (idx, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per / 2.0 + (per * idx as f32);
                    }
                }
                AlignValue::SpaceEvenly => {
                    let per = free_cross / (n_lines + 1) as f32;
                    for (idx, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * (idx as f32 + 1.0);
                    }
                }
                AlignValue::Stretch => {
                    let per = free_cross / n_lines as f32;
                    for (idx, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * idx as f32;
                    }
                    for size in frame.init.line_cross_sizes.iter_mut() {
                        *size += per;
                    }
                }
                _ => {}
            }

            if is_column {
                // CSS Flexbox §8.3 for a wrapped column: lines are spread along x.
                // `finish_line` aligned each item inside the line's *natural* width;
                // a stretched line is wider, so the align shift is redone for it.
                let per_stretch = if matches!(effective, AlignValue::Stretch) { free_cross / n_lines as f32 } else { 0.0 };
                for (li, &offset) in line_offsets.iter().enumerate() {
                    let new_cross = frame.init.line_cross_sizes[li];
                    let old_cross = new_cross - per_stretch;
                    let n_items = frame.init.line_inits[li].line_keys.len();
                    for jx in 0..n_items {
                        let k = frame.init.line_inits[li].line_keys[jx];
                        let i = frame.init.item_idxs[k];
                        let dx = offset
                            + column_item_cross_shift(&frame.b.children[i], new_cross, &frame.init.s, frame.init.cross_rev, frame.init.wrap_reverse, viewport)
                            - column_item_cross_shift(&frame.b.children[i], old_cross, &frame.init.s, frame.init.cross_rev, frame.init.wrap_reverse, viewport);
                        if dx != 0.0 {
                            shift_tree(&mut frame.b.children[i], dx, 0.0);
                        }
                    }
                }
            }

            for (li, &offset) in line_offsets.iter().enumerate() {
                if !is_column && (offset > 0.0 || matches!(effective, AlignValue::Stretch)) {
                    let n_items = frame.init.line_inits[li].line_keys.len();
                    for jx in 0..n_items {
                        let k = frame.init.line_inits[li].line_keys[jx];
                        let i = frame.init.item_idxs[k];
                        // Shift the whole item subtree — see the removed
                        // code's BUG-165 comment.
                        if offset > 0.0 {
                            shift_y_box(&mut frame.b.children[i], offset);
                        }
                        // CSS Flexbox §9.4 step 9 + §8.3: `align-content: stretch` grew the
                        // line, so an auto-height `align-self: stretch` item grows with it.
                        if matches!(effective, AlignValue::Stretch) {
                            let item = &mut frame.b.children[i];
                            let is = &item.style;
                            let own = if matches!(is.align_self, AlignValue::Auto) {
                                frame.init.s.align_items
                            } else {
                                is.align_self
                            };
                            if is.height.is_none()
                                && matches!(own, AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal)
                                && !matches!(is.margin_top, LengthOrAuto::Auto)
                                && !matches!(is.margin_bottom, LengthOrAuto::Auto)
                            {
                                item.rect.height += free_cross / n_lines as f32;
                            }
                        }
                    }
                }
            }

            total_cross = frame.init.line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        }

        // The lines of a wrapped row container for gap rules cut by a multicol break
        // (`multicol_trampoline::emit_grid_fragments`): `(top, bottom)` of every line from the
        // content box's top, after `align-content`. `wrap-reverse` and vertical writing modes
        // mirror the cross axis, so they keep the paint-side search by the items' rects.
        if !is_column && n_lines >= 2 && !frame.init.wrap_reverse && frame.init.vertical.is_none() {
            let mut top = 0.0f32;
            let mut tracks = Vec::with_capacity(n_lines);
            for li in 0..n_lines {
                let lo = top + line_offsets[li];
                tracks.push((lo, lo + frame.init.line_cross_sizes[li]));
                top += natural_sizes[li] + cross_gap;
            }
            frame.b.subgrid_tracks =
                Some(Box::new(crate::subgrid::SubgridTracks { cols: None, rows: Some(tracks), fragment: false }));
        }
    }

    // FLEX-VWM: a content-sized block size (`width: auto` in a vertical writing
    // mode) is what the lines occupy: the sum of the lines for a vertical main
    // axis, the longest line for a horizontal one.
    if let Some(v) = frame.init.vertical
        && v.block_size_auto
    {
        let occupied = if is_column {
            total_cross
        } else {
            let cx = frame.init.content_x;
            frame
                .init
                .item_idxs
                .iter()
                .map(|&i| {
                    let it = &frame.b.children[i];
                    let m_r = it.style.margin_right.resolve_or_zero(it.style.font_size, frame.init.content_width, viewport);
                    it.rect.x + it.rect.width + m_r - cx
                })
                .fold(0.0_f32, f32::max)
        };
        frame.b.rect.width = occupied + v.frame_horiz;
        frame.init.content_width = occupied;
    }

    mirror_reversed_axes(frame, total_cross, viewport);

    let content_height = if is_column {
        frame
            .init
            .item_idxs
            .iter()
            .map(|&i| frame.b.children[i].rect.y + frame.b.children[i].rect.height - frame.init.content_y)
            .fold(0.0_f32, f32::max)
    } else {
        total_cross
    };

    // `layout_dispatch.rs`'s former post-`lay_out_flex` code: container
    // height (CSS 2.1 §10.6.3/§10.6.7, CSS Box Sizing L4 §5) and CSS Flexbox
    // L1 §4.1 absolutely-positioned children (excluded from flex layout
    // above; positioned now against this container's content box).
    let s = Arc::clone(&frame.init.s);
    let em = frame.init.em;
    let padding_top = frame.init.padding_top;
    let padding_bottom = frame.init.padding_bottom;
    frame.b.rect.height = if let Some(h_len) = &s.height
        && let Some(h) = resolve_block_size(h_len, em, frame.init.available_height, viewport)
    {
        match s.box_sizing {
            BoxSizing::ContentBox => {
                (h + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width).max(0.0)
            }
            BoxSizing::BorderBox => {
                h.max(padding_top + padding_bottom + s.border_top_width + s.border_bottom_width)
            }
        }
    } else if let Some((aw, ah)) = s.aspect_ratio
        && aw > 0.0
        && ah > 0.0
    {
        (frame.b.rect.width * ah / aw).max(0.0)
    } else if frame.init.vertical.is_some_and(|v| v.fill_inline_size) && frame.init.available_height.is_some() {
        // The inline size of a vertical writing mode fills what is available
        // (Writing Modes L3 §7.3 — `height: auto` there is `width: auto` of a
        // horizontal box).
        frame.init.available_height.unwrap_or(viewport.height).max(0.0)
    } else if frame.init.vertical.is_some_and(|v| v.fill_inline_size) {
        // No definite room along the inline axis (the parent's height is auto):
        // an orthogonal flow root shrinks to its content, wrapping at the initial
        // containing block (Writing Modes L3 §7.3.1, FLEX-VWM-4).
        let ch = contained_content_height(frame.init.size_contained, &s, em, viewport, content_height);
        (ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width).min(viewport.height.max(0.0))
    } else {
        let ch = contained_content_height(frame.init.size_contained, &s, em, viewport, content_height);
        ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
    };

    let flex_abs: Vec<(usize, f32, f32)> = frame
        .b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| (i, frame.init.content_x, frame.init.content_y))
        .collect();
    if !flex_abs.is_empty() {
        let my_pcb = if frame.init.is_positioned {
            Rect::new(
                frame.b.rect.x + s.border_left_width,
                frame.b.rect.y + s.border_top_width,
                (frame.b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (frame.b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            )
        } else {
            frame.init.own_pcb
        };
        lay_out_abs_children(&mut frame.b, &flex_abs, measurer, viewport, my_pcb, hp);
        align_abs_static_positions(frame, &flex_abs, viewport);
    }
    if frame.init.is_positioned {
        super::multicol_abspos::fix_out_of_flow_descendants(&mut frame.b, measurer, viewport, hp);
    }
}

/// Physical position of a lone item inside `free` space along one axis, as a
/// fraction (0 — top/left edge, 1 — bottom/right) — CSS Flexbox §4.1: the static
/// position of an abspos child is where it would sit as the only flex item.
///
/// `safe_overflow`: the item is `safe`-aligned *and* overflows (the caller decides
/// against what — see `align_abs_static_positions`).
/// `rev`: the axis' start edge is the physical bottom/right one; `dir_rev`: the
/// `*-reverse`/`wrap-reverse` keyword is on (it makes the writing-mode `start`/
/// `end` the *opposite* of `flex-start`/`flex-end`).
fn lone_item_fraction(
    value: AlignValue,
    wm_relative: bool,
    safe_overflow: bool,
    rev: bool,
    dir_rev: bool,
) -> f32 {
    let safe = safe_overflow;
    let mut logical = match value {
        AlignValue::End => 1.0,
        AlignValue::Center | AlignValue::SpaceAround | AlignValue::SpaceEvenly => 0.5,
        _ => 0.0,
    };
    if wm_relative && dir_rev && matches!(value, AlignValue::Start | AlignValue::End) {
        logical = 1.0 - logical;
    }
    // `safe` and overflowing: the writing-mode start edge instead.
    if safe && matches!(value, AlignValue::Start | AlignValue::End | AlignValue::Center) {
        logical = if dir_rev { 1.0 } else { 0.0 };
    }
    if rev { 1.0 - logical } else { logical }
}

/// CSS Flexbox §4.1 — moves each abspos child whose inset on an axis is `auto` on
/// both sides from the content-box corner `lay_out_abs_children` left it at to
/// where `justify-content` (main axis) / `align-self` (cross axis) put a lone
/// item.
fn align_abs_static_positions(frame: &mut Frame, abs: &[(usize, f32, f32)], viewport: Size) {
    let s = Arc::clone(&frame.init.s);
    let main_vertical = frame.init.is_column;
    let extra = s.content_align_extra;
    let width = frame.init.content_width;
    let height = (frame.b.rect.height
        - frame.init.padding_top
        - frame.init.padding_bottom
        - s.border_top_width
        - s.border_bottom_width)
        .max(0.0);
    for &(idx, _, _) in abs {
        let child = &frame.b.children[idx];
        let cs = &child.style;
        let em = cs.font_size;
        let m_l = cs.margin_left.resolve_or_zero(em, width, viewport);
        let m_r = cs.margin_right.resolve_or_zero(em, width, viewport);
        let m_t = cs.margin_top.resolve_or_zero(em, width, viewport);
        let m_b = cs.margin_bottom.resolve_or_zero(em, width, viewport);
        let free_x = width - (child.rect.width + m_l + m_r);
        let free_y = height - (child.rect.height + m_t + m_b);
        let (mut cross_value, cross_safe, mut cross_wm) = if matches!(cs.align_self, AlignValue::Auto) {
            (s.align_items, extra.items_safe, extra.items_wm)
        } else {
            (cs.align_self, cs.content_align_extra.self_safe, cs.content_align_extra.self_wm)
        };
        // `baseline`/`last baseline` of a lone item fall back to the writing-mode
        // `start`/`end` (CSS Box Alignment §9.3).
        match cross_value {
            AlignValue::Baseline => {
                cross_value = AlignValue::Start;
                cross_wm = true;
            }
            AlignValue::LastBaseline => {
                cross_value = AlignValue::End;
                cross_wm = true;
            }
            _ => {}
        }
        // `justify-content: left|right`: physical along the inline axis (also a
        // vertical one), the writing-mode `start` along the block axis.
        let main_value = match extra.justify_side {
            Some(side) if !matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse) => {
                return_side_value(side, frame.init.is_reverse)
            }
            Some(_) => if frame.init.reverse_kw { AlignValue::End } else { AlignValue::Start },
            None => s.justify_content,
        };
        let main_wm = extra.justify_wm;
        // `safe` asks whether the item overflows its *containing block*: the flex
        // container's content box when it is itself the containing block, the
        // outer positioned ancestor's padding box otherwise (WPT
        // `flex-abspos-align-self-safe-outer-cb-*`).
        let (overflow_x, overflow_y) = if frame.init.is_positioned || matches!(cs.position, Position::Fixed) {
            (free_x < 0.0, free_y < 0.0)
        } else {
            // The height of a positioned ancestor's rect is not known yet while
            // its descendants are laid out (it reads 0): fall back to the flex
            // container's own content box then.
            let cb = frame.init.own_pcb;
            let cb_w = if cb.width > 0.0 { cb.width } else { width };
            let cb_h = if cb.height > 0.0 { cb.height } else { height };
            (cb_w - (child.rect.width + m_l + m_r) < 0.0, cb_h - (child.rect.height + m_t + m_b) < 0.0)
        };
        let (fx, fy) = {
            let main = |overflow: bool| {
                lone_item_fraction(
                    main_value, main_wm, extra.justify_safe && overflow,
                    frame.init.is_reverse, frame.init.reverse_kw,
                )
            };
            let cross = |overflow: bool| {
                lone_item_fraction(
                    cross_value, cross_wm, cross_safe && overflow,
                    frame.init.cross_rev, frame.init.wrap_reverse,
                )
            };
            if main_vertical {
                (cross(overflow_x), main(overflow_y))
            } else {
                (main(overflow_x), cross(overflow_y))
            }
        };
        // `self-start`/`self-end` name a side of the item's own box: a physical one.
        let own_cross = if matches!(cs.align_self, AlignValue::Auto) {
            extra.items_own
        } else {
            cs.content_align_extra.self_own
        };
        let (fx, fy) = if own_cross && matches!(cross_value, AlignValue::Start | AlignValue::End) {
            let low = own_start_is_low(cs, main_vertical) == matches!(cross_value, AlignValue::Start);
            let f = if low { 0.0 } else { 1.0 };
            if main_vertical { (f, fy) } else { (fx, f) }
        } else {
            (fx, fy)
        };
        let auto_x = matches!(cs.left, LengthOrAuto::Auto) && matches!(cs.right, LengthOrAuto::Auto);
        let auto_y = matches!(cs.top, LengthOrAuto::Auto) && matches!(cs.bottom, LengthOrAuto::Auto);
        let dx = if auto_x { free_x * fx } else { 0.0 };
        let dy = if auto_y { free_y * fy } else { 0.0 };
        shift_tree(&mut frame.b.children[idx], dx, dy);
    }
}

/// `justify-content: left | right` along a horizontal main axis as the
/// start-based `Start`/`End` it is: `left` is the start edge when the axis is not
/// mirrored.
fn return_side_value(side: crate::style::ContentSide, main_rev: bool) -> AlignValue {
    if (side == crate::style::ContentSide::Left) == !main_rev { AlignValue::Start } else { AlignValue::End }
}
