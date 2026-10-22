//! CSS Container Queries L1 second-pass re-layout + CSS Anchor Positioning L1
//! post-layout repositioning.
//!
//! Перенесено батчем SPLIT-BT4 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn apply_container_styles`) без правок тел.

use super::*;
use crate::counters::CounterMap;
use crate::style::{matching_container_rules, apply_matched_container_rules, ContainerKinds, ScrollStateInfo};
use std::collections::HashSet;

/// The nodes whose `overflow` goes to the viewport: the root element, and `body` while the
/// root's own `overflow` is `visible` (CSS Overflow 3 §3.5).
fn viewport_propagating_nodes(root: &LayoutBox) -> HashSet<NodeId> {
    fn first_box(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip))
    }
    let mut out = HashSet::new();
    // `root` is the document box; its first real child is `html`, whose first is `body`.
    let Some(html) = first_box(root) else { return out };
    out.insert(html.node);
    let visible = |s: &ComputedStyle| matches!(s.overflow_x, Overflow::Visible) && matches!(s.overflow_y, Overflow::Visible);
    if visible(&html.style)
        && let Some(body) = first_box(html)
    {
        out.insert(body.node);
    }
    out
}

/// The box establishes a query container (`container-type` other than `normal`).
fn is_query_container(style: &ComputedStyle) -> bool {
    !matches!(style.container_type, ContainerType::Normal) || style.container_scroll_state
}

/// CSS Container Queries L1: second-pass after layout.
///
/// Walks the laid-out box tree looking for elements that establish containers
/// (`container-type: size | inline-size`). For each container, resolves its
/// content dimensions from the first-pass layout rect, re-applies matching
/// `@container` rules to all descendants, then re-lays out those descendants
/// so that layout-affecting properties (width, height, display, …) take effect.
///
/// Phase 0 limitations:
/// - Only block-flow children are re-laid out (Flex/Grid children use first-pass positions).
/// - Nested containers are processed outermost-first (inner containers are re-entered in
///   the same walk, but they use the parent container's context for their own re-layout).
#[allow(clippy::too_many_arguments)]
pub fn apply_container_styles(
    root: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    counters: &mut CounterMap,
) {
    // No container rules in this sheet → fast path.
    if sheet.container_rules.is_empty() {
        return;
    }
    let pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    // CSS2.1 §10.5: the initial containing block's height is the viewport height —
    // the `%`-height basis a root-level container itself would resolve against.
    let mut boxless = HashMap::new();
    let propagating = viewport_propagating_nodes(root);
    apply_container_inner(root, doc, sheet, viewport, measurer, pcb, viewport.height, hp, dark_mode, counters, &propagating, &mut boxless);
    counters.set_cq_boxless(boxless);
}

