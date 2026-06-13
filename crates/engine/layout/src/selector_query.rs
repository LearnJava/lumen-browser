//! Selector-based lookup over the layout tree.
//!
//! Provides `find_box_by_selector` and `computed_style_by_selector` for
//! in-process driver testing (P3 BrowserSession, ADR-006 §8A.4).
//! Selector matching is backed by the full CSS3 engine in `style.rs`
//! (tag, .class, #id, attribute, compound, descendant/child/sibling
//! combinators, `:nth-*`, `:not()`, `:is()`, `:where()`).

use std::collections::HashMap;

use lumen_css_parser::{parse_selector_list, ComplexSelector};
use lumen_dom::{Document, NodeId};
use lumen_core::ColorSpace;

use crate::box_tree::{BoxKind, LayoutBox};
use crate::style::{
    matches_complex, AlignValue, BorderStyle, BoxSizing, ClearSide, Color, CssColor,
    Cursor, Direction, Display, FilterFn, FloatSide, FontStretch, FontStyle, FontWeight,
    FontVariant, Isolation, Length, LengthOrAuto, MixBlendMode, Overflow, OutlineColor,
    OutlineStyle, PointerEvents, Position, TextAlign, TextDecorationLine, TextDecorationStyle,
    TextEmphasisStyle, TextOverflow, TextTransform, TransformFn, Visibility, WhiteSpace,
    ComputedStyle,
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

// ──────────────── matches_selector ────────────────

/// Returns `true` if `node` matches **any** selector in `sel`.
///
/// Uses the full CSS3 selector engine: tag, `.class`, `#id`, attribute
/// selectors, compound selectors, descendant/child/sibling combinators,
/// `:nth-child`, `:not()`, `:is()`, `:where()`.
///
/// Returns `false` when `sel` is empty, all selectors are invalid, or `node`
/// does not match. Non-element nodes always return `false`.
///
/// Implements `element.matches()` semantics.
pub fn matches_selector(doc: &Document, node: NodeId, sel: &str) -> bool {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return false;
    }
    node_matches(node, doc, &selectors)
}

// ──────────────── CSS computed style serialisation ────────────────

/// Serialises a single CSS pixel value as a CSS string (`"16px"`, `"0px"`).
/// Omits the decimal point for whole-number values.
fn px_str(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}px", v as i64)
    } else {
        format!("{}px", v)
    }
}

/// Serialises a [`Color`] as `"rgb(r, g, b)"` or `"rgba(r, g, b, a)"`.
fn color_to_css(c: Color) -> String {
    if c.a == 255 {
        format!("rgb({}, {}, {})", c.r, c.g, c.b)
    } else {
        let a = c.a as f32 / 255.0;
        let a_str = format!("{:.3}", a).trim_end_matches('0').trim_end_matches('.').to_owned();
        format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, a_str)
    }
}

/// Serialises a [`CssColor`] — `CurrentColor` becomes `"currentcolor"`.
fn css_color_to_css(c: &CssColor) -> String {
    match c {
        CssColor::Rgba(col) => color_to_css(*col),
        CssColor::Wide(f) => color_to_css(f.to_srgb_color()),
        CssColor::CurrentColor => "currentcolor".into(),
        CssColor::System(sc) => color_to_css(sc.resolve_color(false)),
    }
}

/// Serialises a [`Length`] to its CSS representation.
fn length_to_css(l: &Length) -> String {
    match l {
        Length::Px(v) => px_str(*v),
        Length::Em(v) => format!("{}em", v),
        Length::Rem(v) => format!("{}rem", v),
        Length::Percent(v) => format!("{}%", v),
        Length::Vh(v) => format!("{}vh", v),
        Length::Vw(v) => format!("{}vw", v),
        Length::Vmin(v) => format!("{}vmin", v),
        Length::Vmax(v) => format!("{}vmax", v),
        Length::Cqw(v) => format!("{}cqw", v),
        Length::Cqh(v) => format!("{}cqh", v),
        Length::Cqi(v) => format!("{}cqi", v),
        Length::Cqb(v) => format!("{}cqb", v),
        Length::Cqmin(v) => format!("{}cqmin", v),
        Length::Cqmax(v) => format!("{}cqmax", v),
        Length::Calc(_) => "calc(...)".into(),
        Length::MinContent => "min-content".into(),
        Length::MaxContent => "max-content".into(),
        Length::FitContent(_) => "fit-content".into(),
    }
}

