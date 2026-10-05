//! CSS 2.1 §9.5 floats inside a vertical writing mode (CSS Writing Modes L3 §7.1).
//!
//! The float context of a vertical block lives in its own logical frame: the
//! *block* coordinate runs along the stacking axis (physical `x`, from the
//! right edge for `vertical-rl`) and the *inline* one along physical `y` from
//! the content box's top, so [`FloatContext`] — whose `y` is the stacking axis
//! and whose `x` is the line axis — is reused unchanged. `float: left` is the
//! line-left side, i.e. the top edge (`sideways-lr`: the bottom one).

use super::*;
use super::layout_dispatch::lay_out;
use super::vertical_margins::{adjoin, collapsed_margin, own_margins, MarginCaches};
use crate::vertical::VerticalInit;

/// Block-axis gap between the previous sibling's block-end margin and `child`'s
/// block-start margin (CSS 2.1 §8.3.1), plus the child's collapsed block-end
/// margin. Percentages resolve against the containing block's inline size (§8.3).
/// The first in-flow child of a box whose own start margin it collapses with
/// contributes none: that margin already sits in the box's.
pub(super) fn block_gap(
    init: &VerticalInit,
    child: &LayoutBox,
    viewport: Size,
    caches: &mut MarginCaches,
) -> (f32, f32) {
    let cb = init.content_inline;
    // An out-of-flow box takes no part in margin collapsing: its static position
    // is past the margin pending before it, and its own margin comes on top.
    if matches!(child.style.position, Position::Absolute | Position::Fixed) {
        let (m_start, m_end) = own_margins(child, init.is_rtl, cb, viewport);
        return (init.pending_end_margin + m_start, m_end);
    }
    let m_end = collapsed_margin(child, false, init.is_rtl, cb, viewport, &mut caches.end);
    let m_start = if init.collapses_start && !init.seen_inflow {
        0.0
    } else {
        collapsed_margin(child, true, init.is_rtl, cb, viewport, &mut caches.start)
    };
    (adjoin(init.pending_end_margin, m_start), m_end)
}

/// A float that is out of flow: `float` set and not absolutely positioned.
pub(super) fn is_float(child: &LayoutBox) -> bool {
    child.style.float_side != FloatSide::None
        && !matches!(child.style.position, Position::Absolute | Position::Fixed)
}

/// CSS 2.1 §9.5.2 — the block position `child`'s border edge must not precede
/// because of its `clear`, or `None` when it has no clearance to apply.
pub(super) fn clear_target(init: &VerticalInit, child: &LayoutBox, from: f32) -> Option<f32> {
    if init.fc.is_empty() {
        return None;
    }
    let side = match (child.style.clear, init.float_sides_swapped) {
        (ClearSide::None, _) => return None,
        (ClearSide::Left, true) => ClearSide::Right,
        (ClearSide::Right, true) => ClearSide::Left,
        (c, _) => c,
    };
    Some(init.fc.clear_y(from, side))
}

/// The container's floats in the frame of an inline run starting at block
/// position `b_start` (its first column is block `0`), or `None` without floats.
pub(super) fn floats_for_run(init: &VerticalInit, b_start: f32) -> Option<FloatContext> {
    (!init.fc.is_empty()).then(|| init.fc.rebased_block(b_start))
}

/// CSS 2.1 §9.5 (as the horizontal flow does): the block-size of the container
/// also encloses its floats. Far block edge of the lowest float, `0` without any.
pub(super) fn floats_extent(init: &VerticalInit) -> f32 {
    init.fc.left.iter().chain(init.fc.right.iter()).map(|(bot, _)| *bot).fold(0.0, f32::max)
}

/// The last column of the inline run just before a float (CSS 2.1 §9.5.1: a
/// float that follows inline content stays on that line box when it fits).
/// `block` is where the column starts in the container's block coordinates.
pub(super) struct RunTail<'a> {
    pub(super) run: &'a mut LayoutBox,
    pub(super) block: f32,
}

/// Where the text of `line` ends along the inline axis, and where it starts;
/// `None` for a blank column.
fn text_span(line: &[InlineFrag]) -> Option<(f32, f32)> {
    let mut frags = line.iter().filter(|f| !f.text.is_empty());
    let first = frags.next()?;
    Some(frags.fold((first.x, first.x + first.width), |(s, e), f| (s.min(f.x), e.max(f.x + f.width))))
}

