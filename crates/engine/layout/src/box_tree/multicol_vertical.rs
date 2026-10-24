//! CSS Multicol L1 §3 in a vertical writing mode (Writing Modes L3 §7.1): the columns of a
//! `vertical-*`/`sideways-rl` multicol container run along the inline axis (physically down), a
//! column is as long as the container's physical width (its block size), and the content of the
//! first column starts at the block-start edge (right for `vertical-rl`).
//!
//! `multicol_flow` (break model, pagination, cutting) is written for `horizontal-tb`: it reads
//! `rect.y`/`rect.height` as the position along the column and the top/bottom margin, padding and
//! border as its block-start/end edges. Rather than duplicating it per axis, the container's
//! items are measured with the ordinary vertical layout and then viewed through a *logical-to-
//! horizontal* map ([`to_logical`]): inline axis → `x`, block axis → `y` (reversed for `-rl`),
//! the four sides of every margin/padding/border/corner rotate to match, and `writing-mode` of
//! the container's own flow flips to `horizontal-tb`, so the horizontal engine sees a plain
//! `horizontal-tb` stack. Cut fragments are mapped back by [`from_logical`]; the map is a
//! bijection, so an uncut box returns unchanged.
//!
//! Boxes the horizontal engine cannot cut by its own geometry (flex, grid, tables) are barred
//! with a private `contain` bit for the length of the map; orthogonal flows are monolithic by
//! their `writing-mode`.
//!
//! Not covered: `direction: rtl` and `sideways-lr` (the inline axis runs upwards there), a
//! fieldset legend, `column-height`/`column-wrap`, out-of-flow boxes inside the fragmented
//! content.

use super::block_flow_trampoline::{self, DispatchOutcome};
use super::layout_dispatch::{dispatch_box, finish_after_match, FontContext};
use super::multicol_flow::{self, Flow, Item, Pg, Window};
use super::multicol_span::is_column_spanner;
use super::*;
use crate::style::{BreakValue, WritingMode};

/// Private `contain` bit marking a box [`to_logical`] made monolithic.
const BARRED_MARK: u8 = 0x80;

/// The physical axes of the container, as the map needs them.
#[derive(Clone, Copy)]
struct Axes {
    mode: WritingMode,
}

impl Axes {
    /// The block axis runs from the right edge to the left one.
    fn rtl(self) -> bool {
        matches!(self.mode, WritingMode::VerticalRl | WritingMode::SidewaysRl)
    }
}

/// Is `b` a multicol container this module lays out?
pub(super) fn applies(b: &LayoutBox, s: &ComputedStyle) -> bool {
    (s.column_count.is_some() || s.column_width.is_some())
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
        && !b.children.is_empty()
        && b.fieldset_legend.is_none()
        && matches!(s.writing_mode, WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl)
        && s.direction == Direction::Ltr
        && matches!(s.display, Display::Block | Display::FlowRoot | Display::InlineBlock | Display::ListItem)
}

// ── the logical-to-horizontal map ────────────────────────────────────────────────────────────

/// Source index of each new side `[top, right, bottom, left]` when the old ones are rotated into
/// the logical frame (`forward`) or back.
fn side_perm(rtl: bool, forward: bool) -> [usize; 4] {
    match (rtl, forward) {
        (true, true) => [1, 2, 3, 0],
        (true, false) => [3, 0, 1, 2],
        (false, _) => [3, 2, 1, 0],
    }
}

/// The same for the corners `[top-left, top-right, bottom-right, bottom-left]`.
fn corner_perm(rtl: bool, forward: bool) -> [usize; 4] {
    match (rtl, forward) {
        (true, true) => [1, 2, 3, 0],
        (true, false) => [3, 0, 1, 2],
        (false, _) => [0, 3, 2, 1],
    }
}

macro_rules! rotate_sides {
    ($st:ident, $p:expr, $a:ident, $b:ident, $c:ident, $d:ident) => {{
        let old = [$st.$a.clone(), $st.$b.clone(), $st.$c.clone(), $st.$d.clone()];
        $st.$a = old[$p[0]].clone();
        $st.$b = old[$p[1]].clone();
        $st.$c = old[$p[2]].clone();
        $st.$d = old[$p[3]].clone();
    }};
}

