//! Selector-based lookup over the layout tree.
//!
//! Provides `find_box_by_selector` and `computed_style_by_selector` for
//! in-process driver testing (P3 BrowserSession, ADR-006 §8A.4).
//! Selector matching is backed by the full CSS3 engine in `style.rs`
//! (tag, .class, #id, attribute, compound, descendant/child/sibling
//! combinators, `:nth-*`, `:not()`, `:is()`, `:where()`).

use lumen_css_parser::{parse_selector_list, ComplexSelector};
use lumen_dom::{Document, NodeId};

use crate::box_tree::{BoxKind, LayoutBox};
use crate::style::{
    matches_complex, Color, ColorSpace, CssColor, Direction, Display, FontStretch, FontStyle,
    FontWeight, FontVariant, Length, LengthOrAuto, Overflow, Position, TextAlign,
    TextDecorationLine, TextDecorationStyle, TextEmphasisStyle, TextTransform, Visibility,
    WhiteSpace, ComputedStyle,
};

// ──────────────── LayoutBox extension methods ────────────────

impl LayoutBox {
    /// Finds the first descendant LayoutBox matching the given selector.
    ///
    /// Searches this box's descendants in document order. Returns `None` if
    /// `sel` is empty, invalid, or no descendant matches.
    ///
    /// # Arguments
    /// * `doc` - The Document for selector matching
    /// * `sel` - CSS selector string (tag, .class, #id, compound, combinators, pseudo-classes)
    ///
    /// # Example
    /// ```ignore
    /// let found = root_box.find_descendant_by_selector(&doc, "div.container > p");
    /// ```
    pub fn find_descendant_by_selector<'a>(
        &'a self,
        doc: &Document,
        sel: &str,
    ) -> Option<&'a LayoutBox> {
        find_box_by_selector(self, doc, sel)
    }

    /// Finds all descendant LayoutBoxes matching the given selector.
    ///
    /// Traverses this box's descendants in document order. Returns an empty
    /// Vec if `sel` is empty, invalid, or no descendants match.
    ///
    /// # Arguments
    /// * `doc` - The Document for selector matching
    /// * `sel` - CSS selector string (tag, .class, #id, compound, combinators, pseudo-classes)
    ///
    /// # Example
    /// ```ignore
    /// let items = container_box.find_all_descendants_by_selector(&doc, ".item");
    /// ```
    pub fn find_all_descendants_by_selector<'a>(
        &'a self,
        doc: &Document,
        sel: &str,
    ) -> Vec<&'a LayoutBox> {
        find_all_by_selector(self, doc, sel)
    }

    /// Returns the computed style snapshot for this box.
    ///
    /// Converts the internal ComputedStyle to a snapshot suitable for
    /// driver assertions and debugging.
    pub fn style_snapshot(&self) -> ComputedStyleSnapshot {
        ComputedStyleSnapshot::from(&self.style)
    }
}

// ──────────────── ComputedStyleSnapshot ────────────────