/// CSS 2.1 §9.5.1 — places the float `child` of a vertical block: shrink-to-fit
/// along the inline axis, beside earlier floats where its margin box fits, else
/// dropped to the next block position where it does. `min_block` is the
/// clearance floor (`clear` on the float itself). With `tail`, a float that fits
/// on the last column of the preceding inline run joins it — a left one shifts
/// the text already there past itself.
pub(super) fn place_float(
    init: &mut VerticalInit,
    child: &mut LayoutBox,
    min_block: Option<f32>,
    tail: Option<RunTail>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let content_inline = init.content_inline;
    let block_start_x = init.content_x_left + init.content_block_avail;
    let b0 = (init.cursor_block_consumed + init.pending_end_margin)
        .max(min_block.unwrap_or(f32::NEG_INFINITY));
    let cem = child.style.font_size;
    let is_left = (child.style.float_side == FloatSide::Left) != init.float_sides_swapped;
    let child_vertical = !matches!(child.style.writing_mode, crate::style::WritingMode::HorizontalTb);
    let room_block = (init.content_block_avail - b0).max(0.0);
    let band_width =
        |fc: &FloatContext, b: f32| (fc.right_edge_at(b, content_inline) - fc.left_edge_at(b, 0.0)).max(0.0);

    // Lays `child` out for the line at block position `b`: an auto-height vertical
    // float shrinks to its max-content inline size (CSS 2.1 §10.3.5 on the inline
    // axis), an orthogonal horizontal one to its preferred width along the block axis.
    let lay_out_at = |child: &mut LayoutBox, init: &VerticalInit, b: f32| {
        let avail_inline = band_width(&init.fc, b);
        let (probe_w, probe_h) = if child_vertical {
            let h = if child.style.height.is_none() && matches!(child.kind, BoxKind::Block | BoxKind::FlowRoot) {
                avail_inline.min(max_content_outer_height(child, measurer, viewport))
            } else {
                avail_inline
            };
            (room_block, h)
        } else {
            let margins = child.style.margin_left.resolve_or_zero(cem, content_inline, viewport)
                + child.style.margin_right.resolve_or_zero(cem, content_inline, viewport);
            let w = if child.style.width.is_some() {
                room_block
            } else {
                preferred_inline_block_width(child, measurer, viewport)
                    .or_else(|| {
                        let w = max_content_outer_width(child, measurer, viewport);
                        (w > 0.0).then_some(w)
                    })
                    .map_or(room_block, |pw| (pw + margins).min(room_block))
            };
            (w, content_inline)
        };
        lay_out(
            child, init.content_x_left, init.content_y, probe_w, Some(probe_h), measurer, viewport,
            init.pcb, hp, false,
        );
    };
    lay_out_at(child, init, b0);

    let margins = |child: &LayoutBox| {
        let m = |l: &LengthOrAuto| l.resolve_or_zero(cem, content_inline, viewport);
        (
            m(&child.style.margin_left),
            m(&child.style.margin_right),
            m(&child.style.margin_top),
            m(&child.style.margin_bottom),
        )
    };
    let extents = |child: &LayoutBox| {
        let (ml, mr, mt, mb) = margins(child);
        (ml + child.rect.width + mr, mt + child.rect.height + mb)
    };
    let (mut be, mut ie) = extents(child);

    // CSS 2.1 §9.5.1 rule 8: a float whose margin box does not fit beside the
    // floats already there drops to the next block position where it does.
    let mut fb = b0;
    let mut joined = false;
    if let Some(RunTail { run, block }) = tail
        && min_block.is_none_or(|m| m <= block)
        && let BoxKind::InlineRun { lines, .. } = &mut run.kind
    {
        let lh = run.used_line_height.max(0.01);
        let (l, r) = init.fc.line_band(block, block + lh, 0.0, content_inline);
        let text = lines.last().and_then(|line| text_span(line));
        let text_end = text.map_or(l, |(_, e)| e.max(l));
        if ie <= r - text_end + 0.01 {
            fb = block;
            joined = true;
            if is_left && let Some(line) = lines.last_mut() {
                for f in line.iter_mut() {
                    f.x += ie;
                }
                run.rect.height = run.rect.height.max(text_end + ie);
            }
        }
    }
    while !joined && !init.fc.is_empty() {
        if ie <= band_width(&init.fc, fb) {
            break;
        }
        match init.fc.next_float_bottom(fb) {
            Some(nb) => fb = nb,
            None => break,
        }
    }
    if !joined && (fb - b0).abs() > f32::EPSILON {
        lay_out_at(child, init, fb);
        (be, ie) = extents(child);
    }

    let (ml, _mr, mt, _mb) = margins(child);
    let i0 = if is_left {
        init.fc.left_edge_at(fb, 0.0)
    } else {
        init.fc.right_edge_at(fb, content_inline) - ie
    };
    if is_left {
        init.fc.add_left(fb + be, i0 + ie);
    } else {
        init.fc.add_right(fb + be, i0);
    }
    let x0 = if init.is_rtl { block_start_x - fb - be } else { init.content_x_left + fb };
    let (target_x, target_y) = (x0 + ml, init.content_y + i0 + mt);

    // `position: relative` was applied by the trial layout already; the new
    // margin-box position has to carry it too.
    let (rel_x, rel_y) = if matches!(child.style.position, Position::Relative) {
        let rel = |a: &LengthOrAuto, b: &LengthOrAuto, basis: f32| match (a, b) {
            (LengthOrAuto::Length(l), _) => l.resolve(cem, Some(basis), viewport).unwrap_or(0.0),
            (LengthOrAuto::Auto, LengthOrAuto::Length(r)) => -r.resolve(cem, Some(basis), viewport).unwrap_or(0.0),
            (LengthOrAuto::Auto, LengthOrAuto::Auto) => 0.0,
        };
        (
            rel(&child.style.left, &child.style.right, init.content_block_avail),
            rel(&child.style.top, &child.style.bottom, content_inline),
        )
    } else {
        (0.0, 0.0)
    };
    let (dx, dy) = (target_x + rel_x - child.rect.x, target_y + rel_y - child.rect.y);
    shift_tree(child, dx, dy);
}
