//! CSS Scroll-Driven Animations Level 1 — algorithm stubs.
//!
//! Provides [`ScrollTimeline`] and [`ViewTimeline`] progress resolvers for the
//! layout engine.  P4 wires `scroll-timeline-name`, `scroll-timeline-axis`,
//! `view-timeline-name`, `view-timeline-axis`, and `animation-timeline` on top
//! of these building blocks.
//!
//! # Progress semantics
//! * [`resolve_scroll_progress`] — scroll fraction: 0.0 = scroll start, 1.0 = max scroll.
//! * [`resolve_view_progress`] — "cover" range: 0.0 when element enters viewport from below,
//!   1.0 when element has fully left the viewport above.
//!
//! # CSS reference
//! <https://www.w3.org/TR/scroll-animations-1/>

use lumen_dom::NodeId;

use crate::box_tree::LayoutBox;

// CSS: scroll-timeline-axis, view-timeline-axis, animation-timeline
/// Selects which scroll axis drives a timeline.
///
/// Maps to the `scroll-timeline-axis` / `view-timeline-axis` CSS descriptor.
/// `Block` and `Inline` are writing-mode–relative; `X` and `Y` are physical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollAxis {
    /// Block axis (vertical in horizontal writing modes).  Default.
    #[default]
    Block,
    /// Inline axis (horizontal in horizontal writing modes).
    Inline,
    /// Physical horizontal axis.
    X,
    /// Physical vertical axis.
    Y,
}

/// Viewport dimensions used during progress resolution.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    /// Viewport width in CSS px.
    pub width: f32,
    /// Viewport height in CSS px.
    pub height: f32,
}

// CSS: animation-timeline, scroll-timeline-name, scroll-timeline-axis
/// Scroll progress timeline (CSS `scroll()` function / named `scroll-timeline`).
///
/// Tracks the scroll fraction of a scroll container.  When `element` is
/// `None` the root viewport is used.
#[derive(Debug, Clone)]
pub struct ScrollTimeline {
    /// Scroll container node.  `None` = root viewport.
    pub element: Option<NodeId>,
    /// Which axis drives this timeline.
    pub axis: ScrollAxis,
}

// CSS: animation-timeline, view-timeline-name, view-timeline-axis
/// View progress timeline (CSS `view()` function / named `view-timeline`).
///
/// Tracks the visibility fraction of `element` inside its nearest scroll
/// container, using the "cover" range by default.
#[derive(Debug, Clone)]
pub struct ViewTimeline {
    /// Subject element whose visibility is tracked.
    pub element: NodeId,
    /// Which axis drives this timeline.
    pub axis: ScrollAxis,
}

// CSS: scroll-timeline-name
/// Named scroll timeline resolved from the layout tree.
///
/// Collected by [`collect_named_scroll_timelines`]; P4 matches names against
/// `animation-timeline` values.
#[derive(Debug, Clone)]
pub struct NamedScrollTimeline {
    /// Scroll container that defines this timeline.
    pub container: NodeId,
    /// Value of `scroll-timeline-name`.
    pub name: String,
    /// Value of `scroll-timeline-axis`.
    pub axis: ScrollAxis,
}

// CSS: view-timeline-name
/// Named view timeline resolved from the layout tree.
///
/// Collected by [`collect_named_view_timelines`]; P4 matches names against
/// `animation-timeline` values.
#[derive(Debug, Clone)]
pub struct NamedViewTimeline {
    /// Subject element that defines this timeline.
    pub subject: NodeId,
    /// Value of `view-timeline-name`.
    pub name: String,
    /// Value of `view-timeline-axis`.
    pub axis: ScrollAxis,
}

// ─── helpers ────────────────────────────────────────────────────────────────

/// Walk the layout tree and return the first box whose node matches `id`.
fn find_box(root: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
    if root.node == id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, id) {
            return Some(found);
        }
    }
    None
}