/// Flat snapshot of the most-queried CSS properties for in-process testing.
///
/// Constructed from `&ComputedStyle` via `From`. All field types match
/// `ComputedStyle` exactly — no lossy conversion.
/// Intended for assertions in P3 BrowserSession driver tests.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyleSnapshot {
    /// CSS `display`. Determines the box model (block, inline, flex, etc.).
    pub display: Display,
    /// CSS `visibility`. `Hidden` boxes still occupy space.
    pub visibility: Visibility,
    /// CSS `position` (static/relative/absolute/fixed/sticky).
    pub position: Position,
    /// CSS `direction` (ltr/rtl). Inherited.
    pub direction: Direction,
    /// CSS `width`. `None` = auto.
    pub width: Option<Length>,
    /// CSS `height`. `None` = auto.
    pub height: Option<Length>,
    /// CSS `margin-top`.
    pub margin_top: LengthOrAuto,
    /// CSS `margin-right`.
    pub margin_right: LengthOrAuto,
    /// CSS `margin-bottom`.
    pub margin_bottom: LengthOrAuto,
    /// CSS `margin-left`.
    pub margin_left: LengthOrAuto,
    /// CSS `padding-top`.
    pub padding_top: Length,
    /// CSS `padding-right`.
    pub padding_right: Length,
    /// CSS `padding-bottom`.
    pub padding_bottom: Length,
    /// CSS `padding-left`.
    pub padding_left: Length,
    /// CSS `border-top-width` in CSS px.
    pub border_top_width: f32,
    /// CSS `border-right-width` in CSS px.
    pub border_right_width: f32,
    /// CSS `border-bottom-width` in CSS px.
    pub border_bottom_width: f32,
    /// CSS `border-left-width` in CSS px.
    pub border_left_width: f32,
    /// CSS `color` (foreground text colour).
    pub color: Color,
    /// CSS `color-space` annotation (for wide-gamut rendering).
    pub color_space: ColorSpace,
    /// CSS `background-color`. `None` = transparent (initial).
    pub background_color: Option<CssColor>,
    /// CSS `font-size` in CSS px. Inherited.
    pub font_size: f32,
    /// CSS `line-height` as resolved px. Inherited.
    pub line_height: f32,
    /// CSS `font-style` (normal/italic/oblique). Inherited.
    pub font_style: FontStyle,
    /// CSS `font-weight` (100–900). Inherited.
    pub font_weight: FontWeight,
    /// CSS `font-variant` (normal/small-caps). Inherited.
    pub font_variant: FontVariant,
    /// CSS `font-stretch` (50%–200%). Inherited.
    pub font_stretch: FontStretch,
    /// CSS `text-align`. Inherited.
    pub text_align: TextAlign,
    /// CSS `text-transform`. Inherited.
    pub text_transform: TextTransform,
    /// CSS `white-space`. Inherited.
    pub white_space: WhiteSpace,
    /// CSS `text-decoration-line`.
    pub text_decoration_line: TextDecorationLine,
    /// CSS `text-decoration-style`.
    pub text_decoration_style: TextDecorationStyle,
    /// CSS `text-emphasis-style`.
    pub text_emphasis_style: TextEmphasisStyle,
    /// CSS `opacity` (0.0–1.0).
    pub opacity: f32,
    /// CSS `overflow-x`.
    pub overflow_x: Overflow,
    /// CSS `overflow-y`.
    pub overflow_y: Overflow,
    /// CSS `z-index`. `None` = auto.
    pub z_index: Option<i32>,
}

impl From<&ComputedStyle> for ComputedStyleSnapshot {
    fn from(s: &ComputedStyle) -> Self {
        Self {
            display: s.display,
            visibility: s.visibility,
            position: s.position,
            direction: s.direction,
            width: s.width.clone(),
            height: s.height.clone(),
            margin_top: s.margin_top.clone(),
            margin_right: s.margin_right.clone(),
            margin_bottom: s.margin_bottom.clone(),
            margin_left: s.margin_left.clone(),
            padding_top: s.padding_top.clone(),
            padding_right: s.padding_right.clone(),
            padding_bottom: s.padding_bottom.clone(),
            padding_left: s.padding_left.clone(),
            border_top_width: s.border_top_width,
            border_right_width: s.border_right_width,
            border_bottom_width: s.border_bottom_width,
            border_left_width: s.border_left_width,
            color: s.color,
            color_space: s.color_space,
            background_color: s.background_color,
            font_size: s.font_size,
            line_height: s.line_height,
            font_style: s.font_style,
            font_weight: s.font_weight,
            font_variant: s.font_variant,
            font_stretch: s.font_stretch,
            text_align: s.text_align,
            text_transform: s.text_transform,
            white_space: s.white_space,
            text_decoration_line: s.text_decoration_line,
            text_decoration_style: s.text_decoration_style,
            text_emphasis_style: s.text_emphasis_style.clone(),
            opacity: s.opacity,
            overflow_x: s.overflow_x,
            overflow_y: s.overflow_y,
            z_index: s.z_index,
        }
    }
}

// ──────────────── find_box_by_selector ────────────────

/// Returns a reference to the first `LayoutBox` in document order whose
/// DOM node matches **any** selector in `sel` (comma-separated selector list).
///
/// Uses the full CSS3 selector engine: tag, `.class`, `#id`, attribute
/// selectors, compound selectors, descendant/child/sibling combinators,
/// `:nth-child`, `:not()`, `:is()`, `:where()`.
///
/// Returns `None` when `sel` is empty, all selectors are invalid, or no
/// node in the tree matches.
pub fn find_box_by_selector<'a>(
    root: &'a LayoutBox,
    doc: &Document,
    sel: &str,
) -> Option<&'a LayoutBox> {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return None;
    }
    find_rec(root, doc, &selectors)
}

