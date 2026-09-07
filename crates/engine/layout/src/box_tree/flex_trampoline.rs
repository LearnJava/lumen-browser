use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};

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
    /// actually placed — reversed from source order for `row-reverse`/
    /// `column-reverse` (mirrors the removed `ordered_keys` local).
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
    pub(super) is_column: bool,
    pub(super) is_reverse: bool,
    pub(super) is_wrap: bool,
    pub(super) content_x: f32,
    pub(super) content_y: f32,
    pub(super) content_width: f32,
    pub(super) explicit_cross: Option<f32>,
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
        let replayable = frame.init.column_probe[pos.k]
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
            shift_tree(&mut frame.b.children[pos.i], 0.0, dy);
            post_item_place(frame, li, &pos, viewport);
            return StepOutcome::Advance;
        }

        match dispatch_box(
            &mut frame.b.children[pos.i], content_x, content_y + main_cursor, item_avail_cross,
            Some(inner_main), measurer, viewport, pcb, hp, false, None, AlignValue::Auto,
            Some(UsedSizeOverride {
                height: Some(inner_main),
                box_sizing: Some(BoxSizing::BorderBox),
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
        match dispatch_box(
            &mut frame.b.children[pos.i], content_x + main_cursor, content_y + cross_cursor, inner_main,
            explicit_cross, measurer, viewport, pcb, hp, false, None, AlignValue::Auto,
            Some(UsedSizeOverride { width: Some(used_main), ..Default::default() }),
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
    if frame.init.is_column {
        let content_width = frame.init.content_width;
        let item = &frame.b.children[pos.i];
        let is = &item.style;
        let iem = is.font_size;
        let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
        let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);
        let avail_cross = (content_width - m_l - m_r).max(0.0);
        let auto_cross_l = matches!(is.margin_left, LengthOrAuto::Auto);
        let auto_cross_r = matches!(is.margin_right, LengthOrAuto::Auto);
        let cross_align = if matches!(is.align_self, AlignValue::Auto) {
            frame.init.s.align_items
        } else {
            is.align_self
        };
        let free_cross = (avail_cross - item.rect.width).max(0.0);
        let cross_shift = if auto_cross_l && auto_cross_r {
            free_cross / 2.0
        } else if auto_cross_l {
            free_cross
        } else if auto_cross_r {
            0.0
        } else {
            match cross_align {
                AlignValue::Center => free_cross / 2.0,
                AlignValue::End => free_cross,
                _ => 0.0,
            }
        };
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
    let line_cross: f32 = if is_column {
        0.0
    } else {
        let mut max_h = 0.0_f32;
        for jx in 0..n_items {
            let k = frame.init.line_inits[li].line_keys[jx];
            let i = frame.init.item_idxs[k];
            max_h = max_h.max(frame.b.children[i].rect.height);
        }
        max_h
    };
    frame.init.line_cross_sizes.push(line_cross);

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
            let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
            let auto_cross_start = matches!(is.margin_top, LengthOrAuto::Auto);
            let auto_cross_end = matches!(is.margin_bottom, LengthOrAuto::Auto);
            let item_rect_height = frame.b.children[i].rect.height;
            let outer_cross = item_rect_height + m_t + m_b;
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
                    let relayout_column_flex = is.height.is_none()
                        && frame.init.explicit_cross.is_some()
                        && stretch_h > 0.0
                        && matches!(is.display, Display::Flex | Display::InlineFlex)
                        && matches!(is.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
                    if frame.b.children[i].rect.height < stretch_h {
                        frame.b.children[i].rect.height = stretch_h;
                    }
                    frame.b.children[i].rect.y = content_y + cross_cursor + m_t;
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

    if !is_column && frame.init.is_wrap {
        let line_gap_total = cross_gap * (n_lines.saturating_sub(1)) as f32;
        let used_cross: f32 = frame.init.line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        let free_cross = frame.init.explicit_cross.map_or(0.0, |h| (h - used_cross).max(0.0));

        if free_cross > 0.0 {
            let mut line_offsets: Vec<f32> = vec![0.0; n_lines];
            let effective = match frame.init.s.align_content {
                AlignValue::Auto | AlignValue::Normal => AlignValue::Stretch,
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

            for (li, &offset) in line_offsets.iter().enumerate() {
                if !is_column && offset > 0.0 {
                    let n_items = frame.init.line_inits[li].line_keys.len();
                    for jx in 0..n_items {
                        let k = frame.init.line_inits[li].line_keys[jx];
                        let i = frame.init.item_idxs[k];
                        // Shift the whole item subtree — see the removed
                        // code's BUG-165 comment.
                        shift_y_box(&mut frame.b.children[i], offset);
                    }
                }
            }

            total_cross = frame.init.line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        }
    }

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
    }
}
