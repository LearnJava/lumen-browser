//! CSS Fragmentation L3 §3.1 / §4.1 — forced column breaks that sit *inside* a child block of a
//! multicol container.
//!
//! `multicol_trampoline` places the direct children of the container into columns and
//! honours `break-before` / `break-after: column|always` between them
//! (`multicol_fragmentation::forced_breaks`). A break that is not between two direct children —
//! `<div style="columns:2"><div><p>a</p><p style="break-before:column">b</p></div></div>` —
//! has to reach the container and cut the block it sits in (Fragmentation L3 §4.1: a break
//! between siblings propagates up through the first/last child to the wrapper's edge, and the
//! wrapper is split at the break). [`hoist_nested_breaks`] rewrites the child list *before*
//! segmentation, like `multicol_span::hoist_nested_spanners` does for `column-span: all`:
//! every transparent wrapper (`multicol_span::is_span_transparent`) with a forced break below
//! it is replaced by a sequence of fragments of itself, the later ones carrying
//! `break-before: column`, so the existing atomic pipeline sees plain sibling items with a
//! forced break between them. Fragments follow `box-decoration-break: slice`: the top
//! margin/padding/border stays on the first fragment, the bottom ones on the last.

use super::multicol_span::{is_column_spanner, is_span_transparent, trim_edge};
use super::*;
use crate::style::BreakValue;

/// Wrappers nested deeper than this are left alone (the walk is native-recursive).
const MAX_DEPTH: usize = 32;

fn forces(v: BreakValue) -> bool {
    matches!(v, BreakValue::Column | BreakValue::Always)
}

/// A child that takes part in the wrapper's block flow (not out of flow, not a placeholder).
fn is_flow(b: &LayoutBox) -> bool {
    !matches!(b.kind, BoxKind::Skip) && !matches!(b.style.position, Position::Absolute | Position::Fixed)
}

/// Fragmentation L3 §4.1: a forced break before the first child of a box is a break before the
/// box itself.
pub(super) fn breaks_before(b: &LayoutBox, depth: usize) -> bool {
    forces(b.style.break_before)
        || (depth < MAX_DEPTH
            && is_span_transparent(b)
            && b.children.iter().find(|c| is_flow(c)).is_some_and(|c| breaks_before(c, depth + 1)))
}

/// The same for a break after the last child.
pub(super) fn breaks_after(b: &LayoutBox, depth: usize) -> bool {
    forces(b.style.break_after)
        || (depth < MAX_DEPTH
            && is_span_transparent(b)
            && b.children.iter().rev().find(|c| is_flow(c)).is_some_and(|c| breaks_after(c, depth + 1)))
}

/// Does the transparent wrapper `b` hold a forced break strictly *inside* it — between two of
/// its flow children, or inside one of its own transparent children?
fn splits_inside(b: &LayoutBox, depth: usize) -> bool {
    if depth > MAX_DEPTH || !is_span_transparent(b) {
        return false;
    }
    let mut prev: Option<&LayoutBox> = None;
    for c in b.children.iter().filter(|c| is_flow(c)) {
        if is_column_spanner(c) {
            // A spanner ends the segment anyway; hoisting already split around it.
            prev = None;
            continue;
        }
        if prev.is_some_and(|p| breaks_after(p, depth + 1) || breaks_before(c, depth + 1))
            || splits_inside(c, depth + 1)
        {
            return true;
        }
        prev = Some(c);
    }
    false
}

/// Splits the wrapper `b` (`splits_inside`) at its forced breaks into fragments, source order.
fn split_at_breaks(mut b: LayoutBox, depth: usize) -> Vec<LayoutBox> {
    // Flatten: a child wrapper with a break of its own is replaced by its fragments.
    let mut items: Vec<LayoutBox> = Vec::new();
    for child in std::mem::take(&mut b.children) {
        if splits_inside(&child, depth + 1) {
            items.extend(split_at_breaks(child, depth + 1));
        } else {
            items.push(child);
        }
    }

    // Cut between two flow items when a forced break lies between them; out-of-flow and
    // placeholder items ride along with the run they follow.
    let mut out: Vec<LayoutBox> = Vec::new();
    let mut run: Vec<LayoutBox> = Vec::new();
    let mut prev_flow: Option<usize> = None; // index into `run` of the last flow item
    let flush = |run: &mut Vec<LayoutBox>, out: &mut Vec<LayoutBox>, shell: &LayoutBox| {
        if run.is_empty() {
            return;
        }
        let mut piece = shell.clone();
        piece.children = std::mem::take(run);
        out.push(piece);
    };
    for item in items {
        if is_flow(&item) {
            if let Some(p) = prev_flow
                && (breaks_after(&run[p], depth + 1) || breaks_before(&item, depth + 1))
            {
                flush(&mut run, &mut out, &b);
            }
            prev_flow = Some(run.len());
        }
        run.push(item);
    }
    flush(&mut run, &mut out, &b);

    let n = out.len();
    for (idx, piece) in out.iter_mut().enumerate() {
        if idx > 0 {
            let st = Arc::make_mut(&mut piece.style);
            st.break_before = BreakValue::Column;
            trim_edge(piece, true);
        }
        if idx + 1 < n {
            let st = Arc::make_mut(&mut piece.style);
            // The break is announced by the next fragment's `break-before`.
            st.break_after = BreakValue::Auto;
            trim_edge(piece, false);
        }
    }
    out
}

/// Replaces every direct child that hides a forced column break by its fragment sequence. A
/// no-op — and an untouched `children` — when there is none, the common case.
pub(super) fn hoist_nested_breaks(children: &mut Vec<LayoutBox>) {
    if !children.iter().any(|c| !is_column_spanner(c) && splits_inside(c, 0)) {
        return;
    }
    let mut out: Vec<LayoutBox> = Vec::with_capacity(children.len() + 2);
    for child in std::mem::take(children) {
        if !is_column_spanner(&child) && splits_inside(&child, 0) {
            out.extend(split_at_breaks(child, 0));
        } else {
            out.push(child);
        }
    }
    *children = out;
}