fn permute_style(st: &mut ComputedStyle, rtl: bool, forward: bool) {
    let p = side_perm(rtl, forward);
    rotate_sides!(st, p, margin_top, margin_right, margin_bottom, margin_left);
    rotate_sides!(st, p, padding_top, padding_right, padding_bottom, padding_left);
    rotate_sides!(st, p, border_top_width, border_right_width, border_bottom_width, border_left_width);
    // The horizontal radius of a corner is the vertical one after the map and vice versa.
    let c = corner_perm(rtl, forward);
    let rx = [
        st.border_top_left_radius.clone(),
        st.border_top_right_radius.clone(),
        st.border_bottom_right_radius.clone(),
        st.border_bottom_left_radius.clone(),
    ];
    let ry = [
        st.border_top_left_radius_y.clone(),
        st.border_top_right_radius_y.clone(),
        st.border_bottom_right_radius_y.clone(),
        st.border_bottom_left_radius_y.clone(),
    ];
    st.border_top_left_radius = ry[c[0]].clone();
    st.border_top_right_radius = ry[c[1]].clone();
    st.border_bottom_right_radius = ry[c[2]].clone();
    st.border_bottom_left_radius = ry[c[3]].clone();
    st.border_top_left_radius_y = rx[c[0]].clone();
    st.border_top_right_radius_y = rx[c[1]].clone();
    st.border_bottom_right_radius_y = rx[c[2]].clone();
    st.border_bottom_left_radius_y = rx[c[3]].clone();
}

/// The flow of the container becomes `horizontal-tb`, an orthogonal `horizontal-tb` one takes its
/// place (so it stays monolithic); the map is its own inverse.
fn swap_mode(m: WritingMode, container: WritingMode) -> WritingMode {
    if m == container {
        WritingMode::HorizontalTb
    } else if m == WritingMode::HorizontalTb {
        container
    } else {
        m
    }
}

/// Boxes whose geometry the horizontal break model reads in a way that does not survive the map.
fn is_barred_display(d: Display) -> bool {
    matches!(
        d,
        Display::Flex
            | Display::InlineFlex
            | Display::Grid
            | Display::InlineGrid
            | Display::Table
            | Display::InlineTable
            | Display::TableRowGroup
            | Display::TableHeaderGroup
            | Display::TableFooterGroup
            | Display::TableRow
            | Display::TableCell
            | Display::TableCaption
    )
}

/// Style and `used_padding` of one box, forward or back.
fn restyle(n: &mut LayoutBox, ax: Axes, forward: bool) {
    let rtl = ax.rtl();
    let st = Arc::make_mut(&mut n.style);
    permute_style(st, rtl, forward);
    st.writing_mode = swap_mode(st.writing_mode, ax.mode);
    let barred = ContainFlags::SIZE.0 | ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0;
    if forward {
        if is_barred_display(st.display) && st.contain.0 & barred == 0 {
            st.contain.0 |= BARRED_MARK | ContainFlags::LAYOUT.0;
        }
    } else if st.contain.0 & BARRED_MARK != 0 {
        st.contain.0 &= !(BARRED_MARK | ContainFlags::LAYOUT.0);
    }
    if let Some(p) = n.used_padding {
        let q = side_perm(rtl, forward);
        n.used_padding = Some([p[q[0]], p[q[1]], p[q[2]], p[q[3]]]);
    }
}

/// Styles only (no geometry) of a not yet laid out subtree — for the hoisting passes of
/// `multicol_span`/`multicol_break`, which cut wrappers by their top/bottom edges.
fn restyle_tree(list: &mut [LayoutBox], ax: Axes, forward: bool) {
    let mut stack: Vec<&mut LayoutBox> = list.iter_mut().collect();
    while let Some(n) = stack.pop() {
        restyle(n, ax, forward);
        stack.extend(n.children.iter_mut());
    }
}

/// Maps the laid-out subtree of `b` into the logical frame: `x` is the offset along the inline
/// axis from the physical top, `y` the block offset from the block-start edge of `b`.
fn to_logical(b: &mut LayoutBox, ax: Axes) {
    let rtl = ax.rtl();
    let r = if rtl { b.rect.x + b.rect.width } else { b.rect.x };
    let mut stack: Vec<&mut LayoutBox> = vec![b];
    while let Some(n) = stack.pop() {
        let Rect { x, y, width, height } = n.rect;
        n.rect = Rect::new(y, if rtl { r - (x + width) } else { x - r }, height, width);
        restyle(n, ax, true);
        stack.extend(n.children.iter_mut());
    }
}

