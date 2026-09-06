//! LAYOUT-1: `collapsed_top_margin`/`collapsed_bottom_margin` (`box_tree/bfc.rs`)
//! used to recurse one call frame per link in the first-/last-child chain —
//! on a page that is a straight run of single-child `<div>`s (BUG-987's
//! fandom.com/OneTrust repro shape) that chain is the full DOM depth, making
//! it an independent stack-overflow source from `lay_out_inner`'s own
//! descent. These tests lock in the max-of-chain result the old recursion
//! produced and prove the new loop survives a chain far deeper than any
//! stack would tolerate one frame per level.

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Length, LengthOrAuto};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};
use super::super::{collapsed_bottom_margin, collapsed_top_margin};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };

/// Plain `Block` with the given margin, zero padding/border, `height: auto` —
/// the shape `first_collapsible_child`/`last_collapsible_child` accept as a
/// link in the collapsing chain.
fn block_with_margins(margin_top_px: f32, margin_bottom_px: f32, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.margin_top = LengthOrAuto::Length(Length::Px(margin_top_px));
    style.margin_bottom = LengthOrAuto::Length(Length::Px(margin_bottom_px));
    LayoutBox {
        node: lumen_dom::NodeId::from_index(0),
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

#[test]
fn collapsed_top_margin_folds_the_whole_first_child_chain() {
    // root(5) -> child1(20) -> child2(8), all plain blocks, nothing breaks
    // the chain -> the collapsed top margin is the max of all three.
    let child2 = block_with_margins(8.0, 0.0, vec![]);
    let child1 = block_with_margins(20.0, 0.0, vec![child2]);
    let root = block_with_margins(5.0, 0.0, vec![child1]);
    let got = collapsed_top_margin(&root, 800.0, VIEWPORT);
    assert!((got - 20.0).abs() < 0.01, "expected max(5,20,8)=20, got {got}");
}

#[test]
fn collapsed_top_margin_stops_at_a_child_with_top_padding() {
    // child1 has non-zero padding-top, which breaks the chain at child1: its
    // own margin still counts, but child2's (100px) never gets folded in.
    let child2 = block_with_margins(100.0, 0.0, vec![]);
    let mut child1 = block_with_margins(20.0, 0.0, vec![child2]);
    std::sync::Arc::make_mut(&mut child1.style).padding_top = Length::Px(4.0);
    let root = block_with_margins(5.0, 0.0, vec![child1]);
    let got = collapsed_top_margin(&root, 800.0, VIEWPORT);
    assert!((got - 20.0).abs() < 0.01, "expected max(5,20)=20 (100 excluded), got {got}");
}

#[test]
fn collapsed_bottom_margin_folds_the_whole_last_child_chain() {
    let child2 = block_with_margins(0.0, 8.0, vec![]);
    let child1 = block_with_margins(0.0, 20.0, vec![child2]);
    let root = block_with_margins(0.0, 5.0, vec![child1]);
    let got = collapsed_bottom_margin(&root, 800.0, VIEWPORT);
    assert!((got - 20.0).abs() < 0.01, "expected max(5,20,8)=20, got {got}");
}

#[test]
fn collapsed_bottom_margin_stops_at_a_definite_height() {
    // child1 has an explicit height, which blocks the through-collapse at
    // child1: child2's 100px bottom margin never escapes past it.
    let child2 = block_with_margins(0.0, 100.0, vec![]);
    let mut child1 = block_with_margins(0.0, 20.0, vec![child2]);
    std::sync::Arc::make_mut(&mut child1.style).height = Some(Length::Px(10.0));
    let root = block_with_margins(0.0, 5.0, vec![child1]);
    let got = collapsed_bottom_margin(&root, 800.0, VIEWPORT);
    assert!((got - 20.0).abs() < 0.01, "expected max(5,20)=20 (100 excluded), got {got}");
}

/// The actual LAYOUT-1 regression guard: a chain deep enough that the old
/// per-level recursion would have overflowed a normal (non-bumped) thread
/// stack long before reaching the bottom. Built bottom-up in a loop so the
/// *construction* itself never recurses either.
#[test]
fn collapsed_margins_survive_a_very_deep_single_child_chain() {
    const DEPTH: usize = 200_000;
    let mut node = block_with_margins(1.0, 1.0, vec![]);
    for _ in 0..DEPTH {
        node = block_with_margins(1.0, 1.0, vec![node]);
    }
    // Every level contributes the same 1px margin, so the folded result is
    // just 1px — the assertion that matters is that these calls return at
    // all instead of overflowing the stack.
    let top = collapsed_top_margin(&node, 800.0, VIEWPORT);
    let bottom = collapsed_bottom_margin(&node, 800.0, VIEWPORT);
    assert!((top - 1.0).abs() < 0.01, "top={top}");
    assert!((bottom - 1.0).abs() < 0.01, "bottom={bottom}");
    // `LayoutBox`'s compiler-derived `Drop` glue walks `children` recursively
    // (one frame per nesting level, same shape as the pre-fix
    // `collapsed_top_margin`/`collapsed_bottom_margin`) — a stack-overflow
    // source of its own, unrelated to what this test checks, and LAYOUT-1's
    // remaining scope (not this slice). `forget` sidesteps it so the test
    // isolates the one thing it's here to prove.
    std::mem::forget(node);
}
