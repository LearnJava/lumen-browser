//! Липкое позиционирование (`position: sticky`) и CSS Scroll Snap L1:
//! снимки липких боксов и снап-контейнеров после раскладки, расчёт сдвига
//! и снап-цели при прокрутке. Вынесено из `lib.rs` без изменений.

use crate::{box_tree, style, LayoutBox};

// ─── Sticky-position algorithm ────────────────────────────────────────────────
// CSS: position: sticky — P4 wires insets from ComputedStyle (top/right/bottom/left);
//                         P3 wires scroll_x/scroll_y from shell scroll state.

/// Snapshot of a `position: sticky` element captured after normal-flow layout.
///
/// P3 integration: call `collect_sticky_boxes()` after every re-layout, then at
/// each scroll event call `compute_sticky_offset()` per entry and apply the
/// returned `(dx, dy)` translate to the element's paint transform.
#[derive(Debug, Clone)]
pub struct StickyBox {
    /// DOM node that owns this sticky element.
    pub node: lumen_dom::NodeId,
    /// Border-box rectangle as placed by normal flow, in CSS px (document-relative).
    pub static_rect: lumen_core::geom::Rect,
    /// CSS `top` inset in px, resolved against `em`/`rem`/the containing block's
    /// height/viewport as applicable. `None` when the property is `auto` or an
    /// intrinsic-sizing keyword (not valid for offsets, but defensively handled).
    pub top: Option<f32>,
    /// CSS `bottom` inset in px. `None` when auto.
    pub bottom: Option<f32>,
    /// CSS `left` inset in px, resolved against the containing block's width.
    /// `None` when auto.
    pub left: Option<f32>,
    /// CSS `right` inset in px. `None` when auto.
    pub right: Option<f32>,
    /// Border-box of the nearest block/flow-root ancestor — the sticky
    /// *containing block*.  The element cannot scroll visually past its edges.
    pub containing_rect: lumen_core::geom::Rect,
}

/// Collect all `position: sticky` elements from the layout tree in document order.
///
/// Returns one [`StickyBox`] per DOM element with `position: sticky`; `display:
/// none` subtrees (`BoxKind::Skip`) are omitted.  `containing_rect` in each
/// entry is the border-box of the nearest block or flow-root ancestor.
///
/// `viewport` is the layout viewport size, needed to resolve `vh`/`vw`/`vmin`/
/// `vmax` insets; pass the same `Size` used for the preceding layout pass.
///
/// Deduplicates by NodeId: the layout engine may produce both a `Block` wrapper
/// and a `FlowRoot` inner box for the same element (e.g. when sticky creates a
/// new BFC).  Only the first box seen (outermost, document-order) is recorded.
pub fn collect_sticky_boxes(root: &LayoutBox, viewport: lumen_core::geom::Size) -> Vec<StickyBox> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_sticky_rec(root, root.rect, viewport, &mut seen, &mut out);
    out
}

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion: `next_cb` is threaded down like a plain parameter (never read
/// back from a deeper call), so this is the same safe mechanical class
/// LAYOUT-1 already converted. `seen`/`out` insertion order does not need to
/// match document order (callers sort/index by node, not position), but
/// children are still pushed in reverse to keep behavior visibly identical
/// to the old left-to-right recursion.
fn collect_sticky_rec(
    root: &LayoutBox,
    containing_rect: lumen_core::geom::Rect,
    viewport: lumen_core::geom::Size,
    seen: &mut std::collections::HashSet<lumen_dom::NodeId>,
    out: &mut Vec<StickyBox>,
) {
    use box_tree::BoxKind;
    use style::Position;

    let mut stack: Vec<(&LayoutBox, lumen_core::geom::Rect)> = vec![(root, containing_rect)];
    while let Some((b, containing_rect)) = stack.pop() {
        if matches!(b.kind, BoxKind::Skip) {
            continue;
        }

        if matches!(b.style.position, Position::Sticky) && seen.insert(b.node) {
            let em = b.style.font_size;
            out.push(StickyBox {
                node: b.node,
                static_rect: b.rect,
                // top/bottom percentages resolve against the containing block's height;
                // left/right against its width (CSS Position L3 §9.4.1).
                top: b.style.top.resolve(em, containing_rect.height, viewport),
                bottom: b.style.bottom.resolve(em, containing_rect.height, viewport),
                left: b.style.left.resolve(em, containing_rect.width, viewport),
                right: b.style.right.resolve(em, containing_rect.width, viewport),
                containing_rect,
            });
        }

        // Blocks and flow roots establish a new sticky-containment boundary.
        let next_cb = if matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot) {
            b.rect
        } else {
            containing_rect
        };

        for child in b.children.iter().rev() {
            stack.push((child, next_cb));
        }
    }
}