/// Returns true for LayoutBox kinds that are the primary (non-anonymous) box
/// for a DOM element. Anonymous boxes (InlineRun, InlineBlockRow, etc.) share
/// their parent node's NodeId and must not match independently.
fn is_element_box(kind: &BoxKind) -> bool {
    !matches!(
        kind,
        BoxKind::InlineRun { .. }
            | BoxKind::InlineBlockRow
            | BoxKind::InlineSpace
            | BoxKind::Marker { .. }
            | BoxKind::Contents
    )
}

fn find_rec<'a>(
    b: &'a LayoutBox,
    doc: &Document,
    selectors: &[ComplexSelector],
) -> Option<&'a LayoutBox> {
    if matches!(b.kind, BoxKind::Skip) {
        return None;
    }
    // Only match primary element boxes; anonymous boxes share the parent's NodeId
    // and must not produce a second match for the same selector.
    if is_element_box(&b.kind) && node_matches(b.node, doc, selectors) {
        return Some(b);
    }
    for child in &b.children {
        if let Some(found) = find_rec(child, doc, selectors) {
            return Some(found);
        }
    }
    None
}

fn node_matches(node: lumen_dom::NodeId, doc: &Document, selectors: &[ComplexSelector]) -> bool {
    // matches_complex internally checks NodeData::Element; non-elements return false.
    selectors.iter().any(|sel| matches_complex(sel, doc, node))
}

// ──────────────── computed_style_by_selector ────────────────

/// Returns the computed style snapshot of the first matching `LayoutBox`.
///
/// Equivalent to `find_box_by_selector` followed by `ComputedStyleSnapshot::from(&b.style)`.
/// Returns `None` under the same conditions as `find_box_by_selector`.
pub fn computed_style_by_selector(
    root: &LayoutBox,
    doc: &Document,
    sel: &str,
) -> Option<ComputedStyleSnapshot> {
    find_box_by_selector(root, doc, sel).map(|b| ComputedStyleSnapshot::from(&b.style))
}

// ──────────────── find_all_by_selector ────────────────

/// Returns references to **all** `LayoutBox`es (in document order) whose
/// DOM node matches any selector in `sel`.
///
/// Useful for asserting the count of matching elements or iterating over
/// all occurrences. Returns an empty Vec when `sel` is empty/invalid or
/// no node matches.
pub fn find_all_by_selector<'a>(
    root: &'a LayoutBox,
    doc: &Document,
    sel: &str,
) -> Vec<&'a LayoutBox> {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    find_all_rec(root, doc, &selectors, &mut out);
    out
}

fn find_all_rec<'a>(
    b: &'a LayoutBox,
    doc: &Document,
    selectors: &[ComplexSelector],
    out: &mut Vec<&'a LayoutBox>,
) {
    if matches!(b.kind, BoxKind::Skip) {
        return;
    }
    if is_element_box(&b.kind) && node_matches(b.node, doc, selectors) {
        out.push(b);
    }
    for child in &b.children {
        find_all_rec(child, doc, selectors, out);
    }
}

// ──────────────── query_all ────────────────

/// Returns all [`NodeId`]s in the document that match `sel`.
///
/// Traverses the entire DOM tree (not just the layout tree), so inline elements
/// and other nodes without a dedicated [`LayoutBox`] are included. Non-element
/// nodes (text, comments, processing instructions) never match any selector.
///
/// Implements `document.querySelectorAll` semantics. Returns an empty Vec when
/// `sel` is empty, all selectors are invalid, or no node matches.
pub fn query_all(doc: &Document, sel: &str) -> Vec<NodeId> {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    query_all_rec(doc, doc.root(), &selectors, &mut out);
    out
}