/// The inverse of [`to_logical`] for a box placed in the logical frame of a container whose
/// content box starts at `(x0, y0)` and is `block` wide.
fn from_logical(b: &mut LayoutBox, ax: Axes, x0: f32, y0: f32, block: f32) {
    let rtl = ax.rtl();
    let mut stack: Vec<&mut LayoutBox> = vec![b];
    while let Some(n) = stack.pop() {
        let Rect { x, y, width, height } = n.rect;
        n.rect = Rect::new(if rtl { x0 + block - (y + height) } else { x0 + y }, y0 + x, height, width);
        restyle(n, ax, false);
        stack.extend(n.children.iter_mut());
    }
}

// ── layout ───────────────────────────────────────────────────────────────────────────────────

/// Dispatches one item with the column's inline size and settles whatever loop it asks for.
#[allow(clippy::too_many_arguments)]
fn dispatch_item(
    child: &mut LayoutBox,
    block_avail: f32,
    inline: f32,
    pcb: Rect,
    same_mode: bool,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    match dispatch_box(
        child, 0.0, 0.0, block_avail, Some(inline), measurer, viewport, pcb, hp, same_mode, None, AlignValue::Auto,
        None,
    ) {
        DispatchOutcome::Done => {}
        DispatchOutcome::NeedsBlockFlowLoop(ci) => { block_flow_trampoline::run(child, ci, measurer, viewport, hp); }
        DispatchOutcome::NeedsFlexLoop(ci) => { super::flex_trampoline::run(child, ci, measurer, viewport, hp); }
        DispatchOutcome::NeedsGridLoop(ci) => { super::grid_trampoline::run(child, ci, measurer, viewport, hp); }
        DispatchOutcome::NeedsTableLoop(ci) => { super::table_trampoline::run(child, ci, measurer, viewport, hp); }
        DispatchOutcome::NeedsMulticolLoop(ci) => { super::multicol_trampoline::run(child, ci, measurer, viewport, hp); }
        DispatchOutcome::NeedsVerticalLoop(ci) => { super::vertical_trampoline::run(child, ci, measurer, viewport, hp); }
    }
}

/// A run of items between two `column-span: all` boxes, and the spanner that closes it.
struct Segment {
    items: Vec<usize>,
    span: Option<usize>,
}

/// Is there anything below `list` that the hoisting passes would rewrite?
fn needs_hoist(list: &[LayoutBox], depth: usize) -> bool {
    depth < 32
        && list.iter().any(|c| {
            c.style.column_span_all
                || c.style.break_before != BreakValue::Auto
                || c.style.break_after != BreakValue::Auto
                || needs_hoist(&c.children, depth + 1)
        })
}

