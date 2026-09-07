//! LAYOUT-2 срез 6: `lay_out_table`/`lay_out_table_row`'s per-row/per-cell
//! placement (CSS 2.1 §17.5/§17.6.2) — the recursive `lay_out`/`dispatch_box`
//! call inside the per-cell loop — used to recurse one native call frame per
//! table-nesting level (a `<td>` containing another `<table>`), same class of
//! defect the block-flow/flex/grid trampolines fixed. It is now driven by an
//! explicit heap stack (`table_trampoline::run`) instead.
//!
//! Unlike a flex/grid item — which CAN be the very same box as the container
//! it nests into, so `flex_trampoline`/`grid_trampoline` push a same-kind
//! chain onto their OWN `stack` with zero added native frames per level — a
//! `<table>` can never itself be a `<td>`; a nested table is always reached
//! through its cell's own block-flow context (CSS 2.1 §17.2 wraps table
//! content in an anonymous block-level box). So `table_trampoline::run` calls
//! `block_flow_trampoline::run` for the cell (one native frame), which calls
//! `table_trampoline::run` again for the nested table (another native frame)
//! — this is the exact "one native frame per kind transition" cost
//! `block_flow_trampoline::step_child`'s own `NeedsFlexLoop` arm doc comment
//! already documents as accepted for a flex/block/flex chain, not something
//! this (or any) LAYOUT-2 slice eliminates. `DEPTH` is chosen for that native-
//! frame-pair-per-level ceiling, not the `20_000` bar `block_flow_trampoline`/
//! `flex_trampoline`/`grid_trampoline` reach for a *same-kind* chain —
//! empirically: 500 passes, 1 000 and 5 000 overflow a `cargo test` worker
//! thread's stack (same order of magnitude as LAYOUT-1's ROADMAP entry
//! measured for wholly-unconverted recursive functions, ~150–800 — the two
//! native frames this slice leaves per table-in-table level land in the same
//! range as one unconverted call used to).

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Display};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };
const LEAF_HEIGHT: f32 = 10.0;
/// See the file doc comment — bounded by one native frame pair
/// (`table_trampoline::run` ↔ `block_flow_trampoline::run`) per level, not by
/// this slice's own explicit-stack machinery.
const DEPTH: usize = 500;

/// Plain (non-table) block with an explicit height — the innermost cell's one
/// child.
fn leaf() -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.height = Some(crate::style::Length::Px(LEAF_HEIGHT));
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

fn plain_box(node_index: usize, kind: BoxKind, display: Display, children: Vec<LayoutBox>) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.display = display;
    LayoutBox {
        node: lumen_dom::NodeId::from_index(node_index),
        rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        used_line_height: style.font_size * style.line_height,
        style: std::sync::Arc::new(style),
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

/// One `<table><tr><td>{child}</td></tr></table>` level — three nodes
/// (`base_index`, `base_index+1`, `base_index+2`), single column/row/cell so
/// every level's own width/height resolution is O(1) regardless of depth.
fn wrapper_level(base_index: usize, child: LayoutBox) -> LayoutBox {
    let cell = plain_box(base_index, BoxKind::Block, Display::TableCell, vec![child]);
    let row = plain_box(base_index + 1, BoxKind::TableRow, Display::TableRow, vec![cell]);
    plain_box(base_index + 2, BoxKind::Table, Display::Table, vec![row])
}

/// `depth` single-cell table wrappers around one `leaf`, root first. Node
/// indices count up from `1`, mirroring `grid_trampoline`/`flex_trampoline`'s
/// sibling deep-chain builders (three consumed per level here, one there).
fn deep_chain(depth: usize) -> LayoutBox {
    let mut node = leaf();
    let mut idx = 2usize;
    for _ in 0..depth {
        node = wrapper_level(idx, node);
        idx += 3;
    }
    node
}

/// Applied to item (4) of LAYOUT-2's ROADMAP entry: laying out a `DEPTH`-deep
/// single-cell table chain must return instead of overflowing the stack, and
/// — since every level is a zero-margin/padding/border auto-height table
/// wrapping one row/one cell — the root's final border-box height must still
/// come out to exactly `LEAF_HEIGHT` (each level's row/table height equals
/// its one cell's content height, folded up through every level's
/// `finish_row`/`finish_table`), proving the explicit-stack driver reproduces
/// the recursive algorithm's result, not just its termination.
#[test]
fn deep_table_chain_lays_out_without_overflowing_the_stack() {
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

/// Same shape, but each level's row lives inside a `<tbody>` (`TableRowGroup`)
/// instead of being a direct `<table>` child — exercises `begin_group`/
/// `finish_entry`'s group-boundary bookkeeping on every level of the chain,
/// not just the flat `TopEntry::Row` path the test above covers.
#[test]
fn deep_table_chain_through_row_groups_lays_out_without_overflowing_the_stack() {
    fn grouped_wrapper_level(base_index: usize, child: LayoutBox) -> LayoutBox {
        let cell = plain_box(base_index, BoxKind::Block, Display::TableCell, vec![child]);
        let row = plain_box(base_index + 1, BoxKind::TableRow, Display::TableRow, vec![cell]);
        let group = plain_box(base_index + 2, BoxKind::TableRowGroup, Display::TableRowGroup, vec![row]);
        plain_box(base_index + 3, BoxKind::Table, Display::Table, vec![group])
    }
    // Same native-frame-pair-per-level ceiling as `DEPTH` (see the file doc
    // comment) — kept comfortably below it since a grouped row's extra
    // `TopEntry::Group` bookkeeping (`begin_group`/`finish_entry`) adds a
    // little more per-level stack depth inside the (already dominant)
    // `table_trampoline::run`/`block_flow_trampoline::run` frame pair.
    const GROUP_DEPTH: usize = 400;
    let mut node = leaf();
    let mut idx = 2usize;
    for _ in 0..GROUP_DEPTH {
        node = grouped_wrapper_level(idx, node);
        idx += 4;
    }
    let mut root = node;
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, false,
    );
    assert_eq!(root.rect.height, LEAF_HEIGHT, "height={}", root.rect.height);
}
