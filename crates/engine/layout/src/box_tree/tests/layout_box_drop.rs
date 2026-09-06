//! LAYOUT-1 срез 2: `LayoutBox`'s compiler-derived `Drop` glue used to walk
//! `children: Vec<LayoutBox>` (and nested `SvgMaskContent::content`)
//! depth-first on the native call stack — one frame per nesting level, the
//! same overflow shape as `lay_out_inner`'s own descent (BUG-987) and the
//! pre-fix `collapsed_top_margin`/`collapsed_bottom_margin` from slice 1.
//! `bfc_margin_collapse.rs`'s deep-chain test had to `mem::forget` its tree
//! to dodge this exact issue. These tests build trees deep enough that the
//! old recursive glue would have overflowed a normal thread stack, and let
//! them drop for real — no `forget` needed once `Drop` is iterative.

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect, SvgMaskContent, SvgShapeKind, SvgTransform};
use crate::style::ComputedStyle;
use crate::style::MaskMode;

/// Plain `Block` wrapping the given children — same shape as
/// `bfc_margin_collapse.rs`'s `block_with_margins`, minus the margins.
fn block(children: Vec<LayoutBox>) -> LayoutBox {
    let style = ComputedStyle::root();
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

/// `SvgShape` optionally carrying a `<mask>` reference — the other field
/// through which a `LayoutBox` subtree nests (`svg_mask.content`, boxed
/// separately from `children` because `ComputedStyle` cannot depend on
/// `box_tree`, see `SvgMaskContent`'s doc comment).
fn svg_shape_with_mask(mask_content: Vec<LayoutBox>) -> LayoutBox {
    let style = ComputedStyle::root();
    let svg_mask = if mask_content.is_empty() {
        None
    } else {
        Some(Box::new(SvgMaskContent { content: mask_content, mode: MaskMode::Alpha }))
    };
    LayoutBox {
        node: lumen_dom::NodeId::from_index(0),
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        used_line_height: style.font_size * style.line_height,
        style: std::sync::Arc::new(style),
        kind: BoxKind::SvgShape {
            shape: SvgShapeKind::Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0, rx: 0.0, ry: 0.0 },
            svg_transform: SvgTransform::identity(),
            svg_paint_matrix: SvgTransform::identity(),
            svg_mask,
        },
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin::default(),
    }
}

/// The regression guard: a single-child chain deep enough that the old
/// per-level recursive `Drop` glue would have overflowed a normal
/// (non-bumped) thread stack long before reaching the bottom. Built
/// bottom-up in a loop so construction never recurses either, then dropped
/// for real at the end of the test (no `mem::forget`) — reaching the end of
/// the function without the test process crashing IS the assertion.
#[test]
fn deep_single_child_chain_drops_without_overflowing_the_stack() {
    const DEPTH: usize = 200_000;
    let mut node = block(vec![]);
    for _ in 0..DEPTH {
        node = block(vec![node]);
    }
    drop(node);
}

/// A wide-and-shallow tree exercises the other extreme of the same drain
/// loop (many siblings queued at once rather than one nested chain) —
/// makes sure the iterative drop isn't accidentally depth-only.
#[test]
fn wide_tree_drops_without_overflowing_the_stack() {
    const WIDTH: usize = 50_000;
    let children: Vec<LayoutBox> = (0..WIDTH).map(|_| block(vec![])).collect();
    let root = block(children);
    drop(root);
}

/// `SvgMaskContent::content` is the second (boxed, off `LayoutBox::kind`)
/// place a subtree nests — a chain built entirely through mask references
/// must drop iteratively too, not just the direct `children` field.
#[test]
fn deep_svg_mask_chain_drops_without_overflowing_the_stack() {
    const DEPTH: usize = 200_000;
    let mut node = svg_shape_with_mask(vec![]);
    for _ in 0..DEPTH {
        node = svg_shape_with_mask(vec![node]);
    }
    drop(node);
}