/// Lays out vertical multicol container `b` (see the module doc comment); `vs` is `b.style` with
/// a flex item's used-size override applied.
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_out(
    b: &mut LayoutBox,
    vs: &Arc<ComputedStyle>,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    in_block_flow: bool,
) {
    let _font_ctx = FontContext::enter(b, measurer);
    let init = crate::vertical::build_vertical_init(
        b, vs, start_x, start_y, available_width, available_height, measurer, viewport, pcb, in_block_flow,
    );
    let s = vs;
    let em = s.font_size;
    let ax = Axes { mode: s.writing_mode };
    let inline = init.content_inline;
    let col_gap = s.multicol_column_gap(em, inline, viewport);
    let n_cols = super::multicol_abspos::used_column_count(s, em, inline, col_gap, viewport) as usize;
    let col_w = ((inline - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);
    let block_avail = init.content_block_avail;
    // CSS Multicol L1 §7.1: the column height limit is the definite block size, else a definite
    // `max-width` (the block-size limit of a vertical box).
    let limit: Option<f32> = match init.explicit_block_size {
        Some(bs) => Some((bs - init.frame_horiz).max(0.0)),
        None => init.max_block.is_finite().then(|| (init.max_block - init.frame_horiz).max(0.0)),
    };
    let children_pcb = pcb;

    let mut work = std::mem::take(&mut b.children);
    if needs_hoist(&work, 0) {
        // The hoisting passes cut wrappers by their top/bottom edges: show them the logical frame.
        restyle_tree(&mut work, ax, true);
        super::multicol_span::hoist_nested_spanners(&mut work);
        super::multicol_break::hoist_nested_breaks(&mut work);
        restyle_tree(&mut work, ax, false);
    }

    let in_flow = |c: &LayoutBox| {
        !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed)
    };
    let mut segments: Vec<Segment> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for (i, c) in work.iter().enumerate().filter(|(_, c)| in_flow(c)) {
        if is_column_spanner(c) {
            segments.push(Segment { items: std::mem::take(&mut run), span: Some(i) });
        } else {
            run.push(i);
        }
    }
    segments.push(Segment { items: run, span: None });

    let same_mode = |c: &LayoutBox| c.style.writing_mode == s.writing_mode;
    let mut consumed = vec![false; work.len()];
    let mut placed: Vec<LayoutBox> = Vec::new();
    // Block offset from the block-start edge of the content box where the next segment starts.
    let mut cur_block = 0.0f32;
    for seg in &segments {
        if !seg.items.is_empty() {
            for &i in &seg.items {
                let sm = same_mode(&work[i]);
                dispatch_item(&mut work[i], block_avail, col_w, children_pcb, sm, measurer, viewport, hp);
                to_logical(&mut work[i], ax);
                consumed[i] = true;
            }
            // A segment below a spanner has the rest of the container's block size; once that is
            // used up the full limit stays.
            let seg_limit = limit.map(|l| {
                let rest = l - cur_block;
                if rest >= 1.0 { rest } else { l }
            });
            let items: Vec<Item> = seg
                .items
                .iter()
                .map(|&i| {
                    let c = &work[i];
                    let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
                    let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
                    (c, mt, mb)
                })
                .collect();
            let flow = Flow::new(&items, viewport);
            let model = &flow.model;
            let col_h = if s.column_fill_balance {
                model.balanced(n_cols, seg_limit)
            } else {
                seg_limit.unwrap_or(f32::INFINITY).max(1.0)
            };
            let h_of = |_: usize| col_h;
            let pages = model
                .paginate_ex(&h_of, false)
                .unwrap_or_else(|| vec![Pg { lo: 0.0, hi: model.end, lead: 0.0, tail: 0.0 }]);
            let mut seg_extent = 0.0f32;
            for (c, pg) in pages.iter().enumerate() {
                let win = Window {
                    lo: pg.lo,
                    hi: pg.hi,
                    lead: pg.lead,
                    tail: pg.tail,
                    last: c + 1 == pages.len(),
                    extent: col_h,
                    col_top: cur_block,
                    dx: c as f32 * (col_w + col_gap),
                };
                for &(it, off) in &flow.items {
                    if let Some(mut frag) = multicol_flow::cut(it, off, &win, 0) {
                        if matches!(frag.kind, BoxKind::InlineRun { .. }) {
                            frag.rect.width = col_w;
                        }
                        placed.push(frag);
                    }
                }
                seg_extent = seg_extent.max(pg.hi - pg.lo + pg.lead + pg.tail);
            }
            cur_block += seg_extent.max(0.0);
        }
        if let Some(si) = seg.span {
            // A spanner is as long as the whole container (Multicol L1 §6.1).
            let sm = same_mode(&work[si]);
            dispatch_item(&mut work[si], block_avail, inline, children_pcb, sm, measurer, viewport, hp);
            to_logical(&mut work[si], ax);
            consumed[si] = true;
            let shell = multicol_flow::shell(&work[si]);
            let mut sp = std::mem::replace(&mut work[si], shell);
            let c = &mut sp;
            let mbs = c.style.margin_top.resolve_or_zero(c.style.font_size, inline, viewport);
            let mbe = c.style.margin_bottom.resolve_or_zero(c.style.font_size, inline, viewport);
            let at = cur_block + mbs;
            shift_tree(c, 0.0, at);
            cur_block = at + c.rect.height + mbe;
            placed.push(sp);
        }
    }

    // The used block size: the explicit one, else what the columns took (within min/max).
    let total_block = match init.explicit_block_size {
        Some(bs) => bs.max(init.frame_horiz),
        None => (cur_block + init.frame_horiz).min(init.max_block).max(init.min_block),
    };
    let content_block = (total_block - init.frame_horiz).max(0.0);
    b.rect.width = total_block;
    for p in &mut placed {
        from_logical(p, ax, init.content_x_left, init.content_y, content_block);
    }

    let mut children = placed;
    let mut abs_deferred: Vec<(usize, f32, f32)> = Vec::new();
    for (i, c) in work.into_iter().enumerate() {
        if !consumed[i] {
            if matches!(c.style.position, Position::Absolute | Position::Fixed) {
                abs_deferred.push((children.len(), init.content_x_left, init.content_y));
            }
            children.push(c);
        }
    }
    b.children = children;

    let cb = init.content_block_avail + init.frame_horiz;
    let is_positioned = super::multicol_abspos::establishes_abs_cb(s);
    finish_after_match(b, s, em, cb, Some(cb), is_positioned, init.pcb, &abs_deferred, measurer, viewport, hp);
}
