//! CSSOM §resolved value for the box-geometry properties (CSSOM-9, BUG-472).
//!
//! [`computed_style_to_map`](crate::computed_style_to_map) serialises the
//! *computed* value of every property — for `width` that is `"auto"` or
//! `"50%"`, not the pixels a real UA answers from `getComputedStyle()`.
//! CSSOM §6.7.2 makes `width`/`height`, `margin-*`, `padding-*` and (for
//! positioned boxes) `top`/`right`/`bottom`/`left` *resolved value special
//! case properties like height*: when the element is rendered, the resolved
//! value is the **used** value. That value exists only once layout ran, so it
//! is patched into the already-serialised map here, from the laid-out
//! [`LayoutBox`] tree, instead of in the style-only serialiser.
//!
//! [`LayoutBox`] keeps only the border-box `rect`; padding and margins are
//! re-derived the way layout resolved them — percentages against the
//! containing block's width, carried down the walk as [`GeomCtx`].
//!
//! Known gaps (keep the computed value, as before this slice):
//! * `auto` margins outside block flow (flex/grid items, floats, abspos) —
//!   their used value depends on sibling placement, not just the parent box.
//! * `position: fixed` insets that are `auto` — the fixed box's page-space
//!   `rect` includes the scroll offset at layout time, which this walk does
//!   not know.
//! * `position: sticky` insets — the used offset depends on the scroll
//!   position at read time.


use lumen_core::geom::{Rect, Size};

use crate::box_tree::{BoxKind, BoxRole, LayoutBox};
use crate::style_map::StyleMap;
use crate::style::{BoxSizing, ComputedStyle, Display, FloatSide, LengthOrAuto, Overflow, Position};

/// Key prefix under which [`apply_used_geometry`] keeps the *computed* value
/// of every property it overwrites with a used one: `"computed:width"` →
/// `"auto"` next to `"width"` → `"784px"`. CSS Typed OM
/// (`computedStyleMap()`, CSS Typed OM L1 §5.3) answers computed values — a
/// `CSSKeywordValue('auto')` for an auto width, exactly as Chrome does — while
/// `getComputedStyle()` answers resolved ones, and both read this one
/// snapshot. `:` cannot appear in a CSS property name, so the key never
/// collides with a real property; readers of the resolved view skip it.
pub const COMPUTED_VALUE_KEY_PREFIX: &str = "computed:";

/// Marker key (BUG-1191) on a snapshot entry published for an element that owns
/// no `LayoutBox` — an empty inline, a `display: none` /
/// `content-visibility: hidden` descendant. The entry carries the element's
/// cascaded values so `getComputedStyle` answers like Chrome, but «has an
/// entry» no longer means «was laid out»; `innerText`/`checkVisibility` read
/// this marker (via `_lumen_get_computed_style`, the one key under the prefix
/// it answers) to tell the two apart. Under [`COMPUTED_VALUE_KEY_PREFIX`], so
/// it never shows up as a property.
pub const BOXLESS_KEY: &str = "computed:-lumen-boxless";

/// Inserts the used value `v` for `name`; the computed value it displaces stays reachable under
/// [`COMPUTED_VALUE_KEY_PREFIX`] when the two differ (decided on read, see [`StyleMap::set_used`]).
fn set_used(m: &mut StyleMap, name: &'static str, v: String) {
    m.set_used(name, v);
}

/// Containing blocks a box's children resolve against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GeomCtx {
    /// Content box of the parent box — the containing block of in-flow and
    /// relatively positioned children (CSS 2.1 §10.1 item 2).
    pub flow_cb: Rect,
    /// Padding box of the nearest positioned ancestor, or the initial
    /// containing block — the containing block of `position: absolute`
    /// children (CSS 2.1 §10.1 item 4).
    pub abs_cb: Rect,
    /// Whether the parent lays its block-level children out in normal block
    /// flow — the only context whose `auto` horizontal margins are recovered
    /// from geometry.
    pub parent_block_flow: bool,
    /// Padding box of the nearest ancestor that is a scroll container
    /// (`overflow` other than `visible`/`clip` on either axis), or the
    /// viewport — the scrollport `position: sticky` percentage insets resolve
    /// against (css-position §sticky-pos, csswg-drafts #3115).
    pub scrollport: Rect,
}