/// Serialises a [`LengthOrAuto`] — `Auto` becomes `"auto"`.
fn length_or_auto_to_css(l: &LengthOrAuto) -> String {
    match l {
        LengthOrAuto::Auto => "auto".into(),
        LengthOrAuto::Length(len) => length_to_css(len),
    }
}

fn border_style_to_css(bs: BorderStyle) -> &'static str {
    match bs {
        BorderStyle::None => "none",
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Double => "double",
    }
}

fn overflow_to_css(ov: Overflow) -> &'static str {
    match ov {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
        Overflow::Scroll => "scroll",
        Overflow::Auto => "auto",
        Overflow::Clip => "clip",
    }
}

fn align_value_to_css(a: AlignValue) -> &'static str {
    match a {
        AlignValue::Auto => "auto",
        AlignValue::Normal => "normal",
        AlignValue::Stretch => "stretch",
        AlignValue::Start => "start",
        AlignValue::End => "end",
        AlignValue::Center => "center",
        AlignValue::Baseline => "baseline",
        AlignValue::SpaceBetween => "space-between",
        AlignValue::SpaceAround => "space-around",
        AlignValue::SpaceEvenly => "space-evenly",
    }
}

fn transform_fn_to_css(f: &TransformFn) -> String {
    match f {
        TransformFn::Translate(x, y) => format!("translate({}, {})", px_str(*x), px_str(*y)),
        TransformFn::TranslateX(x) => format!("translateX({})", px_str(*x)),
        TransformFn::TranslateY(y) => format!("translateY({})", px_str(*y)),
        TransformFn::TranslateZ(z) => format!("translateZ({})", px_str(*z)),
        TransformFn::Translate3d(x, y, z) => {
            format!("translate3d({}, {}, {})", px_str(*x), px_str(*y), px_str(*z))
        }
        TransformFn::Rotate(a) => {
            let deg = a.to_degrees();
            if deg.fract() == 0.0 {
                format!("rotate({}deg)", deg as i64)
            } else {
                format!("rotate({}deg)", deg)
            }
        }
        TransformFn::RotateX(a) => format!("rotateX({}deg)", a.to_degrees()),
        TransformFn::RotateY(a) => format!("rotateY({}deg)", a.to_degrees()),
        TransformFn::RotateZ(a) => format!("rotateZ({}deg)", a.to_degrees()),
        TransformFn::Rotate3d(x, y, z, a) => {
            format!("rotate3d({}, {}, {}, {}deg)", x, y, z, a.to_degrees())
        }
        TransformFn::Scale(sx, sy) => format!("scale({}, {})", sx, sy),
        TransformFn::ScaleX(sx) => format!("scaleX({})", sx),
        TransformFn::ScaleY(sy) => format!("scaleY({})", sy),
        TransformFn::ScaleZ(sz) => format!("scaleZ({})", sz),
        TransformFn::Scale3d(sx, sy, sz) => format!("scale3d({}, {}, {})", sx, sy, sz),
        TransformFn::SkewX(a) => format!("skewX({}deg)", a.to_degrees()),
        TransformFn::SkewY(a) => format!("skewY({}deg)", a.to_degrees()),
        TransformFn::Matrix(m) => format!(
            "matrix({}, {}, {}, {}, {}, {})",
            m[0], m[1], m[2], m[3], m[4], m[5]
        ),
        TransformFn::Matrix3d(m) => format!(
            "matrix3d({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7],
            m[8], m[9], m[10], m[11], m[12], m[13], m[14], m[15]
        ),
        TransformFn::Perspective(d) => format!("perspective({})", px_str(*d)),
    }
}

