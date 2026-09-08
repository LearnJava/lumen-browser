//! LAYOUT-2 срез 10 (final item): `fill_buckets`'s per-child recursion
//! (`box_layer.rs`, the ordered/anim-aware paint path) — the last remaining
//! item of LAYOUT-2's object list after `walk`'s own conversion (срез 9,
//! `walk_trampoline.rs`) and all six layout dispatchers (срезы 1–8). Deep-
//! chain stack-safety regression test at the same `<div>`×20000 acceptance
//! number every other LAYOUT-2 срез used.
//!
//! Cross-function recursion still reaching `walk` natively (`emit_table_box`
//! calling back into `walk` for row-group/cell content, `walk_with_anim`'s
//! own independent copy of this traversal, `emit_svg_shape_masked`'s
//! mask-content walk) is a separate, self-contained class documented in
//! `walk.rs`'s own doc comment (срез 9) — `fill_buckets` has no such
//! mutual recursion of its own (its only recursive call site is itself).

use super::*;
use lumen_dom::NodeId;
use std::sync::Arc;

const DEPTH: usize = 20_000;
const LEAF_SIZE: f32 = 10.0;
const BG: Color = Color { r: 10, g: 20, b: 30, a: 255 };

/// `depth`-deep single-child `BoxKind::Block` chain (`depth + 1` boxes total,
/// leaf included), every box carrying an opaque background so `fill_buckets`
/// emits exactly one `FillRect` per box — a stack-overflow-only fix could
/// still pass a bare "did it return" check without this, if it happened to
/// also (wrongly) stop short after the first level. Plain
/// `ComputedStyle::root()` throughout: no `position`/`opacity`/`transform`/
/// `z-index` trigger, so no box here ever creates its own stacking context —
/// the whole chain is one flat SC, exercising exactly the self-recursion
/// this срез converted (not `fill_buckets`'s per-child SC bookkeeping,
/// unrelated to stack depth).
fn deep_chain(depth: usize) -> LayoutBox {
    let mut style = ComputedStyle::root();
    style.background_color = Some(CssColor::Rgba(BG));
    let style = Arc::new(style);
    let leaf_line_height = style.font_size * style.line_height;
    let mut node = LayoutBox {
        node: NodeId::from_index(1),
        rect: Rect::new(0.0, 0.0, LEAF_SIZE, LEAF_SIZE),
        used_line_height: leaf_line_height,
        style: style.clone(),
        kind: BoxKind::Block,
        children: Vec::new(),
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin::default(),
    };
    for i in 1..=depth {
        node = LayoutBox {
            node: NodeId::from_index(i + 1),
            rect: Rect::new(0.0, 0.0, LEAF_SIZE, LEAF_SIZE),
            used_line_height: leaf_line_height,
            style: style.clone(),
            kind: BoxKind::Block,
            children: vec![node],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
            origin: BoxOrigin::default(),
        };
    }
    node
}

fn count_fill_rects(cmds: &[DisplayCommand]) -> usize {
    cmds.iter().filter(|c| matches!(c, DisplayCommand::FillRect { .. })).count()
}

/// LAYOUT-2 acceptance criterion for `fill_buckets`, called directly
/// (bypassing `StackingTree`/`PaintOrder`, whose own recursion is separate
/// machinery this срез doesn't touch) with a single bucket: no box in this
/// fixture triggers `creates_stacking_context`, so the whole chain after the
/// root stays non-SC and lands in that one bucket's `root_bg` (the root
/// itself) plus `contents` (every descendant) — `next_sc_id` must therefore
/// never advance past its seed value, and the display list must show one
/// `FillRect` per level (proving every level painted, not just "didn't
/// crash").
#[test]
fn deep_chain_fill_buckets_does_not_overflow_the_stack() {
    let root = deep_chain(DEPTH);
    let mut buckets = vec![ScBucket::default()];
    let mut next_sc_id: u32 = 1;
    let mut split = SplitTracker {
        enabled: false,
        animated_scs: Vec::new(),
        content_spans: Vec::new(),
        invalid: false,
        sc_entries: 0,
    };
    let mut raw_spans: Vec<RawSpan> = Vec::new();
    fill_buckets(
        &root, StackingContextId::ROOT, &mut next_sc_id, &mut buckets, true, None, 1.0, &[],
        &mut split, &mut raw_spans,
    );
    let total = count_fill_rects(&buckets[0].root_bg) + count_fill_rects(&buckets[0].contents);
    assert_eq!(total, DEPTH + 1);
    assert_eq!(next_sc_id, 1, "plain boxes must not allocate any child stacking context");
}
