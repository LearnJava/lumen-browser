//! LAYOUT-2 срез 9: `walk`'s per-child recursion (the shared Block/FlowRoot/
//! Table/TableRow/TableRowGroup branch's plain and `preserve-3d`-sorted
//! children loops, plus `InlineBlockRow`) — a chain of nested containers
//! used to recurse one native call frame per level; it is now driven by an
//! explicit heap stack (`walk.rs::run`) instead. Boxes are built directly
//! (no HTML parse, no `lumen_layout::layout` pass) so the test exercises
//! only the paint traversal, not the html-parser's or layout's own
//! recursion — those are separate, already-addressed classes (LAYOUT-1/
//! LAYOUT-2).

use super::*;
use lumen_dom::NodeId;
use std::sync::Arc;

const DEPTH: usize = 20_000;

fn leaf_style(overflow_hidden: bool) -> ComputedStyle {
    let mut s = ComputedStyle::root();
    if overflow_hidden {
        s.overflow_x = Overflow::Hidden;
        s.overflow_y = Overflow::Hidden;
    }
    s
}

/// One box, `100×100` at `(0, 0)` (geometry is irrelevant here — no
/// `lay_out` pass runs, `walk` only reads `.rect`/`.style`, never resolves
/// it), wrapping `children`.
fn wrapper(node_index: usize, style: ComputedStyle, kind: BoxKind, children: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        node: NodeId::from_index(node_index),
        rect: Rect::new(0.0, 0.0, 100.0, 100.0),
        used_line_height: style.font_size * style.line_height,
        style: Arc::new(style),
        kind,
        children,
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin::default(),
    }
}

/// `depth` `wrapper`s around one childless leaf `wrapper`, root first — same
/// shape as `lumen-layout`'s own LAYOUT-2 deep-chain builders (`block_flow_
/// trampoline.rs` etc.). Built with a plain loop, not recursion.
fn deep_chain(depth: usize, overflow_hidden: bool, preserve_3d: bool) -> LayoutBox {
    // Leaf itself never gets `overflow_hidden` — it has no children, so its
    // own `PushClipRect`/`PopClip` pair (if any) would throw off the exact
    // `DEPTH` count below; only the `depth` wrapper levels above it do.
    let mut node = wrapper(1, leaf_style(false), BoxKind::Block, Vec::new());
    for i in 1..=depth {
        let mut s = leaf_style(overflow_hidden);
        if preserve_3d {
            s.transform_style = TransformStyle::Preserve3d;
        }
        node = wrapper(i + 1, s, BoxKind::Block, vec![node]);
    }
    node
}

/// Applied to `walk`'s shared Block/FlowRoot/Table/TableRow/TableRowGroup
/// branch's plain-order children loop (`establishes_3d_rendering_context ==
/// false`, the common case — no `transform-style: preserve-3d` anywhere in
/// the chain): building the display list for a `DEPTH`-deep chain must
/// return instead of overflowing the stack. `overflow: hidden` at every
/// level exercises `dispatch`'s `Frame`/`Epilogue::Full` machinery (not just
/// `run`'s outer loop) — each level pushes one `PushClipRect` before its
/// child and the matching `PopClip` only after that child (and everything
/// below it) is fully walked, so the counts below are proof the explicit
/// stack matches the removed recursion's push/pop pairing, not just that it
/// terminates.
#[test]
fn deep_plain_block_chain_builds_display_list_without_overflowing_the_stack() {
    let root = deep_chain(DEPTH, true, false);
    let dl = build_display_list(&root);
    let pushes = dl.iter().filter(|c| matches!(c, DisplayCommand::PushClipRect { .. })).count();
    let pops = dl.iter().filter(|c| matches!(c, DisplayCommand::PopClip)).count();
    assert_eq!(pushes, DEPTH, "one PushClipRect per level");
    assert_eq!(pops, DEPTH, "one matching PopClip per level");
}

/// Same shape, `transform-style: preserve-3d` at every level instead —
/// forces `establishes_3d_rendering_context(b) == true`, so `dispatch` takes
/// the `depth_sorted_child_order` branch (still just one child per level,
/// so the sort itself is trivial) instead of the plain-order one. Proves
/// `run`'s `Descend` chain works from either children-collection path, not
/// only the plain-order one the first test exercises.
#[test]
fn deep_preserve_3d_chain_builds_display_list_without_overflowing_the_stack() {
    let root = deep_chain(DEPTH, true, true);
    let dl = build_display_list(&root);
    let pushes = dl.iter().filter(|c| matches!(c, DisplayCommand::PushClipRect { .. })).count();
    let pops = dl.iter().filter(|c| matches!(c, DisplayCommand::PopClip)).count();
    assert_eq!(pushes, DEPTH);
    assert_eq!(pops, DEPTH);
}

/// `InlineBlockRow` — no setup/teardown of its own (`Epilogue::None`), the
/// simplest of the three converted shapes; nested directly in itself here
/// (not a realistic anonymous-box shape, but `dispatch`/`run` don't care —
/// this only stresses the traversal mechanism, same as the layout crate's
/// own synthetic deep-chain fixtures).
#[test]
fn deep_inline_block_row_chain_builds_display_list_without_overflowing_the_stack() {
    let mut node = wrapper(1, ComputedStyle::root(), BoxKind::InlineBlockRow, Vec::new());
    for i in 1..=DEPTH {
        node = wrapper(i + 1, ComputedStyle::root(), BoxKind::InlineBlockRow, vec![node]);
    }
    // No FillRect/DrawBorder possible (InlineBlockRow paints nothing of its
    // own) — an empty display list is itself the correctness assertion:
    // `dispatch` really did fall through every level to `BoxKind::Skip`'s
    // sibling arm (`InlineBlockRow`'s `{}`-shaped setup) rather than, say,
    // silently mis-descending into a `Full` epilogue that would emit
    // spurious commands.
    let dl = build_display_list(&node);
    assert!(dl.is_empty(), "InlineBlockRow paints nothing, got {} commands", dl.len());
}