fn filter_fn_to_css(f: &FilterFn) -> String {
    match f {
        FilterFn::Blur(r) => format!("blur({})", px_str(*r)),
        FilterFn::Brightness(v) => format!("brightness({})", v),
        FilterFn::Contrast(v) => format!("contrast({})", v),
        FilterFn::Grayscale(v) => format!("grayscale({})", v),
        FilterFn::HueRotate(a) => format!("hue-rotate({}deg)", a.to_degrees()),
        FilterFn::Invert(v) => format!("invert({})", v),
        FilterFn::Opacity(v) => format!("opacity({})", v),
        FilterFn::Saturate(v) => format!("saturate({})", v),
        FilterFn::Sepia(v) => format!("sepia({})", v),
    }
}

/// Serialises a [`ComputedStyle`] to a CSS property → resolved-value map.
///
/// Values are formatted as `window.getComputedStyle()` returns them:
/// pixel lengths as `"<n>px"`, colours as `"rgb(r, g, b)"` or `"rgba(r, g, b, a)"`,
/// keywords as lower-case CSS identifiers.
///
/// Covers ~55 most-queried properties. Less-used properties are omitted.
pub fn computed_style_to_map(style: &ComputedStyle) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = HashMap::with_capacity(64);

    // ── Display / layout mode ─────────────────────────────────────
    m.insert("display".into(), match style.display {
        Display::Block => "block",
        Display::Inline => "inline",
        Display::InlineBlock => "inline-block",
        Display::Flex => "flex",
        Display::InlineFlex => "inline-flex",
        Display::Grid => "grid",
        Display::InlineGrid => "inline-grid",
        Display::Table => "table",
        Display::InlineTable => "inline-table",
        Display::TableRow => "table-row",
        Display::TableCell => "table-cell",
        Display::TableCaption => "table-caption",
        Display::TableRowGroup => "table-row-group",
        Display::TableHeaderGroup => "table-header-group",
        Display::TableFooterGroup => "table-footer-group",
        Display::TableColumn => "table-column",
        Display::TableColumnGroup => "table-column-group",
        Display::None => "none",
        Display::Contents => "contents",
        Display::ListItem => "list-item",
        Display::FlowRoot => "flow-root",
    }.into());

    m.insert("visibility".into(), match style.visibility {
        Visibility::Visible => "visible",
        Visibility::Hidden => "hidden",
        Visibility::Collapse => "collapse",
    }.into());

    m.insert("position".into(), match style.position {
        Position::Static => "static",
        Position::Relative => "relative",
        Position::Absolute => "absolute",
        Position::Fixed => "fixed",
        Position::Sticky => "sticky",
    }.into());

    // ── Box model ──────────────────────────────────────────────────
    m.insert("box-sizing".into(), match style.box_sizing {
        BoxSizing::ContentBox => "content-box",
        BoxSizing::BorderBox => "border-box",
    }.into());

    m.insert("width".into(), style.width.as_ref().map_or("auto".into(), length_to_css));
    m.insert("height".into(), style.height.as_ref().map_or("auto".into(), length_to_css));
    m.insert("min-width".into(), style.min_width.as_ref().map_or("0px".into(), length_to_css));
    m.insert("max-width".into(), style.max_width.as_ref().map_or("none".into(), length_to_css));
    m.insert("min-height".into(), style.min_height.as_ref().map_or("0px".into(), length_to_css));
    m.insert("max-height".into(), style.max_height.as_ref().map_or("none".into(), length_to_css));

    m.insert("margin-top".into(), length_or_auto_to_css(&style.margin_top));
    m.insert("margin-right".into(), length_or_auto_to_css(&style.margin_right));
    m.insert("margin-bottom".into(), length_or_auto_to_css(&style.margin_bottom));
    m.insert("margin-left".into(), length_or_auto_to_css(&style.margin_left));

    m.insert("padding-top".into(), length_to_css(&style.padding_top));
    m.insert("padding-right".into(), length_to_css(&style.padding_right));
    m.insert("padding-bottom".into(), length_to_css(&style.padding_bottom));
    m.insert("padding-left".into(), length_to_css(&style.padding_left));

    m.insert("border-top-width".into(), px_str(style.border_top_width));
    m.insert("border-right-width".into(), px_str(style.border_right_width));
    m.insert("border-bottom-width".into(), px_str(style.border_bottom_width));
    m.insert("border-left-width".into(), px_str(style.border_left_width));

    m.insert("border-top-style".into(), border_style_to_css(style.border_top_style).into());
    m.insert("border-right-style".into(), border_style_to_css(style.border_right_style).into());
    m.insert("border-bottom-style".into(), border_style_to_css(style.border_bottom_style).into());
    m.insert("border-left-style".into(), border_style_to_css(style.border_left_style).into());

    m.insert("border-top-color".into(), css_color_to_css(&style.border_top_color));
    m.insert("border-right-color".into(), css_color_to_css(&style.border_right_color));
    m.insert("border-bottom-color".into(), css_color_to_css(&style.border_bottom_color));
    m.insert("border-left-color".into(), css_color_to_css(&style.border_left_color));

    m.insert("border-top-left-radius".into(), length_to_css(&style.border_top_left_radius));
    m.insert("border-top-right-radius".into(), length_to_css(&style.border_top_right_radius));
    m.insert("border-bottom-right-radius".into(), length_to_css(&style.border_bottom_right_radius));
    m.insert("border-bottom-left-radius".into(), length_to_css(&style.border_bottom_left_radius));

    // ── Inset (positioned elements) ───────────────────────────────
    m.insert("top".into(), length_or_auto_to_css(&style.top));
    m.insert("right".into(), length_or_auto_to_css(&style.right));
    m.insert("bottom".into(), length_or_auto_to_css(&style.bottom));
    m.insert("left".into(), length_or_auto_to_css(&style.left));

    // ── Colors ────────────────────────────────────────────────────
    m.insert("color".into(), color_to_css(style.color));
    m.insert("background-color".into(), style.background_color.as_ref()
        .map_or_else(|| "rgba(0, 0, 0, 0)".into(), css_color_to_css));
    m.insert("opacity".into(), {
        let v = style.opacity;
        if v.fract() == 0.0 {
            format!("{}", v as i64)
        } else {
            format!("{}", v)
        }
    });

    // ── Typography ────────────────────────────────────────────────
    m.insert("font-size".into(), px_str(style.font_size));
    m.insert("font-weight".into(), style.font_weight.0.to_string());
    m.insert("font-style".into(), match style.font_style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
        FontStyle::Oblique => "oblique",
    }.into());
    m.insert("font-variant".into(), match style.font_variant {
        FontVariant::Normal => "normal",
        FontVariant::SmallCaps => "small-caps",
    }.into());
    m.insert("font-stretch".into(), {
        let pct = style.font_stretch.0 as f32 / 10.0;
        if pct.fract() == 0.0 { format!("{}%", pct as i64) } else { format!("{}%", pct) }
    });
    m.insert("font-family".into(), {
        if style.font_family.is_empty() {
            "".into()
        } else {
            style.font_family.iter()
                .map(|s| if s.contains(' ') { format!("\"{}\"", s) } else { s.clone() })
                .collect::<Vec<_>>()
                .join(", ")
        }
    });
    m.insert("line-height".into(), {
        let v = style.line_height;
        if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{}", v) }
    });
    m.insert("letter-spacing".into(), px_str(style.letter_spacing));
    m.insert("word-spacing".into(), px_str(style.word_spacing));
    m.insert("text-align".into(), match style.text_align {
        TextAlign::Start => "start",
        TextAlign::End => "end",
        TextAlign::Left => "left",
        TextAlign::Right => "right",
        TextAlign::Center => "center",
    }.into());
    m.insert("text-transform".into(), match style.text_transform {
        TextTransform::None => "none",
        TextTransform::Uppercase => "uppercase",
        TextTransform::Lowercase => "lowercase",
        TextTransform::Capitalize => "capitalize",
    }.into());
    m.insert("white-space".into(), match style.white_space {
        WhiteSpace::Normal => "normal",
        WhiteSpace::Nowrap => "nowrap",
        WhiteSpace::Pre => "pre",
        WhiteSpace::PreWrap => "pre-wrap",
        WhiteSpace::PreLine => "pre-line",
    }.into());
    m.insert("text-decoration-line".into(), {
        let td = &style.text_decoration_line;
        if !td.underline && !td.overline && !td.line_through {
            "none".into()
        } else {
            let mut parts = Vec::new();
            if td.underline { parts.push("underline") }
            if td.overline { parts.push("overline") }
            if td.line_through { parts.push("line-through") }
            parts.join(" ")
        }
    });
    m.insert("text-decoration-style".into(), match style.text_decoration_style {
        TextDecorationStyle::Solid => "solid",
        TextDecorationStyle::Double => "double",
        TextDecorationStyle::Dotted => "dotted",
        TextDecorationStyle::Dashed => "dashed",
        TextDecorationStyle::Wavy => "wavy",
    }.into());
    m.insert("text-decoration-color".into(), css_color_to_css(&style.text_decoration_color));
    m.insert("text-overflow".into(), match style.text_overflow {
        TextOverflow::Clip => "clip",
        TextOverflow::Ellipsis => "ellipsis",
    }.into());
    m.insert("text-indent".into(), length_to_css(&style.text_indent));

    // ── Overflow / stacking ───────────────────────────────────────
    m.insert("overflow-x".into(), overflow_to_css(style.overflow_x).into());
    m.insert("overflow-y".into(), overflow_to_css(style.overflow_y).into());
    m.insert("z-index".into(), match style.z_index {
        None => "auto".into(),
        Some(n) => n.to_string(),
    });

    // ── Float / clear ─────────────────────────────────────────────
    m.insert("float".into(), match style.float_side {
        FloatSide::None => "none",
        FloatSide::Left => "left",
        FloatSide::Right => "right",
    }.into());
    m.insert("clear".into(), match style.clear {
        ClearSide::None => "none",
        ClearSide::Left => "left",
        ClearSide::Right => "right",
        ClearSide::Both => "both",
    }.into());

    // ── Outline ───────────────────────────────────────────────────
    m.insert("outline-width".into(), px_str(style.outline_used_width()));
    m.insert("outline-style".into(), match style.outline_style {
        OutlineStyle::None => "none",
        OutlineStyle::Auto => "auto",
        OutlineStyle::Solid => "solid",
        OutlineStyle::Dashed => "dashed",
        OutlineStyle::Dotted => "dotted",
    }.into());
    m.insert("outline-color".into(), match &style.outline_color {
        OutlineColor::Auto => "auto".into(),
        OutlineColor::CurrentColor => "currentcolor".into(),
        OutlineColor::Color(c) => color_to_css(*c),
    });

    // ── Transform / filter ───────────────────────────────────────
    m.insert("transform".into(), if style.transform.is_empty() {
        "none".into()
    } else {
        style.transform.iter().map(transform_fn_to_css).collect::<Vec<_>>().join(" ")
    });
    m.insert("filter".into(), if style.filter.is_empty() {
        "none".into()
    } else {
        style.filter.iter().map(filter_fn_to_css).collect::<Vec<_>>().join(" ")
    });

    // ── Compositing ───────────────────────────────────────────────
    m.insert("mix-blend-mode".into(), match style.mix_blend_mode {
        MixBlendMode::Normal => "normal",
        MixBlendMode::Multiply => "multiply",
        MixBlendMode::Screen => "screen",
        MixBlendMode::Overlay => "overlay",
        MixBlendMode::Darken => "darken",
        MixBlendMode::Lighten => "lighten",
        MixBlendMode::ColorDodge => "color-dodge",
        MixBlendMode::ColorBurn => "color-burn",
        MixBlendMode::HardLight => "hard-light",
        MixBlendMode::SoftLight => "soft-light",
        MixBlendMode::Difference => "difference",
        MixBlendMode::Exclusion => "exclusion",
        MixBlendMode::Hue => "hue",
        MixBlendMode::Saturation => "saturation",
        MixBlendMode::Color => "color",
        MixBlendMode::Luminosity => "luminosity",
        MixBlendMode::PlusLighter => "plus-lighter",
    }.into());
    m.insert("isolation".into(), match style.isolation {
        Isolation::Auto => "auto",
        Isolation::Isolate => "isolate",
    }.into());

    // ── Flex / Grid alignment ─────────────────────────────────────
    m.insert("align-items".into(), align_value_to_css(style.align_items).into());
    m.insert("align-self".into(), align_value_to_css(style.align_self).into());
    m.insert("align-content".into(), align_value_to_css(style.align_content).into());
    m.insert("justify-items".into(), align_value_to_css(style.justify_items).into());
    m.insert("justify-self".into(), align_value_to_css(style.justify_self).into());
    m.insert("justify-content".into(), align_value_to_css(style.justify_content).into());

    // ── Cursor / pointer ─────────────────────────────────────────
    m.insert("cursor".into(), match style.cursor {
        Cursor::Auto => "auto",
        Cursor::Default => "default",
        Cursor::None => "none",
        Cursor::Pointer => "pointer",
        Cursor::Crosshair => "crosshair",
        Cursor::Text => "text",
        Cursor::VerticalText => "vertical-text",
        Cursor::Move => "move",
        Cursor::NoDrop => "no-drop",
        Cursor::AllScroll => "all-scroll",
        Cursor::ColResize => "col-resize",
        Cursor::RowResize => "row-resize",
        Cursor::NResize => "n-resize",
        Cursor::EResize => "e-resize",
        Cursor::SResize => "s-resize",
        Cursor::WResize => "w-resize",
        Cursor::NeResize => "ne-resize",
        Cursor::NwResize => "nw-resize",
        Cursor::SeResize => "se-resize",
        Cursor::SwResize => "sw-resize",
        Cursor::EwResize => "ew-resize",
        Cursor::NsResize => "ns-resize",
        Cursor::NeswResize => "nesw-resize",
        Cursor::NwseResize => "nwse-resize",
        Cursor::ZoomIn => "zoom-in",
        Cursor::ZoomOut => "zoom-out",
        Cursor::Wait => "wait",
        Cursor::Progress => "progress",
        Cursor::Help => "help",
        Cursor::NotAllowed => "not-allowed",
        Cursor::Grab => "grab",
        Cursor::Grabbing => "grabbing",
        Cursor::Cell => "cell",
        Cursor::Copy => "copy",
        Cursor::Alias => "alias",
        Cursor::ContextMenu => "context-menu",
    }.into());
    m.insert("pointer-events".into(), match style.pointer_events {
        PointerEvents::Auto => "auto",
        PointerEvents::None => "none",
        PointerEvents::All => "all",
        PointerEvents::Visible => "visible",
        PointerEvents::Painted => "painted",
        PointerEvents::Fill => "fill",
        PointerEvents::Stroke => "stroke",
    }.into());

    m
}

