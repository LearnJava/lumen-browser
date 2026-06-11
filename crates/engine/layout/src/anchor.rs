//! CSS Anchor Positioning L1 algorithm stub.
//!
//! Implements the two-phase algorithm for resolving anchored element positions:
//!
//! 1. **Collect phase** — walk the completed layout tree and record the
//!    border-box [`Rect`] of every element that has an `anchor-name`.
//!    See [`collect_anchors`] → [`AnchorRegistry`].
//!
//! 2. **Resolve phase** — for a positioned element with `position-anchor`,
//!    look up the anchor rect and compute:
//!    * Individual inset offsets from `anchor()` function calls in `top`/`right`/`bottom`/`left`.
//!      See [`resolve_anchor_function`].
//!    * Grid-cell position from the `inset-area` shorthand.
//!      See [`resolve_inset_area`].
//!
//! P4 wires the CSS properties:
//! - `anchor-name: --foo` → `ComputedStyle.anchor_name: Option<String>`
//! - `position-anchor: --foo` → `ComputedStyle.position_anchor: Option<String>`
//! - `inset-area: <row> <col>` → `ComputedStyle.inset_area_row` / `ComputedStyle.inset_area_col`
//! - `anchor(<anchor-element> <side>)` in inset values → call [`resolve_anchor_function`]
//! - Wire `collect_anchors(root)` before the positioned-layout pass in `box_tree.rs`.
//!
//! # CSS specification
//! <https://drafts.csswg.org/css-anchor-position-1/>

use std::collections::HashMap;

use lumen_core::geom::Rect;
use lumen_dom::NodeId;

use crate::box_tree::LayoutBox;

// ─── AnchorSide ──────────────────────────────────────────────────────────────

// CSS: anchor-name, position-anchor, anchor()
/// Which edge or point of an anchor element the `anchor()` function references.
///
/// Corresponds to `<anchor-side>` in the CSS Anchor Positioning L1 spec §3.1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnchorSide {
    /// Physical `top` edge.  CSS: `anchor(top)`.
    Top,
    /// Physical `right` edge.  CSS: `anchor(right)`.
    Right,
    /// Physical `bottom` edge.  CSS: `anchor(bottom)`.
    Bottom,
    /// Physical `left` edge.  CSS: `anchor(left)`.
    Left,
    /// Horizontal center.  CSS: `anchor(center)`.
    Center,
    /// Inline-start edge (left in LTR writing modes).  CSS: `anchor(start)`.
    Start,
    /// Inline-end edge (right in LTR writing modes).  CSS: `anchor(end)`.
    End,
    /// Percentage along the anchor's inline axis.  CSS: `anchor(25%)`.
    /// `0.0` = left/top, `100.0` = right/bottom.
    Percentage(f32),
}

// ─── InsetAreaKeyword ────────────────────────────────────────────────────────

// CSS: inset-area
/// Single-axis `inset-area` keyword, as defined in §5.2 of the spec.
///
/// The CSS `inset-area` property maps two keywords (one for each axis) to a
/// 3×3 grid relative to the default anchor element.  Each keyword names one
/// or more cells in that grid row/column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InsetAreaKeyword {
    /// No `inset-area` constraint on this axis (default).
    #[default]
    None,
    /// Positioned element occupies the "start" cell (above / left of anchor).
    Start,
    /// Positioned element is placed at the center cell (overlapping the anchor).
    Center,
    /// Positioned element occupies the "end" cell (below / right of anchor).
    End,
    /// Positioned element spans start + center cells.
    SpanStart,
    /// Positioned element spans center + end cells.
    SpanEnd,
    /// Positioned element spans all three cells (entire axis).
    SpanAll,
    /// `self-start` — aligns to the writing-mode start of the anchored element.
    SelfStart,
    /// `self-end` — aligns to the writing-mode end of the anchored element.
    SelfEnd,
}

// ─── AnchorRegistry ──────────────────────────────────────────────────────────

