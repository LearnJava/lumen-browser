//! Липкое позиционирование (`position: sticky`) и CSS Scroll Snap L1:
//! снимки липких боксов и снап-контейнеров после раскладки, расчёт сдвига
//! и снап-цели при прокрутке. Вынесено из `lib.rs` без изменений.

use crate::{box_tree, style, LayoutBox};

// ─── Sticky-position algorithm ────────────────────────────────────────────────
// CSS: position: sticky — insets читаются из ComputedStyle (top/right/bottom/left),
//                      scroll_x/scroll_y приходят из shell scroll state.

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

/// Static geometry of a `position: sticky` box that lives inside a scroll
/// container — everything `getBoundingClientRect` needs to compute the stuck
/// offset *at read time*, without re-walking the tree on every scroll.
///
/// The container's current scroll is deliberately *not* stored here: it changes
/// on every wheel/`scrollTop` write (which publishes `scroll_states` but does not
/// relayout), so the reader looks it up from `scroll_states` and passes it to
/// [`StickyGeom::correction`]. Only page-level sticky (no scroll container on the
/// ancestor chain) is absent from [`collect_sticky_geoms`] — its geometry depends
/// on the page scroll, a separate gap (BUG-1166).
#[derive(Debug, Clone, Copy)]
pub struct StickyGeom {
    /// Resolved `top`/`bottom`/`left`/`right` insets in px; `None` = `auto`.
    pub top: Option<f32>,
    pub bottom: Option<f32>,
    pub left: Option<f32>,
    pub right: Option<f32>,
    /// Node index of the nearest ancestor scroll container.
    pub container: u32,
    /// That container's scrollport `[x, y, width, height]` in document coords.
    pub scrollport: [f32; 4],
    /// The box's containing block — nearest block/flow-root ancestor's
    /// border-box `[x, y, width, height]` in document coords (BUG-1825: the
    /// stuck box is clamped inside it, CSS Position L3 §2.2).
    pub containing: [f32; 4],
    /// `true` when the containing block sits at-or-above the nearest scroll
    /// container (it *frames* the scroller instead of scrolling with the
    /// content): its clamp range then moves with the scroller's own scroll,
    /// which [`StickyGeom::correction`] adds. `false` — the cb is inside the
    /// scrolled content and the range is doc-static.
    pub containing_frames_scroller: bool,
}

impl StickyGeom {
    /// The stuck shift `[cdx, cdy]` in CSS px to add on top of the ancestor-scroll
    /// shift `getBoundingClientRect` already applies (it subtracts the container
    /// scrolls). `[0, 0]` = not stuck. Mirrors paint's `sticky_dy`/`sticky_dx`
    /// (`display_list/box_layer.rs`) and `renderer/sticky_geom.rs`.
    pub fn correction(&self, flow: [f32; 4], scroll_x: f32, scroll_y: f32) -> [f32; 2] {
        // A cb that frames the scroller does not scroll with the content: its
        // clamp range in the shifted-box's coordinate space moves by the
        // scroller's own scroll. A cb inside the content scrolls identically
        // to the box, so its range stays doc-static (BUG-1825).
        let (sx, sy) = if self.containing_frames_scroller { (scroll_x, scroll_y) } else { (0.0, 0.0) };
        let (cdx, cdy) = sticky_correction(
            lumen_core::geom::Rect::new(flow[0], flow[1], flow[2], flow[3]),
            self.top,
            self.bottom,
            self.left,
            self.right,
            scroll_x,
            scroll_y,
            lumen_core::geom::Rect::new(
                self.scrollport[0],
                self.scrollport[1],
                self.scrollport[2],
                self.scrollport[3],
            ),
            lumen_core::geom::Rect::new(
                self.containing[0] + sx,
                self.containing[1] + sy,
                self.containing[2],
                self.containing[3],
            ),
        );
        [cdx, cdy]
    }
}

/// CSS Position L3 §2.2 — the sticky-constraint-rectangle edge clamp shared by
/// the geometry reader and all paint paths (BUG-1825): a stuck box may not
/// leave its containing block. `pos` is the inset-corrected stuck position,
/// `size` the box extent on the axis, `cb_pos`/`cb_size` the containing
/// block's start edge and extent — every operand in one shared space (each
/// caller passes them in the space its own inset math uses). Clamps `pos` into
/// `[cb_pos, cb_pos + cb_size - size]`; the start edge wins when the
/// containing block is shorter than the element, matching
/// `compute_sticky_offset`'s "top wins" convention.
pub fn clamp_stuck_to_containing(pos: f32, size: f32, cb_pos: f32, cb_size: f32) -> f32 {
    pos.min(cb_pos + cb_size - size).max(cb_pos)
}