/// `parent_h` is the content-box height (px) of `b`'s *immediate* parent —
/// the CSS2.1 §10.5 containing-block-height basis for resolving `%` in `b`'s
/// own `height`/`top`/`bottom` in a `style()` query. Distinct from `pcb`
/// (nearest *positioned* containing block, used for abs/fixed descendants):
/// for a statically-positioned `b` nested several levels under a positioned
/// ancestor, `pcb` is that ancestor's rect, not `b`'s immediate parent.
///
/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion into itself: nothing after either branch's own closing loop
/// reads a value the descent produced, so the "recurse to find nested
/// containers" part of this function is the same safe mechanical class
/// LAYOUT-1 already converted — found alongside `shift_tree`/`shift_y_box`
/// while auditing container-query/anchor post-layout passes for the same
/// gap. The `lay_out(child, ...)` calls inside the `is_container` branch are
/// a different, already-tracked recursion class (layout dispatch) and stay
/// untouched — each is a bounded per-child call, not this function calling
/// itself.
#[allow(clippy::too_many_arguments)]
fn apply_container_inner(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    pcb: Rect,
    parent_h: f32,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    counters: &CounterMap,
    propagating: &HashSet<NodeId>,
    boxless: &mut HashMap<NodeId, Arc<ComputedStyle>>,
) {
    // The third element is the nearest query container above the box — the head of
    // the chain a descendant container links to via `ContainerContext::parent`.
    type Outer = Option<Arc<ContainerContext>>;
    let mut stack: Vec<(&mut LayoutBox, Rect, f32, Outer)> = vec![(b, pcb, parent_h, None)];
    while let Some((b, pcb, parent_h, outer)) = stack.pop() {
        // Derive content dimensions from already-laid-out rect + style — needed
        // both for container-query context (if `b` is a container) and as the
        // `parent_h` basis passed down to `b`'s own children either way.
        let em = b.style.font_size;
        let bw = b.rect.width;
        let pad_l = b.style.padding_left.resolve_or_zero(em, bw, viewport);
        let pad_r = b.style.padding_right.resolve_or_zero(em, bw, viewport);
        let pad_t = b.style.padding_top.resolve_or_zero(em, bw, viewport);
        let pad_b = b.style.padding_bottom.resolve_or_zero(em, bw, viewport);
        let content_w = (bw - pad_l - pad_r
            - b.style.border_left_width - b.style.border_right_width).max(0.0);
        let content_h_val = (b.rect.height - pad_t - pad_b
            - b.style.border_top_width - b.style.border_bottom_width).max(0.0);

        // A `Skip` box (`display: none`) is no principal box: it answers no query.
        let is_container = !matches!(b.kind, BoxKind::Skip) && is_query_container(&b.style);
        if is_container {
            let size_container = !matches!(b.style.container_type, ContainerType::Normal);
            let content_h = if matches!(b.style.container_type, ContainerType::Size) {
                Some(content_h_val)
            } else {
                None // inline-size: height not queryable
            };
            let ctx = Arc::new(ContainerContext {
                width: content_w,
                height: content_h,
                names: b.style.container_name.clone(),
                custom_props: b.style.custom_props.clone(),
                style_props: crate::selector_query::computed_style_to_map(&b.style),
                font_size: em,
                viewport,
                own_containing_block_height: parent_h,
                kinds: ContainerKinds { size: size_container, scroll_state: b.style.container_scroll_state },
                scroll_state: if b.style.container_scroll_state {
                    crate::scroll_state::scroll_state_info(b, propagating.contains(&b.node))
                } else {
                    ScrollStateInfo::default()
                },
                parent: outer,
                rule_hits: Vec::new(),
            }.with_rule_hits(sheet));
            // Re-apply container rules to all direct + indirect descendants.
            let mut seen = SeenBoxes::default();
            for child in &mut b.children {
                re_style_subtree(child, doc, sheet, &ctx, viewport, dark_mode, &mut seen);
            }
            restyle_boxless(doc, sheet, counters, b.node, &seen, &ctx, viewport, dark_mode, boxless);
            if !size_container {
                // A scroll-state-only container does not contain its size: the queries
                // restyle the subtree, but the box tree below keeps its first-pass geometry
                // (re-laying out a block stack here would drop margin collapsing, floats and
                // inline flow of an `overflow: auto` scroller).
                for child in &mut b.children {
                    stack.push((child, pcb, content_h_val, Some(Arc::clone(&ctx))));
                }
                continue;
            }
            // Re-lay out block-flow children with updated styles.
            let content_x = b.rect.x + pad_l + b.style.border_left_width;
            let content_y = b.rect.y + pad_t + b.style.border_top_width;
            let avail_h: Option<f32> = content_h;
            let child_pcb = if super::multicol_abspos::establishes_abs_cb(&b.style) {
                Rect::new(b.rect.x, b.rect.y, b.rect.width, b.rect.height)
            } else {
                pcb
            };
            // Expose this container's dimensions to cq* unit resolution during re-layout.
            set_cq_context(content_w, content_h);
            let mut child_y = content_y;
            for child in &mut b.children {
                if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                    // Re-lay out against new pcb but don't advance child_y.
                    lay_out(child, content_x, child_y, content_w, avail_h, measurer, viewport, child_pcb, hp, false);
                    continue;
                }
                lay_out(child, content_x, child_y, content_w, avail_h, measurer, viewport, child_pcb, hp, false);
                if matches!(child.kind, BoxKind::Skip) {
                    continue;
                }
                let child_mb = child.style.margin_bottom
                    .resolve_or_zero(child.style.font_size, content_w, viewport);
                child_y = child.rect.y + child.rect.height + child_mb;
            }
            clear_cq_context();
            // After re-layout, push children to catch nested containers.
            // Each nested container will set its own cq* context during its own re-layout.
            for child in &mut b.children {
                stack.push((child, child_pcb, content_h_val, Some(Arc::clone(&ctx))));
            }
        } else {
            // Not a container — just descend looking for container descendants.
            for child in &mut b.children {
                stack.push((child, pcb, content_h_val, outer.clone()));
            }
        }
    }
}

