//! CSS Multi-column Layout L1 §6.1 — `column-span: all` elements that are not
//! direct children of the multicol container.
//!
//! A spanner breaks out of the columns wherever it sits in the container's
//! block-flow chain, not only as a direct child: `<div style="columns:2">
//! <section><p>…</p><h2 style="column-span:all">…</h2><p>…</p></section></div>`
//! spans the `<h2>` across the whole container, and the `<section>` is split
//! around it into two fragments. `multicol_trampoline` only knows direct
//! children (a segment ends at a child whose style carries the flag), so
//! [`hoist_nested_spanners`] rewrites the child list *before* segmentation:
//! every "transparent" wrapper that contains a spanner is replaced by
//! `[fragment-before, spanner, fragment-after]` (recursively for deeper nesting),
//! after which the existing per-segment pipeline sees plain direct-child
//! spanners.
//!
//! Fragments follow `box-decoration-break: slice` (CSS Fragmentation L3 §5.1):
//! the top border/padding/margin stays on the first fragment only, the bottom
//! ones on the last only. A wrapper whose first (last) item is the spanner
//! itself loses its top (bottom) decoration instead of keeping an empty
//! fragment for it.

use super::*;
use crate::style::WritingMode;

/// Spanners nested deeper than this many wrapper levels are left unhoisted —
/// the walk is native-recursive, and a 32-level chain of plain blocks that
/// still ends in a `column-span: all` is far beyond real markup.
const MAX_HOIST_DEPTH: usize = 32;

/// CSS Multicol §6.1 — does this box actually span? The property applies to
/// in-flow block-level elements only: a float or an absolutely/fixed positioned
/// box never spans, and neither does an inline-level box (those do not reach
/// the multicol container's child list as boxes of their own anyway).
pub(super) fn is_column_spanner(b: &LayoutBox) -> bool {
    b.style.column_span_all
        && !matches!(b.kind, BoxKind::Skip)
        && b.style.float_side == FloatSide::None
        && !matches!(b.style.position, Position::Absolute | Position::Fixed)
        && matches!(
            b.style.display,
            Display::Block
                | Display::Flex
                | Display::Grid
                | Display::FlowRoot
                | Display::Table
                | Display::ListItem
        )
}

/// Can a spanner below this box still reach the multicol container? CSS
/// Multicol §6.1: only through ancestors that take part in the container's own
/// block flow — a plain, non-floated, non-positioned-out, non-scrolling block
/// that is not itself a multicol container (a nested one owns its spanners) and
/// does not establish layout/paint/size containment.
fn is_span_transparent(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block)
        && !b.children.is_empty()
        && b.style.display == Display::Block
        && !b.style.column_span_all
        && b.style.float_side == FloatSide::None
        && !matches!(b.style.position, Position::Absolute | Position::Fixed)
        && b.style.overflow_x == Overflow::Visible
        && b.style.overflow_y == Overflow::Visible
        && b.style.column_count.is_none()
        && b.style.column_width.is_none()
        && b.style.contain == ContainFlags::NONE
        && b.style.writing_mode == WritingMode::HorizontalTb
}

/// Is there a spanner among `b`'s descendants, reachable through transparent
/// wrappers only?
fn contains_spanner(b: &LayoutBox, depth: usize) -> bool {
    if depth > MAX_HOIST_DEPTH {
        return false;
    }
    b.children
        .iter()
        .any(|c| is_column_spanner(c) || (is_span_transparent(c) && contains_spanner(c, depth + 1)))
}

/// Zeroes the decoration on one edge of a fragment (`slice`).
fn trim_edge(piece: &mut LayoutBox, top: bool) {
    let st = Arc::make_mut(&mut piece.style);
    if top {
        st.margin_top = LengthOrAuto::ZERO;
        st.padding_top = Length::Px(0.0);
        st.border_top_width = 0.0;
    } else {
        st.margin_bottom = LengthOrAuto::ZERO;
        st.padding_bottom = Length::Px(0.0);
        st.border_bottom_width = 0.0;
    }
}

/// Splits wrapper `b` (known to satisfy `contains_spanner`) into the sequence
/// `[wrapper-fragment, spanner, wrapper-fragment, …]` in source order.
fn split_around_spanners(mut b: LayoutBox, depth: usize) -> Vec<LayoutBox> {
    // Flatten children: spanners stay as they are, nested wrappers that hide a
    // spanner are replaced by their own fragment sequence.
    let mut items: Vec<LayoutBox> = Vec::new();
    for child in std::mem::take(&mut b.children) {
        if !is_column_spanner(&child)
            && is_span_transparent(&child)
            && contains_spanner(&child, depth + 1)
        {
            items.extend(split_around_spanners(child, depth + 1));
        } else {
            items.push(child);
        }
    }

    let starts_with_spanner = items.first().is_some_and(is_column_spanner);
    let ends_with_spanner = items.last().is_some_and(is_column_spanner);

    // Group each run of non-spanner items into one fragment of `b`.
    let mut out: Vec<LayoutBox> = Vec::new();
    let mut run: Vec<LayoutBox> = Vec::new();
    let flush = |run: &mut Vec<LayoutBox>, out: &mut Vec<LayoutBox>, shell: &LayoutBox| {
        if run.is_empty() {
            return;
        }
        let mut piece = shell.clone();
        piece.children = std::mem::take(run);
        out.push(piece);
    };
    for item in items {
        if is_column_spanner(&item) {
            flush(&mut run, &mut out, &b);
            out.push(item);
        } else {
            run.push(item);
        }
    }
    flush(&mut run, &mut out, &b);

    // Apply `slice` decoration: only fragments that are not the first / last
    // item of the whole sequence lose their top / bottom edge.
    let n = out.len();
    for (idx, piece) in out.iter_mut().enumerate() {
        if is_column_spanner(piece) {
            continue;
        }
        if idx > 0 || starts_with_spanner {
            trim_edge(piece, true);
        }
        if idx + 1 < n || ends_with_spanner {
            trim_edge(piece, false);
        }
    }
    out
}

/// Replaces every direct child that hides a nested `column-span: all`
/// descendant by its fragment sequence (see the module doc). A no-op — and an
/// untouched `children` — when there is no nested spanner, which is the common
/// case.
pub(super) fn hoist_nested_spanners(children: &mut Vec<LayoutBox>) {
    let needs_hoist = children
        .iter()
        .any(|c| !is_column_spanner(c) && is_span_transparent(c) && contains_spanner(c, 0));
    if !needs_hoist {
        return;
    }
    let mut out: Vec<LayoutBox> = Vec::with_capacity(children.len() + 2);
    for child in std::mem::take(children) {
        if !is_column_spanner(&child) && is_span_transparent(&child) && contains_spanner(&child, 0)
        {
            out.extend(split_around_spanners(child, 0));
        } else {
            out.push(child);
        }
    }
    *children = out;
}
