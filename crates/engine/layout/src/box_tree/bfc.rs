use super::*;

/// CSS 2.1 §9.4.1 — does this box establish a new Block Formatting Context?
///
/// A BFC root does NOT collapse its margins with its in-flow children
/// (CSS 2.1 §8.3.1). Within the block-layout arm a box is always `Block` or
/// `FlowRoot`; the remaining BFC triggers detectable from the box alone are a
/// non-`visible` overflow, a float, and out-of-flow positioning. (Being a flex
/// / grid item also establishes an independent FC, but that depends on the
/// parent and is signalled separately via `lay_out`'s `in_block_flow` flag.)
pub(crate) fn establishes_bfc(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::FlowRoot)
        || b.style.overflow_x != Overflow::Visible
        || b.style.overflow_y != Overflow::Visible
        || b.style.float_side != FloatSide::None
        || matches!(b.style.position, Position::Absolute | Position::Fixed)
}

/// True if the box has any in-flow child that produces content (i.e. a child
/// that is not a float, out-of-flow box, `::marker`, or zero-height `Skip`).
///
/// CSS 2.1 §9.5: a block-level box beside a float keeps full containing-block
/// width while only its *line boxes* are shortened. Lumen cannot yet shorten
/// line boxes inside a child block (floats are not propagated into nested
/// layout), so it approximates the narrowing by clipping the box itself. That
/// clip is only geometrically faithful when the box has no in-flow content to
/// reflow — this predicate gates the full-width path to such boxes (e.g. an
/// empty `<div>` background sitting in the gap between two floats).
pub(crate) fn has_in_flow_content(b: &LayoutBox) -> bool {
    b.children.iter().any(|c| {
        !matches!(c.kind, BoxKind::Skip | BoxKind::Marker { .. })
            && c.style.float_side == FloatSide::None
            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
    })
}