/// CSS Anchor Positioning L1 — post-layout pass.
///
/// Builds the anchor registry from the completed layout tree, then walks the
/// tree to reposition every absolutely/fixed-positioned element that has both
/// `position-anchor` and a non-`none` `inset-area` set.  Called once after all
/// layout and container-query passes complete.
///
/// Respects `anchor-scope`: anchors in a scoped subtree are invisible to
/// positioned elements that are not descendants of the scope root.
pub(crate) fn apply_anchor_positions(
    root: &mut LayoutBox,
    doc: &lumen_dom::Document,
    counters: &CounterMap,
    viewport: Size,
) {
    let inline = crate::anchor::InlineAnchors::from_cascade(doc, counters.styles());
    let registry = collect_anchors_inline(root, inline.as_ref());
    if registry.is_empty() {
        return;
    }
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let mut links = Vec::new();
    apply_anchor_positions_rec(root, &registry, viewport, init_pcb, &mut Vec::new(), &mut links);
    chain_anchor_scroll(root, &links);
    // A relayout keeps the scroll offsets of the previous tree, so the fresh chains start from them.
    crate::refresh_anchor_scroll(root);
}

/// An anchor-positioned box and the anchor it was positioned against, kept for [`chain_anchor_scroll`].
struct AnchorLink {
    /// The positioned box.
    node: lumen_dom::NodeId,
    /// The element carrying the anchor name.
    anchor: lumen_dom::NodeId,
    /// Box ancestors of the positioned box: their scrolling needs no compensation.
    ancestors: Vec<lumen_dom::NodeId>,
    /// Scroll containers of the anchor (see [`anchor_scroll_for`]).
    scrollers: Vec<lumen_dom::NodeId>,
}

/// CSS Anchor Positioning L1 §4.3: a box positioned against an anchor that is itself positioned
/// against another anchor follows both chains of scroll containers (`anchor-scroll-chained-*`).
fn chain_anchor_scroll(root: &mut LayoutBox, links: &[AnchorLink]) {
    use std::collections::HashMap;
    let by_node: HashMap<lumen_dom::NodeId, usize> = links.iter().enumerate().map(|(i, l)| (l.node, i)).collect();
    if !links.iter().any(|l| by_node.contains_key(&l.anchor)) {
        return;
    }
    let mut chained: HashMap<lumen_dom::NodeId, Vec<lumen_dom::NodeId>> = HashMap::new();
    for l in links {
        let mut all = l.scrollers.clone();
        let mut hop = l.anchor;
        // A cycle (`a` against `b` against `a`) ends at the hop bound, not by luck.
        for _ in 0..links.len() {
            let Some(&i) = by_node.get(&hop) else { break };
            for &n in &links[i].scrollers {
                if !all.contains(&n) {
                    all.insert(0, n);
                }
            }
            hop = links[i].anchor;
        }
        all.retain(|n| !l.ancestors.contains(n));
        if all != l.scrollers {
            chained.insert(l.node, all);
        }
    }
    if chained.is_empty() {
        return;
    }
    let mut stack: Vec<&mut LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        // The principal box of the node only (first in pre-order).
        if let Some(all) = chained.remove(&b.node) {
            b.anchor_scroll = scroll_of(&all);
        }
        stack.extend(b.children.iter_mut().rev());
    }
}

/// The anchor `lb` is positioned against and the scroll containers around it that `lb` follows
/// (CSS Anchor Positioning L1 §4.3): the ones that are not among `lb`'s own box ancestors, which
/// already scroll it. The anchor is the `position-anchor` one, else the first one named in an
/// `anchor()`/`anchor-size()`. `None` — not an anchor-positioned box.
fn anchor_scroll_for(
    lb: &LayoutBox,
    registry: &crate::anchor::AnchorRegistry,
    ancestors: &[lumen_dom::NodeId],
) -> Option<(lumen_dom::NodeId, Vec<lumen_dom::NodeId>)> {
    if !matches!(lb.style.position, Position::Absolute | Position::Fixed) {
        return None;
    }
    let s = &lb.style;
    let visible = |n: &&str| registry.get_scoped(n, ancestors).is_some();
    let named = [&s.anchor_top, &s.anchor_right, &s.anchor_bottom, &s.anchor_left]
        .into_iter()
        .flatten()
        .filter_map(|f| f.anchor_name.as_deref())
        .chain([&s.anchor_size_w, &s.anchor_size_h].into_iter().flatten().filter_map(|f| f.anchor_name.as_deref()));
    let name = s.position_anchor.as_deref().filter(visible).or_else(|| named.into_iter().find(visible))?;
    let anchor = registry.get(name)?.node;
    let scrollers = registry
        .scroll_chains
        .get(name)
        .map(|chain| chain.iter().filter(|n| !ancestors.contains(n)).copied().collect())
        .unwrap_or_default();
    Some((anchor, scrollers))
}