impl GeomCtx {
    /// Context of the root box: both containing blocks are the initial
    /// containing block (CSS 2.1 §10.1 item 1), the viewport-sized rectangle
    /// at the canvas origin.
    pub(crate) fn root(viewport: Size) -> Self {
        let icb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
        GeomCtx { flow_cb: icb, abs_cb: icb, parent_block_flow: true, scrollport: icb }
    }
}

/// Whether `b` is an element's principal box whose used geometry CSSOM
/// reports. SVG shape/text boxes size from presentation attributes, not the
/// CSS box model; inline runs and markers are not element boxes at all.
fn reports_used_geometry(b: &LayoutBox) -> bool {
    b.origin.role == BoxRole::Element
        && matches!(
            b.kind,
            BoxKind::Block
                | BoxKind::FlowRoot
                | BoxKind::Table
                | BoxKind::TableRow
                | BoxKind::TableRowGroup
                | BoxKind::Image { .. }
                | BoxKind::Video { .. }
                | BoxKind::Canvas { .. }
                | BoxKind::Audio { .. }
                | BoxKind::Iframe { .. }
                | BoxKind::FormControl { .. }
                | BoxKind::SvgRoot { .. }
        )
}

fn lays_out_block_flow(display: Display) -> bool {
    matches!(
        display,
        Display::Block
            | Display::ListItem
            | Display::FlowRoot
            | Display::InlineBlock
            | Display::TableCell
            | Display::TableCaption
    )
}

fn is_block_level(display: Display) -> bool {
    matches!(display, Display::Block | Display::ListItem | Display::FlowRoot | Display::Table)
}

/// Used padding widths `[top, right, bottom, left]` in (zoomed) px.
fn used_padding(s: &ComputedStyle, cb_w: f32, vp: Size) -> [f32; 4] {
    let em = s.font_size;
    [
        s.padding_top.resolve_or_zero(em, cb_w, vp),
        s.padding_right.resolve_or_zero(em, cb_w, vp),
        s.padding_bottom.resolve_or_zero(em, cb_w, vp),
        s.padding_left.resolve_or_zero(em, cb_w, vp),
    ]
}

fn resolve_opt(l: &LengthOrAuto, em: f32, basis: f32, vp: Size) -> Option<f32> {
    match l {
        LengthOrAuto::Auto => None,
        LengthOrAuto::Length(len) => Some(len.resolve_or_zero(em, basis, vp)),
    }
}

/// Used inset pair along one axis for a relatively positioned box
/// (CSS 2.1 §9.4.3): one `auto` side mirrors the other; both `auto` is 0;
/// both set is over-constrained and the start side wins.
fn relative_pair(start: Option<f32>, end: Option<f32>) -> (f32, f32) {
    match (start, end) {
        (Some(s), _) => (s, -s),
        (None, Some(e)) => (-e, e),
        (None, None) => (0.0, 0.0),
    }
}

/// Containing block of a box with style `s` in context `ctx`.
fn containing_block(s: &ComputedStyle, ctx: &GeomCtx, vp: Size) -> Rect {
    match s.position {
        Position::Absolute => ctx.abs_cb,
        Position::Fixed => Rect::new(0.0, 0.0, vp.width, vp.height),
        _ => ctx.flow_cb,
    }
}