/// Returns the first in-flow `Block` child whose top margin collapses with the
/// owning box's top margin (CSS 2.1 §8.3.1). Out-of-flow children (floats,
/// absolutely positioned), `::marker`s and `Skip` boxes are transparent and
/// skipped. If the first remaining in-flow child is not a plain `Block` (e.g.
/// an inline run or a replaced element) the collapsing chain is broken and
/// `None` is returned. A child with clearance also breaks the chain.
fn first_collapsible_child(b: &LayoutBox) -> Option<&LayoutBox> {
    for child in &b.children {
        if matches!(child.kind, BoxKind::Marker { .. } | BoxKind::Skip) {
            continue;
        }
        if child.style.float_side != FloatSide::None
            || matches!(child.style.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if child.style.clear != ClearSide::None {
            return None;
        }
        return matches!(child.kind, BoxKind::Block).then_some(child);
    }
    None
}

/// Per-`run()`-pass memo for [`collapsed_top_margin`]/[`collapsed_bottom_margin`]
/// — see BUG-1026. Keyed by `(NodeId, BoxRole)`, not bare `NodeId`: one DOM
/// node can back several distinct `LayoutBox`es (an element's principal box
/// and an anonymous wrapper/pseudo-element box both carry the *same*
/// `LayoutBox::node`, ADR-025 §1), so `BoxRole` disambiguates them the same
/// way `LayoutInPlaceKey` does. The stored `f32` is the containing-block
/// width (`cb`) the entry was computed with — a lookup only reuses the cached
/// result when a fresh call passes that exact `cb` back, so a mismatch (e.g.
/// a non-zero margin/scrollbar-gutter making the real per-level
/// containing-block width diverge from this module's own padding/border-only
/// narrowing) falls back to a full recompute instead of returning a wrong
/// value.
pub(crate) type MarginCollapseCache = std::collections::HashMap<(NodeId, BoxRole), (f32, f32)>;

/// CSS 2.1 §8.3.1 — the *collapsed* top margin of a block-level box (px).
///
/// The top margin of an in-flow block collapses with the top margin of its
/// first in-flow block-level child when nothing separates them: the box has no
/// top border, no top padding, establishes no BFC, and the first in-flow child
/// is itself a plain block with no clearance. The collapse walks down the
/// chain of first children. `cb` is the containing-block width used to resolve
/// percentage margins. Only the common non-negative case is folded (parity with
/// sibling collapse); negative margins fall through as the box's own margin.
///
/// LAYOUT-1: this used to recurse one call frame per link in the first-child
/// chain — on a page whose markup is a straight run of single-child `<div>`s
/// (the exact shape BUG-987's fandom.com/OneTrust repro hit) that chain can be
/// the full DOM depth, so it was an independent stack-overflow source from
/// `lay_out_inner`'s own descent. Each step folds into the running max the
/// same way the old `own.max(collapsed_top_margin(child, ..))` did — the loop
/// computes the identical value in O(1) stack.
///
/// BUG-1026: LAYOUT-1's loop still did O(remaining-chain-length) *work* per
/// call, and `block_flow_trampoline.rs` calls this once per level of a
/// block-flow descent — O(N²) total on an N-deep single-child chain. `cache`
/// memoizes every node visited along a chain walk with the suffix-max value
/// computed from that node down, so the (very common) case where a deeper
/// call in the same chain asks for a `cb` this walk already resolved becomes
/// an O(1) lookup instead of a fresh walk — see `MarginCollapseCache`'s doc
/// comment for why the cache key includes `BoxRole` and why a `cb` mismatch
/// safely falls back to recomputing rather than trusting a stale entry.
pub(crate) fn collapsed_top_margin(
    b: &LayoutBox,
    cb: f32,
    viewport: Size,
    cache: &mut MarginCollapseCache,
) -> f32 {
    if let Some(&(cached_cb, cached_val)) = cache.get(&(b.node, b.origin.role))
        && cached_cb == cb
    {
        return cached_val;
    }

    // Walk down the first-child chain exactly as the pre-BUG-1026 loop did,
    // but collect each visited link instead of folding into a running max
    // immediately — the fold needs to run from the *end* of the walk backward
    // (a suffix max), so it can double as the per-node cache entry for
    // whichever direct call reaches that node next.
    let mut chain: Vec<((NodeId, BoxRole), f32, f32)> = Vec::new(); // (key, own_margin, cb_used)
    let mut node = b;
    let mut cur_cb = cb;
    let mut tail = f32::NEG_INFINITY;
    loop {
        let key = (node.node, node.origin.role);
        if !std::ptr::eq(node, b)
            && let Some(&(cached_cb, cached_val)) = cache.get(&key)
            && cached_cb == cur_cb
        {
            tail = cached_val;
            break;
        }
        let em = node.style.font_size;
        let own = node.style.margin_top.resolve_or_zero(em, cur_cb, viewport);
        chain.push((key, own, cur_cb));
        if !matches!(node.kind, BoxKind::Block) || establishes_bfc(node) {
            break;
        }
        let pt = node.style.padding_top.resolve_or_zero(em, cur_cb, viewport);
        if pt != 0.0 || node.style.border_top_width != 0.0 {
            break;
        }
        match first_collapsible_child(node) {
            Some(child) => {
                // Child's containing-block width = this box's content width.
                cur_cb = (cur_cb
                    - node.style.padding_left.resolve_or_zero(em, cur_cb, viewport)
                    - node.style.padding_right.resolve_or_zero(em, cur_cb, viewport)
                    - node.style.border_left_width
                    - node.style.border_right_width)
                    .max(0.0);
                node = child;
            }
            None => break,
        }
    }

    let mut running = tail;
    for (key, own, cb_used) in chain.into_iter().rev() {
        running = running.max(own);
        cache.insert(key, (cb_used, running));
    }
    running
}

/// Returns the last in-flow `Block` child whose bottom margin collapses with the
/// owning box's bottom margin (CSS 2.1 §8.3.1). Mirror of `first_collapsible_child`
/// for the bottom edge: out-of-flow children (floats, absolutely positioned),
/// `::marker`s and zero-height `Skip` boxes are transparent and skipped. If the
/// last remaining in-flow child is not a plain `Block` (e.g. an inline run or a
/// replaced element) the collapsing chain is broken and `None` is returned. A
/// child with clearance also breaks the chain.
pub(crate) fn last_collapsible_child(b: &LayoutBox) -> Option<&LayoutBox> {
    for child in b.children.iter().rev() {
        if matches!(child.kind, BoxKind::Marker { .. } | BoxKind::Skip) {
            continue;
        }
        if child.style.float_side != FloatSide::None
            || matches!(child.style.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if child.style.clear != ClearSide::None {
            return None;
        }
        return matches!(child.kind, BoxKind::Block).then_some(child);
    }
    None
}

/// CSS 2.1 §8.3.1 — the *collapsed* bottom margin of a block-level box (px).
///
/// The bottom margin of an in-flow block collapses with the bottom margin of its
/// last in-flow block-level child when nothing separates them: the box has an
/// `auto` height, no bottom border, no bottom padding, establishes no BFC, and the
/// last in-flow child is itself a plain block with no clearance. The collapse
/// walks down the chain of last children. `cb` is the containing-block width
/// used to resolve percentage margins. Only the common non-negative case is folded
/// (parity with `collapsed_top_margin`); negative margins fall through as the box's
/// own margin.
///
/// LAYOUT-1: mirrors `collapsed_top_margin`'s conversion from per-link
/// recursion to an O(1)-stack loop — see its doc comment for why the
/// last-child chain is just as much a BUG-987 stack-overflow source as the
/// first-child one.
///
/// BUG-1026: mirrors `collapsed_top_margin`'s `cache` memoization — see that
/// function's doc comment and `MarginCollapseCache`'s for the full rationale.
pub(crate) fn collapsed_bottom_margin(
    b: &LayoutBox,
    cb: f32,
    viewport: Size,
    cache: &mut MarginCollapseCache,
) -> f32 {
    if let Some(&(cached_cb, cached_val)) = cache.get(&(b.node, b.origin.role))
        && cached_cb == cb
    {
        return cached_val;
    }

    let mut chain: Vec<((NodeId, BoxRole), f32, f32)> = Vec::new(); // (key, own_margin, cb_used)
    let mut node = b;
    let mut cur_cb = cb;
    let mut tail = f32::NEG_INFINITY;
    loop {
        let key = (node.node, node.origin.role);
        if !std::ptr::eq(node, b)
            && let Some(&(cached_cb, cached_val)) = cache.get(&key)
            && cached_cb == cur_cb
        {
            tail = cached_val;
            break;
        }
        let em = node.style.font_size;
        let own = node.style.margin_bottom.resolve_or_zero(em, cur_cb, viewport);
        chain.push((key, own, cur_cb));
        if !matches!(node.kind, BoxKind::Block) || establishes_bfc(node) {
            break;
        }
        // A definite height blocks the last child's bottom margin from reaching
        // the box's bottom edge, so the through-collapse does not happen.
        if node.style.height.is_some() {
            break;
        }
        let pb = node.style.padding_bottom.resolve_or_zero(em, cur_cb, viewport);
        if pb != 0.0 || node.style.border_bottom_width != 0.0 {
            break;
        }
        match last_collapsible_child(node) {
            Some(child) => {
                // Child's containing-block width = this box's content width.
                cur_cb = (cur_cb
                    - node.style.padding_left.resolve_or_zero(em, cur_cb, viewport)
                    - node.style.padding_right.resolve_or_zero(em, cur_cb, viewport)
                    - node.style.border_left_width
                    - node.style.border_right_width)
                    .max(0.0);
                node = child;
            }
            None => break,
        }
    }

    let mut running = tail;
    for (key, own, cb_used) in chain.into_iter().rev() {
        running = running.max(own);
        cache.insert(key, (cb_used, running));
    }
    running
}

/// CSS Box Sizing L4 §5 — content block-size contribution under size containment.
/// When `size_contained` is true the box ignores its children for auto sizing and
/// uses the resolved `contain-intrinsic-height` (content-box px, clamped ≥ 0), or
/// `0.0` when the value is `none`/unset. Otherwise returns the measured
/// `content_height` unchanged.
pub(crate) fn contained_content_height(
    size_contained: bool,
    style: &ComputedStyle,
    em: f32,
    viewport: Size,
    content_height: f32,
) -> f32 {
    if size_contained {
        style
            .contain_intrinsic_height
            .as_ref()
            .and_then(|l| l.resolve(em, None, viewport))
            .map_or(0.0, |v| v.max(0.0))
    } else {
        content_height
    }
}
