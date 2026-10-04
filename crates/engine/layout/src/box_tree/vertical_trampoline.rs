use super::*;
use super::layout_dispatch::dispatch_box;
use super::block_flow_trampoline::{self, DispatchOutcome};
use crate::vertical::{shift_subtree_x, VerticalInit};

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
                    finish_child(&mut parent, idx, viewport);
                    parent.next_child_idx += 1;
                    current = parent;
                }
            }
            continue;
        }

        let i = current.next_child_idx;
        match step_child(&mut current, i, measurer, viewport, hp) {
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
) -> StepOutcome {
    let content_x_left = frame.init.content_x_left;
    let content_y = frame.init.content_y;

    if matches!(frame.b.children[i].kind, BoxKind::Skip) {
        frame.b.children[i].rect = Rect::new(content_x_left, content_y, 0.0, 0.0);
        return StepOutcome::Advance;
    }

    let content_block_avail = frame.init.content_block_avail;
    let content_inline = frame.init.content_inline;
    let cursor_block_consumed = frame.init.cursor_block_consumed;
    let remaining_block = (content_block_avail - cursor_block_consumed).max(0.0);
    let pcb = frame.init.pcb;

    let child = &mut frame.b.children[i];
    match dispatch_box(
        child, content_x_left, content_y, remaining_block, Some(content_inline),
        measurer, viewport, pcb, hp, false, None, AlignValue::Auto, None,
    ) {
        DispatchOutcome::Done => {
            finish_child(frame, i, viewport);
            StepOutcome::Advance
        }
        DispatchOutcome::NeedsBlockFlowLoop(ci) => {
            block_flow_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a flex container — same
        // shape as the block-flow arm above (a different init type, so it
        // runs synchronously via its own trampoline rather than descending
        // onto this stack).
        DispatchOutcome::NeedsFlexLoop(ci) => {
            super::flex_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a grid container — same
        // shape as the flex arm above.
        DispatchOutcome::NeedsGridLoop(ci) => {
            super::grid_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a table — same shape as the
        // grid arm above.
        DispatchOutcome::NeedsTableLoop(ci) => {
            super::table_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport);
            StepOutcome::Advance
        }
        // A vertical block child that is itself a multicol container — same
        // shape as the grid/table arms above.
        DispatchOutcome::NeedsMulticolLoop(ci) => {
            super::multicol_trampoline::run(&mut frame.b.children[i], ci, measurer, viewport, hp);
            finish_child(frame, i, viewport);
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
fn finish_child(frame: &mut Frame, i: usize, viewport: Size) {
    let is_rtl = frame.init.is_rtl;
    let content_x_left = frame.init.content_x_left;
    let content_block_avail = frame.init.content_block_avail;
    let content_inline = frame.init.content_inline;

    let child = &mut frame.b.children[i];
    // child.rect.width is the child's physical width = block-size consumed.
    let child_block = child.rect.width.max(0.0);
    // Physical margins along the block axis: the block-start side is the left
    // one in `vertical-lr`, the right one in `vertical-rl`. Percentages resolve
    // against the containing block's inline size (CSS 2.1 §8.3).
    let cem = child.style.font_size;
    let ml = child.style.margin_left.resolve_or_zero(cem, content_inline, viewport);
    let mr = child.style.margin_right.resolve_or_zero(cem, content_inline, viewport);
    let (m_start, m_end) = if is_rtl { (mr, ml) } else { (ml, mr) };
    // Adjacent block-axis margins of siblings collapse (§8.3.1): the larger
    // positive and the most negative one are combined.
    let prev = frame.init.pending_end_margin;
    let gap = if prev >= 0.0 && m_start >= 0.0 {
        prev.max(m_start)
    } else if prev < 0.0 && m_start < 0.0 {
        prev.min(m_start)
    } else {
        prev + m_start
    };
    frame.init.cursor_block_consumed += gap;
    let cursor_block_consumed = frame.init.cursor_block_consumed;
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
    frame.init.pending_end_margin = m_end;
}

/// Runs once `frame.b`'s children are all processed — finalises the physical
/// width (explicit CSS width wins; otherwise shrink-to-fit the summed child
/// widths plus padding+border), copied from the removed loop's post-loop
/// epilogue.
fn finish_frame(frame: &mut Frame) {
    // The last child's block-end margin closes the box (it does not collapse
    // through it: a vertical box is sized as a flow root here).
    frame.init.cursor_block_consumed += std::mem::take(&mut frame.init.pending_end_margin);
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
