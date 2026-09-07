//! LAYOUT-2 срез 1: `lay_out_inner`'s plain block-flow branch (CSS 2.1
//! §9.5/§8.3.1 — float placement, margin collapsing, the recursive descent
//! into each normal-flow child) used to recurse one native call frame per
//! nesting level, the last of LAYOUT-1's four still-open overflow sources on
//! a `<div>`×N chain (`box_tree::tests::deep_traversal_stress`'s module doc
//! calls it out by name — "a separate, still-open source of the same
//! overflow"). It is now driven by an explicit heap stack
//! (`block_flow_trampoline::run`) instead, so this — unlike the traversal-only
//! helpers those other tests exercise — calls `lay_out` itself, all the way
//! through real block-flow layout, on a chain deep enough that the old
//! recursion would have overflowed a normal thread stack long before
//! reaching the bottom.

use lumen_core::geom::Size;
use crate::style::{ComputedStyle, Length};

use super::super::{BoxKind, BoxOrigin, LayoutBox, Rect};

const VIEWPORT: Size = Size { width: 800.0, height: 600.0 };
// LAYOUT-2's acceptance criterion (ROADMAP.md) is `<div>×20000`, not the
// 200_000 the sibling LAYOUT-1 traversal-only tests use (drop glissade,
// serialize, collect_* — all O(1) work per node). This chain instead drives
// real block-flow layout, and `collapsed_top_margin`/`collapsed_bottom_margin`
// (`bfc.rs`) are only O(1) *stack* since LAYOUT-1 срез 1 — each call still
// walks its whole remaining first/last-child chain, so calling either once
// per level (as `step_child`/`post_child_bookkeeping`/`finish_frame` do) costs
// O(depth) per level, O(depth²) total. At 200_000 that is ~2*10^10 loop
// iterations — confirmed to not finish inside an hour; the pre-LAYOUT-1/2
// native recursion never reached a depth close to this, so the quadratic cost
// was never observable before. Not fixed here — filed as a follow-up (see
// ROADMAP). 20_000 keeps the documented acceptance depth while finishing in
// low single-digit seconds (~4*10^8 total iterations across the O(depth)
// helpers).
const DEPTH: usize = 20_000;
const LEAF_HEIGHT: f32 = 10.0;

/// Plain `Block`, zero margin/padding/border, `width`/`height: auto` — wraps
/// its single child to exactly that child's border-box height, so a chain of
/// these contributes nothing but recursion depth to the final geometry.
/// `node_index` — see `deep_chain`'s doc comment for why this must be
/// distinct per level and never 0 except at the true (outermost) root.
fn wrapper(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
    let style = ComputedStyle::root();
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

/// The one box in the chain with a definite size — every ancestor auto-sizes
/// to wrap exactly this, so the root's final height is a direct readout of
/// whether the whole chain laid out correctly (not just "didn't crash").
/// Always index 1 (never 0 — see `deep_chain`'s doc comment).
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

/// `depth` single-child `wrapper`s around one `leaf`, root first. Node indices
/// count down from `depth + 1` at the root to `1` at the leaf (never 0):
/// `step_child`'s `child_is_root_element` gate (CSS 2.1 §8.3.1 — the true
/// document root's margin never collapses into its first child) keys off
/// `b.node.index() == 0`, and every box here plays "parent of the next level"
/// in turn — reusing index 0 across levels would spuriously trip that gate at
/// every level instead of only the outermost, silently disabling the very
/// margin-collapse chain `deep_chain_with_margins_collapses_through_the_trampoline`
/// means to exercise.
fn deep_chain(depth: usize) -> LayoutBox {
    let mut node = leaf();
    for i in 1..=depth {
        node = wrapper(i + 1, vec![node]);
    }
    node
}

/// LAYOUT-2 acceptance criterion for this item (ROADMAP: "`--dump-layout`/
/// `--screenshot`/live `--print-to-pdf` on `<div>`×20000 … without increased
/// stacks"): laying out a `DEPTH`-deep single-child block-flow chain must
/// return instead of overflowing the stack, and — since every level is a
/// zero-margin auto-size wrapper around one 10px-tall leaf — the root's
/// final border-box height must still come out to exactly `LEAF_HEIGHT`,
/// proving the explicit-stack driver reproduces the recursive algorithm's
/// result, not just its termination.
#[test]
fn deep_single_child_chain_lays_out_without_overflowing_the_stack() {
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

/// Same chain, but each wrapper also carries a 1px top margin. None of these
/// boxes is the true document root (`deep_chain` keeps every index ≥ 1), so
/// CSS 2.1 §8.3.1 parent↔first-child collapsing applies uniformly at every
/// level: the whole chain's margins fold into ONE 1px gap rather than
/// stacking `DEPTH`×1px, and — because collapsing only ever escapes
/// *upward*, never inward — that one gap surfaces as the outermost box's own
/// position (`lay_out`'s unconditional `rect.y = start_y + margin_top` at the
/// entry point), while every descendant lands flush against its parent's
/// content top and the total height is unaffected. Proves the trampoline's
/// margin bookkeeping (`step_child`'s `collapsed_mt`/`post_child_bookkeeping`)
/// folds correctly across a chain far deeper than the removed recursive
/// version could ever reach in a test, not just on the shallow fixtures
/// `bfc_margin_collapse.rs` already covers.
///
/// Passes `in_block_flow: true` to the top-level [`lay_out`] call — `false`
/// (as the sibling stack-depth test above uses) is documented
/// (`layout_dispatch.rs`) to mean "this box establishes an independent
/// formatting context / is the document root", which is exactly the BUG-153
/// exception this test's own premise excludes: it disables `root`'s
/// `b_collapses_top` for its own first child, stacking one extra 1px
/// (root's own margin *and* the child's, instead of folding them) and
/// inflating the measured height by 1px regardless of `DEPTH`. The sibling
/// test never notices because its margins are all zero.
#[test]
fn deep_chain_with_margins_collapses_through_the_trampoline() {
    fn margined_wrapper(node_index: usize, children: Vec<LayoutBox>) -> LayoutBox {
        let mut style = ComputedStyle::root();
        style.margin_top = crate::style::LengthOrAuto::Length(Length::Px(1.0));
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
    for i in 1..=DEPTH {
        node = margined_wrapper(i + 1, vec![node]);
    }
    let mut root = node;
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height);
    super::super::lay_out(
        &mut root, 0.0, 0.0, VIEWPORT.width, Some(VIEWPORT.height), None, VIEWPORT, init_pcb,
        &null_hp, true,
    );
    assert_eq!(root.rect.y, 1.0, "y={}", root.rect.y);
    assert_eq!(root.rect.height, LEAF_HEIGHT, "height={}", root.rect.height);
}