/// Map from CSS `anchor-name` value (e.g. `"--foo"`) to the border-box [`Rect`]
/// of the element that declared that name.
///
/// Built once after layout completes via [`collect_anchors`].  Consumed by the
/// positioned-layout second pass and by `anchor()` function resolution.
///
/// When multiple elements share the same `anchor-name` only the **last one in
/// tree order** is kept — matching the spec's "last in paint order" rule.
#[derive(Debug, Default, Clone)]
pub struct AnchorRegistry {
    /// Maps anchor-name strings (including the `--` prefix) to border-box rects.
    pub entries: HashMap<String, AnchorEntry>,
}

/// One registered anchor element.
#[derive(Debug, Clone, Copy)]
pub struct AnchorEntry {
    /// DOM node that carries `anchor-name`.
    pub node: NodeId,
    /// Border-box rectangle of that element in document coordinates (CSS px).
    /// Matches `LayoutBox::rect` semantics: (x, y) is top-left after margin,
    /// includes padding + border, excludes margin.
    pub rect: Rect,
}

impl AnchorRegistry {
    /// Look up an anchor by CSS name (e.g. `"--tooltip-anchor"`).
    ///
    /// Returns `None` when no element in the current layout tree declared this
    /// `anchor-name`.
    pub fn get(&self, name: &str) -> Option<&AnchorEntry> {
        self.entries.get(name)
    }

