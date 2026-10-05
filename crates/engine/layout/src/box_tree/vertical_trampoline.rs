use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};
use crate::vertical::{shift_subtree_x, VerticalInit};
use super::vertical_float as vfloat;
use super::vertical_margins::{adjoin, collapsed_margin, collapses_through, MarginCaches};

/// One level of the explicit stack `run` maintains in place of the native
/// call stack — same `take_box`/swap-back shape as
/// `block_flow_trampoline::Frame` (see that type's doc comment for why an
/// owned swap is required instead of nesting `&mut LayoutBox` borrows).
struct Frame {
    b: LayoutBox,
    init: Box<VerticalInit>,
    next_child_idx: usize,
}

/// Drives a vertical writing-mode Block/FlowRoot container's per-child
/// block-axis stacking loop (CSS Writing Modes L3 §3 — see
/// `crate::vertical::build_vertical_init`'s doc comment for the axis-swap
/// math) and every further vertical-writing-mode descendant it meets on an
/// explicit heap stack, so a chain of nested `writing-mode: vertical-*`
/// containers no longer grows the native call stack one frame per level
/// (LAYOUT-2's acceptance criterion, applied to item (6) of its ROADMAP
/// entry). `b` is the box `dispatch_box`'s vertical arm was originally
/// called on.
pub(super) fn run(
    b: &mut LayoutBox,
    init: Box<VerticalInit>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let mut current = Frame { b: block_flow_trampoline::take_box(b), init, next_child_idx: 0 };
    let mut stack: Vec<Frame> = Vec::new();
    let mut caches = MarginCaches::default();

    loop {
        if current.next_child_idx >= current.b.children.len() {
            finish_frame(&mut current);
            match stack.pop() {
                None => {
                    *b = current.b;
                    return;
                }
                Some(mut parent) => {
                    let idx = parent.next_child_idx;
                    parent.b.children[idx] = current.b;
                    finish_child(&mut parent, idx, viewport, &mut caches);
                    parent.next_child_idx += 1;
                    current = parent;
                }
            }
            continue;
        }

        let i = current.next_child_idx;
        match step_child(&mut current, i, measurer, viewport, hp, &mut caches) {
            StepOutcome::Advance => {
                current.next_child_idx += 1;
            }
            StepOutcome::Descend(child_init) => {
                let child_box = block_flow_trampoline::take_box(&mut current.b.children[i]);
                let child_frame = Frame { b: child_box, init: child_init, next_child_idx: 0 };
                stack.push(current);
                current = child_frame;
            }
        }
    }
}

enum StepOutcome {
    /// This child is fully placed — move on to the next index.
    Advance,
    /// This child is itself a vertical writing-mode container with its own
    /// children to process — push the current frame and continue processing
    /// this one.
    Descend(Box<VerticalInit>),
}

