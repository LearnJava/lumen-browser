//! LAYOUT-2 срез 3: `lay_out_flex`'s per-item final-placement pass (CSS
//! Flexbox L1 §9.5/§9.6 — the recursive `lay_out_with_used_size` call inside
//! the per-line item loop, both the column and row arms) used to recurse one
//! native call frame per flex-nesting level, same class of defect
//! `block_flow_trampoline.rs` fixed for plain block-flow. It is now driven by
//! an explicit heap stack (`flex_trampoline::run`) instead — this exercises
//! real flex layout on a chain deep enough that the old recursion would have
//! overflowed a normal thread stack long before reaching the bottom.
//!
//! The chain is deliberately built so that ONLY the final-placement
//! recursion this slice fixes is exercised, and not the two OTHER, separate,
//! out-of-scope recursions a naive flex chain would also hit:
//! - The Step 1 probe (`build_flex_init`, native recursion, out of scope —
//!   see its doc comment) triggers whenever `flex-basis` is `auto`/`content`
//!   in a column container, or in a row container with an explicit `width`.
//!   Every node here uses `flex-basis: <length>` instead, which the removed
//!   code's `needs_prelayout` never probes for (row direction) regardless of
//!   `width`.
//! - `flex_auto_base_main_width` → `max_content_outer_width` (intrinsic
//!   sizing, `intrinsic.rs`) recurses into a box's whole descendant subtree
//!   and is a pre-existing, unrelated source of native recursion depth — it
//!   is reached only when a row item's `flex-basis` is `auto`/`content`
//!   *and* it has no explicit `width`. `flex-basis: <length>` skips this
//!   computation entirely (`all_hyp`'s `FlexBasis::Length` arm resolves the
//!   style value directly).

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Display, FlexBasis, FlexDirection, Length};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };
// Same depth as LAYOUT-2's acceptance criterion (ROADMAP.md) and
// `block_flow_trampoline`'s sibling test. Unlike that test's margin-collapse
// variant, nothing on this chain's path is O(depth) per level (flex's
// grow/shrink loop here never runs — see `wrapper`'s doc comment — and
// cross-axis alignment/`shift_tree` are O(subtree-below), not O(chain-above)),
// so there is no quadratic blow-up to work around.
const DEPTH: usize = 20_000;
const LEAF_HEIGHT: f32 = 10.0;
const ITEM_BASIS: f32 = 50.0;