fn query_all_rec(
    doc: &Document,
    id: NodeId,
    selectors: &[ComplexSelector],
    out: &mut Vec<NodeId>,
) {
    // matches_complex returns false for non-element nodes internally.
    if node_matches(id, doc, selectors) {
        out.push(id);
    }
    for &child in &doc.get(id).children.clone() {
        query_all_rec(doc, child, selectors, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;

    fn layout_tree(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let tree = crate::layout(&doc, &sheet, Size::new(1024.0, 600.0));
        (doc, tree)
    }

    #[test]
    fn find_by_tag() {
        let (doc, tree) = layout_tree("<div>hello</div>", "");
        assert!(find_box_by_selector(&tree, &doc, "div").is_some());
    }

    #[test]
    fn find_by_id() {
        let (doc, tree) = layout_tree(r#"<div id="main">text</div>"#, "");
        let found = find_box_by_selector(&tree, &doc, "#main");
        assert!(found.is_some());
    }

    #[test]
    fn find_by_class() {
        let (doc, tree) = layout_tree(r#"<div class="container active">x</div>"#, "");
        assert!(find_box_by_selector(&tree, &doc, ".container").is_some());
        assert!(find_box_by_selector(&tree, &doc, ".active").is_some());
    }

    #[test]
    fn find_miss_returns_none() {
        let (doc, tree) = layout_tree("<div>text</div>", "");
        assert!(find_box_by_selector(&tree, &doc, "#nonexistent").is_none());
    }

    #[test]
    fn empty_selector_returns_none() {
        let (doc, tree) = layout_tree("<div>text</div>", "");
        assert!(find_box_by_selector(&tree, &doc, "").is_none());
    }

    #[test]
    fn find_nested_block() {
        // Block-level elements get their own LayoutBox and are findable by selector.
        let (doc, tree) = layout_tree(
            r#"<div><div id="target">inner</div></div>"#,
            "",
        );
        assert!(find_box_by_selector(&tree, &doc, "#target").is_some());
    }

    #[test]
    fn inline_elements_not_in_layout_tree() {
        // Inline elements (<span>, <a>, etc.) are merged into anonymous InlineRun
        // boxes in Phase 0 and do NOT get a dedicated LayoutBox. find_box_by_selector
        // returns None for them — this is a documented Phase 0 limitation.
        let (doc, tree) = layout_tree(
            r#"<div><span id="inline-target">text</span></div>"#,
            "",
        );
        assert!(find_box_by_selector(&tree, &doc, "#inline-target").is_none());
    }

    #[test]
    fn comma_selector_matches_either() {
        let (doc, tree) = layout_tree(r#"<div id="foo">x</div>"#, "");
        assert!(find_box_by_selector(&tree, &doc, "#bar, #foo").is_some());
    }

    #[test]
    fn computed_style_returns_snapshot() {
        let (doc, tree) = layout_tree(r#"<div id="x">text</div>"#, "");
        let snap = computed_style_by_selector(&tree, &doc, "#x");
        assert!(snap.is_some());
    }

    #[test]
    fn computed_style_reflects_css() {
        let (doc, tree) = layout_tree(
            r#"<div id="box">text</div>"#,
            "#box { opacity: 0.5; }",
        );
        let snap = computed_style_by_selector(&tree, &doc, "#box").unwrap();
        assert!((snap.opacity - 0.5).abs() < 0.001);
    }

    #[test]
    fn find_all_returns_multiple() {
        let (doc, tree) = layout_tree(
            "<div class=\"item\">a</div><div class=\"item\">b</div>",
            "",
        );
        let all = find_all_by_selector(&tree, &doc, ".item");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn compound_selector_tag_and_class() {
        let (doc, tree) = layout_tree(r#"<div class="hero">x</div>"#, "");
        assert!(find_box_by_selector(&tree, &doc, "div.hero").is_some());
        assert!(find_box_by_selector(&tree, &doc, "span.hero").is_none());
    }

    #[test]
    fn descendant_combinator() {
        let (doc, tree) = layout_tree(
            r#"<section><p id="inner">text</p></section>"#,
            "",
        );
        assert!(find_box_by_selector(&tree, &doc, "section p").is_some());
        assert!(find_box_by_selector(&tree, &doc, "section #inner").is_some());
    }

    #[test]
    fn find_all_empty_for_no_match() {
        let (doc, tree) = layout_tree("<p>text</p>", "");
        assert!(find_all_by_selector(&tree, &doc, "h1").is_empty());
    }

    #[test]
    fn layout_box_method_find_descendant() {
        let (doc, tree) = layout_tree(
            r#"<div class="container"><p id="target">text</p></div>"#,
            "",
        );
        let found = tree.find_descendant_by_selector(&doc, "#target");
        assert!(found.is_some());
    }

    #[test]
    fn layout_box_method_find_all_descendants() {
        let (doc, tree) = layout_tree(
            "<div><p class=\"item\">a</p><p class=\"item\">b</p></div>",
            "",
        );
        let all = tree.find_all_descendants_by_selector(&doc, ".item");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn layout_box_method_style_snapshot() {
        let (doc, tree) = layout_tree(
            r#"<div id="box">text</div>"#,
            "#box { opacity: 0.5; }",
        );
        let found = tree.find_descendant_by_selector(&doc, "#box");
        assert!(found.is_some());
        let snap = found.unwrap().style_snapshot();
        assert!((snap.opacity - 0.5).abs() < 0.001);
    }
}