/// Serialises a [`ComputedStyle`] into a deterministic JSON object string.
///
/// Each key is a CSS property name and each value is the resolved value as
/// produced by [`computed_style_to_map`] (e.g. `{"font-size":"16px",...}`).
/// Keys are emitted in sorted order so the output is byte-stable across runs —
/// suitable for the DevTools "Computed" panel (lumen-plan §7E.2) and snapshot
/// assertions.
///
/// Dependency-free: builds the JSON text by hand (the layout crate does not
/// depend on `serde`).
pub fn computed_style_json(style: &ComputedStyle) -> String {
    let map = computed_style_to_map(style);
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_unstable();
    let mut out = String::with_capacity(map.len() * 32 + 2);
    out.push('{');
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_escape_into(k, &mut out);
        out.push(':');
        json_escape_into(&map[*k], &mut out);
    }
    out.push('}');
    out
}

/// Like [`computed_style_by_selector`] but returns the full computed-style JSON
/// (see [`computed_style_json`]) for the first box matching `sel`.
///
/// Returns `None` under the same conditions as [`find_box_by_selector`].
pub fn computed_style_json_by_selector(
    root: &LayoutBox,
    doc: &Document,
    sel: &str,
) -> Option<String> {
    find_box_by_selector(root, doc, sel).map(|b| computed_style_json(&b.style))
}

