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
use lumen_core::{ColorSpace, Size};

use crate::box_tree::{BoxKind, LayoutBox};
use crate::ruby::{RubyAlign, RubyMerge, RubyPosition};
use crate::style::{
    matches_complex, AlignValue, AnimationDirection, AnimationFillMode, AnimationPlayState,
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin,
    BackgroundRepeat, BackgroundSize, BgSizeAxis, BorderStyle, BoxShadow, BoxSizing,
    ClearSide, Color, ColorScheme,
    ContainFlags, Content, ContentItem, ContentVisibility,
    CssColor, CssContinue,
    Cursor, Direction, Display, FillRule, FilterFn, FloatSide, ForcedColorAdjust, FontStretch,
    FontStyle, FontWeight,
    FontVariantCaps, FontVariantEmoji, ImageRendering, Isolation, IterationCount, Length,
    LengthOrAuto,
    MixBlendMode, ObjectFit, ObjectPosition, Overflow, overflow_clip_margin_serialize,
    OutlineColor, OverscrollBehavior,
    OutlineStyle, PointerEvents, Position, PositionComponent, PrintColorAdjust, Quotes,
    ScrollbarGutter, ScrollbarWidth, StepPosition, StrokeLinecap, StrokeLinejoin, SvgPaint, TextAlign,
    TextDecorationLine, TextDecorationStyle,
    TextEmphasisStyle, TextOverflow, TextShadow, TextTransform, TimingFunction, TransformFn,
    VerticalAlign, Visibility, WebkitBoxOrient, WhiteSpace,
    WhiteSpaceCollapse, WritingMode,
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
        ComputedStyleSnapshot::from(&*self.style)
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
    /// CSS `font-variant-caps` (CSS Fonts L4 §6.2, весь набор значений). Inherited.
    pub font_variant_caps: FontVariantCaps,
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
            font_variant_caps: s.font_variant_caps,
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

// ──────────────── find_first_dom_node_by_selector ────────────────

/// Returns the first DOM node matching `sel`, walking the DOM tree directly
/// instead of the layout tree — so it finds nodes `find_box_by_selector`
/// cannot (`display: none`, and anything else the box tree skips).
///
/// Uses the same full CSS3 selector engine as `find_box_by_selector`
/// (`node_matches`/`matches_complex`), so the two never disagree on what
/// counts as a match — needed by introspection tools like `explain_element`
/// (DEVX-10) that must distinguish "not in the DOM" from "in the DOM but
/// excluded from layout".
pub fn find_first_dom_node_by_selector(doc: &Document, sel: &str) -> Option<lumen_dom::NodeId> {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return None;
    }
    find_dom_rec(doc, doc.root(), &selectors)
}

