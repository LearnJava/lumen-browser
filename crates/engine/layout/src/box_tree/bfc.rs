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
        // CSS Overflow L3 §3.1: `clip`, unlike `hidden`, does not establish a formatting context.
        || !matches!(b.style.overflow_x, Overflow::Visible | Overflow::Clip)
        || !matches!(b.style.overflow_y, Overflow::Visible | Overflow::Clip)
        || b.style.float_side != FloatSide::None
        || matches!(b.style.position, Position::Absolute | Position::Fixed)
        // CSS Multicol L1 §2: a multicol container establishes a new block formatting context, so
        // its first child's margin-top stays inside it.
        || b.style.column_count.is_some()
        || b.style.column_width.is_some()
        // CSS Box Alignment L3 §«Block Containers»: a non-`normal` `align-content` makes the block
        // container an independent formatting context.
        || super::block_align_content::applies(&b.style)
        // CSS Containment L1 §3.2 / §3.3: `contain: layout` and `contain: paint` make the box an
        // independent formatting context, so a child's margin never collapses through it.
        || b.style.contain.0 & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0) != 0
        // Flexbox §3 / Grid §3: a flex/grid container is an independent
        // formatting context, so its items' margins never reach its ancestors.
        || matches!(
            b.style.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        )
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
    // A vertical box stacks its children along x: its top margin has no
    // block-axis neighbour inside it to collapse with.
    if !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb) {
        return None;
    }
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
///
/// BUG-1365: the entry holds the chain's largest non-negative margin and its most
/// negative one *separately* — `(cb, pos, neg)` with `pos >= 0 >= neg` — because
/// §8.3.1 sums the two, and a single running scalar cannot be extended by an
/// outer link once the sign information is gone.
pub(crate) type MarginCollapseCache = std::collections::HashMap<(NodeId, BoxRole), (f32, f32, f32)>;

/// Folds one more margin `own` (the outer link) into a chain's `(pos, neg)` pair.
fn fold_margin((pos, neg): (f32, f32), own: f32) -> (f32, f32) {
    (pos.max(own), neg.min(own))
}

/// The content-box width `node` gets inside a containing block `cb` wide — the
/// containing-block width its in-flow children resolve percentages against
/// (BUG-1431). Mirrors the width part of `lay_out`: a definite `width` (px / %
/// / `calc`, in the box's `box-sizing`) wins over the `auto` fill of the
/// containing block minus margins, then `max-width` and `min-width` clamp it
/// (`min` last). Intrinsic keywords (`min-content`, `fit-content`, …) cannot be
/// resolved without measuring, so they fall back to the `auto` fill.
fn content_box_width(node: &LayoutBox, cb: f32, viewport: Size) -> f32 {
    let s = &node.style;
    let em = s.font_size;
    let frame = s.padding_left.resolve_or_zero(em, cb, viewport)
        + s.padding_right.resolve_or_zero(em, cb, viewport)
        + s.border_left_width
        + s.border_right_width;
    // Border-box size -> content-box size.
    let content = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v,
        BoxSizing::BorderBox => v - frame,
    };
    let resolve = |l: &Option<Length>| {
        l.as_ref()
            .filter(|l| !l.is_intrinsic() && !matches!(l, Length::Stretch))
            .and_then(|l| l.resolve(em, Some(cb), viewport))
            .map(|v| content(v.max(0.0)))
    };
    let mut w = resolve(&s.width).unwrap_or_else(|| {
        let margins = s.margin_left.resolve_or_zero(em, cb, viewport)
            + s.margin_right.resolve_or_zero(em, cb, viewport);
        cb - margins - frame
    });
    if let Some(max) = resolve(&s.max_width) {
        w = w.min(max);
    }
    if let Some(min) = resolve(&s.min_width) {
        w = w.max(min);
    }
    w.max(0.0)
}

/// CSS 2.1 §8.3.1 — the *collapsed* top margin of a block-level box (px).
///
/// The top margin of an in-flow block collapses with the top margin of its
/// first in-flow block-level child when nothing separates them: the box has no
/// top border, no top padding, establishes no BFC, and the first in-flow child
/// is itself a plain block with no clearance. The collapse walks down the
/// chain of first children. `cb` is the containing-block width used to resolve
/// percentage margins. BUG-1365: the result is the largest non-negative margin of
/// the chain plus its most negative one (§8.3.1), so a negative margin on a box
/// with block children is no longer swallowed by a `0` child margin.
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
    if let Some(&(cached_cb, pos, neg)) = cache.get(&(b.node, b.origin.role))
        && cached_cb == cb
    {
        return pos + neg;
    }

    // Walk down the first-child chain exactly as the pre-BUG-1026 loop did,
    // but collect each visited link instead of folding into a running max
    // immediately — the fold needs to run from the *end* of the walk backward
    // (a suffix max), so it can double as the per-node cache entry for
    // whichever direct call reaches that node next.
    let mut chain: Vec<((NodeId, BoxRole), f32, f32)> = Vec::new(); // (key, own_margin, cb_used)
    let mut node = b;
    let mut cur_cb = cb;
    let mut tail = (0.0_f32, 0.0_f32);
    loop {
        let key = (node.node, node.origin.role);
        if !std::ptr::eq(node, b)
            && let Some(&(cached_cb, pos, neg)) = cache.get(&key)
            && cached_cb == cur_cb
        {
            tail = (pos, neg);
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
                cur_cb = content_box_width(node, cur_cb, viewport);
                node = child;
            }
            None => break,
        }
    }

    let mut running = tail;
    for (key, own, cb_used) in chain.into_iter().rev() {
        running = fold_margin(running, own);
        cache.insert(key, (cb_used, running.0, running.1));
    }
    running.0 + running.1
}

/// Returns the last in-flow `Block` child whose bottom margin collapses with the
/// owning box's bottom margin (CSS 2.1 §8.3.1). Mirror of `first_collapsible_child`
/// for the bottom edge: out-of-flow children (floats, absolutely positioned),
/// `::marker`s and zero-height `Skip` boxes are transparent and skipped. If the
/// last remaining in-flow child is not a plain `Block` (e.g. an inline run or a
/// replaced element) the collapsing chain is broken and `None` is returned. A
/// child with clearance also breaks the chain.
pub(crate) fn last_collapsible_child(b: &LayoutBox) -> Option<&LayoutBox> {
    if !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb) {
        return None;
    }
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
/// used to resolve percentage margins. Folds like `collapsed_top_margin`: largest
/// non-negative margin of the chain plus its most negative one (BUG-1365).
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
    if let Some(&(cached_cb, pos, neg)) = cache.get(&(b.node, b.origin.role))
        && cached_cb == cb
    {
        return pos + neg;
    }

    let mut chain: Vec<((NodeId, BoxRole), f32, f32)> = Vec::new(); // (key, own_margin, cb_used)
    let mut node = b;
    let mut cur_cb = cb;
    let mut tail = (0.0_f32, 0.0_f32);
    loop {
        let key = (node.node, node.origin.role);
        if !std::ptr::eq(node, b)
            && let Some(&(cached_cb, pos, neg)) = cache.get(&key)
            && cached_cb == cur_cb
        {
            tail = (pos, neg);
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
                cur_cb = content_box_width(node, cur_cb, viewport);
                node = child;
            }
            None => break,
        }
    }

    let mut running = tail;
    for (key, own, cb_used) in chain.into_iter().rev() {
        running = fold_margin(running, own);
        cache.insert(key, (cb_used, running.0, running.1));
    }
    running.0 + running.1
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