/// CSS Positioning L3 §6.3 — the extra shift a stuck `position: sticky` box gets
/// beyond `-scroll` (the plain ancestor-scroll translation). `flow` is the box's
/// document-space flow rect, `bound` the nearest scroll container's scrollport,
/// `scroll_x`/`scroll_y` that container's own scroll offset, `containing` the
/// document-space border-box of the box's containing block (BUG-1825: without
/// the clamp a sticky box escaped its container's far edge). Returns `(cdx, cdy)`
/// in CSS px; `(0, 0)` when not stuck. Exact mirror of paint's `sticky_dy`/`sticky_dx`.
#[allow(clippy::too_many_arguments)] // mirror of the two paint helpers, one fn for both axes
pub fn sticky_correction(
    flow: lumen_core::geom::Rect,
    top: Option<f32>,
    bottom: Option<f32>,
    left: Option<f32>,
    right: Option<f32>,
    scroll_x: f32,
    scroll_y: f32,
    bound: lumen_core::geom::Rect,
    containing: lumen_core::geom::Rect,
) -> (f32, f32) {
    let mut cdy = 0.0;
    let mut stuck_y = false;
    if let Some(t) = top {
        let screen_y = flow.y - scroll_y;
        let min_y = bound.y + t;
        if screen_y < min_y {
            cdy += min_y - screen_y;
            stuck_y = true;
        }
    }
    if let Some(b) = bottom {
        let max_screen_y = bound.y + bound.height - b - flow.height;
        let actual_screen_y = flow.y - scroll_y;
        if actual_screen_y > max_screen_y {
            cdy -= actual_screen_y - max_screen_y;
            stuck_y = true;
        }
    }
    // BUG-1825: clamp only a genuinely stuck axis — an unstuck box scrolls with
    // the content and may legitimately sit outside the containing block's range.
    if stuck_y {
        let stuck = flow.y + cdy;
        let clamped = clamp_stuck_to_containing(stuck, flow.height, containing.y, containing.height);
        cdy += clamped - stuck;
    }
    let mut cdx = 0.0;
    let mut stuck_x = false;
    if let Some(l) = left {
        let screen_x = flow.x - scroll_x;
        let min_x = bound.x + l;
        if screen_x < min_x {
            cdx += min_x - screen_x;
            stuck_x = true;
        }
    }
    if let Some(r) = right {
        let max_screen_x = bound.x + bound.width - r - flow.width;
        let actual_screen_x = flow.x - scroll_x;
        if actual_screen_x > max_screen_x {
            cdx -= actual_screen_x - max_screen_x;
            stuck_x = true;
        }
    }
    if stuck_x {
        let stuck = flow.x + cdx;
        let clamped = clamp_stuck_to_containing(stuck, flow.width, containing.x, containing.width);
        cdx += clamped - stuck;
    }
    (cdx, cdy)
}

/// Collect the [`StickyGeom`] of every sticky box whose nearest scroll container
/// is an ancestor (the "baked" case paint emits as `PushClipRect`→`PushTransform`).
/// Page-level sticky boxes (no scroll container) are omitted — their stuck offset
/// depends on the page scroll, tracked separately (BUG-1166).
pub fn collect_sticky_geoms(root: &LayoutBox) -> std::collections::HashMap<u32, StickyGeom> {
    use box_tree::BoxKind;
    use style::{Overflow, Position};

    let mut out = std::collections::HashMap::new();
    // (box, nearest ancestor scroll container: (scrollport, node index),
    //  containing block: (nearest block/flow-root ancestor border-box — same
    //  rule as `collect_sticky_boxes` —, whether the cb was established
    //  at-or-below the nearest scroller))
    type ScrollContainer = Option<(lumen_core::geom::Rect, u32)>;
    type ContainingBlock = (lumen_core::geom::Rect, bool);
    let mut stack: Vec<(&LayoutBox, ScrollContainer, ContainingBlock)> =
        vec![(root, None, (root.rect, false))];
    while let Some((b, nearest, (containing, cb_in_content))) = stack.pop() {
        let is_sc = matches!(
            b.style.overflow_x,
            Overflow::Scroll | Overflow::Auto | Overflow::Hidden
        ) || matches!(
            b.style.overflow_y,
            Overflow::Scroll | Overflow::Auto | Overflow::Hidden
        );
        let next = if is_sc {
            Some((sticky_scrollport(b), b.node.index() as u32))
        } else {
            nearest
        };
        if matches!(b.style.position, Position::Sticky) && let Some((bound, container)) = next {
            let em = b.style.font_size;
            let vp = lumen_core::geom::Size::new(0.0, 0.0);
            let inset = |len: &style::LengthOrAuto, basis: f32| len.resolve(em, basis, vp);
            out.entry(b.node.index() as u32).or_insert(StickyGeom {
                top: inset(&b.style.top, bound.height),
                bottom: inset(&b.style.bottom, bound.height),
                left: inset(&b.style.left, bound.width),
                right: inset(&b.style.right, bound.width),
                container,
                scrollport: [bound.x, bound.y, bound.width, bound.height],
                containing: [containing.x, containing.y, containing.width, containing.height],
                containing_frames_scroller: !cb_in_content,
            });
        }
        // Blocks and flow roots establish a new sticky-containment boundary.
        // A cb established at-or-below the nearest scroller scrolls with the
        // content (`cb_in_content`); one established by the scroller itself or
        // above it frames the scroller.
        let next_cb = if matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot) {
            (b.rect, nearest.is_some() && !is_sc)
        } else {
            (containing, cb_in_content)
        };
        for child in b.children.iter().rev() {
            stack.push((child, next, next_cb));
        }
    }
    out
}

/// The scrollport a `position: sticky` box is clamped against — the scroll
/// container's padding box on the axes it clips, a `BIG` sentinel on the others
/// (mirror of paint's `overflow_clip_rect`), so a single-axis scroller never
/// spuriously clamps the free axis.
fn sticky_scrollport(b: &LayoutBox) -> lumen_core::geom::Rect {
    use style::Overflow;
    const BIG: f32 = 1_000_000.0;
    let s = &b.style;
    let (bl, bt, br, bb) = (s.border_left_width, s.border_top_width, s.border_right_width, s.border_bottom_width);
    let clip_x = !matches!(s.overflow_x, Overflow::Visible);
    let clip_y = !matches!(s.overflow_y, Overflow::Visible);
    lumen_core::geom::Rect::new(
        if clip_x { b.rect.x + bl } else { -BIG },
        if clip_y { b.rect.y + bt } else { -BIG },
        if clip_x { (b.rect.width - bl - br).max(0.0) } else { 2.0 * BIG },
        if clip_y { (b.rect.height - bt - bb).max(0.0) } else { 2.0 * BIG },
    )
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
