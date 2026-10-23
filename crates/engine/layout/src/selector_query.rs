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
use crate::ruby::{RubyAlign, RubyMerge};
use crate::style::{
    SvgColorInterpolation,
    matches_complex, AlignValue, AnimationDirection, AnimationFillMode, AnimationPlayState,
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin,
    BackgroundRepeat, BackgroundSize, BgSizeAxis, RepeatStyle, BlockStepAlign, BlockStepInsert, BlockStepRound,
    BorderStyle, BoxShadow, BoxSizing,
    ClearSide, Color, ColorScheme,
    FlexBasis, FlexDirection, FlexWrap,
    ContainerType, Content, ContentItem, ContentVisibility,
    CssColor, CssContinue,
    Cursor, Direction, Display, FillRule, FilterFn, FloatSide, ForcedColorAdjust, FontStretch,
    FontStyle, FontWeight,
    FontVariantCaps, FontVariantEmoji, FontVariantLigatures, FontVariantNumeric,
    FontVariantPosition, ImageRendering, InterpolateSizeMode, Isolation, IterationCount, Length,
    LengthOrAuto,
    MixBlendMode, ObjectFit, ObjectPosition, Overflow, OverflowAnchor, overflow_clip_margin_serialize,
    OutlineColor, OverscrollBehavior, ScrollBehavior,
    OutlineStyle, PointerEvents, Position, PositionComponent, PrintColorAdjust, Quotes,
    ScrollbarGutter, ScrollbarWidth, StepPosition, StrokeLinecap, StrokeLinejoin, SvgPaint, TextAlign,
    TextAlignLast, TextDecorationLine, TextDecorationStyle, TextWrapMode, TextWrapStyle,
    Hyphens, LineBreak, OverflowWrap, WordBreak,
    TextEmphasisStyle, TextOrientation, TextShadow, TextTransform, TimingFunction,
    TransformFn, UnicodeBidi, VerticalAlign, Visibility, WebkitBoxAlign, WebkitBoxDirection, WebkitBoxOrient, WebkitBoxPack, WhiteSpace,
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

// LAYOUT-1 срез 3: явный стек вместо рекурсии по DOM — `querySelector`
// пересчитывается на каждый вызов из JS, потенциально на глубоко вложенном
// дереве (BUG-987). LIFO-стек с детьми в обратном порядке посещает узлы в
// том же pre-order, что и рекурсия, так что "первый матч в document order"
// не меняется; ранний `return` на первом совпадении сохранён как есть.
fn find_dom_rec(
    doc: &Document,
    id: lumen_dom::NodeId,
    selectors: &[ComplexSelector],
) -> Option<lumen_dom::NodeId> {
    let mut stack = vec![id];
    while let Some(id) = stack.pop() {
        if node_matches(id, doc, selectors) {
            return Some(id);
        }
        stack.extend(doc.get(id).children.iter().rev().copied());
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

// LAYOUT-1 срез 3: явный стек вместо рекурсии по DOM — см. `find_dom_rec`.
// `querySelectorAll` без `.clone()` для `children`: заимствование живёт только
// на время `stack.extend`, следующая итерация заново берёт `doc.get(id)`, так
// что конфликта заимствований, ради которого клонировали в `query_all_within`,
// здесь нет.
fn query_all_rec(
    doc: &Document,
    id: NodeId,
    selectors: &[ComplexSelector],
    out: &mut Vec<NodeId>,
) {
    let mut stack = vec![id];
    while let Some(id) = stack.pop() {
        // matches_complex returns false for non-element nodes internally.
        if node_matches(id, doc, selectors) {
            out.push(id);
        }
        stack.extend(doc.get(id).children.iter().rev().copied());
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
pub(crate) fn px_str(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}px", v as i64)
    } else {
        format!("{}px", v)
    }
}

/// Computed-value text of `abs px + pct × 100%` when the percentage part is non-zero
/// (`110%`, `calc(-15% + 10px)`); `None` for a plain length. Same term order as CSS Values 4
/// §10.10: percentage first.
fn length_percentage_css(abs_px: f32, pct: f32) -> Option<String> {
    let round = |v: f32| (v * 1000.0).round() / 1000.0;
    let pct = round(pct * 100.0);
    if pct == 0.0 {
        return None;
    }
    let abs = round(abs_px);
    Some(if abs == 0.0 {
        format!("{}%", number_str(pct))
    } else if abs < 0.0 {
        format!("calc({}% - {})", number_str(pct), px_str(-abs))
    } else {
        format!("calc({}% + {})", number_str(pct), px_str(abs))
    })
}

/// Serialises one `aspect-ratio` ratio component as a bare CSS `<number>`
/// (no unit) — omits the decimal point for whole-number values, matching
/// `test_computed_value("aspect-ratio", …)`'s expected `"auto W / H"` form.
fn number_str(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// CSS Rhythmic Sizing L1 §3.1 (BUG-517) — computed-value serialization for
/// the `block-step` shorthand: `none` when all four longhands are at their
/// initial value, else the non-initial ones joined in `size insert align
/// round` order (the grammar's own declaration order) with the initial ones
/// elided. Confirmed against every `test_computed_value("block-step", …)`
/// case in the vendored `block-step-computed.html`.
fn block_step_shorthand_computed(style: &ComputedStyle) -> String {
    let is_default = style.block_step_size.is_none()
        && style.block_step_insert == BlockStepInsert::MarginBox
        && style.block_step_align == BlockStepAlign::Auto
        && style.block_step_round == BlockStepRound::Up;
    if is_default {
        return "none".to_string();
    }
    let mut parts = Vec::with_capacity(4);
    if let Some(px) = style.block_step_size {
        parts.push(px_str(px));
    }
    if style.block_step_insert != BlockStepInsert::MarginBox {
        parts.push(style.block_step_insert.to_css().to_string());
    }
    if style.block_step_align != BlockStepAlign::Auto {
        parts.push(style.block_step_align.to_css().to_string());
    }
    if style.block_step_round != BlockStepRound::Up {
        parts.push(style.block_step_round.to_css().to_string());
    }
    parts.join(" ")
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
        format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, alpha_to_css(c.a))
    }
}

/// CSS Color 4 §15.2 / Chrome: the shortest decimal (two digits, else three) that rounds back to
/// the same 8-bit alpha — `128` serialises as `0.5`, not `0.502` (BUG-1424).
fn alpha_to_css(a: u8) -> String {
    let exact = f32::from(a) / 255.0;
    let two = (exact * 100.0).round() / 100.0;
    let v = if (two * 255.0).round() as u8 == a { two } else { (exact * 1000.0).round() / 1000.0 };
    // `{}` prints the shortest round-trip form (`0.5`, `0.2`, `0.502`), without trailing zeros.
    format!("{v}")
}

/// Serialises a [`CssColor`] — `CurrentColor` becomes `"currentcolor"`.
/// [`css_color_to_css`], but `currentcolor` is the used colour `current`, as `getComputedStyle`
/// reports it (BUG-1297, `border-color-computed`).
fn used_css_color(c: &CssColor, current: Color) -> String {
    match c {
        CssColor::CurrentColor => color_to_css(current),
        other => css_color_to_css(other),
    }
}

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
/// Gradients, `image-set()` and `cross-fade()` carry their computed text in
/// [`BackgroundLayer::image_css`] (BUG-1428, built by `parse::image_computed`);
/// `url()` is re-quoted from the stored path. `paint()` and the forms that
/// module does not recognise (legacy `-webkit-*-gradient`) serialise as `"none"`.
fn background_layers_to_css(layers: &[BackgroundLayer]) -> String {
    if layers.is_empty() {
        return "none".into();
    }
    layers
        .iter()
        .map(|l| match (&l.image_css, &l.image) {
            (Some(css), _) => css.to_string(),
            (None, BackgroundImage::Url(s)) => {
                format!("url(\"{}\")", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
            (
                None,
                BackgroundImage::None
                | BackgroundImage::Gradient(_)
                | BackgroundImage::CrossFade { .. }
                | BackgroundImage::Paint { .. },
            ) => "none".to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Serialises one axis of a `background-position`/`object-position` value —
/// `Px` as `"<n>px"`, `Percent` (stored as a `0.0..=1.0` fraction) as `"<n>%"`.
pub(crate) fn position_component_to_css(c: PositionComponent) -> String {
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
        // Смещение от дальнего края — `calc(<pct>% ± <px>px)` (Typed OM /
        // CSS Values L4 §10.1 сериализация mixed length-percentage).
        PositionComponent::PercentPlusPx { percent, px } => {
            let pct = percent * 100.0;
            let pct_s = if pct.fract() == 0.0 { format!("{}", pct as i64) } else { format!("{pct}") };
            let (sign, mag) = if px < 0.0 { ('-', -px) } else { ('+', px) };
            format!("calc({pct_s}% {sign} {})", px_str(mag))
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
    specified: &[PositionComponent],
    axis: impl Fn(&BackgroundLayer) -> PositionComponent,
) -> String {
    if !specified.is_empty() {
        return specified.iter().map(|c| position_component_to_css(*c)).collect::<Vec<_>>().join(", ");
    }
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
/// Computed-значение `<length-percentage>`: `em` и `calc()` без процентов сворачиваются в px
/// по `font_size`, остальное — как [`length_to_css`] (BUG-1325, `text-indent`).
fn absolute_length_css(l: &Length, font_size: f32) -> String {
    match l {
        Length::Em(v) => px_str(v * font_size),
        Length::Calc(node) => match node.resolve(font_size, None, Size::ZERO) {
            Some(px) => px_str(px),
            None => length_to_css(l),
        },
        other => length_to_css(other),
    }
}

/// Computed-значение `min-/max-width|height` (CSS Sizing L3 §4): `<length-percentage [0,∞]>` с
/// `em`/`calc()` без процентов сворачивается в px по `font_size` (отрицательная сумма — `0px`),
/// `fit-content(<lp>)` сворачивает свой аргумент; ключевые слова и значения с `%` — как есть
/// (BUG-1391).
fn sizing_computed_css(l: &Length, font_size: f32) -> String {
    match l {
        Length::FitContent(Some(arg)) => format!("fit-content({})", sizing_computed_css(arg, font_size)),
        Length::Em(v) => px_str((v * font_size).max(0.0)),
        Length::Calc(node) => match node.resolve(font_size, None, Size::ZERO) {
            Some(px) => px_str(px.max(0.0)),
            None => length_to_css(l),
        },
        other => length_to_css(other),
    }
}

fn text_wrap_mode_css(v: TextWrapMode) -> &'static str {
    match v {
        TextWrapMode::Wrap => "wrap",
        TextWrapMode::Nowrap => "nowrap",
    }
}

fn text_wrap_style_css(v: TextWrapStyle) -> &'static str {
    match v {
        TextWrapStyle::Auto => "auto",
        TextWrapStyle::Balance => "balance",
        TextWrapStyle::Stable => "stable",
        TextWrapStyle::Pretty => "pretty",
    }
}

pub(crate) fn length_to_css(l: &Length) -> String {
    match l {
        Length::Px(v) => px_str(*v),
        Length::Em(v) => format!("{}em", v),
        Length::Rem(v) => format!("{}rem", v),
        Length::Ch(v) => format!("{}ch", v),
        Length::Ex(v) => format!("{}ex", v),
        Length::Cap(v) => format!("{}cap", v),
        Length::Lh(v) => format!("{}lh", v),
        Length::Rlh(v) => format!("{}rlh", v),
        Length::Rex(v) => format!("{}rex", v),
        Length::Rch(v) => format!("{}rch", v),
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
        Length::Stretch => "stretch".into(),
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

/// Un-zooms a [`Length`] for `getComputedStyle()` reporting (CSS Viewport L1
/// §5, `#zoom-om`): `cascade.rs::apply_zoom_to_lengths` bakes `effective_zoom`
/// into already-resolved `Length::Px` box-model lengths, so the raw
/// `ComputedStyle` no longer holds the "as if zoom weren't applied" computed
/// value the CSSOM getter must return. Other units pass through unchanged —
/// `apply_zoom_to_lengths` never scaled them either (they resolve later
/// against an already-zoomed basis).
fn unzoom_length_to_css(l: &Length, z: f32) -> String {
    match l {
        Length::Px(v) => px_str(*v / z),
        _ => length_to_css(l),
    }
}

/// [`unzoom_length_to_css`] for the `<length> | auto` fields (margin/inset).
fn unzoom_length_or_auto_to_css(l: &LengthOrAuto, z: f32) -> String {
    match l {
        LengthOrAuto::Auto => "auto".into(),
        LengthOrAuto::Length(len) => unzoom_length_to_css(len, z),
    }
}

/// Serialises a [`LengthOrAuto`] — `Auto` becomes `"auto"`.
fn length_or_auto_to_css(l: &LengthOrAuto) -> String {
    match l {
        LengthOrAuto::Auto => "auto".into(),
        LengthOrAuto::Length(len) => length_to_css(len),
    }
}

pub(crate) fn border_style_to_css(bs: BorderStyle) -> &'static str {
    match bs {
        BorderStyle::None => "none",
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Double => "double",
        BorderStyle::Hidden => "hidden",
        BorderStyle::Groove => "groove",
        BorderStyle::Ridge => "ridge",
        BorderStyle::Inset => "inset",
        BorderStyle::Outset => "outset",
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
        AlignValue::LastBaseline => "last baseline",
        AlignValue::SpaceBetween => "space-between",
        AlignValue::SpaceAround => "space-around",
        AlignValue::SpaceEvenly => "space-evenly",
    }
}

/// Serialize a CSS `opacity` computed value — shared with
/// `AnimationFrame::to_computed_style_patches` (GAP-CSSANIM срез 3) so a
/// live interpolated `opacity` reads back identically to a static one.
pub(crate) fn opacity_to_css(v: f32) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{}", v) }
}

/// Key marking `prop` of a snapshot entry as written by the frame loop's scheduler
/// (`patch_animated_computed_styles`) rather than produced by a cascade: the JS
/// read-time transition overlay must not take such a value for a style change. A
/// full snapshot replaces the entry, which drops the marker with it.
pub fn animated_marker_key(prop: &str) -> String {
    format!("{}-lumen-animated:{prop}", crate::resolved_geometry::COMPUTED_VALUE_KEY_PREFIX)
}

/// Key under which the snapshot keeps a box's `transform` as a function list
/// (`translate(200px, 0px) rotate(12.566371rad)`) next to the resolved `matrix()` —
/// under [`COMPUTED_VALUE_KEY_PREFIX`](crate::resolved_geometry::COMPUTED_VALUE_KEY_PREFIX),
/// so it is not a property to any reader.
pub const TRANSFORM_LIST_KEY: &str = "computed:-lumen-transform-list";

/// A transform list as text `parse_transform_list` reads back exactly: angles stay in
/// radians (the parser's unit, an f32 round-trip), lengths keep their units.
pub(crate) fn transform_fns_to_list_css(list: &[TransformFn]) -> String {
    let px = |v: f32| px_str(v);
    let rad = |v: f32| format!("{v}rad");
    list.iter()
        .map(|f| match f {
            TransformFn::Translate(x, y) => format!("translate({}, {})", px(*x), px(*y)),
            TransformFn::TranslateX(x) => format!("translateX({})", px(*x)),
            TransformFn::TranslateY(y) => format!("translateY({})", px(*y)),
            TransformFn::TranslateZ(z) => format!("translateZ({})", px(*z)),
            TransformFn::Translate3d(x, y, z) => format!("translate3d({}, {}, {})", px(*x), px(*y), px(*z)),
            TransformFn::TranslateLp(x, y, z) => {
                format!("translate3d({}, {}, {})", length_to_css(x), length_to_css(y), px(*z))
            }
            TransformFn::Rotate(a) => format!("rotate({})", rad(*a)),
            TransformFn::RotateX(a) => format!("rotateX({})", rad(*a)),
            TransformFn::RotateY(a) => format!("rotateY({})", rad(*a)),
            TransformFn::RotateZ(a) => format!("rotateZ({})", rad(*a)),
            TransformFn::Rotate3d(x, y, z, a) => format!("rotate3d({x}, {y}, {z}, {})", rad(*a)),
            TransformFn::Scale(x, y) => format!("scale({x}, {y})"),
            TransformFn::ScaleX(x) => format!("scaleX({x})"),
            TransformFn::ScaleY(y) => format!("scaleY({y})"),
            TransformFn::ScaleZ(z) => format!("scaleZ({z})"),
            TransformFn::Scale3d(x, y, z) => format!("scale3d({x}, {y}, {z})"),
            TransformFn::SkewX(a) => format!("skewX({})", rad(*a)),
            TransformFn::SkewY(a) => format!("skewY({})", rad(*a)),
            TransformFn::Matrix(v) => {
                format!("matrix({})", v.iter().map(f32::to_string).collect::<Vec<_>>().join(", "))
            }
            TransformFn::Matrix3d(v) => {
                format!("matrix3d({})", v.iter().map(f32::to_string).collect::<Vec<_>>().join(", "))
            }
            TransformFn::Perspective(d) => format!("perspective({})", px(*d)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Serialize one CSS `transform` function to its computed-style text —
/// shared with `AnimationFrame::to_computed_style_patches` (GAP-CSSANIM
/// срез 3) so a live interpolated `transform` reads back identically to a
/// static one.
pub(crate) fn transform_list_to_css(list: &[TransformFn]) -> String {
    if list.is_empty() {
        return "none".into();
    }
    // CSSOM §9 / Transforms L1 §18: resolved value — `matrix()` для 2D,
    // иначе `matrix3d()`, а не заданный список функций (BUG-1157).
    let m = crate::property_trees::compute_local_transform(list, (0.0, 0.0, 0.0), crate::property_trees::TransformRef::default());
    let v = &m.0;
    if m.is_2d_affine() {
        let parts = [v[0], v[1], v[4], v[5], v[12], v[13]].map(matrix_num);
        format!("matrix({})", parts.join(", "))
    } else {
        format!("matrix3d({})", v.map(matrix_num).join(", "))
    }
}

/// CSSOM resolved value of a `<number>` (CSS Values L4 §10.9.1): at most six
/// significant digits like Chrome, `e+NN` form outside `1e-5..1e21`, signed zero kept.
pub(crate) fn css_number(v: f32) -> String {
    if v.is_nan() || v == 0.0 {
        return if v.is_sign_negative() && !v.is_nan() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{:.5e}", f64::from(v));
    let rounded: f64 = sci.parse().unwrap_or(f64::from(v));
    let a = rounded.abs();
    if (1e-5..1e21).contains(&a) {
        return format!("{rounded}");
    }
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let mantissa = if mantissa.contains('.') { mantissa.trim_end_matches('0').trim_end_matches('.') } else { mantissa };
    let exp: i32 = exp.parse().unwrap_or(0);
    format!("{mantissa}e{}{}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

/// `translate` resolved value: `none`, or `<x>` / `<x> <y>` with a zero `y` dropped.
fn translate_to_css(t: Option<&(Length, Length)>) -> String {
    match t {
        None => "none".into(),
        Some((x, Length::Px(0.0))) => length_to_css(x),
        Some((x, y)) => format!("{} {}", length_to_css(x), length_to_css(y)),
    }
}

/// `rotate` resolved value: `none` or the angle in degrees.
fn rotate_to_css(r: Option<f32>) -> String {
    match r {
        None => "none".into(),
        Some(rad) => format!("{}deg", css_number(rad.to_degrees())),
    }
}

/// `scale` resolved value: `none`, or `<x>` when both factors agree, else `<x> <y>`.
fn scale_to_css(s: Option<(f32, f32)>) -> String {
    match s {
        None => "none".into(),
        Some((x, y)) if x.to_bits() == y.to_bits() => css_number(x),
        Some((x, y)) => format!("{} {}", css_number(x), css_number(y)),
    }
}

/// Число в сериализации матрицы: до 6 значащих цифр, как у Chrome;
/// микроскопический f32-шум от `sin/cos` даёт `-0` → `0`.
fn matrix_num(v: f32) -> String {
    let v = f64::from(v);
    if v == 0.0 {
        return "0".into();
    }
    let r: f64 = format!("{:.5e}", v).parse().unwrap_or(v);
    if r.abs() < 1e-4 { format!("{:e}", r) } else { format!("{}", r) }
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
        FilterFn::Url(u) => format!("url(\"#{}\")", u.id),
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

pub(crate) fn background_repeat_to_css(r: BackgroundRepeat) -> String {
    fn axis(s: RepeatStyle) -> &'static str {
        match s {
            RepeatStyle::Repeat => "repeat",
            RepeatStyle::NoRepeat => "no-repeat",
            RepeatStyle::Round => "round",
            RepeatStyle::Space => "space",
        }
    }
    match r {
        BackgroundRepeat::REPEAT_X => "repeat-x".into(),
        BackgroundRepeat::REPEAT_Y => "repeat-y".into(),
        BackgroundRepeat { x, y } if x == y => axis(x).into(),
        BackgroundRepeat { x, y } => format!("{} {}", axis(x), axis(y)),
    }
}

fn background_attachment_to_css(a: BackgroundAttachment) -> &'static str {
    match a {
        BackgroundAttachment::Scroll => "scroll",
        BackgroundAttachment::Fixed => "fixed",
        BackgroundAttachment::Local => "local",
    }
}

pub(crate) fn background_origin_to_css(o: BackgroundOrigin) -> &'static str {
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

pub(crate) fn background_size_to_css(s: BackgroundSize) -> String {
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
fn background_layer_field_to_css<T: Copy, S: Into<String>>(
    layers: &[BackgroundLayer],
    specified: &[T],
    initial: T,
    extract: impl Fn(&BackgroundLayer) -> T,
    to_css: impl Fn(T) -> S,
) -> String {
    // The author's own list wins: its length is not the layer count (BUG-1297).
    if !specified.is_empty() {
        return specified.iter().map(|v| to_css(*v).into()).collect::<Vec<String>>().join(", ");
    }
    if layers.is_empty() {
        return to_css(initial).into();
    }
    layers.iter().map(|l| to_css(extract(l)).into()).collect::<Vec<String>>().join(", ")
}

pub(crate) fn fill_rule_to_css(r: FillRule) -> &'static str {
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
        SvgPaint::Gradient(_) | SvgPaint::Pattern(_) => "none".into(),
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

/// Shortest serialisation of a four-sided shorthand (CSS Values §… `margin`
/// style): 1 value when all agree, 2 when top=bottom and left=right, 3 when
/// only left=right, else 4.
fn box_sides_shorthand(t: &str, r: &str, b: &str, l: &str) -> String {
    if t == r && r == b && b == l {
        t.to_string()
    } else if t == b && r == l {
        format!("{t} {r}")
    } else if r == l {
        format!("{t} {r} {b}")
    } else {
        format!("{t} {r} {b} {l}")
    }
}

fn box_shadow_list_to_css(v: &[BoxShadow], current: Color) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.iter()
        .map(|s| {
            // `currentcolor` serialises as the used colour, like Chrome (BUG-1297).
            let color = color_to_css(s.color.unwrap_or(current));
            // CSSOM: the colour serialises first (BUG-1050).
            let mut out = format!(
                "{color} {} {} {} {}",
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
            format!("{color} {} {} {}", px_str(s.offset_x), px_str(s.offset_y), px_str(s.blur))
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
        && (style.line_clamp.is_some() || style.line_clamp_auto || style.continue_value == CssContinue::Discard);
    if let Some((display, form)) = style.display_form
        && display == style.display
        && style.legacy_box_display.is_none()
    {
        return form;
    }
    match style.legacy_box_display.unwrap_or(style.display) {
        Display::Block => "block",
        Display::Inline => "inline",
        Display::InlineBlock => "inline-block",
        Display::Flex => "flex",
        Display::InlineFlex => "inline-flex",
        Display::Grid if style.grid_lanes.enabled => "grid-lanes",
        Display::InlineGrid if style.grid_lanes.enabled => "inline-grid-lanes",
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
    // CSS Viewport L1 §5 `#zoom-om` — properties `cascade.rs::apply_zoom_to_lengths`
    // scaled by `effective_zoom` are un-zoomed again below before serialization.
    // `width`/`height`/inset/min-*/max-* are deliberately excluded: those report
    // the *used* value (already zoom-scaled on the real UA too), not the computed
    // one.
    let z = style.effective_zoom;
    // BUG-1438: `letter_spacing` already holds `abs + pct * font_size`; the computed value
    // keeps the `%` share symbolic, so split it off before the zoom is divided out.
    let letter_spacing_abs = (style.letter_spacing - style.letter_spacing_pct * style.font_size)
        / if z > 0.0 { z } else { 1.0 };
    // BUG-1592: radii, shadows, outline, flex-basis, transform offsets etc. are scaled
    // by `scale_visual_lengths`; report them as if `zoom` were not applied.
    let unzoomed;
    let style = if z > 0.0 && (z - 1.0).abs() > f32::EPSILON {
        let mut u = style.clone();
        crate::style::scale_visual_lengths(&mut u, 1.0 / z, 1.0, 1.0);
        crate::style::scale_inherited_lengths(&mut u, 1.0 / z, 1.0, 1.0, crate::style::ZoomDeclared::ALL);
        unzoomed = u;
        &unzoomed
    } else {
        style
    };

    // ── Display / layout mode ─────────────────────────────────────
    m.insert("display".into(), webkit_box_computed_display(style).into());

    // ── CSS Anchor Positioning L1 (BUG-1478) ──────────────────────
    m.insert("anchor-name".into(), style.anchor_name.as_deref().unwrap_or("none").into());
    m.insert("position-anchor".into(), style.position_anchor.as_deref().unwrap_or("auto").into());
    m.insert("anchor-scope".into(), match &style.anchor_scope {
        crate::anchor::AnchorScope::None => "none".into(),
        crate::anchor::AnchorScope::All => "all".into(),
        crate::anchor::AnchorScope::Named(list) => list.to_string(),
    });
    m.insert("position-area".into(), style.position_area_text.as_deref().unwrap_or("none").into());
    m.insert("position-visibility".into(), style.position_visibility.unwrap_or("anchors-visible").into());

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

    // CSS Rhythmic Sizing L1 §3 (BUG-517) — block-step-* longhands plus the
    // `block-step` shorthand, resolved to the same canonical order/`none`
    // collapse `block_step_shorthand_computed` documents.
    m.insert("block-step-size".into(), style.block_step_size.map_or("none".into(), px_str));
    m.insert("block-step-insert".into(), style.block_step_insert.to_css().into());
    m.insert("block-step-align".into(), style.block_step_align.to_css().into());
    m.insert("block-step-round".into(), style.block_step_round.to_css().into());
    m.insert("block-step".into(), block_step_shorthand_computed(style));
    // CSS Gap Decorations L1 §3–§4: `column-rule*` / `row-rule*` / `rule*`.
    crate::style::insert_gap_rule_computed(style, &mut m);
    // CSS Multi-column L1/L2: `column-count/width/height/fill/span/wrap` and `columns`.
    crate::style::insert_multicol_computed(style, &mut m);

    m.insert("width".into(), style.width.as_ref().map_or("auto".into(), length_to_css));
    m.insert("height".into(), style.height.as_ref().map_or("auto".into(), length_to_css));
    let fs = style.font_size;
    m.insert("min-width".into(), style.min_width.as_ref().map_or("auto".into(), |l| sizing_computed_css(l, fs)));
    m.insert("max-width".into(), style.max_width.as_ref().map_or("none".into(), |l| sizing_computed_css(l, fs)));
    m.insert("min-height".into(), style.min_height.as_ref().map_or("auto".into(), |l| sizing_computed_css(l, fs)));
    m.insert("max-height".into(), style.max_height.as_ref().map_or("none".into(), |l| sizing_computed_css(l, fs)));

    m.insert("margin-top".into(), unzoom_length_or_auto_to_css(&style.margin_top, z));
    m.insert("margin-right".into(), unzoom_length_or_auto_to_css(&style.margin_right, z));
    m.insert("margin-bottom".into(), unzoom_length_or_auto_to_css(&style.margin_bottom, z));
    m.insert("margin-left".into(), unzoom_length_or_auto_to_css(&style.margin_left, z));

    m.insert("padding-top".into(), unzoom_length_to_css(&style.padding_top, z));
    m.insert("padding-right".into(), unzoom_length_to_css(&style.padding_right, z));
    m.insert("padding-bottom".into(), unzoom_length_to_css(&style.padding_bottom, z));
    m.insert("padding-left".into(), unzoom_length_to_css(&style.padding_left, z));

    m.insert("border-top-width".into(), px_str(style.border_top_width / z));
    m.insert("border-right-width".into(), px_str(style.border_right_width / z));
    m.insert("border-bottom-width".into(), px_str(style.border_bottom_width / z));
    m.insert("border-left-width".into(), px_str(style.border_left_width / z));

    m.insert("border-top-style".into(), border_style_to_css(style.border_top_style).into());
    m.insert("border-right-style".into(), border_style_to_css(style.border_right_style).into());
    m.insert("border-bottom-style".into(), border_style_to_css(style.border_bottom_style).into());
    m.insert("border-left-style".into(), border_style_to_css(style.border_left_style).into());

    m.insert("border-top-color".into(), used_css_color(&style.border_top_color, style.color));
    m.insert("border-right-color".into(), used_css_color(&style.border_right_color, style.color));
    m.insert("border-bottom-color".into(), used_css_color(&style.border_bottom_color, style.color));
    m.insert("border-left-color".into(), used_css_color(&style.border_left_color, style.color));

    // Each corner is `rx` or `rx ry` (the latter only when the radii differ, CSS Backgrounds L3 §5.5);
    // the shorthand lists the four horizontal radii, then the four vertical ones after `/`.
    let hr = [
        length_to_css(&style.border_top_left_radius),
        length_to_css(&style.border_top_right_radius),
        length_to_css(&style.border_bottom_right_radius),
        length_to_css(&style.border_bottom_left_radius),
    ];
    let vr = [
        length_to_css(&style.border_top_left_radius_y),
        length_to_css(&style.border_top_right_radius_y),
        length_to_css(&style.border_bottom_right_radius_y),
        length_to_css(&style.border_bottom_left_radius_y),
    ];
    let corner = |i: usize| if hr[i] == vr[i] { hr[i].clone() } else { format!("{} {}", hr[i], vr[i]) };
    m.insert("border-top-left-radius".into(), corner(0));
    m.insert("border-top-right-radius".into(), corner(1));
    m.insert("border-bottom-right-radius".into(), corner(2));
    m.insert("border-bottom-left-radius".into(), corner(3));
    m.insert("border-radius".into(), crate::style::serialize_border_radius(
        [hr[0].as_str(), hr[1].as_str(), hr[2].as_str(), hr[3].as_str()],
        [vr[0].as_str(), vr[1].as_str(), vr[2].as_str(), vr[3].as_str()],
    ));

    // ── Inset (positioned elements) ───────────────────────────────
    m.insert("top".into(), length_or_auto_to_css(&style.top));
    m.insert("right".into(), length_or_auto_to_css(&style.right));
    m.insert("bottom".into(), length_or_auto_to_css(&style.bottom));
    m.insert("left".into(), length_or_auto_to_css(&style.left));

    // ── Colors ────────────────────────────────────────────────────
    // BUG-1425: lab()/oklch()/color()/color-mix() сохраняют своё пространство в computed-значении.
    m.insert(
        "color".into(),
        style.color_css.as_deref().map_or_else(|| color_to_css(style.color), str::to_owned),
    );
    m.insert("background-color".into(), style.background_color.as_ref()
        .map_or_else(|| "rgba(0, 0, 0, 0)".into(), css_color_to_css));
    m.insert("background-image".into(), background_layers_to_css(&style.background_layers));
    let pos_spec = style.background_specified.as_deref();
    let pos_x = pos_spec.map_or(&[][..], |s| s.position_x.as_slice());
    let pos_y = pos_spec.map_or(&[][..], |s| s.position_y.as_slice());
    m.insert("background-position-x".into(),
        background_position_axis_to_css(&style.background_layers, pos_x, |l| l.position.x));
    m.insert("background-position-y".into(),
        background_position_axis_to_css(&style.background_layers, pos_y, |l| l.position.y));
    // CSS Backgrounds L3 §3.6: `<x> <y>` per layer, the longer of the two lists sets the count.
    m.insert("background-position".into(), {
        let xs = m["background-position-x"].split(", ").map(String::from).collect::<Vec<_>>();
        let ys = m["background-position-y"].split(", ").map(String::from).collect::<Vec<_>>();
        (0..xs.len().max(ys.len()))
            .map(|i| format!("{} {}", xs[i % xs.len()], ys[i % ys.len()]))
            .collect::<Vec<_>>()
            .join(", ")
    });

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
    m.insert("aspect-ratio".into(), match style.aspect_ratio {
        Some((w, h)) => format!("auto {} / {}", number_str(w), number_str(h)),
        None => "auto".into(),
    });

    // `border-color` shorthand — the four sides serialise in the shortest margin-style form
    // (CSSOM §6.7.2), like `border-width`/`border-style` below.
    m.insert("border-color".into(), box_sides_shorthand(
        &used_css_color(&style.border_top_color, style.color),
        &used_css_color(&style.border_right_color, style.color),
        &used_css_color(&style.border_bottom_color, style.color),
        &used_css_color(&style.border_left_color, style.color),
    ));

    // CSS 2.1 §17.6 `border-spacing` — one value when horizontal/vertical
    // components are equal (the common case, including every legacy
    // `cellspacing` presentational hint), two otherwise.
    m.insert("border-spacing".into(), {
        let (h, v) = (style.border_spacing_h, style.border_spacing_v);
        if h == v { px_str(h) } else { format!("{} {}", px_str(h), px_str(v)) }
    });
    m.insert("opacity".into(), opacity_to_css(style.opacity));

    // ── Typography ────────────────────────────────────────────────
    m.insert("font-size".into(), px_str(style.font_size / z));
    m.insert("font-weight".into(), style.font_weight.0.to_string());
    m.insert("font-style".into(), match style.font_style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
        FontStyle::Oblique => "oblique",
    }.into());
    // CSS Fonts L4 §6.10: shorthand сериализуется значениями реализованных
    // компонент (ligatures, caps, numeric, position, emoji) — `-east-asian`/
    // `-alternates` всегда в initial. Все в initial → `normal`; иначе — только
    // не-initial части в порядке longhand-ов. `none` (ligatures) в списке
    // невыразим → пустая строка (§6.10 serialization).
    m.insert("font-variant".into(), {
        let lig = style.font_variant_ligatures;
        let num = style.font_variant_numeric;
        let mut parts: Vec<String> = Vec::with_capacity(5);
        if lig != FontVariantLigatures::default() {
            parts.push(lig.serialize());
        }
        if style.font_variant_caps != FontVariantCaps::Normal {
            parts.push(style.font_variant_caps.as_str().into());
        }
        if num != FontVariantNumeric::default() {
            parts.push(num.serialize());
        }
        if style.font_variant_position != FontVariantPosition::Normal {
            parts.push(style.font_variant_position.as_str().into());
        }
        if style.font_variant_emoji != FontVariantEmoji::Normal {
            parts.push(style.font_variant_emoji.as_str().into());
        }
        if parts.is_empty() {
            "normal".to_string()
        } else if lig == FontVariantLigatures::NONE && parts.len() > 1 {
            String::new()
        } else {
            parts.join(" ")
        }
    });
    m.insert("font-variant-ligatures".into(), style.font_variant_ligatures.serialize());
    m.insert("font-variant-numeric".into(), style.font_variant_numeric.serialize());
    m.insert("font-variant-position".into(), style.font_variant_position.as_str().into());
    m.insert("font-variant-caps".into(), style.font_variant_caps.as_str().into());
    m.insert("font-variant-emoji".into(), style.font_variant_emoji.as_str().into());
    // BUG-1407: `font`, `font-feature/variation-settings`, `font-size-adjust`, `font-optical-sizing`,
    // `font-palette`, `clip-path`, `clip-rule`, `mask*`.
    crate::computed_font_mask::insert_font_mask_computed(style, style.font_size / z, &mut m);
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
        if !style.line_height_is_relative {
            // BUG-1050: an absolute `<length>` is stored as a ratio of the
            // (zoomed) font-size; the computed value is that length in px.
            px_str(v * style.font_size / z)
        } else if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{}", v) }
    });
    // CSS Text L4 §11.2: `letter-spacing: 0` вычисляется в `normal`; `word-spacing: normal` — в `0px`.
    // CSS Text L4: a `<percentage>` stays a percentage in the computed value (`110%`,
    // `calc(-15% + 10px)`), it is only resolved against the font in layout.
    m.insert("letter-spacing".into(), match length_percentage_css(letter_spacing_abs, style.letter_spacing_pct) {
        Some(css) => css,
        None if style.letter_spacing == 0.0 => "normal".into(),
        None => px_str(style.letter_spacing),
    });
    m.insert("word-spacing".into(), length_percentage_css(style.word_spacing, style.word_spacing_pct)
        .unwrap_or_else(|| px_str(style.word_spacing)));
    m.insert("text-align".into(), match style.text_align {
        TextAlign::Start => "start",
        TextAlign::End => "end",
        TextAlign::Left => "left",
        TextAlign::Right => "right",
        TextAlign::Center => "center",
        TextAlign::Justify => "justify",
        // Не резолвлен каскадом (стиль собран в обход `compute_style`): ведёт себя как `start`.
        TextAlign::MatchParent => "start",
    }.into());
    // BUG-1325: остальные свойства CSS Text, которых раньше не было в карте.
    m.insert("text-align-last".into(), if style.text_extra.align_last_match_parent { "match-parent" } else { match style.text_align_last {
        TextAlignLast::Auto => "auto",
        TextAlignLast::Start => "start",
        TextAlignLast::End => "end",
        TextAlignLast::Left => "left",
        TextAlignLast::Right => "right",
        TextAlignLast::Center => "center",
        TextAlignLast::Justify => "justify",
        // Не резолвлен каскадом (стиль собран в обход `compute_style`): ведёт себя как `start`.
        TextAlignLast::MatchParent => "start",
    } }.into());
    m.insert("text-justify".into(), style.text_extra.justify.serialize().into());
    m.insert("tab-size".into(), match style.text_extra.tab_size_number {
        Some(n) => number_str(n),
        None => px_str(style.tab_size),
    });
    m.insert("text-wrap-mode".into(), text_wrap_mode_css(style.text_wrap_mode).into());
    m.insert("text-wrap-style".into(), text_wrap_style_css(style.text_wrap_style).into());
    m.insert("text-wrap".into(), match (style.text_wrap_mode, style.text_wrap_style) {
        (TextWrapMode::Wrap, TextWrapStyle::Auto) => "wrap".into(),
        (TextWrapMode::Wrap, st) => text_wrap_style_css(st).into(),
        (TextWrapMode::Nowrap, TextWrapStyle::Auto) => "nowrap".into(),
        (TextWrapMode::Nowrap, st) => format!("nowrap {}", text_wrap_style_css(st)),
    });
    m.insert("line-break".into(), match style.line_break {
        LineBreak::Auto => "auto",
        LineBreak::Loose => "loose",
        LineBreak::Normal => "normal",
        LineBreak::Strict => "strict",
        LineBreak::Anywhere => "anywhere",
    }.into());
    m.insert("word-break".into(), match style.word_break {
        WordBreak::Normal => "normal",
        WordBreak::KeepAll => "keep-all",
        WordBreak::BreakAll => "break-all",
        WordBreak::BreakWord => "break-word",
        WordBreak::AutoPhrase => "auto-phrase",
    }.into());
    let overflow_wrap = match style.overflow_wrap {
        OverflowWrap::Normal => "normal",
        OverflowWrap::BreakWord => "break-word",
        OverflowWrap::Anywhere => "anywhere",
    };
    m.insert("overflow-wrap".into(), overflow_wrap.into());
    // `word-wrap` — унаследованный псевдоним `overflow-wrap` (CSS Text L3 §5.5).
    m.insert("word-wrap".into(), overflow_wrap.into());
    m.insert("hyphens".into(), match style.hyphens {
        Hyphens::None => "none",
        Hyphens::Manual => "manual",
        Hyphens::Auto => "auto",
    }.into());
    // CSS Writing Modes L4 §2.1/§2.2/§3.1/§5.1 — computed value "as
    // specified" for all four. The fields were cascaded and consumed by
    // layout/bidi all along, but never serialised here, so
    // `getComputedStyle(el).writingMode`/`.direction` read back `""`
    // (BUG-506 residual: `css/css-logical/animation-001.html`'s
    // "…is not animatable" subtests, and `css-writing-modes/parsing/*-computed`).
    m.insert("writing-mode".into(), match style.writing_mode {
        WritingMode::HorizontalTb => "horizontal-tb",
        WritingMode::VerticalRl => "vertical-rl",
        WritingMode::VerticalLr => "vertical-lr",
        WritingMode::SidewaysRl => "sideways-rl",
        WritingMode::SidewaysLr => "sideways-lr",
    }.into());
    m.insert("direction".into(), match style.direction {
        Direction::Ltr => "ltr",
        Direction::Rtl => "rtl",
    }.into());
    m.insert("unicode-bidi".into(), match style.unicode_bidi {
        UnicodeBidi::Normal => "normal",
        UnicodeBidi::Embed => "embed",
        UnicodeBidi::Isolate => "isolate",
        UnicodeBidi::BidiOverride => "bidi-override",
        UnicodeBidi::IsolateOverride => "isolate-override",
        UnicodeBidi::Plaintext => "plaintext",
    }.into());
    m.insert("text-orientation".into(), match style.text_orientation {
        TextOrientation::Mixed => "mixed",
        TextOrientation::Upright => "upright",
        TextOrientation::Sideways => "sideways",
    }.into());
    m.insert("text-transform".into(), style.text_extra.transform.serialize(style.text_transform));
    // CSS Text L4 §2.1: shorthand над `white-space-collapse` + `text-wrap-mode`. Пары без
    // legacy-ключевого слова (`preserve-breaks nowrap`, `break-spaces nowrap`) пишутся длинно.
    m.insert("white-space".into(), match (style.white_space_collapse, style.text_wrap_mode) {
        (WhiteSpaceCollapse::Collapse, TextWrapMode::Wrap) => "normal".into(),
        (WhiteSpaceCollapse::Collapse, TextWrapMode::Nowrap) => "nowrap".into(),
        (WhiteSpaceCollapse::Preserve | WhiteSpaceCollapse::PreserveSpaces, TextWrapMode::Nowrap) => "pre".into(),
        (WhiteSpaceCollapse::Preserve | WhiteSpaceCollapse::PreserveSpaces, TextWrapMode::Wrap) => "pre-wrap".into(),
        (WhiteSpaceCollapse::PreserveBreaks, TextWrapMode::Wrap) => "pre-line".into(),
        (WhiteSpaceCollapse::PreserveBreaks, TextWrapMode::Nowrap) => "preserve-breaks nowrap".into(),
        (WhiteSpaceCollapse::BreakSpaces, TextWrapMode::Wrap) => "break-spaces".into(),
        (WhiteSpaceCollapse::BreakSpaces, TextWrapMode::Nowrap) => "break-spaces nowrap".into(),
    });
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
    // `currentcolor` сериализуется в used-значение, как в Chrome.
    m.insert("text-decoration-color".into(),
        color_to_css(style.text_decoration_color.resolve(style.color)));
    m.insert("-webkit-text-fill-color".into(), color_to_css(style.used_text_fill_color()));
    // WHATWG Compat §3.2: `currentcolor` сериализуется в used-значение; шортхенд —
    // `<width> <color>` (WPT `webkit-text-stroke-computed`).
    let stroke_width = px_str(style.text_stroke_width);
    let stroke_color = color_to_css(style.used_text_stroke_color());
    m.insert("-webkit-text-stroke".into(), format!("{stroke_width} {stroke_color}"));
    m.insert("-webkit-text-stroke-width".into(), stroke_width);
    m.insert("-webkit-text-stroke-color".into(), stroke_color);
    m.insert("text-overflow".into(), if style.text_overflow == style.text_overflow_right {
        style.text_overflow.to_css()
    } else {
        format!("{} {}", style.text_overflow.to_css(), style.text_overflow_right.to_css())
    });
    // CSS Text Size Adjustment L1 §2 (BUG-513): both spellings read the same
    // underlying field, same convention as `-webkit-line-clamp`/`line-clamp`
    // above. `none` never round-trips — its computed value is `100%`.
    m.insert("text-size-adjust".into(), style.text_size_adjust.to_css());
    m.insert("-webkit-text-size-adjust".into(), style.text_size_adjust.to_css());
    // CSS Overflow L4 §13.4 / compat `-webkit-line-clamp` (BUG-505): both
    // names read the same underlying field — the engine implements only the
    // reduced `none | <integer>` grammar, not the full `line-clamp`
    // shorthand (`max-lines`/`block-ellipsis`/`continue`/`-webkit-legacy`
    // longhands are unimplemented, see BUG-505).
    let line_clamp = match style.line_clamp {
        None if style.line_clamp_auto => "auto".to_string(),
        None => "none".to_string(),
        Some(n) if style.line_clamp_auto => format!("{n} auto"),
        Some(n) => n.to_string(),
    };
    m.insert("-webkit-line-clamp".into(), line_clamp.clone());
    m.insert("line-clamp".into(), line_clamp);
    // WHATWG Compat §2.1 / CSS Overflow L4 §continue (BUG-505 срез 5) — feed
    // `webkit_box_computed_display`'s condition, plus their own round-trip.
    m.insert("-webkit-box-orient".into(), match style.box_orient {
        WebkitBoxOrient::Horizontal => "horizontal",
        WebkitBoxOrient::Vertical => "vertical",
    }.into());
    m.insert("-webkit-box-pack".into(), match style.box_pack {
        WebkitBoxPack::Start => "start",
        WebkitBoxPack::Center => "center",
        WebkitBoxPack::End => "end",
        WebkitBoxPack::Justify => "justify",
    }.into());
    m.insert("-webkit-box-align".into(), match style.box_align {
        WebkitBoxAlign::Start => "start",
        WebkitBoxAlign::Center => "center",
        WebkitBoxAlign::End => "end",
        WebkitBoxAlign::Baseline => "baseline",
        WebkitBoxAlign::Stretch => "stretch",
    }.into());
    m.insert("-webkit-box-direction".into(), match style.box_direction {
        WebkitBoxDirection::Normal => "normal",
        WebkitBoxDirection::Reverse => "reverse",
    }.into());
    m.insert("-webkit-box-flex".into(), style.box_flex.to_string());
    m.insert("-webkit-box-ordinal-group".into(), style.box_ordinal_group.to_string());
    m.insert("continue".into(), match style.continue_value {
        CssContinue::Normal => "normal",
        CssContinue::Discard => "discard",
        CssContinue::Collapse => "collapse",
        CssContinue::WebkitLegacy => "-webkit-legacy",
    }.into());
    {
        // CSS Text L3 §7.1: `<length-percentage> hanging? each-line?`; `em` и `calc()` без `%`
        // сворачиваются в px (computed value — абсолютная длина).
        let mut indent = absolute_length_css(&style.text_indent, style.font_size);
        if style.text_extra.indent_hanging {
            indent.push_str(" hanging");
        }
        if style.text_extra.indent_each_line {
            indent.push_str(" each-line");
        }
        m.insert("text-indent".into(), indent);
    }
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
    // CSS Overflow L5 §scroll-marker-group-property (BUG-505 срез 6): `None`
    // = initial `none`; otherwise the placement(+mode) round-trips through
    // `ScrollMarkerGroup::to_css`.
    m.insert("scroll-marker-group".into(), match style.scroll_marker_group {
        None => "none".to_string(),
        Some(v) => v.to_css(),
    });
    m.insert("scroll-target-group".into(), style.scroll_target_group.to_css().into());
    // CSS Scroll Snap L1 §4 `scroll-margin-*` (BUG-962): already resolved to
    // px at cascade time (`style/apply/motion.rs`) and, unlike `margin-*`,
    // never touched by `apply_zoom_to_lengths` — reported as-is, no unzoom.
    m.insert("scroll-margin-top".into(), px_str(style.scroll_margin_top));
    m.insert("scroll-margin-right".into(), px_str(style.scroll_margin_right));
    m.insert("scroll-margin-bottom".into(), px_str(style.scroll_margin_bottom));
    m.insert("scroll-margin-left".into(), px_str(style.scroll_margin_left));
    m.insert("scroll-padding-top".into(), px_str(style.scroll_padding_top));
    m.insert("scroll-padding-right".into(), px_str(style.scroll_padding_right));
    m.insert("scroll-padding-bottom".into(), px_str(style.scroll_padding_bottom));
    m.insert("scroll-padding-left".into(), px_str(style.scroll_padding_left));
    // CSS Scroll Anchoring 1 — `overflow-anchor` (BUG-524 срез 1, parsing/CSSOM only).
    m.insert("overflow-anchor".into(), match style.overflow_anchor {
        OverflowAnchor::Auto => "auto",
        OverflowAnchor::None => "none",
    }.into());
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
    m.insert("transform".into(), transform_list_to_css(&style.transform));
    m.insert("translate".into(), translate_to_css(style.translate.as_ref()));
    m.insert("rotate".into(), rotate_to_css(style.rotate));
    m.insert("scale".into(), scale_to_css(style.scale));

    m.insert("filter".into(), if style.filter.is_empty() {
        "none".into()
    } else {
        style.filter.iter().map(filter_fn_to_css).collect::<Vec<_>>().join(" ")
    });

    // ── Compositing ───────────────────────────────────────────────
    m.insert("mix-blend-mode".into(), mix_blend_mode_to_css(style.mix_blend_mode).into());
    // VT-API: `getComputedStyle(el).viewTransitionName` and the JS side's
    // duplicate-name check (`_lumen_vt_named_elements`) read it from here.
    m.insert(
        "view-transition-name".into(),
        style.view_transition_name.as_deref().unwrap_or("none").into(),
    );
    m.insert(
        "view-transition-class".into(),
        style.view_transition_class.as_deref().unwrap_or("none").into(),
    );
    m.insert(
        "view-transition-group".into(),
        style.view_transition_group.as_deref().unwrap_or("normal").into(),
    );
    m.insert("interpolate-size".into(), match style.interpolate_size {
        InterpolateSizeMode::NumericOnly => "numeric-only",
        InterpolateSizeMode::AllowKeywords => "allow-keywords",
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

    // ── Flexbox (CSS Flexbox L1 §4–§7) ────────────────────────────
    let fd = match style.flex_direction {
        FlexDirection::Row => "row",
        FlexDirection::RowReverse => "row-reverse",
        FlexDirection::Column => "column",
        FlexDirection::ColumnReverse => "column-reverse",
    };
    let fw = match (style.flex_wrap, style.flex_wrap_balance) {
        (FlexWrap::Nowrap, _) => "nowrap",
        (FlexWrap::Wrap, false) => "wrap",
        (FlexWrap::Wrap, true) => "balance",
        (FlexWrap::WrapReverse, false) => "wrap-reverse",
        (FlexWrap::WrapReverse, true) => "wrap-reverse balance",
    };
    let fb = match &style.flex_basis {
        FlexBasis::Auto => "auto".to_string(),
        FlexBasis::Content => "content".to_string(),
        FlexBasis::Length(l) => length_to_css(l),
    };
    m.insert("flex-direction".into(), fd.into());
    m.insert("flex-wrap".into(), fw.into());
    m.insert("flex-line-count".into(), style.flex_line_count.to_string());
    m.insert("flex-flow".into(), format!("{fd} {fw}"));
    m.insert("flex-grow".into(), style.flex_grow.to_string());
    m.insert("flex-shrink".into(), style.flex_shrink.to_string());
    m.insert("flex".into(), format!("{} {} {fb}", style.flex_grow, style.flex_shrink));
    m.insert("flex-basis".into(), fb);
    m.insert("order".into(), style.order.to_string());

    // ── Grid (CSS Grid L1/L2) and gaps (Box Alignment L3 §8) ─────────
    crate::grid_computed::insert_grid_computed(style, &mut m);

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
    // CSS Conditional 5 §3.1: `normal | [ [ size | inline-size ] || scroll-state ]`, the size
    // axis first.
    m.insert("container-type".into(), {
        let axis = match style.container_type {
            ContainerType::Normal => "",
            ContainerType::Size => "size",
            ContainerType::InlineSize => "inline-size",
        };
        match (axis, style.container_scroll_state) {
            ("", false) => "normal".to_owned(),
            ("", true) => "scroll-state".to_owned(),
            (axis, false) => axis.to_owned(),
            (axis, true) => format!("{axis} scroll-state"),
        }
    });
    m.insert(
        "container-name".into(),
        if style.container_name.is_empty() { "none".to_owned() } else { style.container_name.join(" ") },
    );
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
    // `border-width`/`border-style` mirror `border-color` above.
    m.insert("border-width".into(), box_sides_shorthand(
        &px_str(style.border_top_width / z), &px_str(style.border_right_width / z),
        &px_str(style.border_bottom_width / z), &px_str(style.border_left_width / z),
    ));
    m.insert("border-style".into(), box_sides_shorthand(
        border_style_to_css(style.border_top_style), border_style_to_css(style.border_right_style),
        border_style_to_css(style.border_bottom_style), border_style_to_css(style.border_left_style),
    ));

    // ── Background per-layer longhands (CSS Backgrounds L3 §3.5-3.8) ────
    let spec = style.background_specified.as_deref().cloned().unwrap_or_default();
    m.insert("background-attachment".into(), background_layer_field_to_css(
        &style.background_layers, spec.attachment.as_slice(), BackgroundAttachment::default(), |l| l.attachment, background_attachment_to_css,
    ));
    m.insert("background-origin".into(), background_layer_field_to_css(
        &style.background_layers, spec.origin.as_slice(), BackgroundOrigin::default(), |l| l.origin, background_origin_to_css,
    ));
    m.insert("background-clip".into(), background_layer_field_to_css(
        &style.background_layers, spec.clip.as_slice(), BackgroundClip::default(), |l| l.clip, background_clip_to_css,
    ));
    m.insert("background-repeat".into(), background_layer_field_to_css(
        &style.background_layers, spec.repeat.as_slice(), BackgroundRepeat::default(), |l| l.repeat, background_repeat_to_css,
    ));
    m.insert("background-size".into(), if !spec.size.is_empty() {
        spec.size.iter().map(|s| background_size_to_css(*s)).collect::<Vec<_>>().join(", ")
    } else if style.background_layers.is_empty() {
        background_size_to_css(BackgroundSize::default())
    } else {
        style.background_layers.iter().map(|l| background_size_to_css(l.size)).collect::<Vec<_>>().join(", ")
    });
    // CSS Compositing L1 §3 — per-layer, unlike top-level `mix-blend-mode`
    // (compositing of the element as a whole against its backdrop).
    m.insert("background-blend-mode".into(), background_layer_field_to_css(
        &style.background_layers, &[], MixBlendMode::Normal, |l| l.blend_mode, mix_blend_mode_to_css,
    ));

    // ── Shadows (CSS Backgrounds L3 §7.1, CSS Text Decoration L3 §2.5) ──
    m.insert("box-shadow".into(), box_shadow_list_to_css(&style.box_shadow, style.color));
    m.insert("text-shadow".into(), text_shadow_list_to_css(&style.text_shadow));

    // CSS Text Decoration L4 §3.3/§3.7 (BUG-1050) — `auto`/`from-font`/`<length>`.
    m.insert("text-decoration-thickness".into(), match style.text_decoration_thickness {
        crate::TextDecorationThickness::Auto => "auto".into(),
        crate::TextDecorationThickness::FromFont => "from-font".into(),
        crate::TextDecorationThickness::Length(v) => px_str(v),
        crate::TextDecorationThickness::Percentage(p) => format!("{p}%"),
    });
    m.insert("text-underline-offset".into(), style.text_underline_offset.map_or("auto".into(), px_str));

    // ── Perspective ──────────────────────────────────────────────────
    m.insert("perspective-origin".into(), format!(
        "{} {}",
        position_component_to_css(style.perspective_origin.0),
        position_component_to_css(style.perspective_origin.1),
    ));

    // ── Transitions / animations (CSS Transitions L1 §3, CSS Animations L1 §4.3) ─
    m.insert("transition-duration".into(), seconds_list_to_css(&style.transition_durations));
    m.insert("transition-delay".into(), seconds_list_to_css(&style.transition_delays));
    // Interest Invokers: `normal` или время в секундах (`123ms` → `0.123s`).
    for (name, v) in [
        ("interest-delay-start", style.interest_delay_start),
        ("interest-delay-end", style.interest_delay_end),
    ] {
        m.insert(name.into(), v.map_or_else(|| "normal".into(), |s| seconds_list_to_css(&[s])));
    }
    m.insert("transition-behavior".into(), {
        if style.transition_behaviors.is_empty() {
            "normal".to_string()
        } else {
            style.transition_behaviors.iter().map(|b| b.to_css()).collect::<Vec<_>>().join(", ")
        }
    });
    m.insert("transition-timing-function".into(), timing_function_list_to_css(&style.transition_timing_functions));
    m.insert("transition-property".into(), if style.transition_properties.is_empty() {
        if style.transition_property_none { "none".into() } else { "all".into() }
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

    // ── Scroll behaviour (CSSOM View §13 / CSS Overflow 3) ──────────
    // SMOOTH-SCROLL: the scroll methods' `behavior: "auto"` reads it from here.
    m.insert("scroll-behavior".into(), match style.scroll_behavior {
        ScrollBehavior::Auto => "auto",
        ScrollBehavior::Smooth => "smooth",
    }.into());

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
    // CSS Overscroll Behavior L1 §2 (BUG-516): flow-relative axes read back
    // the already-resolved physical value on the axis they map to, same
    // swap as `overflow-block`/`-inline` above.
    fn overscroll_behavior_to_css(v: OverscrollBehavior) -> &'static str {
        match v {
            OverscrollBehavior::Auto => "auto",
            OverscrollBehavior::Contain => "contain",
            OverscrollBehavior::None => "none",
        }
    }
    let (overscroll_behavior_block, overscroll_behavior_inline) = if vertical_wm {
        (style.overscroll_behavior_x, style.overscroll_behavior_y)
    } else {
        (style.overscroll_behavior_y, style.overscroll_behavior_x)
    };
    m.insert("overscroll-behavior-block".into(), overscroll_behavior_to_css(overscroll_behavior_block).into());
    m.insert("overscroll-behavior-inline".into(), overscroll_behavior_to_css(overscroll_behavior_inline).into());

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
    // CSS UI L4 §6.4 — interactivity.
    m.insert("interactivity".into(), style.interactivity.to_css().into());
    // CSS UI L4 §6.5 — input-security.
    m.insert("input-security".into(), style.input_security.to_css().into());
    // CSS UI L4 §6.2 — caret-shape / caret-animation.
    m.insert("caret-shape".into(), style.caret_shape.to_css().into());
    m.insert("caret-animation".into(), style.caret_animation.to_css().into());
    m.insert("forced-color-adjust".into(), match style.forced_color_adjust {
        ForcedColorAdjust::Auto => "auto",
        ForcedColorAdjust::None => "none",
        ForcedColorAdjust::PreserveParentColor => "preserve-parent-color",
    }.into());
    // CSS Color HDR L1 §2 — dynamic-range-limit. BUG-508.
    m.insert("dynamic-range-limit".into(), style.dynamic_range_limit.to_css());
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
    // SVG 2 §Geometry / §Painting (BUG-1094).
    m.insert("cx".into(), length_to_css(&style.svg_cx));
    m.insert("cy".into(), length_to_css(&style.svg_cy));
    m.insert("r".into(), length_to_css(&style.svg_r));
    m.insert("x".into(), length_to_css(&style.svg_x));
    m.insert("y".into(), length_to_css(&style.svg_y));
    m.insert("rx".into(), length_or_auto_to_css(&style.svg_rx));
    m.insert("ry".into(), length_or_auto_to_css(&style.svg_ry));
    m.insert("color-interpolation".into(), match style.svg_color_interpolation {
        SvgColorInterpolation::Auto => "auto",
        SvgColorInterpolation::Srgb => "srgb",
        SvgColorInterpolation::LinearRgb => "linearrgb",
    }.into());
    m.insert("path-length".into(), match style.svg_path_length {
        Some(n) => format!("{}", n),
        None => "none".into(),
    });
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
    m.insert("ruby-position".into(), style.ruby_position.as_css().into());
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
    // Sizing longhands depend on `writing-mode` only (not `direction`):
    // vertical modes swap inline/block onto height/width.
    {
        let vertical = matches!(
            style.writing_mode,
            WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl | WritingMode::SidewaysLr
        );
        let pairs: [(&str, &str); 6] = if vertical {
            [("inline-size", "height"), ("min-inline-size", "min-height"), ("max-inline-size", "max-height"),
             ("block-size", "width"), ("min-block-size", "min-width"), ("max-block-size", "max-width")]
        } else {
            [("inline-size", "width"), ("min-inline-size", "min-width"), ("max-inline-size", "max-width"),
             ("block-size", "height"), ("min-block-size", "min-height"), ("max-block-size", "max-height")]
        };
        for (logical, physical) in pairs {
            if let Some(v) = m.get(physical).cloned() {
                m.insert(logical.into(), v);
            }
        }
    }
    if style.writing_mode == WritingMode::HorizontalTb && style.direction == Direction::Ltr {
        const LOGICAL_LONGHANDS: &[(&str, &str)] = &[
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
            ("border-start-start-radius", "border-top-left-radius"), ("border-start-end-radius", "border-top-right-radius"),
            ("border-end-start-radius", "border-bottom-left-radius"), ("border-end-end-radius", "border-bottom-right-radius"),
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

    // Four-sided shorthands (BUG-1050): serialised from the longhands already
    // in the map (so un-zoom is inherited), collapsed to the shortest form.
    for (short, [t, r, b, l]) in [
        ("margin", ["margin-top", "margin-right", "margin-bottom", "margin-left"]),
        ("padding", ["padding-top", "padding-right", "padding-bottom", "padding-left"]),
        ("inset", ["top", "right", "bottom", "left"]),
        ("scroll-margin", ["scroll-margin-top", "scroll-margin-right", "scroll-margin-bottom", "scroll-margin-left"]),
        ("scroll-padding", ["scroll-padding-top", "scroll-padding-right", "scroll-padding-bottom", "scroll-padding-left"]),
    ] {
        let v = box_sides_shorthand(&m[t], &m[r], &m[b], &m[l]);
        m.insert(short.into(), v);
    }

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

mod contain_css;
use contain_css::{contain_intrinsic_to_css, contain_to_css};

#[cfg(test)]
mod tests;