/// Compute the total content size of `node` by walking its subtree.
///
/// Returns `(content_width, content_height)` in CSS px — the bounding box of
/// all descendant rects unioned together, relative to `node.rect.origin`.
fn content_size(node: &LayoutBox) -> (f32, f32) {
    let origin_x = node.rect.x;
    let origin_y = node.rect.y;

    let mut max_x: f32 = node.rect.x + node.rect.width;
    let mut max_y: f32 = node.rect.y + node.rect.height;

    fn walk(lb: &LayoutBox, max_x: &mut f32, max_y: &mut f32) {
        let right = lb.rect.x + lb.rect.width;
        let bottom = lb.rect.y + lb.rect.height;
        if right > *max_x {
            *max_x = right;
        }
        if bottom > *max_y {
            *max_y = bottom;
        }
        for child in &lb.children {
            walk(child, max_x, max_y);
        }
    }

    for child in &node.children {
        walk(child, &mut max_x, &mut max_y);
    }

    (max_x - origin_x, max_y - origin_y)
}

// ─── public API ─────────────────────────────────────────────────────────────

/// Resolve the scroll progress fraction `[0.0, 1.0]` for a [`ScrollTimeline`].
///
/// `scroll_x` / `scroll_y` are the current scroll offsets of the root
/// viewport in CSS px (from `LayoutBox::scroll_x/scroll_y`).
///
/// When `timeline.element` is `Some(id)` the function locates that node's box
/// and uses its own `scroll_x/scroll_y` fields plus its content size.
///
/// Returns `0.0` when the container has no overflow to scroll.
pub fn resolve_scroll_progress(
    timeline: &ScrollTimeline,
    root: &LayoutBox,
    scroll_x: f32,
    scroll_y: f32,
    vp: Viewport,
) -> f32 {
    match timeline.element {
        None => {
            // Root viewport scroll.
            let (content_w, content_h) = content_size(root);
            match timeline.axis {
                ScrollAxis::Block | ScrollAxis::Y => {
                    let max_scroll = (content_h - vp.height).max(0.0);
                    if max_scroll <= 0.0 {
                        return 0.0;
                    }
                    (scroll_y / max_scroll).clamp(0.0, 1.0)
                }
                ScrollAxis::Inline | ScrollAxis::X => {
                    let max_scroll = (content_w - vp.width).max(0.0);
                    if max_scroll <= 0.0 {
                        return 0.0;
                    }
                    (scroll_x / max_scroll).clamp(0.0, 1.0)
                }
            }
        }
        Some(id) => {
            let Some(container) = find_box(root, id) else {
                return 0.0;
            };
            let (content_w, content_h) = content_size(container);
            match timeline.axis {
                ScrollAxis::Block | ScrollAxis::Y => {
                    let max_scroll = (content_h - container.rect.height).max(0.0);
                    if max_scroll <= 0.0 {
                        return 0.0;
                    }
                    (container.scroll_y / max_scroll).clamp(0.0, 1.0)
                }
                ScrollAxis::Inline | ScrollAxis::X => {
                    let max_scroll = (content_w - container.rect.width).max(0.0);
                    if max_scroll <= 0.0 {
                        return 0.0;
                    }
                    (container.scroll_x / max_scroll).clamp(0.0, 1.0)
                }
            }
        }
    }
}

/// Resolve the view progress fraction `[0.0, 1.0]` for a [`ViewTimeline`].
///
/// Uses the "cover" range:
/// * `0.0` — subject element's leading edge enters the viewport from below
///   (or from the right for inline axis).
/// * `1.0` — subject element's trailing edge has left the viewport above
///   (or to the left for inline axis).
///
/// `scroll_y` / `scroll_x` are the current root-viewport scroll offsets.
/// Returns `0.0` when the subject element is not found in the tree, or when
/// the view range collapses to zero.
pub fn resolve_view_progress(
    timeline: &ViewTimeline,
    root: &LayoutBox,
    scroll_y: f32,
    scroll_x: f32,
    vp: Viewport,
) -> f32 {
    let Some(subject) = find_box(root, timeline.element) else {
        return 0.0;
    };

    match timeline.axis {
        ScrollAxis::Block | ScrollAxis::Y => {
            let elem_top = subject.rect.y;
            let elem_bottom = subject.rect.y + subject.rect.height;
            // Range: starts when top edge reaches the bottom of viewport,
            // ends when bottom edge reaches the top of the viewport.
            let range_start = elem_top - vp.height;
            let range_end = elem_bottom;
            let range = range_end - range_start;
            if range <= 0.0 {
                return 0.0;
            }
            ((scroll_y - range_start) / range).clamp(0.0, 1.0)
        }
        ScrollAxis::Inline | ScrollAxis::X => {
            let elem_left = subject.rect.x;
            let elem_right = subject.rect.x + subject.rect.width;
            let range_start = elem_left - vp.width;
            let range_end = elem_right;
            let range = range_end - range_start;
            if range <= 0.0 {
                return 0.0;
            }
            ((scroll_x - range_start) / range).clamp(0.0, 1.0)
        }
    }
}