/// Context `b`'s children resolve against, given `b`'s own context. Called
/// for every box, including anonymous ones (whose rect *is* their content box).
pub(crate) fn child_ctx(b: &LayoutBox, ctx: &GeomCtx, vp: Size) -> GeomCtx {
    let s = &b.style;
    let r = b.rect;
    if b.origin.role != BoxRole::Element {
        return GeomCtx { flow_cb: r, abs_cb: ctx.abs_cb, parent_block_flow: ctx.parent_block_flow, scrollport: ctx.scrollport };
    }
    let (bt, br, bb, bl) = (s.border_top_width, s.border_right_width, s.border_bottom_width, s.border_left_width);
    let [pt, pr, pb, pl] = used_padding(s, containing_block(s, ctx, vp).width, vp);
    let padding_box = Rect::new(r.x + bl, r.y + bt, (r.width - bl - br).max(0.0), (r.height - bt - bb).max(0.0));
    let content_box = Rect::new(
        padding_box.x + pl,
        padding_box.y + pt,
        (padding_box.width - pl - pr).max(0.0),
        (padding_box.height - pt - pb).max(0.0),
    );
    let positioned = s.position != Position::Static;
    let scrolls = |o: Overflow| matches!(o, Overflow::Scroll | Overflow::Auto | Overflow::Hidden);
    GeomCtx {
        flow_cb: content_box,
        abs_cb: if positioned { padding_box } else { ctx.abs_cb },
        parent_block_flow: lays_out_block_flow(s.display),
        scrollport: if scrolls(s.overflow_x) || scrolls(s.overflow_y) { padding_box } else { ctx.scrollport },
    }
}

/// Overwrites the geometry entries of `m` (built by `computed_style_to_map`
/// from `b.style`) with their CSSOM resolved values. No-op for boxes that are
/// not an element's principal CSS box.
pub(crate) fn apply_used_geometry(m: &mut StyleMap, b: &LayoutBox, ctx: &GeomCtx, vp: Size) {
    if !reports_used_geometry(b) {
        return;
    }
    let s = &b.style;
    let z = if s.effective_zoom > 0.0 { s.effective_zoom } else { 1.0 };
    let px = |v: f32| crate::selector_query::px_str(v / z);
    let em = s.font_size;
    let r = b.rect;

    let cb = containing_block(s, ctx, vp);

    let [pt, pr, pb, pl] = used_padding(s, cb.width, vp);
    set_used(m, "padding-top", px(pt));
    set_used(m, "padding-right", px(pr));
    set_used(m, "padding-bottom", px(pb));
    set_used(m, "padding-left", px(pl));

    let (bt, br, bb, bl) = (s.border_top_width, s.border_right_width, s.border_bottom_width, s.border_left_width);
    let (w, h) = match s.box_sizing {
        BoxSizing::BorderBox => (r.width, r.height),
        BoxSizing::ContentBox => (
            (r.width - bl - br - pl - pr).max(0.0),
            (r.height - bt - bb - pt - pb).max(0.0),
        ),
    };
    set_used(m, "width", px(w));
    set_used(m, "height", px(h));

    // Insets first: a relative offset moves `rect` away from where the auto
    // margins placed it, so margin recovery below needs to undo it.
    let mut rel_dx = 0.0;
    match s.position {
        Position::Relative => {
            let (l, rt) = relative_pair(
                resolve_opt(&s.left, em, cb.width, vp),
                resolve_opt(&s.right, em, cb.width, vp),
            );
            let (t, bm) = relative_pair(
                resolve_opt(&s.top, em, cb.height, vp),
                resolve_opt(&s.bottom, em, cb.height, vp),
            );
            rel_dx = l;
            set_used(m, "left", px(l));
            set_used(m, "right", px(rt));
            set_used(m, "top", px(t));
            set_used(m, "bottom", px(bm));
        }
        Position::Absolute | Position::Fixed => {
            let margins = [&s.margin_top, &s.margin_right, &s.margin_bottom, &s.margin_left]
                .map(|mg| resolve_opt(mg, em, cb.width, vp).unwrap_or(0.0));
            let auto_geom = s.position == Position::Absolute;
            let sides: [(&str, &LengthOrAuto, f32, f32); 4] = [
                ("top", &s.top, cb.height, r.y - margins[0] - cb.y),
                ("right", &s.right, cb.width, cb.x + cb.width - (r.x + r.width) - margins[1]),
                ("bottom", &s.bottom, cb.height, cb.y + cb.height - (r.y + r.height) - margins[2]),
                ("left", &s.left, cb.width, r.x - margins[3] - cb.x),
            ];
            for (name, val, basis, from_geom) in sides {
                match resolve_opt(val, em, basis, vp) {
                    Some(v) => {
                        set_used(m, name, px(v));
                    }
                    None if auto_geom => {
                        set_used(m, name, px(from_geom));
                    }
                    None => {}
                }
            }
        }
        Position::Sticky => {
            // The used value of a sticky inset is the specified one; percentages
            // are taken of the nearest scrollport, not the containing block.
            let sp = ctx.scrollport;
            for (name, val, basis) in [
                ("top", &s.top, sp.height),
                ("right", &s.right, sp.width),
                ("bottom", &s.bottom, sp.height),
                ("left", &s.left, sp.width),
            ] {
                if let Some(v) = resolve_opt(val, em, basis, vp) {
                    set_used(m, name, px(v));
                }
            }
        }
        Position::Static => {}
    }

    let in_block_flow = ctx.parent_block_flow
        && is_block_level(s.display)
        && s.float_side == FloatSide::None
        && matches!(s.position, Position::Static | Position::Relative);
    let margins: [(&str, &LengthOrAuto, Option<f32>); 4] = [
        ("margin-top", &s.margin_top, in_block_flow.then_some(0.0)),
        ("margin-right", &s.margin_right, in_block_flow.then_some(cb.x + cb.width - (r.x - rel_dx + r.width))),
        ("margin-bottom", &s.margin_bottom, in_block_flow.then_some(0.0)),
        ("margin-left", &s.margin_left, in_block_flow.then_some(r.x - rel_dx - cb.x)),
    ];
    for (name, val, from_geom) in margins {
        match (resolve_opt(val, em, cb.width, vp), from_geom) {
            (Some(v), _) => {
                set_used(m, name, px(v));
            }
            (None, Some(g)) => {
                set_used(m, name, px(g));
            }
            (None, None) => {}
        }
    }

    if !s.transform.is_empty() {
        set_used(m, "transform", resolved_transform(&s.transform, z));
    }
}

