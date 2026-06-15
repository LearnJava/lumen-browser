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
//!    * Dimension from `anchor-size()` function calls in `width`/`height`.
//!      See [`resolve_anchor_size`].
//!
//! P4 wires the CSS properties:
//! - `anchor-name: --foo` → `ComputedStyle.anchor_name: Option<String>`
//! - `position-anchor: --foo` → `ComputedStyle.position_anchor: Option<String>`
//! - `inset-area: <row> <col>` → `ComputedStyle.inset_area_row` / `ComputedStyle.inset_area_col`
//! - `anchor-scope: all | none | --name` → `ComputedStyle.anchor_scope`
//! - `width: anchor-size(...)` / `height: anchor-size(...)` → `ComputedStyle.anchor_size_w/h`
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

// ─── AnchorScope ─────────────────────────────────────────────────────────────

// CSS: anchor-scope
/// Value of the CSS `anchor-scope` property (CSS Anchor Positioning L1 §2.1).
///
/// When set on an element, limits which named anchors from its descendants are
/// visible to positioned elements outside the element's subtree.  Prevents
/// anchor names leaking across shadow DOM or component boundaries.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AnchorScope {
    /// `anchor-scope: none` — default, no scoping restriction.
    #[default]
    None,
    /// `anchor-scope: all` — all anchor names in this subtree are scoped.
    All,
    /// `anchor-scope: --name` — only this specific anchor name is scoped.
    Named(Box<str>),
}

// ─── AnchorSizeDimension ─────────────────────────────────────────────────────

// CSS: anchor-size()
/// Which dimension the `anchor-size()` function references.
///
/// Corresponds to `<anchor-size>` in CSS Anchor Positioning L1 §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSizeDimension {
    /// `anchor-size(width)` — the anchor's border-box width.
    Width,
    /// `anchor-size(height)` — the anchor's border-box height.
    Height,
    /// `anchor-size(block)` — block-axis size (height in horizontal writing mode).
    Block,
    /// `anchor-size(inline)` — inline-axis size (width in horizontal writing mode).
    Inline,
    /// `anchor-size(self-block)` — block-axis size of the positioned element itself.
    SelfBlock,
    /// `anchor-size(self-inline)` — inline-axis size of the positioned element itself.
    SelfInline,
}

// ─── AnchorSizeFunc ──────────────────────────────────────────────────────────

/// Parsed `anchor-size(<anchor-el>? <anchor-size>)` value stored in ComputedStyle.
///
/// Used when `width` or `height` contains an `anchor-size()` function call.
/// Resolved in `lay_out_abs_children` via [`resolve_anchor_size`] once the
/// anchor registry is available.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorSizeFunc {
    /// Optional anchor element name (e.g. `"--my-anchor"`).
    /// `None` = use the element's `position-anchor` default.
    pub anchor_name: Option<Box<str>>,
    /// Which dimension of the anchor to use.
    pub dimension: AnchorSizeDimension,
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
    /// If `Some(node)`: this anchor is scoped — only visible to positioned elements
    /// that are descendants of the given scope root.
    /// `None` = globally visible (no scope restriction).
    pub scope_root: Option<NodeId>,
}

impl AnchorRegistry {
    /// Look up an anchor by CSS name (e.g. `"--tooltip-anchor"`).
    ///
    /// Returns `None` when no element in the current layout tree declared this
    /// `anchor-name`.
    pub fn get(&self, name: &str) -> Option<&AnchorEntry> {
        self.entries.get(name)
    }

    /// Scope-aware lookup: returns the anchor entry only if it is visible to a
    /// positioned element whose ancestors include the given slice of node IDs.
    ///
    /// An anchor is visible when:
    /// - It has no `scope_root` (globally visible), OR
    /// - Its `scope_root` is in `ancestor_ids` (positioned element is a descendant
    ///   of the scope root).
    pub fn get_scoped<'a>(
        &'a self,
        name: &str,
        ancestor_ids: &[NodeId],
    ) -> Option<&'a AnchorEntry> {
        let entry = self.entries.get(name)?;
        match entry.scope_root {
            None => Some(entry),
            Some(scope) => ancestor_ids.contains(&scope).then_some(entry),
        }
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
/// Respects `anchor-scope`: anchors inside a scoped subtree carry `scope_root`
/// set to the scoping ancestor's node ID so callers can filter by ancestry.
// CSS: anchor-name, anchor-scope
pub fn collect_anchors(root: &LayoutBox) -> AnchorRegistry {
    let mut registry = AnchorRegistry::default();
    collect_anchors_rec(root, &mut registry, None);
    registry
}

