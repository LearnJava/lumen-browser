use super::*;

/// Crate-internal shim so `vertical.rs` can recursively invoke the main
/// `lay_out` for children inside a vertical writing-mode container.
///
/// Same parameters and semantics as the private `lay_out`. Exists only
/// because Rust modules cannot reach a sibling module's private functions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_for_vertical(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    lay_out(b, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp, false);
}

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

/// CSS 2.1 §8.3.1 — the *collapsed* top margin of a block-level box (px).
///
/// The top margin of an in-flow block collapses with the top margin of its
/// first in-flow block-level child when nothing separates them: the box has no
/// top border, no top padding, establishes no BFC, and the first in-flow child
/// is itself a plain block with no clearance. The collapse recurses down the
/// chain of first children. `cb` is the containing-block width used to resolve
/// percentage margins. Only the common non-negative case is folded (parity with
/// sibling collapse); negative margins fall through as the box's own margin.
pub(crate) fn collapsed_top_margin(b: &LayoutBox, cb: f32, viewport: Size) -> f32 {
    let em = b.style.font_size;
    let own = b.style.margin_top.resolve_or_zero(em, cb, viewport);
    if !matches!(b.kind, BoxKind::Block) || establishes_bfc(b) {
        return own;
    }
    let pt = b.style.padding_top.resolve_or_zero(em, cb, viewport);
    if pt != 0.0 || b.style.border_top_width != 0.0 {
        return own;
    }
    match first_collapsible_child(b) {
        Some(child) => {
            // Child's containing-block width = this box's content width.
            let child_cb = (cb
                - b.style.padding_left.resolve_or_zero(em, cb, viewport)
                - b.style.padding_right.resolve_or_zero(em, cb, viewport)
                - b.style.border_left_width
                - b.style.border_right_width)
                .max(0.0);
            own.max(collapsed_top_margin(child, child_cb, viewport))
        }
        None => own,
    }
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
/// recurses down the chain of last children. `cb` is the containing-block width
/// used to resolve percentage margins. Only the common non-negative case is folded
/// (parity with `collapsed_top_margin`); negative margins fall through as the box's
/// own margin.
pub(crate) fn collapsed_bottom_margin(b: &LayoutBox, cb: f32, viewport: Size) -> f32 {
    let em = b.style.font_size;
    let own = b.style.margin_bottom.resolve_or_zero(em, cb, viewport);
    if !matches!(b.kind, BoxKind::Block) || establishes_bfc(b) {
        return own;
    }
    // A definite height blocks the last child's bottom margin from reaching the
    // box's bottom edge, so the through-collapse does not happen.
    if b.style.height.is_some() {
        return own;
    }
    let pb = b.style.padding_bottom.resolve_or_zero(em, cb, viewport);
    if pb != 0.0 || b.style.border_bottom_width != 0.0 {
        return own;
    }
    match last_collapsible_child(b) {
        Some(child) => {
            // Child's containing-block width = this box's content width.
            let child_cb = (cb
                - b.style.padding_left.resolve_or_zero(em, cb, viewport)
                - b.style.padding_right.resolve_or_zero(em, cb, viewport)
                - b.style.border_left_width
                - b.style.border_right_width)
                .max(0.0);
            own.max(collapsed_bottom_margin(child, child_cb, viewport))
        }
        None => own,
    }
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