/// CSS Transforms L1 §2: the resolved value of `transform` for a box that has
/// one is the list collapsed into a single `matrix()` (or `matrix3d()` when
/// any 3D component is non-trivial), with translations in CSS px (`zoom`
/// undone). The origin does not enter — it is applied at paint time.
pub(crate) fn resolved_transform(fns: &[crate::style::TransformFn], zoom: f32) -> String {
    let mut a = crate::property_trees::compute_local_transform(fns, (0.0, 0.0, 0.0)).0;
    a[12] /= zoom;
    a[13] /= zoom;
    a[14] /= zoom;
    let num = |v: f32| (if v.abs() < 1e-6 { 0.0 } else { v }).to_string();
    let flat = a[2] == 0.0
        && a[3] == 0.0
        && a[6] == 0.0
        && a[7] == 0.0
        && a[8] == 0.0
        && a[9] == 0.0
        && a[10] == 1.0
        && a[11] == 0.0
        && a[14] == 0.0
        && a[15] == 1.0;
    let pick: &[usize] = if flat { &[0, 1, 4, 5, 12, 13] } else { &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15] };
    let body = pick.iter().map(|&i| num(a[i])).collect::<Vec<_>>().join(", ");
    format!("{}({body})", if flat { "matrix" } else { "matrix3d" })
}

#[cfg(test)]
mod tests {
    use super::resolved_transform;
    use crate::style::TransformFn;

    #[test]
    fn transform_collapses_to_2d_matrix() {
        let fns = [TransformFn::TranslateX(100.0), TransformFn::Scale(2.0, 3.0)];
        assert_eq!(resolved_transform(&fns, 1.0), "matrix(2, 0, 0, 3, 100, 0)");
    }

    #[test]
    fn transform_with_3d_component_is_matrix3d() {
        let s = resolved_transform(&[TransformFn::TranslateZ(5.0)], 1.0);
        assert!(s.starts_with("matrix3d(") && s.ends_with(", 5, 1)"), "{s}");
    }

    #[test]
    fn transform_translation_undoes_zoom() {
        assert_eq!(resolved_transform(&[TransformFn::TranslateX(100.0)], 2.0), "matrix(1, 0, 0, 1, 50, 0)");
    }
}
