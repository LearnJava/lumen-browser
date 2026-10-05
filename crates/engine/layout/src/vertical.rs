//! Vertical writing-mode layout (CSS Writing Modes L3 §3).
//!
//! Implements axis-swap layout for `writing-mode: vertical-rl` and
//! `writing-mode: vertical-lr`. In these modes:
//! - The *inline axis* runs top→bottom (physical y-direction).
//! - The *block axis* runs right→left (rl) or left→right (lr) — physical x.
//! - CSS `height` → inline-size → physical height.
//! - CSS `width`  → block-size  → physical width.
//!
//! Vertical inline text flow (`lay_out_vertical_inline_run` /
//! `wrap_inline_run_vertical`, below) is implemented: text wraps top→bottom
//! by inline-size, in addition to the block-direction stacking this header
//! used to describe as the only thing done here.
//!
//! Text orientation (rotating glyphs 90°) is a paint concern
//! (`docs/tasks/ph3-writing-mode-vertical.md`), not layout: this module only
//! computes column positions. All three backends — the CPU rasterizer, the
//! wgpu renderer (live default, ADR-017) and the femtovg fallback — honor
//! `text_orientation`, including the per-glyph `mixed` CJK-upright/
//! Latin-rotated split (`is_cjk`, below).
//!
//! Algorithm sketch (vertical-rl):
//! 1. Inline-size (physical height) comes from CSS `height` or `available_height`.
//! 2. Children stack along the block axis: rightmost child has the largest x;
//!    each subsequent child's x decreases by its physical width (= block-size).
//! 3. The container's physical width is the sum of all children's physical widths
//!    plus padding+border, unless CSS `width` is set explicitly.
//!
//! For `vertical-lr` the only change is the cursor direction: leftmost child
//! has the smallest x; the cursor increments rather than decrements.

use lumen_core::ext::HyphenationProvider;
use lumen_core::geom::{Rect, Size};

use crate::{InlineFrag, InlineSegment, TextMeasurer};
use crate::box_tree::{measure_text_w_varied, strip_soft_hyphens, BoxKind, LayoutBox};
use crate::style::{BoxSizing, Length, WritingMode};

/// CSS Writing Modes L4 §4 — codepoint ranges treated as CJK for
/// `text-orientation: mixed` (upright ideographs vs. rotated Latin/other).
/// Public: consumed by `lumen-paint` (`display_list::split_mixed_runs`) to
/// classify glyphs per character at paint time (layout itself only needs the
/// total run advance, not the per-character split).
pub fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x303F |
        0x3040..=0x309F |
        0x30A0..=0x30FF |
        0x3400..=0x4DBF |
        0x4E00..=0x9FFF |
        0xF900..=0xFAFF |
        0xFF00..=0xFFEF
    )
}

#[allow(dead_code)]
pub(crate) fn is_vertical(mode: WritingMode) -> bool {
    !matches!(mode, WritingMode::HorizontalTb)
}

/// Loop-entry state for a vertical writing-mode Block/FlowRoot container,
/// captured by [`build_vertical_init`] before any child is processed — see
/// `crate::box_tree::vertical_trampoline` for the explicit-stack driver.
/// `cursor_block_consumed` mutates once per child inside that driver; the
/// rest are read-only invariants for this box's whole loop.
pub(crate) struct VerticalInit {
    pub(crate) is_rtl: bool,
    pub(crate) content_x_left: f32,
    pub(crate) content_y: f32,
    pub(crate) content_block_avail: f32,
    pub(crate) content_inline: f32,
    pub(crate) explicit_block_size: Option<f32>,
    /// `min-width`/`max-width` — the clamp on the block size (physical width),
    /// border-box; `0.0`/`INFINITY` when unset.
    pub(crate) min_block: f32,
    pub(crate) max_block: f32,
    pub(crate) frame_horiz: f32,
    pub(crate) pcb: Rect,
    pub(crate) cursor_block_consumed: f32,
    /// Block-end margin of the last placed child, not yet added to
    /// `cursor_block_consumed`: it collapses with the next sibling's block-start
    /// margin (CSS 2.1 §8.3.1, along the block axis), or closes the box.
    pub(crate) pending_end_margin: f32,
    /// CSS 2.1 §9.5 — floats placed in this box, in its own logical frame:
    /// block coordinates run from the content box's block-start edge, inline
    /// ones from its top edge (`content_y`). `left` is the top side, `right`
    /// the bottom one; `sideways-lr` swaps which `float` value maps to which
    /// (`float_sides_swapped`).
    pub(crate) fc: crate::box_tree::FloatContext,
    /// `sideways-lr`: line-left is the *bottom* edge there, so `float: left` /
    /// `clear: left` act on the `right` bucket of `fc`.
    pub(crate) float_sides_swapped: bool,
    /// Block position (from the content box's block-start edge) a child with
    /// `clear` must start at, set by the trampoline just before the child is
    /// dispatched and consumed when its block-axis position is fixed.
    pub(crate) pending_clear: Option<f32>,
    /// Index of the last in-flow child when it is an inline run (floats that
    /// follow it may join its last column); `None` once any other in-flow
    /// child has been placed.
    pub(crate) last_run: Option<usize>,
    /// CSS 2.1 §8.3.1 — this box's block-start / block-end margin collapses with
    /// its first / last in-flow block child's: that child's margin is part of the
    /// box's own, so the first one is placed flush (`collapses_start`) and the
    /// last one's does not close the box (`collapses_end`).
    pub(crate) collapses_start: bool,
    pub(crate) collapses_end: bool,
    /// An in-flow child has already been placed.
    pub(crate) seen_inflow: bool,
    /// CSS 2.1 §10.6.7 — the box's block-size reaches its floats only when it
    /// establishes a block formatting context (or is an independent flow: the
    /// root, an orthogonal child, a flex/grid item).
    pub(crate) encloses_floats: bool,
}