    /// True when the registry has no anchors.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─── collect_anchors ─────────────────────────────────────────────────────────

/// Walk the layout tree and collect all elements that have `anchor-name` set.
///
/// Call this function after `layout()` completes and before resolving positions
/// of anchored elements.  The returned [`AnchorRegistry`] is passed to
/// [`resolve_anchor_function`] and [`resolve_inset_area`].
///
/// # P4 wiring
/// ```text
/// // In box_tree.rs lay_out_positioned_children() or equivalent:
/// let anchors = collect_anchors(root);
/// for child in positioned_children {
///     if let Some(name) = &child.style.position_anchor {   // CSS: position-anchor
///         let pos = resolve_inset_area(&anchors, name, child.style.inset_area_row,
///                                      child.style.inset_area_col, containing_rect);
///         // apply pos.top / pos.left to child
///     }
/// }
/// ```
// CSS: anchor-name
pub fn collect_anchors(root: &LayoutBox) -> AnchorRegistry {
    let mut registry = AnchorRegistry::default();
    collect_anchors_rec(root, &mut registry);
    registry
}

fn collect_anchors_rec(lb: &LayoutBox, registry: &mut AnchorRegistry) {
    if let Some(name) = &lb.style.anchor_name {
        register_anchor(registry, name.to_string(), lb.node, lb.rect);
    }
    for child in &lb.children {
        collect_anchors_rec(child, registry);
    }
}

/// Register an element as a named anchor.  Called by P4's CSS wiring when it
/// encounters `anchor-name: --foo` in the ComputedStyle.
///
/// Using last-in-tree-order semantics: later registrations overwrite earlier
/// ones for the same name.
pub fn register_anchor(registry: &mut AnchorRegistry, name: String, node: NodeId, rect: Rect) {
    registry.entries.insert(name, AnchorEntry { node, rect });
}

// ─── resolve_anchor_function ─────────────────────────────────────────────────

/// Resolve an `anchor(<anchor-element> <side>)` function call to a CSS pixel value.
///
/// This is the core primitive that P4 uses when evaluating `anchor()` in
/// `top`, `right`, `bottom`, and `left` inset values of anchored elements.
///
/// - `registry` — the anchor registry built by [`collect_anchors`].
/// - `anchor_name` — the anchor-element argument (e.g. `"--tooltip-anchor"` or
///   `"implicit"` for the element's `position-anchor` default).
/// - `side` — which edge or percentage of the anchor to reference.
/// - `is_horizontal` — true when resolving `left`/`right` (anchors x-axis
///   values); false when resolving `top`/`bottom` (anchors y-axis values).
///
/// Returns `None` when the anchor is not in the registry (the `anchor()`
/// function makes the property behave as `auto`).
///
/// # Example
/// ```text
/// // Evaluate: top: anchor(--my-anchor bottom);
/// let top = resolve_anchor_function(&registry, "--my-anchor", AnchorSide::Bottom, false);
/// ```
// CSS: anchor(), position-anchor
pub fn resolve_anchor_function(
    registry: &AnchorRegistry,
    anchor_name: &str,
    side: AnchorSide,
    is_horizontal: bool,
) -> Option<f32> {
    let entry = registry.get(anchor_name)?;
    let r = entry.rect;

    // For horizontal axis (left/right insets): reference the anchor's x-extent.
    // For vertical axis (top/bottom insets): reference the anchor's y-extent.
    let value = if is_horizontal {
        match side {
            AnchorSide::Left | AnchorSide::Start => r.x,
            AnchorSide::Right | AnchorSide::End => r.x + r.width,
            AnchorSide::Center => r.x + r.width * 0.5,
            AnchorSide::Top | AnchorSide::Bottom => {
                // Cross-axis side in horizontal resolution — invalid in CSS,
                // treated as None (property becomes `auto`).
                return None;
            }
            AnchorSide::Percentage(pct) => r.x + r.width * pct / 100.0,
            // SelfStart / SelfEnd map the same as Start/End in LTR writing mode.
        }
    } else {
        match side {
            AnchorSide::Top | AnchorSide::Start => r.y,
            AnchorSide::Bottom | AnchorSide::End => r.y + r.height,
            AnchorSide::Center => r.y + r.height * 0.5,
            AnchorSide::Left | AnchorSide::Right => {
                // Cross-axis side in vertical resolution — invalid, treat as None.
                return None;
            }
            AnchorSide::Percentage(pct) => r.y + r.height * pct / 100.0,
        }
    };
    Some(value)
}

// ─── resolve_inset_area ──────────────────────────────────────────────────────

/// Resolved inset-area position for an anchored element.
///
/// All fields are in CSS px.  Apply to the positioned element's insets:
///
/// ```text
/// element.top    = position.top;
/// element.left   = position.left;
/// element.width  = position.width;   // None = use element's own width
/// element.height = position.height;  // None = use element's own height
/// ```
#[derive(Debug, Clone, Copy)]
pub struct AnchoredPosition {
    /// Distance from the containing block's top edge to the element's top edge (CSS px).
    pub top: f32,
    /// Distance from the containing block's left edge to the element's left edge (CSS px).
    pub left: f32,
    /// Optional width constraint imposed by `inset-area` (CSS px).
    /// `None` means the element retains its intrinsic width.
    pub width: Option<f32>,
    /// Optional height constraint imposed by `inset-area` (CSS px).
    /// `None` means the element retains its intrinsic height.
    pub height: Option<f32>,
}

/// Resolve the `inset-area` shorthand for a positioned element using an anchor.
///
/// `inset-area` maps a 3×3 grid (rows: start/center/end, cols: start/center/end)
/// relative to the default anchor element.  This function translates that
/// grid-cell selection to a concrete `(top, left, width, height)` tuple.
///
/// - `registry` — anchor registry from [`collect_anchors`].
/// - `anchor_name` — resolved `position-anchor` value (e.g. `"--my-anchor"`).
/// - `row` — vertical `inset-area` keyword (which row(s) the element occupies).
/// - `col` — horizontal `inset-area` keyword (which column(s) the element occupies).
/// - `containing_rect` — the positioned element's containing block rect
///   (CSS px, same coordinate space as anchor rects).
///
/// Returns `None` when either the anchor isn't in the registry or both `row`
/// and `col` are [`InsetAreaKeyword::None`].
// CSS: inset-area, position-anchor
pub fn resolve_inset_area(
    registry: &AnchorRegistry,
    anchor_name: &str,
    row: InsetAreaKeyword,
    col: InsetAreaKeyword,
    containing_rect: Rect,
) -> Option<AnchoredPosition> {
    if row == InsetAreaKeyword::None && col == InsetAreaKeyword::None {
        return None;
    }
    let entry = registry.get(anchor_name)?;
    let anchor = entry.rect;

    let (top, height) = resolve_axis_band(
        row,
        anchor.y,
        anchor.y + anchor.height,
        containing_rect.y,
        containing_rect.y + containing_rect.height,
    );
    let (left, width) = resolve_axis_band(
        col,
        anchor.x,
        anchor.x + anchor.width,
        containing_rect.x,
        containing_rect.x + containing_rect.width,
    );

    Some(AnchoredPosition {
        top: top - containing_rect.y,
        left: left - containing_rect.x,
        width,
        height,
    })
}

/// Map one axis's `InsetAreaKeyword` to `(start_px, optional_size)`.
///
/// - `kw`              — the inset-area keyword for this axis.
/// - `anchor_start`    — anchor's leading edge on this axis (CSS px, doc space).
/// - `anchor_end`      — anchor's trailing edge on this axis (CSS px, doc space).
/// - `cb_start`        — containing block's leading edge (CSS px, doc space).
/// - `cb_end`          — containing block's trailing edge (CSS px, doc space).
///
/// Returns `(element_start, element_size)` where `element_size` is `None` for
/// keywords that don't constrain the element's size (e.g. `Start` when the
/// element fits anywhere in the start region).
fn resolve_axis_band(
    kw: InsetAreaKeyword,
    anchor_start: f32,
    anchor_end: f32,
    cb_start: f32,
    cb_end: f32,
) -> (f32, Option<f32>) {
    match kw {
        InsetAreaKeyword::None => (cb_start, None),
        // Start region: from containing-block start to anchor start.
        InsetAreaKeyword::Start | InsetAreaKeyword::SelfStart => {
            let band_size = (anchor_start - cb_start).max(0.0);
            (cb_start, Some(band_size))
        }
        // Center region: from anchor start to anchor end (overlapping the anchor).
        InsetAreaKeyword::Center => (anchor_start, Some((anchor_end - anchor_start).max(0.0))),
        // End region: from anchor end to containing-block end.
        InsetAreaKeyword::End | InsetAreaKeyword::SelfEnd => {
            let band_size = (cb_end - anchor_end).max(0.0);
            (anchor_end, Some(band_size))
        }
        // Start + Center: from cb_start to anchor_end.
        InsetAreaKeyword::SpanStart => {
            let band_size = (anchor_end - cb_start).max(0.0);
            (cb_start, Some(band_size))
        }
        // Center + End: from anchor_start to cb_end.
        InsetAreaKeyword::SpanEnd => {
            let band_size = (cb_end - anchor_start).max(0.0);
            (anchor_start, Some(band_size))
        }
        // All three cells: full containing block.
        InsetAreaKeyword::SpanAll => (cb_start, Some((cb_end - cb_start).max(0.0))),
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;
    use lumen_dom::NodeId;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn node(n: u32) -> NodeId {
        NodeId::from_index(n as usize)
    }

    fn make_registry(name: &str, anchor_rect: Rect) -> AnchorRegistry {
        let mut reg = AnchorRegistry::default();
        register_anchor(&mut reg, name.to_string(), node(1), anchor_rect);
        reg
    }

    // ── collect_anchors ──────────────────────────────────────────────────────

    #[test]
    fn collect_anchors_empty_tree_returns_empty_registry() {
        use crate::box_tree::{BoxKind, LayoutBox};
        use crate::style::ComputedStyle;

        let root = LayoutBox {
            node: node(0),
            rect: rect(0.0, 0.0, 800.0, 600.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        let reg = collect_anchors(&root);
        assert!(reg.is_empty());
    }

    #[test]
    fn collect_anchors_single_child_with_anchor_name() {
        use crate::box_tree::{BoxKind, LayoutBox};
        use crate::style::ComputedStyle;

        let mut child = LayoutBox {
            node: node(1),
            rect: rect(100.0, 150.0, 200.0, 100.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        child.style.anchor_name = Some("--tooltip".into());

        let root = LayoutBox {
            node: node(0),
            rect: rect(0.0, 0.0, 800.0, 600.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![child],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };

        let reg = collect_anchors(&root);
        let entry = reg.get("--tooltip").expect("anchor --tooltip not found");
        assert_eq!(entry.node, node(1));
        assert_eq!(entry.rect, rect(100.0, 150.0, 200.0, 100.0));
    }

    #[test]
    fn collect_anchors_nested_hierarchy() {
        use crate::box_tree::{BoxKind, LayoutBox};
        use crate::style::ComputedStyle;

        let mut anchor_deep = LayoutBox {
            node: node(2),
            rect: rect(50.0, 50.0, 100.0, 100.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        anchor_deep.style.anchor_name = Some("--deep".into());

        let child = LayoutBox {
            node: node(1),
            rect: rect(0.0, 0.0, 200.0, 200.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![anchor_deep],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };

        let root = LayoutBox {
            node: node(0),
            rect: rect(0.0, 0.0, 800.0, 600.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![child],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };

        let reg = collect_anchors(&root);
        let entry = reg.get("--deep").expect("anchor --deep not found");
        assert_eq!(entry.node, node(2));
        assert_eq!(entry.rect, rect(50.0, 50.0, 100.0, 100.0));
    }

    #[test]
    fn collect_anchors_multiple_siblings() {
        use crate::box_tree::{BoxKind, LayoutBox};
        use crate::style::ComputedStyle;

        let mut child1 = LayoutBox {
            node: node(1),
            rect: rect(0.0, 0.0, 100.0, 100.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        child1.style.anchor_name = Some("--left".into());

        let mut child2 = LayoutBox {
            node: node(2),
            rect: rect(150.0, 0.0, 100.0, 100.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
        child2.style.anchor_name = Some("--right".into());

        let root = LayoutBox {
            node: node(0),
            rect: rect(0.0, 0.0, 800.0, 600.0),
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: vec![child1, child2],
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };

        let reg = collect_anchors(&root);
        let left = reg.get("--left").expect("anchor --left not found");
        let right = reg.get("--right").expect("anchor --right not found");
        assert_eq!(left.node, node(1));
        assert_eq!(right.node, node(2));
    }

    // ── register_anchor / get ────────────────────────────────────────────────

    #[test]
    fn register_and_get_anchor() {
        let anchor_rect = rect(100.0, 200.0, 50.0, 30.0);
        let reg = make_registry("--btn", anchor_rect);
        let entry = reg.get("--btn").expect("anchor not found");
        assert_eq!(entry.rect, anchor_rect);
    }

    #[test]
    fn later_registration_wins() {
        let mut reg = AnchorRegistry::default();
        register_anchor(&mut reg, "--a".to_string(), node(1), rect(0.0, 0.0, 10.0, 10.0));
        register_anchor(&mut reg, "--a".to_string(), node(2), rect(50.0, 50.0, 20.0, 20.0));
        let entry = reg.get("--a").unwrap();
        assert_eq!(entry.node, node(2)); // last wins
        assert_eq!(entry.rect.x, 50.0);
    }

    #[test]
    fn unknown_name_returns_none() {
        let reg = make_registry("--foo", rect(0.0, 0.0, 10.0, 10.0));
        assert!(reg.get("--bar").is_none());
    }

    // ── resolve_anchor_function ──────────────────────────────────────────────

    // anchor rect: x=100, y=200, w=80, h=40  →  right=180, bottom=240

    #[test]
    fn anchor_function_top_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Top, false);
        assert_eq!(v, Some(200.0));
    }

    #[test]
    fn anchor_function_bottom_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Bottom, false);
        assert_eq!(v, Some(240.0));
    }

    #[test]
    fn anchor_function_left_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Left, true);
        assert_eq!(v, Some(100.0));
    }

    #[test]
    fn anchor_function_right_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Right, true);
        assert_eq!(v, Some(180.0));
    }

    #[test]
    fn anchor_function_center_vertical() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Center, false);
        assert_eq!(v, Some(220.0)); // 200 + 40/2
    }

    #[test]
    fn anchor_function_center_horizontal() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Center, true);
        assert_eq!(v, Some(140.0)); // 100 + 80/2
    }

    #[test]
    fn anchor_function_percentage() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        let v = resolve_anchor_function(&reg, "--a", AnchorSide::Percentage(25.0), true);
        assert_eq!(v, Some(120.0)); // 100 + 80 * 0.25
    }

    #[test]
    fn anchor_function_cross_axis_returns_none() {
        // top/bottom are invalid on the horizontal axis and vice-versa.
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert!(resolve_anchor_function(&reg, "--a", AnchorSide::Top, true).is_none());
        assert!(resolve_anchor_function(&reg, "--a", AnchorSide::Left, false).is_none());
    }

    #[test]
    fn anchor_function_missing_anchor_returns_none() {
        let reg = AnchorRegistry::default();
        assert!(resolve_anchor_function(&reg, "--missing", AnchorSide::Top, false).is_none());
    }

    // ── resolve_inset_area ───────────────────────────────────────────────────

    // Setup:
    //   anchor rect : x=300, y=200, w=100, h=50
    //   containing block: x=0, y=0, w=800, h=600
    //
    // Start-col band : x=[0, 300),  w=300
    // Center-col band: x=[300, 400), w=100
    // End-col band   : x=[400, 800), w=400
    //
    // Start-row band : y=[0, 200),  h=200
    // Center-row band: y=[200, 250), h=50
    // End-row band   : y=[250, 600), h=350

    fn setup_inset_area() -> (AnchorRegistry, Rect) {
        let anchor_rect = rect(300.0, 200.0, 100.0, 50.0);
        let reg = make_registry("--anchor", anchor_rect);
        let cb = rect(0.0, 0.0, 800.0, 600.0);
        (reg, cb)
    }

    #[test]
    fn inset_area_start_start() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::Start, InsetAreaKeyword::Start, cb,
        )
        .unwrap();
        // top: distance from cb.top (0) to anchor.top (200) relative to cb → 0
        // height: 200 (0..200)
        // left: 0, width: 300
        assert_eq!(pos.top, 0.0);
        assert_eq!(pos.height, Some(200.0));
        assert_eq!(pos.left, 0.0);
        assert_eq!(pos.width, Some(300.0));
    }