fn scroll_of(nodes: &[lumen_dom::NodeId]) -> Option<Box<AnchorScroll>> {
    (!nodes.is_empty()).then(|| {
        Box::new(AnchorScroll { scrollers: nodes.iter().map(|&node| AnchorScroller { node, x: 0.0, y: 0.0 }).collect() })
    })
}

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion — same class as `apply_container_inner` above. `ancestors`
/// mirrors the push-before-children/pop-after-children discipline the old
/// recursive call gave for free: each popped frame carries `depth` (its own
/// distance from the walk's root), and `ancestors.truncate(depth)` restores
/// exactly the path this node's *parent* saw before this node's own
/// processing runs — reproducing "push after processing, pop after
/// children" regardless of the LIFO stack's (sibling-reversed, but
/// parent-before-child) visiting order, which is all `resolve_inset_area_scoped`/
/// `resolve_inset_scoped`'s anchor-scope checks depend on.
fn apply_anchor_positions_rec(
    lb: &mut LayoutBox,
    registry: &crate::anchor::AnchorRegistry,
    viewport: Size,
    pcb: Rect,
    ancestors: &mut Vec<lumen_dom::NodeId>,
    links: &mut Vec<AnchorLink>,
) {
    // `fixed_cb` — padding box of the nearest ancestor that captures `position: fixed`
    // (transform/filter/contain, css-transforms-1 §2); `None` — the viewport.
    let mut stack: Vec<(&mut LayoutBox, Rect, Option<Rect>, usize)> = vec![(lb, pcb, None, ancestors.len())];
    while let Some((lb, pcb, fixed_cb, depth)) = stack.pop() {
    ancestors.truncate(depth);
    let viewport_rect = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    // CSS Anchor Positioning L1 §4 — correct `anchor-size()` width/height against the
    // final, global anchor registry. The local registry `lay_out_abs_children` used is
    // collected once before any deferred (abs/fixed) sibling in the same containing
    // block is laid out, so an `anchor-size()` referencing a sibling anchor that is
    // itself abs/fixed-positioned always sees that anchor's stale (pre-layout) rect
    // there. Mirrors the `anchor()` inset correction below; must run first since the
    // inset-area/inset corrections below read `lb.rect.width`/`height`.
    // CSS: anchor-size(), position-anchor
    if matches!(lb.style.position, Position::Absolute | Position::Fixed) {
        let default_anchor = lb.style.position_anchor.as_deref();
        let em = lb.style.font_size;
        let cb = if matches!(lb.style.position, Position::Fixed) {
            fixed_cb.unwrap_or(viewport_rect)
        } else {
            pcb
        };
        if let Some(w) = lb.style.anchor_size_w.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size_or_fallback(registry, f, default_anchor, em, cb.width, viewport)
        }) {
            lb.rect.width = w;
        }
        if let Some(h) = lb.style.anchor_size_h.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size_or_fallback(registry, f, default_anchor, em, cb.height, viewport)
        }) {
            lb.rect.height = h;
        }
    }

    // Resolve inset-area for this element if it is abs/fixed-positioned with a named anchor.
    let anchor_name = matches!(lb.style.position, Position::Absolute | Position::Fixed)
        .then(|| lb.style.position_anchor.as_deref())
        .flatten();
    let mut positioned_by_inset_area = false;
    if let Some(anchor_name) = anchor_name {
        let row = lb.style.inset_area_row;
        let col = lb.style.inset_area_col;
        if row != InsetAreaKeyword::None || col != InsetAreaKeyword::None {
            positioned_by_inset_area = true;
            let cb = if matches!(lb.style.position, Position::Fixed) {
                fixed_cb.unwrap_or(viewport_rect)
            } else {
                pcb
            };
            // A definite-size axis keeps its size and aligns toward the anchor;
            // an `auto` axis stretches to fill its position-area band.
            let elem_w = if lb.style.width.is_some() {
                crate::anchor::AxisSize::Fixed(lb.rect.width)
            } else {
                crate::anchor::AxisSize::Auto
            };
            let elem_h = if lb.style.height.is_some() {
                crate::anchor::AxisSize::Fixed(lb.rect.height)
            } else {
                crate::anchor::AxisSize::Auto
            };
            // Use scope-aware lookup so anchors behind anchor-scope barriers are
            // invisible to positioned elements outside their subtree.
            if let Some(pos) = crate::anchor::resolve_inset_area_scoped(
                registry, anchor_name, row, col, cb, elem_w, elem_h, ancestors,
            ) {
                let new_x = cb.x + pos.left;
                let new_y = cb.y + pos.top;
                let dx = new_x - lb.rect.x;
                let dy = new_y - lb.rect.y;
                shift_tree(lb, dx, dy);
                if let Some(w) = pos.width {
                    lb.rect.width = w;
                }
                if let Some(h) = pos.height {
                    lb.rect.height = h;
                }
            }
        }
    }

    // CSS Anchor Positioning L1 §3.1 — correct plain `anchor()` insets against the
    // final, global anchor registry (mirrors the `inset-area` correction above; the
    // local registry `lay_out_abs_children` used may have missed anchors outside this
    // element's containing-block subtree, or anchors whose rect only became final after
    // a later container-query relayout pass). Skipped when `inset-area` already placed
    // the element — that shorthand takes full precedence over plain insets.
    // CSS: anchor(), position-anchor, anchor-scope
    if !positioned_by_inset_area && matches!(lb.style.position, Position::Absolute | Position::Fixed) {
        let has_anchor_func = lb.style.anchor_left.is_some()
            || lb.style.anchor_right.is_some()
            || lb.style.anchor_top.is_some()
            || lb.style.anchor_bottom.is_some();
        if has_anchor_func {
            let cb = if matches!(lb.style.position, Position::Fixed) {
                fixed_cb.unwrap_or(viewport_rect)
            } else {
                pcb
            };
            let em = lb.style.font_size;
            let default_anchor = lb.style.position_anchor.as_deref();
            let left = crate::anchor::resolve_inset_scoped(
                registry, &lb.style.left, lb.style.anchor_left.as_ref(), default_anchor, true, false,
                cb.x, cb.x + cb.width, em, cb.width, viewport, ancestors,
            );
            let right = crate::anchor::resolve_inset_scoped(
                registry, &lb.style.right, lb.style.anchor_right.as_ref(), default_anchor, true, true,
                cb.x, cb.x + cb.width, em, cb.width, viewport, ancestors,
            );
            let top = crate::anchor::resolve_inset_scoped(
                registry, &lb.style.top, lb.style.anchor_top.as_ref(), default_anchor, false, false,
                cb.y, cb.y + cb.height, em, cb.height, viewport, ancestors,
            );
            let bottom = crate::anchor::resolve_inset_scoped(
                registry, &lb.style.bottom, lb.style.anchor_bottom.as_ref(), default_anchor, false, true,
                cb.y, cb.y + cb.height, em, cb.height, viewport, ancestors,
            );
            let c_ml = lb.style.margin_left.resolve_or_zero(em, cb.width, viewport);
            let c_mr = lb.style.margin_right.resolve_or_zero(em, cb.width, viewport);
            let c_mt = lb.style.margin_top.resolve_or_zero(em, cb.height, viewport);
            let c_mb = lb.style.margin_bottom.resolve_or_zero(em, cb.height, viewport);

            // Only override the axis when the corresponding side resolved — an
            // unresolved `anchor()` with no fallback computes as `auto`, leaving
            // this axis at whatever `lay_out_abs_children` already placed it at
            // (its own static-position fallback).
            let new_x = match (left, right) {
                (Some(l), _) => Some(cb.x + l + c_ml),
                (None, Some(r)) => Some(cb.x + cb.width - r - c_mr - lb.rect.width),
                (None, None) => None,
            };
            let new_y = match (top, bottom) {
                (Some(t), _) => Some(cb.y + t + c_mt),
                (None, Some(bv)) => Some(cb.y + cb.height - bv - c_mb - lb.rect.height),
                (None, None) => None,
            };
            let dx = new_x.map_or(0.0, |nx| nx - lb.rect.x);
            let dy = new_y.map_or(0.0, |ny| ny - lb.rect.y);
            if dx != 0.0 || dy != 0.0 {
                shift_tree(lb, dx, dy);
            }
        }
    }

    // CSS Anchor Positioning L1 §4.3 — the scroll containers the anchor sits in that this
    // box is not inside of: their offsets move the anchor, so they move this box too.
    lb.anchor_scroll = None;
    if let Some((anchor, scrollers)) = anchor_scroll_for(lb, registry, ancestors) {
        lb.anchor_scroll = scroll_of(&scrollers);
        links.push(AnchorLink { node: lb.node, anchor, ancestors: ancestors.to_vec(), scrollers });
    }

    // Compute the containing blocks for absolute- and fixed-positioned children of this element.
    let padding_box = Rect::new(
        lb.rect.x + lb.style.border_left_width,
        lb.rect.y + lb.style.border_top_width,
        (lb.rect.width - lb.style.border_left_width - lb.style.border_right_width).max(0.0),
        (lb.rect.height - lb.style.border_top_width - lb.style.border_bottom_width).max(0.0),
    );
    let my_pcb = if super::multicol_abspos::establishes_abs_cb(&lb.style) { padding_box } else { pcb };
    let my_fixed_cb = if crate::resolved_geometry::contains_fixed_descendants(&lb.style) {
        Some(padding_box)
    } else {
        fixed_cb
    };

    ancestors.push(lb.node);
    let child_depth = ancestors.len();
    for child in &mut lb.children {
        stack.push((child, my_pcb, my_fixed_cb, child_depth));
    }
    }
}