fn collect_anchors_rec(lb: &LayoutBox, registry: &mut AnchorRegistry, scope_root: Option<NodeId>) {
    // Determine the scope root for this element's descendants.
    let next_scope = match &lb.style.anchor_scope {
        AnchorScope::All | AnchorScope::Named(_) => Some(lb.node),
        AnchorScope::None => scope_root,
    };

    if let Some(name) = &lb.style.anchor_name {
        register_anchor_scoped(registry, name.to_string(), lb.node, lb.rect, next_scope);
    }
    for child in &lb.children {
        collect_anchors_rec(child, registry, next_scope);
    }
}

/// Register an element as a named anchor (globally visible, no scope restriction).
///
/// Called by P4's CSS wiring when it encounters `anchor-name: --foo` in ComputedStyle.
/// Uses last-in-tree-order semantics: later registrations overwrite earlier ones.
pub fn register_anchor(registry: &mut AnchorRegistry, name: String, node: NodeId, rect: Rect) {
    registry.entries.insert(name, AnchorEntry { node, rect, scope_root: None });
}

/// Register an element as a named anchor with optional scope restriction.
///
/// `scope_root` — the node ID of the ancestor with `anchor-scope` that restricts
/// this anchor's visibility.  `None` = globally visible.
pub fn register_anchor_scoped(
    registry: &mut AnchorRegistry,
    name: String,
    node: NodeId,
    rect: Rect,
    scope_root: Option<NodeId>,
) {
    registry.entries.insert(name, AnchorEntry { node, rect, scope_root });
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
// CSS: anchor(), position-anchor
pub fn resolve_anchor_function(
    registry: &AnchorRegistry,
    anchor_name: &str,
    side: AnchorSide,
    is_horizontal: bool,
) -> Option<f32> {
    let entry = registry.get(anchor_name)?;
    let r = entry.rect;

    let value = if is_horizontal {
        match side {
            AnchorSide::Left | AnchorSide::Start => r.x,
            AnchorSide::Right | AnchorSide::End => r.x + r.width,
            AnchorSide::Center => r.x + r.width * 0.5,
            AnchorSide::Top | AnchorSide::Bottom => return None,
            AnchorSide::Percentage(pct) => r.x + r.width * pct / 100.0,
        }
    } else {
        match side {
            AnchorSide::Top | AnchorSide::Start => r.y,
            AnchorSide::Bottom | AnchorSide::End => r.y + r.height,
            AnchorSide::Center => r.y + r.height * 0.5,
            AnchorSide::Left | AnchorSide::Right => return None,
            AnchorSide::Percentage(pct) => r.y + r.height * pct / 100.0,
        }
    };
    Some(value)
}

// ─── resolve_anchor_size ─────────────────────────────────────────────────────

/// Resolve an `anchor-size(<anchor-el>? <dimension>)` function to a CSS pixel value.
///
/// Used when `width` or `height` contains an `anchor-size()` call.
///
/// - `registry` — the anchor registry built by [`collect_anchors`].
/// - `func` — the parsed `anchor-size()` value from [`AnchorSizeFunc`].
/// - `default_anchor` — the element's `position-anchor` value, used when
///   `func.anchor_name` is `None`.
///
/// Returns `None` when no resolvable anchor exists in the registry.
// CSS: anchor-size()
pub fn resolve_anchor_size(
    registry: &AnchorRegistry,
    func: &AnchorSizeFunc,
    default_anchor: Option<&str>,
) -> Option<f32> {
    let name = func.anchor_name.as_deref().or(default_anchor)?;
    let entry = registry.get(name)?;
    let r = entry.rect;
    // Horizontal writing mode: inline ↔ width, block ↔ height.
    let value = match func.dimension {
        AnchorSizeDimension::Width
        | AnchorSizeDimension::Inline
        | AnchorSizeDimension::SelfInline => r.width,
        AnchorSizeDimension::Height
        | AnchorSizeDimension::Block
        | AnchorSizeDimension::SelfBlock => r.height,
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
/// relative to the default anchor element.  Translates the grid-cell selection
/// to a concrete `(top, left, width, height)` tuple.
///
/// Returns `None` when the anchor isn't in the registry or both keywords are `None`.
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
    resolve_inset_area_from_entry(entry, row, col, containing_rect)
}

/// Scope-aware variant of [`resolve_inset_area`].
///
/// `ancestor_ids` — NodeIds of all ancestors of the positioned element.
/// Anchors outside the positioned element's scope are not resolved.
// CSS: inset-area, position-anchor, anchor-scope
pub fn resolve_inset_area_scoped(
    registry: &AnchorRegistry,
    anchor_name: &str,
    row: InsetAreaKeyword,
    col: InsetAreaKeyword,
    containing_rect: Rect,
    ancestor_ids: &[NodeId],
) -> Option<AnchoredPosition> {
    if row == InsetAreaKeyword::None && col == InsetAreaKeyword::None {
        return None;
    }
    let entry = registry.get_scoped(anchor_name, ancestor_ids)?;
    resolve_inset_area_from_entry(entry, row, col, containing_rect)
}

fn resolve_inset_area_from_entry(
    entry: &AnchorEntry,
    row: InsetAreaKeyword,
    col: InsetAreaKeyword,
    containing_rect: Rect,
) -> Option<AnchoredPosition> {
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
/// - `kw`           — the inset-area keyword for this axis.
/// - `anchor_start` — anchor's leading edge (CSS px, doc space).
/// - `anchor_end`   — anchor's trailing edge (CSS px, doc space).
/// - `cb_start`     — containing block's leading edge (CSS px, doc space).
/// - `cb_end`       — containing block's trailing edge (CSS px, doc space).
///
/// Returns `(element_start, element_size)`.  `element_size` is `None` when the
/// keyword does not constrain the element's size on this axis.
fn resolve_axis_band(
    kw: InsetAreaKeyword,
    anchor_start: f32,
    anchor_end: f32,
    cb_start: f32,
    cb_end: f32,
) -> (f32, Option<f32>) {
    match kw {
        InsetAreaKeyword::None => (cb_start, None),
        InsetAreaKeyword::Start | InsetAreaKeyword::SelfStart => {
            (cb_start, Some((anchor_start - cb_start).max(0.0)))
        }
        InsetAreaKeyword::Center => {
            (anchor_start, Some((anchor_end - anchor_start).max(0.0)))
        }
        InsetAreaKeyword::End | InsetAreaKeyword::SelfEnd => {
            (anchor_end, Some((cb_end - anchor_end).max(0.0)))
        }
        InsetAreaKeyword::SpanStart => {
            (cb_start, Some((anchor_end - cb_start).max(0.0)))
        }
        InsetAreaKeyword::SpanEnd => {
            (anchor_start, Some((cb_end - anchor_start).max(0.0)))
        }
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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
            dirty: Default::default(),
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

    #[test]
    fn anchor_function_top_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(resolve_anchor_function(&reg, "--a", AnchorSide::Top, false), Some(200.0));
    }

    #[test]
    fn anchor_function_bottom_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(resolve_anchor_function(&reg, "--a", AnchorSide::Bottom, false), Some(240.0));
    }

    #[test]
    fn anchor_function_left_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(resolve_anchor_function(&reg, "--a", AnchorSide::Left, true), Some(100.0));
    }

    #[test]
    fn anchor_function_right_edge() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(resolve_anchor_function(&reg, "--a", AnchorSide::Right, true), Some(180.0));
    }

    #[test]
    fn anchor_function_center_vertical() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(resolve_anchor_function(&reg, "--a", AnchorSide::Center, false), Some(220.0));
    }

    #[test]
    fn anchor_function_center_horizontal() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(resolve_anchor_function(&reg, "--a", AnchorSide::Center, true), Some(140.0));
    }

    #[test]
    fn anchor_function_percentage() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert_eq!(
            resolve_anchor_function(&reg, "--a", AnchorSide::Percentage(25.0), true),
            Some(120.0)
        );
    }

    #[test]
    fn anchor_function_cross_axis_returns_none() {
        let reg = make_registry("--a", rect(100.0, 200.0, 80.0, 40.0));
        assert!(resolve_anchor_function(&reg, "--a", AnchorSide::Top, true).is_none());
        assert!(resolve_anchor_function(&reg, "--a", AnchorSide::Left, false).is_none());
    }

    #[test]
    fn anchor_function_missing_anchor_returns_none() {
        let reg = AnchorRegistry::default();
        assert!(resolve_anchor_function(&reg, "--missing", AnchorSide::Top, false).is_none());
    }

    // ── resolve_anchor_size (BB-8: 8 new unit tests) ─────────────────────────

    #[test]
    fn anchor_size_width_from_default_anchor() {
        let reg = make_registry("--btn", rect(100.0, 200.0, 80.0, 40.0));
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::Width };
        assert_eq!(resolve_anchor_size(&reg, &func, Some("--btn")), Some(80.0));
    }

    #[test]
    fn anchor_size_height_from_default_anchor() {
        let reg = make_registry("--btn", rect(100.0, 200.0, 80.0, 40.0));
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::Height };
        assert_eq!(resolve_anchor_size(&reg, &func, Some("--btn")), Some(40.0));
    }

    #[test]
    fn anchor_size_with_explicit_anchor_name() {
        // anchor-size(--other, width) uses --other, not the default anchor.
        let mut reg = AnchorRegistry::default();
        register_anchor(&mut reg, "--btn".to_string(), node(1), rect(0.0, 0.0, 80.0, 40.0));
        register_anchor(&mut reg, "--other".to_string(), node(2), rect(0.0, 0.0, 120.0, 60.0));
        let func = AnchorSizeFunc {
            anchor_name: Some("--other".into()),
            dimension: AnchorSizeDimension::Width,
        };
        assert_eq!(resolve_anchor_size(&reg, &func, Some("--btn")), Some(120.0));
    }

    #[test]
    fn anchor_size_inline_maps_to_width() {
        let reg = make_registry("--a", rect(0.0, 0.0, 90.0, 45.0));
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::Inline };
        assert_eq!(resolve_anchor_size(&reg, &func, Some("--a")), Some(90.0));
    }

    #[test]
    fn anchor_size_block_maps_to_height() {
        let reg = make_registry("--a", rect(0.0, 0.0, 90.0, 45.0));
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::Block };
        assert_eq!(resolve_anchor_size(&reg, &func, Some("--a")), Some(45.0));
    }

    #[test]
    fn anchor_size_missing_anchor_returns_none() {
        let reg = AnchorRegistry::default();
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::Width };
        assert!(resolve_anchor_size(&reg, &func, Some("--missing")).is_none());
    }

    #[test]
    fn anchor_size_no_default_and_no_explicit_returns_none() {
        let reg = make_registry("--a", rect(0.0, 0.0, 100.0, 50.0));
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::Width };
        assert!(resolve_anchor_size(&reg, &func, None).is_none());
    }

    #[test]
    fn anchor_size_self_inline_maps_to_width() {
        let reg = make_registry("--a", rect(0.0, 0.0, 75.0, 25.0));
        let func = AnchorSizeFunc { anchor_name: None, dimension: AnchorSizeDimension::SelfInline };
        assert_eq!(resolve_anchor_size(&reg, &func, Some("--a")), Some(75.0));
    }

    // ── anchor-scope / get_scoped ────────────────────────────────────────────

    #[test]
    fn anchor_scope_none_visible_everywhere() {
        let reg = make_registry("--global", rect(0.0, 0.0, 100.0, 50.0));
        // Globally visible even with an empty ancestor list.
        assert!(reg.get_scoped("--global", &[]).is_some());
    }

    #[test]
    fn anchor_scope_all_blocks_outside_descendants() {
        let mut reg = AnchorRegistry::default();
        register_anchor_scoped(
            &mut reg,
            "--scoped".to_string(),
            node(10),
            rect(0.0, 0.0, 100.0, 50.0),
            Some(node(5)), // scoped: only visible under node(5)
        );

        // Positioned element whose ancestors do NOT include node(5) → invisible.
        assert!(reg.get_scoped("--scoped", &[node(1), node(2), node(3)]).is_none());
        // Positioned element whose ancestors DO include node(5) → visible.
        assert!(reg.get_scoped("--scoped", &[node(1), node(5), node(8)]).is_some());
    }

    #[test]
    fn anchor_scope_named_restricts_specific_name() {
        let mut reg = AnchorRegistry::default();
        register_anchor_scoped(
            &mut reg,
            "--foo".to_string(),
            node(10),
            rect(0.0, 0.0, 60.0, 30.0),
            Some(node(3)), // scoped to node(3)'s subtree
        );
        register_anchor(&mut reg, "--bar".to_string(), node(11), rect(0.0, 0.0, 80.0, 40.0));

        let outside = [node(1), node(2)]; // node(3) not in ancestors
        assert!(reg.get_scoped("--foo", &outside).is_none()); // scoped → invisible
        assert!(reg.get_scoped("--bar", &outside).is_some()); // global → visible
    }

    // ── resolve_inset_area ───────────────────────────────────────────────────

    fn setup_inset_area() -> (AnchorRegistry, Rect) {
        let reg = make_registry("--anchor", rect(300.0, 200.0, 100.0, 50.0));
        let cb = rect(0.0, 0.0, 800.0, 600.0);
        (reg, cb)
    }

    #[test]
    fn inset_area_start_start() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::Start, InsetAreaKeyword::Start, cb,
        ).unwrap();
        assert_eq!(pos.top, 0.0);
        assert_eq!(pos.height, Some(200.0));
        assert_eq!(pos.left, 0.0);
        assert_eq!(pos.width, Some(300.0));
    }

    #[test]
    fn inset_area_center_center() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::Center, InsetAreaKeyword::Center, cb,
        ).unwrap();
        assert_eq!(pos.top, 200.0);
        assert_eq!(pos.height, Some(50.0));
        assert_eq!(pos.left, 300.0);
        assert_eq!(pos.width, Some(100.0));
    }

    #[test]
    fn inset_area_end_end() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::End, InsetAreaKeyword::End, cb,
        ).unwrap();
        assert_eq!(pos.top, 250.0);
        assert_eq!(pos.height, Some(350.0));
        assert_eq!(pos.left, 400.0);
        assert_eq!(pos.width, Some(400.0));
    }

    #[test]
    fn inset_area_span_all_span_all() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::SpanAll, InsetAreaKeyword::SpanAll, cb,
        ).unwrap();
        assert_eq!(pos.top, 0.0);
        assert_eq!(pos.height, Some(600.0));
        assert_eq!(pos.left, 0.0);
        assert_eq!(pos.width, Some(800.0));
    }

    #[test]
    fn inset_area_span_start_col() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::Center, InsetAreaKeyword::SpanStart, cb,
        ).unwrap();
        assert_eq!(pos.left, 0.0);
        assert_eq!(pos.width, Some(400.0));
    }

    #[test]
    fn inset_area_span_end_row() {
        let (reg, cb) = setup_inset_area();
        let pos = resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::SpanEnd, InsetAreaKeyword::Center, cb,
        ).unwrap();
        assert_eq!(pos.top, 200.0);
        assert_eq!(pos.height, Some(400.0));
    }

    #[test]
    fn inset_area_none_keywords_returns_none() {
        let (reg, cb) = setup_inset_area();
        assert!(resolve_inset_area(
            &reg, "--anchor", InsetAreaKeyword::None, InsetAreaKeyword::None, cb,
        ).is_none());
    }

    #[test]
    fn inset_area_missing_anchor_returns_none() {
        let reg = AnchorRegistry::default();
        let cb = rect(0.0, 0.0, 800.0, 600.0);
        assert!(resolve_inset_area(
            &reg, "--ghost", InsetAreaKeyword::Center, InsetAreaKeyword::End, cb,
        ).is_none());
    }
}
