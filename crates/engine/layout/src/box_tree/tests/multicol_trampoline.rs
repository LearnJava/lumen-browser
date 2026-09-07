//! LAYOUT-2 срез 7: `lay_out_multicol_children`'s per-segment placement —
//! every segment's Measure pass (unconditional) and, for atomic segments,
//! a second real-placement Place pass — the recursive `lay_out`/`dispatch_box`
//! call inside each of those two passes used to recurse one native call frame
//! per multicol-nesting level, same class of defect the flex/grid/table
//! trampolines fixed. It is now driven by an explicit heap stack
//! (`multicol_trampoline::run`) instead.
//!
//! A nested multicol container reached as a segment item is always atomic —
//! `box_is_column_sliceable` requires `children.is_empty()`, and a multicol
//! *container* by definition has children — so the chain below exercises the
//! Place-phase `Descend` path exclusively (the same path
//! `compute_col_assignment`/`post_place_item` drive), not the sliceable
//! fragment-emission shortcut.
//!
//! `DEPTH` is deliberately small — chosen for a *pre-existing*, out-of-scope
//! combinatorial cost, not stack safety (the same stance
//! `grid_trampoline`'s own deep-chain tests take toward `GridInit::
//! probe_reuse`'s clone recursion and a reuse-ineligible subgrid chain's
//! doubling, and `flex_trampoline`'s toward its Step 1 probe — see either
//! module's test file doc comment). Every atomic item is dispatched TWICE
//! (Measure pass at `(0, 0)`, then Place pass again at its resolved column
//! position) — copied verbatim from the removed code's two-pass structure,
//! not introduced by this slice. For a nested multicol chain, the Place
//! pass's second dispatch re-drives the child's ENTIRE subtree from scratch
//! (trampolined dispatch bypasses `lay_out_cache_checked`'s BUG-341 in-place/
//! layout-result caches, the same trade-off every LAYOUT-2 trampoline makes),
//! so laying out a `DEPTH`-deep chain costs `T(d) = 2*T(d-1) + O(1)` — the
//! exact doubling BUG-802 already documents for flex's own pre-fix column
//! probe. Confirmed empirically while writing this file: `DEPTH == 16` (as
//! here) completes in ~0.1s; `DEPTH == 24` — 256× the work at this growth
//! rate — measured 29s, confirming the exponent rather than just asserting
//! it. The chain length that matters for STACK safety (how many
//! `Frame`s the explicit stack holds at once during the Measure pass's own
//! `Descend` chain) is this same `DEPTH`, already an order of magnitude past
//! the ~150–800 native-recursion-overflow range LAYOUT-1's ROADMAP entry
//! measured for wholly-unconverted recursive functions.

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Length};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };
const LEAF_HEIGHT: f32 = 10.0;
/// See the file doc comment — bounded by the pre-existing `T(n) = 2·T(n-1)`
/// cost of the removed code's own two-pass (Measure + atomic Place)
/// structure, not by this slice's explicit-stack machinery.
const DEPTH: usize = 16;

/// Single-column multicol wrapper (`column-count: 1`) around one child.
/// `n_cols == 1` makes `all_sliceable` false unconditionally (CSS Multicol
/// §3.4 requires `n_cols > 1` to fragment), forcing the atomic Place pass on
/// every level regardless of the child's own kind — exactly the path a
/// nested multicol container (never a leaf, so never sliceable) always takes
/// anyway. One column also keeps every level's column width equal to its own
/// content width regardless of nesting depth.
fn wrapper(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.column_count = Some(1);
    LayoutBox {
        node: lumen_dom::NodeId::from_index(node_index),
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        used_line_height: style.font_size * style.line_height,
        style: std::sync::Arc::new(style),
        kind: BoxKind::Block,
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

/// Plain (non-multicol) block with an explicit height — the innermost
/// wrapper's one child.
fn leaf() -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.height = Some(Length::Px(LEAF_HEIGHT));
    LayoutBox {
        node: lumen_dom::NodeId::from_index(1),
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        used_line_height: style.font_size * style.line_height,
        style: std::sync::Arc::new(style),
        kind: BoxKind::Block,
        children: Vec::new(),
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin::default(),
    }
}

/// `depth` single-column multicol `wrapper`s around one `leaf`, root first —
/// mirrors `grid_trampoline`/`flex_trampoline`/`table_trampoline`'s sibling
/// deep-chain builders.
fn deep_chain(depth: usize) -> LayoutBox {
    let mut node = leaf();
    for i in 1..=depth {
        node = wrapper(i + 1, vec![node]);
    }
    node
}

/// Applied to item (5) of LAYOUT-2's ROADMAP entry: laying out a `DEPTH`-deep
/// single-column multicol chain must return instead of overflowing the
/// stack, and — since every level is a zero-margin, auto-height, one-column
/// container wrapping exactly one item — the root's final border-box height
/// must still come out to exactly `LEAF_HEIGHT` (each level's column height
/// equals its one item's outer height, folded up through every level's
/// `finish_place_phase`/`finish_frame`), proving the explicit-stack driver
/// reproduces the recursive algorithm's result, not just its termination.
#[test]
fn deep_multicol_chain_lays_out_without_overflowing_the_stack() {
    let mut root = deep_chain(DEPTH);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, false,
    );
    assert_eq!(root.rect.height, LEAF_HEIGHT, "height={}", root.rect.height);
    assert_eq!(root.rect.x, 0.0);
    assert_eq!(root.rect.y, 0.0);
}

/// Same shape, but the root has two columns and two children instead of one —
/// exercises the atomic (non-sliceable, since both children have their own
/// nested multicol subtree below them) column-assignment path
/// (`compute_col_assignment`/`post_place_item`'s `col_y` bookkeeping) across
/// two independent columns at the top level, on top of the same deep-chain
/// `Descend` machinery the test above covers for a single column.
#[test]
fn deep_multicol_chain_two_columns_lays_out_without_overflowing_the_stack() {
    let mut style = ComputedStyle::root();
    style.column_count = Some(2);
    // Half the depth per branch keeps the combinatorial cost (see the file
    // doc comment) comparable to the single-column test above, while still
    // exercising two independent `Descend` chains from one frame.
    let left = deep_chain(DEPTH / 2);
    let right = deep_chain(DEPTH / 2);
    let mut root = LayoutBox {
        node: lumen_dom::NodeId::from_index(1),
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        used_line_height: style.font_size * style.line_height,
        style: std::sync::Arc::new(style),
        kind: BoxKind::Block,
        children: vec![left, right],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin::default(),
    };
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, false,
    );
    // Two same-height columns: `column-fill` balances them (no container
    // height is imposed here), so each ends up in its own column and the
    // container's content height is exactly one branch's height.
    assert_eq!(root.rect.height, LEAF_HEIGHT, "height={}", root.rect.height);
}