/// `display: flex; flex-direction: row`, one child, `flex-basis: 50px` on
/// every node (including the leaf, since the flex-basis that matters at each
/// level is the ITEM's own, read from its own style by its parent's layout —
/// not the container's). `flex-basis: <length>` keeps `needs_prelayout`
/// false (row direction — see `build_flex_init`) and keeps `all_hyp`'s
/// `FlexBasis::Length` arm from ever calling `flex_auto_base_main_width`, so
/// neither of the two out-of-scope recursions this module's doc comment
/// describes is reachable. Default `flex-grow: 0`/`flex-shrink` initial
/// values keep `free_space > 0.0`'s `total_grow > 0.0` guard false at every
/// level (single 50px-basis item against an 800px-then-50px container), so
/// the grow/shrink loops in `build_line_inits` never run either — every
/// level's precompute is O(1), not just its placement.
fn wrapper(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.display = Display::Flex;
    style.flex_direction = FlexDirection::Row;
    style.flex_basis = FlexBasis::Length(Length::Px(ITEM_BASIS));
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

/// Plain (non-flex) block with an explicit height — the innermost flex
/// container's one item. `flex-basis: 50px` so its immediate parent's
/// `all_hyp` computation takes the same O(1) `FlexBasis::Length` path as
/// every wrapper. Explicit `height` means the row cross-align "stretch" arm
/// (`finish_line`) is a no-op on it (`is.height.is_some()` → `stretch_h =
/// item_rect_height`, unchanged) — see `deep_flex_row_chain_lays_out_without_overflowing_the_stack`'s
/// doc comment for why that matters for the height assertion.
fn leaf() -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.height = Some(Length::Px(LEAF_HEIGHT));
    style.flex_basis = FlexBasis::Length(Length::Px(ITEM_BASIS));
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

/// `depth` single-child flex `wrapper`s around one `leaf`, root first. Node
/// indices count down from `depth + 1` to `1`, mirroring
/// `block_flow_trampoline::deep_chain` (never reusing index 0, though no flex
/// code path keys off it the way `step_child`'s root-element gate does —
/// kept for consistency with the sibling test file).
fn deep_chain(depth: usize) -> LayoutBox {
    let mut node = leaf();
    for i in 1..=depth {
        node = wrapper(i + 1, vec![node]);
    }
    node
}

/// LAYOUT-2 acceptance criterion for item (2) of its ROADMAP entry, applied
/// to the final-placement pass: laying out a `DEPTH`-deep single-item
/// `flex-direction: row` chain must return instead of overflowing the stack,
/// and — since every level is a zero-margin, `height: auto` flex container
/// wrapping one item — the root's final border-box height must still come
/// out to exactly `LEAF_HEIGHT` (row's cross axis is height; a single-item,
/// non-wrapping line's content height equals that item's own height, folded
/// up through every level's `finish_frame`/`finish_line`), proving the
/// explicit-stack driver reproduces the recursive algorithm's result, not
/// just its termination.
#[test]
fn deep_flex_row_chain_lays_out_without_overflowing_the_stack() {
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

/// Same chain, but with `flex-direction: column` on every wrapper and node.
/// Column direction always probes Step 1 natively for `flex-basis: auto`/
/// `content` (out of scope — see `build_flex_init`), but `flex-basis:
/// <length>` skips that too (its `needs_prelayout` arm is
/// `is.min_height.is_none() && is.overflow_y == Visible` — `false` once
/// `min-height` is set), so this exercises the SAME final-placement
/// recursion as the row test above, through the column arm of
/// `flex_trampoline::step_item` instead of the row arm (`shift_tree`'s
/// replay path in particular — see `column_probe`/`probed_main` staying
/// empty throughout, since Step 1 never runs).
///
/// No wrapper sets an explicit `height`, so `explicit_main` (`main_definite`
/// in `build_flex_init`) is `None` at every level — `free_space` is
/// therefore always exactly `0.0` (neither > nor < 0), so `hyp_main` stays
/// at the `flex-basis: 50px` value unconditionally, at every level,
/// including the first (unlike the row test, which needs two levels to
/// settle from the 800px viewport width down to the 50px basis). Each
/// wrapper's own reported height (`finish_frame`, `s.height.is_none()`
/// branch) equals its one item's resolved height, i.e. 50px too — so the
/// root's final height is a direct readout of correct main-axis propagation
/// through the whole chain, the column-arm counterpart of the row test's
/// `LEAF_HEIGHT` check. (The leaf's own `style.height` is irrelevant here:
/// every item's used main size — including the leaf's — is overridden by
/// its parent's `UsedSizeOverride` per CSS Flexbox §9.2, same as `height`
/// never surviving on any row item above `flex-basis`.)
#[test]
fn deep_flex_column_chain_lays_out_without_overflowing_the_stack() {
    fn column_node(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
        let mut style = ComputedStyle::root();
        style.display = Display::Flex;
        style.flex_direction = FlexDirection::Column;
        style.flex_basis = FlexBasis::Length(Length::Px(ITEM_BASIS));
        style.min_height = Some(Length::Px(0.0));
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
    // The innermost item: a leaf `display: flex` box (no children) works
    // the same as a plain block here — its own `lay_out_flex` call just
    // sees zero items and returns immediately (`build_flex_init`'s
    // `item_idxs.is_empty()` branch) — so reuse `column_node` all the way
    // down instead of a separate non-flex leaf helper.
    let mut node = column_node(1, Vec::new());
    for i in 1..=DEPTH {
        node = column_node(i + 1, vec![node]);
    }
    let mut root = node;
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, false,
    );
    assert_eq!(root.rect.height, ITEM_BASIS, "height={}", root.rect.height);
    assert_eq!(root.rect.x, 0.0);
    assert_eq!(root.rect.y, 0.0);
}