/// Re-applies container rules to a subtree — explicit heap-stack pre-order
/// walk (LAYOUT-2 срез 2), same class as the two functions above. Stops
/// descending into elements that are themselves containers (they will be
/// processed by `apply_container_inner` with their own context); that
/// condition depends only on the popped node's own style, not on anything a
/// deeper call would compute, so it does not change the safe classification.
fn re_style_subtree(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
    seen: &mut SeenBoxes,
) {
    let mut stack: Vec<&mut LayoutBox> = vec![b];
    while let Some(b) = stack.pop() {
        if !matches!(b.kind, BoxKind::Skip) {
            seen.boxed.insert(b.node);
            let rules = matching_container_rules(doc, b.node, sheet, ctx);
            if !rules.is_empty() {
                apply_matched_container_rules(Arc::make_mut(&mut b.style), &rules, doc, b.node, viewport, dark_mode);
            }
        }
        // Don't propagate into nested containers — they'll build their own context.
        // A `Skip` box (`display: none`) builds none: its subtree is left to
        // `restyle_boxless`, which records it unmatched.
        if is_query_container(&b.style) {
            if !matches!(b.kind, BoxKind::Skip) {
                seen.nested.insert(b.node);
            }
        } else {
            stack.extend(b.children.iter_mut());
        }
    }
}