/// Collect all named scroll timelines defined in the layout tree.
///
/// Walks the layout tree and returns one [`NamedScrollTimeline`] for each box
/// whose `ComputedStyle::scroll_timeline_name` is `Some`.
///
/// # CSS: scroll-timeline-name, scroll-timeline-axis
pub fn collect_named_scroll_timelines(root: &LayoutBox) -> Vec<NamedScrollTimeline> {
    let mut out = Vec::new();
    collect_named_scroll_timelines_rec(root, &mut out);
    out
}

fn collect_named_scroll_timelines_rec(lb: &LayoutBox, out: &mut Vec<NamedScrollTimeline>) {
    if let Some(ref name) = lb.style.scroll_timeline_name {
        out.push(NamedScrollTimeline {
            container: lb.node,
            name: name.clone(),
            axis: lb.style.scroll_timeline_axis,
        });
    }
    for child in &lb.children {
        collect_named_scroll_timelines_rec(child, out);
    }
}

/// Collect all named view timelines defined in the layout tree.
///
/// Walks the layout tree and returns one [`NamedViewTimeline`] for each box
/// whose `ComputedStyle::view_timeline_name` is `Some`.
///
/// # CSS: view-timeline-name, view-timeline-axis
pub fn collect_named_view_timelines(root: &LayoutBox) -> Vec<NamedViewTimeline> {
    let mut out = Vec::new();
    collect_named_view_timelines_rec(root, &mut out);
    out
}

fn collect_named_view_timelines_rec(lb: &LayoutBox, out: &mut Vec<NamedViewTimeline>) {
    if let Some(ref name) = lb.style.view_timeline_name {
        out.push(NamedViewTimeline {
            subject: lb.node,
            name: name.clone(),
            axis: lb.style.view_timeline_axis,
        });
    }
    for child in &lb.children {
        collect_named_view_timelines_rec(child, out);
    }
}

