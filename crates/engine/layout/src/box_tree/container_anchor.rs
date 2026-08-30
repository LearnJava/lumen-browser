//! CSS Container Queries L1 second-pass re-layout + CSS Anchor Positioning L1
//! post-layout repositioning.
//!
//! Перенесено батчем SPLIT-BT4 из `crates/engine/layout/src/box_tree.rs`
//! (анкер `fn apply_container_styles`) без правок тел.

use super::*;

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
pub fn apply_container_styles(
    root: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) {
    // No container rules in this sheet → fast path.
    if sheet.container_rules.is_empty() {
        return;
    }
    let pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    // CSS2.1 §10.5: the initial containing block's height is the viewport height —
    // the `%`-height basis a root-level container itself would resolve against.
    apply_container_inner(root, doc, sheet, viewport, measurer, pcb, viewport.height, hp, dark_mode);
}

/// `parent_h` is the content-box height (px) of `b`'s *immediate* parent —
/// the CSS2.1 §10.5 containing-block-height basis for resolving `%` in `b`'s
/// own `height`/`top`/`bottom` in a `style()` query. Distinct from `pcb`
/// (nearest *positioned* containing block, used for abs/fixed descendants):
/// for a statically-positioned `b` nested several levels under a positioned
/// ancestor, `pcb` is that ancestor's rect, not `b`'s immediate parent.
#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
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
) {
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

    let is_container = !matches!(b.style.container_type, ContainerType::Normal);
    if is_container {
        let content_h = if matches!(b.style.container_type, ContainerType::Size) {
            Some(content_h_val)
        } else {
            None // inline-size: height not queryable
        };
        let ctx = ContainerContext {
            width: content_w,
            height: content_h,
            names: b.style.container_name.clone(),
            custom_props: b.style.custom_props.clone(),
            style_props: crate::selector_query::computed_style_to_map(&b.style),
            font_size: em,
            viewport,
            own_containing_block_height: parent_h,
        };
        // Re-apply container rules to all direct + indirect descendants.
        for child in &mut b.children {
            re_style_subtree(child, doc, sheet, &ctx, viewport, dark_mode);
        }
        // Re-lay out block-flow children with updated styles.
        let content_x = b.rect.x + pad_l + b.style.border_left_width;
        let content_y = b.rect.y + pad_t + b.style.border_top_width;
        let avail_h: Option<f32> = content_h;
        let child_pcb = if !matches!(b.style.position, Position::Static) {
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
        // After re-layout, recurse into children to catch nested containers.
        // Each nested container will set its own cq* context during its own re-layout.
        for child in &mut b.children {
            apply_container_inner(child, doc, sheet, viewport, measurer, child_pcb, content_h_val, hp, dark_mode);
        }
    } else {
        // Not a container — just recurse looking for container descendants.
        for child in &mut b.children {
            apply_container_inner(child, doc, sheet, viewport, measurer, pcb, content_h_val, hp, dark_mode);
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
pub(crate) fn apply_anchor_positions(root: &mut LayoutBox, viewport: Size) {
    let registry = collect_anchors(root);
    if registry.is_empty() {
        return;
    }
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    apply_anchor_positions_rec(root, &registry, viewport, init_pcb, &mut Vec::new());
}

fn apply_anchor_positions_rec(
    lb: &mut LayoutBox,
    registry: &crate::anchor::AnchorRegistry,
    viewport: Size,
    pcb: Rect,
    ancestors: &mut Vec<lumen_dom::NodeId>,
) {
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
        if let Some(w) = lb.style.anchor_size_w.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size(registry, f, default_anchor)
        }) {
            lb.rect.width = w;
        }
        if let Some(h) = lb.style.anchor_size_h.as_ref().and_then(|f| {
            crate::anchor::resolve_anchor_size(registry, f, default_anchor)
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
                Rect::new(0.0, 0.0, viewport.width, viewport.height)
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
                Rect::new(0.0, 0.0, viewport.width, viewport.height)
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

    // Compute the containing block for absolute-positioned children of this element.
    let is_positioned = !matches!(lb.style.position, Position::Static);
    let my_pcb = if is_positioned {
        Rect::new(
            lb.rect.x + lb.style.border_left_width,
            lb.rect.y + lb.style.border_top_width,
            (lb.rect.width - lb.style.border_left_width - lb.style.border_right_width).max(0.0),
            (lb.rect.height - lb.style.border_top_width - lb.style.border_bottom_width).max(0.0),
        )
    } else {
        pcb
    };

    ancestors.push(lb.node);
    for child in &mut lb.children {
        apply_anchor_positions_rec(child, registry, viewport, my_pcb, ancestors);
    }
    ancestors.pop();
}

/// Recursively re-applies container rules to a subtree.
/// Stops descending into elements that are themselves containers (they will
/// be processed by `apply_container_inner` with their own context).
fn re_style_subtree(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
) {
    if !matches!(b.kind, BoxKind::Skip) {
        apply_container_rules(Arc::make_mut(&mut b.style), doc, b.node, sheet, ctx, viewport, dark_mode);
    }
    // Don't propagate into nested containers — they'll build their own context.
    if matches!(b.style.container_type, ContainerType::Normal) {
        for child in &mut b.children {
            re_style_subtree(child, doc, sheet, ctx, viewport, dark_mode);
        }
    }
}