/// Nodes [`re_style_subtree`] met below a container: those with a box, and the nested
/// containers (whose subtrees belong to the nested container's own pass).
#[derive(Default)]
struct SeenBoxes {
    boxed: HashSet<NodeId>,
    nested: HashSet<NodeId>,
}

/// `@container` rules for the elements below `container` that own no box — plain
/// inline elements, `display: none` / `contents` — which `re_style_subtree` cannot
/// reach through the box tree. Their cascaded style comes from `counters`; the ones a
/// rule matches are restyled, and all of them are recorded in `out`.
#[allow(clippy::too_many_arguments)]
fn restyle_boxless(
    doc: &Document,
    sheet: &Stylesheet,
    counters: &CounterMap,
    container: NodeId,
    seen: &SeenBoxes,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
    out: &mut HashMap<NodeId, Arc<ComputedStyle>>,
) {
    // `(node, reached)`: `reached` is false below a box-less query container.
    let mut stack: Vec<(NodeId, bool)> = doc.get(container).children.iter().rev().map(|&c| (c, true)).collect();
    while let Some((id, reached)) = stack.pop() {
        if !matches!(doc.get(id).data, NodeData::Element { .. }) || seen.nested.contains(&id) {
            continue;
        }
        let mut below_reached = reached;
        if !seen.boxed.contains(&id)
            && let Some(mut style) = counters.style_arc(id)
        {
            // A container with no principal box answers no query: what lies below it is
            // out of this container's reach and is not matched against it either — but it
            // is still recorded below, with its cascaded style (its queries all fail).
            if is_query_container(&style) {
                below_reached = false;
            }
            if reached {
                let rules = matching_container_rules(doc, id, sheet, ctx);
                if !rules.is_empty() {
                    apply_matched_container_rules(Arc::make_mut(&mut style), &rules, doc, id, viewport, dark_mode);
                }
            }
            // Every box-less element below a container is recorded, matched or not: the
            // snapshot collectors read their style (and custom properties) from here.
            out.insert(id, style);
        }
        stack.extend(doc.get(id).children.iter().rev().map(|&c| (c, below_reached)));
    }
}
