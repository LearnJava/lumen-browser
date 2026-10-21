//! Box geometry of generated pseudo-elements for `CSSPseudoElement` (CSS Pseudo-Elements L4 §3,
//! CSSOM-11): `getBoundingClientRect`/`getClientRects`/`getBoxQuads` and the coordinate
//! conversions of a `::before`/`::after`/`::scroll-marker`… object.
//!
//! [`collect_layout_rects`](crate::collect_layout_rects) keys by `NodeId` and lets the owner's own
//! box win, so a pseudo-element box (same `node`, different box) never reaches it. This table is
//! keyed like [`collect_pseudo_computed_styles`](crate::collect_pseudo_computed_styles): by
//! `(owner NodeId index, pseudo name)` with the lowercase no-colon spelling.
//!
//! Only pseudo-elements that own a `LayoutBox` are published (block-level, floated, positioned,
//! flex/grid items…). An inline `::before`/`::after` is flattened into the enclosing `InlineRun`
//! as text fragments that carry no pseudo kind, so it has no entry — the JS side answers an empty
//! quad list for it, same as for a pseudo-element that is not generated at all.

use std::collections::HashMap;

use lumen_core::geom::Size;

use crate::box_tree::{BoxRole, LayoutBox};
use crate::style::{ComputedStyle, LengthOrAuto};

/// Per pseudo-element: the published border box `[x, y, w, h]` (viewport-relative CSS px, the
/// AABB of its own transform like `getBoundingClientRect`) followed by the used border widths,
/// paddings and margins, each `[top, right, bottom, left]`. 16 numbers.
pub type PseudoGeometry = [f32; 16];

/// `(owner node index, pseudo name)` → [`PseudoGeometry`] for every pseudo-element box in `root`.
pub fn collect_pseudo_rects(root: &LayoutBox, viewport: Size) -> HashMap<(u32, String), PseudoGeometry> {
    let mut out = HashMap::new();
    // (box, width of the box it lives in — the basis of percentage paddings/margins).
    let mut stack: Vec<(&LayoutBox, f32)> = vec![(root, viewport.width)];
    while let Some((b, cb_w)) = stack.pop() {
        if let BoxRole::Pseudo(kind) = b.origin.role
            && let Some(name) = crate::pseudo_kind_name(kind)
        {
            let owner = b.origin.node.unwrap_or(b.node);
            out.entry((owner.index() as u32, name.to_string()))
                .or_insert_with(|| geometry_of(b, cb_w, viewport));
        }
        stack.extend(b.children.iter().rev().map(|c| (c, b.rect.width)));
    }
    out
}

fn geometry_of(b: &LayoutBox, cb_w: f32, viewport: Size) -> PseudoGeometry {
    let s: &ComputedStyle = &b.style;
    let r = crate::box_published_rect(b);
    let em = s.font_size;
    let padding = b.used_padding.unwrap_or_else(|| {
        [
            s.padding_top.resolve_or_zero(em, cb_w, viewport),
            s.padding_right.resolve_or_zero(em, cb_w, viewport),
            s.padding_bottom.resolve_or_zero(em, cb_w, viewport),
            s.padding_left.resolve_or_zero(em, cb_w, viewport),
        ]
    });
    let margin = |m: &LengthOrAuto| match m {
        LengthOrAuto::Auto => 0.0,
        LengthOrAuto::Length(l) => l.resolve_or_zero(em, cb_w, viewport),
    };
    [
        r.x,
        r.y,
        r.width,
        r.height,
        s.border_top_width,
        s.border_right_width,
        s.border_bottom_width,
        s.border_left_width,
        padding[0],
        padding[1],
        padding[2],
        padding[3],
        margin(&s.margin_top),
        margin(&s.margin_right),
        margin(&s.margin_bottom),
        margin(&s.margin_left),
    ]
}