// ─── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::box_tree::{BoxKind, LayoutBox};
    use crate::style::ComputedStyle;
    use lumen_core::geom::Rect;
    use lumen_dom::NodeId;

    fn node(id: u32) -> NodeId {
        NodeId::from_index(id as usize)
    }

    fn make_box(id: u32, x: f32, y: f32, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node: node(id),
            rect: Rect { x, y, width: w, height: h },
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }

    fn make_box_scroll(id: u32, x: f32, y: f32, w: f32, h: f32, sx: f32, sy: f32) -> LayoutBox {
        let mut lb = make_box(id, x, y, w, h);
        lb.scroll_x = sx;
        lb.scroll_y = sy;
        lb
    }

    fn vp(w: f32, h: f32) -> Viewport {
        Viewport { width: w, height: h }
    }

    // ── ScrollTimeline (root viewport) ──────────────────────────────────────

    #[test]
    fn scroll_root_block_zero() {
        // At scroll 0 → progress 0.0.
        let root = {
            let mut r = make_box(1, 0.0, 0.0, 1024.0, 2000.0);
            r.children.push(make_box(2, 0.0, 0.0, 1024.0, 2000.0));
            r
        };
        let tl = ScrollTimeline { element: None, axis: ScrollAxis::Block };
        assert_eq!(resolve_scroll_progress(&tl, &root, 0.0, 0.0, vp(1024.0, 720.0)), 0.0);
    }

    #[test]
    fn scroll_root_block_half() {
        // Content 2000px tall, viewport 720px → max_scroll = 1280px.
        // At scroll_y = 640 → progress ≈ 0.5.
        let root = {
            let mut r = make_box(1, 0.0, 0.0, 1024.0, 720.0);
            r.children.push(make_box(2, 0.0, 0.0, 1024.0, 2000.0));
            r
        };
        let tl = ScrollTimeline { element: None, axis: ScrollAxis::Block };
        let p = resolve_scroll_progress(&tl, &root, 0.0, 640.0, vp(1024.0, 720.0));
        assert!((p - 0.5).abs() < 0.01, "expected ~0.5, got {p}");
    }

    #[test]
    fn scroll_root_block_full() {
        let root = {
            let mut r = make_box(1, 0.0, 0.0, 1024.0, 720.0);
            r.children.push(make_box(2, 0.0, 0.0, 1024.0, 2000.0));
            r
        };
        let tl = ScrollTimeline { element: None, axis: ScrollAxis::Block };
        let p = resolve_scroll_progress(&tl, &root, 0.0, 1280.0, vp(1024.0, 720.0));
        assert_eq!(p, 1.0);
    }

    #[test]
    fn scroll_root_no_overflow() {
        // Content shorter than viewport → no scrollable range → always 0.0.
        let root = make_box(1, 0.0, 0.0, 1024.0, 500.0);
        let tl = ScrollTimeline { element: None, axis: ScrollAxis::Block };
        assert_eq!(resolve_scroll_progress(&tl, &root, 0.0, 0.0, vp(1024.0, 720.0)), 0.0);
    }

    #[test]
    fn scroll_root_inline_half() {
        // Horizontal scroll: content 2048px wide, viewport 1024px → max = 1024px.
        let root = {
            let mut r = make_box(1, 0.0, 0.0, 1024.0, 720.0);
            r.children.push(make_box(2, 0.0, 0.0, 2048.0, 720.0));
            r
        };
        let tl = ScrollTimeline { element: None, axis: ScrollAxis::Inline };
        let p = resolve_scroll_progress(&tl, &root, 512.0, 0.0, vp(1024.0, 720.0));
        assert!((p - 0.5).abs() < 0.01, "expected ~0.5, got {p}");
    }

    #[test]
    fn scroll_root_x_axis_full() {
        let root = {
            let mut r = make_box(1, 0.0, 0.0, 1024.0, 720.0);
            r.children.push(make_box(2, 0.0, 0.0, 2048.0, 720.0));
            r
        };
        let tl = ScrollTimeline { element: None, axis: ScrollAxis::X };
        let p = resolve_scroll_progress(&tl, &root, 1024.0, 0.0, vp(1024.0, 720.0));
        assert_eq!(p, 1.0);
    }

    #[test]
    fn scroll_element_specific() {
        // Container node 2 has its own scroll_y and content taller than itself.
        let root = {
            let mut r = make_box(1, 0.0, 0.0, 1024.0, 720.0);
            let mut container = make_box_scroll(2, 100.0, 100.0, 400.0, 300.0, 0.0, 150.0);
            container.children.push(make_box(3, 100.0, 100.0, 400.0, 700.0));
            r.children.push(container);
            r
        };
        // container height = 300, content_h = 700 → max_scroll = 400.
        // scroll_y of container = 150 → progress = 150/400 = 0.375.
        let tl = ScrollTimeline { element: Some(node(2)), axis: ScrollAxis::Block };
        let p = resolve_scroll_progress(&tl, &root, 0.0, 0.0, vp(1024.0, 720.0));
        assert!((p - 0.375).abs() < 0.01, "expected ~0.375, got {p}");
    }

    #[test]
    fn scroll_element_not_found() {
        let root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        let tl = ScrollTimeline { element: Some(node(99)), axis: ScrollAxis::Block };
        assert_eq!(resolve_scroll_progress(&tl, &root, 0.0, 0.0, vp(1024.0, 720.0)), 0.0);
    }

    // ── ViewTimeline ────────────────────────────────────────────────────────

    #[test]
    fn view_below_viewport() {
        // Element is entirely below the viewport → scroll_y = 0 → progress = 0.0.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        root.children.push(make_box(2, 0.0, 1000.0, 200.0, 100.0));
        let tl = ViewTimeline { element: node(2), axis: ScrollAxis::Block };
        // range_start = 1000 - 720 = 280, range_end = 1100.
        // At scroll_y = 0: progress = (0 - 280) / 820 = negative → clamped to 0.
        let p = resolve_view_progress(&tl, &root, 0.0, 0.0, vp(1024.0, 720.0));
        assert_eq!(p, 0.0);
    }

    #[test]
    fn view_entering_viewport() {
        // Element's top just touches bottom of viewport → progress just above 0.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        root.children.push(make_box(2, 0.0, 1000.0, 200.0, 100.0));
        let tl = ViewTimeline { element: node(2), axis: ScrollAxis::Block };
        // range_start = 280, range_end = 1100, range = 820.
        // At scroll_y = 280: progress = 0 / 820 = 0.0 → just entering.
        let p = resolve_view_progress(&tl, &root, 280.0, 0.0, vp(1024.0, 720.0));
        assert!((p - 0.0).abs() < 0.001, "got {p}");
    }

    #[test]
    fn view_center() {
        // Element centered in viewport scroll progress ≈ 0.5.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        root.children.push(make_box(2, 0.0, 1000.0, 200.0, 100.0));
        let tl = ViewTimeline { element: node(2), axis: ScrollAxis::Block };
        // range_start = 280, range_end = 1100, range = 820.
        // Midpoint scroll = 280 + 820/2 = 690.
        let p = resolve_view_progress(&tl, &root, 690.0, 0.0, vp(1024.0, 720.0));
        assert!((p - 0.5).abs() < 0.01, "expected ~0.5, got {p}");
    }

    #[test]
    fn view_fully_exited_above() {
        // Element has scrolled fully past the viewport top → progress = 1.0.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        root.children.push(make_box(2, 0.0, 1000.0, 200.0, 100.0));
        let tl = ViewTimeline { element: node(2), axis: ScrollAxis::Block };
        // range_end = 1100 → at scroll_y = 1100 progress = 820/820 = 1.0.
        let p = resolve_view_progress(&tl, &root, 1100.0, 0.0, vp(1024.0, 720.0));
        assert_eq!(p, 1.0);
    }

    #[test]
    fn view_not_found() {
        let root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        let tl = ViewTimeline { element: node(42), axis: ScrollAxis::Block };
        assert_eq!(resolve_view_progress(&tl, &root, 0.0, 0.0, vp(1024.0, 720.0)), 0.0);
    }

    #[test]
    fn view_inline_axis() {
        // Horizontal view timeline.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        root.children.push(make_box(2, 2000.0, 0.0, 200.0, 100.0));
        let tl = ViewTimeline { element: node(2), axis: ScrollAxis::Inline };
        // range_start = 2000 - 1024 = 976, range_end = 2200, range = 1224.
        // At scroll_x = 976: progress = 0.0.
        let p = resolve_view_progress(&tl, &root, 0.0, 976.0, vp(1024.0, 720.0));
        assert!((p - 0.0).abs() < 0.001, "got {p}");
        // At scroll_x = 2200: progress = 1.0.
        let p = resolve_view_progress(&tl, &root, 0.0, 2200.0, vp(1024.0, 720.0));
        assert_eq!(p, 1.0);
    }

    #[test]
    fn named_timelines_no_name_returns_empty() {
        // LayoutBox with no timeline name → both collectors return empty.
        let root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        assert!(collect_named_scroll_timelines(&root).is_empty());
        assert!(collect_named_view_timelines(&root).is_empty());
    }

    #[test]
    fn collect_scroll_timelines_walks_tree() {
        // Root has a scroll-timeline-name; child does not.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 2000.0);
        root.style.scroll_timeline_name = Some("--parent".to_string());
        root.style.scroll_timeline_axis = ScrollAxis::Inline;
        let child = make_box(2, 0.0, 0.0, 800.0, 600.0);
        root.children.push(child);

        let collected = collect_named_scroll_timelines(&root);
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].name, "--parent");
        assert_eq!(collected[0].axis, ScrollAxis::Inline);
        assert_eq!(collected[0].container, node(1));
    }

    #[test]
    fn collect_scroll_timelines_nested() {
        // Both root and a nested child have scroll-timeline-names.
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 2000.0);
        root.style.scroll_timeline_name = Some("--outer".to_string());
        root.style.scroll_timeline_axis = ScrollAxis::Block;
        let mut child = make_box(2, 0.0, 0.0, 400.0, 800.0);
        child.style.scroll_timeline_name = Some("--inner".to_string());
        child.style.scroll_timeline_axis = ScrollAxis::Y;
        root.children.push(child);

        let collected = collect_named_scroll_timelines(&root);
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].name, "--outer");
        assert_eq!(collected[1].name, "--inner");
        assert_eq!(collected[1].axis, ScrollAxis::Y);
    }

    #[test]
    fn collect_view_timelines_walks_tree() {
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        let mut target = make_box(2, 0.0, 300.0, 400.0, 200.0);
        target.style.view_timeline_name = Some("--fade".to_string());
        target.style.view_timeline_axis = ScrollAxis::Block;
        root.children.push(target);

        let collected = collect_named_view_timelines(&root);
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].name, "--fade");
        assert_eq!(collected[0].subject, node(2));
    }
}