    #[test]
    fn inset_area_center_center() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::Center, InsetAreaKeyword::Center, cb,
        )
        .unwrap();
        // Overlaps the anchor exactly.
        assert_eq!(pos.top, 200.0); // anchor.y - cb.y
        assert_eq!(pos.height, Some(50.0));
        assert_eq!(pos.left, 300.0); // anchor.x - cb.x
        assert_eq!(pos.width, Some(100.0));
    }

    #[test]
    fn inset_area_end_end() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::End, InsetAreaKeyword::End, cb,
        )
        .unwrap();
        assert_eq!(pos.top, 250.0); // anchor.bottom - cb.y
        assert_eq!(pos.height, Some(350.0)); // 600 - 250
        assert_eq!(pos.left, 400.0); // anchor.right - cb.x
        assert_eq!(pos.width, Some(400.0)); // 800 - 400
    }

    #[test]
    fn inset_area_span_all_span_all() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::SpanAll, InsetAreaKeyword::SpanAll, cb,
        )
        .unwrap();
        // Full containing block.
        assert_eq!(pos.top, 0.0);
        assert_eq!(pos.height, Some(600.0));
        assert_eq!(pos.left, 0.0);
        assert_eq!(pos.width, Some(800.0));
    }

    #[test]
    fn inset_area_span_start_col() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::Center, InsetAreaKeyword::SpanStart, cb,
        )
        .unwrap();
        // col: SpanStart → cb_start to anchor_end → [0, 400), w=400
        assert_eq!(pos.left, 0.0);
        assert_eq!(pos.width, Some(400.0));
    }

    #[test]
    fn inset_area_span_end_row() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::SpanEnd, InsetAreaKeyword::Center, cb,
        )
        .unwrap();
        // row: SpanEnd → anchor_start to cb_end → [200, 600), h=400
        assert_eq!(pos.top, 200.0);
        assert_eq!(pos.height, Some(400.0));
    }

    #[test]
    fn inset_area_none_keywords_returns_none() {
        let (reg, cb) = setup_inset_area();
        assert!(resolve_inset_area(
            &reg, "--anchor",
            InsetAreaKeyword::None, InsetAreaKeyword::None, cb,
        )
        .is_none());
    }

    #[test]
    fn inset_area_missing_anchor_returns_none() {
        let reg = AnchorRegistry::default();
        let cb = rect(0.0, 0.0, 800.0, 600.0);
        assert!(resolve_inset_area(
            &reg, "--ghost",
            InsetAreaKeyword::Center, InsetAreaKeyword::End, cb,
        )
        .is_none());
    }
}