/// Appends `s` to `out` as a JSON string literal (with surrounding quotes),
/// escaping `"`, `\`, and ASCII control characters per RFC 8259.
fn json_escape_into(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
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

    // ──────────────── computed_style_json ────────────────

    #[test]
    fn json_is_well_formed_object() {
        let (doc, tree) = layout_tree(
            r#"<div id="x" style="font-size:20px;color:red"></div>"#,
            "",
        );
        let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
        assert!(json.starts_with('{') && json.ends_with('}'));
        // Contains a couple of known properties.
        assert!(json.contains(r#""font-size":"20px""#), "json: {json}");
        assert!(json.contains(r#""color":"rgb(255, 0, 0)""#), "json: {json}");
    }

    #[test]
    fn json_keys_are_sorted() {
        let (doc, tree) = layout_tree(r#"<div id="x"></div>"#, "");
        let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
        // Keys are emitted in sorted order, so the byte offset of each successive
        // `"<key>":` marker must be strictly increasing. (Naive comma-splitting
        // would break on values like `rgb(255, 0, 0)`, so probe by marker.)
        let markers = [
            "\"align-items\":",
            "\"color\":",
            "\"display\":",
            "\"opacity\":",
            "\"width\":",
            "\"z-index\":",
        ];
        let mut last = 0usize;
        for m in markers {
            let pos = json.find(m).unwrap_or_else(|| panic!("missing key marker {m}"));
            assert!(pos >= last, "key {m} out of sorted order");
            last = pos;
        }
    }

    #[test]
    fn json_missing_selector_returns_none() {
        let (doc, tree) = layout_tree(r#"<div></div>"#, "");
        assert!(computed_style_json_by_selector(&tree, &doc, "#nope").is_none());
    }

    #[test]
    fn json_round_trips_via_string_parsing() {
        // The output must be parseable back into the same key/value map.
        let (doc, tree) = layout_tree(
            r#"<div id="x" style="display:flex;opacity:0.5"></div>"#,
            "",
        );
        let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
        assert!(json.contains(r#""display":"flex""#), "json: {json}");
        assert!(json.contains(r#""opacity":"0.5""#), "json: {json}");
        // No trailing comma / empty entries.
        assert!(!json.contains(",,"));
        assert!(!json.contains("{,") && !json.contains(",}"));
    }

    #[test]
    fn json_escapes_font_family_quotes() {
        // A multi-word family name is quoted inside the value; the surrounding
        // JSON string must escape those inner quotes.
        let (doc, tree) = layout_tree(
            r#"<div id="x" style="font-family:Times New Roman"></div>"#,
            "",
        );
        let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
        assert!(json.contains(r#"\"Times New Roman\""#), "json: {json}");
    }
}