/// Compute the visual offset `(dx, dy)` in CSS px to apply to a sticky element
/// at the given scroll position.
///
/// The returned offset should be added to the element's document-space position
/// at paint time (e.g. as a layer translate transform).  `(0.0, 0.0)` means no
/// sticking is needed.
///
/// # Algorithm (per axis)
///
/// The element's ideal viewport coordinate is `static_pos − scroll`.  CSS inset
/// properties clamp that within `[lo, hi]`; the containing block further
/// restricts the range so the element cannot leave its parent.
///
/// When `top` and `bottom` both fire simultaneously (e.g. containing block is
/// shorter than the viewport), `top` wins — matching browser behaviour.
pub fn compute_sticky_offset(
    sticky: &StickyBox,
    scroll_x: f32,
    scroll_y: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> (f32, f32) {
    let w = sticky.static_rect.width;
    let h = sticky.static_rect.height;

    // ── Y axis ───────────────────────────────────────────────────────────────
    let ideal_y = sticky.static_rect.y - scroll_y;
    // lo_y: the smallest (highest-on-screen) viewport-y the element may have.
    let lo_y = {
        let inset = sticky.top.unwrap_or(f32::NEG_INFINITY);
        let cb_top = sticky.containing_rect.y - scroll_y;
        inset.max(cb_top)
    };
    // hi_y: the largest (lowest-on-screen) viewport-y the element may have.
    let hi_y = {
        let inset = sticky
            .bottom
            .map(|b| viewport_height - b - h)
            .unwrap_or(f32::INFINITY);
        let cb_bot =
            sticky.containing_rect.y + sticky.containing_rect.height - scroll_y - h;
        inset.min(cb_bot)
    };
    // clamp: if lo_y > hi_y (containing block shorter than element), lo wins.
    let actual_y = ideal_y.clamp(lo_y, hi_y);
    let off_y = actual_y - ideal_y;

    // ── X axis ───────────────────────────────────────────────────────────────
    let ideal_x = sticky.static_rect.x - scroll_x;
    let lo_x = {
        let inset = sticky.left.unwrap_or(f32::NEG_INFINITY);
        let cb_left = sticky.containing_rect.x - scroll_x;
        inset.max(cb_left)
    };
    let hi_x = {
        let inset = sticky
            .right
            .map(|r| viewport_width - r - w)
            .unwrap_or(f32::INFINITY);
        let cb_right =
            sticky.containing_rect.x + sticky.containing_rect.width - scroll_x - w;
        inset.min(cb_right)
    };
    let actual_x = ideal_x.clamp(lo_x, hi_x);
    let off_x = actual_x - ideal_x;

    (off_x, off_y)
}

// ─── CSS Scroll Snap L1 algorithm stub ───────────────────────────────────────
// CSS: scroll-snap-type, scroll-snap-align, scroll-snap-stop
//
// P3 integration: after every re-layout call `collect_snap_containers(root)`.
// At each scroll event (shell::handle_scroll) call `find_snap_target()` per
// container and apply the returned scroll offset.
//
// P4 integration: `scroll_snap_type`, `scroll_snap_align`, `scroll_snap_stop`
// are already in `ComputedStyle` (style.rs). No additional CSS wiring needed.

/// A single snap area inside a [`SnapContainer`].
///
/// `snap_x` / `snap_y` are the container scroll offsets (CSS px) required to
/// align this area per its `scroll-snap-align` declaration.  `None` on an axis
/// means that axis does not contribute a snap position (keyword `none`).
///
/// All coordinates are in document space; subtract the container's own origin
/// to convert to content-relative scroll offsets.
#[derive(Debug, Clone)]
pub struct SnapPoint {
    /// DOM node that declares this snap area.
    pub node: lumen_dom::NodeId,
    /// Required container scroll-x for inline-axis alignment. `None` = not snapped on x.
    pub snap_x: Option<f32>,
    /// Required container scroll-y for block-axis alignment. `None` = not snapped on y.
    pub snap_y: Option<f32>,
    /// True when `scroll-snap-stop: always` — the scroller must stop here even
    /// during a high-velocity fling.
    pub stop_always: bool,
}

/// A scroll container that participates in CSS Scroll Snap L1.
///
/// Only containers whose `scroll-snap-type.axis` is not `None` are collected.
/// P3 integration: wire `rect` to the element's actual viewport dimensions and
/// call [`find_snap_target`] on every programmatic or user-driven scroll event.
#[derive(Debug, Clone)]
pub struct SnapContainer {
    /// DOM node of the scroll container element.
    pub node: lumen_dom::NodeId,
    /// CSS `scroll-snap-type` (axis + strictness). `axis` is never `None` here.
    pub snap_type: style::ScrollSnapType,
    /// Border-box of the scroll container in CSS px (document-relative).
    pub rect: lumen_core::geom::Rect,
    /// CSS `scroll-padding-top` in CSS px — shrinks the snap port from the block-start edge.
    pub scroll_padding_top: f32,
    /// CSS `scroll-padding-right` in CSS px — shrinks the snap port from the inline-end edge.
    pub scroll_padding_right: f32,
    /// CSS `scroll-padding-bottom` in CSS px — shrinks the snap port from the block-end edge.
    pub scroll_padding_bottom: f32,
    /// CSS `scroll-padding-left` in CSS px — shrinks the snap port from the inline-start edge.
    pub scroll_padding_left: f32,
    /// All snap areas found inside this container, in document order.
    pub points: Vec<SnapPoint>,
}

/// Collect all scroll containers that participate in CSS Scroll Snap L1.
///
/// Returns one [`SnapContainer`] per layout-tree element whose
/// `scroll-snap-type.axis` is not `None`.  Each entry's `points` list contains
/// all direct-descendant snap areas (elements with a non-`None`
/// `scroll-snap-align` on at least one axis).  Nested snap containers form
/// independent entries — snap areas inside an inner container are not counted
/// toward any outer container.
///
/// Deduplicates by `NodeId`: the layout engine may emit multiple boxes for the
/// same element (e.g. a `Block` wrapper + an `InlineRun` sub-box).  Only the
/// first box seen per node is recorded as a snap area.
///
/// `BoxKind::Skip` subtrees are omitted entirely.
pub fn collect_snap_containers(root: &LayoutBox) -> Vec<SnapContainer> {
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut seen_areas: std::collections::HashSet<lumen_dom::NodeId> =
        std::collections::HashSet::new();
    collect_snap_rec(root, &mut stack, &mut out, &mut seen_areas);
    out
}

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion — same "ancestor stack" class as
/// `container_anchor.rs::apply_anchor_positions_rec`: each popped frame
/// carries `depth` (the length `container_stack` had when this node's
/// *parent* pushed it), and `container_stack.truncate(depth)` restores
/// exactly that state before the node is processed — reproducing the old
/// "push before children, pop after children" discipline regardless of the
/// LIFO stack's (sibling-reversed, but parent-before-child) visiting order.
/// Children of a non-container node reuse the current depth (no push to
/// undo); a container's children get `container_stack.len()` *after* its own
/// push.
fn collect_snap_rec(
    root: &LayoutBox,
    container_stack: &mut Vec<usize>,
    out: &mut Vec<SnapContainer>,
    seen_areas: &mut std::collections::HashSet<lumen_dom::NodeId>,
) {
    use box_tree::BoxKind;
    use style::ScrollSnapAxis;

    let mut stack: Vec<(&LayoutBox, usize)> = vec![(root, container_stack.len())];
    while let Some((b, depth)) = stack.pop() {
    container_stack.truncate(depth);

    if matches!(b.kind, BoxKind::Skip) {
        continue;
    }

    let is_container = b.style.scroll_snap_type.axis != ScrollSnapAxis::None;

    if is_container {
        let idx = out.len();
        out.push(SnapContainer {
            node: b.node,
            snap_type: b.style.scroll_snap_type,
            rect: b.rect,
            scroll_padding_top: b.style.scroll_padding_top,
            scroll_padding_right: b.style.scroll_padding_right,
            scroll_padding_bottom: b.style.scroll_padding_bottom,
            scroll_padding_left: b.style.scroll_padding_left,
            points: Vec::new(),
        });
        container_stack.push(idx);
        let child_depth = container_stack.len();
        for child in b.children.iter().rev() {
            stack.push((child, child_depth));
        }
        continue;
    }

    // Check if this element is a snap area for the nearest ancestor container.
    if let Some(&cidx) = container_stack.last() {
        let align = b.style.scroll_snap_align;
        let cr = out[cidx].rect;
        // scroll-margin expands the snap area; scroll-padding shrinks the snap port.
        let snap_x = snap_offset_x(
            align.inline,
            b.rect,
            cr,
            b.style.scroll_margin_left,
            b.style.scroll_margin_right,
            out[cidx].scroll_padding_left,
            out[cidx].scroll_padding_right,
        );
        let snap_y = snap_offset_y(
            align.block,
            b.rect,
            cr,
            b.style.scroll_margin_top,
            b.style.scroll_margin_bottom,
            out[cidx].scroll_padding_top,
            out[cidx].scroll_padding_bottom,
        );
        if (snap_x.is_some() || snap_y.is_some()) && seen_areas.insert(b.node) {
            let stop_always =
                b.style.scroll_snap_stop == style::ScrollSnapStop::Always;
            out[cidx].points.push(SnapPoint {
                node: b.node,
                snap_x,
                snap_y,
                stop_always,
            });
        }
    }

    for child in b.children.iter().rev() {
        stack.push((child, depth));
    }
    }
}

/// Compute the x-axis snap offset for `align` keyword relative to `container`.
///
/// `margin_left`/`margin_right` expand the snap area (CSS `scroll-margin`).
/// `padding_left`/`padding_right` shrink the snap port (CSS `scroll-padding`).
///
/// Returns the container scroll-x value at which the (margin-expanded) snap
/// area edge aligns with the (padding-shrunk) snap port edge per CSS Scroll
/// Snap L1 §6.1 and §6.3.
pub(crate) fn snap_offset_x(
    align: style::ScrollSnapAlignKeyword,
    area: lumen_core::geom::Rect,
    container: lumen_core::geom::Rect,
    margin_left: f32,
    margin_right: f32,
    padding_left: f32,
    padding_right: f32,
) -> Option<f32> {
    use style::ScrollSnapAlignKeyword;
    // Content offset of the area's origin within the container's content space.
    let ax = area.x - container.x;
    match align {
        ScrollSnapAlignKeyword::None => None,
        // Align expanded-area start with port start: scroll_x = area_left − port_left
        ScrollSnapAlignKeyword::Start => Some(ax - margin_left - padding_left),
        // Align expanded-area end with port end: scroll_x = area_right − port_right
        ScrollSnapAlignKeyword::End => {
            Some(ax + area.width + margin_right - container.width + padding_right)
        }
        // Align expanded-area center with port center.
        ScrollSnapAlignKeyword::Center => Some(
            ax + area.width * 0.5 - container.width * 0.5
                + (margin_right - margin_left) * 0.5
                + (padding_right - padding_left) * 0.5,
        ),
    }
}

/// Compute the y-axis snap offset for `align` keyword relative to `container`.
///
/// `margin_top`/`margin_bottom` expand the snap area (CSS `scroll-margin`).
/// `padding_top`/`padding_bottom` shrink the snap port (CSS `scroll-padding`).
///
/// Returns the container scroll-y value at which the (margin-expanded) snap
/// area edge aligns with the (padding-shrunk) snap port edge per CSS Scroll
/// Snap L1 §6.1 and §6.3.
pub(crate) fn snap_offset_y(
    align: style::ScrollSnapAlignKeyword,
    area: lumen_core::geom::Rect,
    container: lumen_core::geom::Rect,
    margin_top: f32,
    margin_bottom: f32,
    padding_top: f32,
    padding_bottom: f32,
) -> Option<f32> {
    use style::ScrollSnapAlignKeyword;
    let ay = area.y - container.y;
    match align {
        ScrollSnapAlignKeyword::None => None,
        ScrollSnapAlignKeyword::Start => Some(ay - margin_top - padding_top),
        ScrollSnapAlignKeyword::End => {
            Some(ay + area.height + margin_bottom - container.height + padding_bottom)
        }
        ScrollSnapAlignKeyword::Center => Some(
            ay + area.height * 0.5 - container.height * 0.5
                + (margin_bottom - margin_top) * 0.5
                + (padding_bottom - padding_top) * 0.5,
        ),
    }
}

/// Find the nearest snap target for a scroll gesture.
///
/// Given the container's active snap type, the current scroll position
/// `current_scroll`, and the intended post-scroll position `target_scroll`,
/// returns the adjusted scroll offset `(snap_x, snap_y)` that the container
/// should actually land on, or `None` if no snap applies.
///
/// # Axes
///
/// Only the container's declared axis/axes are considered:
/// - `X` / `Inline` → x axis only; y component is passed through unchanged.
/// - `Y` / `Block`  → y axis only; x component is passed through unchanged.
/// - `Both`         → both axes must snap independently.
///
/// # Strictness
///
/// - `Mandatory` — always snaps to the nearest point, regardless of distance.
/// - `Proximity` — snaps only if the nearest point is within 50 % of the scroll
///   port on the relevant axis (browser-defined threshold per the spec note).
///
/// # Integration
///
/// Call this from the shell scroll handler after computing `target_scroll` from
/// the user gesture.  If `Some((sx, sy))` is returned, animate/clamp to that
/// position instead of `target_scroll`.
pub fn find_snap_target(
    container: &SnapContainer,
    current_scroll: (f32, f32),
    target_scroll: (f32, f32),
) -> Option<(f32, f32)> {
    use style::{ScrollSnapAxis, ScrollSnapStrictness};

    if container.points.is_empty() {
        return None;
    }

    let axis = container.snap_type.axis;
    let strictness = container.snap_type.strictness;

    // Proximity threshold: 50% of the effective snap port (after scroll-padding).
    let port_w = (container.rect.width
        - container.scroll_padding_left
        - container.scroll_padding_right)
        .max(0.0);
    let port_h = (container.rect.height
        - container.scroll_padding_top
        - container.scroll_padding_bottom)
        .max(0.0);
    let prox_x = port_w * 0.5;
    let prox_y = port_h * 0.5;

    let snaps_x = matches!(axis, ScrollSnapAxis::X | ScrollSnapAxis::Inline | ScrollSnapAxis::Both);
    let snaps_y = matches!(axis, ScrollSnapAxis::Y | ScrollSnapAxis::Block | ScrollSnapAxis::Both);

    let mut best_dist = f32::INFINITY;
    let mut best: Option<(f32, f32)> = None;

    for pt in &container.points {
        // Resolve snap coordinates: fall back to target on axes we don't snap.
        let sx = if snaps_x {
            pt.snap_x.unwrap_or(target_scroll.0)
        } else {
            target_scroll.0
        };
        let sy = if snaps_y {
            pt.snap_y.unwrap_or(target_scroll.1)
        } else {
            target_scroll.1
        };

        let dx = sx - target_scroll.0;
        let dy = sy - target_scroll.1;

        // Proximity filter: skip if beyond threshold.
        if matches!(strictness, ScrollSnapStrictness::Proximity) {
            if snaps_x && dx.abs() > prox_x {
                continue;
            }
            if snaps_y && dy.abs() > prox_y {
                continue;
            }
        }

        // `scroll-snap-stop: always` forces a stop at this point when scrolling
        // past it from `current_scroll`.  Model this as a hard barrier: if
        // `current_scroll` is on the near side and `target_scroll` overshoots,
        // this becomes the mandatory snap target.
        if pt.stop_always {
            let crosses_x = snaps_x && {
                let cs = current_scroll.0;
                let ts = target_scroll.0;
                (cs <= sx && sx <= ts) || (ts <= sx && sx <= cs)
            };
            let crosses_y = snaps_y && {
                let cs = current_scroll.1;
                let ts = target_scroll.1;
                (cs <= sy && sy <= ts) || (ts <= sy && sy <= cs)
            };
            if (!snaps_x || crosses_x) && (!snaps_y || crosses_y) {
                return Some((sx, sy));
            }
        }

        let dist = dx * dx + dy * dy;
        if dist < best_dist {
            best_dist = dist;
            best = Some((sx, sy));
        }
    }

    best
}

/// The snap areas a container is currently snapped to, one per axis.
///
/// CSS Scroll Snap L2 §`snapchanging`/`snapchanged`: the snap events expose
/// `snapTargetBlock` / `snapTargetInline` — the elements snapped on the block
/// and inline axes respectively. Either may be `None` when no area is snapped
/// on that axis (e.g. the container only snaps on one axis, or no area aligns).
///
/// `block` corresponds to the y axis and `inline` to the x axis under the
/// default `horizontal-tb` writing mode (matching [`snap_offset_x`] /
/// [`snap_offset_y`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnapTargets {
    /// DOM node snapped on the block axis (y under `horizontal-tb`), or `None`.
    pub block: Option<lumen_dom::NodeId>,
    /// DOM node snapped on the inline axis (x under `horizontal-tb`), or `None`.
    pub inline: Option<lumen_dom::NodeId>,
}