/// Precomputes the loop-entry state for laying out a Block/FlowRoot box in
/// vertical writing mode — everything `dispatch_box` (`box_tree/
/// layout_dispatch.rs`) used to compute inline before its vertical arm called
/// straight through to the (removed) `lay_out_vertical_block`. Called when
/// the element's `style.writing_mode` is `VerticalRl`, `VerticalLr`,
/// `SidewaysRl`, or `SidewaysLr`.
///
/// # Parameters
/// - `style`: `b.style`, or the clone carrying a flex item's used-size override.
/// - `b`: the box to lay out (`rect.x`/`rect.y`/`rect.height` written in
///   place here; `rect.width` is finalised later by the trampoline once the
///   children's block-extent is known).
/// - `start_x`, `start_y`: top-left corner of the containing block's content area.
/// - `available_width`: physical width available; in vertical mode this is the
///   available *block-size* (room for children to stack horizontally).
/// - `available_height`: physical height available; in vertical mode this is the
///   available *inline-size* (room for the inline axis = lines of text).
/// - `measurer`: for the intrinsic `min-width`/`max-width` keywords.
/// - `viewport`, `pcb`: forwarded to child layout via the returned init.
/// - `in_block_flow`: `b` is a normal in-flow block of a parent of the same
///   writing mode — the only case where its margins collapse with its children's.
///
/// # Axis mapping
/// - `vertical-rl` / `sideways-rl`: block direction is right→left (x decreases).
/// - `vertical-lr` / `sideways-lr`: block direction is left→right (x increases).
/// - In both cases the inline direction is top→bottom (y increases).
///
/// # Limitations (Phase 0 stub)
/// - InlineRun children fall back to horizontal text flow (sideways glyphs).
/// - Margin collapsing along the block axis is not implemented.
/// - Floats and `clear` are placed by `box_tree::vertical_float`; a block child
///   is not shortened by them (only inline runs are), and `shape-outside` is
///   ignored.
/// - `min-/max-width` / `min-/max-height` are not clamped in vertical mode.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_vertical_init(
    b: &mut LayoutBox,
    style: &std::sync::Arc<crate::style::ComputedStyle>,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    in_block_flow: bool,
) -> VerticalInit {
    // `style` is `b.style` with the flex item's `UsedSizeOverride` applied (the
    // caller's `style_with_used_size`) — `width`/`height` are what the used
    // block/inline size derive from below.
    let s = style.clone();
    let em = s.font_size;

    // Physical box-model offsets. In vertical mode CSS sides keep their
    // *physical* meaning (top stays top, left stays left): the cascade does
    // not re-map padding/border to logical sides. This matches the Writing
    // Modes L3 spec — only width/height swap roles.
    // CSS Box 3 §5.2: percentage margins and padding resolve against the
    // containing block's *inline* size — the height here, since the inline axis
    // of a vertical box runs along y.
    let cb_for_percents = available_height.unwrap_or(viewport.height).max(0.0);
    let margin_left = s.margin_left.resolve_or_zero(em, cb_for_percents, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb_for_percents, viewport);
    let margin_bottom = s.margin_bottom.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb_for_percents, viewport);

    let border_left = s.border_left_width;
    let border_right = s.border_right_width;
    let border_top = s.border_top_width;
    let border_bottom = s.border_bottom_width;

    let frame_horiz = padding_left + padding_right + border_left + border_right;
    let frame_vert = padding_top + padding_bottom + border_top + border_bottom;

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;

    // Inline-size (physical height) — from CSS `height`, fall back to available.
    // `height` is the inline axis in vertical mode; auto means "fill available
    // inline-size", mirroring how `width: auto` fills the available inline-size
    // in horizontal-tb.
    let inline_size_avail = available_height
        .unwrap_or(viewport.height)
        .max(0.0);
    let inline_size = resolve_axis_size(
        s.height.as_ref(),
        em,
        Some(inline_size_avail),
        viewport,
        s.box_sizing,
        frame_vert,
    )
    // CSS 2.1 §10.3.3 along the inline axis: an auto inline-size of an in-flow block
    // fills what the containing block leaves after the box's own inline-start/end
    // margins. Boxes laid out by a flex/grid/table algorithm get the whole room (the
    // algorithm accounts for the margins itself).
    .unwrap_or(if in_block_flow {
        (inline_size_avail - margin_top - margin_bottom).max(0.0)
    } else {
        inline_size_avail
    });

    // CSS Sizing L3 §5 — `min-height`/`max-height` are the bounds of the
    // *inline* size in a vertical writing mode, however the size was reached
    // (an explicit `height` or the room offered, e.g. a flex stretch).
    let inline_limit = |len: Option<&Length>| -> Option<f32> {
        let raw = len.filter(|l| !l.is_intrinsic())?.resolve(em, Some(inline_size_avail), viewport)?;
        Some(match s.box_sizing {
            BoxSizing::ContentBox => raw + frame_vert,
            BoxSizing::BorderBox => raw.max(frame_vert),
        })
    };
    let inline_size = inline_size
        .min(inline_limit(s.max_height.as_ref()).unwrap_or(f32::INFINITY))
        .max(inline_limit(s.min_height.as_ref()).unwrap_or(0.0));

    b.rect.height = inline_size.max(frame_vert);

    // Block-size (physical width) — from CSS `width`. If absent, the container
    // shrinks to fit its children: we lay out children first, then sum their
    // physical widths.
    let explicit_block_size = resolve_axis_size(
        s.width.as_ref(),
        em,
        Some(available_width.max(0.0)),
        viewport,
        s.box_sizing,
        frame_horiz,
    );

    // CSS Sizing L3 §5 — `min-width`/`max-width` bound the block size (physical
    // width) the same way they bound the inline size of a horizontal box; the
    // content keywords measure the box's own contents.
    let block_limit = |len: Option<&Length>| -> Option<f32> {
        let len = len?;
        if len.is_intrinsic() {
            return match len {
                Length::MinContent => Some(crate::box_tree::min_content_outer_width_of_contents(b, measurer, viewport)),
                Length::MaxContent => Some(crate::box_tree::max_content_outer_width(b, measurer, viewport)),
                _ => None,
            };
        }
        let raw = len.resolve(em, Some(available_width.max(0.0)), viewport)?;
        let bb = match s.box_sizing {
            BoxSizing::ContentBox => raw + frame_horiz,
            BoxSizing::BorderBox => raw.max(frame_horiz),
        };
        Some(bb.max(0.0))
    };
    let max_block = block_limit(s.max_width.as_ref()).unwrap_or(f32::INFINITY);
    let min_block = block_limit(s.min_width.as_ref()).unwrap_or(0.0);
    // The explicit size is clamped too: max first, then min (min wins).
    let explicit_block_size = explicit_block_size.map(|v| v.min(max_block).max(min_block));

    // CSS Scrollbars L1 §6.2: the block axis is physically horizontal in a
    // vertical writing mode, so `scrollbar-gutter: stable`'s reservation for
    // the block-axis (horizontal) scrollbar reduces the *inline* dimension —
    // physical height — not width; `scrollbar_gutter_block`/`_block_start`
    // are the ones that key off `overflow-x`, matching this axis (the
    // `_inline` pair, keyed off `overflow-y`, would be wrong here — that
    // pair applies only under `writing-mode: horizontal-tb`, the branch this
    // function is never reached from). Own border-box height (`b.rect.height`
    // above) is left unreduced — only the content area handed to children
    // shrinks, same contract as `children_available_height` in
    // `layout_dispatch.rs`.
    let content_inline =
        (inline_size - frame_vert - crate::box_tree::scrollbar_gutter_block(&s)).max(0.0);
    let content_y =
        b.rect.y + border_top + padding_top + crate::box_tree::scrollbar_gutter_block_start(&s);

    // Block-axis content cursor (physical x for stacking).
    let is_rtl = matches!(
        s.writing_mode,
        WritingMode::VerticalRl | WritingMode::SidewaysRl
    );

    // If the container has explicit width, the children's available block
    // extent is bounded by that width; otherwise grow as needed.
    let content_block_avail = match explicit_block_size {
        Some(bs) => (bs - frame_horiz).max(0.0),
        None => (available_width - margin_left - frame_horiz).max(0.0),
    };

    // Starting x for the children's stacking cursor:
    //   vertical-rl: cursor starts at the right edge of the content box and
    //                moves leftwards as children are placed.
    //   vertical-lr: cursor starts at the left edge of the content box and
    //                moves rightwards.
    let content_x_left = b.rect.x + border_left + padding_left;

    // Per-child placement (tentative left-edge dispatch, then reposition to
    // the true block-axis physical x once the child's width is known) and the
    // post-loop width finalisation both used to live here — moved to
    // `crate::box_tree::vertical_trampoline` so a chain of nested vertical
    // containers drives on an explicit heap stack instead of recursing (the
    // per-child dispatch reads `.rect` back for the block-axis cursor, so it
    // cannot be a simple pre-order walk — same class as the other five
    // LAYOUT-2 dispatchers). The two "available_*" parameters threaded into
    // that per-child dispatch retain their PHYSICAL meaning across writing
    // modes (CSS Writing Modes L3 §5: containing-block dimensions are
    // physical; only `width`/`height` semantics swap): `available_width` is
    // remaining block-size, `available_height` is the parent's content
    // inline-size. Horizontal-fallback children (InlineRun, etc.) treat
    // `available_width` as physical width — they get the remaining block
    // extent, which produces sideways text inside the inline-axis strip.
    // Acceptable Phase 0 behaviour.
    VerticalInit {
        is_rtl,
        content_x_left,
        content_y,
        content_block_avail,
        content_inline,
        explicit_block_size,
        min_block,
        max_block,
        frame_horiz,
        pcb,
        cursor_block_consumed: 0.0,
        pending_end_margin: 0.0,
        fc: crate::box_tree::FloatContext::new(),
        float_sides_swapped: matches!(s.writing_mode, WritingMode::SidewaysLr),
        pending_clear: None,
        last_run: None,
        collapses_start: in_block_flow
            && crate::box_tree::escapes_start(b, is_rtl, cb_for_percents, viewport),
        collapses_end: in_block_flow
            && crate::box_tree::escapes_end(b, is_rtl, cb_for_percents, viewport),
        seen_inflow: false,
        encloses_floats: !in_block_flow || crate::box_tree::establishes_bfc(b),
    }
}