fn find_dom_rec(
    doc: &Document,
    id: lumen_dom::NodeId,
    selectors: &[ComplexSelector],
) -> Option<lumen_dom::NodeId> {
    if node_matches(id, doc, selectors) {
        return Some(id);
    }
    for &child in &doc.get(id).children {
        if let Some(found) = find_dom_rec(doc, child, selectors) {
            return Some(found);
        }
    }
    None
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
    find_box_by_selector(root, doc, sel).map(|b| ComputedStyleSnapshot::from(&*b.style))
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

/// Returns all [`NodeId`]s among `start`'s descendants (excluding `start`
/// itself) that match `sel`, in document order.
///
/// Implements `Element`/`DocumentFragment`/`ShadowRoot` `querySelector(All)`
/// scoping (DOM LS §4.2.6), which searches only the subtree rooted at the
/// calling node — unlike [`query_all`], which always searches from the
/// document root and therefore never finds matches inside a detached
/// subtree (a node created but not yet attached to the document).
pub fn query_all_within(doc: &Document, start: NodeId, sel: &str) -> Vec<NodeId> {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &child in &doc.get(start).children.clone() {
        query_all_rec(doc, child, &selectors, &mut out);
    }
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

// ──────────────── query_all_scoped ────────────────

/// Returns all descendant [`NodeId`]s of `scope` (excluding `scope` itself)
/// that match `sel`, in document order.
///
/// Implements `Element`/`DocumentFragment`/`ShadowRoot.querySelector(All)`
/// scoping (DOM Parentnode §4.2.5): unlike [`query_all`], which always walks
/// the whole document tree from `doc.root()`, this only descends from
/// `scope`'s children — so it also finds matches inside a subtree that is
/// not (yet) attached to the document (`scope` has no ancestor path to
/// `doc.root()`). Returns an empty Vec when `sel` is empty/invalid or no
/// descendant matches.
pub fn query_all_scoped(doc: &Document, scope: NodeId, sel: &str) -> Vec<NodeId> {
    let selectors = parse_selector_list(sel);
    if selectors.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &child in &doc.get(scope).children.clone() {
        query_all_rec(doc, child, &selectors, &mut out);
    }
    out
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
///
/// `pub(crate)`: also reused by `style::parse::color::canonical_specified_color`
/// (BUG-465) for inline-`style` `<color>` reflection — same canonical CSSOM
/// §6.7.3 rgb()/rgba() serialization as `getComputedStyle()`, one source of
/// truth for the format.
pub(crate) fn color_to_css(c: Color) -> String {
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

/// Serialises `style.background_layers` for `getComputedStyle(...).getPropertyValue("background-image")`
/// (BUG-603 point 2 — needed so `background` presentational-hint values become
/// observable). Layers are comma-joined, topmost (index 0) first, matching
/// `background-color`/other multi-layer shorthand serialization order.
///
/// Only [`BackgroundImage::None`] and [`BackgroundImage::Url`] round-trip
/// exactly; the engine has no source text to reconstruct for the other three
/// variants (they're computed/generated, not stored verbatim), so those
/// serialise as `"none"` — a known gap, not a regression, since this key did
/// not exist in the map at all before this change.
fn background_layers_to_css(layers: &[BackgroundLayer]) -> String {
    if layers.is_empty() {
        return "none".into();
    }
    layers
        .iter()
        .map(|l| match &l.image {
            BackgroundImage::Url(s) => {
                format!("url(\"{}\")", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
            BackgroundImage::None
            | BackgroundImage::Gradient(_)
            | BackgroundImage::CrossFade { .. }
            | BackgroundImage::Paint(_) => "none".to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialises one axis of a `background-position`/`object-position` value —
/// `Px` as `"<n>px"`, `Percent` (stored as a `0.0..=1.0` fraction) as `"<n>%"`.
fn position_component_to_css(c: PositionComponent) -> String {
    match c {
        PositionComponent::Px(v) => px_str(v),
        PositionComponent::Percent(p) => {
            let pct = p * 100.0;
            if pct.fract() == 0.0 {
                format!("{}%", pct as i64)
            } else {
                format!("{}%", pct)
            }
        }
    }
}

/// Serialises `style.vertical_align` — CSS 2.1 §10.8.1 keywords as their
/// lowercase-hyphenated form, `Length`/`Percent` as `<n>px`/`<n>%` (the
/// latter stored as a raw percentage number, same convention as
/// [`length_to_css`]'s `Length::Percent`, not the `0.0..=1.0` fraction
/// [`PositionComponent`] uses).
fn vertical_align_to_css(va: &VerticalAlign) -> String {
    match va {
        VerticalAlign::Baseline => "baseline".into(),
        VerticalAlign::Sub => "sub".into(),
        VerticalAlign::Super => "super".into(),
        VerticalAlign::Top => "top".into(),
        VerticalAlign::TextTop => "text-top".into(),
        VerticalAlign::Middle => "middle".into(),
        VerticalAlign::Bottom => "bottom".into(),
        VerticalAlign::TextBottom => "text-bottom".into(),
        VerticalAlign::Length(px) => px_str(*px),
        VerticalAlign::Percent(p) => format!("{p}%"),
    }
}

/// Serialises `style.background_layers`' per-layer `position.x`/`.y` for the
/// standalone `background-position-x`/`-y` longhands (CSS Backgrounds L4
/// §3.5) — comma-joined, topmost (index 0) first, matching every other
/// multi-layer longhand's serialization order.
fn background_position_axis_to_css(
    layers: &[BackgroundLayer],
    axis: impl Fn(&BackgroundLayer) -> PositionComponent,
) -> String {
    if layers.is_empty() {
        return position_component_to_css(ObjectPosition::background_initial().x);
    }
    layers
        .iter()
        .map(|l| position_component_to_css(axis(l)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialises a [`Length`] to its CSS representation.
///
/// `pub(crate)`: also reused by `style::values::length::canonical_specified_length`
/// (CSSOM-2/BUG-484) for inline-`style` `<length-percentage>` reflection —
/// same canonical serialization as `getComputedStyle()`, one source of truth.
pub(crate) fn length_to_css(l: &Length) -> String {
    match l {
        Length::Px(v) => px_str(*v),
        Length::Em(v) => format!("{}em", v),
        Length::Rem(v) => format!("{}rem", v),
        Length::Ch(v) => format!("{}ch", v),
        Length::Ex(v) => format!("{}ex", v),
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
        Length::Calc(node) => crate::style::calc_node_to_css(node),
        Length::MinContent => "min-content".into(),
        Length::MaxContent => "max-content".into(),
        Length::FitContent(None) => "fit-content".into(),
        Length::FitContent(Some(arg)) => format!("fit-content({})", length_to_css(arg)),
    }
}

/// The length half of `overflow-clip-margin`'s computed-value serialization
/// (BUG-505 срез 4): a `Length::Calc` tree resolves to a plain px number
/// when its basis is fully known — em is always known at computed-value
/// time (`style.font_size`), and `%` was already rejected at parse time
/// (`parse_overflow_clip_margin_length`), so the only way `resolve` fails
/// here is an unresolvable relative unit this property's WPT coverage never
/// exercises (`ch`/`ex`/`cq*` outside a layout pass) — falls back to the
/// unresolved `calc(...)` text in that case. `Size::ZERO` stands in for the
/// viewport: `vh`/`vw`/`vmin`/`vmax` inside this property's `calc()` are
/// the same known Phase 0 gap as everywhere else `length_to_css` is used
/// for a computed value (no viewport threaded through this function).
/// Non-`Calc` lengths pass straight to [`length_to_css`], same as before
/// this slice. Returns the serialized text plus whether it is exactly zero,
/// for `overflow_clip_margin_serialize`'s elision rule.
fn overflow_clip_margin_computed_length(length: &Length, font_size: f32) -> (String, bool) {
    if let Length::Calc(node) = length
        && let Some(px) = node.resolve(font_size, None, Size::ZERO)
    {
        return (px_str(px), px == 0.0);
    }
    let is_zero = matches!(length, Length::Px(v) if *v == 0.0);
    (length_to_css(length), is_zero)
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

/// Quotes and escapes a raw string as a CSS `<string>` token — `"` and `\`
/// are backslash-escaped, matching the serialization CSSOM §6.7.2 asks for
/// on any property whose computed value contains free text (`content`,
/// `quotes`).
fn css_string_literal(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn mix_blend_mode_to_css(v: MixBlendMode) -> &'static str {
    match v {
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
    }
}

fn background_repeat_to_css(r: BackgroundRepeat) -> &'static str {
    match r {
        BackgroundRepeat::Repeat => "repeat",
        BackgroundRepeat::NoRepeat => "no-repeat",
        BackgroundRepeat::RepeatX => "repeat-x",
        BackgroundRepeat::RepeatY => "repeat-y",
        BackgroundRepeat::Round => "round",
        BackgroundRepeat::Space => "space",
    }
}

fn background_attachment_to_css(a: BackgroundAttachment) -> &'static str {
    match a {
        BackgroundAttachment::Scroll => "scroll",
        BackgroundAttachment::Fixed => "fixed",
        BackgroundAttachment::Local => "local",
    }
}

fn background_origin_to_css(o: BackgroundOrigin) -> &'static str {
    match o {
        BackgroundOrigin::BorderBox => "border-box",
        BackgroundOrigin::PaddingBox => "padding-box",
        BackgroundOrigin::ContentBox => "content-box",
    }
}

fn background_clip_to_css(c: BackgroundClip) -> &'static str {
    match c {
        BackgroundClip::BorderBox => "border-box",
        BackgroundClip::PaddingBox => "padding-box",
        BackgroundClip::ContentBox => "content-box",
        BackgroundClip::Text => "text",
    }
}

fn bg_size_axis_to_css(a: BgSizeAxis) -> String {
    match a {
        BgSizeAxis::Auto => "auto".into(),
        BgSizeAxis::Px(v) => px_str(v),
        BgSizeAxis::Percent(p) => {
            let pct = p * 100.0;
            if pct.fract() == 0.0 { format!("{}%", pct as i64) } else { format!("{}%", pct) }
        }
    }
}

fn background_size_to_css(s: BackgroundSize) -> String {
    match s {
        BackgroundSize::Auto => "auto".into(),
        BackgroundSize::Cover => "cover".into(),
        BackgroundSize::Contain => "contain".into(),
        BackgroundSize::Length(w, h) => format!("{} {}", bg_size_axis_to_css(w), bg_size_axis_to_css(h)),
    }
}

/// Serialises one keyword-valued per-layer background longhand across every
/// layer of `style.background_layers`, comma-joined topmost-first — the same
/// multi-layer serialization order every other background longhand in
/// [`computed_style_to_map`] uses (`background-image`/`background-position-x/y`).
/// An empty layer list (no `background-*` declared at all) falls back to the
/// property's own initial keyword, matching [`background_position_axis_to_css`]'s
/// empty-layers handling.
fn background_layer_field_to_css<T: Copy>(
    layers: &[BackgroundLayer],
    initial: T,
    extract: impl Fn(&BackgroundLayer) -> T,
    to_css: impl Fn(T) -> &'static str,
) -> String {
    if layers.is_empty() {
        return to_css(initial).into();
    }
    layers.iter().map(|l| to_css(extract(l)).to_string()).collect::<Vec<_>>().join(", ")
}

fn fill_rule_to_css(r: FillRule) -> &'static str {
    match r {
        FillRule::NonZero => "nonzero",
        FillRule::EvenOdd => "evenodd",
    }
}

fn stroke_linecap_to_css(c: StrokeLinecap) -> &'static str {
    match c {
        StrokeLinecap::Butt => "butt",
        StrokeLinecap::Round => "round",
        StrokeLinecap::Square => "square",
    }
}

fn stroke_linejoin_to_css(j: StrokeLinejoin) -> &'static str {
    match j {
        StrokeLinejoin::Miter => "miter",
        StrokeLinejoin::Round => "round",
        StrokeLinejoin::Bevel => "bevel",
    }
}

/// Serialises an [`SvgPaint`] value. `Url` stores only the bare fragment id
/// (the leading `#` is stripped at parse time, `style/apply/paint.rs::svg_paint_url_id`),
/// so it is re-added here to round-trip back to a valid `url(#id)` token.
fn svg_paint_to_css(p: &SvgPaint) -> String {
    match p {
        SvgPaint::None => "none".into(),
        SvgPaint::CurrentColor => "currentcolor".into(),
        SvgPaint::Color(c) => color_to_css(*c),
        SvgPaint::Url(id) => format!("url(#{id})"),
        // Computed-value serialization for an SVG paint server reference has
        // no source text to reconstruct from (same "computed, not stored
        // verbatim" gap as `background_layers_to_css`'s non-Url image variants).
        SvgPaint::Gradient(_) => "none".into(),
    }
}

fn step_position_to_css(p: StepPosition) -> &'static str {
    match p {
        StepPosition::JumpStart => "jump-start",
        StepPosition::JumpEnd => "jump-end",
        StepPosition::JumpNone => "jump-none",
        StepPosition::JumpBoth => "jump-both",
    }
}

fn timing_function_to_css(f: &TimingFunction) -> String {
    match f {
        TimingFunction::Linear => "linear".into(),
        TimingFunction::CubicBezier(a, b, c, d) => format!("cubic-bezier({a}, {b}, {c}, {d})"),
        TimingFunction::Steps(n, pos) => format!("steps({n}, {})", step_position_to_css(*pos)),
        TimingFunction::LinearStops(points) => {
            let body = points
                .iter()
                .map(|p| {
                    let out = p.output;
                    if out.fract() == 0.0 { format!("{}", out as i64) } else { format!("{out}") }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("linear({body})")
        }
    }
}

/// Comma-joins a `Vec<TimingFunction>` — `animation-timing-function` and
/// `transition-timing-function` both cycle a list across the sibling
/// `animation-name`/`transition-property` list (CSS Animations L1 §4.3,
/// CSS Transitions L1 §3), so the computed value is always the full list,
/// never a single function, even for a page that declared just one.
fn timing_function_list_to_css(v: &[TimingFunction]) -> String {
    if v.is_empty() {
        return timing_function_to_css(&TimingFunction::default());
    }
    v.iter().map(timing_function_to_css).collect::<Vec<_>>().join(", ")
}

/// Comma-joins a seconds list (`transition-duration`/`-delay`,
/// `animation-duration`/`-delay`) as `"<n>s"` tokens.
fn seconds_list_to_css(v: &[f32]) -> String {
    if v.is_empty() {
        return "0s".into();
    }
    v.iter()
        .map(|s| if s.fract() == 0.0 { format!("{}s", *s as i64) } else { format!("{s}s") })
        .collect::<Vec<_>>()
        .join(", ")
}

fn box_shadow_list_to_css(v: &[BoxShadow]) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.iter()
        .map(|s| {
            let color = s.color.map_or_else(|| "currentcolor".into(), color_to_css);
            let mut out = format!(
                "{} {} {} {} {color}",
                px_str(s.offset_x), px_str(s.offset_y), px_str(s.blur), px_str(s.spread),
            );
            if s.inset {
                out.push_str(" inset");
            }
            out
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn text_shadow_list_to_css(v: &[TextShadow]) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.iter()
        .map(|s| {
            let color = s.color.map_or_else(|| "currentcolor".into(), color_to_css);
            format!("{} {} {} {color}", px_str(s.offset_x), px_str(s.offset_y), px_str(s.blur))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialises one `content: ...` fragment (CSS Content L3 §3).
fn content_item_to_css(item: &ContentItem) -> String {
    match item {
        ContentItem::String(s) => css_string_literal(s),
        ContentItem::Attr(name) => format!("attr({name})"),
        ContentItem::Url(u) => format!("url({})", css_string_literal(u)),
        ContentItem::Counter { name, style } => match style {
            Some(s) => format!("counter({name}, {s})"),
            None => format!("counter({name})"),
        },
        ContentItem::Counters { name, separator, style } => match style {
            Some(s) => format!("counters({name}, {}, {s})", css_string_literal(separator)),
            None => format!("counters({name}, {})", css_string_literal(separator)),
        },
        ContentItem::OpenQuote => "open-quote".into(),
        ContentItem::CloseQuote => "close-quote".into(),
        ContentItem::NoOpenQuote => "no-open-quote".into(),
        ContentItem::NoCloseQuote => "no-close-quote".into(),
    }
}

fn content_to_css(c: &Content) -> String {
    match c {
        Content::Normal => "normal".into(),
        Content::None => "none".into(),
        Content::Items(items) => items.iter().map(content_item_to_css).collect::<Vec<_>>().join(" "),
    }
}

fn quotes_to_css(q: &Quotes) -> String {
    match q {
        Quotes::Auto => "auto".into(),
        Quotes::None => "none".into(),
        Quotes::Pairs(pairs) => pairs
            .iter()
            .flat_map(|(open, close)| [css_string_literal(open), css_string_literal(close)])
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Serialises the three properties [`crate::INLINE_SEGMENT_PROPERTIES`] names,
/// in the same string form [`computed_style_to_map`] uses for them.
///
/// Separate from that function rather than a filter over its result: this one
/// runs once per text node of the page on the layout path, so building the full
/// ~55-entry map only to throw away 52 entries would be the whole point of the
/// smaller entry, lost.
pub fn inline_segment_style_map(style: &ComputedStyle) -> HashMap<String, String> {
    let mut m = HashMap::with_capacity(4);
    m.insert("visibility".into(), match style.visibility {
        Visibility::Visible => "visible",
        Visibility::Hidden => "hidden",
        Visibility::Collapse => "collapse",
    }.into());
    m.insert("white-space".into(), match style.white_space {
        WhiteSpace::Normal => "normal",
        WhiteSpace::Nowrap => "nowrap",
        WhiteSpace::Pre => "pre",
        WhiteSpace::PreWrap => "pre-wrap",
        WhiteSpace::PreLine => "pre-line",
        WhiteSpace::BreakSpaces => "break-spaces",
    }.into());
    m.insert("text-transform".into(), match style.text_transform {
        TextTransform::None => "none",
        TextTransform::Uppercase => "uppercase",
        TextTransform::Lowercase => "lowercase",
        TextTransform::Capitalize => "capitalize",
    }.into());
    m
}

/// CSS Overflow L4 §continue / WHATWG Compat §2.1 — `display`'s computed-
/// value special case for `-webkit-box`/`-webkit-inline-box` (BUG-505 срез
/// 5, confirmed against `css/css-overflow/parsing/webkit-box-computed.html`):
/// normally `display: -webkit-box`/`-webkit-inline-box` compute AS SPECIFIED
/// (the literal keyword round-trips), but when `-webkit-box-orient` is
/// `vertical` AND the box is actually clamping — either `-webkit-line-clamp`/
/// `line-clamp` resolves to a definite integer (not `none`/`auto`), or
/// `continue` is `discard` — the computed value becomes `flow-root` (for
/// `-webkit-box`) / `inline-block` (for `-webkit-inline-box`) instead. This
/// quirk is deliberately narrower than "any legacy webkit flex alias": WPT's
/// own test asserts `-webkit-flex`/`flex` do NOT get it even under the exact
/// same orient+clamp combination — those already alias straight to
/// `Display::Flex`/`InlineFlex` at parse time (`style/apply/layout.rs`) and
/// never reach this function as a `WebkitBox`/`WebkitInlineBox` variant.
fn webkit_box_computed_display(style: &ComputedStyle) -> &'static str {
    let is_clamping = style.box_orient == WebkitBoxOrient::Vertical
        && (style.line_clamp.is_some() || style.continue_value == CssContinue::Discard);
    match style.display {
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
        Display::WebkitBox => if is_clamping { "flow-root" } else { "-webkit-box" },
        Display::WebkitInlineBox => if is_clamping { "inline-block" } else { "-webkit-inline-box" },
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
    m.insert("display".into(), webkit_box_computed_display(style).into());

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
    m.insert("background-image".into(), background_layers_to_css(&style.background_layers));
    m.insert("background-position-x".into(),
        background_position_axis_to_css(&style.background_layers, |l| l.position.x));
    m.insert("background-position-y".into(),
        background_position_axis_to_css(&style.background_layers, |l| l.position.y));

    // ── Replaced elements (CSS Images L3) ──────────────────────────
    m.insert("object-fit".into(), match style.object_fit {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::None => "none",
        ObjectFit::ScaleDown => "scale-down",
    }.into());
    m.insert("object-position".into(), format!(
        "{} {}",
        position_component_to_css(style.object_position.x),
        position_component_to_css(style.object_position.y),
    ));
    m.insert("image-rendering".into(), match style.image_rendering {
        ImageRendering::Auto => "auto",
        ImageRendering::Smooth => "smooth",
        ImageRendering::HighQuality => "high-quality",
        ImageRendering::CrispEdges => "crisp-edges",
        ImageRendering::Pixelated => "pixelated",
    }.into());

    // `border-color` shorthand — CSSOM `getPropertyValue` on a shorthand only
    // resolves when every longhand it covers serializes to the same value
    // (matches real UA behaviour: differing per-side colors read back as "").
    m.insert("border-color".into(), {
        let (t, r, b, l) = (
            css_color_to_css(&style.border_top_color),
            css_color_to_css(&style.border_right_color),
            css_color_to_css(&style.border_bottom_color),
            css_color_to_css(&style.border_left_color),
        );
        if t == r && r == b && b == l { t } else { String::new() }
    });

    // CSS 2.1 §17.6 `border-spacing` — one value when horizontal/vertical
    // components are equal (the common case, including every legacy
    // `cellspacing` presentational hint), two otherwise.
    m.insert("border-spacing".into(), {
        let (h, v) = (style.border_spacing_h, style.border_spacing_v);
        if h == v { px_str(h) } else { format!("{} {}", px_str(h), px_str(v)) }
    });
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
    // CSS Fonts L4 §6.10: shorthand сериализуется значениями реализованных
    // компонент (caps + emoji) — остальные longhand-ы всегда в initial.
    // Обе в initial → `normal`; иначе — только не-initial части, в порядке
    // грамматики.
    m.insert("font-variant".into(), {
        let mut parts: Vec<&str> = Vec::with_capacity(2);
        if style.font_variant_caps != FontVariantCaps::Normal {
            parts.push(style.font_variant_caps.as_str());
        }
        if style.font_variant_emoji != FontVariantEmoji::Normal {
            parts.push(style.font_variant_emoji.as_str());
        }
        if parts.is_empty() { "normal".to_string() } else { parts.join(" ") }
    });
    m.insert("font-variant-caps".into(), style.font_variant_caps.as_str().into());
    m.insert("font-variant-emoji".into(), style.font_variant_emoji.as_str().into());
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
        WhiteSpace::BreakSpaces => "break-spaces",
    }.into());
    m.insert("white-space-collapse".into(), match style.white_space_collapse {
        WhiteSpaceCollapse::Collapse => "collapse",
        WhiteSpaceCollapse::Preserve => "preserve",
        WhiteSpaceCollapse::PreserveBreaks => "preserve-breaks",
        WhiteSpaceCollapse::PreserveSpaces => "preserve-spaces",
        WhiteSpaceCollapse::BreakSpaces => "break-spaces",
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
    // CSS Overflow L4 §13.4 / compat `-webkit-line-clamp` (BUG-505): both
    // names read the same underlying field — the engine implements only the
    // reduced `none | <integer>` grammar, not the full `line-clamp`
    // shorthand (`max-lines`/`block-ellipsis`/`continue`/`-webkit-legacy`
    // longhands are unimplemented, see BUG-505).
    m.insert("-webkit-line-clamp".into(), match style.line_clamp {
        None => "none".to_string(),
        Some(n) => n.to_string(),
    });
    m.insert("line-clamp".into(), match style.line_clamp {
        None => "none".to_string(),
        Some(n) => n.to_string(),
    });
    // WHATWG Compat §2.1 / CSS Overflow L4 §continue (BUG-505 срез 5) — feed
    // `webkit_box_computed_display`'s condition, plus their own round-trip.
    m.insert("-webkit-box-orient".into(), match style.box_orient {
        WebkitBoxOrient::Horizontal => "horizontal",
        WebkitBoxOrient::Vertical => "vertical",
    }.into());
    m.insert("continue".into(), match style.continue_value {
        CssContinue::Normal => "normal",
        CssContinue::Discard => "discard",
        CssContinue::Collapse => "collapse",
        CssContinue::WebkitLegacy => "-webkit-legacy",
    }.into());
    m.insert("text-indent".into(), length_to_css(&style.text_indent));
    m.insert("vertical-align".into(), vertical_align_to_css(&style.vertical_align));

    // ── Overflow / stacking ───────────────────────────────────────
    m.insert("overflow-x".into(), overflow_to_css(style.overflow_x).into());
    m.insert("overflow-y".into(), overflow_to_css(style.overflow_y).into());
    // CSS Overflow L3 §propdef-overflow (BUG-505 срез 3): the shorthand's
    // own computed-value serialization — a single keyword when both axes
    // (already coerced by `coerce_overflow_axes`) agree, else `"x y"`, same
    // collapse rule the CSSOM `.style.overflow` specified-value path already
    // uses in the JS shim (`_lumen_overflow_shorthand_value`).
    m.insert("overflow".into(), if style.overflow_x == style.overflow_y {
        overflow_to_css(style.overflow_x).to_string()
    } else {
        format!("{} {}", overflow_to_css(style.overflow_x), overflow_to_css(style.overflow_y))
    });
    // CSS Overflow L3 §logical (BUG-505 срез 3): flow-relative axes read
    // back the already-resolved physical value on the axis they map to —
    // `overflow-block` → `overflow-y`/`overflow-x` depending on whether
    // `writing-mode` is horizontal or vertical (`css/css-overflow/logical-
    // overflow-001.html`), same swap `resolve_overflow_logical_properties`
    // (`style/logical.rs`) applies pre-cascade.
    let vertical_wm = matches!(
        style.writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl | WritingMode::SidewaysLr
    );
    let (overflow_block, overflow_inline) = if vertical_wm {
        (style.overflow_x, style.overflow_y)
    } else {
        (style.overflow_y, style.overflow_x)
    };
    m.insert("overflow-block".into(), overflow_to_css(overflow_block).into());
    m.insert("overflow-inline".into(), overflow_to_css(overflow_inline).into());
    // CSS Scrollbars L1 §3 — `auto | <color>{2}`. The field is `None` for
    // `auto`, so an unstyled page answers `"auto"`, not `""` (BUG-388).
    m.insert("scrollbar-color".into(), match style.scrollbar_color {
        None => "auto".to_string(),
        Some((thumb, track)) => format!("{} {}", color_to_css(thumb), color_to_css(track)),
    });
    m.insert("scrollbar-gutter".into(), match style.scrollbar_gutter {
        ScrollbarGutter::Auto => "auto",
        ScrollbarGutter::Stable => "stable",
        ScrollbarGutter::StableBothEdges => "stable both-edges",
    }.into());
    // CSS Overflow L3 §overflow-clip-margin (BUG-505 срез 4): initial value
    // is `padding-box 0px`, which per `overflow_clip_margin_serialize`'s
    // elision rules serializes as bare `"0px"`.
    m.insert("overflow-clip-margin".into(), match &style.overflow_clip_margin {
        None => "0px".to_string(),
        Some((box_kw, l)) => {
            let (css, is_zero) = overflow_clip_margin_computed_length(l, style.font_size);
            overflow_clip_margin_serialize(*box_kw, css, is_zero)
        }
    });
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
    m.insert("mix-blend-mode".into(), mix_blend_mode_to_css(style.mix_blend_mode).into());
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

    // ── Containment (CSS Containment L3 §3–§4, CSS Box Sizing L4 §5) ───────
    // BUG-852: both properties reach layout (the skip decision and the size
    // placeholder are live) but had no entry here, so a page could set them and
    // never read them back — `getComputedStyle(el).contentVisibility` answered
    // `""`, which is not a value the property can ever take.
    m.insert("content-visibility".into(), match style.content_visibility {
        ContentVisibility::Visible => "visible",
        ContentVisibility::Auto => "auto",
        ContentVisibility::Hidden => "hidden",
    }.into());
    m.insert("contain".into(), contain_to_css(style.contain));
    let cis_w = contain_intrinsic_to_css(
        style.contain_intrinsic_width_auto, style.contain_intrinsic_width.as_ref());
    let cis_h = contain_intrinsic_to_css(
        style.contain_intrinsic_height_auto, style.contain_intrinsic_height.as_ref());
    // CSSOM §6.2: a shorthand serialises to one component when both axes agree.
    m.insert("contain-intrinsic-size".into(),
             if cis_w == cis_h { cis_w.clone() } else { format!("{cis_w} {cis_h}") });
    m.insert("contain-intrinsic-width".into(), cis_w);
    m.insert("contain-intrinsic-height".into(), cis_h);

    // ── Border shorthands (CSSOM-3 срез 1) ──────────────────────────
    // `border-width`/`border-style` mirror `border-color` above: a shorthand
    // resolves only when all four sides agree, otherwise `""` — matches real
    // UA `getPropertyValue` behaviour on a per-side-differing shorthand.
    m.insert("border-width".into(), {
        let (t, r, b, l) = (
            px_str(style.border_top_width), px_str(style.border_right_width),
            px_str(style.border_bottom_width), px_str(style.border_left_width),
        );
        if t == r && r == b && b == l { t } else { String::new() }
    });
    m.insert("border-style".into(), {
        let (t, r, b, l) = (
            border_style_to_css(style.border_top_style), border_style_to_css(style.border_right_style),
            border_style_to_css(style.border_bottom_style), border_style_to_css(style.border_left_style),
        );
        if t == r && r == b && b == l { t.to_string() } else { String::new() }
    });

    // ── Background per-layer longhands (CSS Backgrounds L3 §3.5-3.8) ────
    m.insert("background-attachment".into(), background_layer_field_to_css(
        &style.background_layers, BackgroundAttachment::default(), |l| l.attachment, background_attachment_to_css,
    ));
    m.insert("background-origin".into(), background_layer_field_to_css(
        &style.background_layers, BackgroundOrigin::default(), |l| l.origin, background_origin_to_css,
    ));
    m.insert("background-clip".into(), background_layer_field_to_css(
        &style.background_layers, BackgroundClip::default(), |l| l.clip, background_clip_to_css,
    ));
    m.insert("background-repeat".into(), background_layer_field_to_css(
        &style.background_layers, BackgroundRepeat::default(), |l| l.repeat, background_repeat_to_css,
    ));
    m.insert("background-size".into(), if style.background_layers.is_empty() {
        background_size_to_css(BackgroundSize::default())
    } else {
        style.background_layers.iter().map(|l| background_size_to_css(l.size)).collect::<Vec<_>>().join(", ")
    });
    // CSS Compositing L1 §3 — per-layer, unlike top-level `mix-blend-mode`
    // (compositing of the element as a whole against its backdrop).
    m.insert("background-blend-mode".into(), background_layer_field_to_css(
        &style.background_layers, MixBlendMode::Normal, |l| l.blend_mode, mix_blend_mode_to_css,
    ));

    // ── Shadows (CSS Backgrounds L3 §7.1, CSS Text Decoration L3 §2.5) ──
    m.insert("box-shadow".into(), box_shadow_list_to_css(&style.box_shadow));
    m.insert("text-shadow".into(), text_shadow_list_to_css(&style.text_shadow));

    // ── Perspective ──────────────────────────────────────────────────
    m.insert("perspective-origin".into(), format!(
        "{} {}",
        position_component_to_css(style.perspective_origin.0),
        position_component_to_css(style.perspective_origin.1),
    ));

    // ── Transitions / animations (CSS Transitions L1 §3, CSS Animations L1 §4.3) ─
    m.insert("transition-duration".into(), seconds_list_to_css(&style.transition_durations));
    m.insert("transition-delay".into(), seconds_list_to_css(&style.transition_delays));
    m.insert("transition-timing-function".into(), timing_function_list_to_css(&style.transition_timing_functions));
    m.insert("transition-property".into(), if style.transition_properties.is_empty() {
        "all".into()
    } else {
        style.transition_properties.join(", ")
    });
    m.insert("animation-duration".into(), seconds_list_to_css(&style.animation_durations));
    m.insert("animation-delay".into(), seconds_list_to_css(&style.animation_delays));
    m.insert("animation-timing-function".into(), timing_function_list_to_css(&style.animation_timing_functions));
    m.insert("animation-name".into(), if style.animation_names.is_empty() {
        "none".into()
    } else {
        style.animation_names.join(", ")
    });
    m.insert("animation-iteration-count".into(), {
        let counts = if style.animation_iteration_counts.is_empty() {
            vec![IterationCount::default()]
        } else {
            style.animation_iteration_counts.clone()
        };
        counts.iter().map(|c| match c {
            IterationCount::Infinite => "infinite".to_string(),
            IterationCount::Finite(n) => if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{n}") },
        }).collect::<Vec<_>>().join(", ")
    });
    m.insert("animation-direction".into(), {
        let dirs = if style.animation_directions.is_empty() {
            vec![AnimationDirection::default()]
        } else {
            style.animation_directions.clone()
        };
        dirs.iter().map(|d| match d {
            AnimationDirection::Normal => "normal",
            AnimationDirection::Reverse => "reverse",
            AnimationDirection::Alternate => "alternate",
            AnimationDirection::AlternateReverse => "alternate-reverse",
        }).collect::<Vec<_>>().join(", ")
    });
    m.insert("animation-fill-mode".into(), {
        let modes = if style.animation_fill_modes.is_empty() {
            vec![AnimationFillMode::default()]
        } else {
            style.animation_fill_modes.clone()
        };
        modes.iter().map(|f| match f {
            AnimationFillMode::None => "none",
            AnimationFillMode::Forwards => "forwards",
            AnimationFillMode::Backwards => "backwards",
            AnimationFillMode::Both => "both",
        }).collect::<Vec<_>>().join(", ")
    });
    m.insert("animation-play-state".into(), {
        let states = if style.animation_play_states.is_empty() {
            vec![AnimationPlayState::default()]
        } else {
            style.animation_play_states.clone()
        };
        states.iter().map(|s| match s {
            AnimationPlayState::Running => "running",
            AnimationPlayState::Paused => "paused",
        }).collect::<Vec<_>>().join(", ")
    });

    // ── Overscroll behaviour (CSS Overscroll Behavior §3) ───────────
    m.insert("overscroll-behavior-x".into(), match style.overscroll_behavior_x {
        OverscrollBehavior::Auto => "auto",
        OverscrollBehavior::Contain => "contain",
        OverscrollBehavior::None => "none",
    }.into());
    m.insert("overscroll-behavior-y".into(), match style.overscroll_behavior_y {
        OverscrollBehavior::Auto => "auto",
        OverscrollBehavior::Contain => "contain",
        OverscrollBehavior::None => "none",
    }.into());

    // ── Color adjustment (CSS Color Adjustment L1) ──────────────────
    m.insert("color-scheme".into(), match style.color_scheme {
        ColorScheme::Normal => "normal",
        ColorScheme::Light => "light",
        ColorScheme::Dark => "dark",
        ColorScheme::LightDark => "light dark",
        ColorScheme::DarkLight => "dark light",
        ColorScheme::OnlyLight => "only light",
        ColorScheme::OnlyDark => "only dark",
    }.into());
    m.insert("forced-color-adjust".into(), match style.forced_color_adjust {
        ForcedColorAdjust::Auto => "auto",
        ForcedColorAdjust::None => "none",
        ForcedColorAdjust::PreserveParentColor => "preserve-parent-color",
    }.into());
    m.insert("print-color-adjust".into(), match style.print_color_adjust {
        PrintColorAdjust::Economy => "economy",
        PrintColorAdjust::Exact => "exact",
    }.into());
    // `color-adjust` is the legacy alias of `print-color-adjust` (CSS Color
    // Adjustment L1 §3) — the cascade already parses both names onto the
    // same `print_color_adjust` field (`style/apply/{css_wide,paint}.rs`),
    // so the resolved value is identical.
    m.insert("color-adjust".into(), m["print-color-adjust"].clone());

    // ── SVG paint properties (SVG2 §11) ──────────────────────────────
    m.insert("fill".into(), svg_paint_to_css(&style.svg_fill));
    m.insert("fill-opacity".into(), format!("{}", style.svg_fill_opacity));
    m.insert("fill-rule".into(), fill_rule_to_css(style.svg_fill_rule).into());
    m.insert("stroke".into(), svg_paint_to_css(&style.svg_stroke));
    m.insert("stroke-opacity".into(), format!("{}", style.svg_stroke_opacity));
    m.insert("stroke-width".into(), px_str(style.svg_stroke_width));
    m.insert("stroke-linecap".into(), stroke_linecap_to_css(style.svg_stroke_linecap).into());
    m.insert("stroke-linejoin".into(), stroke_linejoin_to_css(style.svg_stroke_linejoin).into());
    m.insert("stroke-miterlimit".into(), format!("{}", style.svg_stroke_miterlimit));
    m.insert("stroke-dashoffset".into(), px_str(style.svg_stroke_dashoffset));
    m.insert("stroke-dasharray".into(), if style.svg_stroke_dasharray.is_empty() {
        "none".into()
    } else {
        style.svg_stroke_dasharray.iter().map(|v| px_str(*v)).collect::<Vec<_>>().join(", ")
    });

    // ── Generated content (CSS Generated Content L3) ────────────────
    m.insert("content".into(), content_to_css(&style.content));
    m.insert("quotes".into(), quotes_to_css(&style.quotes));

    // ── Scrollbars (CSS Scrollbars L1 §2) ────────────────────────────
    m.insert("scrollbar-width".into(), match style.scrollbar_width {
        ScrollbarWidth::Auto => "auto",
        ScrollbarWidth::Thin => "thin",
        ScrollbarWidth::None => "none",
    }.into());

    // ── Ruby (CSS Ruby L1 §4-6) ───────────────────────────────────────
    m.insert("ruby-position".into(), match style.ruby_position {
        RubyPosition::Over => "over",
        RubyPosition::Under => "under",
    }.into());
    m.insert("ruby-align".into(), match style.ruby_align {
        RubyAlign::Start => "start",
        RubyAlign::Center => "center",
        RubyAlign::SpaceBetween => "space-between",
        RubyAlign::SpaceAround => "space-around",
    }.into());
    m.insert("ruby-merge".into(), match style.ruby_merge {
        RubyMerge::Separate => "separate",
        RubyMerge::Merge => "merge",
        RubyMerge::Auto => "auto",
    }.into());

    // ── CSS Logical Properties L1 (CSSOM-3 срез 2) ───────────────────
    // Resolved value of a logical longhand is the resolved value of its
    // flow-relative physical twin (already computed above). Phase 0 layout
    // (`style::logical::resolve_logical_properties`) only resolves
    // horizontal-tb/LTR onto physical fields — gate the same way here rather
    // than fabricate a mapping the layout side never applied.
    if style.writing_mode == WritingMode::HorizontalTb && style.direction == Direction::Ltr {
        const LOGICAL_LONGHANDS: &[(&str, &str)] = &[
            ("inline-size", "width"), ("min-inline-size", "min-width"), ("max-inline-size", "max-width"),
            ("block-size", "height"), ("min-block-size", "min-height"), ("max-block-size", "max-height"),
            ("inset-inline-start", "left"), ("inset-inline-end", "right"),
            ("inset-block-start", "top"), ("inset-block-end", "bottom"),
            ("margin-inline-start", "margin-left"), ("margin-inline-end", "margin-right"),
            ("margin-block-start", "margin-top"), ("margin-block-end", "margin-bottom"),
            ("padding-inline-start", "padding-left"), ("padding-inline-end", "padding-right"),
            ("padding-block-start", "padding-top"), ("padding-block-end", "padding-bottom"),
            ("border-inline-start-width", "border-left-width"), ("border-inline-end-width", "border-right-width"),
            ("border-block-start-width", "border-top-width"), ("border-block-end-width", "border-bottom-width"),
            ("border-inline-start-style", "border-left-style"), ("border-inline-end-style", "border-right-style"),
            ("border-block-start-style", "border-top-style"), ("border-block-end-style", "border-bottom-style"),
            ("border-inline-start-color", "border-left-color"), ("border-inline-end-color", "border-right-color"),
            ("border-block-start-color", "border-top-color"), ("border-block-end-color", "border-bottom-color"),
        ];
        for (logical, physical) in LOGICAL_LONGHANDS {
            if let Some(v) = m.get(*physical).cloned() {
                m.insert((*logical).into(), v);
            }
        }

        // Two-value logical shorthands (CSS Logical L1 §6): collapse to one
        // component when start/end agree, otherwise "start end" — same rule
        // already used above for `contain-intrinsic-size`.
        fn two_value(m: &HashMap<String, String>, a: &str, b: &str) -> String {
            let (va, vb) = (m.get(a).cloned().unwrap_or_default(), m.get(b).cloned().unwrap_or_default());
            if va == vb { va } else { format!("{va} {vb}") }
        }
        m.insert("margin-inline".into(), two_value(&m, "margin-left", "margin-right"));
        m.insert("margin-block".into(), two_value(&m, "margin-top", "margin-bottom"));
        m.insert("padding-inline".into(), two_value(&m, "padding-left", "padding-right"));
        m.insert("padding-block".into(), two_value(&m, "padding-top", "padding-bottom"));
        m.insert("inset-inline".into(), two_value(&m, "left", "right"));
        m.insert("inset-block".into(), two_value(&m, "top", "bottom"));
    }

    m
}

/// Serialises [`ContainFlags`] the way CSS Containment L3 §3 asks for: the
/// `none` keyword when empty, and otherwise the individual keywords in
/// canonical order — never the `strict`/`content` shorthand keywords, which the
/// spec defines as computing to the set they expand to.
fn contain_to_css(flags: ContainFlags) -> String {
    if flags == ContainFlags::NONE {
        return "none".into();
    }
    let mut out: Vec<&str> = Vec::with_capacity(5);
    for (bit, name) in [
        (ContainFlags::SIZE, "size"),
        (ContainFlags::INLINE_SIZE, "inline-size"),
        (ContainFlags::LAYOUT, "layout"),
        (ContainFlags::STYLE, "style"),
        (ContainFlags::PAINT, "paint"),
    ] {
        if flags.0 & bit.0 != 0 {
            out.push(name);
        }
    }
    out.join(" ")
}

/// Serialises one axis of `contain-intrinsic-size`: `auto? [ none | <length> ]`.
/// The `auto` keyword is behaviourally ignored by layout but is part of the
/// computed value, so it must survive the round-trip (BUG-852).
fn contain_intrinsic_to_css(auto: bool, len: Option<&Length>) -> String {
    let base = len.map_or_else(|| "none".to_string(), length_to_css);
    if auto { format!("auto {base}") } else { base }
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

// ──────────────── Matched CSS rules for DevTools Styles panel ────────────────

/// One CSS rule that matched a specific DOM node.
///
/// Used by the DevTools Styles panel (§PH3-1) to show the source CSS rules
/// alongside their selectors and declarations, similar to Chrome DevTools.
#[derive(Debug, Clone)]
pub struct MatchedRule {
    /// The matching CSS selector text, e.g. `"div.container > p"`.
    pub selector: String,
    /// CSS Selectors L3 specificity `(a, b, c)`.
    pub specificity: (u32, u32, u32),
    /// Property-value pairs from the rule's declarations, in source order.
    /// Values are the raw CSS strings as written in the stylesheet.
    pub declarations: Vec<(String, String)>,
}

/// Return all CSS rules from `sheet` whose selectors match `node` in `doc`.
///
/// Iterates `sheet.rules` in source order. Each rule is included at most once:
/// the first selector in the rule that matches `node` is used as the display
/// selector, and the whole rule's declaration list is included. Only toplevel
/// rules are checked (media/layer/supports blocks are skipped in this pass —
/// they contribute their matched-rule set separately if the condition holds).
///
/// Used by [`crate::shell::devtools::inspector`] to populate the Styles tab
/// when the user clicks on a DOM element.
pub fn matched_rules_for_node(
    doc: &lumen_dom::Document,
    node: lumen_dom::NodeId,
    sheet: &lumen_css_parser::Stylesheet,
) -> Vec<MatchedRule> {
    let mut out = Vec::new();
    for rule in &sheet.rules {
        for selector in &rule.selectors {
            if matches_complex(selector, doc, node) {
                let spec = selector.specificity();
                out.push(MatchedRule {
                    selector: selector.to_css_str(),
                    specificity: (spec.a, spec.b, spec.c),
                    declarations: rule
                        .declarations
                        .iter()
                        .map(|d| (d.property.clone(), d.value.clone()))
                        .collect(),
                });
                break; // include each rule at most once
            }
        }
    }
    out
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
    fn find_first_dom_node_by_selector_finds_display_none() {
        let (doc, _tree) = layout_tree(r#"<div id="hidden" style="display:none">x</div>"#, "");
        // `find_box_by_selector` cannot see it — no layout box is built for
        // `display: none` — but the node is still in the DOM.
        assert!(find_box_by_selector(&_tree, &doc, "#hidden").is_none());
        assert!(find_first_dom_node_by_selector(&doc, "#hidden").is_some());
    }

    #[test]
    fn find_first_dom_node_by_selector_miss_returns_none() {
        let (doc, _tree) = layout_tree("<div>text</div>", "");
        assert!(find_first_dom_node_by_selector(&doc, "#nonexistent").is_none());
    }

    #[test]
    fn empty_selector_returns_none() {
        let (doc, tree) = layout_tree("<div>text</div>", "");
        assert!(find_box_by_selector(&tree, &doc, "").is_none());
    }

    // BUG-291: `query_all_scoped` must find matches inside a subtree that has
    // no path to `doc.root()` at all — `query_all` (whole-document search)
    // cannot, which is exactly what broke `Element.querySelector` on
    // `testharness.js`'s off-document results table.
    #[test]
    fn query_all_scoped_finds_matches_in_detached_subtree() {
        let mut doc = lumen_dom::Document::new();
        let table = doc.create_element(lumen_dom::QualName::html("table"));
        let tbody = doc.create_element(lumen_dom::QualName::html("tbody"));
        doc.append_child(table, tbody);
        // `table` is intentionally never attached to `doc.root()`.

        assert_eq!(query_all_scoped(&doc, table, "tbody"), vec![tbody]);
        assert!(query_all(&doc, "tbody").is_empty());
    }

    // BUG-291: scoped queries match descendants only — never the scope node
    // itself, and never nodes outside its subtree.
    #[test]
    fn query_all_scoped_excludes_self_and_siblings() {
        let mut doc = lumen_dom::Document::new();
        let root = doc.root();
        let a = doc.create_element(lumen_dom::QualName::html("div"));
        let b = doc.create_element(lumen_dom::QualName::html("div"));
        let c = doc.create_element(lumen_dom::QualName::html("div"));
        doc.append_child(root, a);
        doc.append_child(root, b);
        doc.append_child(a, c);

        // Only the descendant `c` matches — not `a` itself, not sibling `b`.
        assert_eq!(query_all_scoped(&doc, a, "div"), vec![c]);
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
    fn query_all_within_only_matches_descendants_of_start() {
        // BUG-298: `Element.querySelectorAll` must be scoped to the calling
        // node's subtree — `#outer` has one `.item` inside it and there's a
        // second `.item` sibling outside; `query_all_within` must find only
        // the inner one, unlike `query_all` (document-wide) which finds both.
        let (doc, tree) = layout_tree(
            r#"<div id="outer"><span class="item">a</span></div><span class="item">b</span>"#,
            "",
        );
        let outer = find_box_by_selector(&tree, &doc, "#outer").unwrap().node;
        let scoped = query_all_within(&doc, outer, ".item");
        assert_eq!(scoped.len(), 1);
        assert_eq!(query_all(&doc, ".item").len(), 2);
    }

    #[test]
    fn query_all_within_excludes_start_itself() {
        let (doc, tree) = layout_tree(r#"<div id="outer" class="item"></div>"#, "");
        let outer = find_box_by_selector(&tree, &doc, "#outer").unwrap().node;
        assert!(query_all_within(&doc, outer, ".item").is_empty());
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

    // ──────────────── matched_rules_for_node ────────────────

    fn parse_doc_sheet(html: &str, css: &str) -> (lumen_dom::Document, lumen_css_parser::Stylesheet) {
        (lumen_html_parser::parse(html), lumen_css_parser::parse(css))
    }

    #[test]
    fn matched_rules_empty_sheet_returns_empty() {
        let (doc, sheet) = parse_doc_sheet(r#"<div id="box">text</div>"#, "");
        let node = doc.find_by_id("box").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert!(rules.is_empty());
    }

    #[test]
    fn matched_rules_tag_selector_matches() {
        let (doc, sheet) = parse_doc_sheet(r#"<div id="x">text</div>"#, "div { color: red; }");
        let node = doc.find_by_id("x").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "div");
        assert_eq!(rules[0].specificity, (0, 0, 1));
        assert!(rules[0].declarations.iter().any(|(p, v)| p == "color" && v == "red"));
    }

    #[test]
    fn matched_rules_id_selector_matches() {
        let (doc, sheet) = parse_doc_sheet(
            r#"<div id="box">text</div>"#,
            "#box { opacity: 0.5; display: flex; }",
        );
        let node = doc.find_by_id("box").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "#box");
        assert_eq!(rules[0].specificity, (1, 0, 0));
        assert!(rules[0].declarations.iter().any(|(p, v)| p == "opacity" && v == "0.5"));
        assert!(rules[0].declarations.iter().any(|(p, v)| p == "display" && v == "flex"));
    }

    #[test]
    fn matched_rules_class_selector_matches() {
        let (doc, sheet) = parse_doc_sheet(
            r#"<p id="intro-p" class="intro">text</p>"#,
            ".intro { font-size: 18px; }",
        );
        let node = doc.find_by_id("intro-p").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, ".intro");
        assert_eq!(rules[0].specificity, (0, 1, 0));
    }

    #[test]
    fn matched_rules_non_matching_selector_not_included() {
        let (doc, sheet) = parse_doc_sheet(
            r#"<div id="box">text</div>"#,
            "#other { color: red; }",
        );
        let node = doc.find_by_id("box").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert!(rules.is_empty());
    }

    #[test]
    fn matched_rules_multiple_matching_rules() {
        let (doc, sheet) = parse_doc_sheet(
            r#"<div id="box" class="card">text</div>"#,
            "div { color: blue; } .card { margin: 10px; } #box { opacity: 1; }",
        );
        let node = doc.find_by_id("box").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert_eq!(rules.len(), 3);
        let selectors: Vec<&str> = rules.iter().map(|r| r.selector.as_str()).collect();
        assert!(selectors.contains(&"div"));
        assert!(selectors.contains(&".card"));
        assert!(selectors.contains(&"#box"));
    }

    #[test]
    fn matched_rules_specificity_correct_for_tag_and_class() {
        let (doc, sheet) = parse_doc_sheet(
            r#"<div id="x" class="hero">text</div>"#,
            "div.hero { display: block; }",
        );
        let node = doc.find_by_id("x").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        assert_eq!(rules.len(), 1);
        // tag (0,0,1) + class (0,1,0) = (0,1,1)
        assert_eq!(rules[0].specificity, (0, 1, 1));
    }

    #[test]
    fn matched_rules_rule_included_once_per_matching_selector() {
        // A rule with comma selectors — each matching selector yields one entry.
        let (doc, sheet) = parse_doc_sheet(
            r#"<div id="foo">text</div>"#,
            "#foo, #bar { color: green; }",
        );
        let node = doc.find_by_id("foo").expect("node exists");
        let rules = matched_rules_for_node(&doc, node, &sheet);
        // Only "#foo" matches; the rule appears once with the matching selector.
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector, "#foo");
    }

    // ── computed_style_to_map: BUG-388 additions ─────────────────────────────

    /// `computed_style_to_map` for the first `<div>` box of the laid-out page.
    fn div_computed_map(html: &str, css: &str) -> HashMap<String, String> {
        let (doc, tree) = layout_tree(html, css);
        let b = find_box_by_selector(&tree, &doc, "div").expect("div box exists");
        computed_style_to_map(&b.style)
    }

    #[test]
    fn computed_map_scrollbar_color_defaults_to_auto() {
        // Before BUG-388 the key was absent entirely, so `getComputedStyle`
        // answered "" where every other engine answers "auto".
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("scrollbar-color").map(String::as_str), Some("auto"));
    }

    #[test]
    fn computed_map_scrollbar_color_serialises_pair() {
        let m = div_computed_map("<div>x</div>", "div { scrollbar-color: lime red; }");
        assert_eq!(
            m.get("scrollbar-color").map(String::as_str),
            Some("rgb(0, 255, 0) rgb(255, 0, 0)")
        );
    }

    // ── computed_style_to_map: BUG-852 containment additions ─────────────────

    #[test]
    fn computed_map_containment_defaults() {
        // All five keys were absent before BUG-852, so `getComputedStyle`
        // answered `""` — a value neither property can ever take.
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("content-visibility").map(String::as_str), Some("visible"));
        assert_eq!(m.get("contain").map(String::as_str), Some("none"));
        assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("none"));
        assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("none"));
        assert_eq!(m.get("contain-intrinsic-height").map(String::as_str), Some("none"));
    }

    #[test]
    fn computed_map_content_visibility_auto() {
        let m = div_computed_map("<div>x</div>", "div { content-visibility: auto; }");
        assert_eq!(m.get("content-visibility").map(String::as_str), Some("auto"));
        let m = div_computed_map("<div>x</div>", "div { content-visibility: hidden; }");
        assert_eq!(m.get("content-visibility").map(String::as_str), Some("hidden"));
    }

    #[test]
    fn computed_map_contain_expands_shorthand_keywords() {
        // CSS Containment L3 §3: `strict`/`content` compute to the set they
        // expand to, so neither keyword may appear in the computed value.
        let m = div_computed_map("<div>x</div>", "div { contain: layout paint; }");
        assert_eq!(m.get("contain").map(String::as_str), Some("layout paint"));
        let m = div_computed_map("<div>x</div>", "div { contain: content; }");
        assert_eq!(m.get("contain").map(String::as_str), Some("layout style paint"));
    }

    #[test]
    fn computed_map_contain_intrinsic_size_keeps_auto_keyword() {
        // The `auto` keyword changes nothing in layout, which is why the
        // parser used to drop it — but it is part of the computed value, so
        // `auto 1px` must not read back as `1px`.
        let m = div_computed_map("<div>x</div>", "div { contain-intrinsic-size: auto 1px; }");
        assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("auto 1px"));
        assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("auto 1px"));
        assert_eq!(m.get("contain-intrinsic-height").map(String::as_str), Some("auto 1px"));
    }

    #[test]
    fn computed_map_contain_intrinsic_size_two_components() {
        let m = div_computed_map("<div>x</div>", "div { contain-intrinsic-size: 30px 40px; }");
        assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("30px 40px"));
        assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("30px"));
        assert_eq!(m.get("contain-intrinsic-height").map(String::as_str), Some("40px"));
        // A longhand set on its own leaves the other axis at `none`.
        // (Relative units are not absolutised here — `length_to_css` reports
        // `5em` as `5em` for every length property in this map, not just this
        // one; that is a pre-existing property of the snapshot, so the case is
        // written in px rather than papering over it.)
        let m = div_computed_map("<div>x</div>", "div { contain-intrinsic-height: 80px; }");
        assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("none"));
        assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("none 80px"));
    }

    // ──────────────── CSSOM-3 срез 1: extended computed-style coverage ────────────────

    #[test]
    fn computed_map_border_width_style_shorthands() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { border-width: 2px; border-style: dashed; }",
        );
        assert_eq!(m.get("border-width").map(String::as_str), Some("2px"));
        assert_eq!(m.get("border-style").map(String::as_str), Some("dashed"));

        // Differing per-side values → shorthand resolves to "", matching
        // border-color's already-established behaviour above.
        let m = div_computed_map(
            "<div>x</div>",
            "div { border-top-width: 1px; border-right-width: 2px; border-bottom-width: 3px; border-left-width: 4px; }",
        );
        assert_eq!(m.get("border-width").map(String::as_str), Some(""));
    }

    #[test]
    fn computed_map_background_per_layer_longhands() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { background-attachment: fixed; background-origin: content-box; \
             background-clip: padding-box; background-repeat: repeat-x; \
             background-size: 50% 10px; background-blend-mode: multiply; }",
        );
        assert_eq!(m.get("background-attachment").map(String::as_str), Some("fixed"));
        assert_eq!(m.get("background-origin").map(String::as_str), Some("content-box"));
        assert_eq!(m.get("background-clip").map(String::as_str), Some("padding-box"));
        assert_eq!(m.get("background-repeat").map(String::as_str), Some("repeat-x"));
        assert_eq!(m.get("background-size").map(String::as_str), Some("50% 10px"));
        assert_eq!(m.get("background-blend-mode").map(String::as_str), Some("multiply"));
    }

    #[test]
    fn computed_map_background_per_layer_longhands_default_without_layers() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("background-attachment").map(String::as_str), Some("scroll"));
        assert_eq!(m.get("background-origin").map(String::as_str), Some("padding-box"));
        assert_eq!(m.get("background-clip").map(String::as_str), Some("border-box"));
        assert_eq!(m.get("background-repeat").map(String::as_str), Some("repeat"));
        assert_eq!(m.get("background-size").map(String::as_str), Some("auto"));
        assert_eq!(m.get("background-blend-mode").map(String::as_str), Some("normal"));
    }

    #[test]
    fn computed_map_box_shadow_and_text_shadow() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { box-shadow: 1px 2px 3px 4px red inset; text-shadow: 5px 6px 7px blue; }",
        );
        assert_eq!(
            m.get("box-shadow").map(String::as_str),
            Some("1px 2px 3px 4px rgb(255, 0, 0) inset"),
        );
        assert_eq!(
            m.get("text-shadow").map(String::as_str),
            Some("5px 6px 7px rgb(0, 0, 255)"),
        );

        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("box-shadow").map(String::as_str), Some("none"));
        assert_eq!(m.get("text-shadow").map(String::as_str), Some("none"));
    }

    #[test]
    fn computed_map_transition_and_animation_lists() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { transition-duration: 1s, 200ms; transition-timing-function: linear, ease; \
             animation-duration: 2s; animation-timing-function: steps(4, jump-end); \
             animation-iteration-count: infinite; animation-direction: alternate; \
             animation-fill-mode: both; animation-play-state: paused; }",
        );
        assert_eq!(m.get("transition-duration").map(String::as_str), Some("1s, 0.2s"));
        assert_eq!(
            m.get("transition-timing-function").map(String::as_str),
            Some("linear, cubic-bezier(0.25, 0.1, 0.25, 1)"),
        );
        assert_eq!(m.get("animation-duration").map(String::as_str), Some("2s"));
        assert_eq!(
            m.get("animation-timing-function").map(String::as_str),
            Some("steps(4, jump-end)"),
        );
        assert_eq!(m.get("animation-iteration-count").map(String::as_str), Some("infinite"));
        assert_eq!(m.get("animation-direction").map(String::as_str), Some("alternate"));
        assert_eq!(m.get("animation-fill-mode").map(String::as_str), Some("both"));
        assert_eq!(m.get("animation-play-state").map(String::as_str), Some("paused"));
    }

    #[test]
    fn computed_map_transition_defaults_without_declaration() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("transition-duration").map(String::as_str), Some("0s"));
        assert_eq!(m.get("transition-property").map(String::as_str), Some("all"));
        assert_eq!(m.get("animation-name").map(String::as_str), Some("none"));
        assert_eq!(m.get("animation-iteration-count").map(String::as_str), Some("1"));
        assert_eq!(m.get("animation-play-state").map(String::as_str), Some("running"));
    }

    #[test]
    fn computed_map_overscroll_behavior() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { overscroll-behavior-x: contain; overscroll-behavior-y: none; }",
        );
        assert_eq!(m.get("overscroll-behavior-x").map(String::as_str), Some("contain"));
        assert_eq!(m.get("overscroll-behavior-y").map(String::as_str), Some("none"));
    }

    #[test]
    fn computed_map_color_scheme_family() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { color-scheme: light dark; forced-color-adjust: none; print-color-adjust: exact; }",
        );
        assert_eq!(m.get("color-scheme").map(String::as_str), Some("light dark"));
        assert_eq!(m.get("forced-color-adjust").map(String::as_str), Some("none"));
        assert_eq!(m.get("print-color-adjust").map(String::as_str), Some("exact"));
    }

    #[test]
    fn computed_map_color_adjust_mirrors_print_color_adjust_alias() {
        let m = div_computed_map("<div>x</div>", "div { color-adjust: exact; }");
        assert_eq!(m.get("print-color-adjust").map(String::as_str), Some("exact"));
        assert_eq!(m.get("color-adjust").map(String::as_str), Some("exact"));
    }

    #[test]
    fn computed_map_svg_paint_properties() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { fill: none; stroke: url(#grad); stroke-width: 3px; fill-rule: evenodd; \
             stroke-linecap: round; stroke-linejoin: bevel; stroke-dasharray: 1px 2px; }",
        );
        assert_eq!(m.get("fill").map(String::as_str), Some("none"));
        assert_eq!(m.get("stroke").map(String::as_str), Some("url(#grad)"));
        assert_eq!(m.get("stroke-width").map(String::as_str), Some("3px"));
        assert_eq!(m.get("fill-rule").map(String::as_str), Some("evenodd"));
        assert_eq!(m.get("stroke-linecap").map(String::as_str), Some("round"));
        assert_eq!(m.get("stroke-linejoin").map(String::as_str), Some("bevel"));
        assert_eq!(m.get("stroke-dasharray").map(String::as_str), Some("1px, 2px"));
    }

    #[test]
    fn computed_map_content_and_quotes() {
        // `attr(data-x)` is deliberately not exercised here: `content`'s
        // `attr()` (a bare string substitution, CSS2.1 §12.2) shares a name
        // with the newer typed `attr()` substitution `cascade.rs` expands
        // pre-`apply_declaration` (CSS Values L4 §7.7) — that pre-pass drops
        // the whole declaration when the referenced attribute is absent,
        // which is a distinct, pre-existing gap from the map coverage this
        // test targets.
        let m = div_computed_map(
            "<div>x</div>",
            r#"div { content: "a" counter(x); quotes: "«" "»"; }"#,
        );
        assert_eq!(m.get("content").map(String::as_str), Some(r#""a" counter(x)"#));
        assert_eq!(m.get("quotes").map(String::as_str), Some(r#""«" "»""#));

        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("content").map(String::as_str), Some("normal"));
        assert_eq!(m.get("quotes").map(String::as_str), Some("auto"));
    }

    #[test]
    fn computed_map_scrollbar_width_and_ruby() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { scrollbar-width: thin; ruby-position: under; ruby-align: center; ruby-merge: merge; }",
        );
        assert_eq!(m.get("scrollbar-width").map(String::as_str), Some("thin"));
        assert_eq!(m.get("ruby-position").map(String::as_str), Some("under"));
        assert_eq!(m.get("ruby-align").map(String::as_str), Some("center"));
        assert_eq!(m.get("ruby-merge").map(String::as_str), Some("merge"));
    }

    #[test]
    fn computed_map_font_variant_emoji() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("font-variant-emoji").map(String::as_str), Some("normal"));
        let m = div_computed_map("<div>x</div>", "div { font-variant-emoji: unicode; }");
        assert_eq!(m.get("font-variant-emoji").map(String::as_str), Some("unicode"));
    }

    #[test]
    fn computed_map_font_variant_shorthand_joins_implemented_components() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("font-variant").map(String::as_str), Some("normal"));
        let m = div_computed_map("<div>x</div>", "div { font-variant: small-caps unicode; }");
        assert_eq!(m.get("font-variant").map(String::as_str), Some("small-caps unicode"));
        let m = div_computed_map("<div>x</div>", "div { font-variant-emoji: text; }");
        assert_eq!(m.get("font-variant").map(String::as_str), Some("text"));
    }

    // ── computed_style_to_map: BUG-603 additions ─────────────────────────────

    #[test]
    fn computed_map_background_image_defaults_to_none() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("background-image").map(String::as_str), Some("none"));
    }

    #[test]
    fn computed_map_background_image_url() {
        let m = div_computed_map("<div>x</div>", "div { background-image: url(a.png); }");
        assert_eq!(m.get("background-image").map(String::as_str), Some("url(\"a.png\")"));
    }

    // ── computed_style_to_map: BUG-495 additions ─────────────────────────────

    #[test]
    fn computed_map_background_position_xy_default_top_left() {
        // No layer at all — falls back to the `background-position` initial
        // value (`0% 0%`), same as an explicit single default layer would.
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("background-position-x").map(String::as_str), Some("0%"));
        assert_eq!(m.get("background-position-y").map(String::as_str), Some("0%"));
    }

    #[test]
    fn computed_map_background_position_x_percent_and_px() {
        let m = div_computed_map("<div>x</div>", "div { background-position-x: 25%; }");
        assert_eq!(m.get("background-position-x").map(String::as_str), Some("25%"));
        let m = div_computed_map("<div>x</div>", "div { background-position-y: 10px; }");
        assert_eq!(m.get("background-position-y").map(String::as_str), Some("10px"));
    }

    #[test]
    fn computed_map_background_position_x_multi_layer_joins_commas() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { background-image: url(a.png), url(b.png); background-position-x: 10%, 90%; }",
        );
        assert_eq!(m.get("background-position-x").map(String::as_str), Some("10%, 90%"));
    }

    #[test]
    fn computed_map_border_color_uniform_sides() {
        let m = div_computed_map("<div>x</div>", "div { border-color: red; }");
        assert_eq!(m.get("border-color").map(String::as_str), Some("rgb(255, 0, 0)"));
    }

    #[test]
    fn computed_map_border_color_empty_when_sides_differ() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { border-top-color: red; border-right-color: blue; \
             border-bottom-color: red; border-left-color: red; }",
        );
        assert_eq!(m.get("border-color").map(String::as_str), Some(""));
    }

    #[test]
    fn computed_map_border_spacing_single_value_when_equal() {
        let m = div_computed_map("<div>x</div>", "div { border-spacing: 10px; }");
        assert_eq!(m.get("border-spacing").map(String::as_str), Some("10px"));
    }

    #[test]
    fn computed_map_border_spacing_two_values_when_differ() {
        let m = div_computed_map("<div>x</div>", "div { border-spacing: 10px 20px; }");
        assert_eq!(m.get("border-spacing").map(String::as_str), Some("10px 20px"));
    }

    // ──────────────── CSSOM-3 срез 2: CSS Logical Properties L1 ────────────────

    #[test]
    fn computed_map_logical_sizing_mirrors_physical() {
        let m = div_computed_map("<div>x</div>", "div { inline-size: 120px; block-size: 80px; }");
        assert_eq!(m.get("inline-size").map(String::as_str), Some("120px"));
        assert_eq!(m.get("block-size").map(String::as_str), Some("80px"));
        assert_eq!(m.get("width").map(String::as_str), Some("120px"));
        assert_eq!(m.get("height").map(String::as_str), Some("80px"));
    }

    #[test]
    fn computed_map_logical_margin_padding_start_end_mirror_physical() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { margin-inline-start: 5px; margin-inline-end: 6px; \
             padding-block-start: 7px; padding-block-end: 8px; }",
        );
        assert_eq!(m.get("margin-inline-start").map(String::as_str), Some("5px"));
        assert_eq!(m.get("margin-left").map(String::as_str), Some("5px"));
        assert_eq!(m.get("margin-inline-end").map(String::as_str), Some("6px"));
        assert_eq!(m.get("margin-right").map(String::as_str), Some("6px"));
        assert_eq!(m.get("padding-block-start").map(String::as_str), Some("7px"));
        assert_eq!(m.get("padding-top").map(String::as_str), Some("7px"));
        assert_eq!(m.get("padding-block-end").map(String::as_str), Some("8px"));
        assert_eq!(m.get("padding-bottom").map(String::as_str), Some("8px"));
    }

    #[test]
    fn computed_map_logical_inset_mirrors_physical() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { position: absolute; inset-inline-start: 1px; inset-inline-end: 2px; \
             inset-block-start: 3px; inset-block-end: 4px; }",
        );
        assert_eq!(m.get("inset-inline-start").map(String::as_str), Some("1px"));
        assert_eq!(m.get("left").map(String::as_str), Some("1px"));
        assert_eq!(m.get("inset-inline-end").map(String::as_str), Some("2px"));
        assert_eq!(m.get("right").map(String::as_str), Some("2px"));
        assert_eq!(m.get("inset-block-start").map(String::as_str), Some("3px"));
        assert_eq!(m.get("top").map(String::as_str), Some("3px"));
        assert_eq!(m.get("inset-block-end").map(String::as_str), Some("4px"));
        assert_eq!(m.get("bottom").map(String::as_str), Some("4px"));
    }

    #[test]
    fn computed_map_logical_border_width_style_color_mirror_physical() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { border-inline-start-width: 2px; border-inline-start-style: dashed; \
             border-inline-start-color: red; }",
        );
        assert_eq!(m.get("border-inline-start-width").map(String::as_str), Some("2px"));
        assert_eq!(m.get("border-left-width").map(String::as_str), Some("2px"));
        assert_eq!(m.get("border-inline-start-style").map(String::as_str), Some("dashed"));
        assert_eq!(m.get("border-left-style").map(String::as_str), Some("dashed"));
        assert_eq!(m.get("border-inline-start-color").map(String::as_str), Some("rgb(255, 0, 0)"));
        assert_eq!(m.get("border-left-color").map(String::as_str), Some("rgb(255, 0, 0)"));
    }

    #[test]
    fn computed_map_logical_two_value_shorthand_collapses_when_equal() {
        let m = div_computed_map("<div>x</div>", "div { margin-inline: 10px; }");
        assert_eq!(m.get("margin-inline").map(String::as_str), Some("10px"));
    }

    #[test]
    fn computed_map_logical_two_value_shorthand_keeps_both_when_differ() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { margin-inline-start: 10px; margin-inline-end: 20px; }",
        );
        assert_eq!(m.get("margin-inline").map(String::as_str), Some("10px 20px"));
    }

    #[test]
    fn computed_map_logical_properties_absent_outside_horizontal_tb_ltr() {
        // Phase 0 (`resolve_logical_properties`) only resolves logical props
        // onto physical fields for horizontal-tb/LTR — the CSSOM mirror gates
        // the same way rather than serve a made-up value for vertical modes.
        let m = div_computed_map(
            "<div>x</div>",
            "div { writing-mode: vertical-rl; inline-size: 120px; }",
        );
        assert_eq!(m.get("inline-size"), None);
    }

    // ── computed_style_to_map: CSSOM-3 slice 3 (BUG-537 family) ──────────────

    #[test]
    fn computed_map_object_fit_and_position_defaults() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("object-fit").map(String::as_str), Some("fill"));
        assert_eq!(m.get("object-position").map(String::as_str), Some("50% 50%"));
        assert_eq!(m.get("image-rendering").map(String::as_str), Some("auto"));
    }

    #[test]
    fn computed_map_object_fit_and_position_set() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { object-fit: cover; object-position: left 20px; image-rendering: pixelated; }",
        );
        assert_eq!(m.get("object-fit").map(String::as_str), Some("cover"));
        assert_eq!(m.get("object-position").map(String::as_str), Some("0% 20px"));
        assert_eq!(m.get("image-rendering").map(String::as_str), Some("pixelated"));
    }

    #[test]
    fn computed_map_vertical_align_keyword_default_and_set() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("vertical-align").map(String::as_str), Some("baseline"));
        let m = div_computed_map("<div>x</div>", "div { vertical-align: text-top; }");
        assert_eq!(m.get("vertical-align").map(String::as_str), Some("text-top"));
    }

    #[test]
    fn computed_map_vertical_align_percent_and_length() {
        let m = div_computed_map("<div>x</div>", "div { vertical-align: 25%; }");
        assert_eq!(m.get("vertical-align").map(String::as_str), Some("25%"));
        let m = div_computed_map("<div>x</div>", "div { vertical-align: 4px; }");
        assert_eq!(m.get("vertical-align").map(String::as_str), Some("4px"));
    }

    // ── computed_style_to_map: BUG-505 additions ─────────────────────────────

    #[test]
    fn computed_map_line_clamp_defaults_to_none() {
        // Both names were absent entirely before BUG-505, so `getComputedStyle`
        // answered "" instead of the property's own initial value.
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("line-clamp").map(String::as_str), Some("none"));
        assert_eq!(m.get("-webkit-line-clamp").map(String::as_str), Some("none"));
    }

    #[test]
    fn computed_map_line_clamp_reports_integer() {
        let m = div_computed_map("<div>x</div>", "div { -webkit-line-clamp: 3; }");
        assert_eq!(m.get("line-clamp").map(String::as_str), Some("3"));
        assert_eq!(m.get("-webkit-line-clamp").map(String::as_str), Some("3"));
    }

    #[test]
    fn computed_map_webkit_box_orient_and_continue_default() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("-webkit-box-orient").map(String::as_str), Some("horizontal"));
        assert_eq!(m.get("continue").map(String::as_str), Some("normal"));
    }

    #[test]
    fn computed_map_webkit_box_orient_and_continue_set() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { -webkit-box-orient: vertical; continue: discard; }",
        );
        assert_eq!(m.get("-webkit-box-orient").map(String::as_str), Some("vertical"));
        assert_eq!(m.get("continue").map(String::as_str), Some("discard"));
    }

    // ── computed_style_to_map: BUG-505 срез 5, `display: -webkit-box` quirk
    // (CSS Overflow L4 §continue / WHATWG Compat §2.1) — matrix transcribed
    // from `css/css-overflow/parsing/webkit-box-computed.html`.

    #[test]
    fn webkit_box_display_computes_as_specified_without_orient_or_clamp() {
        let m = div_computed_map("<div>x</div>", "div { display: -webkit-box; }");
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_box_display_unaffected_by_orient_alone() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-box-orient: vertical; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_box_display_unaffected_by_vertical_orient_plus_clamp_none() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: none; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_box_display_unaffected_by_clamp_without_vertical_orient() {
        // Default box-orient is horizontal — clamp alone doesn't trigger the quirk.
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_box_display_unaffected_by_explicit_horizontal_orient_plus_clamp() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-box-orient: horizontal; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_box_display_becomes_flow_root_with_vertical_orient_and_clamp() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("flow-root"));
    }

    #[test]
    fn webkit_inline_box_display_becomes_inline_block_with_vertical_orient_and_clamp() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-inline-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("inline-block"));
    }

    #[test]
    fn webkit_box_display_becomes_flow_root_with_continue_discard() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-box-orient: vertical; continue: discard; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("flow-root"));
    }

    #[test]
    fn webkit_box_display_unaffected_by_continue_discard_without_vertical_orient() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; continue: discard; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_box_display_unaffected_by_continue_none() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-box; -webkit-box-orient: vertical; continue: none; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
    }

    #[test]
    fn webkit_flex_alias_ignores_the_webkit_box_quirk() {
        // WPT explicitly asserts `-webkit-flex`/`flex`/`inline-flex` do NOT
        // get the quirk even under the identical orient+clamp combination
        // that turns `-webkit-box` into `flow-root` — they alias straight
        // to `Display::Flex` at parse time and never become `WebkitBox`.
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: flex; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("flex"));
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-flex; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("flex"));
        let m = div_computed_map(
            "<div>x</div>",
            "div { display: -webkit-inline-flex; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
        );
        assert_eq!(m.get("display").map(String::as_str), Some("inline-flex"));
    }

    #[test]
    fn computed_map_scrollbar_gutter_defaults_to_auto() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("scrollbar-gutter").map(String::as_str), Some("auto"));
    }

    #[test]
    fn computed_map_scrollbar_gutter_reports_stable_both_edges() {
        let m = div_computed_map("<div>x</div>", "div { scrollbar-gutter: stable both-edges; }");
        assert_eq!(m.get("scrollbar-gutter").map(String::as_str), Some("stable both-edges"));
        // CSS Overflow L4 §3.3 double-bar grammar: token order doesn't matter.
        let m = div_computed_map("<div>x</div>", "div { scrollbar-gutter: both-edges stable; }");
        assert_eq!(m.get("scrollbar-gutter").map(String::as_str), Some("stable both-edges"));
    }

    #[test]
    fn computed_map_overflow_clip_margin_defaults_to_zero() {
        let m = div_computed_map("<div>x</div>", "");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("0px"));
    }

    #[test]
    fn computed_map_overflow_clip_margin_reports_length() {
        let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: 10px; }");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("10px"));
    }

    // BUG-505 срез 4: `overflow-clip-margin`'s `<visual-box>` component,
    // confirmed against WPT `overflow-clip-margin-computed.html`.

    #[test]
    fn computed_map_overflow_clip_margin_box_alone_elides_zero_length() {
        let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: content-box; }");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("content-box"));
        let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: border-box 0px; }");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("border-box"));
    }

    #[test]
    fn computed_map_overflow_clip_margin_box_and_length_reorder_canonical() {
        let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: 10px content-box; }");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("content-box 10px"));
    }

    #[test]
    fn computed_map_overflow_clip_margin_padding_box_elides_keyword() {
        let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: padding-box 10px; }");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("10px"));
    }

    #[test]
    fn computed_map_overflow_clip_margin_resolves_calc_of_absolute_units() {
        // `calc(100px - 50px)` — both operands absolute, resolves to a
        // plain px number at computed-value time (unlike the specified
        // value, which keeps the `calc(...)` text).
        let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: calc(100px - 50px); }");
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("50px"));
    }

    #[test]
    fn computed_map_overflow_clip_margin_resolves_calc_with_em() {
        // Default computed font-size is 16px, so `0.5em` resolves to 8px —
        // WPT `overflow-clip-margin-computed.html`'s own case (108px at its
        // default font-size, same 16px root).
        let m = div_computed_map(
            "<div>x</div>",
            "div { overflow-clip-margin: calc(0.5em + 100px); }",
        );
        assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("108px"));
    }

    // BUG-505 срез 3: `overflow` shorthand computed-value serialization
    // (`css/css-overflow/parsing/overflow-computed.html`,
    // `overflow-shorthand-001.html`) — collapses to one keyword when both
    // axes agree, else `"x y"`, reading the axis-coerced values.
    #[test]
    fn computed_map_overflow_shorthand_collapses_equal_axes() {
        let m = div_computed_map("<div>x</div>", "div { overflow: hidden; }");
        assert_eq!(m.get("overflow").map(String::as_str), Some("hidden"));
    }

    #[test]
    fn computed_map_overflow_shorthand_reports_pair() {
        let m = div_computed_map("<div>x</div>", "div { overflow-x: hidden; overflow-y: scroll; }");
        assert_eq!(m.get("overflow").map(String::as_str), Some("hidden scroll"));
    }

    #[test]
    fn computed_map_overflow_shorthand_reflects_axis_coercion() {
        // overflow-x: visible + overflow-y: hidden → overflow-x computes to
        // `auto` (CSS Overflow L3 §2.1), so the shorthand reports "auto hidden",
        // not the raw specified "visible hidden".
        let m = div_computed_map("<div>x</div>", "div { overflow-x: visible; overflow-y: hidden; }");
        assert_eq!(m.get("overflow-x").map(String::as_str), Some("auto"));
        assert_eq!(m.get("overflow").map(String::as_str), Some("auto hidden"));
    }

    // BUG-505 срез 3: `overflow-block`/`overflow-inline` (CSS Overflow L3
    // §logical) map to `overflow-y`/`overflow-x` under `horizontal-tb` and
    // swap to `overflow-x`/`overflow-y` under a vertical writing mode
    // (`css/css-overflow/logical-overflow-001.html`).
    #[test]
    fn computed_map_overflow_logical_horizontal_tb() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { overflow-block: hidden; overflow-inline: scroll; }",
        );
        assert_eq!(m.get("overflow-x").map(String::as_str), Some("scroll"));
        assert_eq!(m.get("overflow-y").map(String::as_str), Some("hidden"));
        assert_eq!(m.get("overflow-block").map(String::as_str), Some("hidden"));
        assert_eq!(m.get("overflow-inline").map(String::as_str), Some("scroll"));
    }

    #[test]
    fn computed_map_overflow_logical_vertical_rl_swaps_axes() {
        let m = div_computed_map(
            "<div>x</div>",
            "div { writing-mode: vertical-rl; overflow-block: hidden; overflow-inline: scroll; }",
        );
        assert_eq!(m.get("overflow-x").map(String::as_str), Some("hidden"));
        assert_eq!(m.get("overflow-y").map(String::as_str), Some("scroll"));
        assert_eq!(m.get("overflow-block").map(String::as_str), Some("hidden"));
        assert_eq!(m.get("overflow-inline").map(String::as_str), Some("scroll"));
    }
}