/// Determine which snap areas a container is snapped to at scroll offset `scroll`.
///
/// For each axis the container actually snaps on (per its `scroll-snap-type`),
/// picks the snap area whose required offset on that axis is closest to the
/// container's current scroll position. Returns the node ids as [`SnapTargets`].
///
/// Returns the default (both `None`) when the container has no snap areas.
///
/// # Integration
///
/// Shell scroll handler: after [`find_snap_target`] resolves a new scroll
/// offset, call this to learn the snapped elements, then dispatch the snap
/// events via [`crate`]-external JS bindings — fire `snapchanging` while the
/// gesture is in flight and `snapchanged` once the scroll settles, passing
/// `block`/`inline` node ids as `snapTargetBlock` / `snapTargetInline`
/// (`QuickJsRuntime::fire_snap_changing` / `fire_snap_changed`).
pub fn find_snapped_nodes(container: &SnapContainer, scroll: (f32, f32)) -> SnapTargets {
    use style::ScrollSnapAxis;

    if container.points.is_empty() {
        return SnapTargets::default();
    }

    let axis = container.snap_type.axis;
    let snaps_x = matches!(
        axis,
        ScrollSnapAxis::X | ScrollSnapAxis::Inline | ScrollSnapAxis::Both
    );
    let snaps_y = matches!(
        axis,
        ScrollSnapAxis::Y | ScrollSnapAxis::Block | ScrollSnapAxis::Both
    );

    let mut inline = None;
    let mut block = None;
    let mut best_inline = f32::INFINITY;
    let mut best_block = f32::INFINITY;

    for pt in &container.points {
        if snaps_x && let Some(sx) = pt.snap_x {
            let d = (sx - scroll.0).abs();
            if d < best_inline {
                best_inline = d;
                inline = Some(pt.node);
            }
        }
        if snaps_y && let Some(sy) = pt.snap_y {
            let d = (sy - scroll.1).abs();
            if d < best_block {
                best_block = d;
                block = Some(pt.node);
            }
        }
    }

    SnapTargets { block, inline }
}