/// Resolve an axis-sizing CSS length (`width` or `height` in vertical mode).
///
/// Returns the border-box size in CSS px when the length is resolvable;
/// returns `None` for `auto`, intrinsic keywords (Phase 0), or percentage
/// without a basis. Applies `box-sizing` (`content-box` adds padding+border).
pub(crate) fn resolve_axis_size(
    len: Option<&Length>,
    em: f32,
    basis: Option<f32>,
    viewport: Size,
    sizing: BoxSizing,
    frame: f32,
) -> Option<f32> {
    let len = len?;
    if len.is_intrinsic() {
        return None;
    }
    let raw = len.resolve(em, basis, viewport)?;
    let bb = match sizing {
        BoxSizing::ContentBox => raw + frame,
        BoxSizing::BorderBox => raw.max(frame),
    };
    Some(bb.max(0.0))
}

/// Translate every rect under `b` by `dx` along the x axis.
///
/// Required because the child's layout positions descendants relative to the
/// tentative `content_x_left`; once the parent commits the child's true
/// physical x (right→left for `vertical-rl`), the whole subtree must follow.
///
/// LAYOUT-2 срез 8: `finish_child` (`box_tree::vertical_trampoline`) calls
/// this once per placed child whenever its tentative and true x differ — for
/// a `vertical-rl` chain (or any non-first sibling under `vertical-lr`) that
/// is effectively every child — so the native-recursive pre-order walk this
/// used to be defeated the trampoline's own point: an already-laid-out
/// `DEPTH`-deep subtree still overflowed the stack right here, one call
/// frame per descendant, regardless of how the dispatch loop above it was
/// driven. Explicit heap-stack pre-order walk instead — same conversion and
/// `Vec<&mut LayoutBox>` shape as `box_tree::shapes_floats::shift_tree`.
pub(crate) fn shift_subtree_x(b: &mut LayoutBox, dx: f32) {
    let mut stack: Vec<&mut LayoutBox> = vec![b];
    while let Some(node) = stack.pop() {
        node.rect.x += dx;
        // `InlineFrag::x` is an offset from the run's own origin (along y for a
        // vertical run), so it must NOT follow the box: shifting it by `dx`
        // threw vertical text off the bottom of its container.
        if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut node.kind {
            svg_paint_matrix.matrix[4] += dx;
        }
        stack.extend(node.children.iter_mut());
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_vertical_inline_run(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    _available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    _pcb: Rect,
    hp: &dyn HyphenationProvider,
    floats: Option<&crate::box_tree::FloatContext>,
) {
    let s = b.style.clone();
    let em = s.font_size;

    let inline_size_avail = available_height.unwrap_or(viewport.height).max(0.0);
    let padding_top = s.padding_top.resolve_or_zero(em, inline_size_avail, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, inline_size_avail, viewport);
    let frame_vert = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;
    let content_inline = (inline_size_avail - frame_vert).max(0.0);

    let BoxKind::InlineRun { segments, lines, .. } = &mut b.kind else {
        return;
    };
    let Some(m) = measurer else {
        return;
    };

    let wrap_budget = if s.white_space.is_nowrap() || s.text_wrap_mode == crate::style::TextWrapMode::Nowrap {
        f32::INFINITY
    } else {
        content_inline
    };

    // CSS 2.1 §9.5 — each column is a line box: beside a float it only gets the
    // inline range the float leaves free. `floats` is in the run's own frame
    // (block `0` = its first column), so column `k` is `[k·lh, (k+1)·lh)`.
    let col_step = b.used_line_height.max(0.01);
    let band_of = |k: usize| -> (f32, f32) {
        let fc = floats.filter(|fc| !fc.is_empty());
        let Some(fc) = fc else { return (0.0, wrap_budget) };
        let top = k as f32 * col_step;
        let (l, r) = fc.line_band(top, top + col_step, 0.0, content_inline);
        let l = l.clamp(0.0, content_inline);
        // An unwrapped run keeps its single line, only moved clear of the float.
        let budget = if wrap_budget.is_finite() { (r.max(l) - l).max(0.0) } else { f32::INFINITY };
        (l, budget)
    };
    let banded = floats.is_some_and(|fc| !fc.is_empty());

    *lines = wrap_inline_run_vertical(
        segments,
        wrap_budget,
        em,
        viewport,
        m,
        hp,
        s.white_space,
        s.word_break,
        s.overflow_wrap,
        s.writing_mode,
        s.text_orientation,
        banded.then_some(&band_of as &dyn Fn(usize) -> (f32, f32)),
    );

    // The run's inline extent is its longest column, not the sum over all of
    // them, and each wrapped line is a column of its own: the box is
    // `lines × line-height` wide (BUG-1264 — text wrapped by the box's
    // inline-size). `emit_inline_run_vertical` places column N at the same
    // `N * used_line_height` step inside this rect.
    let longest_column = lines
        .iter()
        // The empty spacer frag a soft wrap leaves at a column's end is
        // trailing whitespace: it hangs, it doesn't lengthen the column.
        .map(|l| l.iter().filter(|f| !f.text.is_empty()).map(|f| f.x + f.width).fold(0.0_f32, f32::max))
        .fold(0.0_f32, f32::max);
    let min_height = b.used_line_height;
    let total_vertical_extent = longest_column.max(min_height);

    b.rect.x = start_x;
    b.rect.y = start_y;
    let col_width = b.used_line_height;
    b.rect.width = col_width * lines.len().max(1) as f32;
    b.rect.height = total_vertical_extent;
}

/// `(start, end)` of column `k`'s inline range from the per-column
/// `(start, budget)` provider of [`wrap_inline_run_vertical`].
fn column_range(band_for: &dyn Fn(usize) -> (f32, f32), k: usize) -> (f32, f32) {
    let (start, budget) = band_for(k);
    (start, start + budget)
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn wrap_inline_run_vertical(
    segments: &[InlineSegment],
    max_height: f32,
    container_font_size: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    _hp: &dyn HyphenationProvider,
    _white_space: crate::style::WhiteSpace,
    _word_break: crate::style::WordBreak,
    _overflow_wrap: crate::style::OverflowWrap,
    _writing_mode: WritingMode,
    _text_orientation: crate::style::TextOrientation,
    band: Option<&dyn Fn(usize) -> (f32, f32)>,
) -> Vec<Vec<InlineFrag>> {
    let space_w = m.char_width(' ', container_font_size);

    // Per-column inline range `(start, budget)`: the whole `max_height` from the
    // run's top unless a float narrows the column (CSS 2.1 §9.5).
    let band_for = |k: usize| band.map_or((0.0, max_height), |f| f(k));
    let (mut line_start, first_budget) = band_for(0);
    let mut line_end = line_start + first_budget;
    let mut result: Vec<Vec<InlineFrag>> = vec![Vec::new()];
    let mut current_line: &mut Vec<InlineFrag> = result.last_mut().unwrap();
    let mut current_y: f32 = line_start;
    let mut prev_trailing_ws: bool = false;

    for seg in segments {
        if seg.forced_break {
            if !current_line.is_empty() && !current_line.last().map(|f| f.text == "\n").unwrap_or(false) {
                current_line.push(InlineFrag {
                    x: 0.0,
                    y_offset: 0.0,
                    width: 0.0,
                    text: "\n".to_string(),
                    style: seg.style.clone(),
                    padding_left: 0.0,
                    padding_right: 0.0,
                    is_element_box: false,
                    img_src: None,
                    img_is_lazy: false,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: seg.source_char_offset,
                    bidi_level: seg.bidi_level,
                    merged_sources: Vec::new(),
                });
            }
            current_y = line_start;
            prev_trailing_ws = false;
            continue;
        }

        let seg_lead_ws = seg.text.starts_with(|c: char| c.is_whitespace());
        let seg_trail_ws = seg.text.ends_with(|c: char| c.is_whitespace());

        if _white_space.preserves_whitespace() {
            if seg.text.is_empty() {
                continue;
            }
            prev_trailing_ws = false;
            let style = &seg.style;
            let em_s = style.font_size;
            let ls = style.letter_spacing;
            let tab_size = style.tab_size;
            let pad_l = style.padding_left.resolve_or_zero(em_s, max_height, viewport);
            let _pad_r = style.padding_right.resolve_or_zero(em_s, max_height, viewport);
            let frag_h = measure_text_w_varied(&seg.text, em_s, ls, tab_size, &style.font_family, &style.font_variation_settings, m);
            current_line.push(InlineFrag {
                x: current_y,
                y_offset: 0.0,
                width: frag_h,
                text: seg.text.clone(),
                style: style.clone(),
                padding_left: pad_l,
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
                merged_sources: Vec::new(),
            });
            current_y += frag_h;
            continue;
        }

        if let Some(img_src) = &seg.img_src {
            let img_advance = m.char_width(' ', container_font_size) * 3.0;
            if !current_line.is_empty() && current_y + img_advance > line_end {
                let next_col = result.len();
                result.push(Vec::new());
                current_line = result.last_mut().unwrap();
                (line_start, line_end) = column_range(&band_for, next_col);
                current_y = line_start;
            }
            current_line.push(InlineFrag {
                x: current_y,
                y_offset: 0.0,
                width: img_advance,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: true,
                img_src: Some(img_src.clone()),
                img_is_lazy: seg.img_is_lazy,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
                bidi_level: seg.bidi_level,
                merged_sources: Vec::new(),
            });
            current_y += img_advance;
            prev_trailing_ws = seg_trail_ws;
            continue;
        }

        let raw_words: Vec<&str> = seg.text.split_whitespace().collect();
        if raw_words.is_empty() {
            if seg_lead_ws || seg_trail_ws {
                prev_trailing_ws = true;
            }
            continue;
        }

        let style = &seg.style;
        let em_s = style.font_size;
        let ls = style.letter_spacing;
        let ws = style.word_spacing;
        let inter_word = space_w + ls + ws;
        let pad_l = style.padding_left.resolve_or_zero(em_s, max_height, viewport);
        let _pad_r = style.padding_right.resolve_or_zero(em_s, max_height, viewport);

        let n = raw_words.len();
        for (wi, raw_word) in raw_words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;
            let (display_word, _) = strip_soft_hyphens(raw_word);

            let frag_source_offset = {
                let raw_ptr = raw_word.as_ptr() as usize;
                let seg_ptr = seg.text.as_ptr() as usize;
                let word_off = if raw_ptr >= seg_ptr && raw_ptr <= seg_ptr + seg.text.len() {
                    (raw_ptr - seg_ptr) as u32
                } else {
                    0u32
                };
                seg.source_char_offset.saturating_add(word_off)
            };

            let pre = if is_seg_first { seg.pre_space } else { 0.0 };
            let post = if is_seg_last { seg.post_space } else { 0.0 };

            let word_h = measure_text_w_varied(&display_word, em_s, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
            let word_inter = if is_seg_first && !(prev_trailing_ws || seg_lead_ws) { 0.0 } else { inter_word };

            // CSS 2.1 §9.5: a column that cannot hold even its first word beside
            // a float moves on past it; the column grid is uniform, so the skipped
            // one stays in the run as a blank column.
            macro_rules! skip_narrow_columns {
                () => {
                    let mut skipped = 0;
                    while current_line.is_empty()
                        && band.is_some()
                        && (line_start > 0.0 || line_end < max_height)
                        && line_start + word_h > line_end + 0.5
                        && skipped < 4096
                    {
                        let next_col = result.len();
                        result.push(Vec::new());
                        current_line = result.last_mut().unwrap();
                        (line_start, line_end) = column_range(&band_for, next_col);
                        current_y = line_start;
                        skipped += 1;
                    }
                };
            }
            skip_narrow_columns!();
            let gap = if current_line.is_empty() { 0.0 } else { word_inter };
            let needs_wrap = !current_line.is_empty()
                && current_y + gap + pre + word_h > line_end;

            if needs_wrap {
                current_line.push(InlineFrag {
                    x: current_y,
                    y_offset: 0.0,
                    width: gap + pre,
                    text: " ".repeat(0),
                    style: style.clone(),
                    padding_left: if is_seg_first { pad_l } else { 0.0 },
                    padding_right: 0.0,
                    is_element_box: seg.is_element_box,
                    img_src: None,
                    img_is_lazy: false,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: frag_source_offset,
                    bidi_level: seg.bidi_level,
                    merged_sources: Vec::new(),
                });
                let next_col = result.len();
                result.push(Vec::new());
                current_line = result.last_mut().unwrap();
                (line_start, line_end) = column_range(&band_for, next_col);
                current_y = line_start;
                skip_narrow_columns!();
            }

            let _entry_pre = if is_seg_first { pre } else { 0.0 };
            // The word gap decided `needs_wrap` above; when the word stays on
            // this column it also has to advance the cursor, or words run
            // together ("helloworld").
            if !needs_wrap && !current_line.is_empty() {
                current_y += gap + pre;
            }
            current_line.push(InlineFrag {
                x: current_y,
                y_offset: 0.0,
                width: word_h,
                text: display_word.to_string(),
                style: style.clone(),
                padding_left: if is_seg_first { pad_l } else { 0.0 },
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: frag_source_offset,
                bidi_level: seg.bidi_level,
                merged_sources: Vec::new(),
            });
            current_y += word_h + post;
            prev_trailing_ws = seg_trail_ws;
        }
    }

    if result.is_empty() {
        result.push(Vec::new());
    }
    result
}

#[cfg(test)]
mod tests {
    use lumen_core::geom::Size;

    use super::*;
    use crate::BoxKind;

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        crate::box_tree::layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    /// Walk the tree to find the first descendant `Block` element whose style
    /// has the requested writing mode set (i.e. the test's `<div>` under test).
    fn find_vertical_block(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block)
            && !matches!(b.style.writing_mode, WritingMode::HorizontalTb)
        {
            return Some(b);
        }
        for c in &b.children {
            if let Some(found) = find_vertical_block(c) {
                return Some(found);
            }
        }
        None
    }

    fn first_non_skip_child(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip))
    }

    #[test]
    fn vertical_rl_container_height_is_inline_size() {
        let root = lay(
            "<div id=v><div></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 300px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        assert!(
            (v.rect.height - 200.0).abs() < 0.5,
            "expected physical height 200 (CSS height = inline-size), got {}",
            v.rect.height,
        );
        assert!(
            (v.rect.width - 300.0).abs() < 0.5,
            "expected physical width 300 (CSS width = block-size), got {}",
            v.rect.width,
        );
    }

    #[test]
    fn vertical_lr_container_height_is_inline_size() {
        let root = lay(
            "<div id=v><div></div></div>",
            "#v { writing-mode: vertical-lr; height: 250px; width: 120px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        assert!(
            (v.rect.height - 250.0).abs() < 0.5,
            "expected physical height 250, got {}",
            v.rect.height,
        );
        assert!(
            (v.rect.width - 120.0).abs() < 0.5,
            "expected physical width 120, got {}",
            v.rect.width,
        );
    }

    #[test]
    fn vertical_rl_single_child_fills_inline_extent() {
        // Single child with no explicit height should fill the parent's
        // inline-size (= parent's physical height = 200px).
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 100px; } \
             .c { width: 40px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        assert!(
            (c.rect.height - 200.0).abs() < 0.5,
            "child should fill parent's inline-size (200), got {}",
            c.rect.height,
        );
    }

    #[test]
    fn vertical_rl_children_stack_right_to_left() {
        let root = lay(
            "<div id=v><div class=a></div><div class=b></div></div>",
            "#v { writing-mode: vertical-rl; height: 100px; width: 200px; } \
             .a { width: 50px; } .b { width: 50px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let kids: Vec<&LayoutBox> = v
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert!(kids.len() >= 2, "expected at least 2 children");
        // First child (.a) should be to the right of the second (.b).
        assert!(
            kids[0].rect.x > kids[1].rect.x,
            "vertical-rl: first child should be rightmost, got a.x={} b.x={}",
            kids[0].rect.x,
            kids[1].rect.x,
        );
    }

    #[test]
    fn vertical_lr_children_stack_left_to_right() {
        let root = lay(
            "<div id=v><div class=a></div><div class=b></div></div>",
            "#v { writing-mode: vertical-lr; height: 100px; width: 200px; } \
             .a { width: 50px; } .b { width: 50px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let kids: Vec<&LayoutBox> = v
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert!(kids.len() >= 2, "expected at least 2 children");
        // First child (.a) should be to the left of the second (.b).
        assert!(
            kids[0].rect.x < kids[1].rect.x,
            "vertical-lr: first child should be leftmost, got a.x={} b.x={}",
            kids[0].rect.x,
            kids[1].rect.x,
        );
    }

    #[test]
    fn vertical_rl_explicit_child_block_size() {
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-rl; height: 100px; width: 200px; } \
             .c { width: 60px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        // Child inherits writing-mode from parent; its CSS width (60) is its
        // block-size = physical width.
        assert!(
            (c.rect.width - 60.0).abs() < 0.5,
            "child explicit width 60 should yield physical width 60, got {}",
            c.rect.width,
        );
    }

    #[test]
    fn vertical_rl_auto_container_width_grows() {
        // No explicit width on the container; physical width should equal the
        // sum of children's physical widths (here 40+30 = 70).
        let root = lay(
            "<div id=v><div class=a></div><div class=b></div></div>",
            "#v { writing-mode: vertical-rl; height: 100px; } \
             .a { width: 40px; } .b { width: 30px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        assert!(
            (v.rect.width - 70.0).abs() < 0.5,
            "auto-width container should shrink-to-fit children (70), got {}",
            v.rect.width,
        );
    }

    #[test]
    fn vertical_rl_nested_containers() {
        // Outer vertical-rl with two children; the second child is itself a
        // vertical-rl block. Both inner and outer should layout independently
        // without panicking, and the inner container should have a sensible
        // physical size.
        let root = lay(
            "<div id=v><div class=a></div><div id=inner><div class=ic></div></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 300px; } \
             .a { width: 50px; } \
             #inner { writing-mode: vertical-rl; height: 100px; width: 80px; } \
             .ic { width: 25px; }",
        );
        let v = find_vertical_block(&root).expect("outer vertical block missing");
        // Outer must have explicit physical size (300×200).
        assert!((v.rect.width - 300.0).abs() < 0.5);
        assert!((v.rect.height - 200.0).abs() < 0.5);
        // The second child (#inner) must have its own explicit physical size (80×100).
        let kids: Vec<&LayoutBox> = v
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert!(kids.len() >= 2, "expected at least 2 children, got {}", kids.len());
        let inner = kids[1];
        assert!(
            (inner.rect.width - 80.0).abs() < 0.5,
            "inner #inner physical width should be 80, got {}",
            inner.rect.width,
        );
        assert!(
            (inner.rect.height - 100.0).abs() < 0.5,
            "inner #inner physical height should be 100, got {}",
            inner.rect.height,
        );
    }

    // BUG-504 remainder (part 5): `scrollbar-gutter` under a vertical writing
    // mode. The block axis is physically horizontal there, so it is
    // `overflow-x` (not `overflow-y`) that triggers the reservation, and the
    // dimension it reserves is physical height (the inline dimension), not
    // width. WPT `css/css-overflow/scrollbar-gutter-vertical-{lr,rl}-001.html`.

    #[test]
    fn vertical_lr_scrollbar_gutter_stable_reduces_child_inline_size() {
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-lr; height: 200px; width: 200px; \
                  overflow-x: auto; scrollbar-gutter: stable; } \
             .c { height: 100%; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        assert!(
            (c.rect.height - 188.0).abs() < 0.5,
            "expected 200 - 12 gutter unit = 188, got {}",
            c.rect.height,
        );
        assert!(
            (c.rect.y - v.rect.y).abs() < 0.5,
            "plain `stable` reserves the end edge only — child top must stay \
             flush with the container's, got child.y={} container.y={}",
            c.rect.y,
            v.rect.y,
        );
    }

    #[test]
    fn vertical_lr_scrollbar_gutter_stable_both_edges_shifts_and_double_reduces() {
        // `both-edges` mirrors the gutter onto the block-start (physical top)
        // edge too: double the reduction (24, not 12) and the child starts one
        // unit further down — WPT's "… stable both-edges" subtest asserts both
        // facts, plus a cross-check that this reduction exceeds plain `stable`'s.
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-lr; height: 200px; width: 200px; \
                  overflow-x: auto; scrollbar-gutter: stable both-edges; } \
             .c { height: 100%; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        assert!(
            (c.rect.height - 176.0).abs() < 0.5,
            "expected 200 - 2*12 gutter unit = 176, got {}",
            c.rect.height,
        );
        assert!(
            c.rect.y > v.rect.y + 0.5,
            "`both-edges` must shift the child's top edge down, got \
             child.y={} container.y={}",
            c.rect.y,
            v.rect.y,
        );
    }

    #[test]
    fn vertical_rl_scrollbar_gutter_stable_both_edges_matches_vertical_lr() {
        // The gutter's physical top/bottom placement doesn't depend on the
        // block-flow direction (`vertical-lr` vs `vertical-rl` only affects the
        // *other*, block/width axis) — same numbers as the `vertical-lr` case.
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 200px; \
                  overflow-x: auto; scrollbar-gutter: stable both-edges; } \
             .c { height: 100%; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        assert!(
            (c.rect.height - 176.0).abs() < 0.5,
            "expected 176, got {}",
            c.rect.height,
        );
        assert!(
            c.rect.y > v.rect.y + 0.5,
            "expected shifted top edge, got child.y={} container.y={}",
            c.rect.y,
            v.rect.y,
        );
    }

    #[test]
    fn vertical_lr_scrollbar_gutter_auto_no_reduction() {
        // Default `scrollbar-gutter: auto` — Lumen's overlay scrollbars never
        // reserve layout space, so the child fills the full inline extent.
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-lr; height: 200px; width: 200px; \
                  overflow-x: auto; } \
             .c { height: 100%; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        assert!(
            (c.rect.height - 200.0).abs() < 0.5,
            "expected no reduction (200), got {}",
            c.rect.height,
        );
    }
}
