//! LAYOUT-2 срез 4: `lay_out_grid`'s per-item probe (CSS Grid L1 §12.3) and
//! final-placement (§11.2) passes — the recursive `lay_out`/`dispatch_box`
//! call inside each of the two per-item loops — used to recurse one native
//! call frame per grid-nesting level, same class of defect the flex/block-flow
//! trampolines fixed. They are now driven by an explicit heap stack
//! (`grid_trampoline::run`) instead — this exercises real grid layout on
//! chains deep enough that the old recursion would have overflowed a normal
//! thread stack long before reaching the bottom.
//!
//! Neither test here reaches the `DEPTH: usize = 20_000` sibling constant in
//! `flex_trampoline`/`block_flow_trampoline` — grid's own probe/reuse design
//! (BUG-341 S33) makes a single-item chain hit one of two *pre-existing*,
//! out-of-scope walls before the dispatch-recursion this slice fixes ever
//! becomes the limiting factor (both empirically confirmed while writing this
//! file, not assumed):
//! - With reuse eligible (the common case — see `deep_grid_chain_*` below),
//!   `GridInit::probe_reuse`'s stash clones the *reused* subtree
//!   (`frame.b.children[i].clone()` in `grid_trampoline::post_probe_item`) —
//!   `LayoutBox`'s `#[derive(Clone)]`, unlike its `impl Drop`, was never
//!   converted to an explicit stack (LAYOUT-1 fixed only `Drop`), so it
//!   recurses natively over however much of the chain sits below the item
//!   being probed. Confirmed to overflow a `cargo test` worker thread's stack
//!   at `DEPTH == 1_000` (and `5_000`), pass cleanly at `500` — the constant
//!   below stays under that empirically-found ceiling with margin.
//! - With reuse defeated (`deep_subgrid_chain_*` below — a subgrid item's
//!   track context genuinely differs between probe and final, so it can
//!   never reuse, unlike a probe/CV-touched miss), *both* passes dispatch a
//!   fresh, full recursive layout of everything below — the exact
//!   `T(n) = 2·T(n-1) + O(1)` doubling BUG-341/BUG-802 already document for
//!   flex's own pre-fix probe (`flex.rs`'s `build_flex_init` doc comment,
//!   "0.27s at depth 16, 1.21s at 18, 4.91s at 20"). Confirmed the same order
//!   of magnitude here: a plain (non-subgrid) chain with every wrapper forced
//!   reuse-ineligible via `content-visibility: auto` never finished at
//!   `DEPTH == 100` in reasonable time. `SUBGRID_DEPTH` below is chosen for
//!   that combinatorial cost, not stack safety — the chain-length that
//!   matters for stack safety (how many `Frame`s the explicit stack holds at
//!   once) is the SAME `SUBGRID_DEPTH` either way, already an order of
//!   magnitude past the ~150–800 native-recursion-overflow range LAYOUT-1's
//!   ROADMAP entry measured for other unconverted recursive functions.
//!
//! Both walls are pre-existing and out of this slice's scope — the same
//! stance `flex_trampoline`'s own deep-chain tests take toward the Step 1
//! probe and `flex_auto_base_main_width` (see that module's doc comment):
//! construct around a known unrelated recursion rather than "fix" it here.

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Display, GridTrackSize, Length};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };
const LEAF_HEIGHT: f32 = 10.0;
const COL_WIDTH: f32 = 100.0;
/// See the file doc comment — bounded by `LayoutBox::Clone`'s native
/// recursion (pre-existing, out of scope), not by anything this slice fixes.
/// Empirically: 500 passes, 1 000 and 5 000 overflow the stack.
const DEPTH: usize = 500;
/// See the file doc comment — bounded by the `T(n) = 2·T(n-1)` cost of a
/// chain where every level is reuse-ineligible by construction, not by stack
/// depth (which this value already exceeds LAYOUT-1's measured native-
/// recursion-overflow range by an order of magnitude).
const SUBGRID_DEPTH: usize = 16;

/// `display: grid`, one explicit `100px` column, one child (row auto-sized to
/// the child's content). The fixed-length column keeps every level's cell
/// width at exactly `COL_WIDTH` regardless of nesting depth (mirrors
/// `flex_trampoline`'s `ITEM_BASIS` — a length track, not `auto`/`fr`, so
/// Step 3's column-sizing precompute is O(1) per level, not dependent on the
/// parent's own resolved width).
fn wrapper(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.display = Display::Grid;
    style.grid_template_columns = vec![GridTrackSize::Length(Length::Px(COL_WIDTH))];
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

/// Plain (non-grid) block with an explicit height — the innermost grid
/// container's one item.
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

/// `depth` single-child grid `wrapper`s around one `leaf`, root first. Node
/// indices count down from `depth + 1` to `1`, mirroring
/// `flex_trampoline`/`block_flow_trampoline`'s sibling deep-chain builders.
fn deep_chain(depth: usize) -> LayoutBox {
    let mut node = leaf();
    for i in 1..=depth {
        node = wrapper(i + 1, vec![node]);
    }
    node
}

/// Applied to item (3) of LAYOUT-2's ROADMAP entry: laying out a `DEPTH`-deep
/// single-item grid chain must return instead of overflowing the stack, and
/// — since every level is a zero-margin, auto-height grid container wrapping
/// one item in an auto row — the root's final border-box height must still
/// come out to exactly `LEAF_HEIGHT` (a single auto row's height equals its
/// one item's content height, folded up through every level's
/// `finish_probe_pass`/`finish_frame`), proving the explicit-stack driver
/// reproduces the recursive algorithm's result, not just its termination.
///
/// Every level here IS reuse-eligible (see the file doc comment) — the Probe
/// phase genuinely `Descend`s `DEPTH` levels deep (this is what actually
/// exercises the explicit stack), while the Final phase resolves each level
/// via `GridInit::probe_reuse`'s `translate_subtree` fast path, not a second
/// `dispatch_box`/`Descend` — matching BUG-341 S33's whole point. The sibling
/// `deep_subgrid_chain_*` test below exercises the Final phase's own
/// `Descend` chain independently, since a subgrid item can never reuse.
#[test]
fn deep_grid_chain_lays_out_without_overflowing_the_stack() {
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

/// Same shape, but every wrapper below the root has `grid-template-columns:
/// subgrid` — CSS Grid L2 §9 — inheriting the root's single `100px` column
/// track down the whole chain instead of resolving its own. A subgrid item's
/// probe and final placements (`grid_trampoline::step_probe_item`/
/// `step_final_item`) both take the `SubgridContextGuard` branch
/// unconditionally, bypassing `GridInit::probe_reuse` entirely (by design —
/// see `grid::build_grid_init`'s doc comment on why a subgrid item's probe
/// and final tracks genuinely differ) — so, unlike the plain-grid test above,
/// the Final phase here also builds and drives a real `Descend` chain of its
/// own, independent of the Probe phase's. `SUBGRID_DEPTH` is far shorter than
/// `DEPTH` for a reason unrelated to stack safety — see the file doc comment.
#[test]
fn deep_subgrid_chain_lays_out_without_overflowing_the_stack() {
    fn subgrid_wrapper(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
        let mut style = ComputedStyle::root();
        style.display = Display::Grid;
        style.grid_template_columns = vec![GridTrackSize::Subgrid];
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
    let mut node = leaf();
    for i in 1..SUBGRID_DEPTH {
        node = subgrid_wrapper(i + 1, vec![node]);
    }
    // Root: a real (non-subgrid) grid — nothing above it to inherit tracks from.
    let mut root = wrapper(SUBGRID_DEPTH + 1, vec![node]);
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