/// Handles exactly one child of `frame.b` at `i` — a zero-height `Skip` box
/// short-circuits (mirrors the removed loop's `continue`, which never
/// advanced the block-axis cursor for it either); otherwise the child is
/// dispatched at the current cursor position and either finished
/// synchronously (`Advance`, running the same reposition/shift the old loop
/// did right after its recursive call) or, for a nested vertical container,
/// handed back to `run` as a `Descend`.
fn step_child(
    frame: &mut Frame,
    i: usize,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    caches: &mut MarginCaches,
) -> StepOutcome {
    let content_x_left = frame.init.content_x_left;
    let content_y = frame.init.content_y;

    if matches!(frame.b.children[i].kind, BoxKind::Skip) {
        frame.b.children[i].rect = Rect::new(content_x_left, content_y, 0.0, 0.0);
        return StepOutcome::Advance;
    }

    // CSS 2.1 §9.5.2 — `clear` moves the child's border edge past the floats on
    // that side; the move is applied to its block position in `finish_child`.
    let clear_floor = {
        let child = &frame.b.children[i];
        let from = frame.init.cursor_block_consumed + vfloat::block_gap(&frame.init, child, viewport, caches).0;
        vfloat::clear_target(&frame.init, child, from)
    };
    // CSS 2.1 §9.5.1 — a float is placed out of flow and does not advance the cursor.
    if vfloat::is_float(&frame.b.children[i]) {
        // Split so the float and the run it may join are two disjoint borrows.
        let run_idx = frame.init.last_run.filter(|&r| r < i);
        let (head, tail) = frame.b.children.split_at_mut(i);
        let join = run_idx.map(|r| {
            let run = &mut head[r];
            let block = frame.init.cursor_block_consumed - run.used_line_height;
            vfloat::RunTail { run, block }
        });
        vfloat::place_float(&mut frame.init, &mut tail[0], clear_floor, join, measurer, viewport, hp);
        return StepOutcome::Advance;
    }
    frame.init.pending_clear = clear_floor;

    let content_block_avail = frame.init.content_block_avail;
    let content_inline = frame.init.content_inline;
    let cursor_block_consumed = frame.init.cursor_block_consumed;
    let remaining_block = (content_block_avail - cursor_block_consumed).max(0.0);
    let pcb = frame.init.pcb;

    // CSS 2.1 §9.5: an inline run beside floats gets each of its columns
    // shortened by them — the floats are handed down in the run's own frame.
    let run_floats = if matches!(frame.b.children[i].kind, BoxKind::InlineRun { .. }) {
        let (gap, _) = vfloat::block_gap(&frame.init, &frame.b.children[i], viewport, caches);
        let b_start = (cursor_block_consumed + gap).max(clear_floor.unwrap_or(f32::NEG_INFINITY));
        vfloat::floats_for_run(&frame.init, b_start)
    } else {
        None
    };

    // CSS 2.1 §8.3.1: a child of the same writing mode is a normal in-flow block,
    // its margins collapse with its own children's; an orthogonal one is not.
    let same_mode = frame.b.children[i].style.writing_mode == frame.b.style.writing_mode;
    let child = &mut frame.b.children[i];
    match dispatch_box(
        child, content_x_left, content_y, remaining_block, Some(content_inline),
        measurer, viewport, pcb, hp, same_mode, run_floats.as_ref(), AlignValue::Auto, None,
    ) {
        DispatchOutcome::Done => {
            finish_child(frame, i, viewport, caches);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsBlockFlowLoop(ci) => {
            block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport, caches);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a flex container — same
        // shape as the block-flow arm above (a different init type, so it
        // runs synchronously via its own trampoline rather than descending
        // onto this stack).
        DispatchOutcome::NeedsFlexLoop(ci) => {
            super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport, caches);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a grid container — same
        // shape as the flex arm above.
        DispatchOutcome::NeedsGridLoop(ci) => {
            super::grid_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport, caches);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a table — same shape as the
        // grid arm above.
        DispatchOutcome::NeedsTableLoop(ci) => {
            super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport, caches);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a multicol container — same
        // shape as the grid/table arms above.
        DispatchOutcome::NeedsMulticolLoop(ci) => {
            super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport, caches);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsVerticalLoop(ci) => StepOutcome::Descend(ci),
    }
}

/// Runs right after a child finishes (synchronously or via a resumed
/// descent) — repositions it to its true block-axis (physical x) position
/// and shifts its subtree, then advances the cursor. Copied from immediately
/// after the removed loop's recursive call in the pre-LAYOUT-2
/// `crate::vertical::lay_out_vertical_block`.
fn finish_child(frame: &mut Frame, i: usize, viewport: Size, caches: &mut MarginCaches) {
    let is_rtl = frame.init.is_rtl;
    let content_x_left = frame.init.content_x_left;
    let content_block_avail = frame.init.content_block_avail;
    let content_inline = frame.init.content_inline;

    // child.rect.width is the child's physical width = block-size consumed.
    let child_block = frame.b.children[i].rect.width.max(0.0);
    let cem = frame.b.children[i].style.font_size;
    // Adjacent block-axis margins of siblings collapse (§8.3.1): the larger
    // positive and the most negative one are combined.
    let (mut gap, m_end) = vfloat::block_gap(&frame.init, &frame.b.children[i], viewport, caches);
    // CSS 2.1 §8.3.1: an empty block's margins collapse through it, joining those
    // already pending and the next sibling's.
    let through = collapses_through(&frame.b.children[i]);
    if through {
        let m_start = collapsed_margin(
            &frame.b.children[i], true, is_rtl, content_inline, viewport, &mut caches.start,
        );
        let m_start = if frame.init.collapses_start && !frame.init.seen_inflow { 0.0 } else { m_start };
        gap = adjoin(adjoin(frame.init.pending_end_margin, m_start), m_end);
    }
    frame.init.cursor_block_consumed += if through { 0.0 } else { gap };
    // CSS 2.1 §9.5.2: clearance — the border edge sits at the larger of its
    // natural position and the bottom of the floats it clears.
    if let Some(floor) = frame.init.pending_clear.take() {
        frame.init.cursor_block_consumed = frame.init.cursor_block_consumed.max(floor);
    }
    let cursor_block_consumed = frame.init.cursor_block_consumed + if through { gap } else { 0.0 };
    let child = &mut frame.b.children[i];
    let placed_x = if is_rtl {
        // vertical-rl: rightmost cursor minus consumed-so-far minus this child's width.
        let right_edge = content_x_left + content_block_avail;
        right_edge - cursor_block_consumed - child_block
    } else {
        content_x_left + cursor_block_consumed
    };
    // Shift child (and any nested geometry produced during its layout).
    let dx = placed_x - child.rect.x;
    if dx != 0.0 {
        shift_subtree_x(child, dx);
    }
    // CSS 2.1 §10.3.3 along the inline (y) axis: an over-constrained block
    // ignores its inline-start margin, which for `direction: rtl` (bottom-to-top
    // inline direction, top-to-bottom for `sideways-lr`) puts it against the
    // bottom edge instead of the top one.
    let inline_start_at_end = (frame.b.style.direction == crate::style::Direction::Rtl)
        != matches!(frame.b.style.writing_mode, crate::style::WritingMode::SidewaysLr);
    if inline_start_at_end
        && matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot)
        && !child.style.margin_top.is_auto()
        && !child.style.margin_bottom.is_auto()
    {
        let mb = child.style.margin_bottom.resolve_or_zero(cem, content_inline, viewport);
        let dy = frame.init.content_y + content_inline - mb - child.rect.height - child.rect.y;
        if dy.abs() > 0.01 {
            shift_tree(child, 0.0, dy);
        }
    }
    frame.init.cursor_block_consumed += child_block;
    frame.init.pending_end_margin = if through { gap } else { m_end };
    frame.init.seen_inflow = true;
    frame.init.last_run = matches!(frame.b.children[i].kind, BoxKind::InlineRun { .. }).then_some(i);
}

