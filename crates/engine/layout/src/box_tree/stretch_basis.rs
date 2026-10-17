//! FLEX-VWM-6: the height a percentage block size resolves against while a box's
//! intrinsic width is measured.
//!
//! The intrinsic-width functions (`max_content_outer_width`,
//! `preferred_inline_block_width`) read style and contents, never a laid-out
//! rectangle, so a `height: 100%` below a flex item whose stretched height is
//! already definite (CSS Flexbox §9.8: a stretched item is definite, and the
//! percentage resolves against it) used to be `auto` there — a `<canvas
//! height="100%">` contributed its bitmap width instead of `height × ratio`
//! (`css-flexbox/aspect-ratio-intrinsic-size-001/002`).
//!
//! The basis is a thread-local rather than a parameter: both functions are
//! called from dozens of places that have no height to give.

use super::*;

thread_local! {
    /// Definite content height of the box whose children are being measured, or
    /// `None` — every percentage block size is `auto`, as before.
    static HEIGHT_BASIS: Cell<Option<f32>> = const { Cell::new(None) };
    /// The next measured box is a stretched flex item of this definite border-box
    /// height — its `height` is `auto`, but its children resolve against it.
    static STRETCHED_ITEM: Cell<Option<f32>> = const { Cell::new(None) };
}

/// The basis the percentage block sizes of `container`'s children resolve against.
pub(super) fn current() -> Option<f32> {
    HEIGHT_BASIS.with(Cell::get)
}

/// Runs `f` with `basis` as the definite content height of the box being measured
/// (`None` switches the machinery off for the call).
pub(super) fn with_container_height<R>(basis: Option<f32>, f: impl FnOnce() -> R) -> R {
    let prev = HEIGHT_BASIS.with(|c| c.replace(basis));
    let r = f();
    HEIGHT_BASIS.with(|c| c.set(prev));
    r
}

/// Runs `f` — a measurement of one flex item — with the item's stretched
/// border-box height; `None` leaves the item's own `height` to decide.
pub(super) fn with_stretched_item<R>(height: Option<f32>, f: impl FnOnce() -> R) -> R {
    let Some(h) = height else { return f() };
    let prev = HEIGHT_BASIS.with(|c| c.replace(Some(h)));
    STRETCHED_ITEM.with(|c| c.set(Some(h)));
    let r = f();
    STRETCHED_ITEM.with(|c| c.set(None));
    HEIGHT_BASIS.with(|c| c.set(prev));
    r
}

/// Does a row flex container's `item` stretch to the container's definite cross
/// size (CSS Flexbox §9.4 step 11): `align-self`/`align-items` is `stretch`,
/// neither cross margin is `auto`, `height` is `auto` and the container has a
/// single line?
pub(super) fn item_stretches(container: &ComputedStyle, item: &LayoutBox) -> bool {
    let is = &item.style;
    let align = if matches!(is.align_self, AlignValue::Auto) { container.align_items } else { is.align_self };
    matches!(align, AlignValue::Auto | AlignValue::Normal | AlignValue::Stretch)
        && matches!(container.flex_wrap, FlexWrap::Nowrap)
        && (is.height.is_none() || is.height_is_intrinsic_hint)
        && !matches!(is.margin_top, LengthOrAuto::Auto)
        && !matches!(is.margin_bottom, LengthOrAuto::Auto)
}

/// Border-box width of a `<canvas>` whose `width` is only the bitmap's intrinsic
/// one and whose `height` is authored: the height run through the ratio
/// (CSS 2.1 §10.3.2, Sizing L4 §4.1). `None` — `b` is not such a canvas, or its
/// height does not resolve against the current basis.
pub(super) fn canvas_transferred_width(b: &LayoutBox, viewport: Size) -> Option<f32> {
    let s = &b.style;
    if !matches!(b.kind, BoxKind::Canvas { .. })
        || !s.width_is_intrinsic_hint
        || s.height_is_intrinsic_hint
    {
        return None;
    }
    let (aw, ah) = s.aspect_ratio?;
    if aw <= 0.0 || ah <= 0.0 {
        return None;
    }
    let em = s.font_size;
    let h = s.height.as_ref()?.resolve(em, current(), viewport)?;
    let content_h = match s.box_sizing {
        BoxSizing::ContentBox => h,
        BoxSizing::BorderBox => h - frame_vertical(s, viewport),
    };
    let frame_h = s.padding_left.resolve_or_zero(em, 0.0, viewport)
        + s.padding_right.resolve_or_zero(em, 0.0, viewport)
        + s.border_left_width
        + s.border_right_width;
    Some(content_h.max(0.0) * aw / ah + frame_h)
}

/// Runs `f` — the measurement of `b` — with the basis its children resolve
/// against: a stretched item's own height, else `b`'s resolved `height`, else
/// none. Does nothing when no basis is active.
pub(super) fn scoped_for_children<R>(b: &LayoutBox, viewport: Size, f: impl FnOnce() -> R) -> R {
    let Some(basis) = current() else { return f() };
    // An anonymous line wrapper has no `height` of its own: it passes its parent's
    // content height on to the atomic inlines it holds.
    if matches!(b.kind, BoxKind::InlineBlockRow) {
        return f();
    }
    let stretched = STRETCHED_ITEM.with(Cell::take);
    let s = &b.style;
    let content = stretched
        .map(|h| (h - frame_vertical(s, viewport)).max(0.0))
        .or_else(|| {
            let h = s.height.as_ref().filter(|l| !l.is_intrinsic())?.resolve(s.font_size, Some(basis), viewport)?;
            Some(match s.box_sizing {
                BoxSizing::ContentBox => h,
                BoxSizing::BorderBox => (h - frame_vertical(s, viewport)).max(0.0),
            })
        });
    with_container_height(content, f)
}

fn frame_vertical(s: &ComputedStyle, viewport: Size) -> f32 {
    let em = s.font_size;
    s.padding_top.resolve_or_zero(em, 0.0, viewport)
        + s.padding_bottom.resolve_or_zero(em, 0.0, viewport)
        + s.border_top_width
        + s.border_bottom_width
}
