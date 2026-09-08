//! LAYOUT-2 срез 8 (final dispatcher item): `vertical.rs`'s per-child
//! block-axis stacking loop (CSS Writing Modes L3 §3) — reads each child's
//! `.rect` back to reposition it to its true physical x and to advance the
//! block-axis cursor, so it is non-tail-recursive the same way the other five
//! LAYOUT-2 dispatchers are. It is now driven by an explicit heap stack
//! (`vertical_trampoline::run`) instead of `vertical.rs`'s own removed
//! recursive loop.
//!
//! Writing this file's first version at `DEPTH = 200_000` (matching
//! `block_flow_trampoline`'s own single-dispatch-per-level depth) caught a
//! SECOND, independent native-recursion source the trampolined dispatch loop
//! does nothing about: `finish_child` repositions each child from its
//! tentative left-edge placement to its true block-axis x via
//! `shift_subtree_x`, which — before this slice — walked the child's whole
//! already-laid-out subtree on the native call stack, one frame per
//! descendant, same defect class as the dispatch recursion itself but
//! entirely separate code. A `vertical-rl` chain shifts on essentially every
//! level (the tentative and true x coincide only when the container's full
//! available block extent happens to equal the child's own size), so this
//! alone overflowed the stack at `DEPTH` in the low tens of thousands, well
//! before dispatch depth was ever the bottleneck. Fixed in `vertical.rs`
//! alongside this test, converting `shift_subtree_x` to the same
//! `Vec<&mut LayoutBox>` explicit-stack walk `box_tree::shapes_floats::
//! shift_tree` already uses.
//!
//! That fix makes the walk iterative but not cheap: it still visits every
//! node of the shifted subtree, so a `vertical-rl` chain that shifts at
//! (near) every level costs `O(depth)` per shift call against an `O(depth)`-
//! deep subtree — `O(depth²)` total, the same complexity class
//! `grid_trampoline`/`multicol_trampoline`'s own doc comments document for
//! their unrelated combinatorial reasons. `DEPTH` is chosen accordingly: the
//! literal `<div>×20000` LAYOUT-2 ROADMAP acceptance number (not
//! `block_flow_trampoline`'s 200_000, which would push this specific test
//! from ~1s into minutes) — comfortably past the ~150–800-level native
//! recursion threshold LAYOUT-1's ROADMAP entry measured, while keeping the
//! `O(depth²)` shift cost in the low seconds. Turning this into a real
//! `O(depth)` fix (e.g. computing each child's true x before its own layout
//! instead of shifting after) is a separate, out-of-scope algorithmic
//! change — the same stance the multicol abs-in-abs chain and flex's
//! BUG-802 doubling take toward their own pre-existing costs.

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Length, WritingMode};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };
const LEAF_WIDTH: f32 = 10.0;
// Same acceptance-criterion depth as `block_flow_trampoline`'s own deep-chain
// test — single dispatch per level, no probe/measure doubling, so `O(depth)`
// total holds at this size the same way it does there.
const DEPTH: usize = 20_000;

/// `writing-mode: vertical-rl`/`vertical-lr` `Block`, auto width/height —
/// wraps its single child so the child's physical width (= its own
/// block-size, CSS Writing Modes L3 §3) becomes this wrapper's own physical
/// width (`finish_frame`'s shrink-to-fit), contributing nothing but
/// recursion depth to the final geometry.
fn vertical_wrapper(node_index: usize, mode: WritingMode, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.writing_mode = mode;
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

/// Plain horizontal-writing-mode `Block` with an explicit CSS `width` —
/// inside a vertical-writing-mode parent this is the value `step_child`
/// reads back as `child.rect.width` (the physical block-size it consumes),
/// so the root's final `rect.width` is a direct readout of whether the whole
/// chain laid out correctly, not just "didn't crash". Always index 1 (never
/// 0, matching the sibling trampoline tests' convention).
fn leaf() -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.width = Some(Length::Px(LEAF_WIDTH));
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

/// `depth` single-child vertical `vertical_wrapper`s (all the same `mode`)
/// around one `leaf`, root first. Node indices count down from `depth + 1`
/// at the root to `1` at the leaf, mirroring the sibling trampoline tests.
fn deep_chain(depth: usize, mode: WritingMode) -> LayoutBox {
    let mut node = leaf();
    for i in 1..=depth {
        node = vertical_wrapper(i + 1, mode, vec![node]);
    }
    node
}

/// LAYOUT-2 acceptance criterion for this item (ROADMAP: "`--dump-layout`/
/// `--screenshot`/live `--print-to-pdf` on `<div>`×20000 … without increased
/// stacks"), for a `vertical-rl` chain: laying out a `DEPTH`-deep single-child
/// chain must return instead of overflowing the stack, and — since every
/// level auto-sizes to wrap exactly its one child — the root's final
/// border-box width must still come out to exactly `LEAF_WIDTH`, proving the
/// explicit-stack driver reproduces the recursive algorithm's result, not
/// just its termination. `vertical-rl` exercises `is_rtl == true` in
/// `finish_child`'s placement math (cursor counts down from the right edge).
#[test]
fn deep_vertical_rl_chain_lays_out_without_overflowing_the_stack() {
    let mut root = deep_chain(DEPTH, WritingMode::VerticalRl);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, false,
    );
    assert_eq!(root.rect.width, LEAF_WIDTH, "width={}", root.rect.width);
    assert_eq!(root.rect.x, 0.0);
    assert_eq!(root.rect.y, 0.0);
}

/// Same shape, but `vertical-lr` — `is_rtl == false` in `finish_child`, the
/// other branch of the same placement math (cursor counts up from the left
/// edge) — and the root has two independent deep sub-chains as direct
/// siblings instead of one, exercising `run`'s per-frame `next_child_idx`
/// loop (advance to the second child, including its own full `Descend`
/// chain, after the first child's chain finishes and pops) rather than only
/// the single-child-per-frame path the test above covers. Two same-width
/// leaves stacked along the block axis: the root's final width is exactly
/// twice `LEAF_WIDTH`, proving the cursor correctly sums both children's
/// consumed block-size rather than, say, only keeping the last one.
#[test]
fn deep_vertical_lr_chain_two_siblings_lays_out_without_overflowing_the_stack() {
    let left = deep_chain(DEPTH / 2, WritingMode::VerticalLr);
    let right = deep_chain(DEPTH / 2, WritingMode::VerticalLr);
    let mut root = vertical_wrapper(DEPTH + 2, WritingMode::VerticalLr, vec![left, right]);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, false,
    );
    assert_eq!(root.rect.width, 2.0 * LEAF_WIDTH, "width={}", root.rect.width);
    assert_eq!(root.rect.x, 0.0);
    assert_eq!(root.rect.y, 0.0);
}