/// Runs once `frame.b`'s children are all processed — finalises the physical
/// width (explicit CSS width wins; otherwise shrink-to-fit the summed child
/// widths plus padding+border), copied from the removed loop's post-loop
/// epilogue.
fn finish_frame(frame: &mut Frame) {
    // The last child's block-end margin closes the box — unless it collapses with
    // the box's own (§8.3.1): then it escapes to the parent, which reads it off the
    // box (`collapsed_margin`). A float reaching past the last child keeps it inside.
    let floats = if frame.init.encloses_floats { vfloat::floats_extent(&frame.init) } else { 0.0 };
    let end_margin = std::mem::take(&mut frame.init.pending_end_margin);
    if !(frame.init.collapses_end && floats <= frame.init.cursor_block_consumed + 0.01) {
        frame.init.cursor_block_consumed += end_margin;
    }
    // CSS 2.1 §9.5: the box encloses its floats too.
    frame.init.cursor_block_consumed = frame.init.cursor_block_consumed.max(floats);
    frame.b.rect.width = if let Some(bs) = frame.init.explicit_block_size {
        bs.max(frame.init.frame_horiz)
    } else {
        (frame.init.cursor_block_consumed + frame.init.frame_horiz)
            .min(frame.init.max_block)
            .max(frame.init.min_block)
    };
    // `vertical-rl` stacks from the content box's right edge, but while the
    // children were placed that edge was `content_block_avail` away (the room
    // offered, not the size the box ended up with). A shrink-to-fit or
    // min/max-clamped box has to pull them back onto its own right edge
    // (FLEX-VWM-2, BUG-1263: a vertical-rl flex item sized by its content).
    if frame.init.is_rtl {
        let content_block = (frame.b.rect.width - frame.init.frame_horiz).max(0.0);
        let dx = content_block - frame.init.content_block_avail;
        if dx != 0.0 {
            for child in &mut frame.b.children {
                shift_subtree_x(child, dx);
            }
        }
    }
}
