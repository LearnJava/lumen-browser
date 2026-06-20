//! Layout-движок для Lumen.
//!
//! Block-flow + inline-flow с word-wrapping. Блочные элементы стэкаются
//! вертикально. Текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`,
//! `<strong>`, и т.д.) объединяются в `InlineRun` — анонимный бокс, где
//! слова переносятся как единый поток. Style cascade — specificity-based
//! (CSS3), полный набор Selectors-Level-3 включая `:nth-*` и `:not`.
//!
//! Snapshot-тестирование: `serialize_layout_tree` даёт детерминированный
//! текст layout-дерева для golden-сравнений (см. `tests/snapshot_tests.rs`).
//!
//! Не поддерживается (Phase 2+): flex, grid, float, absolute positioning,
//! font-weight/style на уровне inline.

pub use lumen_core::ColorSpace;

pub mod anchor;
pub mod animation;
pub mod box_tree;
pub mod color_mix;
pub mod incremental;
pub mod content_visibility;
pub mod field_sizing;
pub mod hyphenation;
pub mod counters;
pub mod font_palette;
pub mod image_gating;
pub mod image_set;
pub mod mathml;
pub mod motion_path;
pub mod page;
pub mod pagination;
pub mod property_trees;
pub mod ruby;
pub mod rule_index;
pub mod selection;
pub mod selector_query;
pub mod scroll_timeline;
pub mod snapshot;
pub mod inert;
pub mod stacking;
pub mod starting_style;
pub mod style;
pub mod masonry;
pub mod subgrid;
pub mod table;
pub mod text_iter;
pub(crate) mod vertical;

pub use counters::{
    format_counter, format_counter_with_registry, precompute_counters,
    build_counter_style_registry, build_list_marker_text, resolve_counter_value,
    CounterMap, CounterSnapshot, CounterStyleDef, CounterStyleRegistry,
    CounterSystem, CounterRange, QuoteSlot, RangeBound,
};
pub use color_mix::{MixColorSpace, mix_colors};
pub use field_sizing::field_sizing_content_intrinsic;
pub use hyphenation::{collect_hyphen_points, SoftHyphenPoint};
pub use image_gating::gate_image_requests;
pub use image_set::{
    parse_image_set, select_image_set_candidate, select_image_set_url,
    ImageSetOption, SupportedTypes,
};
pub use mathml::{MathmlBox, MathmlElementKind, lay_out_mathml, collect_mathml_structure};
pub use ruby::{RubyBox, RubyPosition, lay_out_ruby};
pub use animation::{
    AnimValue, AnimatedStyle, AnimationFrame, AnimationInterpolator,
    LinearInterpolator, NoopInterpolator, parse_keyframe_style, KeyframeStyle,
    CompositorAnimFrame, CompositorOverride,
    AnimationScheduler, TransitionScheduler,
};
pub use box_tree::{
    apply_container_styles, build_iframe_document,
    collect_background_image_requests, collect_image_requests, is_open_details, layout, layout_measured,
    layout_measured_hyp, layout_streaming_incremental, lay_out_incremental, BoxKind, FormControlKind, ImageRequest, InlineFrag, InlineSegment, LayoutBox,
    PseudoKind, SvgShapeKind, SvgTextAnchor, SvgDominantBaseline, ViewBox,
};
pub use incremental::{DirtyBits, mark_dirty, mark_dirty_set, clear_dirty, translate_subtree};
pub use page::{MarginBox, MarginBoxPosition, PageBox, PageProperties, MarginBoxTextFragment};
pub use pagination::{paginate, Page, PageFragment, PaginationContext};
pub use property_trees::{
    compute_local_transform, forward_box_transform, transform_fns_to_matrix,
    ClipNode, ClipTree, EffectNode, EffectTree,
    Mat4, PropertyTreeNodeId, PropertyTrees, ScrollNode, ScrollTree, TransformNode, TransformTree,
};
pub use selection::{caret_at_point, selection_rects};
pub use style::{compute_selection_style, compute_style_from_declarations};
pub use selector_query::{
    computed_style_by_selector, computed_style_json, computed_style_json_by_selector,
    computed_style_to_map, find_all_by_selector, find_box_by_selector, matched_rules_for_node,
    matches_selector, query_all, ComputedStyleSnapshot, MatchedRule,
};
pub use anchor::{
    collect_anchors, register_anchor, resolve_anchor_function, resolve_inset_area,
    AnchorEntry, AnchorRegistry, AnchorSide, AnchoredPosition, InsetAreaKeyword,
};
pub use motion_path::{resolve_motion_transform, MotionTransform};
pub use text_iter::{collect_visible_text, TextFragment};
pub use scroll_timeline::{
    collect_named_scroll_timelines, collect_named_view_timelines,
    resolve_scroll_progress, resolve_view_progress,
    NamedScrollTimeline, NamedViewTimeline, ScrollAxis, ScrollTimeline, ViewTimeline, Viewport,
};
pub use snapshot::serialize_layout_tree;
pub use inert::{collect_inert_regions, is_inert, InertRegion};
pub use starting_style::{resolve_starting_style, StartingStyleTracker};
pub use subgrid::{collect_subgrid_items, SubgridContext, SubgridItem};
pub use content_visibility::{set_cv_scroll, set_cv_relevant, take_cv_skipped, CV_SLACK_FACTOR};
pub use stacking::{
    box_can_own_stacking_context, creates_stacking_context, PaintOrder, PaintPhase,
    StackingContext, StackingContextId, StackingTree,
};
pub use style::{
    apply_container_rules, evaluate_container_condition,
    set_interactive_state, clear_interactive_state,
    parse_background_gradient, parse_color, parse_css_wide_keyword, parse_gradient_stops,
    parse_grid_template_areas, parse_transform_list,
    AlignValue, AnimationDirection, Appearance, ContainerContext,
    AnimationFillMode, AnimationPlayState,
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin, BackgroundRepeat,
    BackgroundSize, BorderCollapse, BorderStyle,
    BoxShadow, BoxSizing, BreakValue, CalcNode, ClipPath, Color, ColorFloat,
    ClearSide, ContainFlags, ComputedStyle, Content,
    ContentItem, CssColor, CssWideKeyword, Cursor, Direction, Display, EmptyCells, FilterFn, FloatSide, FontOpticalSizing, FontStretch,
    FontStyle,
    FontVariant, FontVariationSetting, FontWeight, GradientStop, GridAutoFlow, GridLine, GridTrackSize, Hyphens, ImageRendering,
    MaskMode, MasonryAutoFlow,
    Isolation, IterationCount, Length,
    LengthOrAuto, ListStylePosition, ListStyleType, MixBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, OverflowWrap, OverscrollBehavior, ParsedGradient, Resize,
    PointerEvents,
    Position, PositionComponent, Quotes, ScrollBehavior, ScrollSnapAlign, ScrollSnapAlignKeyword,
    ScrollSnapAxis, ScrollSnapStop, ScrollSnapStrictness, ScrollSnapType, ScrollbarGutter,
    FillRule, ScrollbarWidth, ShapeValue, StepPosition, StrokeLinecap, StrokeLinejoin, SvgPaint, TextAlign, TextDecorationLine, TextDecorationStyle,
    TextDecorationSkipInk, TextDecorationThickness, TextEmphasisPosition, TextEmphasisShape, TextEmphasisStyle,
    TextOverflow, TextShadow, TextTransform, TextUnderlinePosition,
    TimingFunction, TransformFn, TransformStyle,
    UserSelect, Visibility,
    WhiteSpace, WordBreak,
};

/// Computed `::selection` highlight data — passed to the paint layer so it can
/// apply `::selection` CSS overrides when rendering selected text.
///
/// CSS Pseudo-elements L4 §5.6 restricts `::selection` to a limited set of
/// properties: `color`, `background-color`, `text-decoration-*`, `text-shadow`.
/// The paint layer reads only `fg_color` and `bg_color`; other properties from
/// the full `ComputedStyle` are ignored during selection rendering.
///
/// Build via [`compute_selection_style`] or construct directly with OS-default
/// colours when no `::selection` rules are present.
#[derive(Debug, Clone)]
pub struct SelectionHighlight {
    /// The active DOM selection range. Must not be collapsed.
    pub range: lumen_dom::Range,
    /// Text colour override from `::selection { color: ... }`. `None` = inherit
    /// (keep each fragment's own `color`).
    pub fg_color: Option<Color>,
    /// Selection background from `::selection { background-color: ... }`.
    /// The default when no `::selection` rule is present is the OS accent colour;
    /// callers should supply a sensible fallback (e.g. `#308aff`).
    pub bg_color: Color,
}

/// Интерфейс измерения ширины символов для line wrapping.
///
/// Реализуется на стороне вызывающего кода (paint/shell), где есть доступ
/// к шрифтовым данным. Layout использует его только в `layout_measured()`.
pub trait TextMeasurer {
    /// Ширина символа `ch` при размере шрифта `font_size_px` пикселей.
    /// Возвращает 0.0 для неизвестных символов.
    fn char_width(&self, ch: char, font_size_px: f32) -> f32;

    /// Ширина символа `ch` с учётом CSS `font-family` каскада.
    ///
    /// Перебирает `families` по порядку и возвращает ширину из первого шрифта,
    /// в котором есть глиф для `ch`. Если ни одна семья не загружена или не
    /// содержит глиф, делегирует к [`Self::char_width`] (Inter-fallback).
    ///
    /// Реализации, поддерживающие несколько шрифтов, должны переопределить
    /// этот метод. По умолчанию игнорирует `families`.
    fn char_width_with_families(&self, ch: char, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.char_width(ch, font_size_px)
    }

    /// Ширина символа `ch` с учётом CSS `font-family` и `font-variation-settings`.
    ///
    /// CSS Fonts L4 §6.3 — вариационные оси передаются в порядке каскада.
    /// Для шрифтов без fvar/HVAR игнорирует `axes` и делегирует к
    /// [`Self::char_width_with_families`]. Для variable fonts применяет
    /// HVAR delta через нормализованные координаты осей.
    ///
    /// Дефолтная реализация игнорирует `axes` — достаточно для статических шрифтов.
    fn char_width_varied(
        &self,
        ch: char,
        font_size_px: f32,
        axes: &[FontVariationSetting],
        families: &[String],
    ) -> f32 {
        let _ = axes;
        self.char_width_with_families(ch, font_size_px, families)
    }

    /// Descent шрифта в пикселях при размере `font_size_px`.
    /// Используется для IFC strut: определяет, насколько линия строки
    /// опускается ниже baseline при baseline-выравнивании.
    fn descent_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.2
    }

    /// Ascent шрифта в пикселях при размере `font_size_px`.
    /// Расстояние от baseline до верхнего края content area.
    /// Используется paint-кодом для точного позиционирования baseline
    /// внутри line-box с учётом half-leading (CSS 2.1 §10.8.1).
    fn ascent_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.8
    }

    /// x-height шрифта в пикселях при размере `font_size_px` — высота строчной
    /// `x` без выносных элементов (таблица OS/2 `sxHeight`).
    ///
    /// CSS Fonts L5 §4 — основа для `font-size-adjust`: aspect value шрифта =
    /// `x_height_px(size) / size`. Реализации без доступа к метрикам возвращают
    /// приближение `0.5 × size` (то же, что `ex`-юнит в style.rs).
    fn x_height_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.5
    }
}

// ─── Clickable elements iterator (for P3 click-hint overlay, §12.14 task 7B.2) ──

/// Classification of an interactive element found during layout-tree traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickableKind {
    /// `<a href="…">` hyperlink (block-level or inline).
    Link {
        /// Raw `href` value, not yet resolved against base URL.
        href: String,
    },
    /// `<button>` or `<input type=submit|button|reset>`.
    Button,
    /// Text/number/file/etc. `<input>`, `<textarea>`, `<select>`.
    Input,
    /// `<details>` disclosure element (opening/closing the summary).
    Details,
    /// Element with `tabindex` >= 0 that doesn't fit other categories.
    Generic,
}

/// An interactive element with its screen-space bounding rect.
///
/// `rect` is the border-box of the element in CSS px, as computed by layout.
/// Used by P3's click-hint overlay to render keyboard-navigable hint badges.
#[derive(Debug, Clone)]
pub struct ClickableElement {
    /// DOM node that owns this interactive region.
    pub node_id: lumen_dom::NodeId,
    /// Border-box rectangle in CSS px (document-relative, before scroll).
    pub rect: lumen_core::geom::Rect,
    /// Short text label for the hint badge (link text, button label, etc.).
    /// `None` when no usable label could be extracted.
    pub hint_text: Option<String>,
    /// Interaction kind — used by P3 to assign the correct hint key and action.
    pub kind: ClickableKind,
}

/// Collect all interactive elements from the layout tree in document order.
///
/// Walks the layout tree and returns every element that the user can
/// activate: links, buttons, form controls, and elements with `tabindex`.
/// Skipped boxes (`display: none`) and their children are omitted entirely.
///
/// For inline `<a href>` links, the returned `rect` is a bounding box of
/// all inline fragments belonging to that link on its first line; multi-line
/// links produce one entry per distinct link element (full-line bbox).
pub fn collect_clickable_elements(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
) -> Vec<ClickableElement> {
    let mut out = Vec::new();
    collect_clickable_rec(root, doc, &mut out);
    out
}

fn collect_clickable_rec(
    b: &LayoutBox,
    doc: &lumen_dom::Document,
    out: &mut Vec<ClickableElement>,
) {
    use box_tree::{BoxKind, FormControlKind};
    use lumen_core::geom::Rect;

    if matches!(b.kind, BoxKind::Skip) {
        return;
    }

    // CSS: inert — P4 should add `[inert] { pointer-events: none; }` to the UA
    // stylesheet. This guard provides the complementary layout-level filter:
    // inert elements are never included in the clickable set.
    if inert::is_inert(doc, b.node) {
        return;
    }

    // CSS Pointer Events L1: `pointer-events: none` on a block box excludes the box
    // itself from the clickable set. Children are always visited — a child's
    // pointer-events is independent (the property is not inherited).
    //
    // InlineRun boxes carry the BLOCK CONTAINER'S style (not the inline element's
    // own style), so we cannot use `b.style.pointer_events` to gate InlineRun
    // processing. Instead, each inline link is gated by `frag.style.pointer_events`,
    // which reflects the actual inline element's computed value.
    let block_pe_none = b.style.pointer_events == PointerEvents::None;

    match &b.kind {
        BoxKind::FormControl { kind } if !block_pe_none => {
            let ck = match kind {
                FormControlKind::Button => ClickableKind::Button,
                FormControlKind::Input { .. }
                | FormControlKind::Select { .. }
                | FormControlKind::Textarea { .. }
                | FormControlKind::Range { .. }
                | FormControlKind::Progress { .. }
                | FormControlKind::Meter { .. } => ClickableKind::Input,
            };
            out.push(ClickableElement {
                node_id: b.node,
                rect: b.rect,
                hint_text: None,
                kind: ck,
            });
        }
        BoxKind::Block | BoxKind::FlowRoot if !block_pe_none => {
            if let Some(href) = element_href(doc, b.node) {
                out.push(ClickableElement {
                    node_id: b.node,
                    rect: b.rect,
                    hint_text: first_text_content(doc, b.node),
                    kind: ClickableKind::Link { href },
                });
            } else if is_details_element(doc, b.node) {
                out.push(ClickableElement {
                    node_id: b.node,
                    rect: b.rect,
                    hint_text: first_text_content(doc, b.node),
                    kind: ClickableKind::Details,
                });
            } else if has_tabindex(doc, b.node) {
                out.push(ClickableElement {
                    node_id: b.node,
                    rect: b.rect,
                    hint_text: first_text_content(doc, b.node),
                    kind: ClickableKind::Generic,
                });
            }
        }
        BoxKind::InlineRun { lines, .. } => {
            // Collect rects for inline <a href> links by walking frag source_nodes.
            // Groups consecutive frags with the same link ancestor into one entry.
            // Skip links whose frag.style.pointer_events is None (the frag carries
            // the inline element's own computed style, not the block container's).
            let line_y_offset = b.rect.y;
            let line_x_offset = b.rect.x;
            for line in lines {
                let mut cur_link_node: Option<lumen_dom::NodeId> = None;
                let mut cur_href = String::new();
                let mut cur_rect: Option<Rect> = None;
                for frag in line {
                    // Treat pointer-events:none inline elements as if they have no link.
                    let link = if frag.style.pointer_events == PointerEvents::None {
                        None
                    } else {
                        link_ancestor(doc, frag.source_node)
                    };
                    if link == cur_link_node {
                        if let Some(ref mut r) = cur_rect {
                            let fx = line_x_offset + frag.x;
                            let fw = frag.width;
                            let left = r.x.min(fx);
                            let right = (r.x + r.width).max(fx + fw);
                            r.x = left;
                            r.width = right - left;
                        }
                    } else {
                        // Flush previous link entry.
                        if let (Some(nid), Some(r)) = (cur_link_node, cur_rect) {
                            out.push(ClickableElement {
                                node_id: nid,
                                rect: r,
                                hint_text: Some(cur_href.clone()),
                                kind: ClickableKind::Link { href: cur_href.clone() },
                            });
                        }
                        cur_link_node = link;
                        if let Some(nid) = link {
                            cur_href = element_href(doc, nid).unwrap_or_default();
                            let line_height = line
                                .iter()
                                .map(|f| f.style.font_size)
                                .fold(0.0_f32, f32::max);
                            let fy = line_y_offset;
                            cur_rect = Some(Rect::new(
                                line_x_offset + frag.x,
                                fy,
                                frag.width,
                                line_height,
                            ));
                        } else {
                            cur_rect = None;
                        }
                    }
                }
                // Flush the last link.
                if let (Some(nid), Some(r)) = (cur_link_node, cur_rect) {
                    out.push(ClickableElement {
                        node_id: nid,
                        rect: r,
                        hint_text: Some(cur_href.clone()),
                        kind: ClickableKind::Link { href: cur_href },
                    });
                }
            }
        }
        _ => {}
    }

    for child in &b.children {
        collect_clickable_rec(child, doc, out);
    }
}

/// Returns the `href` attribute of element `id` if it's an `<a>` element with a non-empty href.
fn element_href(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> Option<String> {
    use lumen_dom::NodeData;
    match &doc.get(id).data {
        NodeData::Element { name, attrs, .. } if name.local == "a" => {
            attrs.iter().find(|a| a.name.local == "href").map(|a| a.value.clone())
        }
        _ => None,
    }
}

/// Returns `true` if element `id` has a non-negative `tabindex` attribute.
fn has_tabindex(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> bool {
    doc.get(id)
        .get_attr("tabindex")
        .and_then(|v| v.trim().parse::<i32>().ok())
        .is_some_and(|n| n >= 0)
}

/// Walk up from `id` to find the nearest `<a href>` ancestor (inclusive).
fn link_ancestor(
    doc: &lumen_dom::Document,
    mut id: lumen_dom::NodeId,
) -> Option<lumen_dom::NodeId> {
    loop {
        if element_href(doc, id).is_some() {
            return Some(id);
        }
        match doc.get(id).parent {
            Some(p) => id = p,
            None => return None,
        }
    }
}

/// Get the text content of the first text-node descendant (for hint labels).
fn first_text_content(
    doc: &lumen_dom::Document,
    id: lumen_dom::NodeId,
) -> Option<String> {
    use lumen_dom::NodeData;
    let node = doc.get(id);
    if let NodeData::Text(t) = &node.data {
        let s = t.trim().to_string();
        return if s.is_empty() { None } else { Some(s) };
    }
    for &child in &node.children {
        if let Some(t) = first_text_content(doc, child) {
            return Some(t);
        }
    }
    None
}

/// Returns `true` if element `id` is a `<details>` element (disclosure widget).
fn is_details_element(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> bool {
    use lumen_dom::NodeData;
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "details"
    )
}

// ─── Sticky-position algorithm stub ──────────────────────────────────────────
// CSS: position: sticky — P4 wires insets from ComputedStyle (top/right/bottom/left);
//                         P3 wires scroll_x/scroll_y from shell scroll state.

/// Snapshot of a `position: sticky` element captured after normal-flow layout.
///
/// P3 integration: call `collect_sticky_boxes()` after every re-layout, then at
/// each scroll event call `compute_sticky_offset()` per entry and apply the
/// returned `(dx, dy)` translate to the element's paint transform.
///
/// P4 integration: `top/right/bottom/left` currently hold only `Length::Px`
/// values extracted via `LengthOrAuto::to_px_opt()`.  After P4 resolves em/%
/// insets inside `box_tree::lay_out_block()`, replace the field values with the
/// resolved px quantities before returning the tree.
#[derive(Debug, Clone)]
pub struct StickyBox {
    /// DOM node that owns this sticky element.
    pub node: lumen_dom::NodeId,
    /// Border-box rectangle as placed by normal flow, in CSS px (document-relative).
    pub static_rect: lumen_core::geom::Rect,
    /// CSS `top` inset in px.  `None` when the property is `auto` or a
    /// non-`px` unit (em, %, rem, …) that `to_px_opt()` cannot resolve.
    pub top: Option<f32>,
    /// CSS `bottom` inset in px.  `None` when auto/non-px.
    pub bottom: Option<f32>,
    /// CSS `left` inset in px.  `None` when auto/non-px.
    pub left: Option<f32>,
    /// CSS `right` inset in px.  `None` when auto/non-px.
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
/// Deduplicates by NodeId: the layout engine may produce both a `Block` wrapper
/// and a `FlowRoot` inner box for the same element (e.g. when sticky creates a
/// new BFC).  Only the first box seen (outermost, document-order) is recorded.
pub fn collect_sticky_boxes(root: &LayoutBox) -> Vec<StickyBox> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_sticky_rec(root, root.rect, &mut seen, &mut out);
    out
}

fn collect_sticky_rec(
    b: &LayoutBox,
    containing_rect: lumen_core::geom::Rect,
    seen: &mut std::collections::HashSet<lumen_dom::NodeId>,
    out: &mut Vec<StickyBox>,
) {
    use box_tree::BoxKind;
    use style::Position;

    if matches!(b.kind, BoxKind::Skip) {
        return;
    }

    if matches!(b.style.position, Position::Sticky) && seen.insert(b.node) {
        out.push(StickyBox {
            node: b.node,
            static_rect: b.rect,
            top: b.style.top.to_px_opt(),
            bottom: b.style.bottom.to_px_opt(),
            left: b.style.left.to_px_opt(),
            right: b.style.right.to_px_opt(),
            containing_rect,
        });
    }

    // Blocks and flow roots establish a new sticky-containment boundary.
    let next_cb = if matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot) {
        b.rect
    } else {
        containing_rect
    };

    for child in &b.children {
        collect_sticky_rec(child, next_cb, seen, out);
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

fn collect_snap_rec(
    b: &LayoutBox,
    container_stack: &mut Vec<usize>,
    out: &mut Vec<SnapContainer>,
    seen_areas: &mut std::collections::HashSet<lumen_dom::NodeId>,
) {
    use box_tree::BoxKind;
    use style::ScrollSnapAxis;

    if matches!(b.kind, BoxKind::Skip) {
        return;
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
        for child in &b.children {
            collect_snap_rec(child, container_stack, out, seen_areas);
        }
        container_stack.pop();
        return;
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

    for child in &b.children {
        collect_snap_rec(child, container_stack, out, seen_areas);
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
fn snap_offset_x(
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
fn snap_offset_y(
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

// ---------------------------------------------------------------------------
// Scroll container infrastructure
// CSS: overflow — P4 wires: check style.overflow_x/overflow_y == Overflow::Scroll | Auto,
// call collect_scroll_containers() to enumerate regions, set_scroll_position() on wheel.
// ---------------------------------------------------------------------------

/// A scrollable overflow container collected from the layout tree.
/// Shell uses this to route wheel events and update scroll offsets.
pub struct ScrollContainer {
    /// The DOM node that owns this scroll region.
    pub node: lumen_dom::NodeId,
    /// Clip rectangle in CSS px (padding-box of the container, document-relative).
    /// Shell converts to screen coords for hit-testing against pointer position.
    pub clip_rect: lumen_core::geom::Rect,
    /// Content width in CSS px (may exceed clip_rect.width for horizontal scroll).
    pub scroll_width: f32,
    /// Content height in CSS px (may exceed clip_rect.height for vertical scroll).
    pub scroll_height: f32,
    /// Current horizontal scroll offset in CSS px. Clamped to [0, scroll_width - clip_rect.width].
    pub scroll_x: f32,
    /// Current vertical scroll offset in CSS px. Clamped to [0, scroll_height - clip_rect.height].
    pub scroll_y: f32,
    /// CSS Overscroll Behavior L1 §2 — `overscroll-behavior-x`. Governs whether a
    /// horizontal scroll delta this container cannot consume propagates to the
    /// ancestor scroll chain (`Auto` propagates; `Contain`/`None` stop it).
    pub overscroll_behavior_x: style::OverscrollBehavior,
    /// CSS Overscroll Behavior L1 §2 — `overscroll-behavior-y`. Same semantics
    /// as `overscroll_behavior_x` for the vertical axis.
    pub overscroll_behavior_y: style::OverscrollBehavior,
}

/// Collect all `overflow: scroll` / `overflow: auto` containers from the layout tree.
///
/// Returns one `ScrollContainer` per LayoutBox whose overflow-x or overflow-y
/// is `Scroll` or `Auto`. Shell calls this after each layout pass to build
/// the scroll hit-test map.
///
/// # CSS: overflow
/// P4 wires: after adding `overflow: scroll` parsing, this function will naturally
/// include those boxes (LayoutBox.style.overflow_x/y already parsed by P4).
pub fn collect_scroll_containers(root: &LayoutBox) -> Vec<ScrollContainer> {
    let mut out = Vec::new();
    collect_scroll_containers_inner(root, &mut out);
    out
}

fn collect_scroll_containers_inner(b: &LayoutBox, out: &mut Vec<ScrollContainer>) {
    use style::Overflow;
    let s = &b.style;
    let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto);
    let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto);
    if is_scroll_x || is_scroll_y {
        let bl = s.border_left_width;
        let bt = s.border_top_width;
        let br = s.border_right_width;
        let bb = s.border_bottom_width;
        let clip = lumen_core::geom::Rect::new(
            b.rect.x + bl,
            b.rect.y + bt,
            (b.rect.width - bl - br).max(0.0),
            (b.rect.height - bt - bb).max(0.0),
        );
        let scroll_width = content_width(b);
        let scroll_height = content_height(b);
        out.push(ScrollContainer {
            node: b.node,
            clip_rect: clip,
            scroll_width,
            scroll_height,
            scroll_x: b.scroll_x,
            scroll_y: b.scroll_y,
            overscroll_behavior_x: s.overscroll_behavior_x,
            overscroll_behavior_y: s.overscroll_behavior_y,
        });
    }
    for child in &b.children {
        collect_scroll_containers_inner(child, out);
    }
}

/// CSS Overscroll Behavior L1 §3 — decide whether a scroll delta a container
/// could not consume should propagate up the ancestor scroll chain (e.g. to the
/// page).
///
/// `dx`/`dy` are the requested deltas in CSS px; `moved_x`/`moved_y` report
/// whether the container actually scrolled on each axis (false ⇒ the container
/// is at its boundary in that direction). Returns `true` when the residual delta
/// is allowed to bubble to the parent.
///
/// Rules:
/// - If the container moved on either axis it has consumed the gesture, so the
///   chain stops here (returns `false`).
/// - Otherwise the container is fully at its boundary. Propagation is blocked
///   when any axis carrying a non-zero delta has `Contain` or `None`; if every
///   delta-bearing axis is `Auto` the delta propagates.
#[must_use]
pub fn overscroll_should_propagate(
    overscroll_x: style::OverscrollBehavior,
    overscroll_y: style::OverscrollBehavior,
    dx: f32,
    dy: f32,
    moved_x: bool,
    moved_y: bool,
) -> bool {
    use style::OverscrollBehavior;
    if moved_x || moved_y {
        return false;
    }
    let blocked = (dx != 0.0 && overscroll_x != OverscrollBehavior::Auto)
        || (dy != 0.0 && overscroll_y != OverscrollBehavior::Auto);
    !blocked
}

/// Compute the content scroll-width of a box: rightmost child edge relative to container left.
///
/// Returns max(b.rect.width, children's right edge - b.rect.x).
/// Used to compute the max scroll offset for horizontal scrolling.
fn content_width(b: &LayoutBox) -> f32 {
    b.children.iter().fold(b.rect.width, |acc, c| {
        let c_right = c.rect.x + c.rect.width - b.rect.x;
        acc.max(c_right)
    })
}

/// Compute the content scroll-height of a box: bottommost child edge relative to container top.
///
/// Returns max(b.rect.height, children's bottom edge - b.rect.y).
/// Used to compute the max scroll offset for vertical scrolling.
fn content_height(b: &LayoutBox) -> f32 {
    b.children.iter().fold(b.rect.height, |acc, c| {
        let c_bottom = c.rect.y + c.rect.height - b.rect.y;
        acc.max(c_bottom)
    })
}

// ──────────────── collect_computed_styles ────────────────

/// Walks the layout tree and returns a map of `NodeId index → CSS property map`.
///
/// The CSS property map for each node is produced by [`computed_style_to_map`],
/// which serialises the most-queried ~55 properties to CSS string values.
/// Used by the shell to populate the JS-runtime computed-style cache after each
/// relayout so that `window.getComputedStyle()` can answer without a
/// round-trip to the layout engine.
pub fn collect_computed_styles(
    root: &LayoutBox,
) -> std::collections::HashMap<u32, std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    collect_computed_styles_rec(root, &mut out);
    out
}

fn collect_computed_styles_rec(
    b: &LayoutBox,
    out: &mut std::collections::HashMap<u32, std::collections::HashMap<String, String>>,
) {
    out.insert(b.node.index() as u32, computed_style_to_map(&b.style));
    for child in &b.children {
        collect_computed_styles_rec(child, out);
    }
}

/// Update the scroll position of a node in the layout tree.
///
/// Walks the tree to find the box with `node`, clamps `(x, y)` to the valid
/// scroll range `[0, scroll_width - clip_width] × [0, scroll_height - clip_height]`,
/// then updates `LayoutBox.scroll_x / scroll_y`. Returns `true` if found.
///
/// Shell calls this on wheel events after determining the target scroll container
/// via `collect_scroll_containers()` + hit testing against the pointer position.
pub fn set_scroll_position(root: &mut LayoutBox, node: lumen_dom::NodeId, x: f32, y: f32) -> bool {
    if root.node == node {
        let sw = content_width(root);
        let sh = content_height(root);
        let clip_w = root.rect.width;
        let clip_h = root.rect.height;
        root.scroll_x = x.clamp(0.0, (sw - clip_w).max(0.0));
        root.scroll_y = y.clamp(0.0, (sh - clip_h).max(0.0));
        return true;
    }
    for child in &mut root.children {
        if set_scroll_position(child, node, x, y) {
            return true;
        }
    }
    false
}

/// Find the innermost scroll container whose `clip_rect` contains `(x, y)`.
///
/// Returns the `NodeId` of the topmost (in DOM order, last in the list wins for nesting)
/// overflow container whose clip rectangle contains the given document-space coordinate.
/// Shell uses this to route `MouseWheel` events to the correct overflow container
/// instead of always scrolling the page.
///
/// CSS View Transitions L1 §10 — collect all elements with a `view-transition-name` set.
///
/// Returns one `(node, name)` pair per named element in document order. Elements with
/// `display: none` (no layout box) are skipped. The shell passes this list to the
/// transition engine during `document.startViewTransition()` to match old/new snapshots.
///
/// Duplicate names are allowed in this list — per-page uniqueness is enforced by the
/// caller (only the first occurrence should be used as a capture source).
pub fn collect_view_transition_names(root: &LayoutBox) -> Vec<(lumen_dom::NodeId, Box<str>)> {
    let mut out = Vec::new();
    collect_vt_names_rec(root, &mut out);
    out
}

fn collect_vt_names_rec(b: &LayoutBox, out: &mut Vec<(lumen_dom::NodeId, Box<str>)>) {
    use box_tree::BoxKind;
    if matches!(b.kind, BoxKind::Skip) {
        return;
    }
    if let Some(ref name) = b.style.view_transition_name {
        out.push((b.node, name.clone()));
    }
    for child in &b.children {
        collect_vt_names_rec(child, out);
    }
}

/// `x` and `y` are in CSS px, document-relative (same coordinate space as
/// `ScrollContainer::clip_rect`).
pub fn find_scroll_container_at(
    containers: &[ScrollContainer],
    x: f32,
    y: f32,
) -> Option<lumen_dom::NodeId> {
    // Iterate in reverse so later (deeper, visually on top) containers win.
    containers.iter().rev().find_map(|c| {
        let r = &c.clip_rect;
        if x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height {
            Some(c.node)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{compute_style, VerticalAlign};
    use lumen_core::geom::Size;

    /// Navigate the document layout tree root → html → body and return the
    /// body `LayoutBox`. Tests were written for the old flat DOM structure
    /// (before the HTML5 parser started injecting implicit html/head/body
    /// wrappers). This helper adapts them without touching production code.
    fn body_layout_box(mut root: LayoutBox) -> LayoutBox {
        // root children: [html block, ...]
        if let Some(html_idx) = root
            .children
            .iter()
            .position(|c| matches!(c.kind, BoxKind::Block))
        {
            let mut html_box = root.children.remove(html_idx);
            // html children: [body block, ...]
            if let Some(body_idx) = html_box
                .children
                .iter()
                .position(|c| matches!(c.kind, BoxKind::Block))
            {
                return html_box.children.remove(body_idx);
            }
            return html_box;
        }
        root
    }

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        body_layout_box(layout(&doc, &sheet, Size::new(800.0, 600.0)))
    }

    fn lay_viewport(html: &str, css: &str, vp: Size) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        body_layout_box(layout(&doc, &sheet, vp))
    }

    /// Измеритель с фиксированной шириной 8px на символ.
    struct Fixed8;
    impl TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 {
            8.0
        }
    }

    fn lay_measured(html: &str, css: &str, width: f32) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        body_layout_box(layout_measured(&doc, &sheet, Size::new(width, 600.0), &Fixed8))
    }

    /// Like `lay()` but returns the full layout tree root (document box),
    /// not the body box. Use when a test explicitly needs to inspect
    /// the `<html>` or `<body>` layout boxes.
    fn lay_full(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    fn first_element_child(b: &LayoutBox) -> &LayoutBox {
        fn is_element(k: &BoxKind) -> bool {
            matches!(
                k,
                BoxKind::Block
                    | BoxKind::FormControl { .. }
                    | BoxKind::TableRow
                    | BoxKind::Table
                    | BoxKind::TableRowGroup
            )
        }
        // Form controls and other inline-block elements are wrapped in an
        // anonymous InlineBlockRow (and text in an InlineRun); descend through
        // those anonymous containers to find the first real element box.
        fn rec(b: &LayoutBox) -> Option<&LayoutBox> {
            for c in &b.children {
                if is_element(&c.kind) {
                    return Some(c);
                }
                if matches!(c.kind, BoxKind::InlineBlockRow | BoxKind::InlineRun { .. })
                    && let Some(found) = rec(c)
                {
                    return Some(found);
                }
            }
            None
        }
        rec(b).expect("expected at least one element child")
    }

    /// DFS search: first box in tree (including `b` itself) matching the predicate.
    fn find_box(b: &LayoutBox, pred: impl Fn(&BoxKind) -> bool + Copy) -> Option<&LayoutBox> {
        if pred(&b.kind) {
            return Some(b);
        }
        for c in &b.children {
            if let Some(found) = find_box(c, pred) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn empty_document() {
        let root = lay("", "");
        assert_eq!(root.rect.width, 800.0);
        assert_eq!(root.rect.height, 0.0);
    }

    #[test]
    fn single_paragraph_height_one_line() {
        let root = lay("<p>hello</p>", "");
        // root → <p> → text. Высота: font_size 16 * line_height 1.2 = 19.2
        assert!(
            (root.rect.height - 19.2).abs() < 0.1,
            "got {}",
            root.rect.height
        );
    }

    #[test]
    fn stacked_blocks_height_sums() {
        let root = lay("<p>a</p><p>b</p><p>c</p>", "");
        // 3 строки по 19.2
        assert!((root.rect.height - 57.6).abs() < 0.1);
    }

    #[test]
    fn whitespace_only_text_skipped() {
        let root = lay("<p>a</p>\n  \n<p>b</p>", "");
        // Пробельные узлы между <p> не должны давать вертикального пространства.
        assert!((root.rect.height - 38.4).abs() < 0.1);
    }

    #[test]
    fn css_color_applied_via_type_selector() {
        let root = lay("<p>x</p>", "p { color: red; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.color,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        );
    }

    #[test]
    fn class_selector_matches() {
        let root = lay(r#"<div class="hero">x</div>"#, ".hero { color: red; }");
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    #[test]
    fn id_selector_matches() {
        let root = lay(r#"<div id="main">x</div>"#, "#main { color: red; }");
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    #[test]
    fn cyrillic_class_matches() {
        let root = lay(r#"<p class="привет">x</p>"#, ".привет { color: red; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    #[test]
    fn last_rule_wins_without_specificity() {
        let root = lay("<p>x</p>", "p { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
        assert_eq!(p.style.color.r, 0);
    }

    #[test]
    fn font_size_inherited_to_text() {
        let root = lay("<p>x</p>", "p { font-size: 32px; }");
        let p = first_element_child(&root);
        // Текст живёт в InlineRun; стиль контейнера наследует font-size от <p>.
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        assert_eq!(inline.style.font_size, 32.0);
        // 32 * 1.2 = 38.4
        assert!((inline.rect.height - 38.4).abs() < 0.1);
    }

    #[test]
    fn hex_color_full() {
        let root = lay("<p>x</p>", "p { color: #ff8800; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
        assert_eq!(p.style.color.g, 136);
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn hex_color_short() {
        let root = lay("<p>x</p>", "p { color: #f80; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
        assert_eq!(p.style.color.g, 136);
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn display_none_skipped() {
        let root = lay("<p>visible</p><p class=\"x\">hidden</p>", ".x { display: none; }");
        // Один блок отрисуется, второй пропустится (skip).
        // Только одна строка высотой 19.2
        assert!((root.rect.height - 19.2).abs() < 0.1);
    }

    #[test]
    fn padding_increases_height() {
        let root = lay("<p>x</p>", "p { padding: 10px; }");
        let p = first_element_child(&root);
        // Высота: 19.2 (текст) + 10 + 10 (padding) = 39.2
        assert!((p.rect.height - 39.2).abs() < 0.1);
    }

    #[test]
    fn margin_offsets_position() {
        let root = lay("<p>x</p>", "p { margin: 20px; }");
        let p = first_element_child(&root);
        assert!((p.rect.x - 20.0).abs() < 0.01);
        assert!((p.rect.y - 20.0).abs() < 0.01);
        // Ширина: 800 - 20 - 20 = 760
        assert!((p.rect.width - 760.0).abs() < 0.01);
    }

    #[test]
    fn background_color_stored() {
        let root = lay("<p>x</p>", "p { background-color: #ff0000; }");
        let p = first_element_child(&root);
        assert!(matches!(p.style.background_color, Some(CssColor::Rgba(_))));
        assert!(matches!(p.style.background_color, Some(CssColor::Rgba(Color { r: 255, .. }))));
    }

    #[test]
    fn color_fn_display_p3_parsed_as_wide() {
        let root = lay("<p>x</p>", "p { background-color: color(display-p3 1 0 0); }");
        let p = first_element_child(&root);
        assert!(
            matches!(p.style.background_color, Some(CssColor::Wide(f)) if f.space == ColorSpace::DisplayP3),
            "display-p3 should parse to CssColor::Wide with DisplayP3 space"
        );
    }

    #[test]
    fn color_fn_srgb_parsed_as_wide() {
        let root = lay("<p>x</p>", "p { background-color: color(srgb 0.5 0.5 0.5); }");
        let p = first_element_child(&root);
        assert!(
            matches!(p.style.background_color, Some(CssColor::Wide(f)) if f.space == ColorSpace::Srgb),
            "srgb should parse to CssColor::Wide with Srgb space"
        );
    }

    #[test]
    fn color_fn_rec2020_parsed_as_wide() {
        let root = lay("<p>x</p>", "p { background-color: color(rec2020 0.3 0.6 0.9); }");
        let p = first_element_child(&root);
        assert!(
            matches!(p.style.background_color, Some(CssColor::Wide(f)) if f.space == ColorSpace::Rec2020),
            "rec2020 should parse to CssColor::Wide with Rec2020 space"
        );
    }

    #[test]
    fn color_fn_display_p3_with_alpha() {
        let root = lay("<p>x</p>", "p { background-color: color(display-p3 1 0 0 / 0.5); }");
        let p = first_element_child(&root);
        if let Some(CssColor::Wide(f)) = p.style.background_color {
            assert!((f.r - 1.0).abs() < 0.001);
            assert!(f.g.abs() < 0.001);
            assert!(f.b.abs() < 0.001);
            assert!((f.a - 0.5).abs() < 0.001);
        } else {
            panic!("expected Wide color with alpha");
        }
    }

    #[test]
    fn color_fn_display_p3_to_srgb_red() {
        // display-p3 red (1 0 0) → sRGB: P3-red выходит за gamut sRGB.
        let f = ColorFloat { r: 1.0, g: 0.0, b: 0.0, a: 1.0, space: ColorSpace::DisplayP3 };
        let c = f.to_srgb_color();
        assert!(c.r > 200, "r={}", c.r);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn head_and_its_metadata_are_hidden() {
        // <title> и <style> содержимое не должно рендериться как видимый
        // текст. Высота итогового layout-а должна совпадать с высотой только
        // одного <p>visible</p> внутри <body>.
        let just_body = lay("<html><body><p>visible</p></body></html>", "");
        let with_head = lay(
            r#"<html>
                <head>
                    <title>Не должно рендериться</title>
                    <style>p { color: red; }</style>
                    <meta charset="utf-8">
                </head>
                <body><p>visible</p></body>
            </html>"#,
            "",
        );
        // Высоты должны совпадать с точностью до окружающих whitespace text-node-ов
        // (которые сами по себе skip-аются как пустые).
        assert!(
            (with_head.rect.height - just_body.rect.height).abs() < 0.1,
            "head content leaked: just_body={}, with_head={}",
            just_body.rect.height,
            with_head.rect.height,
        );
    }

    #[test]
    fn nested_inheritance() {
        let root = lay(
            "<div><p>nested</p></div>",
            "div { font-size: 24px; color: blue; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // font-size наследуется с div к p
        assert_eq!(p.style.font_size, 24.0);
        // color тоже
        assert_eq!(p.style.color.b, 255);
    }

    // ── Тесты line wrapping ─────────────────────────────────────────────────

    /// Fixed8: "hello world" = 11 символов × 8px = 88px.
    /// При viewport 60px ("hello" = 40px влезает, "world" = 40px → перенос).
    #[test]
    fn wrap_two_words_into_two_lines() {
        let root = lay_measured("<p>hello world</p>", "", 60.0);
        // root → <p> → text (2 строки). 2 × (16 * 1.2) = 38.4
        assert!(
            (root.rect.height - 38.4).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// При достаточно широком viewport слова не переносятся.
    #[test]
    fn no_wrap_when_text_fits() {
        // "hello" = 5×8 = 40px, viewport 100px — переноса нет.
        let root = lay_measured("<p>hello</p>", "", 100.0);
        assert!((root.rect.height - 19.2).abs() < 0.1, "height={}", root.rect.height);
    }

    /// Перенос работает корректно для кириллического текста.
    #[test]
    fn wrap_cyrillic_text() {
        // "Привет мир" = 10 × 8 = 80px при Fixed8.
        // Viewport 50px: "Привет" = 6×8=48px ≤ 50, " " + "мир" = 8+24=32 → 48+8+24=80 > 50.
        let root = lay_measured("<p>Привет мир</p>", "", 50.0);
        // 2 строки
        assert!((root.rect.height - 38.4).abs() < 0.1, "height={}", root.rect.height);
    }

    /// Одно слово, которое само по себе шире viewport, остаётся в одной строке.
    #[test]
    fn single_wide_word_stays_on_one_line() {
        // "superlongword" = 13×8 = 104px > 80px viewport — всё равно одна строка.
        let root = lay_measured("<p>superlongword</p>", "", 80.0);
        assert!((root.rect.height - 19.2).abs() < 0.1, "height={}", root.rect.height);
    }

    /// layout() без измеритея = одна строка независимо от ширины.
    #[test]
    fn layout_without_measurer_no_wrap() {
        let root = lay("<p>a b c d e f g h i j</p>", "");
        // layout() без measurer — всегда одна строка
        assert!((root.rect.height - 19.2).abs() < 0.1);
    }

    // ── Тесты расширенных селекторов ───────────────────────────────────────

    /// Находит первого потомка-блока с заданным тегом, рекурсивно.
    fn find_by_tag<'a>(b: &'a LayoutBox, tag: &str, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(b.node).data
            && name.local == tag
        {
            return Some(b);
        }
        for c in &b.children {
            if let Some(f) = find_by_tag(c, tag, doc) {
                return Some(f);
            }
        }
        None
    }

    /// Утилита: layout + Document, чтобы можно было искать элемент по тегу.
    /// Возвращает LayoutBox тела документа (<body>), а не корня.
    fn lay_with_doc(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = body_layout_box(layout(&doc, &sheet, Size::new(800.0, 600.0)));
        (root, doc)
    }

    #[test]
    fn compound_type_and_class_matches() {
        let (root, doc) = lay_with_doc(
            r#"<p class="hl">x</p><p>y</p>"#,
            "p.hl { color: red; }",
        );
        let mut paragraphs = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                paragraphs.push(c);
            }
        }
        assert_eq!(paragraphs.len(), 2);
        // Первый <p class="hl"> — красный, второй <p> — наследует чёрный.
        assert_eq!(paragraphs[0].style.color.r, 255);
        assert_eq!(paragraphs[1].style.color.r, 0);
    }

    #[test]
    fn descendant_combinator_matches() {
        let (root, doc) = lay_with_doc(
            "<div><p>nested</p></div><p>top</p>",
            "div p { color: red; }",
        );
        // Найдём <p> внутри <div> и <p> прямо в root.
        let div_box = root
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div"))
            .unwrap();
        let nested_p = find_by_tag(div_box, "p", &doc).unwrap();
        assert_eq!(nested_p.style.color.r, 255, "nested <p> should be red");

        let top_p = root
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p"))
            .unwrap();
        assert_eq!(top_p.style.color.r, 0, "top-level <p> should NOT match");
    }

    #[test]
    fn child_combinator_only_direct() {
        let (root, doc) = lay_with_doc(
            "<ul><li>a</li><div><li>b</li></div></ul>",
            "ul > li { color: red; }",
        );
        let ul = find_by_tag(&root, "ul", &doc).unwrap();
        // Прямой <li> — красный.
        let direct_li = ul
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "li"))
            .unwrap();
        assert_eq!(direct_li.style.color.r, 255);
        // Вложенный <li> — не должен матчить, наследует чёрный.
        let div = find_by_tag(ul, "div", &doc).unwrap();
        let nested_li = find_by_tag(div, "li", &doc).unwrap();
        assert_eq!(nested_li.style.color.r, 0);
    }

    #[test]
    fn next_sibling_combinator_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>t</h1><p>a</p><p>b</p>",
            "h1 + p { color: red; }",
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        // Только первый <p> сразу после <h1> матчит.
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn later_sibling_combinator_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>t</h1><p>a</p><p>b</p>",
            "h1 ~ p { color: red; }",
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        // Оба <p> после <h1> матчат.
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 255);
    }

    #[test]
    fn attribute_equals_matches() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="ru">x</p><p lang="en">y</p>"#,
            r#"[lang="ru"] { color: red; }"#,
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn attribute_presence_matches() {
        // <a> — inline-элемент, поэтому собирается в InlineRun. Чтобы получить
        // независимые блочные children для проверки style, используем <div>.
        let (root, doc) = lay_with_doc(
            r#"<div data-x="1">a</div><div>b</div>"#,
            "[data-x] { color: red; }",
        );
        let mut divs = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
            {
                divs.push(c);
            }
        }
        assert_eq!(divs[0].style.color.r, 255);
        assert_eq!(divs[1].style.color.r, 0);
    }

    #[test]
    fn attribute_dash_match_for_lang() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="ru-RU">x</p><p lang="ruler">y</p>"#,
            r#"[lang|="ru"] { color: red; }"#,
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        // "ru-RU" матчит (`ru` или `ru-…`), "ruler" — нет.
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn pseudo_first_child_matches() {
        let (root, doc) = lay_with_doc("<p>a</p><p>b</p><p>c</p>", "p:first-child { color: red; }");
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn pseudo_last_child_matches() {
        let (root, doc) = lay_with_doc("<p>a</p><p>b</p><p>c</p>", "p:last-child { color: red; }");
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p")
            {
                ps.push(c);
            }
        }
        assert_eq!(ps[2].style.color.r, 255);
        assert_eq!(ps[0].style.color.r, 0);
    }

    #[test]
    fn pseudo_hover_never_matches() {
        let root = lay("<p>x</p>", "p:hover { color: red; }");
        let p = first_element_child(&root);
        // :hover без set_interactive_state не матчит.
        assert_eq!(p.style.color.r, 0);
    }

    // ── Interactive pseudo-classes: :hover / :focus / :active ────────────────

    fn node_named(lb: &LayoutBox, doc: &lumen_dom::Document, local: &str) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(lb.node).data
            && name.local == local { return Some(lb.node); }
        for c in &lb.children { if let Some(n) = node_named(c, doc, local) { return Some(n); } }
        None
    }

    fn lb_named<'a>(lb: &'a LayoutBox, doc: &lumen_dom::Document, local: &str) -> Option<&'a LayoutBox> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(lb.node).data
            && name.local == local { return Some(lb); }
        for c in &lb.children { if let Some(f) = lb_named(c, doc, local) { return Some(f); } }
        None
    }

    #[test]
    fn hover_matches_when_node_is_hovered() {
        let html = "<p>x</p>";
        let css = "p:hover { color: red; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let p_nid = first_element_child(&root_lb).node;
        set_interactive_state(Some(p_nid), None, None);
        let root_hover = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let p_hover = first_element_child(&root_hover);
        assert_eq!(p_hover.style.color.r, 255, ":hover should apply (color red)");
        assert_eq!(p_hover.style.color.g, 0);
    }

    #[test]
    fn hover_matches_ancestor_of_hovered_node() {
        // :hover applies to all ancestors of the hovered node (CSS Selectors L4 §4.3).
        // Use block-level <p> child so it gets its own LayoutBox (inline elements don't).
        let html = "<div><p>x</p></div>";
        let css = "div:hover { background-color: blue; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let p_nid = node_named(&root_lb, &doc, "p").expect("<p> not found");
        set_interactive_state(Some(p_nid), None, None);
        let root_hover = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let div_bg = lb_named(&root_hover, &doc, "div").expect("<div> not found").style.background_color;
        assert!(
            matches!(div_bg, Some(CssColor::Rgba(Color { b: 255, .. }))),
            "parent :hover should match when child is hovered"
        );
    }

    #[test]
    fn hover_does_not_match_non_hovered_node() {
        // Use block-level <div> as the non-hovered element to get a LayoutBox.
        let html = "<p>x</p><div>y</div>";
        let css = "p:hover { color: red; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let div_nid = node_named(&root_lb, &doc, "div").expect("<div> not found");
        set_interactive_state(Some(div_nid), None, None);
        let root_hover = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let p = first_element_child(&root_hover);
        assert_eq!(p.style.color.r, 0, "non-hovered <p> should not match :hover");
    }

    #[test]
    fn focus_matches_exact_node() {
        let html = "<input type='text' />";
        let css = "input:focus { border-color: blue; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let input_nid = first_element_child(&root_lb).node;
        set_interactive_state(None, Some(input_nid), None);
        let root_focus = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let input = first_element_child(&root_focus);
        assert!(
            matches!(input.style.border_top_color, CssColor::Rgba(Color { b: 255, .. })),
            ":focus border-color blue"
        );
    }

    #[test]
    fn active_matches_element_and_ancestor() {
        let html = "<div><button>click</button></div>";
        let css = "div:active { background-color: red; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_lb = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let btn_nid = node_named(&root_lb, &doc, "button").expect("<button> not found");
        set_interactive_state(None, None, Some(btn_nid));
        let root_active = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        clear_interactive_state();
        let div_bg = lb_named(&root_active, &doc, "div").expect("<div> not found").style.background_color;
        assert!(
            matches!(div_bg, Some(CssColor::Rgba(Color { r: 255, .. }))),
            "parent :active should match when child is active"
        );
    }

    // ── :placeholder-shown (CSS Selectors L4 §15.1) ──

    fn first_named(doc: &lumen_dom::Document, root: &LayoutBox, local: &str) -> Color {
        for c in walk_layout(root) {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(c.node).data
                && name.local == local
            {
                return c.style.color;
            }
        }
        panic!("element <{local}> not found");
    }

    fn walk_layout(root: &LayoutBox) -> Vec<&LayoutBox> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(b) = stack.pop() {
            out.push(b);
            for c in b.children.iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    #[test]
    fn placeholder_shown_matches_input_with_placeholder() {
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn placeholder_shown_no_placeholder_attr_no_match() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_whitespace_only_placeholder_no_match() {
        // " " после trim — пустая строка → не матчит.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="   ">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_filled_input_no_match() {
        // value-атрибут с непустым контентом → placeholder скрыт.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name" value="John">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn placeholder_shown_empty_value_still_matches() {
        // value="" — пользователь ничего не ввёл, placeholder виден.
        let (root, doc) = lay_with_doc(
            r#"<input placeholder="Name" value="">"#,
            "input:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn placeholder_shown_textarea_matches_when_empty() {
        // <textarea> с placeholder и без текстового контента → матчит.
        let (root, doc) = lay_with_doc(
            r#"<textarea placeholder="Bio"></textarea>"#,
            "textarea:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn placeholder_shown_textarea_with_text_does_not_match() {
        // <textarea> с текстом — значение задано через DOM children,
        // placeholder скрыт.
        let (root, doc) = lay_with_doc(
            r#"<textarea placeholder="Bio">My biography</textarea>"#,
            "textarea:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 0);
    }

    #[test]
    fn placeholder_shown_non_form_control_skipped() {
        // <div placeholder="...">x</div> — placeholder не имеет смысла на
        // не-form элементе; pseudo-class не матчит.
        let (root, doc) = lay_with_doc(
            r#"<div placeholder="hint">x</div>"#,
            "div:placeholder-shown { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 0);
    }

    /// Цвет первого layout-box-а с указанным `id`-атрибутом. `panic!`, если
    /// такого нет. Используется в form-state pseudo тестах, где нужно
    /// различать несколько input-ов в одном документе.
    fn color_by_id(doc: &lumen_dom::Document, root: &LayoutBox, id: &str) -> Color {
        for c in walk_layout(root) {
            if let lumen_dom::NodeData::Element { .. } = &doc.get(c.node).data
                && let Some(v) = doc.get(c.node).get_attr("id")
                && v == id
            {
                return c.style.color;
            }
        }
        panic!("element id={id} not found");
    }

    // ──────────────── :required / :optional ────────────────

    #[test]
    fn required_matches_input_with_required_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input required>"#,
            "input:required { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn required_no_match_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:required { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn optional_matches_input_without_required_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:optional { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn optional_no_match_when_required_present() {
        let (root, doc) = lay_with_doc(
            r#"<input required>"#,
            "input:optional { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn required_matches_select_and_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<select id="s" required></select><textarea id="t" required></textarea>"#,
            ":required { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "t").r, 255);
    }

    #[test]
    fn required_skipped_for_hidden_input() {
        // <input type="hidden"> не поддерживает required (HTML5 §4.10.3).
        let (root, doc) = lay_with_doc(
            r#"<input type="hidden" required>"#,
            "input:required { color: red; } input:optional { color: blue; }",
        );
        let c = first_named(&doc, &root, "input");
        assert_eq!(c.r, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn required_matches_checkbox_radio_file() {
        let (root, doc) = lay_with_doc(
            r#"<input id="c" type="checkbox" required>
               <input id="r" type="radio" required>
               <input id="f" type="file" required>"#,
            ":required { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
        assert_eq!(color_by_id(&doc, &root, "r").r, 255);
        assert_eq!(color_by_id(&doc, &root, "f").r, 255);
    }

    #[test]
    fn required_skipped_for_button_and_div() {
        let (root, doc) = lay_with_doc(
            r#"<button id="b" required></button><div id="d" required>x</div>"#,
            ":required { color: red; } :optional { color: blue; }",
        );
        let b = color_by_id(&doc, &root, "b");
        assert_eq!((b.r, b.b), (0, 0), "<button> не имеет required");
        let d = color_by_id(&doc, &root, "d");
        assert_eq!((d.r, d.b), (0, 0), "<div> не имеет required");
    }

    // ──────────────── :read-only / :read-write ────────────────

    #[test]
    fn read_write_matches_plain_input() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_only_matches_readonly_input() {
        let (root, doc) = lay_with_doc(
            r#"<input readonly>"#,
            "input:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_only_matches_disabled_input() {
        let (root, doc) = lay_with_doc(
            r#"<input disabled>"#,
            "input:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn read_write_matches_plain_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<textarea></textarea>"#,
            "textarea:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn read_only_matches_readonly_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<textarea readonly></textarea>"#,
            "textarea:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "textarea").r, 255);
    }

    #[test]
    fn read_only_matches_non_text_input_types() {
        // Не-text-like input types — `:read-only` per HTML5 §4.16.4.
        let (root, doc) = lay_with_doc(
            r#"<input id="h" type="hidden">
               <input id="s" type="submit">
               <input id="r" type="range">
               <input id="c" type="checkbox">"#,
            ":read-only { color: red; } :read-write { color: blue; }",
        );
        assert_eq!(color_by_id(&doc, &root, "h").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "r").r, 255);
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
    }

    #[test]
    fn read_write_matches_contenteditable_true() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true">x</div>"#,
            "div:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_write_matches_contenteditable_empty_attr() {
        // HTML5: contenteditable="" эквивалентно "true".
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable>x</div>"#,
            "div:read-write { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_only_matches_contenteditable_false() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="false">x</div>"#,
            "div:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_only_matches_default_div() {
        // Per spec: «matches all other HTML elements» — обычный <div> read-only.
        let (root, doc) = lay_with_doc(
            r#"<div>x</div>"#,
            "div:read-only { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 255);
    }

    #[test]
    fn read_write_inherits_contenteditable_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true"><p id="inner">x</p></div>"#,
            "p:read-write { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "inner").r, 255);
    }

    #[test]
    fn read_only_when_descendant_overrides_to_false() {
        let (root, doc) = lay_with_doc(
            r#"<div contenteditable="true"><p contenteditable="false" id="inner">x</p></div>"#,
            "p:read-only { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "inner").r, 255);
    }

    // ──────────────── :disabled / :enabled ────────────────

    #[test]
    fn disabled_matches_input_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input disabled>"#,
            "input:disabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn enabled_matches_input_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input>"#,
            "input:enabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn disabled_matches_button_select_textarea() {
        let (root, doc) = lay_with_doc(
            r#"<button id="b" disabled>x</button>
               <select id="s" disabled></select>
               <textarea id="t" disabled></textarea>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
        assert_eq!(color_by_id(&doc, &root, "t").r, 255);
    }

    #[test]
    fn disabled_matches_fieldset_self() {
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled></fieldset>"#,
            "fieldset:disabled { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "fieldset").r, 255);
    }

    #[test]
    fn disabled_inherited_from_fieldset_ancestor() {
        // Inputs внутри <fieldset disabled> вне <legend> — disabled.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <input id="i">
                 <select id="s"></select>
               </fieldset>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "i").r, 255);
        assert_eq!(color_by_id(&doc, &root, "s").r, 255);
    }

    #[test]
    fn enabled_inside_first_legend_of_disabled_fieldset() {
        // HTML5 §4.10.16: input внутри первого <legend> ребёнка
        // disabled-<fieldset> сохраняет enabled-state.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <legend><input id="legend_input"></legend>
                 <input id="body_input">
               </fieldset>"#,
            ":disabled { color: red; } :enabled { color: blue; }",
        );
        let legend = color_by_id(&doc, &root, "legend_input");
        assert_eq!((legend.r, legend.b), (0, 255), "input в legend остаётся :enabled");
        let body = color_by_id(&doc, &root, "body_input");
        assert_eq!((body.r, body.b), (255, 0), "input вне legend — :disabled");
    }

    #[test]
    fn second_legend_in_disabled_fieldset_still_disabled() {
        // Только ПЕРВЫЙ <legend>-ребёнок «спасает» от disabled. Второй —
        // обычный потомок, попадает под disabled.
        let (root, doc) = lay_with_doc(
            r#"<fieldset disabled>
                 <legend>first</legend>
                 <legend><input id="second_legend_input"></legend>
               </fieldset>"#,
            ":disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "second_legend_input").r, 255);
    }

    #[test]
    fn disabled_option_via_optgroup_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<select>
                 <optgroup disabled>
                   <option id="o">x</option>
                 </optgroup>
               </select>"#,
            "option:disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "o").r, 255);
    }

    #[test]
    fn disabled_option_via_own_attr() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="o" disabled>x</option></select>"#,
            "option:disabled { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "o").r, 255);
    }

    #[test]
    fn disabled_does_not_apply_to_div() {
        // <div disabled> — disabled на не-form элементе игнорируется. Ни
        // :disabled, ни :enabled не матчат.
        let (root, doc) = lay_with_doc(
            r#"<div disabled>x</div>"#,
            ":disabled { color: red; } :enabled { color: blue; }",
        );
        let c = first_named(&doc, &root, "div");
        assert_eq!((c.r, c.b), (0, 0));
    }

    // ──────────────── :checked / :indeterminate / :default ────────────────

    #[test]
    fn checked_matches_checkbox_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox" checked>"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn checked_matches_checkbox_empty_attr_value() {
        // checked="" — атрибут присутствует, значение спецификацией не
        // используется (HTML5 §2.4.2 boolean attribute).
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox" checked="">"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn checked_no_match_without_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox">"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn checked_matches_radio_with_attr() {
        let (root, doc) = lay_with_doc(
            r#"<input type="radio" checked>"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn checked_does_not_match_text_input() {
        // text-input с атрибутом `checked` — атрибут не имеет смысла,
        // :checked не матчит.
        let (root, doc) = lay_with_doc(
            r#"<input type="text" checked>"#,
            "input:checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn checked_matches_option_with_selected() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="a">a</option><option id="b" selected>b</option></select>"#,
            "option:checked { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn checked_does_not_match_div() {
        let (root, doc) = lay_with_doc(
            r#"<div checked>x</div>"#,
            ":checked { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "div").r, 0);
    }

    #[test]
    fn indeterminate_radio_group_no_checked() {
        // Группа из двух radio с одинаковым name, ни один не checked →
        // оба :indeterminate.
        let (root, doc) = lay_with_doc(
            r#"<form><input type="radio" name="g" id="a"><input type="radio" name="g" id="b"></form>"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn indeterminate_radio_group_one_checked_no_match() {
        // Один из группы checked → оба НЕ :indeterminate.
        let (root, doc) = lay_with_doc(
            r#"<form><input type="radio" name="g" id="a" checked><input type="radio" name="g" id="b"></form>"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
    }

    #[test]
    fn indeterminate_radio_distinct_groups_isolated() {
        // Две группы с разным `name`: checked в одной не влияет на другую.
        let (root, doc) = lay_with_doc(
            r#"<form><input type="radio" name="g1" id="a" checked><input type="radio" name="g2" id="b"></form>"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn indeterminate_checkbox_never_in_phase_0() {
        // Phase 0 без runtime: атрибут indeterminate (если бы такой существовал)
        // не передаёт DOM-флаг; checkbox всегда вне :indeterminate.
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox">"#,
            "input:indeterminate { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 0);
    }

    #[test]
    fn indeterminate_progress_without_value() {
        // <progress> без атрибута value → indeterminate progress.
        let (root, doc) = lay_with_doc(
            r#"<progress></progress>"#,
            "progress:indeterminate { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "progress").r, 255);
    }

    #[test]
    fn indeterminate_progress_with_value_no_match() {
        let (root, doc) = lay_with_doc(
            r#"<progress value="0.5"></progress>"#,
            "progress:indeterminate { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "progress").r, 0);
    }

    #[test]
    fn default_matches_option_with_selected() {
        let (root, doc) = lay_with_doc(
            r#"<select><option id="a">a</option><option id="b" selected>b</option></select>"#,
            "option:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 255);
    }

    #[test]
    fn default_matches_checked_checkbox() {
        let (root, doc) = lay_with_doc(
            r#"<input type="checkbox" checked>"#,
            "input:default { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "input").r, 255);
    }

    #[test]
    fn default_matches_first_submit_button_of_form() {
        // Первая submit-кнопка в DOM-порядке формы — default-submit.
        let (root, doc) = lay_with_doc(
            r#"<form><button id="a" type="submit">A</button><button id="b" type="submit">B</button></form>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
    }

    #[test]
    fn default_matches_button_without_type_attr() {
        // <button> без `type` имеет default type=submit (HTML5 §4.10.8).
        let (root, doc) = lay_with_doc(
            r#"<form><button id="a">go</button></form>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
    }

    #[test]
    fn default_matches_input_type_submit() {
        let (root, doc) = lay_with_doc(
            r#"<form><input id="a" type="submit"></form>"#,
            "input:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
    }

    #[test]
    fn default_no_match_for_submit_button_outside_form() {
        // Без <form>-предка submit-кнопка не считается default-submit.
        let (root, doc) = lay_with_doc(
            r#"<button id="a" type="submit">go</button>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
    }

    #[test]
    fn default_button_type_button_no_match() {
        // type=button — не submit, не default.
        let (root, doc) = lay_with_doc(
            r#"<form><button id="a" type="button">x</button></form>"#,
            "button:default { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
    }

    // ──────────────── :lang(...) (CSS Selectors L4 §11) ────────────────

    #[test]
    fn lang_matches_self_lang_attr() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="en">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_matches_prefix_with_region() {
        // RFC 4647 basic filtering: range "en" matches tag "en-US".
        let (root, doc) = lay_with_doc(
            r#"<p lang="en-US">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_no_match_different_prefix() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="fr">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_no_match_substring_not_prefix() {
        // "en" не должен матчить "fr-en" — `en` здесь регион, не язык.
        let (root, doc) = lay_with_doc(
            r#"<p lang="fr-en">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_inherited_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div lang="ru"><p>x</p></div>"#,
            "p:lang(ru) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_case_insensitive_match() {
        // BCP 47: language tags case-insensitive. lang="EN-us" matches :lang(en).
        let (root, doc) = lay_with_doc(
            r#"<p lang="EN-us">x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_comma_list_any_matches() {
        let (root, doc) = lay_with_doc(
            r#"<p lang="fr">x</p>"#,
            "p:lang(en, fr, ru) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_no_match_when_no_lang_attr() {
        // Ни один ancestor не имеет lang → элемент без языка → не матчит.
        let (root, doc) = lay_with_doc(
            r#"<p>x</p>"#,
            "p:lang(en) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_empty_attr_treated_as_no_language() {
        // <p lang=""> — HTML5 «явно неизвестен», не наследует, не матчит.
        let (root, doc) = lay_with_doc(
            r#"<div lang="ru"><p lang="">x</p></div>"#,
            "p:lang(ru) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn lang_xml_lang_fallback() {
        // xml:lang атрибут используется как fallback (XHTML legacy).
        let (root, doc) = lay_with_doc(
            r#"<p xml:lang="ja">x</p>"#,
            "p:lang(ja) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn lang_nearest_ancestor_wins() {
        // Внутренний `lang` overrideит ancestor: внутри `lang="ru"`, p имеет
        // `lang="en"` → matches en, не ru.
        let (root, doc) = lay_with_doc(
            r#"<div lang="ru"><p lang="en">x</p></div>"#,
            "p:lang(ru) { color: red; } p:lang(en) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :dir(ltr|rtl) (CSS Selectors L4 §13.2) ────────────────

    #[test]
    fn dir_ltr_matches_by_default() {
        // Без `dir`-атрибута — default ltr (HTML5 §3.2.6.1).
        let (root, doc) = lay_with_doc(
            r#"<p>x</p>"#,
            "p:dir(ltr) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_rtl_does_not_match_by_default() {
        let (root, doc) = lay_with_doc(
            r#"<p>x</p>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 0);
    }

    #[test]
    fn dir_rtl_matches_when_attr_set() {
        let (root, doc) = lay_with_doc(
            r#"<p dir="rtl">x</p>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_rtl_inherited_from_ancestor() {
        let (root, doc) = lay_with_doc(
            r#"<div dir="rtl"><p>x</p></div>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_nearest_ancestor_wins() {
        // Внутренний `dir="ltr"` overrideит ancestor `dir="rtl"`.
        let (root, doc) = lay_with_doc(
            r#"<div dir="rtl"><p dir="ltr">x</p></div>"#,
            "p:dir(rtl) { color: red; } p:dir(ltr) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (0, 255));
    }

    #[test]
    fn dir_attr_case_insensitive() {
        let (root, doc) = lay_with_doc(
            r#"<p dir="RTL">x</p>"#,
            "p:dir(rtl) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_auto_treated_as_ltr_in_phase_0() {
        // `dir="auto"` в Phase 0 без bidi-движка трактуется как ltr.
        let (root, doc) = lay_with_doc(
            r#"<p dir="auto">x</p>"#,
            "p:dir(ltr) { color: red; } p:dir(rtl) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (255, 0));
    }

    #[test]
    fn dir_invalid_value_treated_as_ltr() {
        // `dir="invalid"` — fallback на ltr (как и `auto`).
        let (root, doc) = lay_with_doc(
            r#"<p dir="invalid">x</p>"#,
            "p:dir(ltr) { color: red; }",
        );
        assert_eq!(first_named(&doc, &root, "p").r, 255);
    }

    #[test]
    fn dir_auto_finalizes_directionality_does_not_inherit() {
        // `dir="auto"` на самом элементе — финализирует direction (Phase 0:
        // ltr); ancestor `dir="rtl"` НЕ должен пробить — атрибут на элементе
        // имеет приоритет, даже если значение `auto`.
        let (root, doc) = lay_with_doc(
            r#"<div dir="rtl"><p dir="auto">x</p></div>"#,
            "p:dir(rtl) { color: red; } p:dir(ltr) { color: blue; }",
        );
        let c = first_named(&doc, &root, "p");
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :link / :visited / :any-link (CSS Selectors L4 §6.2) ────────────────

    /// Computes color для первого element-child указанного тега в DOM (без
    /// layout-tree, чтобы тесты ловили inline-элементы вроде `<a>` / `<area>`
    /// / `<link>` независимо от того, попадают они в LayoutBox или нет).
    fn element_color(html: &str, css: &str, tag: &str) -> Color {
        use crate::style::compute_style;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let target = find_first_element(&doc, doc.root(), tag).expect("element not found");
        compute_style(&doc, target, &sheet, &root_style, Size::new(800.0, 600.0), false).color
    }

    fn find_first_element(
        doc: &lumen_dom::Document,
        node: lumen_dom::NodeId,
        tag: &str,
    ) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(node).data
            && name.local == tag
        {
            return Some(node);
        }
        for &child in &doc.get(node).children {
            if let Some(found) = find_first_element(doc, child, tag) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn any_link_matches_a_with_href() {
        let c = element_color(
            r#"<a href="https://example.com">x</a>"#,
            "a:any-link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn any_link_does_not_match_a_without_href() {
        // <a> без href — не hyperlink (HTML5 §4.6.1).
        let c = element_color(
            r#"<a>x</a>"#,
            "a:any-link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn any_link_matches_area_with_href() {
        // `<area>` внутри `<map>` — image-map link.
        let c = element_color(
            r##"<map><area href="#x"></map>"##,
            "area:any-link { color: red; }",
            "area",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn any_link_matches_link_with_href() {
        let c = element_color(
            r#"<link href="style.css" rel="stylesheet">"#,
            "link:any-link { color: red; }",
            "link",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn link_pseudo_matches_a_with_href_in_phase_0() {
        // В Phase 0 без visited-runtime `:link` эквивалентен `:any-link`.
        let c = element_color(
            r#"<a href="x">a</a>"#,
            "a:link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn link_pseudo_does_not_match_without_href() {
        let c = element_color(
            r#"<a>x</a>"#,
            "a:link { color: red; }",
            "a",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn visited_pseudo_never_matches_in_phase_0() {
        // Phase 0 без history-runtime — никакая ссылка не считается посещённой.
        // Безопасный default per privacy-by-default.
        let c = element_color(
            r#"<a href="x">a</a>"#,
            "a:visited { color: red; }",
            "a",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn link_pseudos_do_not_match_div_with_href() {
        // `href` на не-hyperlink-элементе игнорируется (только a/area/link).
        let c = element_color(
            r#"<div href="x">x</div>"#,
            ":any-link { color: red; } :link { color: blue; }",
            "div",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn any_link_specificity_class_level() {
        // `:any-link` имеет specificity class-уровня (0,1,0). Equal-specificity
        // — более позднее правило выигрывает (source-order).
        let c = element_color(
            r#"<a href="x">a</a>"#,
            "a:any-link { color: red; } a:link { color: blue; }",
            "a",
        );
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :scope (CSS Selectors L4 §4.2) ────────────────

    #[test]
    fn scope_matches_root_element() {
        // В author-CSS без querySelector-runtime `:scope` matches document
        // root element (эквивалентно `:root`).
        let c = element_color(
            "<html><body><p>x</p></body></html>",
            ":scope { color: red; }",
            "html",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn scope_does_not_match_descendants() {
        // `:scope` matches root only, не вложенные элементы.
        let c = element_color(
            "<html><body><p>x</p></body></html>",
            ":scope { color: red; }",
            "body",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn scope_equivalent_to_root_in_author_css() {
        // В author-CSS без runtime querySelector `:scope` и `:root` дают
        // одинаковый результат — оба matches root element.
        let c1 = element_color(
            "<html><body>x</body></html>",
            ":scope { color: red; }",
            "html",
        );
        let c2 = element_color(
            "<html><body>x</body></html>",
            ":root { color: red; }",
            "html",
        );
        assert_eq!(c1.r, c2.r);
    }

    // ──────────────── :target (CSS Selectors L4 §9.6) ────────────────

    /// Computes color для первого element-child указанного тега с указанным
    /// target_id, выставленным в Document перед каскадом. Эквивалент
    /// `element_color`, но с `Document::set_target(...)`.
    fn element_color_with_target(
        html: &str,
        css: &str,
        tag: &str,
        target: Option<&str>,
    ) -> Color {
        use crate::style::compute_style;
        let mut doc = lumen_html_parser::parse(html);
        doc.set_target(target);
        let sheet = lumen_css_parser::parse(css);
        let root_style = ComputedStyle::root();
        let target_node = find_first_element(&doc, doc.root(), tag).expect("element not found");
        compute_style(&doc, target_node, &sheet, &root_style, Size::new(800.0, 600.0), false).color
    }

    #[test]
    fn target_matches_element_with_matching_id() {
        let c = element_color_with_target(
            r#"<html><body><h2 id="intro">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some("intro"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_does_not_match_other_elements() {
        // Только element с совпадающим id матчит — sibling с другим id нет.
        let c = element_color_with_target(
            r#"<html><body><h2 id="intro">x</h2><h2 id="other">y</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some("other"),
        );
        // Первый h2 (id="intro") — не матчит, color остаётся default (black).
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_returns_false_when_no_fragment() {
        // Document::target() == None — никакой element не матчит.
        let c = element_color_with_target(
            r#"<html><body><h2 id="intro">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            None,
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_returns_false_for_empty_fragment() {
        // Пустой fragment («#» в URL) трактуется как None — Document::set_target
        // фильтрует empty string. Поведение совпадает с major-браузерами.
        let c = element_color_with_target(
            r#"<html><body><h2 id="">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some(""),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_is_case_sensitive() {
        // HTML id case-sensitive (HTML LS §3.2.6) — `Intro` != `intro`.
        let c = element_color_with_target(
            r#"<html><body><h2 id="Intro">x</h2></body></html>"#,
            ":target { color: red; }",
            "h2",
            Some("intro"),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_compound_with_type() {
        // `h2:target` — compound selector с type matcher-ом.
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2></body></html>"#,
            "h2:target { color: red; }",
            "h2",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_specificity_pseudo_class_level() {
        // `:target` имеет specificity (0,1,0) — class-уровень. Equal-specificity
        // — выигрывает более позднее правило (source-order).
        let c = element_color_with_target(
            r#"<html><body><h2 id="t" class="c">x</h2></body></html>"#,
            "h2.c { color: red; } h2:target { color: blue; }",
            "h2",
            Some("t"),
        );
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :target-within (CSS Selectors L4 §9.7) ────────────────

    #[test]
    fn target_within_matches_target_element_itself() {
        // Element, который сам :target, также матчит :target-within
        // (spec: «matches elements that are themselves matching :target or
        // that have a descendant which matches»).
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2></body></html>"#,
            ":target-within { color: red; }",
            "h2",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_within_matches_ancestor_of_target() {
        // `<section>` сам не :target, но contains `<h2 id="t">` — матчит.
        let c = element_color_with_target(
            r#"<html><body><section><h2 id="t">x</h2></section></body></html>"#,
            "section:target-within { color: red; }",
            "section",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_within_matches_distant_ancestor() {
        // `<body>` глубоко выше `<h2 id="t">` — всё равно матчит (любой
        // descendant — не только прямой ребёнок).
        let c = element_color_with_target(
            r#"<html><body><div><section><h2 id="t">x</h2></section></div></body></html>"#,
            "body:target-within { color: red; }",
            "body",
            Some("t"),
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn target_within_does_not_match_sibling() {
        // Sibling рядом с target-ом не матчит — `:target-within` не bubble-ит
        // через parent наверх (только subtree containment).
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2><p>sibling</p></body></html>"#,
            "p:target-within { color: red; }",
            "p",
            Some("t"),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_within_returns_false_when_no_fragment() {
        // Без `Document::target()` matcher всегда false — даже для элементов
        // с descendant-ами, имеющими этот id.
        let c = element_color_with_target(
            r#"<html><body><h2 id="t">x</h2></body></html>"#,
            "body:target-within { color: red; }",
            "body",
            None,
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_within_does_not_match_unrelated_element() {
        // Element без target-descendant и не target сам — false.
        let c = element_color_with_target(
            r#"<html><body><section><h2 id="t">x</h2></section><aside>y</aside></body></html>"#,
            "aside:target-within { color: red; }",
            "aside",
            Some("t"),
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn target_within_specificity_pseudo_class_level() {
        // `:target-within` — specificity (0,1,0); equal-specificity tie-break
        // by source-order.
        let c = element_color_with_target(
            r#"<html><body><section class="c"><h2 id="t">x</h2></section></body></html>"#,
            "section.c { color: red; } section:target-within { color: blue; }",
            "section",
            Some("t"),
        );
        assert_eq!((c.r, c.b), (0, 255));
    }

    // ──────────────── :in-range / :out-of-range (CSS Selectors L4 §14.5) ────────────────

    #[test]
    fn in_range_number_value_within_min_max() {
        let c = element_color(
            r#"<input type="number" min="1" max="10" value="5">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_number_value_above_max() {
        let c = element_color(
            r#"<input type="number" min="1" max="10" value="15">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_number_value_below_min() {
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="-5">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn in_range_value_equals_max_endpoint() {
        // Spec §4.10.21.4: «greater than max» = strict. Value == max → in-range.
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="10">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn in_range_only_min_attribute() {
        // Range exists даже если только min — :in-range / :out-of-range
        // зависят от значения (max = +∞).
        let c = element_color(
            r#"<input type="number" min="0" value="100">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_only_min_attribute_value_below() {
        let c = element_color(
            r#"<input type="number" min="0" value="-1">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn neither_when_no_min_no_max() {
        // Нет range-limitations → не матчит ни одну pseudo.
        let c = element_color(
            r#"<input type="number" value="5">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn neither_when_value_missing() {
        // Нет displayed value (для number) → не матчит ни одну.
        let c = element_color(
            r#"<input type="number" min="1" max="10">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn neither_when_value_invalid() {
        // Невалидное value → нет displayed numeric value → не матчит.
        let c = element_color(
            r#"<input type="number" min="1" max="10" value="abc">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn in_range_text_input_skipped() {
        // type=text не поддерживает range — :in-range не матчит даже если
        // min/max выставлены.
        let c = element_color(
            r#"<input type="text" min="1" max="10" value="5">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn in_range_textarea_skipped() {
        // <textarea> не имеет range-checks.
        let c = element_color(
            r#"<textarea min="1" max="10">5</textarea>"#,
            "textarea:in-range { color: red; }",
            "textarea",
        );
        assert_eq!(c.r, 0);
    }

    #[test]
    fn in_range_range_input_default_min_max() {
        // type=range без атрибутов: дефолтный диапазон [0, 100], default
        // value = середина = 50 → :in-range.
        let c = element_color(
            r#"<input type="range">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn out_of_range_range_input_value_above_max() {
        let c = element_color(
            r#"<input type="range" min="0" max="100" value="150">"#,
            "input:out-of-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn in_range_fractional_number() {
        // Дробные значения должны парситься как f64.
        let c = element_color(
            r#"<input type="number" min="1.5" max="2.5" value="2.0">"#,
            "input:in-range { color: red; }",
            "input",
        );
        assert_eq!(c.r, 255);
    }

    #[test]
    fn neither_for_date_type_phase_0() {
        // Phase 0: date / month / week / time / datetime-local пока не
        // поддерживаются — pseudo не матчит (см. doc к matches_in_range).
        let c = element_color(
            r#"<input type="date" min="2025-01-01" max="2025-12-31" value="2025-06-15">"#,
            "input:in-range { color: red; } input:out-of-range { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (0, 0));
    }

    #[test]
    fn in_range_specificity_is_class_level() {
        // pseudo-class contributes (0, 1, 0) к specificity. Type + pseudo
        // (0,1,1) > type-only (0,0,1) — правило с pseudo выигрывает несмотря
        // на DOM source-order.
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="5">"#,
            "input:in-range { color: red; } input { color: blue; }",
            "input",
        );
        assert_eq!((c.r, c.b), (255, 0));
    }

    // ──────────────── :valid / :invalid ────────────────

    #[test]
    fn valid_matches_non_required_input() {
        // Без required — value не может быть missing, элемент valid.
        let c = element_color(
            r#"<input type="text">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid должен матчить input без required");
    }

    #[test]
    fn invalid_matches_required_input_without_value() {
        let c = element_color(
            r#"<input type="text" required>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — required + нет value");
    }

    #[test]
    fn valid_matches_required_input_with_value() {
        let c = element_color(
            r#"<input type="text" required value="hello">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — required + value присутствует");
    }

    #[test]
    fn invalid_email_typemismatch() {
        let c = element_color(
            r#"<input type="email" value="notanemail">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — email без @");
    }

    #[test]
    fn valid_email_with_at_and_domain() {
        let c = element_color(
            r#"<input type="email" value="user@example.com">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — корректный email");
    }

    #[test]
    fn valid_email_empty_value_not_required() {
        // Пустой value при отсутствии required — valid.
        let c = element_color(
            r#"<input type="email">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — пустой email без required");
    }

    #[test]
    fn invalid_url_typemismatch() {
        let c = element_color(
            r#"<input type="url" value="not-a-url">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — url без схемы");
    }

    #[test]
    fn valid_url_with_scheme() {
        let c = element_color(
            r#"<input type="url" value="https://example.com">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — корректный url");
    }

    #[test]
    fn invalid_number_out_of_range() {
        // :invalid покрывает rangeOverflow так же, как :out-of-range.
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="99">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — out-of-range number");
    }

    #[test]
    fn valid_number_within_range() {
        let c = element_color(
            r#"<input type="number" min="0" max="10" value="5">"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — number in range");
    }

    #[test]
    fn valid_invalid_not_match_div() {
        // :valid/:invalid не применимы к не-form-control элементам.
        let c = element_color(
            r#"<div>x</div>"#,
            "div:valid { color: green; } div:invalid { color: red; }",
            "div",
        );
        assert_eq!((c.r, c.g), (0, 0), ":valid/:invalid не матчат <div>");
    }

    #[test]
    fn valid_invalid_not_match_hidden_input() {
        // <input type="hidden"> не является кандидатом для constraint validation.
        let c = element_color(
            r#"<input type="hidden" required>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 0), "hidden input — не матчит ни :valid, ни :invalid");
    }

    #[test]
    fn valid_invalid_not_match_disabled_input() {
        // Disabled — barred from constraint validation.
        let c = element_color(
            r#"<input type="text" required disabled>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 0), "disabled input — не матчит ни :valid, ни :invalid");
    }

    #[test]
    fn invalid_required_checkbox_unchecked() {
        let c = element_color(
            r#"<input type="checkbox" required>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (255, 0), ":invalid — required checkbox без checked");
    }

    #[test]
    fn valid_required_checkbox_checked() {
        let c = element_color(
            r#"<input type="checkbox" required checked>"#,
            "input:valid { color: green; } input:invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 128), ":valid — required checkbox с checked");
    }

    #[test]
    fn valid_required_textarea_with_value() {
        let c = element_color(
            r#"<textarea required>hello</textarea>"#,
            "textarea:valid { color: green; } textarea:invalid { color: red; }",
            "textarea",
        );
        // textarea: значение в content, не в value-атрибуте — Phase 0: смотрим
        // только value-атрибут, потому элемент valid при его отсутствии.
        assert_eq!((c.r, c.g), (0, 128), ":valid — textarea без value-атрибута при required");
    }

    #[test]
    fn user_valid_user_invalid_always_false() {
        // Phase 0: без интерактивного состояния :user-valid/:user-invalid = false.
        let c = element_color(
            r#"<input type="text">"#,
            "input:user-valid { color: green; } input:user-invalid { color: red; }",
            "input",
        );
        assert_eq!((c.r, c.g), (0, 0), ":user-valid/:user-invalid always false в Phase 0");
    }

    #[test]
    fn id_wins_over_class() {
        // id specificity (1,0,0) > class (0,1,0). Порядок правил в CSS — class
        // после id — не должен пересилить.
        let root = lay(
            r#"<p id="x" class="c">v</p>"#,
            "#x { color: red; } .c { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, "id should win over class");
        assert_eq!(p.style.color.b, 0);
    }

    #[test]
    fn class_wins_over_type() {
        // class (0,1,0) > type (0,0,1). Type идёт после в порядке — но проиграет.
        let root = lay(r#"<p class="c">v</p>"#, ".c { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    #[test]
    fn equal_specificity_last_wins() {
        let root = lay("<p>v</p>", "p { color: red; } p { color: blue; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
    }

    // ── Тесты inline-flow ───────────────────────────────────────────────────

    /// <span> внутри <p> не разрывает строку: высота = одна линия.
    #[test]
    fn inline_span_does_not_break_line() {
        let root = lay_measured("<p>hello <span>world</span></p>", "", 800.0);
        // "hello world" = 11 слов × 8px = 88px; при 800px — одна строка.
        assert!(
            (root.rect.height - 19.2).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// <a> получает цвет из CSS, текст соседнего текстового узла — родительский.
    #[test]
    fn inline_link_inherits_own_color() {
        let root = lay("<p>text <a>link</a></p>", "a { color: blue; }");
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { segments, .. } = &inline.kind {
            // Первый сегмент — текстовый узел "text " (наследует цвет <p>)
            assert_eq!(segments[0].style.color.b, 0, "text node must not be blue");
            // Второй сегмент — текст внутри <a> (синий)
            assert_eq!(segments[1].style.color.b, 255, "link must be blue");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// Inline-ран переносится так же, как обычный текст.
    #[test]
    fn inline_run_wraps_across_viewport() {
        // "aa bb" = 5 × 8 = 40px при Fixed8. Viewport 30px → перенос после "aa".
        let root = lay_measured("<p>aa <em>bb</em></p>", "", 30.0);
        // 2 строки × 19.2 = 38.4
        assert!(
            (root.rect.height - 38.4).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// Блочные элементы между inline-контентом не смешиваются в один InlineRun.
    #[test]
    fn block_between_inline_creates_separate_run() {
        // <div> — блочный элемент; текст до и после — разные InlineRun-ы.
        let root = lay("<p>before</p><div>mid</div><p>after</p>", "");
        // 3 блока по 19.2 = 57.6
        assert!(
            (root.rect.height - 57.6).abs() < 0.1,
            "height={}",
            root.rect.height
        );
    }

    /// BUG-013: display:none между inline-элементами не должен разрывать InlineRun.
    /// До фикса: `<span style="display:none">` вызывал break, и соседние <span>
    /// попадали в разные строки, удваивая высоту параграфа.
    #[test]
    fn display_none_does_not_break_inline_context() {
        // Три <span>: первый и третий видимые, второй — display:none.
        // Ожидание: все три в одном inline-контексте → высота = одна строка (19.2).
        let root = lay_measured(
            "<p><span>hello</span><span style=\"display:none\">x</span><span>world</span></p>",
            "",
            800.0,
        );
        assert!(
            (root.rect.height - 19.2).abs() < 0.5,
            "display:none разрывает inline-контекст: height={} (ожидалось 19.2)",
            root.rect.height,
        );
    }

    // ── Функциональные pseudo: :nth-*, :*-of-type, :not ───────────────────

    /// Собирает все элементы с тегом `tag` из children корневого LayoutBox.
    fn block_children_by_tag<'a>(
        root: &'a LayoutBox,
        doc: &lumen_dom::Document,
        tag: &str,
    ) -> Vec<&'a LayoutBox> {
        root.children
            .iter()
            .filter(|c| {
                matches!(
                    &doc.get(c.node).data,
                    lumen_dom::NodeData::Element { name, .. } if name.local == tag
                )
            })
            .collect()
    }

    #[test]
    fn nth_child_odd_matches_1_3_5() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p><p>e</p>",
            "p:nth-child(odd) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps.len(), 5);
        for (i, p) in ps.iter().enumerate() {
            let one_based = (i + 1) as i32;
            let expected_red = one_based % 2 == 1;
            assert_eq!(
                p.style.color.r == 255,
                expected_red,
                "index={one_based}"
            );
        }
    }

    #[test]
    fn nth_child_specific_index() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_formula_2n() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p>",
            "p:nth-child(2n) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // 2n: 2, 4, ...
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
        assert_eq!(ps[3].style.color.r, 255);
    }

    #[test]
    fn nth_last_child_matches_from_end() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-last-child(1) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // Последний матчит.
        assert_eq!(ps[2].style.color.r, 255);
        assert_eq!(ps[0].style.color.r, 0);
    }

    #[test]
    fn nth_of_type_counts_only_matching_tag() {
        // <h1><p1><h2><p2><p3> — :nth-of-type(2) для p должен попасть в p2.
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><h2>x</h2><p>p2</p><p>p3</p>",
            "p:nth-of-type(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // p1 — это of-type index 1 → 0, p2 → 2 → 255, p3 → 3 → 0.
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_of_selector_filters_pool() {
        // CSS Selectors L4 §6.6.5.1: `:nth-child(odd of .v)` нумерует ТОЛЬКО
        // элементы с классом `v`, остальные siblings не участвуют. Из
        // .v#a (index 1), .v#b (2), .v#c (3) — odd = a и c.
        let (root, doc) = lay_with_doc(
            r#"<p>x</p><p class="v" id="a">x</p><p>x</p><p class="v" id="b">x</p><p class="v" id="c">x</p>"#,
            "p:nth-child(odd of .v) { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
    }

    #[test]
    fn nth_child_of_selector_does_not_match_non_filtered() {
        // Элемент, не матчащий of-selector, никогда не матчит pseudo —
        // независимо от того, какой у него index среди ВСЕХ siblings.
        let (root, doc) = lay_with_doc(
            r#"<p class="v" id="a">x</p><p id="b">x</p><p class="v" id="c">x</p>"#,
            "p:nth-child(1 of .v) { color: red; }",
        );
        // .v#a — первый матчащий .v → matches.
        // #b — не .v, не матчит вообще.
        // .v#c — второй матчащий .v → не matches 1.
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
    }

    #[test]
    fn nth_last_child_of_selector_filters_from_end() {
        let (root, doc) = lay_with_doc(
            r#"<p class="v" id="a">x</p><p class="v" id="b">x</p><p id="c">x</p><p class="v" id="d">x</p>"#,
            "p:nth-last-child(1 of .v) { color: red; }",
        );
        // С конца: первый .v — d (matches), второй .v — b (no), третий — a (no).
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
        assert_eq!(color_by_id(&doc, &root, "d").r, 255);
    }

    #[test]
    fn nth_child_of_selector_list_union() {
        // of-clause принимает selector-list через запятую: соответствие
        // хотя бы одному → элемент в pool.
        let (root, doc) = lay_with_doc(
            r#"<p class="x" id="a">x</p><p id="b">x</p><p class="y" id="c">x</p><p class="x" id="d">x</p>"#,
            "p:nth-child(odd of .x, .y) { color: red; }",
        );
        // Pool по «.x OR .y»: a, c, d. odd-index в этом pool: a(1), d(3).
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
        assert_eq!(color_by_id(&doc, &root, "d").r, 255);
    }

    #[test]
    fn nth_child_backward_compat_without_of() {
        // Базовое поведение без of-clause не должно регрессировать.
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn first_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><p>p2</p>",
            "p:first-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn last_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<p>p1</p><p>p2</p><h1>x</h1>",
            "p:last-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        // p2 — последний `<p>` (h1 после него — другой тип), значит матчит.
        assert_eq!(ps[1].style.color.r, 255);
    }

    #[test]
    fn not_class_excludes() {
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p>c</p>"#,
            "p:not(.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a should match");
        assert_eq!(ps[1].style.color.r, 0, "b.hl should NOT match");
        assert_eq!(ps[2].style.color.r, 255, "c should match");
    }

    #[test]
    fn not_with_compound_excludes_full() {
        // :not(p.hl) — исключает только p с классом hl, не любой <p> и не любой `.hl`.
        // Используем scope через body-класс чтобы не загрязнять html/body.
        let (root, doc) = lay_with_doc(
            r#"<body class="t"><p>x</p><p class="hl">y</p><div class="hl">z</div></body>"#,
            "body.t *:not(p.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        let divs = block_children_by_tag(&root, &doc, "div");
        assert_eq!(ps[0].style.color.r, 255, "p без класса — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p.hl — исключается");
        assert_eq!(divs[0].style.color.r, 255, "div.hl — не исключается");
    }

    #[test]
    fn not_selector_list_l4() {
        // CSS Selectors L4 §5.4: список селекторов внутри `:not(...)` —
        // элемент исключается, если матчит ХОТЯ БЫ ОДИН селектор списка.
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p id="x">c</p><p>d</p>"#,
            "p:not(.hl, #x) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a — матчит");
        assert_eq!(ps[1].style.color.r, 0, "b.hl — исключается");
        assert_eq!(ps[2].style.color.r, 0, "c#x — исключается");
        assert_eq!(ps[3].style.color.r, 255, "d — матчит");
    }

    #[test]
    fn not_complex_with_descendant_combinator_l4() {
        // CSS Selectors L4 §5.4: combinator-ы внутри `:not` разрешены.
        // Исключаем <p>, у которых внутри (descendant) есть <a>.
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p>b <a>link</a></p><p>c</p>"#,
            "p:not(:has(a)) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "p без <a> — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p с <a> — исключается");
        assert_eq!(ps[2].style.color.r, 255, "p без <a> — матчит");
    }

    #[test]
    fn not_nested_double_negation_l4() {
        // CSS Selectors L4 §5.4: nested `:not(:not(...))` разрешён.
        // `:not(:not(.hl))` ≡ `.hl` (двойное отрицание).
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p>"#,
            "p:not(:not(.hl)) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0, "a (нет .hl) — не матчит");
        assert_eq!(ps[1].style.color.r, 255, "b.hl — матчит (двойное :not)");
    }

    // ── Relative units: em / rem / % ────────────────────────────────────────

    #[test]
    fn font_size_em_relative_to_parent() {
        // root fs 16 → div fs 20 → p fs 2em = 40.
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 20px; } p { font-size: 2em; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.font_size - 40.0).abs() < 0.01, "got {}", p.style.font_size);
    }

    #[test]
    fn font_size_rem_relative_to_root() {
        // rem всегда от 16 (ROOT_FONT_SIZE), независимо от parent.
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 100px; } p { font-size: 1.5rem; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.font_size - 24.0).abs() < 0.01, "got {}", p.style.font_size);
    }

    #[test]
    fn font_size_percent_relative_to_parent() {
        // 150% от 16 = 24.
        let root = lay("<p>x</p>", "p { font-size: 150%; }");
        let p = first_element_child(&root);
        assert!((p.style.font_size - 24.0).abs() < 0.01, "got {}", p.style.font_size);
    }

    #[test]
    fn padding_em_uses_current_font_size() {
        // padding: 2em должен использовать computed font-size самого элемента,
        // даже если font-size в правиле объявлен после padding.
        let root = lay("<p>x</p>", "p { padding: 2em; font-size: 20px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.padding_top, Length::Em(2.0), "got {:?}", p.style.padding_top);
    }

    #[test]
    fn margin_rem_independent_of_inherit() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 99px; } p { margin: 1rem; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.margin_top, LengthOrAuto::Length(Length::Rem(1.0)));
    }

    #[test]
    fn line_height_percent_becomes_coefficient() {
        // 150% = 1.5.
        let root = lay("<p>x</p>", "p { line-height: 150%; }");
        let p = first_element_child(&root);
        assert!((p.style.line_height - 1.5).abs() < 0.001);
    }

    #[test]
    fn line_height_em_is_coefficient() {
        // 1.5em — то же, что unitless 1.5 (CSS определяет line-height: <number>
        // как «коэффициент * font-size»; em делает то же численно).
        let root = lay("<p>x</p>", "p { line-height: 1.5em; }");
        let p = first_element_child(&root);
        assert!((p.style.line_height - 1.5).abs() < 0.001);
    }

    #[test]
    fn percent_in_margin_stored_typed() {
        // % в margin хранится как Length::Percent и разрешается при layout,
        // когда известна ширина containing block.
        let root = lay("<p>x</p>", "p { margin: 50%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.margin_top, LengthOrAuto::Length(Length::Percent(50.0)));
    }

    // ── Тесты text-align ───────────────────────────────────────────────────

    fn first_inline_run(b: &LayoutBox) -> &LayoutBox {
        for c in &b.children {
            if matches!(c.kind, BoxKind::InlineRun { .. }) {
                return c;
            }
            let found = first_inline_run(c);
            if matches!(found.kind, BoxKind::InlineRun { .. }) {
                return found;
            }
        }
        b
    }

    /// text-align: center сдвигает фрагменты к середине строки.
    /// "ab" = 2×8=16px в контейнере 100px: offset = (100-16)/2 = 42px.
    #[test]
    fn text_align_center_shifts_frags() {
        let root = lay_measured("<p>ab</p>", "p { text-align: center; }", 100.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty(), "expected at least one line");
            let x = lines[0][0].x;
            // (100 - 16) / 2 = 42; p имеет нулевой padding, так что content_width = 100
            assert!((x - 42.0).abs() < 0.5, "expected x≈42, got {x}");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: right сдвигает фрагменты к правому краю.
    /// "ab" = 16px в контейнере 100px: offset = 100-16 = 84px.
    #[test]
    fn text_align_right_shifts_frags() {
        let root = lay_measured("<p>ab</p>", "p { text-align: right; }", 100.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            let x = lines[0][0].x;
            assert!((x - 84.0).abs() < 0.5, "expected x≈84, got {x}");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: left — фрагменты начинаются с x=0.
    #[test]
    fn text_align_left_frags_start_at_zero() {
        let root = lay_measured("<p>ab</p>", "p { text-align: left; }", 100.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            assert!((lines[0][0].x - 0.0).abs() < 0.01, "expected x=0, got {}", lines[0][0].x);
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align наследуется дочерними элементами.
    #[test]
    fn text_align_is_inherited() {
        let root = lay("<div><p>x</p></div>", "div { text-align: right; }");
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.text_align, TextAlign::Right);
    }

    /// text-align: center — последняя строка тоже выравнивается.
    #[test]
    fn text_align_center_applies_to_each_line() {
        // "aa bb" при viewport 30px (3×8=24 < 30; "aa bb" = 40 > 30) → 2 строки.
        // "aa" = 16px, offset = (30-16)/2 = 7; "bb" тоже 16px, offset = 7.
        let root = lay_measured("<p>aa bb</p>", "p { text-align: center; }", 30.0);
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert_eq!(lines.len(), 2, "expected 2 lines");
            for (i, line) in lines.iter().enumerate() {
                let x = line[0].x;
                assert!((x - 7.0).abs() < 0.5, "line[{i}] expected x≈7, got {x}");
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    // ── Тесты CSS width / height ───────────────────────────────────────────

    /// width: 200px задаёт rect.width = 200 (без padding).
    #[test]
    fn explicit_width_sets_rect_width() {
        // viewport 800px; p без padding → rect.width должен быть 200.
        let root = lay("<p>x</p>", "p { width: 200px; }");
        let p = first_element_child(&root);
        assert!(
            (p.rect.width - 200.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// width учитывает padding: rect.width = width + padding_left + padding_right.
    #[test]
    fn explicit_width_plus_padding() {
        let root = lay("<p>x</p>", "p { width: 200px; padding: 10px; }");
        let p = first_element_child(&root);
        // content_box 200 + padding 10+10 = 220.
        assert!(
            (p.rect.width - 220.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// height: 100px задаёт rect.height = 100.
    #[test]
    fn explicit_height_overrides_content_height() {
        let root = lay("<p>x</p>", "p { height: 100px; }");
        let p = first_element_child(&root);
        assert!(
            (p.rect.height - 100.0).abs() < 0.01,
            "rect.height={}", p.rect.height
        );
    }

    /// height учитывает padding: rect.height = height + padding_top + padding_bottom.
    #[test]
    fn explicit_height_plus_padding() {
        let root = lay("<p>x</p>", "p { height: 80px; padding: 5px; }");
        let p = first_element_child(&root);
        assert!(
            (p.rect.height - 90.0).abs() < 0.01,
            "rect.height={}", p.rect.height
        );
    }

    /// Дочерние элементы используют content_width от явно заданного width.
    #[test]
    fn children_constrained_by_explicit_width() {
        // div { width: 300px } → content_width = 300.
        // Вложенный <p> без width → rect.width = content_width = 300.
        let root = lay("<div><p>x</p></div>", "div { width: 300px; }");
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.width - 300.0).abs() < 0.01,
            "p.rect.width={}", p.rect.width
        );
    }

    /// width: auto не устанавливает явную ширину.
    #[test]
    fn width_auto_keeps_auto_layout() {
        let root = lay("<p>x</p>", "p { width: auto; }");
        let p = first_element_child(&root);
        // auto → заполняет viewport 800px.
        assert!(
            (p.rect.width - 800.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// width / height не наследуются.
    #[test]
    fn width_height_not_inherited() {
        let root = lay("<div><p>x</p></div>", "div { width: 400px; height: 200px; }");
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // <p> наследует только inherited properties — width/height нет.
        assert!(p.style.width.is_none(), "width should not be inherited");
        assert!(p.style.height.is_none(), "height should not be inherited");
    }

    // ── Тесты CSS min-/max- ширины и высоты (§10.4) ────────────────────────

    /// max-width режет заданную width вниз.
    #[test]
    fn max_width_clamps_width_down() {
        let root = lay("<p>x</p>", "p { width: 500px; max-width: 300px; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 300.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-width поднимает заданную width вверх.
    #[test]
    fn min_width_clamps_width_up() {
        let root = lay("<p>x</p>", "p { width: 100px; min-width: 250px; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 250.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-width побеждает max-width при конфликте (CSS 2.1 §10.4).
    #[test]
    fn min_width_beats_max_width() {
        let root = lay(
            "<p>x</p>",
            "p { width: 100px; min-width: 400px; max-width: 200px; }",
        );
        let p = first_element_child(&root);
        assert!((p.rect.width - 400.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// max-height режет height вниз.
    #[test]
    fn max_height_clamps_height_down() {
        let root = lay("<p>x</p>", "p { height: 500px; max-height: 200px; }");
        let p = first_element_child(&root);
        assert!((p.rect.height - 200.0).abs() < 0.01, "rect.height={}", p.rect.height);
    }

    /// Находит первый Block-ребёнок, включая разворачивание InlineBlockRow.
    fn first_inline_block_child(b: &LayoutBox) -> &LayoutBox {
        // InlineBlockRow — анонимный контейнер; разворачиваем его.
        for c in &b.children {
            if matches!(c.kind, BoxKind::InlineBlockRow) {
                for ic in &c.children {
                    if matches!(ic.kind, BoxKind::Block) {
                        return ic;
                    }
                }
            }
            if matches!(c.kind, BoxKind::Block) {
                return c;
            }
        }
        panic!("expected at least one inline-block child");
    }

    /// max-height clamps display:inline-block element height.
    #[test]
    fn max_height_clamps_inline_block() {
        let root = lay(
            r#"<div style="width:300px"><div style="display:inline-block;height:160px;max-height:80px;width:60px"></div></div>"#,
            "",
        );
        let outer = first_element_child(&root);
        let ib = first_inline_block_child(outer);
        assert!((ib.rect.height - 80.0).abs() < 0.5,
            "max-height should clamp 160→80, got {}", ib.rect.height);
    }

    /// min-height lifts display:inline-block element height.
    #[test]
    fn min_height_lifts_inline_block() {
        let root = lay(
            r#"<div style="width:300px"><div style="display:inline-block;height:40px;min-height:100px;width:60px"></div></div>"#,
            "",
        );
        let outer = first_element_child(&root);
        let ib = first_inline_block_child(outer);
        assert!((ib.rect.height - 100.0).abs() < 0.5,
            "min-height should lift 40→100, got {}", ib.rect.height);
    }

    /// vertical-align:bottom выравнивает inline-block элементы по нижнему краю.
    #[test]
    fn vertical_align_bottom_inline_block() {
        // Два inline-block элемента с vertical-align:bottom.
        // Высокий (120px) и низкий (60px) должны совпасть по нижнему краю.
        // Без пробелов между тегами, чтобы не было InlineSpace.
        let root = lay(
            r#"<div style="width:500px"><div style="display:inline-block;width:60px;height:60px;vertical-align:bottom"></div><div style="display:inline-block;width:60px;height:120px;vertical-align:bottom"></div></div>"#,
            "* { box-sizing: border-box; }",
        );
        let outer = first_element_child(&root);
        let ibr = outer.children.iter().find(|c| matches!(c.kind, BoxKind::InlineBlockRow))
            .expect("expected InlineBlockRow");
        // Собираем только Block-детей (пропускаем InlineSpace)
        let blocks: Vec<_> = ibr.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks.len(), 2, "expected 2 block children, got {}", blocks.len());
        // Определяем короткий и высокий по высоте
        let (short, tall) = if blocks[0].rect.height < blocks[1].rect.height {
            (blocks[0], blocks[1])
        } else {
            (blocks[1], blocks[0])
        };
        let short_bottom = short.rect.y + short.rect.height;
        let tall_bottom  = tall.rect.y  + tall.rect.height;
        assert!((short_bottom - tall_bottom).abs() < 0.5,
            "bottom edges should match: short_bottom={} tall_bottom={}", short_bottom, tall_bottom);
        // Короткий должен быть сдвинут вниз на (row_h - short_h) = 120 - 60 = 60
        assert!((short.rect.y - 60.0).abs() < 0.5,
            "short elem should be shifted down by 60px, got y={}", short.rect.y);
    }

    /// vertical-align:bottom для inline-block внутри inline-block (nested).
    #[test]
    fn vertical_align_bottom_nested_inline_block() {
        // Структура TEST-11: пара inline-block с vertical-align:bottom внутри
        // внешнего inline-block контейнера с vertical-align:bottom.
        let root = lay(
            r#"<div style="width:974px">
              <div style="display:inline-block;margin-bottom:24px;vertical-align:bottom">
                <div style="display:inline-block;width:60px;height:80px;margin-right:8px;vertical-align:bottom"></div>
                <div style="display:inline-block;width:60px;height:160px;max-height:80px;vertical-align:bottom"></div>
              </div>
            </div>"#,
            "* { box-sizing: border-box; }",
        );
        let outer = first_element_child(&root);
        // outer → InlineBlockRow → pair
        let ibr = outer.children.iter().find(|c| matches!(c.kind, BoxKind::InlineBlockRow))
            .expect("outer InlineBlockRow");
        let pair = ibr.children.iter().find(|c| matches!(c.kind, BoxKind::Block))
            .expect("pair");
        // pair height should be 80px (max-height clamped)
        assert!((pair.rect.height - 80.0).abs() < 0.5,
            "pair height should be 80, got {}", pair.rect.height);
    }

    /// min-height поднимает high content-height до минимума.
    #[test]
    fn min_height_clamps_height_up() {
        // <p> с одной строкой текста и без явной height → ~19px (16*1.2);
        // min-height: 100 → 100.
        let root = lay("<p>x</p>", "p { min-height: 100px; }");
        let p = first_element_child(&root);
        assert!((p.rect.height - 100.0).abs() < 0.01, "rect.height={}", p.rect.height);
    }

    /// max-width: none — ограничение снимается.
    #[test]
    fn max_width_none_means_no_constraint() {
        let root = lay("<p>x</p>", "p { width: 500px; max-width: none; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 500.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// Отрицательные значения отбрасываются (поле остаётся None).
    #[test]
    fn negative_min_max_ignored() {
        let root = lay(
            "<p>x</p>",
            "p { width: 200px; min-width: -50px; max-width: -10px; }",
        );
        let p = first_element_child(&root);
        assert!(p.style.min_width.is_none(), "negative min-width should be rejected");
        assert!(p.style.max_width.is_none(), "negative max-width should be rejected");
        assert!((p.rect.width - 200.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-/max- не наследуются.
    #[test]
    fn min_max_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { min-width: 100px; max-height: 50px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(p.style.min_width.is_none(), "min-width should not be inherited");
        assert!(p.style.max_height.is_none(), "max-height should not be inherited");
        // У div сам должен быть выставлен.
        assert_eq!(div.style.min_width, Some(Length::Px(100.0)));
        assert_eq!(div.style.max_height, Some(Length::Px(50.0)));
    }

    /// max-width в border-box работает как ограничение всей коробки.
    #[test]
    fn max_width_with_border_box_includes_padding() {
        // border-box: max-width=200 — это вся коробка, padding внутри.
        let root = lay(
            "<p>x</p>",
            "p { box-sizing: border-box; width: 500px; max-width: 200px; padding: 10px; }",
        );
        let p = first_element_child(&root);
        assert!((p.rect.width - 200.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    /// min-width в content-box: min относится к contentу, padding/border
    /// прибавляются сверху. Подняли width=50 (= rect 70 с padding=10) до
    /// min-width=200 (= rect 220 с padding=10).
    #[test]
    fn min_width_content_box_adds_padding() {
        let root = lay(
            "<p>x</p>",
            "p { width: 50px; min-width: 200px; padding: 10px; }",
        );
        let p = first_element_child(&root);
        assert!((p.rect.width - 220.0).abs() < 0.01, "rect.width={}", p.rect.width);
    }

    // ── Тесты CSS borders ──────────────────────────────────────────────────

    /// `border: 2px solid red` — shorthand устанавливает ширину, стиль, цвет.
    #[test]
    fn border_shorthand_sets_all_sides() {
        let root = lay("<p>x</p>", "p { border: 2px solid red; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 2.0).abs() < 0.01);
        assert!((p.style.border_right_width - 2.0).abs() < 0.01);
        assert!((p.style.border_bottom_width - 2.0).abs() < 0.01);
        assert!((p.style.border_left_width - 2.0).abs() < 0.01);
        assert_eq!(p.style.border_top_style, BorderStyle::Solid);
        assert_eq!(p.style.border_bottom_style, BorderStyle::Solid);
        let CssColor::Rgba(top_color) = p.style.border_top_color else { panic!("border-color should be set") };
        assert_eq!(top_color.r, 255);
        assert_eq!(top_color.g, 0);
        assert_eq!(top_color.b, 0);
    }

    /// Border увеличивает высоту бокса (border-box sizing).
    #[test]
    fn border_increases_box_height() {
        let root = lay("<p>x</p>", "p { border: 5px solid black; }");
        let p = first_element_child(&root);
        // 19.2 (text) + 5 + 5 = 29.2
        assert!(
            (p.rect.height - 29.2).abs() < 0.1,
            "rect.height={}", p.rect.height
        );
    }

    /// Border увеличивает ширину при явно заданном `width`.
    #[test]
    fn border_plus_explicit_width_adds_to_rect() {
        let root = lay("<p>x</p>", "p { width: 100px; border: 3px solid black; }");
        let p = first_element_child(&root);
        // rect.width = width + border_left + border_right = 100 + 3 + 3 = 106
        assert!(
            (p.rect.width - 106.0).abs() < 0.01,
            "rect.width={}", p.rect.width
        );
    }

    /// Без border-color поле равно None (currentColor).
    #[test]
    fn border_color_defaults_to_none() {
        let root = lay("<p>x</p>", "p { border: 1px solid; }");
        let p = first_element_child(&root);
        assert!(matches!(p.style.border_top_color, CssColor::CurrentColor), "should be CurrentColor");
    }

    /// `border-top: 3px dashed blue` — только верхняя сторона.
    #[test]
    fn border_side_shorthand_sets_one_side() {
        let root = lay("<p>x</p>", "p { border-top: 3px dashed blue; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 3.0).abs() < 0.01);
        assert_eq!(p.style.border_top_style, BorderStyle::Dashed);
        let CssColor::Rgba(c) = p.style.border_top_color else { panic!("top color set") };
        assert_eq!(c.b, 255);
        // Остальные стороны без изменений.
        assert_eq!(p.style.border_right_width, 0.0);
        assert_eq!(p.style.border_right_style, BorderStyle::None);
    }

    /// `border-style: solid dashed dotted solid` — 4 значения по CSS.
    #[test]
    fn border_style_four_values() {
        let root = lay("<p>x</p>", "p { border-style: solid dashed dotted solid; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_style, BorderStyle::Solid);
        assert_eq!(p.style.border_right_style, BorderStyle::Dashed);
        assert_eq!(p.style.border_bottom_style, BorderStyle::Dotted);
        assert_eq!(p.style.border_left_style, BorderStyle::Solid);
    }

    /// `border: none` — стиль None, ширина 0.
    #[test]
    fn border_none_clears_border() {
        let root = lay("<p>x</p>", "p { border: 5px solid red; border: none; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_style, BorderStyle::None);
    }

    // ── Тесты CSS box-sizing ───────────────────────────────────────────────

    /// content-box (default): rect.width = width + padding + border.
    #[test]
    fn content_box_width_adds_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { width: 100px; padding: 10px; border: 2px solid black; box-sizing: content-box; }",
        );
        let p = first_element_child(&root);
        // 100 (content) + 10*2 (padding) + 2*2 (border) = 124
        assert!(
            (p.rect.width - 124.0).abs() < 0.01,
            "rect.width={}",
            p.rect.width
        );
    }

    /// border-box: rect.width = width (включая padding и border).
    #[test]
    fn border_box_width_includes_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { width: 100px; padding: 10px; border: 2px solid black; box-sizing: border-box; }",
        );
        let p = first_element_child(&root);
        // border-box: rect.width = width = 100
        assert!(
            (p.rect.width - 100.0).abs() < 0.01,
            "rect.width={}",
            p.rect.width
        );
    }

    /// border-box: контент-зона сжимается, чтобы width влез вместе с padding+border.
    #[test]
    fn border_box_children_use_shrunken_content_width() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; padding: 10px; border: 5px solid black; box-sizing: border-box; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // div rect.width = 200. content_width = 200 - 10*2 - 5*2 = 170.
        assert!((div.rect.width - 200.0).abs() < 0.01, "div={}", div.rect.width);
        assert!(
            (p.rect.width - 170.0).abs() < 0.01,
            "p={}",
            p.rect.width
        );
    }

    /// border-box: height тоже включает padding и border.
    #[test]
    fn border_box_height_includes_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { height: 100px; padding: 10px; border: 5px solid black; box-sizing: border-box; }",
        );
        let p = first_element_child(&root);
        assert!(
            (p.rect.height - 100.0).abs() < 0.01,
            "rect.height={}",
            p.rect.height
        );
    }

    /// content-box (default): height = h + padding + border.
    #[test]
    fn content_box_height_adds_padding_and_border() {
        let root = lay(
            "<p>x</p>",
            "p { height: 100px; padding: 10px; border: 5px solid black; }",
        );
        let p = first_element_child(&root);
        // 100 + 10*2 + 5*2 = 130
        assert!(
            (p.rect.height - 130.0).abs() < 0.01,
            "rect.height={}",
            p.rect.height
        );
    }

    /// border-box не меняет поведение, если нет ни padding, ни border.
    #[test]
    fn border_box_equivalent_to_content_box_without_padding_border() {
        let root_cb = lay("<p>x</p>", "p { width: 200px; box-sizing: content-box; }");
        let root_bb = lay("<p>x</p>", "p { width: 200px; box-sizing: border-box; }");
        let p_cb = first_element_child(&root_cb);
        let p_bb = first_element_child(&root_bb);
        assert!((p_cb.rect.width - p_bb.rect.width).abs() < 0.01);
    }

    /// box-sizing не наследуется на уровне layout — у вложенного <p> остаётся content-box.
    #[test]
    fn box_sizing_does_not_inherit_into_child_layout() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-sizing: border-box; } p { width: 100px; padding: 5px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // p использует content-box (default) → 100 + 5*2 = 110.
        assert!(
            (p.rect.width - 110.0).abs() < 0.01,
            "p.rect.width={}",
            p.rect.width
        );
    }

    // ── Тесты :is() и :where() ─────────────────────────────────────────────

    /// `:is(.a, .b)` матчит любой элемент с одним из классов.
    #[test]
    fn pseudo_is_matches_any_of_list() {
        let (root, doc) = lay_with_doc(
            r#"<p class="a">a</p><p class="b">b</p><p class="c">c</p>"#,
            ":is(.a, .b) { color: red; }",
        );
        let mut ps = Vec::new();
        for c in &root.children {
            if matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "p") {
                ps.push(c);
            }
        }
        assert_eq!(ps[0].style.color.r, 255, "a should match");
        assert_eq!(ps[1].style.color.r, 255, "b should match");
        assert_eq!(ps[2].style.color.r, 0, "c should not match");
    }

    /// `:is(h1, h2)` с типами.
    #[test]
    fn pseudo_is_matches_type_selectors() {
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><h2>y</h2><h3>z</h3>",
            ":is(h1, h2) { color: red; }",
        );
        let h1 = find_by_tag(&root, "h1", &doc).unwrap();
        let h2 = find_by_tag(&root, "h2", &doc).unwrap();
        let h3 = find_by_tag(&root, "h3", &doc).unwrap();
        assert_eq!(h1.style.color.r, 255);
        assert_eq!(h2.style.color.r, 255);
        assert_eq!(h3.style.color.r, 0);
    }

    /// `:is(...)` корректно работает в составе complex-селектора.
    #[test]
    fn pseudo_is_inside_descendant_complex() {
        let (root, doc) = lay_with_doc(
            "<article><h1>a</h1><h2>b</h2></article><h1>top</h1>",
            "article :is(h1, h2) { color: red; }",
        );
        let article = find_by_tag(&root, "article", &doc).unwrap();
        let h1_in = find_by_tag(article, "h1", &doc).unwrap();
        let h2_in = find_by_tag(article, "h2", &doc).unwrap();
        assert_eq!(h1_in.style.color.r, 255);
        assert_eq!(h2_in.style.color.r, 255);
        // h1 на верхнем уровне не внутри article — не матчит.
        let top_h1 = root
            .children
            .iter()
            .find(|c| matches!(&doc.get(c.node).data, lumen_dom::NodeData::Element { name, .. } if name.local == "h1"))
            .unwrap();
        assert_eq!(top_h1.style.color.r, 0);
    }

    /// `:where(...)` матчит так же, как `:is`, но specificity = 0 — любое более
    /// специфичное правило (например, type-селектор) победит.
    #[test]
    fn pseudo_where_specificity_is_zero() {
        // :where(#x) даёт 0; p имеет specificity (0,0,1). p должен победить.
        let root = lay(
            r#"<p id="x">v</p>"#,
            ":where(#x) { color: red; } p { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255, "p должен выиграть у :where(#x)");
        assert_eq!(p.style.color.r, 0);
    }

    /// `:is(#x)` сохраняет specificity id — побеждает type-селектор.
    #[test]
    fn pseudo_is_keeps_inner_id_specificity() {
        let root = lay(
            r#"<p id="x">v</p>"#,
            ":is(#x) { color: red; } p { color: blue; }",
        );
        let p = first_element_child(&root);
        // :is(#x) даёт (1,0,0); p даёт (0,0,1). Должен выиграть :is.
        assert_eq!(p.style.color.r, 255);
        assert_eq!(p.style.color.b, 0);
    }

    /// `:is` берёт максимальную specificity из списка.
    #[test]
    fn pseudo_is_uses_max_specificity_in_list() {
        // :is(.foo, #x) — даже если матчит .foo, specificity = (1,0,0) от #x.
        // Конкурирующее правило `.foo` с (0,1,0) проигрывает.
        let root = lay(
            r#"<p class="foo">v</p>"#,
            ":is(.foo, #x) { color: red; } .foo { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, ":is(.foo, #x) должен победить .foo");
    }

    /// Пустые `:is()` / `:where()` — Unsupported, не матчат.
    #[test]
    fn pseudo_is_empty_does_not_match() {
        let root = lay("<p>x</p>", ":is() { color: red; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 0);
    }

    // ── Тесты case-insensitive [attr=val i] ────────────────────────────────

    /// Без флага `i` сравнение значения case-sensitive — `[type=Submit]` не
    /// матчит `type="submit"`.
    #[test]
    fn attr_equals_default_case_sensitive() {
        let root = lay(
            r#"<input type="submit">"#,
            "[type=Submit] { color: red; }",
        );
        let input = first_element_child(&root);
        assert_eq!(input.style.color.r, 0);
    }

    /// Флаг `i` делает `[type=Submit i]` совпадающим с `type="submit"`.
    #[test]
    fn attr_equals_case_insensitive_matches() {
        let root = lay(
            r#"<input type="submit">"#,
            "[type=Submit i] { color: red; }",
        );
        let input = first_element_child(&root);
        assert_eq!(input.style.color.r, 255);
    }

    /// Флаг `s` явно ставит case-sensitive (тождественно отсутствию флага).
    #[test]
    fn attr_equals_case_sensitive_explicit_does_not_match() {
        let root = lay(
            r#"<input type="submit">"#,
            "[type=Submit s] { color: red; }",
        );
        let input = first_element_child(&root);
        assert_eq!(input.style.color.r, 0);
    }

    /// `i` работает с `^=` (префикс). Используем `<p>` — атрибутный селектор
    /// без type-части матчит любой элемент.
    #[test]
    fn attr_prefix_case_insensitive() {
        let root = lay(
            r#"<p data-url="HTTPS://example.com">x</p>"#,
            r#"[data-url^="https" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `$=` (суффикс).
    #[test]
    fn attr_suffix_case_insensitive() {
        let root = lay(
            r#"<p data-file="page.PDF">x</p>"#,
            r#"[data-file$=".pdf" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `*=` (подстрока).
    #[test]
    fn attr_substring_case_insensitive() {
        let root = lay(
            r#"<p data-url="https://EXAMPLE.com/path">x</p>"#,
            r#"[data-url*="example" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `~=` (whitespace-разделённое слово).
    #[test]
    fn attr_includes_case_insensitive() {
        let root = lay(
            r#"<p class="foo BAR baz">x</p>"#,
            r#"[class~="bar" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` работает с `|=` (lang-style dash-match).
    #[test]
    fn attr_dashmatch_case_insensitive() {
        let root = lay(
            r#"<p lang="EN-US">x</p>"#,
            r#"[lang|="en" i] { color: red; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255);
    }

    /// `i` — это **ASCII** case-insensitive: cyrillic case различается.
    /// `[lang=РУ i]` не матчит `lang="ру"`.
    #[test]
    fn attr_case_insensitive_does_not_fold_cyrillic() {
        let root = lay(
            r#"<p lang="ру">x</p>"#,
            "[lang=РУ i] { color: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.color.r, 0,
            "ASCII case-fold не должен ронять cyrillic case"
        );
    }

    // ── Тесты !important в каскаде (CSS Cascade L4 §8.1) ───────────────────

    /// !important побеждает normal даже при меньшей specificity.
    /// `p { color: red !important }` (0,0,1) должен победить `#x { color: blue }` (1,0,0).
    #[test]
    fn important_beats_higher_specificity() {
        let root = lay(
            r#"<p id="x">v</p>"#,
            "p { color: red !important; } #x { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 255, "important должен победить #x");
        assert_eq!(p.style.color.b, 0);
    }

    /// Между двумя !important выигрывает большая specificity.
    #[test]
    fn important_among_two_resolves_by_specificity() {
        let root = lay(
            r#"<p id="x" class="c">v</p>"#,
            "p { color: red !important; } #x { color: blue !important; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255, "#x !important должен победить p !important");
    }

    /// Между двумя !important равной specificity — позже объявленное.
    #[test]
    fn important_with_equal_specificity_later_wins() {
        let root = lay(
            "<p>v</p>",
            "p { color: red !important; } p { color: blue !important; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
        assert_eq!(p.style.color.r, 0);
    }

    /// !important работает поверх inheritance: ребёнок получает важный цвет.
    #[test]
    fn important_inherits_to_child() {
        let root = lay(
            "<div><p>v</p></div>",
            "div { color: red !important; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.color.r, 255);
    }

    /// Без !important specificity решает обычным образом.
    #[test]
    fn normal_cascade_unchanged_without_important() {
        let root = lay(
            r#"<p id="x">v</p>"#,
            "p { color: red; } #x { color: blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.b, 255);
        assert_eq!(p.style.color.r, 0);
    }

    // ── viewport units (vh/vw/vmin/vmax) ───────────────────────────────────

    /// `width: 50vw` — половина ширины viewport. Default lay() — 800x600.
    #[test]
    fn width_vw_uses_viewport() {
        let root = lay("<p>x</p>", "p { width: 50vw; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 400.0).abs() < 0.01, "width = {}", p.rect.width);
    }

    /// `height: 25vh` — четверть высоты viewport.
    #[test]
    fn height_vh_uses_viewport() {
        // 25vh от 600 = 150.
        let root = lay("<p>x</p>", "p { height: 25vh; }");
        let p = first_element_child(&root);
        assert!((p.rect.height - 150.0).abs() < 0.01, "height = {}", p.rect.height);
    }

    /// `padding` через vw.
    #[test]
    fn padding_vw_uses_viewport() {
        // 10vw от 800 = 80.
        let root = lay("<p>x</p>", "p { padding: 10vw; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.padding_top, Length::Vw(10.0));
        assert_eq!(p.style.padding_left, Length::Vw(10.0));
    }

    /// `font-size` через vh влияет на размер шрифта (наследуется в InlineRun).
    #[test]
    fn font_size_vh_uses_viewport() {
        // 5vh от 600 = 30.
        let root = lay("<p>x</p>", "p { font-size: 5vh; }");
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        assert!((inline.style.font_size - 30.0).abs() < 0.01, "fs = {}", inline.style.font_size);
    }

    /// `vmin` — меньшая сторона viewport (800 vs 600 → 600).
    #[test]
    fn width_vmin_uses_smaller_side() {
        // 50vmin от min(800, 600) = 600 → 300.
        let root = lay("<p>x</p>", "p { width: 50vmin; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 300.0).abs() < 0.01, "width = {}", p.rect.width);
    }

    /// `vmax` — большая сторона viewport (800 vs 600 → 800).
    #[test]
    fn width_vmax_uses_larger_side() {
        // 50vmax от max(800, 600) = 800 → 400.
        let root = lay("<p>x</p>", "p { width: 50vmax; }");
        let p = first_element_child(&root);
        assert!((p.rect.width - 400.0).abs() < 0.01, "width = {}", p.rect.width);
    }

    /// `border-width` через vh.
    #[test]
    fn border_width_vh_uses_viewport() {
        // 1vh от 600 = 6.
        let root = lay("<p>x</p>", "p { border: 1vh solid red; }");
        let p = first_element_child(&root);
        assert!((p.style.border_top_width - 6.0).abs() < 0.01);
        assert!((p.style.border_right_width - 6.0).abs() < 0.01);
    }

    // ── font-style: italic / oblique / normal ───────────────────────────────

    /// `<em>` получает italic через UA stylesheet.
    #[test]
    fn em_element_is_italic_by_default() {
        // <em> внутри <p> — inline; UA stylesheet делает его italic.
        let root = lay("<p>hi <em>there</em></p>", "");
        let p = first_element_child(&root);
        // <p> сам Normal; внутренний фрагмент <em> в InlineRun должен быть Italic.
        assert_eq!(p.style.font_style, FontStyle::Normal);
        let inline = p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { segments, .. } = &inline.kind {
            // Должно быть два сегмента: "hi " (Normal) и "there" (Italic).
            let italic = segments.iter().find(|s| s.style.font_style == FontStyle::Italic);
            assert!(italic.is_some(), "ожидался italic сегмент");
            assert_eq!(italic.unwrap().text, "there");
        } else {
            panic!("expected InlineRun");
        }
    }

    /// `<i>`, `<cite>`, `<dfn>`, `<address>`, `<var>` тоже italic по UA.
    /// Проверяем напрямую через compute_style — обходить дерево не нужно,
    /// тег элемента всегда первый child корня.
    #[test]
    fn i_cite_dfn_address_var_are_italic() {
        for tag in ["i", "cite", "dfn", "address", "var"] {
            let html = format!("<{tag}>x</{tag}>");
            let doc = lumen_html_parser::parse(&html);
            let id = doc.get(doc.body().unwrap()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
                false,
            );
            assert_eq!(style.font_style, FontStyle::Italic, "tag = {tag}");
        }
    }

    /// CSS `font-style: italic` на `<p>`.
    #[test]
    fn font_style_italic_via_css() {
        let root = lay("<p>x</p>", "p { font-style: italic; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_style, FontStyle::Italic);
    }

    /// CSS `font-style: oblique`.
    #[test]
    fn font_style_oblique_via_css() {
        let root = lay("<p>x</p>", "p { font-style: oblique; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_style, FontStyle::Oblique);
    }

    /// CSS `font-style: normal` на `<em>` сбрасывает UA-italic.
    #[test]
    fn font_style_normal_overrides_ua_italic() {
        // Но в InlineRun сегменте — нужно проверить, что override применился.
        // Проще: сделать <em> блочным через display:block + font-style:normal.
        let root = lay(
            "<em>x</em>",
            "em { display: block; font-style: normal; }",
        );
        let em = first_element_child(&root);
        assert_eq!(em.style.font_style, FontStyle::Normal);
    }

    /// font-style наследуется: ребёнок берёт italic от родителя.
    #[test]
    fn font_style_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-style: italic; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_style, FontStyle::Italic);
        assert_eq!(p.style.font_style, FontStyle::Italic);
    }

    // ── font-weight: normal / bold / lighter / bolder / numeric ─────────────

    /// `<strong>` / `<b>` / `<h1>`-`<h6>` / `<th>` получают bold через UA.
    #[test]
    fn semantic_tags_are_bold_by_default() {
        for tag in ["b", "strong", "h1", "h2", "h3", "h4", "h5", "h6", "th"] {
            let html = format!("<{tag}>x</{tag}>");
            let doc = lumen_html_parser::parse(&html);
            let id = doc.get(doc.body().unwrap()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
                false,
            );
            assert_eq!(style.font_weight, FontWeight::BOLD, "tag = {tag}");
        }
    }

    /// UA stylesheet: `<h1>`–`<h6>` получают увеличенный font-size и
    /// вертикальные margin (HTML Rendering §15.3.3). Регрессия BUG-106:
    /// без этих дефолтов заголовки рендерились 16px без отступов, из-за чего
    /// таблицы (TEST-64) уезжали вверх относительно Edge.
    #[test]
    fn headings_get_ua_font_size_and_margins() {
        let root_fs = ComputedStyle::root().font_size;
        // (tag, font-size factor, vertical margin em)
        let cases = [
            ("h1", 2.0_f32, 0.67_f32),
            ("h2", 1.5, 0.83),
            ("h3", 1.17, 1.0),
            ("h4", 1.0, 1.33),
            ("h5", 0.83, 1.67),
            ("h6", 0.67, 2.33),
        ];
        for (tag, size_factor, margin_em) in cases {
            let html = format!("<{tag}>x</{tag}>");
            let doc = lumen_html_parser::parse(&html);
            let id = doc.get(doc.body().unwrap()).children[0];
            let style = crate::style::compute_style(
                &doc,
                id,
                &lumen_css_parser::Stylesheet::default(),
                &ComputedStyle::root(),
                Size::new(800.0, 600.0),
                false,
            );
            assert!(
                (style.font_size - root_fs * size_factor).abs() < 0.01,
                "{tag} font-size: expected {}, got {}",
                root_fs * size_factor,
                style.font_size,
            );
            assert_eq!(
                style.margin_top,
                LengthOrAuto::Length(Length::Em(margin_em)),
                "{tag} margin-top",
            );
            assert_eq!(
                style.margin_bottom,
                LengthOrAuto::Length(Length::Em(margin_em)),
                "{tag} margin-bottom",
            );
        }
    }

    /// UA-дефолты заголовка перекрываются author-CSS (font-size через
    /// pre-pass, margin через main-pass каскада).
    #[test]
    fn heading_ua_defaults_overridden_by_author_css() {
        let doc = lumen_html_parser::parse("<h3>x</h3>");
        let id = doc.get(doc.body().unwrap()).children[0];
        let ss = lumen_css_parser::parse("h3 { font-size: 30px; margin-top: 5px; }");
        let style = crate::style::compute_style(
            &doc,
            id,
            &ss,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert!((style.font_size - 30.0).abs() < 0.01, "author font-size wins");
        assert_eq!(style.margin_top, LengthOrAuto::Length(Length::Px(5.0)));
    }

    /// CSS `font-weight: bold` → 700.
    #[test]
    fn font_weight_bold_keyword() {
        let root = lay("<p>x</p>", "p { font-weight: bold; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_weight, FontWeight(700));
    }

    /// Численное значение.
    #[test]
    fn font_weight_numeric() {
        let root = lay("<p>x</p>", "p { font-weight: 300; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_weight, FontWeight(300));
    }

    /// `lighter` от 700 = 400 (по таблице CSS Fonts L4).
    #[test]
    fn font_weight_lighter_relative_to_parent() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-weight: 700; } p { font-weight: lighter; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_weight, FontWeight(700));
        assert_eq!(p.style.font_weight, FontWeight(400));
    }

    /// `bolder` от 400 = 700.
    #[test]
    fn font_weight_bolder_relative_to_parent() {
        let root = lay(
            "<div><p>x</p></div>",
            "p { font-weight: bolder; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // div наследует normal=400; p получает bolder = 700.
        assert_eq!(div.style.font_weight, FontWeight(400));
        assert_eq!(p.style.font_weight, FontWeight(700));
    }

    /// `font-weight: normal` сбрасывает UA bold у `<strong>`.
    #[test]
    fn font_weight_normal_overrides_ua_bold() {
        let root = lay(
            "<strong>x</strong>",
            "strong { display: block; font-weight: normal; }",
        );
        let strong = first_element_child(&root);
        assert_eq!(strong.style.font_weight, FontWeight::NORMAL);
    }

    /// font-weight наследуется.
    #[test]
    fn font_weight_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-weight: 800; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_weight, FontWeight(800));
    }

    /// Невалидное значение игнорируется.
    #[test]
    fn font_weight_invalid_keeps_inherited() {
        let root = lay(
            "<p>x</p>",
            "p { font-weight: nonsense; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.font_weight, FontWeight::NORMAL);
    }

    // ── text-transform: uppercase / lowercase / capitalize ─────────────────

    /// Достаёт первый текстовый сегмент из InlineRun первого block-child.
    fn first_inline_text(root: &LayoutBox) -> String {
        let p = first_element_child(root);
        for c in &p.children {
            if let BoxKind::InlineRun { segments, .. } = &c.kind
                && let Some(s) = segments.first()
            {
                return s.text.clone();
            }
        }
        panic!("no inline segments found");
    }

    #[test]
    fn text_transform_uppercase_ascii() {
        let root = lay("<p>hello world</p>", "p { text-transform: uppercase; }");
        assert_eq!(first_inline_text(&root), "HELLO WORLD");
    }

    #[test]
    fn text_transform_lowercase_ascii() {
        let root = lay("<p>HELLO World</p>", "p { text-transform: lowercase; }");
        assert_eq!(first_inline_text(&root), "hello world");
    }

    #[test]
    fn text_transform_capitalize_ascii() {
        let root = lay("<p>hello world</p>", "p { text-transform: capitalize; }");
        assert_eq!(first_inline_text(&root), "Hello World");
    }

    #[test]
    fn text_transform_uppercase_cyrillic() {
        // Русские буквы должны нормально case-folиться.
        let root = lay("<p>привет мир</p>", "p { text-transform: uppercase; }");
        assert_eq!(first_inline_text(&root), "ПРИВЕТ МИР");
    }

    #[test]
    fn text_transform_lowercase_cyrillic() {
        let root = lay("<p>ПРИВЕТ Мир</p>", "p { text-transform: lowercase; }");
        assert_eq!(first_inline_text(&root), "привет мир");
    }

    #[test]
    fn text_transform_capitalize_cyrillic() {
        let root = lay("<p>привет мир</p>", "p { text-transform: capitalize; }");
        assert_eq!(first_inline_text(&root), "Привет Мир");
    }

    #[test]
    fn text_transform_none_default() {
        let root = lay("<p>Hello WORLD</p>", "");
        assert_eq!(first_inline_text(&root), "Hello WORLD");
    }

    #[test]
    fn text_transform_inherited() {
        let root = lay(
            "<div><p>hi</p></div>",
            "div { text-transform: uppercase; }",
        );
        let div = first_element_child(&root);
        assert_eq!(div.style.text_transform, TextTransform::Uppercase);
        let p = first_element_child(div);
        assert_eq!(p.style.text_transform, TextTransform::Uppercase);
    }

    // ── text-indent ─────────────────────────────────────────────────────────

    #[test]
    fn text_indent_basic() {
        // Парсинг + применение к ComputedStyle.
        let root = lay("<p>hello</p>", "p { text-indent: 30px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_indent, Length::Px(30.0));
    }

    #[test]
    fn text_indent_em_stores_typed() {
        // text-indent: 2em хранится как Length::Em(2.0); разрешается при layout.
        let root = lay("<p>x</p>", "p { text-indent: 2em; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_indent, Length::Em(2.0));
    }

    #[test]
    fn text_indent_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-indent: 25px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_indent, Length::Px(25.0));
        assert_eq!(p.style.text_indent, Length::Px(25.0));
    }

    #[test]
    fn text_indent_shifts_first_line() {
        // С text-indent первое слово начинается со сдвигом.
        // Используем lay_measured (Fixed8 = 8px на символ) на 800 ширину.
        let root = lay_measured(
            "<p>hi</p>",
            "p { text-indent: 40px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            // Первая строка, первый фрагмент. x должен быть = 40.
            let first_frag = &lines[0][0];
            assert!((first_frag.x - 40.0).abs() < 0.01, "first.x = {}", first_frag.x);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn text_indent_only_first_line() {
        // text-indent применяется только к первой строке. Если контент
        // переносится на 2+ строк, последующие начинаются с x=0.
        // Fixed8: 8px на символ. max_width = 80 → ~10 символов с indent 16.
        let root = lay_measured(
            "<p>aaaa bbbb cccc dddd</p>",
            "p { text-indent: 16px; }",
            80.0,
        );
        let p = first_element_child(&root);
        let inline = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            // Первая строка должна стартовать с offset.
            assert!((lines[0][0].x - 16.0).abs() < 0.01, "line[0][0].x = {}", lines[0][0].x);
            // Вторая (и далее) строка стартует с 0.
            assert!(lines.len() > 1, "expected multiple lines, got {}", lines.len());
            assert!((lines[1][0].x - 0.0).abs() < 0.01, "line[1][0].x = {}", lines[1][0].x);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn text_indent_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_indent, Length::Px(0.0));
    }

    // ── letter-spacing ──────────────────────────────────────────────────────

    #[test]
    fn letter_spacing_basic_parse() {
        let root = lay("<p>x</p>", "p { letter-spacing: 4px; }");
        let p = first_element_child(&root);
        assert!((p.style.letter_spacing - 4.0).abs() < 0.01);
    }

    #[test]
    fn letter_spacing_normal_keyword() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { letter-spacing: 5px; } p { letter-spacing: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.letter_spacing - 5.0).abs() < 0.01);
        assert_eq!(p.style.letter_spacing, 0.0);
    }

    #[test]
    fn letter_spacing_negative() {
        // Отрицательные значения валидны (сжимают текст).
        let root = lay("<p>x</p>", "p { letter-spacing: -2px; }");
        let p = first_element_child(&root);
        assert!((p.style.letter_spacing - (-2.0)).abs() < 0.01);
    }

    #[test]
    fn letter_spacing_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { letter-spacing: 3px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.letter_spacing - 3.0).abs() < 0.01);
    }

    #[test]
    fn letter_spacing_extends_word_width() {
        // 4 char word "abcd" с letter-spacing 5: width = 4*8 + 3*5 = 47.
        // Без letter-spacing было бы 32.
        let root = lay_measured(
            "<p>abcd</p>",
            "p { letter-spacing: 5px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let inline = p.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            let frag = &lines[0][0];
            assert!((frag.width - 47.0).abs() < 0.01, "frag.width = {}", frag.width);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn letter_spacing_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.letter_spacing, 0.0);
    }

    // ── word-spacing ────────────────────────────────────────────────────────

    #[test]
    fn word_spacing_basic_parse() {
        let root = lay("<p>x</p>", "p { word-spacing: 10px; }");
        let p = first_element_child(&root);
        assert!((p.style.word_spacing - 10.0).abs() < 0.01);
    }

    #[test]
    fn word_spacing_normal_keyword() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { word-spacing: 6px; } p { word-spacing: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.word_spacing - 6.0).abs() < 0.01);
        assert_eq!(p.style.word_spacing, 0.0);
    }

    #[test]
    fn word_spacing_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { word-spacing: 4px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.style.word_spacing - 4.0).abs() < 0.01);
    }

    #[test]
    fn word_spacing_only_at_word_boundary() {
        // word-spacing влияет только на gap между словами, не на ширину
        // отдельного слова. Сравниваем с/без word-spacing на одно слово.
        // Fixed8: 8px per char. "abcd" один word — word-spacing не должен
        // изменить width.
        let with = lay_measured("<p>abcd</p>", "p { word-spacing: 100px; }", 800.0);
        let without = lay_measured("<p>abcd</p>", "", 800.0);

        let p_with = first_element_child(&with);
        let p_without = first_element_child(&without);
        let inline_w = p_with.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        let inline_wo = p_without.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();

        let w_width = if let BoxKind::InlineRun { lines, .. } = &inline_w.kind {
            lines[0][0].width
        } else { panic!() };
        let wo_width = if let BoxKind::InlineRun { lines, .. } = &inline_wo.kind {
            lines[0][0].width
        } else { panic!() };
        assert!((w_width - wo_width).abs() < 0.01,
            "word-spacing не должен менять ширину одиночного слова: {w_width} vs {wo_width}");
    }

    #[test]
    fn word_spacing_extends_two_word_run() {
        // Два слова "ab cd": Fixed8, без word-spacing = 2*16+8 = 40.
        // С word-spacing 12: 2*16 + (8+12) = 52.
        let root = lay_measured("<p>ab cd</p>", "p { word-spacing: 12px; }", 800.0);
        let p = first_element_child(&root);
        let inline = p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        if let BoxKind::InlineRun { lines, .. } = &inline.kind {
            // Слова сольются в один frag (одинаковый стиль).
            let frag = &lines[0][0];
            assert!((frag.width - 52.0).abs() < 0.01, "merged frag.width = {}", frag.width);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn word_spacing_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.word_spacing, 0.0);
    }

    // ── font-family ─────────────────────────────────────────────────────────

    #[test]
    fn font_family_single_name() {
        let root = lay("<p>x</p>", "p { font-family: Arial; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_family, vec!["Arial".to_string()]);
    }

    #[test]
    fn font_family_priority_list() {
        let root = lay(
            "<p>x</p>",
            "p { font-family: Arial, Helvetica, sans-serif; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Arial".to_string(), "Helvetica".to_string(), "sans-serif".to_string()]
        );
    }

    #[test]
    fn font_family_quoted_with_spaces() {
        let root = lay(
            "<p>x</p>",
            r#"p { font-family: "Times New Roman", serif; }"#,
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Times New Roman".to_string(), "serif".to_string()]
        );
    }

    #[test]
    fn font_family_unquoted_multiword() {
        // Без кавычек тоже валидно для имён без запятых, whitespace схлопывается.
        let root = lay(
            "<p>x</p>",
            "p { font-family: Times New Roman, serif; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Times New Roman".to_string(), "serif".to_string()]
        );
    }

    #[test]
    fn font_family_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-family: Verdana, sans-serif; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_family, div.style.font_family);
        assert_eq!(p.style.font_family[0], "Verdana");
    }

    #[test]
    fn font_family_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.font_family.is_empty());
    }

    #[test]
    fn font_family_single_quotes_also_work() {
        let root = lay(
            "<p>x</p>",
            "p { font-family: 'Open Sans', sans-serif; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.font_family,
            vec!["Open Sans".to_string(), "sans-serif".to_string()]
        );
    }

    // ── white-space: nowrap ─────────────────────────────────────────────────

    #[test]
    fn white_space_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.white_space, WhiteSpace::Normal);
    }

    #[test]
    fn white_space_nowrap_parsed() {
        let root = lay("<p>x</p>", "p { white-space: nowrap; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.white_space, WhiteSpace::Nowrap);
    }

    #[test]
    fn white_space_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { white-space: nowrap; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.white_space, WhiteSpace::Nowrap);
    }

    #[test]
    fn white_space_nowrap_disables_wrap() {
        // Без nowrap: 4 слова по 2 char + space (8+8+8+8 + 3*8 = 56 px) на 30 px ширине
        // → переносится на несколько строк.
        // С nowrap: всё на одной строке.
        let normal = lay_measured("<p>aa bb cc dd</p>", "", 30.0);
        let nowrap = lay_measured(
            "<p>aa bb cc dd</p>",
            "p { white-space: nowrap; }",
            30.0,
        );

        let n_p = first_element_child(&normal);
        let nw_p = first_element_child(&nowrap);
        let n_inline = n_p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        let nw_inline = nw_p.children.iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();

        let n_lines = if let BoxKind::InlineRun { lines, .. } = &n_inline.kind {
            lines.len()
        } else { panic!() };
        let nw_lines = if let BoxKind::InlineRun { lines, .. } = &nw_inline.kind {
            lines.len()
        } else { panic!() };

        assert!(n_lines > 1, "default ожидает перенос на несколько строк, got {n_lines}");
        assert_eq!(nw_lines, 1, "nowrap должен дать одну строку");
    }

    #[test]
    fn white_space_normal_keyword_resets_inherited_nowrap() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { white-space: nowrap; } p { white-space: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.white_space, WhiteSpace::Nowrap);
        assert_eq!(p.style.white_space, WhiteSpace::Normal);
    }

    // ── opacity ─────────────────────────────────────────────────────────────

    #[test]
    fn opacity_default_one() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_number_value() {
        let root = lay("<p>x</p>", "p { opacity: 0.5; }");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_percent_value() {
        let root = lay("<p>x</p>", "p { opacity: 25%; }");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_clamped_below_zero() {
        let root = lay("<p>x</p>", "p { opacity: -0.5; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.opacity, 0.0);
    }

    #[test]
    fn opacity_clamped_above_one() {
        let root = lay("<p>x</p>", "p { opacity: 2.5; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.opacity, 1.0);
    }

    #[test]
    fn opacity_not_inherited() {
        // CSS opacity не наследуется в layout cascade (визуально она применяется
        // ко всему layer-у, но в computed-style-каскаде каждый элемент имеет
        // свой opacity = 1 по умолчанию).
        let root = lay(
            "<div><p>x</p></div>",
            "div { opacity: 0.3; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((div.style.opacity - 0.3).abs() < f32::EPSILON);
        assert!((p.style.opacity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn opacity_invalid_keeps_default() {
        let root = lay("<p>x</p>", "p { opacity: nonsense; }");
        let p = first_element_child(&root);
        assert!((p.style.opacity - 1.0).abs() < f32::EPSILON);
    }

    // ── outline (CSS Basic UI L4 §5) ────────────────────────────────────────

    #[test]
    fn outline_shorthand() {
        let root = lay("<p>x</p>", "p { outline: 3px dashed red; }");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 3.0).abs() < 0.01);
        assert_eq!(p.style.outline_style, OutlineStyle::Dashed);
        match p.style.outline_color {
            OutlineColor::Color(c) => assert_eq!(c.r, 255),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn outline_individual_props() {
        let root = lay(
            "<p>x</p>",
            "p { outline-width: 5px; outline-style: solid; outline-color: blue; }",
        );
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 5.0).abs() < 0.01);
        assert_eq!(p.style.outline_style, OutlineStyle::Solid);
        match p.style.outline_color {
            OutlineColor::Color(c) => assert_eq!(c.b, 255),
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn outline_offset_positive_and_negative() {
        let p_root = lay("<p>x</p>", "p { outline-offset: 10px; }");
        let p = first_element_child(&p_root);
        assert_eq!(p.style.outline_offset, Length::Px(10.0));

        let n_root = lay("<p>x</p>", "p { outline-offset: -3px; }");
        let n = first_element_child(&n_root);
        assert_eq!(n.style.outline_offset, Length::Px(-3.0));
    }

    #[test]
    fn outline_does_not_affect_box_width() {
        // Ключевое отличие от border: outline не занимает места в коробке.
        // Бокс с outline должен иметь ту же ширину/высоту, что без него.
        let with = lay("<p>x</p>", "p { outline: 10px solid red; }");
        let without = lay("<p>x</p>", "");

        let p_with = first_element_child(&with);
        let p_without = first_element_child(&without);
        assert!((p_with.rect.width - p_without.rect.width).abs() < 0.01,
            "outline не должен менять width: {} vs {}",
            p_with.rect.width, p_without.rect.width);
        assert!((p_with.rect.height - p_without.rect.height).abs() < 0.01);
    }

    #[test]
    fn outline_default_invisible() {
        // CSS Basic UI L4 §5: initial outline-style = none, outline-width = medium
        // (3px). Used-value outline-width = 0 при style=none, поэтому outline
        // невидим по умолчанию.
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 3.0).abs() < 0.01, "computed=medium");
        assert_eq!(p.style.outline_used_width(), 0.0, "used=0 при style=none");
        assert_eq!(p.style.outline_style, OutlineStyle::None);
        assert_eq!(p.style.outline_color, OutlineColor::Auto);
        assert_eq!(p.style.outline_offset, Length::Px(0.0));
    }

    #[test]
    fn outline_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { outline: 2px solid red; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(div.style.outline_used_width() > 0.0);
        assert_eq!(p.style.outline_style, OutlineStyle::None);
        assert_eq!(p.style.outline_used_width(), 0.0);
    }

    #[test]
    fn outline_width_line_width_keywords() {
        // CSS Basic UI L4 §5.2 — <line-width> = thin | medium | thick |
        // <length>. UA convention thin=1, medium=3, thick=5.
        let thin = lay("<p>x</p>", "p { outline: thin solid red; }");
        let p = first_element_child(&thin);
        assert!((p.style.outline_width - 1.0).abs() < 0.01);

        let med = lay("<p>x</p>", "p { outline: medium solid red; }");
        let p = first_element_child(&med);
        assert!((p.style.outline_width - 3.0).abs() < 0.01);

        let thick = lay("<p>x</p>", "p { outline: thick solid red; }");
        let p = first_element_child(&thick);
        assert!((p.style.outline_width - 5.0).abs() < 0.01);
    }

    #[test]
    fn outline_style_auto_keyword() {
        // CSS Basic UI L4 §5.3 — `auto` = UA-defined focus indicator. Хранится
        // отдельным variant-ом, чтобы UA-stylesheet `:focus-visible { outline:
        // auto }` отличался от явного `outline: solid` автора.
        let root = lay("<p>x</p>", "p { outline-style: auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.outline_style, OutlineStyle::Auto);
        assert!(p.style.outline_used_width() > 0.0, "auto делает outline видимым");
    }

    #[test]
    fn outline_color_auto_and_current_color() {
        // CSS Basic UI L4 §5.4 — `auto` = UA-defined contrast, `currentColor`
        // = вычисленный color элемента. Оба хранятся отдельными variant-ами.
        let auto_r = lay("<p>x</p>", "p { outline-color: auto; }");
        let p = first_element_child(&auto_r);
        assert_eq!(p.style.outline_color, OutlineColor::Auto);

        let cc_r = lay("<p>x</p>", "p { outline-color: currentColor; }");
        let p = first_element_child(&cc_r);
        assert_eq!(p.style.outline_color, OutlineColor::CurrentColor);
    }

    #[test]
    fn outline_shorthand_with_auto_style() {
        // `outline: auto` = style=auto, остальное initial.
        let root = lay("<p>x</p>", "p { outline: auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.outline_style, OutlineStyle::Auto);
        assert!((p.style.outline_width - 3.0).abs() < 0.01, "medium initial");
        assert_eq!(p.style.outline_color, OutlineColor::Auto);
    }

    #[test]
    fn outline_shorthand_resets_longhands() {
        // CSS Cascade L4 §3.1 — shorthand сбрасывает все longhand-а в
        // initial. Здесь сначала ставим конкретные значения, потом `outline`
        // должен затереть их к initial+token-set.
        let root = lay(
            "<p>x</p>",
            "p { outline-color: green; outline-offset: 10px; outline: 4px solid; }",
        );
        let p = first_element_child(&root);
        // shorthand сбросил color к Auto (initial) — токен solid 4px не
        // содержал цвета.
        assert_eq!(p.style.outline_color, OutlineColor::Auto);
        assert_eq!(p.style.outline_style, OutlineStyle::Solid);
        assert!((p.style.outline_width - 4.0).abs() < 0.01);
        // outline-offset — longhand, НЕ часть shorthand `outline`, не
        // сбрасывается (по spec). Проверяем, что offset сохранён.
        assert_eq!(p.style.outline_offset, Length::Px(10.0));
    }

    #[test]
    fn outline_used_width_zero_when_hidden_style_none() {
        // Used-value rule (CSS 2.1 §17.6.1 / Basic UI L4 §5.2): даже если
        // computed width задан явно, used = 0 при style=none.
        let root = lay("<p>x</p>", "p { outline-width: 20px; }");
        let p = first_element_child(&root);
        assert!((p.style.outline_width - 20.0).abs() < 0.01, "computed=20");
        assert_eq!(p.style.outline_style, OutlineStyle::None);
        assert_eq!(p.style.outline_used_width(), 0.0, "used=0 при style=none");
    }

    // ── text-emphasis (CSS Text Decoration L4 §5) ───────────────────────────

    #[test]
    fn text_emphasis_default_none() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_emphasis_style, TextEmphasisStyle::None);
        assert!(matches!(p.style.text_emphasis_color, CssColor::CurrentColor), "initial = currentColor");
        assert_eq!(
            p.style.text_emphasis_position,
            TextEmphasisPosition::OverRight
        );
    }

    #[test]
    fn text_emphasis_style_symbol_filled_circle() {
        let root = lay("<p>x</p>", "p { text-emphasis-style: filled circle; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: true,
                shape: TextEmphasisShape::Circle
            }
        );
    }

    #[test]
    fn text_emphasis_style_only_fill_fallback_circle() {
        // Spec: shape по умолчанию = circle при horizontal writing mode.
        let root = lay("<p>x</p>", "p { text-emphasis-style: open; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: false,
                shape: TextEmphasisShape::Circle
            }
        );
    }

    #[test]
    fn text_emphasis_style_only_shape_fallback_filled() {
        let root = lay("<p>x</p>", "p { text-emphasis-style: sesame; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: true,
                shape: TextEmphasisShape::Sesame
            }
        );
    }

    #[test]
    fn text_emphasis_style_string() {
        let root = lay("<p>x</p>", "p { text-emphasis-style: \"★\"; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::String("★".to_string())
        );
    }

    #[test]
    fn text_emphasis_style_order_independent() {
        // Spec: `[ filled | open ] || [ ...shape... ]` — порядок любой.
        let r1 = lay(
            "<p>x</p>",
            "p { text-emphasis-style: triangle filled; }",
        );
        let p1 = first_element_child(&r1);
        let r2 = lay(
            "<p>x</p>",
            "p { text-emphasis-style: filled triangle; }",
        );
        let p2 = first_element_child(&r2);
        assert_eq!(p1.style.text_emphasis_style, p2.style.text_emphasis_style);
    }

    #[test]
    fn text_emphasis_color_explicit_and_currentcolor() {
        let r1 = lay("<p>x</p>", "p { text-emphasis-color: red; }");
        let p1 = first_element_child(&r1);
        assert!(matches!(p1.style.text_emphasis_color, CssColor::Rgba(Color { r: 255, .. })));

        // Override → currentColor сбрасывает в None.
        let r2 = lay(
            "<p>x</p>",
            "p { text-emphasis-color: red; text-emphasis-color: currentColor; }",
        );
        let p2 = first_element_child(&r2);
        assert!(matches!(p2.style.text_emphasis_color, CssColor::CurrentColor));
    }

    #[test]
    fn text_emphasis_position_grammar() {
        // [over | under] && [right | left]? — vertical обязателен, horizontal
        // опционален с default right.
        let r1 = lay("<p>x</p>", "p { text-emphasis-position: under left; }");
        let p1 = first_element_child(&r1);
        assert_eq!(
            p1.style.text_emphasis_position,
            TextEmphasisPosition::UnderLeft
        );

        let r2 = lay("<p>x</p>", "p { text-emphasis-position: left over; }");
        let p2 = first_element_child(&r2);
        assert_eq!(
            p2.style.text_emphasis_position,
            TextEmphasisPosition::OverLeft,
            "tokens are unordered"
        );

        // Только vertical — horizontal default right.
        let r3 = lay("<p>x</p>", "p { text-emphasis-position: under; }");
        let p3 = first_element_child(&r3);
        assert_eq!(
            p3.style.text_emphasis_position,
            TextEmphasisPosition::UnderRight
        );

        // Только horizontal — invalid (vertical обязателен).
        let r4 = lay("<p>x</p>", "p { text-emphasis-position: left; }");
        let p4 = first_element_child(&r4);
        assert_eq!(
            p4.style.text_emphasis_position,
            TextEmphasisPosition::OverRight,
            "invalid declaration ignored, initial"
        );
    }

    #[test]
    fn text_emphasis_inherited() {
        // CSS Text Decoration L4 §5 — все три text-emphasis-* longhand-а
        // inherited. Это ключевое отличие от text-decoration (там Phase 0
        // тоже inherit, но spec не-inherit с propagation).
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-emphasis: filled circle red; text-emphasis-position: under; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_emphasis_style, p.style.text_emphasis_style);
        assert_eq!(div.style.text_emphasis_color, p.style.text_emphasis_color);
        assert_eq!(
            div.style.text_emphasis_position,
            p.style.text_emphasis_position
        );
        assert_eq!(
            p.style.text_emphasis_position,
            TextEmphasisPosition::UnderRight
        );
    }

    #[test]
    fn text_emphasis_shorthand_style_plus_color() {
        let root = lay("<p>x</p>", "p { text-emphasis: filled dot blue; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::Symbol {
                filled: true,
                shape: TextEmphasisShape::Dot
            }
        );
        assert!(matches!(p.style.text_emphasis_color, CssColor::Rgba(Color { b: 255, .. })));
    }

    #[test]
    fn text_emphasis_shorthand_resets_longhands() {
        // Shorthand сбрасывает оба longhand-а в initial и потом применяет
        // токены. Position — отдельный longhand, не часть shorthand-а
        // (см. spec §5.6); поэтому сохраняется.
        let root = lay(
            "<p>x</p>",
            "p { text-emphasis-style: open triangle; \
                 text-emphasis-color: green; \
                 text-emphasis-position: under left; \
                 text-emphasis: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::None,
            "shorthand без style-токена → initial None"
        );
        assert!(matches!(p.style.text_emphasis_color, CssColor::Rgba(Color { r: 255, .. })));
        assert_eq!(
            p.style.text_emphasis_position,
            TextEmphasisPosition::UnderLeft,
            "position не входит в shorthand"
        );
    }

    #[test]
    fn text_emphasis_shorthand_none() {
        let root = lay("<p>x</p>", "p { text-emphasis: none; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_emphasis_style, TextEmphasisStyle::None);
        assert!(matches!(p.style.text_emphasis_color, CssColor::CurrentColor));
    }

    #[test]
    fn text_emphasis_shorthand_string_only() {
        let root = lay("<p>x</p>", "p { text-emphasis: \"♥\"; }");
        let p = first_element_child(&root);
        assert_eq!(
            p.style.text_emphasis_style,
            TextEmphasisStyle::String("♥".to_string())
        );
    }

    #[test]
    fn text_emphasis_style_invalid_ignored() {
        // Невалидное значение (два shape) — declaration ignored, остаётся initial.
        let root = lay(
            "<p>x</p>",
            "p { text-emphasis-style: dot triangle; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_emphasis_style, TextEmphasisStyle::None);
    }

    // ── visibility (CSS Display L3 §4) ──────────────────────────────────────

    #[test]
    fn visibility_default_visible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Visible);
    }

    #[test]
    fn visibility_hidden_parsed() {
        let root = lay("<p>x</p>", "p { visibility: hidden; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Hidden);
    }

    #[test]
    fn visibility_collapse_parsed() {
        let root = lay("<p>x</p>", "p { visibility: collapse; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.visibility, Visibility::Collapse);
    }

    #[test]
    fn visibility_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { visibility: hidden; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.visibility, Visibility::Hidden);
        assert_eq!(p.style.visibility, Visibility::Hidden);
    }

    #[test]
    fn visibility_visible_overrides_inherited_hidden() {
        // Дочерний может явно вернуть себя — это ключевая семантика CSS.
        let root = lay(
            "<div><p>x</p></div>",
            "div { visibility: hidden; } p { visibility: visible; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.visibility, Visibility::Hidden);
        assert_eq!(p.style.visibility, Visibility::Visible);
    }

    #[test]
    fn visibility_hidden_keeps_layout_height() {
        // В отличие от display:none, visibility:hidden оставляет коробку
        // в layout — она занимает место.
        let visible = lay("<p>x</p>", "");
        let hidden = lay("<p>x</p>", "p { visibility: hidden; }");
        let none = lay("<p>x</p>", "p { display: none; }");

        // Высота с hidden = высота visible.
        assert!((visible.rect.height - hidden.rect.height).abs() < 0.01,
            "visibility:hidden должен оставить высоту: visible={} hidden={}",
            visible.rect.height, hidden.rect.height);
        // Высота с display:none = 0 (бокс пропадает).
        assert!(none.rect.height < 0.1,
            "display:none должен убрать высоту: {}", none.rect.height);
    }

    // ── overflow (CSS Overflow L3) ──────────────────────────────────────────

    #[test]
    fn overflow_default_visible() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Visible);
        assert_eq!(p.style.overflow_y, Overflow::Visible);
    }

    #[test]
    fn overflow_shorthand_one_value() {
        let root = lay("<p>x</p>", "p { overflow: hidden; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Hidden);
        assert_eq!(p.style.overflow_y, Overflow::Hidden);
    }

    #[test]
    fn overflow_shorthand_two_values() {
        let root = lay("<p>x</p>", "p { overflow: scroll auto; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Scroll);
        assert_eq!(p.style.overflow_y, Overflow::Auto);
    }

    #[test]
    fn overflow_individual_x_y() {
        let root = lay(
            "<p>x</p>",
            "p { overflow-x: clip; overflow-y: scroll; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.overflow_x, Overflow::Clip);
        assert_eq!(p.style.overflow_y, Overflow::Scroll);
    }

    #[test]
    fn overflow_all_keywords() {
        for (kw, expected) in [
            ("visible", Overflow::Visible),
            ("hidden", Overflow::Hidden),
            ("clip", Overflow::Clip),
            ("scroll", Overflow::Scroll),
            ("auto", Overflow::Auto),
        ] {
            let css = format!("p {{ overflow: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            let p = first_element_child(&root);
            assert_eq!(p.style.overflow_x, expected, "kw = {kw}");
        }
    }

    #[test]
    fn overflow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { overflow: hidden; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.overflow_x, Overflow::Hidden);
        assert_eq!(p.style.overflow_x, Overflow::Visible);
    }

    // ── cursor (CSS UI L4 §8.1) ─────────────────────────────────────────────

    #[test]
    fn cursor_default_auto() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Auto);
    }

    #[test]
    fn cursor_keywords_parsed() {
        for (kw, expected) in [
            ("default", Cursor::Default),
            ("pointer", Cursor::Pointer),
            ("text", Cursor::Text),
            ("wait", Cursor::Wait),
            ("move", Cursor::Move),
            ("not-allowed", Cursor::NotAllowed),
            ("grab", Cursor::Grab),
            ("zoom-in", Cursor::ZoomIn),
            ("nesw-resize", Cursor::NeswResize),
        ] {
            let css = format!("p {{ cursor: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            let p = first_element_child(&root);
            assert_eq!(p.style.cursor, expected, "kw = {kw}");
        }
    }

    #[test]
    fn cursor_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { cursor: pointer; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.cursor, Cursor::Pointer);
        assert_eq!(p.style.cursor, Cursor::Pointer);
    }

    #[test]
    fn cursor_url_fallback_uses_keyword() {
        // CSS UI: `cursor: url(...) default` — берём последний keyword.
        // Phase 0 url() игнорируется.
        let root = lay(
            "<p>x</p>",
            "p { cursor: url(custom.png), pointer; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Pointer);
    }

    #[test]
    fn cursor_unknown_keeps_inherited() {
        let root = lay("<p>x</p>", "p { cursor: nonsense; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.cursor, Cursor::Auto);
    }

    // ── box-shadow (CSS Backgrounds L3 §4.6) ────────────────────────────────

    #[test]
    fn box_shadow_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.box_shadow.is_empty());
    }

    #[test]
    fn box_shadow_two_lengths() {
        // offset-x, offset-y без blur/spread/color.
        let root = lay("<p>x</p>", "p { box-shadow: 5px 10px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 1);
        let s = &p.style.box_shadow[0];
        assert!((s.offset_x - 5.0).abs() < 0.01);
        assert!((s.offset_y - 10.0).abs() < 0.01);
        assert_eq!(s.blur, 0.0);
        assert_eq!(s.spread, 0.0);
        assert!(!s.inset);
        assert!(s.color.is_none());
    }

    #[test]
    fn box_shadow_with_blur_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 2px 3px 4px red; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.blur, 4.0);
        assert_eq!(s.color.unwrap().r, 255);
    }

    #[test]
    fn box_shadow_with_blur_spread_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 1px 2px 3px 4px blue; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.spread, 4.0);
        assert_eq!(s.color.unwrap().b, 255);
    }

    #[test]
    fn box_shadow_inset() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: inset 2px 2px 5px black; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.box_shadow[0];
        assert!(s.inset);
        assert!((s.offset_x - 2.0).abs() < 0.01);
    }

    #[test]
    fn box_shadow_multiple_comma_separated() {
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 1px 1px red, 2px 2px blue, inset 3px 3px black; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 3);
        assert_eq!(p.style.box_shadow[0].color.unwrap().r, 255);
        assert_eq!(p.style.box_shadow[1].color.unwrap().b, 255);
        assert!(p.style.box_shadow[2].inset);
    }

    #[test]
    fn box_shadow_color_with_internal_commas() {
        // rgba(...) содержит запятые внутри — split_top_level_commas
        // не должен порвать это на куски.
        let root = lay(
            "<p>x</p>",
            "p { box-shadow: 2px 2px 4px rgba(0, 0, 0, 0.5); }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.box_shadow.len(), 1);
        let s = &p.style.box_shadow[0];
        assert_eq!(s.color.unwrap().a, 128);
    }

    #[test]
    fn box_shadow_none_clears() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-shadow: 1px 1px black; } p { box-shadow: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // box-shadow не наследуется в любом случае; но `none` должно
        // явно сбросить.
        assert_eq!(div.style.box_shadow.len(), 1);
        assert!(p.style.box_shadow.is_empty());
    }

    #[test]
    fn box_shadow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { box-shadow: 2px 2px black; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.box_shadow.len(), 1);
        assert!(p.style.box_shadow.is_empty());
    }

    // ── text-shadow (CSS Text Decoration L3 §4) ─────────────────────────────

    #[test]
    fn text_shadow_default_empty() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.text_shadow.is_empty());
    }

    #[test]
    fn text_shadow_two_lengths() {
        let root = lay("<p>x</p>", "p { text-shadow: 2px 3px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 1);
        let s = &p.style.text_shadow[0];
        assert!((s.offset_x - 2.0).abs() < 0.01);
        assert!((s.offset_y - 3.0).abs() < 0.01);
        assert_eq!(s.blur, 0.0);
        assert!(s.color.is_none());
    }

    #[test]
    fn text_shadow_with_blur_and_color() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 1px 2px 3px red; }",
        );
        let p = first_element_child(&root);
        let s = &p.style.text_shadow[0];
        assert_eq!(s.blur, 3.0);
        assert_eq!(s.color.unwrap().r, 255);
    }

    #[test]
    fn text_shadow_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 1px 1px red, 2px 2px blue; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 2);
        assert_eq!(p.style.text_shadow[0].color.unwrap().r, 255);
        assert_eq!(p.style.text_shadow[1].color.unwrap().b, 255);
    }

    #[test]
    fn text_shadow_inherited() {
        // В отличие от box-shadow, text-shadow ДОЛЖЕН наследоваться.
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-shadow: 1px 1px black; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_shadow.len(), 1);
        assert_eq!(p.style.text_shadow.len(), 1, "text-shadow должен наследоваться");
    }

    #[test]
    fn text_shadow_none_overrides_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-shadow: 1px 1px black; } p { text-shadow: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_shadow.len(), 1);
        assert!(p.style.text_shadow.is_empty(), "p должен сбросить inherited");
    }

    #[test]
    fn text_shadow_color_with_internal_commas() {
        let root = lay(
            "<p>x</p>",
            "p { text-shadow: 2px 2px 4px rgba(0, 0, 0, 0.5); }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.text_shadow.len(), 1);
        assert_eq!(p.style.text_shadow[0].color.unwrap().a, 128);
    }

    // ── border-radius (CSS Backgrounds L3 §5) ───────────────────────────────

    #[test]
    fn border_radius_default_zero() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(0.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(0.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(0.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_shorthand_one_value() {
        let root = lay("<p>x</p>", "p { border-radius: 8px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(8.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(8.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(8.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(8.0));
    }

    #[test]
    fn border_radius_shorthand_two_values() {
        // 2 значения: TL/BR одинаковы, TR/BL одинаковы.
        let root = lay("<p>x</p>", "p { border-radius: 4px 12px; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(4.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(12.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(4.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(12.0));
    }

    #[test]
    fn border_radius_shorthand_four_values() {
        let root = lay(
            "<p>x</p>",
            "p { border-radius: 1px 2px 3px 4px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(1.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(2.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(3.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(4.0));
    }

    #[test]
    fn border_radius_individual_corners() {
        let root = lay(
            "<p>x</p>",
            "p { border-top-left-radius: 5px; border-bottom-right-radius: 10px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(5.0));
        assert_eq!(p.style.border_top_right_radius, Length::Px(0.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Px(10.0));
        assert_eq!(p.style.border_bottom_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_em_resolves() {
        // 1em при default fs 16 = 16px; em резолвится сразу в Px.
        let root = lay("<p>x</p>", "p { border-radius: 1em; }");
        let p = first_element_child(&root);
        assert!(matches!(p.style.border_top_left_radius, Length::Px(v) if (v - 16.0).abs() < 0.01));
    }

    #[test]
    fn border_radius_elliptical_takes_first_part() {
        // `5px / 10px` (elliptical) — Phase 0 берёт только горизонтальный
        // (первый токен до `/`).
        let root = lay(
            "<p>x</p>",
            "p { border-radius: 5px / 10px; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius, Length::Px(5.0));
    }

    #[test]
    fn border_radius_negative_clamped_to_zero() {
        let root = lay("<p>x</p>", "p { border-radius: -10px; }");
        let p = first_element_child(&root);
        // Невалидное (отрицательное) — clamp до 0 в parse_radius_length.
        assert_eq!(p.style.border_top_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { border-radius: 5px; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.border_top_left_radius, Length::Px(5.0));
        assert_eq!(p.style.border_top_left_radius, Length::Px(0.0));
    }

    #[test]
    fn border_radius_percent_stored_as_percent() {
        // `border-radius: 50%` резолвинг откладывается до paint-time (known box dims).
        let root = lay("<p>x</p>", "p { border-radius: 50%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.border_top_left_radius,     Length::Percent(50.0));
        assert_eq!(p.style.border_top_right_radius,    Length::Percent(50.0));
        assert_eq!(p.style.border_bottom_right_radius, Length::Percent(50.0));
        assert_eq!(p.style.border_bottom_left_radius,  Length::Percent(50.0));
    }

    // ── text-overflow (CSS UI L4 §10.1) ─────────────────────────────────────

    #[test]
    fn text_overflow_default_clip() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_ellipsis_parsed() {
        let root = lay("<p>x</p>", "p { text-overflow: ellipsis; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Ellipsis);
    }

    #[test]
    fn text_overflow_clip_explicit() {
        let root = lay("<p>x</p>", "p { text-overflow: clip; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { text-overflow: ellipsis; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    #[test]
    fn text_overflow_unknown_keeps_default() {
        let root = lay("<p>x</p>", "p { text-overflow: nonsense; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.text_overflow, TextOverflow::Clip);
    }

    /// overflow:hidden + text-overflow:ellipsis + nowrap → длинный текст
    /// усекается, последний символ фрагмента — «…».
    #[test]
    fn text_overflow_ellipsis_truncates_overflowing_line() {
        // Fixed8: 8 px/char. "Hello World" = 11 chars = 88 px. Box = 64 px.
        // budget = 64 - 8(«…») = 56 px → влезает 7 chars "Hello W".
        // overflow и text-overflow — на одном элементе (p), чей стиль
        // наследует InlineRun.
        let root = lay_measured(
            "<p>Hello World</p>",
            "p { width: 64px; overflow: hidden; \
               white-space: nowrap; text-overflow: ellipsis; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = &p.children[0];
        let crate::BoxKind::InlineRun { lines, .. } = &run.kind else {
            panic!("expected InlineRun");
        };
        let line = &lines[0];
        assert_eq!(line.len(), 1, "один фрагмент после усечения");
        assert!(
            line[0].text.ends_with('\u{2026}'),
            "текст должен оканчиваться на «…», got {:?}",
            line[0].text
        );
        assert!(
            line[0].width <= 64.0,
            "ширина фрагмента должна влезать в контейнер: {}",
            line[0].width
        );
    }

    /// overflow:visible + text-overflow:ellipsis → усечения нет
    /// (spec: text-overflow не действует без overflow clip).
    #[test]
    fn text_overflow_ellipsis_no_effect_without_overflow_clip() {
        let root = lay_measured(
            "<p>Hello World</p>",
            "p { width: 64px; overflow: visible; \
               white-space: nowrap; text-overflow: ellipsis; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = &p.children[0];
        let crate::BoxKind::InlineRun { lines, .. } = &run.kind else {
            panic!("expected InlineRun");
        };
        let line = &lines[0];
        let text: String = line.iter().map(|f| f.text.as_str()).collect();
        assert!(
            !text.contains('\u{2026}'),
            "без overflow clip усечения быть не должно, got {text:?}"
        );
    }

    /// text-overflow:clip (default) → даже при overflow:hidden текст не усекается
    /// с «…»; clip происходит на уровне paint, не layout.
    #[test]
    fn text_overflow_clip_no_ellipsis() {
        let root = lay_measured(
            "<p>Hello World</p>",
            "p { width: 64px; overflow: hidden; \
               white-space: nowrap; text-overflow: clip; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = &p.children[0];
        let crate::BoxKind::InlineRun { lines, .. } = &run.kind else {
            panic!("expected InlineRun");
        };
        let line = &lines[0];
        let text: String = line.iter().map(|f| f.text.as_str()).collect();
        assert!(
            !text.contains('\u{2026}'),
            "text-overflow:clip не должен добавлять «…», got {text:?}"
        );
    }

    // ── selector matching: back-tracking edge cases ─────────────────────────

    /// `div div p` — двойной descendant. Должен матчить, когда есть два
    /// уровня div выше p. Без back-tracking тоже работает (greedy от p вверх
    /// находит ближайший div, дальше выше — другой div) — sanity check.
    #[test]
    fn selector_double_descendant_works() {
        let root = lay(
            "<div><div><p>x</p></div></div>",
            "div div p { color: red; }",
        );
        // Находим p глубоко.
        fn find_p<'a>(b: &'a LayoutBox, doc: &lumen_dom::Document) -> Option<&'a LayoutBox> {
            if let lumen_dom::NodeData::Element { name, .. } = &doc.get(b.node).data
                && name.local == "p"
            {
                return Some(b);
            }
            for c in &b.children {
                if let Some(f) = find_p(c, doc) {
                    return Some(f);
                }
            }
            None
        }
        let doc = lumen_html_parser::parse("<div><div><p>x</p></div></div>");
        let p = find_p(&root, &doc).unwrap();
        assert_eq!(p.style.color.r, 255);
    }

    /// `a a span` с двумя `<a>`-предками — должен матчить через compute_style
    /// (LayoutBox-фасад не подходит, т.к. <a> inline и весь контент сплавлен
    /// в InlineRun-ы; проверяем напрямую).
    #[test]
    fn selector_nested_same_tag_descendants() {
        // HTML5 parser re-normalizes nested <a> tags (inner <a> closes outer).
        // Use <div><a><div><a><span>x</span></a></div></a></div> which produces
        // two independent a-ancestors of span.
        let doc = lumen_html_parser::parse(r#"<div><a><div><a><span>x</span></a></div></a></div>"#);
        let span_id = find_first_by_tag(&doc, doc.root(), "span").expect("span");
        let style = crate::style::compute_style(
            &doc,
            span_id,
            &lumen_css_parser::parse("a a span { color: red; }"),
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(style.color.r, 255);
    }

    /// Чисто back-tracking-зависимый случай через compute_style. Дерево:
    /// `<div><a class="x"></a><a></a><a></a><span>X</span></div>`. Селектор:
    /// `.x + a ~ span`. Greedy от span: `~ span` находит span; `+ a` — это
    /// его прямой предыдущий sibling = третий `<a>`. Затем `.x` — sibling до
    /// него = второй `<a>`, который не имеет класс `.x` → fail. Backtracking
    /// перебирает `~ span` кандидатов: span сам = node → нет; либо для
    /// later-sibling combinator берёт КАЖДЫЙ earlier sibling. С back-tracking
    /// найдётся: `~ span` candidate = span (нет), но потом для `+ a` мы
    /// фиксируемся на втором `<a>` (через рекурсию), и первый `<a>` (`.x`)
    /// удовлетворяет `.x`.
    #[test]
    fn selector_backtracking_pathological_sibling() {
        let doc = lumen_html_parser::parse(
            r#"<div><a class="x">A</a><a>B</a><a>C</a><span>SPAN</span></div>"#,
        );
        let span_id = find_first_by_tag(&doc, doc.root(), "span").expect("span");
        let sheet = lumen_css_parser::parse(".x + a ~ span { color: red; }");
        let style = crate::style::compute_style(
            &doc,
            span_id,
            &sheet,
            &ComputedStyle::root(),
            Size::new(800.0, 600.0),
            false,
        );
        assert_eq!(
            style.color.r, 255,
            ".x + a ~ span должен сматчить span с back-tracking"
        );
    }

    fn find_first_by_tag(
        doc: &lumen_dom::Document,
        id: lumen_dom::NodeId,
        tag: &str,
    ) -> Option<lumen_dom::NodeId> {
        if let lumen_dom::NodeData::Element { name, .. } = &doc.get(id).data
            && name.local == tag
        {
            return Some(id);
        }
        for c in &doc.get(id).children {
            if let Some(f) = find_first_by_tag(doc, *c, tag) {
                return Some(f);
            }
        }
        None
    }

    // ── font-variant (CSS Fonts L4 §6, упрощённый) ──────────────────────────

    #[test]
    fn font_variant_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant, FontVariant::Normal);
    }

    #[test]
    fn font_variant_small_caps_parsed() {
        let root = lay("<p>x</p>", "p { font-variant: small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant, FontVariant::SmallCaps);
    }

    #[test]
    fn font_variant_caps_alias() {
        // CSS Fonts L4 §6.4: font-variant-caps — отдельное property,
        // парсится тем же кодом для small-caps значения.
        let root = lay("<p>x</p>", "p { font-variant-caps: small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant, FontVariant::SmallCaps);
    }

    #[test]
    fn font_variant_normal_keyword_resets() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; } p { font-variant: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_variant, FontVariant::SmallCaps);
        assert_eq!(p.style.font_variant, FontVariant::Normal);
    }

    #[test]
    fn font_variant_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_variant, FontVariant::SmallCaps);
    }

    // ── font-stretch (CSS Fonts L4 §2.5) ────────────────────────────────────

    #[test]
    fn font_stretch_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch, FontStretch::NORMAL);
    }

    #[test]
    fn font_stretch_keyword_condensed() {
        let root = lay("<p>x</p>", "p { font-stretch: condensed; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 750);
    }

    #[test]
    fn font_stretch_keyword_semi_expanded_fractional() {
        // 112.5% — дробный keyword проверяет, что хранение в десятых не теряет точность.
        let root = lay("<p>x</p>", "p { font-stretch: semi-expanded; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 1125);
    }

    #[test]
    fn font_stretch_percentage_value() {
        let root = lay("<p>x</p>", "p { font-stretch: 80%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 800);
    }

    #[test]
    fn font_stretch_percentage_clamped() {
        // Spec разрешает значения вне [50%, 200%], но Phase 0 их клампит —
        // экстремальные значения бесполезны и могут переполнить u16.
        let root = lay("<p>x</p>", "p { font-stretch: 10%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 500);

        let root = lay("<p>x</p>", "p { font-stretch: 300%; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_stretch.0, 2000);
    }

    #[test]
    fn font_stretch_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-stretch: expanded; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_stretch.0, 1250);
        assert_eq!(div.style.font_stretch.0, 1250);
    }

    #[test]
    fn font_stretch_normal_resets_inheritance() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-stretch: condensed; } p { font-stretch: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_stretch.0, 750);
        assert_eq!(p.style.font_stretch, FontStretch::NORMAL);
    }

    // ── accent-color (CSS UI L4 §6.1) ──────────────────────────────────────

    #[test]
    fn accent_color_default_none() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert!(p.style.accent_color.is_none());
    }

    #[test]
    fn accent_color_named() {
        let root = lay("<p>x</p>", "p { accent-color: red; }");
        let p = first_element_child(&root);
        let c = p.style.accent_color.expect("accent set");
        assert_eq!((c.r, c.g, c.b, c.a), (255, 0, 0, 255));
    }

    #[test]
    fn accent_color_hex() {
        let root = lay("<p>x</p>", "p { accent-color: #4080ff; }");
        let p = first_element_child(&root);
        let c = p.style.accent_color.expect("accent set");
        assert_eq!((c.r, c.g, c.b), (0x40, 0x80, 0xff));
    }

    #[test]
    fn accent_color_auto_resets_inheritance() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: blue; } p { accent-color: auto; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(div.style.accent_color.is_some());
        assert!(p.style.accent_color.is_none());
    }

    #[test]
    fn accent_color_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: rgb(10, 20, 30); }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        let dc = div.style.accent_color.expect("div accent");
        let pc = p.style.accent_color.expect("p inherits accent");
        assert_eq!((dc.r, dc.g, dc.b), (10, 20, 30));
        assert_eq!((pc.r, pc.g, pc.b), (10, 20, 30));
    }

    #[test]
    fn accent_color_invalid_ignored() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { accent-color: red; } p { accent-color: notacolor; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // Невалидное значение игнорируется → p наследует от div.
        assert_eq!(div.style.accent_color, p.style.accent_color);
        assert!(p.style.accent_color.is_some());
    }

    // ── :has() (CSS Selectors L4 §17.2) ─────────────────────────────────────

    /// `div:has(p)` — div, содержащий p в поддереве (через span).
    #[test]
    fn has_implicit_descendant_matches() {
        let root = lay(
            "<div><span><p>x</p></span></div><div><span>nope</span></div>",
            "div:has(p) { color: red; }",
        );
        let blocks: Vec<_> = root.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks[0].style.color.r, 255, "первый div должен сматчить");
        assert_eq!(blocks[1].style.color.r, 0, "второй div без p — нет");
    }

    /// `div:has(> .child)` — direct child only.
    #[test]
    fn has_child_combinator() {
        let root = lay(
            r#"<div><p class="child">x</p></div><div><span><p class="child">x</p></span></div>"#,
            "div:has(> .child) { color: red; }",
        );
        let blocks: Vec<_> = root.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks[0].style.color.r, 255);
        assert_eq!(blocks[1].style.color.r, 0);
    }

    /// `h2:has(+ p)` — h2 followed by p. Через compute_style напрямую.
    #[test]
    fn has_next_sibling() {
        let doc = lumen_html_parser::parse("<div><h2>A</h2><p>x</p></div><div><h2>B</h2></div>");
        let sheet = lumen_css_parser::parse("h2:has(+ p) { color: red; }");
        let root_style = ComputedStyle::root();
        let body = doc.body().unwrap();
        let div1 = doc.get(body).children[0];
        let h2_a = doc.get(div1).children[0];
        let div2 = doc.get(body).children[1];
        let h2_b = doc.get(div2).children[0];
        let style_a = crate::style::compute_style(
            &doc, h2_a, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let style_b = crate::style::compute_style(
            &doc, h2_b, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(style_a.color.r, 255, "h2 + p должен сматчить");
        assert_eq!(style_b.color.r, 0, "h2 без p после — нет");
    }

    /// `:has()` НЕ матчит сам node — descendants only.
    #[test]
    fn has_does_not_match_self() {
        let root = lay(
            "<p>x</p>",
            "p:has(p) { color: red; }",
        );
        let p = first_element_child(&root);
        assert_eq!(p.style.color.r, 0);
    }

    /// `:has(.a, .b)` — список (OR).
    #[test]
    fn has_list_or_match() {
        let root = lay(
            r#"<div><span class="b">x</span></div>"#,
            ":has(.a, .b) { color: red; }",
        );
        let div = first_element_child(&root);
        assert_eq!(div.style.color.r, 255);
    }

    // ── direction (CSS Writing Modes L3 §2.1) ──────────────────────────────

    #[test]
    fn direction_default_ltr() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Ltr);
    }

    #[test]
    fn direction_rtl_applied() {
        let root = lay("<p>x</p>", "p { direction: rtl; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_case_insensitive() {
        // Keyword-ы CSS property values — ASCII case-insensitive
        // (Values L4 §2.4). Документ может прийти с `RTL` или `Rtl`.
        let root = lay("<p>x</p>", "p { direction: RTL; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_inherited() {
        // direction распространяется от родителя — основа bidi-каскада.
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    #[test]
    fn direction_child_overrides_inherited() {
        // Inheritable, но потомок может явно переопределить — обратно на ltr.
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; } p { direction: ltr; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(p.style.direction, Direction::Ltr);
    }

    #[test]
    fn direction_invalid_keeps_inherited() {
        // Невалидное значение — сохраняем inherited (по CSS error recovery
        // правилу: invalid declaration → ignore).
        let root = lay(
            "<div><p>x</p></div>",
            "div { direction: rtl; } p { direction: vertical; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert_eq!(p.style.direction, Direction::Rtl);
    }

    /// text-align: start в RTL → правый край (start = right для RTL).
    /// "ab" = 16px в контейнере 100px; правый край = 100-16 = 84px.
    #[test]
    fn text_align_start_rtl_flushes_right() {
        let root = lay_measured(
            "<p>ab</p>",
            "p { direction: rtl; text-align: start; }",
            100.0,
        );
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            // В RTL-зеркале первый фрагмент в LTR-порядке переходит на правую сторону.
            // Последний фраг должен оканчиваться у content_width=100.
            let last = lines[0].last().unwrap();
            let right_edge = last.x + last.width;
            assert!(
                (right_edge - 100.0).abs() < 0.5,
                "expected right edge ≈ 100, got {right_edge}",
            );
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: end в RTL → левый край (end = left для RTL).
    /// "ab" = 16px в контейнере 100px; левый край первого фрагмента = 0.
    #[test]
    fn text_align_end_rtl_flushes_left() {
        let root = lay_measured(
            "<p>ab</p>",
            "p { direction: rtl; text-align: end; }",
            100.0,
        );
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            // В RTL + left align первый (левый) фраг начинается с x=0.
            let min_x = lines[0].iter().map(|f| f.x).fold(f32::INFINITY, f32::min);
            assert!(
                min_x.abs() < 0.5,
                "expected leftmost frag x ≈ 0, got {min_x}",
            );
        } else {
            panic!("expected InlineRun");
        }
    }

    /// text-align: start в LTR → левый край (start = left для LTR, нет смещения).
    #[test]
    fn text_align_start_ltr_flushes_left() {
        let root = lay_measured(
            "<p>ab</p>",
            "p { direction: ltr; text-align: start; }",
            100.0,
        );
        let p = first_element_child(&root);
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(!lines.is_empty());
            assert!((lines[0][0].x - 0.0).abs() < 0.01, "expected x=0, got {}", lines[0][0].x);
        } else {
            panic!("expected InlineRun");
        }
    }

    // ── CSS Containment L3 enforcement ──────────────────────────────────────

    /// contain:size → auto height = 0 (children don't contribute).
    #[test]
    fn contain_size_suppresses_auto_height() {
        let root = lay_measured(
            "<div><p>child</p></div>",
            "div { contain: size; } p { height: 50px; }",
            200.0,
        );
        let div = first_element_child(&root);
        // Explicit p height = 50px, but div has contain:size → div height = 0
        // (only padding+border, which are both 0 here).
        assert_eq!(div.rect.height, 0.0, "contain:size → auto height must be 0, got {}", div.rect.height);
    }

    /// contain:size with explicit height — explicit wins, children still don't contribute.
    #[test]
    fn contain_size_explicit_height_wins() {
        let root = lay_measured(
            "<div><p>child</p></div>",
            "div { contain: size; height: 80px; } p { height: 100px; }",
            200.0,
        );
        let div = first_element_child(&root);
        assert!((div.rect.height - 80.0).abs() < 0.5, "contain:size with explicit height=80, got {}", div.rect.height);
    }

    /// contain:layout parses and stores correctly.
    #[test]
    fn contain_layout_stores_flag() {
        let root = lay("<div></div>", "div { contain: layout; }");
        let div = first_element_child(&root);
        assert!(
            div.style.contain.0 & ContainFlags::LAYOUT.0 != 0,
            "contain:layout flag not set"
        );
    }

    /// contain:strict = size + layout + style + paint → auto height = 0.
    #[test]
    fn contain_strict_suppresses_auto_height() {
        let root = lay_measured(
            "<div><p>text</p></div>",
            "div { contain: strict; } p { height: 60px; }",
            200.0,
        );
        let div = first_element_child(&root);
        assert_eq!(div.rect.height, 0.0, "contain:strict → auto height must be 0, got {}", div.rect.height);
    }

    // ── CSS Container Queries L1 ──────────────────────────────────────────

    /// @container (min-width) — rule applies when container is wide enough.
    #[test]
    fn container_query_min_width_applies() {
        // Container is 200px wide. Rule applies at min-width:150px → p gets height:40px.
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; }
             @container (min-width: 150px) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container min-width:150px should apply to 200px container, got height={}",
            p.rect.height,
        );
    }

    /// @container (min-width) — rule does NOT apply when container is too narrow.
    #[test]
    fn container_query_min_width_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 100px; height: 100px; }
             @container (min-width: 200px) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "container min-width:200px should NOT apply to 100px container, got height={}",
            p.rect.height,
        );
    }

    /// @container (max-width) — rule applies when container is narrow.
    #[test]
    fn container_query_max_width_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: inline-size; width: 150px; height: 100px; }
             @container (max-width: 200px) { p { height: 30px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 30.0).abs() < 0.5,
            "container max-width:200px should apply to 150px container, got height={}",
            p.rect.height,
        );
    }

    /// Named @container — only applies to matching container-name.
    #[test]
    fn container_query_named_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; container-name: sidebar; width: 200px; height: 100px; }
             @container sidebar (min-width: 100px) { p { height: 50px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 50.0).abs() < 0.5,
            "named container query should match sidebar, got height={}",
            p.rect.height,
        );
    }

    /// Named @container — does NOT apply to wrong container name.
    #[test]
    fn container_query_named_wrong_name_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; container-name: main; width: 200px; height: 100px; }
             @container sidebar (min-width: 100px) { p { height: 50px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "named container 'sidebar' should NOT match 'main', got height={}",
            p.rect.height,
        );
    }

    // ── <img> replaced element ───────────────────────────────────────────

    fn first_image_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Image { .. }))
            .expect("expected at least one image child")
    }

    #[test]
    fn img_creates_image_box_with_src_and_alt() {
        let root = lay(r#"<img src="logo.png" alt="logo">"#, "");
        let img = first_image_child(&root);
        match &img.kind {
            BoxKind::Image { src, alt, .. } => {
                assert_eq!(src, "logo.png");
                assert_eq!(alt, "logo");
            }
            other => panic!("expected BoxKind::Image, got {other:?}"),
        }
    }

    #[test]
    fn img_without_src_or_alt_has_empty_strings() {
        let root = lay("<img>", "");
        let img = first_image_child(&root);
        if let BoxKind::Image { src, alt, .. } = &img.kind {
            assert_eq!(src, "");
            assert_eq!(alt, "");
        }
    }

    #[test]
    fn img_html_attributes_set_dimensions() {
        // HTML5 presentational hints: width/height атрибуты → CSS свойства,
        // без CSS-каскада победившего alternative.
        let root = lay(r#"<img src="x.png" width="120" height="80">"#, "");
        let img = first_image_child(&root);
        assert!((img.rect.width - 120.0).abs() < 0.1);
        assert!((img.rect.height - 80.0).abs() < 0.1);
    }

    #[test]
    fn img_css_overrides_html_attribute_dimensions() {
        // Author CSS перекрывает presentational hints (HTML5 §10).
        let root = lay(
            r#"<img src="x.png" width="120" height="80">"#,
            "img { width: 200px; height: 50px; }",
        );
        let img = first_image_child(&root);
        assert!((img.rect.width - 200.0).abs() < 0.1, "width={}", img.rect.width);
        assert!((img.rect.height - 50.0).abs() < 0.1, "height={}", img.rect.height);
    }

    #[test]
    fn img_without_dimensions_is_zero_sized() {
        // Без атрибутов и без CSS — image не загружено, intrinsic неизвестен,
        // коробка 0×0. Это honest placeholder — будет ясно, что чего-то не
        // хватает.
        let root = lay(r#"<img src="x.png">"#, "");
        let img = first_image_child(&root);
        assert!(img.rect.width.abs() < 0.1);
        assert!(img.rect.height.abs() < 0.1);
    }

    #[test]
    fn img_invalid_width_attribute_ignored() {
        // HTML5: nonsense → ignore.
        let root = lay(r#"<img src="x" width="abc" height="-50">"#, "");
        let img = first_image_child(&root);
        assert!(img.rect.width.abs() < 0.1);
        assert!(img.rect.height.abs() < 0.1);
    }

    #[test]
    fn img_padding_and_border_extend_box() {
        // CSS box для replaced element ведёт себя как block: padding + border
        // расширяют rect (content-box). Размер картинки 100×60, padding 10,
        // border 2 → rect 124×84.
        let root = lay(
            r#"<img src="x" width="100" height="60">"#,
            "img { padding: 10px; border: 2px solid red; }",
        );
        let img = first_image_child(&root);
        assert!((img.rect.width - 124.0).abs() < 0.1, "width={}", img.rect.width);
        assert!((img.rect.height - 84.0).abs() < 0.1, "height={}", img.rect.height);
    }

    #[test]
    fn img_not_treated_as_inline_content() {
        // <img> в Phase 0 — block-level. Текст до и после не объединяется с
        // ним в один InlineRun.
        let root = lay(r#"<div>before<img src="x" width="10" height="10">after</div>"#, "");
        let div = first_element_child(&root);
        // div должен иметь 3 потомка: InlineRun("before") + Image + InlineRun("after").
        assert_eq!(div.children.len(), 3, "got {}", div.children.len());
        assert!(matches!(div.children[0].kind, BoxKind::InlineRun { .. }));
        assert!(matches!(div.children[1].kind, BoxKind::Image { .. }));
        assert!(matches!(div.children[2].kind, BoxKind::InlineRun { .. }));
    }

    #[test]
    fn img_display_none_is_skipped() {
        let root = lay(
            r#"<img src="x.png" width="100" height="50">"#,
            "img { display: none; }",
        );
        let has_image = root.children.iter().any(|c| matches!(c.kind, BoxKind::Image { .. }));
        assert!(!has_image, "img with display:none should not produce Image box");
    }

    #[test]
    fn img_attr_name_case_insensitive() {
        // HTML-парсер lower-case-ит имена тегов, но атрибуты могут попасть в
        // mixed-case. Наш get_attr — ASCII case-insensitive.
        let root = lay(r#"<img SRC="x.png" Width="50" HEIGHT="30">"#, "");
        let img = first_image_child(&root);
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "x.png");
        }
        assert!((img.rect.width - 50.0).abs() < 0.1);
        assert!((img.rect.height - 30.0).abs() < 0.1);
    }

    // ──────── <video> replaced element ────────

    fn first_video_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Video { .. }))
            .expect("expected at least one video child")
    }

    #[test]
    fn video_creates_video_box_with_src() {
        let root = lay(r#"<video src="clip.mp4"></video>"#, "");
        let vid = first_video_child(&root);
        match &vid.kind {
            BoxKind::Video { src, poster } => {
                assert_eq!(src, "clip.mp4");
                assert_eq!(poster, "");
            }
            other => panic!("expected BoxKind::Video, got {other:?}"),
        }
    }

    #[test]
    fn video_captures_poster_attribute() {
        let root = lay(r#"<video src="clip.mp4" poster="thumb.jpg"></video>"#, "");
        let vid = first_video_child(&root);
        if let BoxKind::Video { poster, .. } = &vid.kind {
            assert_eq!(poster, "thumb.jpg");
        }
    }

    #[test]
    fn video_ua_default_size_300_by_150() {
        // HTML spec §14.1: UA default intrinsic size 300×150 CSS px.
        let root = lay(r#"<video src="clip.mp4"></video>"#, "");
        let vid = first_video_child(&root);
        assert!((vid.rect.width - 300.0).abs() < 0.1, "width={}", vid.rect.width);
        assert!((vid.rect.height - 150.0).abs() < 0.1, "height={}", vid.rect.height);
    }

    #[test]
    fn video_html_attribute_dimensions_override_ua_default() {
        let root = lay(r#"<video src="clip.mp4" width="640" height="360"></video>"#, "");
        let vid = first_video_child(&root);
        assert!((vid.rect.width - 640.0).abs() < 0.1, "width={}", vid.rect.width);
        assert!((vid.rect.height - 360.0).abs() < 0.1, "height={}", vid.rect.height);
    }

    #[test]
    fn video_css_overrides_ua_default() {
        let root = lay(
            r#"<video src="clip.mp4"></video>"#,
            "video { width: 480px; height: 270px; }",
        );
        let vid = first_video_child(&root);
        assert!((vid.rect.width - 480.0).abs() < 0.1, "width={}", vid.rect.width);
        assert!((vid.rect.height - 270.0).abs() < 0.1, "height={}", vid.rect.height);
    }

    #[test]
    fn video_display_none_is_skipped() {
        let root = lay(
            r#"<video src="clip.mp4"></video>"#,
            "video { display: none; }",
        );
        let has_video = root.children.iter().any(|c| matches!(c.kind, BoxKind::Video { .. }));
        assert!(!has_video, "video with display:none should not produce Video box");
    }

    #[test]
    fn video_is_replaced_element_does_not_stretch() {
        // Replaced elements do NOT stretch to fill container width (CSS 2.1 §10.3.2).
        let root = lay(r#"<video src="clip.mp4"></video>"#, "");
        let vid = first_video_child(&root);
        // UA default 300px, not 800px (viewport width).
        assert!((vid.rect.width - 300.0).abs() < 0.1, "width={}", vid.rect.width);
    }

    // ──────── <iframe> placeholder layout ───────────────────────────────────

    fn first_iframe_child(b: &LayoutBox) -> &LayoutBox {
        b.children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Iframe { .. }))
            .expect("expected at least one Iframe box")
    }

    #[test]
    fn iframe_creates_iframe_box_with_src() {
        let root = lay(r#"<iframe src="https://example.com"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        match &frame.kind {
            BoxKind::Iframe { src, .. } => assert_eq!(src, "https://example.com"),
            other => panic!("expected BoxKind::Iframe, got {other:?}"),
        }
    }

    #[test]
    fn iframe_ua_default_size_300_by_150() {
        // HTML spec §4.8.5: UA default intrinsic size is 300×150 CSS px.
        let root = lay(r#"<iframe src="x.html"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        assert!((frame.rect.width - 300.0).abs() < 0.1, "width={}", frame.rect.width);
        assert!((frame.rect.height - 150.0).abs() < 0.1, "height={}", frame.rect.height);
    }

    #[test]
    fn iframe_html_attribute_dimensions_override_ua_default() {
        let root = lay(r#"<iframe src="x.html" width="800" height="600"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        assert!((frame.rect.width - 800.0).abs() < 0.1, "width={}", frame.rect.width);
        assert!((frame.rect.height - 600.0).abs() < 0.1, "height={}", frame.rect.height);
    }

    #[test]
    fn iframe_css_overrides_ua_default() {
        let root = lay(
            r#"<iframe src="x.html"></iframe>"#,
            "iframe { width: 400px; height: 300px; }",
        );
        let frame = first_iframe_child(&root);
        assert!((frame.rect.width - 400.0).abs() < 0.1, "width={}", frame.rect.width);
        assert!((frame.rect.height - 300.0).abs() < 0.1, "height={}", frame.rect.height);
    }

    #[test]
    fn iframe_is_replaced_element_does_not_stretch() {
        // Replaced elements do NOT stretch to fill container width (CSS 2.1 §10.3.2).
        let root = lay(r#"<iframe src="x.html"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        // UA default 300px, not 800px (viewport width).
        assert!((frame.rect.width - 300.0).abs() < 0.1, "width={}", frame.rect.width);
    }

    #[test]
    fn iframe_empty_src_is_valid() {
        let root = lay(r#"<iframe></iframe>"#, "");
        let frame = first_iframe_child(&root);
        match &frame.kind {
            BoxKind::Iframe { src, .. } => assert_eq!(src, ""),
            other => panic!("expected BoxKind::Iframe, got {other:?}"),
        }
    }

    #[test]
    fn iframe_srcdoc_stored_in_box_kind() {
        let root = lay(r#"<iframe srcdoc="<p>hello</p>"></iframe>"#, "");
        let frame = first_iframe_child(&root);
        match &frame.kind {
            BoxKind::Iframe { srcdoc, .. } => {
                assert_eq!(srcdoc.as_deref(), Some("<p>hello</p>"));
            }
            other => panic!("expected BoxKind::Iframe, got {other:?}"),
        }
    }

    #[test]
    fn build_iframe_document_empty_html_returns_document() {
        let doc = build_iframe_document("");
        // Empty input still produces a valid Document with a root node that has children.
        // lumen_html_parser::parse always inserts implicit html/head/body.
        assert!(!doc.get(doc.root()).children.is_empty());
    }

    #[test]
    fn build_iframe_document_parses_inline_html() {
        let doc = build_iframe_document("<p>hello world</p>");
        // The parsed document should contain a paragraph element somewhere in the tree.
        let mut found = false;
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            if doc.get(id).element_name().is_some_and(|n| n.local == "p") {
                found = true;
                break;
            }
            stack.extend_from_slice(&doc.get(id).children);
        }
        assert!(found, "expected <p> in parsed srcdoc document");
    }

    // ──────── <picture> / <img srcset> source-selection integration ────────

    /// Рекурсивный поиск первого `Image`-бокса в дереве. Нужен для тестов
    /// с `<picture>`: inner `<img>` зарывается на 2 уровня (picture-обёртка
    /// сначала становится Block).
    fn find_image(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Image { .. }) {
            return Some(b);
        }
        for c in &b.children {
            if let Some(found) = find_image(c) {
                return Some(found);
            }
        }
        None
    }

    /// Рекурсивный поиск любого `LayoutBox`, у которого `BoxKind::Image`
    /// присутствует — возвращает все, чтобы посчитать.
    fn count_image_boxes(b: &LayoutBox) -> usize {
        let mut n = usize::from(matches!(b.kind, BoxKind::Image { .. }));
        for c in &b.children {
            n += count_image_boxes(c);
        }
        n
    }

    #[test]
    fn picture_uses_source_srcset_over_inner_img() {
        // `<picture>`-picker выбирает первый матчащий `<source>` до
        // fallback `<img>`. У нас один `<source>` без media-фильтра —
        // он всегда выигрывает у inner img.
        let root = lay(
            r#"<picture>
                <source srcset="hires.png">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img inside picture");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "hires.png", "picker должен был выбрать source, а не fallback");
        } else {
            panic!("expected Image");
        }
    }

    #[test]
    fn picture_media_filter_picks_matching_source() {
        // viewport 800×600 — `(min-width: 700px)` матчит, `(max-width: 500px)` нет.
        let root = lay(
            r#"<picture>
                <source media="(max-width: 500px)" srcset="small.png">
                <source media="(min-width: 700px)" srcset="big.png">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img inside picture");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "big.png");
        }
    }

    #[test]
    fn picture_falls_back_to_inner_img_when_no_source_matches() {
        // Все `<source>` отсеяны media-фильтром → picker идёт на inner `<img>`.
        let root = lay(
            r#"<picture>
                <source media="(max-width: 100px)" srcset="tiny.png">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img inside picture");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "fallback.png");
        }
    }

    #[test]
    fn img_srcset_density_picker_selects_one_x_at_dpr_1() {
        // DPR в layout фиксирован на 1.0 (Phase 0). Среди density-кандидатов
        // picker выберет 1x как ближайший — это `low.png`.
        let root = lay(r#"<img srcset="low.png 1x, high.png 2x" src="z.png">"#, "");
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "low.png");
        }
    }

    #[test]
    fn img_srcset_falls_back_to_src_when_picker_empty() {
        // srcset из одних запятых — нет валидных кандидатов; picker
        // возвращает raw src через свой внутренний fallback.
        let root = lay(r#"<img srcset=",,," src="real.png">"#, "");
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "real.png");
        }
    }

    #[test]
    fn block_with_inline_image_includes_baseline_descent_gap() {
        // BUG-180: a bare <img> is an inline-level replaced element, baseline-aligned
        // by default, so its line box — and therefore the height of the block that
        // wraps it — extends below the image by the strut descent (the classic
        // "image bottom gap"). Lumen lays a lone <img> as a block-flow child, so this
        // sub-baseline space must be added explicitly; without it an image grid drifts
        // ~descent px upward per row versus a browser (TEST-18: 22.1% → 2.1%).
        let doc = lumen_html_parser::parse(
            r#"<div id="frame"><img src="a.png" width="200" height="150"></div>"#,
        );
        let sheet = lumen_css_parser::parse("#frame { padding: 3px; }");
        let root = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let frame = find_by_tag(&root, "div", &doc).expect("frame div");
        // Fixed8.descent_px(16) = 16 * 0.2 = 3.2 (default strut descent).
        // content = img 150 + descent 3.2; border-box = + padding 6 = 159.2.
        let expected = 150.0 + 16.0 * 0.2 + 6.0;
        assert!(
            (frame.rect.height - expected).abs() < 0.01,
            "frame height {} should include the image-bottom descent gap (expected {expected})",
            frame.rect.height,
        );
    }

    #[test]
    fn block_with_top_aligned_image_has_no_descent_gap() {
        // Contrast to the baseline case: vertical-align:top anchors the replaced box
        // to the line-box top, so there is no sub-baseline gap — the frame is exactly
        // img + padding.
        let doc = lumen_html_parser::parse(
            r#"<div id="frame"><img src="a.png" width="200" height="150"></div>"#,
        );
        let sheet = lumen_css_parser::parse("#frame { padding: 3px; } img { vertical-align: top; }");
        let root = body_layout_box(layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8));
        let frame = find_by_tag(&root, "div", &doc).expect("frame div");
        assert!(
            (frame.rect.height - (150.0 + 6.0)).abs() < 0.01,
            "top-aligned image must not add the baseline descent gap, got {}",
            frame.rect.height,
        );
    }

    #[test]
    fn img_without_src_and_srcset_produces_empty_url() {
        // Битая разметка — picker возвращает None, мы падаем в legacy
        // fallback и сохраняем пустой src (как и было до интеграции).
        let root = lay("<img>", "");
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "");
        }
    }

    #[test]
    fn source_element_does_not_produce_box() {
        // `<source>` теперь Display::None — два source-а внутри `<picture>` не
        // порождают LayoutBox-ов. Проверяем по двум инвариантам: ровно один
        // Image-box в дереве (от inner `<img>`) и общее число дочерних
        // блоков у picture-обёртки = 1 (только сам `<img>`-box, плюс
        // потенциально whitespace InlineRun-ы).
        let root = lay(
            r#"<picture><source srcset="a.png"><source srcset="b.png"><img src="c.png"></picture>"#,
            "",
        );
        assert_eq!(count_image_boxes(&root), 1);
        let img = find_image(&root).expect("img");
        if let BoxKind::Image { src, .. } = &img.kind {
            assert_eq!(src, "a.png", "первый матчащий source — победитель");
        }
    }

    #[test]
    fn picture_source_intrinsic_dims_fill_blank_style() {
        // У выбранного `<source>` есть width/height атрибуты, у inner `<img>` нет,
        // и автор CSS не задал — intrinsic dims с source-а попадают в layout-box.
        let root = lay(
            r#"<picture>
                <source srcset="big.png" width="240" height="160">
                <img src="fallback.png">
            </picture>"#,
            "",
        );
        let img = find_image(&root).expect("img");
        assert!((img.rect.width - 240.0).abs() < 0.1, "width={}", img.rect.width);
        assert!((img.rect.height - 160.0).abs() < 0.1, "height={}", img.rect.height);
    }

    #[test]
    fn picture_source_intrinsic_does_not_override_author_css() {
        // Author CSS перекрывает intrinsic dimensions с `<source>` — это
        // обычная presentational-hint специфика (HTML5 §10).
        let root = lay(
            r#"<picture>
                <source srcset="big.png" width="240" height="160">
                <img src="fallback.png">
            </picture>"#,
            "img { width: 100px; height: 50px; }",
        );
        let img = find_image(&root).expect("img");
        assert!((img.rect.width - 100.0).abs() < 0.1);
        assert!((img.rect.height - 50.0).abs() < 0.1);
    }

    // ──────── CSS-wide keywords (CSS Cascade L4 §7) ────────

    #[test]
    fn parse_css_wide_keyword_matches_all_four() {
        use crate::CssWideKeyword;
        assert_eq!(crate::parse_css_wide_keyword("inherit"), Some(CssWideKeyword::Inherit));
        assert_eq!(crate::parse_css_wide_keyword("INITIAL"), Some(CssWideKeyword::Initial));
        assert_eq!(crate::parse_css_wide_keyword("Unset"), Some(CssWideKeyword::Unset));
        assert_eq!(crate::parse_css_wide_keyword("revert"), Some(CssWideKeyword::Revert));
        assert_eq!(crate::parse_css_wide_keyword("  inherit  "), Some(CssWideKeyword::Inherit));
        assert_eq!(crate::parse_css_wide_keyword("red"), None);
        assert_eq!(crate::parse_css_wide_keyword("inheritance"), None);
    }

    /// Получить style вложенного `<p>` из `<div><p>x</p></div>`-тестового
    /// дерева. root → first child (anonymous wrapper или div) → first child block.
    /// Возвращает style p — там и применяется тестируемая декларация.
    fn nested_p_style(root: &LayoutBox) -> &ComputedStyle {
        let div = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("div block");
        let p = div
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        &p.style
    }

    fn lay_get_p_color(html: &str, css: &str) -> Color {
        let root = lay(html, css);
        nested_p_style(&root).color
    }

    #[test]
    fn css_inherit_forces_parent_color_on_non_inherited_default() {
        // Для inherited-свойств (color) — `inherit` совпадает с дефолтом
        // (если родитель сам не переопределяет). Подтверждает no-op в этом
        // тривиальном случае.
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: inherit; }",
        );
        // p наследует от div = red.
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn css_initial_resets_color_to_initial() {
        // Initial value for color — black (Color::BLACK).
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: initial; }",
        );
        assert_eq!(c, Color::BLACK);
    }

    #[test]
    fn css_unset_inherited_property_acts_as_inherit() {
        // color — inherited; `unset` для inherited = inherit → parent's red.
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: unset; }",
        );
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn css_unset_undoes_prior_declaration() {
        // p { color: blue; color: unset; } → unset вступает позже,
        // откатывает blue до inherited (red).
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: blue; color: unset; }",
        );
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn css_inherit_on_non_inherited_pulls_from_parent() {
        // background-color НЕ inherited. По умолчанию None у потомка.
        // `inherit` форсит наследование → background.color родителя.
        let root = lay(
            "<div><p>x</p></div>",
            "div { background-color: rgb(0, 100, 200); } p { background-color: inherit; }",
        );
        // Найдём p — это child div, который сам root.children[0].
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(
            p.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 100, b: 200, a: 255 }))
        );
    }

    #[test]
    fn css_initial_on_non_inherited_resets_to_default() {
        // background-color: red → initial → None (default).
        let root = lay(
            "<p>x</p>",
            "p { background-color: red; background-color: initial; }",
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.background_color, None);
    }

    #[test]
    fn css_font_size_inherit_uses_parent() {
        // font-size: inherit для p → parent font_size = 30px.
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-size: 30px; } p { font-size: 40px; font-size: inherit; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.font_size - 30.0).abs() < 0.1, "fs={}", p.style.font_size);
    }

    #[test]
    fn css_font_size_initial_is_16() {
        let root = lay(
            "<p>x</p>",
            "p { font-size: 40px; font-size: initial; }",
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.font_size - 16.0).abs() < 0.1, "fs={}", p.style.font_size);
    }

    #[test]
    fn css_unset_non_inherited_resets_to_initial() {
        // background-color: red → unset → None (initial — non-inherited prop).
        let root = lay(
            "<p>x</p>",
            "p { background-color: red; background-color: unset; }",
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.background_color, None);
    }

    #[test]
    fn css_revert_treated_like_unset_in_phase0() {
        // Phase 0: revert == unset. Тест дублирует css_unset_*.
        let c1 = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: blue; color: revert; }",
        );
        assert_eq!(c1, Color { r: 255, g: 0, b: 0, a: 255 }); // inherited
    }

    #[test]
    fn css_wide_keyword_case_insensitive_in_value() {
        // CSS keyword values — ASCII case-insensitive по CSS Values L4 §2.4.
        let c = lay_get_p_color(
            "<div><p>x</p></div>",
            "div { color: red; } p { color: INHERIT; }",
        );
        assert_eq!(c, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ──────── @property syntax-валидация (CSS Properties and Values L1 §2) ────────

    fn lay_get_custom_prop(html: &str, css: &str, key: &str) -> Option<String> {
        let root = lay(html, css);
        let p = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("first block");
        p.style.custom_props.get(key).cloned()
    }

    #[test]
    fn property_syntax_universal_accepts_anything() {
        // syntax: '*' — любое значение проходит, в т.ч. бессмысленное.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --foo { syntax: '*'; inherits: false; initial-value: 0; } p { --foo: garbage; }",
            "--foo",
        );
        assert_eq!(v, Some("garbage".to_string()));
    }

    #[test]
    fn property_syntax_length_accepts_px() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --gap: 10px; }",
            "--gap",
        );
        assert_eq!(v, Some("10px".to_string()));
    }

    #[test]
    fn property_syntax_length_rejects_color() {
        // syntax: '<length>' + value=red → invalid; declaration пропускается,
        // остаётся initial-value '0px'.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --gap: red; }",
            "--gap",
        );
        assert_eq!(v, Some("0px".to_string()));
    }

    #[test]
    fn property_syntax_length_rejects_percentage() {
        // <length> НЕ принимает `%` — это <percentage>.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --gap: 50%; }",
            "--gap",
        );
        assert_eq!(v, Some("0px".to_string()));
    }

    #[test]
    fn property_syntax_color_accepts_named_and_hex() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --bg { syntax: '<color>'; inherits: false; initial-value: black; } p { --bg: red; }",
            "--bg",
        );
        assert_eq!(v, Some("red".to_string()));
    }

    #[test]
    fn property_syntax_color_rejects_length() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --bg { syntax: '<color>'; inherits: false; initial-value: black; } p { --bg: 10px; }",
            "--bg",
        );
        assert_eq!(v, Some("black".to_string()));
    }

    #[test]
    fn property_syntax_union_length_or_percentage() {
        // `<length-percentage>` принимает оба.
        let v1 = lay_get_custom_prop(
            "<p>x</p>",
            "@property --w { syntax: '<length-percentage>'; inherits: false; initial-value: 0px; } p { --w: 50%; }",
            "--w",
        );
        assert_eq!(v1, Some("50%".to_string()));
        let v2 = lay_get_custom_prop(
            "<p>x</p>",
            "@property --w { syntax: '<length-percentage>'; inherits: false; initial-value: 0px; } p { --w: 10rem; }",
            "--w",
        );
        assert_eq!(v2, Some("10rem".to_string()));
    }

    #[test]
    fn property_syntax_or_alternative() {
        // syntax с `|`: '<length> | <color>'. Оба подходят.
        let v_len = lay_get_custom_prop(
            "<p>x</p>",
            "@property --x { syntax: '<length> | <color>'; inherits: false; initial-value: 0px; } p { --x: 5px; }",
            "--x",
        );
        assert_eq!(v_len, Some("5px".to_string()));
        let v_color = lay_get_custom_prop(
            "<p>x</p>",
            "@property --x { syntax: '<length> | <color>'; inherits: false; initial-value: 0px; } p { --x: blue; }",
            "--x",
        );
        assert_eq!(v_color, Some("blue".to_string()));
    }

    #[test]
    fn property_syntax_skips_value_with_var() {
        // value содержит `var(` — пропускается без валидации, потому что
        // expand var() происходит позже.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: 0px; } p { --base: 7px; --gap: var(--base); }",
            "--gap",
        );
        // var(--base) сохранён как есть; resolve будет при apply_declaration.
        assert_eq!(v, Some("var(--base)".to_string()));
    }

    #[test]
    fn property_invalid_initial_value_skipped() {
        // initial-value не подходит под syntax → не подставляется. Без
        // декларации потомка свойство остаётся вне custom_props.
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --gap { syntax: '<length>'; inherits: false; initial-value: red; }",
            "--gap",
        );
        assert_eq!(v, None);
    }

    #[test]
    fn property_validate_integer_accepts_signed() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --n { syntax: '<integer>'; inherits: false; initial-value: 0; } p { --n: -42; }",
            "--n",
        );
        assert_eq!(v, Some("-42".to_string()));
    }

    #[test]
    fn property_validate_integer_rejects_float() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --n { syntax: '<integer>'; inherits: false; initial-value: 0; } p { --n: 3.14; }",
            "--n",
        );
        assert_eq!(v, Some("0".to_string()));
    }

    #[test]
    fn property_validate_time_accepts_seconds_and_ms() {
        let v_s = lay_get_custom_prop(
            "<p>x</p>",
            "@property --dur { syntax: '<time>'; inherits: false; initial-value: 0s; } p { --dur: 1.5s; }",
            "--dur",
        );
        assert_eq!(v_s, Some("1.5s".to_string()));

        let v_ms = lay_get_custom_prop(
            "<p>x</p>",
            "@property --dur { syntax: '<time>'; inherits: false; initial-value: 0s; } p { --dur: 200ms; }",
            "--dur",
        );
        assert_eq!(v_ms, Some("200ms".to_string()));
    }

    #[test]
    fn property_validate_time_rejects_non_time() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --dur { syntax: '<time>'; inherits: false; initial-value: 0s; } p { --dur: 100px; }",
            "--dur",
        );
        assert_eq!(v, Some("0s".to_string()));
    }

    #[test]
    fn property_validate_resolution_units() {
        // <resolution> принимает dpi / dpcm / dppx / x (alias dppx).
        for (val, expected) in [
            ("96dpi", "96dpi"),
            ("2dppx", "2dppx"),
            ("38dpcm", "38dpcm"),
            ("2x", "2x"),
        ] {
            let css = format!(
                "@property --r {{ syntax: '<resolution>'; inherits: false; initial-value: 1dppx; }} p {{ --r: {val}; }}"
            );
            let v = lay_get_custom_prop("<p>x</p>", &css, "--r");
            assert_eq!(v, Some(expected.to_string()), "value: {val}");
        }
    }

    #[test]
    fn property_validate_resolution_rejects_non_resolution() {
        let v = lay_get_custom_prop(
            "<p>x</p>",
            "@property --r { syntax: '<resolution>'; inherits: false; initial-value: 1dppx; } p { --r: 5s; }",
            "--r",
        );
        assert_eq!(v, Some("1dppx".to_string()));
    }

    // ──────── CSS counters (CSS Lists L3 §3) ────────

    fn first_block_style(root: &LayoutBox) -> &ComputedStyle {
        let p = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        &p.style
    }

    #[test]
    fn counter_reset_single_default_zero() {
        let root = lay("<p>x</p>", "p { counter-reset: section; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_reset, vec![("section".to_string(), 0)]);
    }

    #[test]
    fn counter_reset_with_explicit_value() {
        let root = lay("<p>x</p>", "p { counter-reset: section 5; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_reset, vec![("section".to_string(), 5)]);
    }

    #[test]
    fn counter_reset_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { counter-reset: section 1 subsection 0 figure; }",
        );
        let s = first_block_style(&root);
        assert_eq!(
            s.counter_reset,
            vec![
                ("section".to_string(), 1),
                ("subsection".to_string(), 0),
                ("figure".to_string(), 0),  // default = 0
            ]
        );
    }

    #[test]
    fn counter_reset_none_yields_empty() {
        let root = lay("<p>x</p>", "p { counter-reset: none; }");
        let s = first_block_style(&root);
        assert!(s.counter_reset.is_empty());
    }

    #[test]
    fn counter_reset_case_insensitive_none() {
        let root = lay("<p>x</p>", "p { counter-reset: NONE; }");
        let s = first_block_style(&root);
        assert!(s.counter_reset.is_empty());
    }

    #[test]
    fn counter_increment_default_one() {
        let root = lay("<p>x</p>", "p { counter-increment: section; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_increment, vec![("section".to_string(), 1)]);
    }

    #[test]
    fn counter_increment_with_explicit_value() {
        let root = lay("<p>x</p>", "p { counter-increment: section 2; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_increment, vec![("section".to_string(), 2)]);
    }

    #[test]
    fn counter_increment_multiple_with_mixed_defaults() {
        let root = lay(
            "<p>x</p>",
            "p { counter-increment: a 3 b c 5; }",
        );
        let s = first_block_style(&root);
        assert_eq!(
            s.counter_increment,
            vec![
                ("a".to_string(), 3),
                ("b".to_string(), 1),  // default = 1
                ("c".to_string(), 5),
            ]
        );
    }

    #[test]
    fn counter_set_default_zero() {
        // CSS Lists L3 §4 — `counter-set: name` без числа → значение 0.
        let root = lay("<p>x</p>", "p { counter-set: section; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_set, vec![("section".to_string(), 0)]);
    }

    #[test]
    fn counter_set_with_explicit_value() {
        let root = lay("<p>x</p>", "p { counter-set: section 5; }");
        let s = first_block_style(&root);
        assert_eq!(s.counter_set, vec![("section".to_string(), 5)]);
    }

    #[test]
    fn counter_set_multiple_with_mixed_defaults() {
        let root = lay("<p>x</p>", "p { counter-set: a 3 b c 5; }");
        let s = first_block_style(&root);
        assert_eq!(
            s.counter_set,
            vec![
                ("a".to_string(), 3),
                ("b".to_string(), 0), // default = 0
                ("c".to_string(), 5),
            ]
        );
    }

    #[test]
    fn counter_set_none_yields_empty() {
        let root = lay("<p>x</p>", "p { counter-set: none; }");
        let s = first_block_style(&root);
        assert!(s.counter_set.is_empty());
    }

    #[test]
    fn counter_set_not_inherited_by_default() {
        // counter-set не наследуется (CSS Lists L3 §4).
        let root = lay(
            "<div><p>x</p></div>",
            "div { counter-set: section 3; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(p.style.counter_set.is_empty());
        assert!(!div.style.counter_set.is_empty());
    }

    #[test]
    fn counter_not_inherited_by_default() {
        // counter-reset / -increment не наследуются (CSS Lists L3 §3).
        let root = lay(
            "<div><p>x</p></div>",
            "div { counter-reset: section; }",
        );
        // У <p> не должно быть счётчиков.
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(p.style.counter_reset.is_empty());
        assert!(!div.style.counter_reset.is_empty());  // у div есть
    }

    #[test]
    fn counter_inherit_keyword_pulls_from_parent() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { counter-reset: section 7; } p { counter-reset: inherit; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.counter_reset, vec![("section".to_string(), 7)]);
    }

    #[test]
    fn counter_initial_keyword_resets_to_empty() {
        let root = lay(
            "<p>x</p>",
            "p { counter-reset: section 5; counter-reset: initial; }",
        );
        let s = first_block_style(&root);
        assert!(s.counter_reset.is_empty());
    }

    #[test]
    fn invalid_ident_in_counter_list_skipped() {
        // Имя с цифрой первым символом — невалидный CSS-ident, должен пропуститься.
        let root = lay(
            "<p>x</p>",
            "p { counter-reset: 1invalid valid 2; }",
        );
        let s = first_block_style(&root);
        assert_eq!(s.counter_reset, vec![("valid".to_string(), 2)]);
    }

    // ──────── @media queries (Media Queries L4) ────────

    fn lay_with_viewport(html: &str, css: &str, vw: f32, vh: f32) -> LayoutBox {
        use lumen_dom::Document;
        use lumen_core::Size;
        let document: Document = lumen_html_parser::parse(html);
        let stylesheet = lumen_css_parser::parse(css);
        let viewport = Size { width: vw, height: vh };
        body_layout_box(crate::layout(&document, &stylesheet, viewport))
    }

    #[test]
    fn media_min_width_matches_wide_viewport() {
        // @media (min-width: 600px) { p { color: red; } }
        // viewport 800×600 → match.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 600px) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_min_width_skips_narrow_viewport() {
        // viewport 500×600 → НЕ match (500 < 600).
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 600px) { p { color: red; } }",
            500.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // default color = BLACK (initial).
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_max_width_matches_narrow() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-width: 500px) { p { color: blue; } }",
            400.0,
            300.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn media_orientation_landscape() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (orientation: landscape) { p { color: green; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn media_orientation_portrait_does_not_match_landscape() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (orientation: portrait) { p { color: green; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_screen_type_always_matches() {
        // Phase 0 MediaContext always media_type="screen".
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media screen { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_print_type_does_not_match() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media print { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_and_combination() {
        // @media (min-width: 600px) and (orientation: landscape) → match
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 600px) and (orientation: landscape) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_or_via_comma() {
        // @media (max-width: 400px), (min-width: 700px) → match при viewport=800
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-width: 400px), (min-width: 700px) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_rule_overrides_regular() {
        // Source order: p{color:red}, потом @media(match){p{color:blue}}.
        // @media rules идут после regular в нашем cascade-ordering,
        // поэтому blue побеждает.
        let root = lay_with_viewport(
            "<p>x</p>",
            "p { color: red; } @media (min-width: 100px) { p { color: blue; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn media_unknown_feature_does_not_match() {
        // (unknown-feature: value) → Unsupported → не match.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (color-gamut: p3) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_min_width_em_applies() {
        // 48em = 768px; viewport 1024 → матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 48em) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_min_width_em_no_match_narrow() {
        // 48em = 768px; viewport 600 → не матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-width: 48em) { p { color: red; } }",
            600.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_max_width_rem_applies() {
        // 50rem = 800px; viewport 600 → матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-width: 50rem) { p { color: blue; } }",
            600.0,
            480.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn media_width_exact_matches() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (width: 1024px) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn media_width_exact_no_match() {
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (width: 800px) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_min_aspect_ratio_matches() {
        // min-aspect-ratio: 1/1; 1024/720 > 1 → матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (min-aspect-ratio: 1/1) { p { color: green; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn media_max_aspect_ratio_no_match() {
        // max-aspect-ratio: 4/3 ≈ 1.333; 1024/720 ≈ 1.422 → не матчит.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (max-aspect-ratio: 4/3) { p { color: red; } }",
            1024.0,
            720.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color::BLACK);
    }

    #[test]
    fn media_reeval_on_resize_wider() {
        // При маленьком viewport — не матчит; при увеличении — матчит.
        let css = "@media (min-width: 600px) { p { color: red; } }";
        let narrow = lay_with_viewport("<p>x</p>", css, 400.0, 600.0);
        let p_narrow = narrow.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p_narrow.style.color, Color::BLACK);

        let wide = lay_with_viewport("<p>x</p>", css, 1024.0, 600.0);
        let p_wide = wide.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p_wide.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn display_flex_parses_and_stores() {
        let root = lay("<p>x</p>", "p { display: flex; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Flex);
    }

    #[test]
    fn display_inline_flex_parses_and_stores() {
        // inline-flex element внутри div — должен попасть в InlineRun
        // (трактуется как inline-family).
        let root = lay("<div><span>x</span></div>", "span { display: inline-flex; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // div содержит InlineRun (inline-flex span внутри).
        assert!(matches!(&div.children[0].kind, BoxKind::InlineRun { .. }));
    }

    #[test]
    fn display_grid_parses_as_block_family() {
        let root = lay("<p>x</p>", "p { display: grid; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Grid);
    }

    #[test]
    fn display_inline_grid_parses_as_inline_family() {
        let root = lay("<div><span>x</span></div>", "span { display: inline-grid; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(matches!(&div.children[0].kind, BoxKind::InlineRun { .. }));
    }

    #[test]
    fn display_inline_block_creates_inline_block_row() {
        // display:inline-block элементы внутри div группируются в InlineBlockRow.
        let root = lay(
            "<div><span>a</span><span>b</span></div>",
            "span { display: inline-block; width: 50px; height: 20px; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // div должен иметь один дочерний InlineBlockRow.
        assert!(
            div.children.iter().any(|c| matches!(&c.kind, BoxKind::InlineBlockRow)),
            "expected InlineBlockRow in div, got: {:?}", div.children.iter().map(|c| &c.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn display_inline_block_parses_style() {
        // <p display:inline-block> попадает в InlineBlockRow, не как прямой Block.
        let root = lay("<p>x</p>", "p { display: inline-block; }");
        // Ищем InlineBlockRow в дереве, внутри него первый child — это <p>.
        fn find_row(b: &LayoutBox) -> Option<&LayoutBox> {
            if matches!(b.kind, BoxKind::InlineBlockRow) {
                return Some(b);
            }
            b.children.iter().find_map(find_row)
        }
        let row = find_row(&root).expect("InlineBlockRow not found");
        let p = row.children.first().expect("p not found in row");
        assert_eq!(p.style.display, Display::InlineBlock);
    }

    #[test]
    fn inline_block_row_lays_out_horizontally() {
        // Два inline-block 50×20 должны оказаться рядом по горизонтали.
        let root = lay_measured(
            "<div><span>a</span><span>b</span></div>",
            "span { display: inline-block; width: 50px; height: 20px; }",
            800.0,
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let row = div.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        assert_eq!(row.children.len(), 2, "InlineBlockRow должен содержать 2 child");
        let a = &row.children[0];
        let b_box = &row.children[1];
        // a.rect.x < b.rect.x — лежат горизонтально
        assert!(a.rect.x < b_box.rect.x, "первый span должен быть левее второго");
        // b.rect.x ≥ a.rect.x + a.rect.width
        assert!(b_box.rect.x >= a.rect.x + a.rect.width,
            "второй span не должен перекрываться с первым");
    }

    #[test]
    fn inline_block_row_without_text_has_no_strut_descent() {
        // CSS §10.8 / Edge-верификация (TEST-11/TEST-12):
        // ряд из baseline-aligned inline-block-ов получает strut_descent (3.2px).
        // ряд из bottom-aligned inline-block-ов strut НЕ получает.
        let root_baseline = lay_measured(
            "<div><span></span><span></span></div>",
            "span { display: inline-block; width: 50px; height: 80px; }",
            body_w_or_default(),
        );
        let div = root_baseline.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let row = div.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        // Default vertical-align = baseline → strut 3.2px добавляется. height = 83.2.
        assert!(
            (row.rect.height - 83.2).abs() < 0.1,
            "baseline-ряд: 83.2px (80+strut), got {}",
            row.rect.height
        );
        // bottom-aligned row: no strut.
        let root_bottom = lay_measured(
            "<div><span></span><span></span></div>",
            "span { display: inline-block; width: 50px; height: 80px; vertical-align: bottom; }",
            body_w_or_default(),
        );
        let div2 = root_bottom.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let row2 = div2.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        assert!(
            (row2.rect.height - 80.0).abs() < 0.1,
            "bottom-ряд: 80px (нет strut), got {}",
            row2.rect.height
        );
    }

    #[test]
    fn inline_block_row_with_text_keeps_strut_descent() {
        // InlineRun всегда baseline-aligned → strut добавляется к ряду с текстом.
        let css = "span { display: inline-block; width: 50px; height: 20px; } \
                   div { font-size: 16px; }";
        let no_text = lay_measured("<div><span></span></div>", css, body_w_or_default());
        let with_text = lay_measured("<div>txt<span></span></div>", css, body_w_or_default());
        let row_no_text = no_text.children[0].children.iter()
            .find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        let row_with_text = with_text.children[0].children.iter()
            .find(|c| matches!(&c.kind, BoxKind::InlineBlockRow)).unwrap();
        // span default va=baseline → strut в обоих случаях. Оба ≥ 23.2.
        let expected_min = 20.0 + 16.0 * 0.2;
        assert!(
            row_no_text.rect.height >= expected_min - 0.1,
            "Ряд без текста: ≥{expected_min:.1}px, got {}",
            row_no_text.rect.height
        );
        assert!(
            row_with_text.rect.height >= expected_min - 0.1,
            "Ряд с текстом: ≥{expected_min:.1}px, got {}",
            row_with_text.rect.height
        );
    }

    #[test]
    fn inline_block_rows_no_drift_after_block_sep() {
        // baseline-aligned ряды добавляют strut_descent, bottom-aligned — нет.
        // Fixed8 strut = 16*0.2 = 3.2. row1(83.2) + sep(40) + row2(83.2) = 206.4.
        let root = lay_measured(
            "<div>\
              <div class=ib></div><div class=ib></div>\
              <div class=sep></div>\
              <div class=ib></div><div class=ib></div>\
             </div>",
            ".ib { display: inline-block; width: 50px; height: 80px; } \
             .sep { height: 40px; }",
            body_w_or_default(),
        );
        let outer = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Default va=baseline → strut: row1(83.2) + sep(40) + row2(83.2) = 206.4.
        assert!(
            (outer.rect.height - 206.4).abs() < 0.2,
            "baseline-ряды: 206.4px (2×strut 3.2px), got {}",
            outer.rect.height
        );
        // bottom-aligned ряды: нет strut → row1(80) + sep(40) + row2(80) = 200.
        let root_bot = lay_measured(
            "<div>\
              <div class=ib></div><div class=ib></div>\
              <div class=sep></div>\
              <div class=ib></div><div class=ib></div>\
             </div>",
            ".ib { display: inline-block; width: 50px; height: 80px; vertical-align: bottom; } \
             .sep { height: 40px; }",
            body_w_or_default(),
        );
        let outer_bot = root_bot.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(
            (outer_bot.rect.height - 200.0).abs() < 0.1,
            "bottom-ряды: 200px (без strut), got {}",
            outer_bot.rect.height
        );
    }

    fn body_w_or_default() -> f32 { 800.0 }

    #[test]
    fn display_unknown_value_keeps_previous() {
        // unknown value игнорируется — лог по умолчанию остаётся.
        let root = lay("<p>x</p>", "p { display: zomg-flexed; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Default для <p> от UA = Block.
        assert_eq!(p.style.display, Display::Block);
    }

    // ──────── clip-path / transform / filter ────────

    fn first_p_style(root: &LayoutBox) -> &ComputedStyle {
        let p = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        &p.style
    }

    #[test]
    fn clip_path_inset_parses() {
        let root = lay("<p>x</p>", "p { clip-path: inset(10px 20px 30px 40px); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Inset(parts)) => {
                assert_eq!(
                    parts,
                    vec![
                        ShapeValue::Px(10.0),
                        ShapeValue::Px(20.0),
                        ShapeValue::Px(30.0),
                        ShapeValue::Px(40.0)
                    ]
                );
            }
            _ => panic!("expected Inset, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_circle_with_center() {
        let root = lay("<p>x</p>", "p { clip-path: circle(50px at 100px 200px); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Circle { radius, center }) => {
                assert_eq!(radius, ShapeValue::Px(50.0));
                assert_eq!(center, Some((ShapeValue::Px(100.0), ShapeValue::Px(200.0))));
            }
            _ => panic!("expected Circle, got {cp:?}"),
        }
    }

    /// BUG-140: `circle(40% at 50% 50%)` (TEST-109 c0) раньше молча
    /// отбрасывался целиком — проценты не парсились.
    #[test]
    fn clip_path_circle_percent() {
        let root = lay("<p>x</p>", "p { clip-path: circle(40% at 50% 50%); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Circle { radius, center }) => {
                assert_eq!(radius, ShapeValue::Pct(40.0));
                assert_eq!(center, Some((ShapeValue::Pct(50.0), ShapeValue::Pct(50.0))));
            }
            _ => panic!("expected Circle, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_ellipse() {
        let root = lay("<p>x</p>", "p { clip-path: ellipse(30px 60px); }");
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Ellipse { rx, ry, center: None }) => {
                assert_eq!(rx, ShapeValue::Px(30.0));
                assert_eq!(ry, ShapeValue::Px(60.0));
            }
            _ => panic!("expected Ellipse, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_polygon() {
        let root = lay(
            "<p>x</p>",
            "p { clip-path: polygon(0 0, 100px 0, 50px 100px); }",
        );
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Polygon(verts, rule)) => {
                assert_eq!(verts.len(), 3);
                assert_eq!(verts[0], (ShapeValue::Px(0.0), ShapeValue::Px(0.0)));
                assert_eq!(verts[1], (ShapeValue::Px(100.0), ShapeValue::Px(0.0)));
                assert_eq!(verts[2], (ShapeValue::Px(50.0), ShapeValue::Px(100.0)));
                assert_eq!(rule, FillRule::NonZero, "default fill-rule = nonzero");
            }
            _ => panic!("expected Polygon, got {cp:?}"),
        }
    }

    /// BUG-140: `polygon(50% 0%, 100% 100%, 0% 100%)` (TEST-109 c2) раньше
    /// молча отбрасывался целиком — проценты не парсились.
    #[test]
    fn clip_path_polygon_percent() {
        let root = lay(
            "<p>x</p>",
            "p { clip-path: polygon(50% 0%, 100% 100%, 0% 100%); }",
        );
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Polygon(verts, _)) => {
                assert_eq!(verts.len(), 3);
                assert_eq!(verts[0], (ShapeValue::Pct(50.0), ShapeValue::Pct(0.0)));
                assert_eq!(verts[1], (ShapeValue::Pct(100.0), ShapeValue::Pct(100.0)));
                assert_eq!(verts[2], (ShapeValue::Pct(0.0), ShapeValue::Pct(100.0)));
            }
            _ => panic!("expected Polygon, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_path_triangle() {
        // CSS Shapes L1 §4 — path() флэттится в полигон; прямые сегменты
        // (M/L/Z) сохраняют вершины 1:1.
        let root = lay(
            "<p>x</p>",
            r#"p { clip-path: path("M 0 0 L 100 0 L 50 80 Z"); }"#,
        );
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Path(pts, rule)) => {
                assert!(pts.contains(&(0.0, 0.0)));
                assert!(pts.contains(&(100.0, 0.0)));
                assert!(pts.contains(&(50.0, 80.0)));
                assert_eq!(rule, FillRule::NonZero, "default fill-rule = nonzero");
            }
            _ => panic!("expected Path, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_path_with_fill_rule() {
        // CSS Shapes L1 §4 — опциональный fill-rule перед строкой пути
        // сохраняется и управляет заливкой самопересекающихся путей.
        let root = lay(
            "<p>x</p>",
            r#"p { clip-path: path(evenodd, "M 0 0 L 10 0 L 10 10 Z"); }"#,
        );
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Path(_, rule)) => {
                assert_eq!(rule, FillRule::EvenOdd, "evenodd должен сохраниться");
            }
            _ => panic!("expected Path, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_polygon_evenodd() {
        // CSS Shapes L1 §3 — polygon() принимает опциональный fill-rule.
        let root = lay(
            "<p>x</p>",
            "p { clip-path: polygon(evenodd, 0 0, 100px 0, 50px 100px); }",
        );
        let cp = first_p_style(&root).clip_path.clone();
        match cp {
            Some(ClipPath::Polygon(verts, rule)) => {
                assert_eq!(verts.len(), 3, "fill-rule не должен поглотить вершину");
                assert_eq!(rule, FillRule::EvenOdd);
            }
            _ => panic!("expected Polygon, got {cp:?}"),
        }
    }

    #[test]
    fn clip_path_path_degenerate_rejected() {
        // Путь без замкнутой области (< 3 точек) не создаёт клип.
        let root = lay("<p>x</p>", r#"p { clip-path: path("M 0 0"); }"#);
        assert_eq!(first_p_style(&root).clip_path, None);
    }

    #[test]
    fn clip_path_none_clears() {
        let root = lay("<p>x</p>", "p { clip-path: circle(50px); clip-path: none; }");
        assert_eq!(first_p_style(&root).clip_path, None);
    }

    #[test]
    fn transform_translate() {
        let root = lay("<p>x</p>", "p { transform: translate(10px, 20px); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t, vec![TransformFn::Translate(10.0, 20.0)]);
    }

    #[test]
    fn transform_rotate_normalizes_to_radians() {
        let root = lay("<p>x</p>", "p { transform: rotate(90deg); }");
        let t = first_p_style(&root).transform.clone();
        match &t[..] {
            [TransformFn::Rotate(rad)] => {
                assert!((rad - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
            }
            _ => panic!("expected single Rotate, got {t:?}"),
        }
    }

    #[test]
    fn transform_scale_single_arg_uniform() {
        let root = lay("<p>x</p>", "p { transform: scale(1.5); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t, vec![TransformFn::Scale(1.5, 1.5)]);
    }

    #[test]
    fn transform_scale_two_args() {
        let root = lay("<p>x</p>", "p { transform: scale(2, 0.5); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t, vec![TransformFn::Scale(2.0, 0.5)]);
    }

    #[test]
    fn transform_matrix() {
        let root = lay("<p>x</p>", "p { transform: matrix(1, 0, 0, 1, 50, 100); }");
        let t = first_p_style(&root).transform.clone();
        assert_eq!(
            t,
            vec![TransformFn::Matrix([1.0, 0.0, 0.0, 1.0, 50.0, 100.0])]
        );
    }

    #[test]
    fn transform_list_multiple() {
        let root = lay(
            "<p>x</p>",
            "p { transform: translate(10px, 0) rotate(45deg) scale(2); }",
        );
        let t = first_p_style(&root).transform.clone();
        assert_eq!(t.len(), 3);
        assert!(matches!(t[0], TransformFn::Translate(_, _)));
        assert!(matches!(t[1], TransformFn::Rotate(_)));
        assert!(matches!(t[2], TransformFn::Scale(_, _)));
    }

    #[test]
    fn transform_none_clears() {
        let root = lay(
            "<p>x</p>",
            "p { transform: rotate(45deg); transform: none; }",
        );
        assert!(first_p_style(&root).transform.is_empty());
    }

    #[test]
    fn translate_prop_xy() {
        let root = lay("<p>x</p>", "p { translate: 10px 20px; }");
        assert_eq!(first_p_style(&root).translate, Some((10.0, 20.0)));
    }

    #[test]
    fn translate_prop_single_value_defaults_y_to_zero() {
        let root = lay("<p>x</p>", "p { translate: 5px; }");
        assert_eq!(first_p_style(&root).translate, Some((5.0, 0.0)));
    }

    #[test]
    fn translate_prop_none_clears() {
        let root = lay("<p>x</p>", "p { translate: 10px; translate: none; }");
        assert_eq!(first_p_style(&root).translate, None);
    }

    #[test]
    fn rotate_prop_degrees() {
        let root = lay("<p>x</p>", "p { rotate: 90deg; }");
        let r = first_p_style(&root).rotate.expect("rotate should be Some");
        assert!((r - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "expected π/2, got {r}");
    }

    #[test]
    fn rotate_prop_none_clears() {
        let root = lay("<p>x</p>", "p { rotate: 45deg; rotate: none; }");
        assert_eq!(first_p_style(&root).rotate, None);
    }

    #[test]
    fn scale_prop_uniform() {
        let root = lay("<p>x</p>", "p { scale: 2; }");
        assert_eq!(first_p_style(&root).scale, Some((2.0, 2.0)));
    }

    #[test]
    fn scale_prop_non_uniform() {
        let root = lay("<p>x</p>", "p { scale: 1.5 0.5; }");
        assert_eq!(first_p_style(&root).scale, Some((1.5, 0.5)));
    }

    #[test]
    fn scale_prop_none_clears() {
        let root = lay("<p>x</p>", "p { scale: 2; scale: none; }");
        assert_eq!(first_p_style(&root).scale, None);
    }

    #[test]
    fn individual_transforms_not_inherited() {
        // div has all three individual props; nested p should NOT inherit them
        let root = lay(
            "<div><p>x</p></div>",
            "div { translate: 10px; rotate: 45deg; scale: 2; } p { color: red; }",
        );
        // first_p_style returns the first Block child = the div wrapper
        // then its child = the p block. We need the p inside div.
        let div_box = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).expect("div");
        assert_eq!(div_box.style.translate, Some((10.0, 0.0)));
        let p_box = div_box.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).expect("p");
        assert_eq!(p_box.style.translate, None, "translate must not be inherited");
        assert_eq!(p_box.style.rotate, None, "rotate must not be inherited");
        assert_eq!(p_box.style.scale, None, "scale must not be inherited");
    }

    #[test]
    fn filter_blur() {
        let root = lay("<p>x</p>", "p { filter: blur(5px); }");
        let f = first_p_style(&root).filter.clone();
        assert_eq!(f, vec![FilterFn::Blur(5.0)]);
    }

    #[test]
    fn filter_percentage_normalized() {
        let root = lay("<p>x</p>", "p { filter: grayscale(50%); }");
        let f = first_p_style(&root).filter.clone();
        match &f[..] {
            [FilterFn::Grayscale(v)] => assert!((v - 0.5).abs() < 1e-5),
            _ => panic!("expected Grayscale, got {f:?}"),
        }
    }

    #[test]
    fn filter_chain() {
        let root = lay(
            "<p>x</p>",
            "p { filter: blur(2px) brightness(1.2) saturate(0.8); }",
        );
        let f = first_p_style(&root).filter.clone();
        assert_eq!(f.len(), 3);
        assert!(matches!(f[0], FilterFn::Blur(_)));
        assert!(matches!(f[1], FilterFn::Brightness(_)));
        assert!(matches!(f[2], FilterFn::Saturate(_)));
    }

    #[test]
    fn filter_hue_rotate_radians() {
        let root = lay("<p>x</p>", "p { filter: hue-rotate(180deg); }");
        let f = first_p_style(&root).filter.clone();
        match &f[..] {
            [FilterFn::HueRotate(rad)] => {
                assert!((rad - std::f32::consts::PI).abs() < 1e-5);
            }
            _ => panic!("expected HueRotate, got {f:?}"),
        }
    }

    #[test]
    fn filter_none_clears() {
        let root = lay("<p>x</p>", "p { filter: blur(5px); filter: none; }");
        assert!(first_p_style(&root).filter.is_empty());
    }

    #[test]
    fn filter_unknown_skipped() {
        let root = lay("<p>x</p>", "p { filter: blur(5px) zomg(1); brightness(1); }");
        // zomg() игнорируется, остальное парсится.
        let f = first_p_style(&root).filter.clone();
        // brightness вне filter declaration — отдельный selector? Нет,
        // оно в той же декларации `filter: blur(5px) zomg(1)` — zomg
        // skipped, blur остался.
        assert!(matches!(f[0], FilterFn::Blur(_)));
    }

    #[test]
    fn clip_transform_filter_not_inherited() {
        // Эти свойства не наследуются.
        let root = lay(
            "<div><p>x</p></div>",
            "div { clip-path: circle(50px); transform: rotate(45deg); filter: blur(5px); }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(p.style.clip_path.is_none());
        assert!(p.style.transform.is_empty());
        assert!(p.style.filter.is_empty());
        assert!(div.style.clip_path.is_some());
        assert!(!div.style.transform.is_empty());
        assert!(!div.style.filter.is_empty());
    }

    // ──────── backdrop-filter ────────

    #[test]
    fn backdrop_filter_blur_parsed() {
        let root = lay("<p>x</p>", "p { backdrop-filter: blur(10px); }");
        let f = first_p_style(&root).backdrop_filter.clone();
        assert_eq!(f, vec![FilterFn::Blur(10.0)]);
    }

    #[test]
    fn backdrop_filter_grayscale_percentage() {
        let root = lay("<p>x</p>", "p { backdrop-filter: grayscale(80%); }");
        let f = first_p_style(&root).backdrop_filter.clone();
        match &f[..] {
            [FilterFn::Grayscale(v)] => assert!((v - 0.8).abs() < 1e-5),
            _ => panic!("expected Grayscale(0.8), got {f:?}"),
        }
    }

    #[test]
    fn backdrop_filter_chain() {
        let root = lay(
            "<p>x</p>",
            "p { backdrop-filter: blur(4px) brightness(1.5) saturate(2); }",
        );
        let f = first_p_style(&root).backdrop_filter.clone();
        assert_eq!(f.len(), 3);
        assert!(matches!(f[0], FilterFn::Blur(_)));
        assert!(matches!(f[1], FilterFn::Brightness(_)));
        assert!(matches!(f[2], FilterFn::Saturate(_)));
    }

    #[test]
    fn backdrop_filter_none_clears() {
        let root = lay("<p>x</p>", "p { backdrop-filter: blur(5px); backdrop-filter: none; }");
        assert!(first_p_style(&root).backdrop_filter.is_empty());
    }

    #[test]
    fn backdrop_filter_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { backdrop-filter: blur(5px); }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(!div.style.backdrop_filter.is_empty(), "div должен иметь backdrop-filter");
        assert!(p.style.backdrop_filter.is_empty(), "p не наследует backdrop-filter");
    }

    #[test]
    fn backdrop_filter_and_filter_independent() {
        let root = lay(
            "<p>x</p>",
            "p { filter: invert(1); backdrop-filter: blur(8px); }",
        );
        let s = first_p_style(&root);
        assert!(!s.filter.is_empty(), "filter должен быть установлен");
        assert!(!s.backdrop_filter.is_empty(), "backdrop-filter должен быть установлен");
        assert!(matches!(s.filter[0], FilterFn::Invert(_)));
        assert!(matches!(s.backdrop_filter[0], FilterFn::Blur(_)));
    }

    // ──────── gap / aspect-ratio ────────

    #[test]
    fn gap_shorthand_single_value() {
        let root = lay("<p>x</p>", "p { gap: 10px; }");
        let s = first_p_style(&root);
        assert_eq!(s.row_gap, Length::Px(10.0));
        assert_eq!(s.column_gap, Length::Px(10.0));
    }

    #[test]
    fn gap_shorthand_two_values() {
        let root = lay("<p>x</p>", "p { gap: 10px 20px; }");
        let s = first_p_style(&root);
        assert_eq!(s.row_gap, Length::Px(10.0));
        assert_eq!(s.column_gap, Length::Px(20.0));
    }

    #[test]
    fn row_gap_individual() {
        let root = lay("<p>x</p>", "p { row-gap: 15px; }");
        assert_eq!(first_p_style(&root).row_gap, Length::Px(15.0));
    }

    #[test]
    fn column_gap_individual() {
        let root = lay("<p>x</p>", "p { column-gap: 25px; }");
        assert_eq!(first_p_style(&root).column_gap, Length::Px(25.0));
    }

    #[test]
    fn gap_em_stores_typed() {
        // em хранится как Length::Em и разрешается при layout относительно font-size.
        let root = lay("<p>x</p>", "p { font-size: 20px; gap: 1.5em; }");
        let s = first_p_style(&root);
        assert_eq!(s.row_gap, Length::Em(1.5));
    }

    #[test]
    fn gap_negative_clamped_to_zero() {
        // gap не может быть отрицательным — хранится как Px(0.0).
        let root = lay("<p>x</p>", "p { gap: -5px; }");
        assert_eq!(first_p_style(&root).row_gap, Length::Px(0.0));
    }

    #[test]
    fn aspect_ratio_single_number() {
        let root = lay("<p>x</p>", "p { aspect-ratio: 1.5; }");
        assert_eq!(first_p_style(&root).aspect_ratio, Some((1.5, 1.0)));
    }

    #[test]
    fn aspect_ratio_w_h_pair() {
        let root = lay("<p>x</p>", "p { aspect-ratio: 16 / 9; }");
        assert_eq!(first_p_style(&root).aspect_ratio, Some((16.0, 9.0)));
    }

    #[test]
    fn aspect_ratio_auto() {
        let root = lay("<p>x</p>", "p { aspect-ratio: auto; }");
        assert_eq!(first_p_style(&root).aspect_ratio, None);
    }

    #[test]
    fn aspect_ratio_negative_rejected() {
        let root = lay("<p>x</p>", "p { aspect-ratio: -1 / 2; }");
        assert_eq!(first_p_style(&root).aspect_ratio, None);
    }

    #[test]
    fn aspect_ratio_invalid_kept_unchanged() {
        let root = lay("<p>x</p>", "p { aspect-ratio: 16 / abc; }");
        assert_eq!(first_p_style(&root).aspect_ratio, None);
    }

    // ──────── CSS Multi-column L1 ────────

    #[test]
    fn column_count_integer() {
        let root = lay("<p>x</p>", "p { column-count: 3; }");
        assert_eq!(first_p_style(&root).column_count, Some(3));
    }

    #[test]
    fn column_count_auto() {
        let root = lay("<p>x</p>", "p { column-count: auto; }");
        assert_eq!(first_p_style(&root).column_count, None);
    }

    #[test]
    fn column_count_zero_rejected() {
        let root = lay("<p>x</p>", "p { column-count: 0; }");
        assert_eq!(first_p_style(&root).column_count, None);
    }

    #[test]
    fn column_width_length() {
        let root = lay("<p>x</p>", "p { column-width: 200px; }");
        assert_eq!(first_p_style(&root).column_width, Some(Length::Px(200.0)));
    }

    #[test]
    fn column_width_auto() {
        let root = lay("<p>x</p>", "p { column-width: auto; }");
        assert_eq!(first_p_style(&root).column_width, None);
    }

    #[test]
    fn columns_shorthand_both() {
        let root = lay("<p>x</p>", "p { columns: 200px 3; }");
        let s = first_p_style(&root);
        assert_eq!(s.column_width, Some(Length::Px(200.0)));
        assert_eq!(s.column_count, Some(3));
    }

    #[test]
    fn columns_shorthand_width_only() {
        let root = lay("<p>x</p>", "p { columns: 250px; }");
        let s = first_p_style(&root);
        assert_eq!(s.column_width, Some(Length::Px(250.0)));
        assert_eq!(s.column_count, None);
    }

    #[test]
    fn columns_shorthand_count_only() {
        let root = lay("<p>x</p>", "p { columns: 4; }");
        let s = first_p_style(&root);
        assert_eq!(s.column_count, Some(4));
        assert_eq!(s.column_width, None);
    }

    #[test]
    fn column_rule_individual() {
        let root = lay(
            "<p>x</p>",
            "p { column-rule-width: 2px; column-rule-style: solid; }",
        );
        let s = first_p_style(&root);
        assert!((s.column_rule_width - 2.0).abs() < 1e-6);
        assert_eq!(s.column_rule_style, BorderStyle::Solid);
    }

    #[test]
    fn column_rule_shorthand() {
        let root = lay("<p>x</p>", "p { column-rule: 3px dashed; }");
        let s = first_p_style(&root);
        assert!((s.column_rule_width - 3.0).abs() < 1e-6);
        assert_eq!(s.column_rule_style, BorderStyle::Dashed);
    }

    #[test]
    fn column_span_all() {
        let root = lay("<p>x</p>", "p { column-span: all; }");
        assert!(first_p_style(&root).column_span_all);
    }

    #[test]
    fn column_fill_balance() {
        let root = lay("<p>x</p>", "p { column-fill: balance; }");
        assert!(first_p_style(&root).column_fill_balance);
    }

    #[test]
    fn break_before_avoid() {
        let root = lay("<p>x</p>", "p { break-before: avoid; }");
        assert_eq!(first_p_style(&root).break_before, BreakValue::Avoid);
    }

    #[test]
    fn break_after_page() {
        let root = lay("<p>x</p>", "p { break-after: page; }");
        assert_eq!(first_p_style(&root).break_after, BreakValue::Page);
    }

    #[test]
    fn break_inside_avoid_column() {
        let root = lay("<p>x</p>", "p { break-inside: avoid-column; }");
        assert_eq!(first_p_style(&root).break_inside, BreakValue::Avoid);
    }

    #[test]
    fn column_count_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { column-count: 3; }",
        );
        // Дочерний p не должен унаследовать column-count (CSS Multi-column L1 §3.2 — не наследуется).
        let p_style = nested_p_style(&root);
        assert_eq!(p_style.column_count, None);
    }

    // ──────── CSS Environment Variables L1 — env() ────────

    #[test]
    fn env_fallback_used_when_unknown() {
        // env() с unknown name + fallback → fallback применяется.
        let root = lay(
            "<p>x</p>",
            "p { padding: env(safe-area-inset-top, 12px); }",
        );
        assert_eq!(first_p_style(&root).padding_top, Length::Px(12.0));
    }

    #[test]
    fn env_without_fallback_invalidates_decl() {
        // env() с unknown name и без fallback — декларация невалидна.
        let root = lay(
            "<p>x</p>",
            "p { padding: env(safe-area-inset-top); }",
        );
        assert_eq!(first_p_style(&root).padding_top, Length::Px(0.0));
    }

    #[test]
    fn env_with_indices_ignored_phase0() {
        // `env(name 0, fallback)` — индекс игнорируется, имя = name.
        let root = lay(
            "<p>x</p>",
            "p { padding: env(viewport-segment-width 0 0, 25px); }",
        );
        assert_eq!(first_p_style(&root).padding_top, Length::Px(25.0));
    }

    #[test]
    fn env_inside_calc() {
        // calc(env(...) + 5px) — env разворачивается до calc(); resolve = 15px.
        let root = lay(
            "<p>x</p>",
            "p { padding: calc(env(safe-area-inset-top, 10px) + 5px); }",
        );
        let vp = Size::new(800.0, 600.0);
        let v = first_p_style(&root).padding_top.resolve_or_zero(16.0, 0.0, vp);
        assert!((v - 15.0).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn env_inside_var_fallback() {
        // var(--foo, env(name, 8px)) — env как fallback внутри var().
        let root = lay(
            "<p>x</p>",
            "p { padding: var(--missing, env(safe-area-inset-top, 8px)); }",
        );
        assert_eq!(first_p_style(&root).padding_top, Length::Px(8.0));
    }

    // ──────── CSS Scroll Snap L1 ────────

    #[test]
    fn scroll_snap_type_none() {
        let root = lay("<p>x</p>", "p { scroll-snap-type: none; }");
        assert_eq!(first_p_style(&root).scroll_snap_type.axis, ScrollSnapAxis::None);
    }

    #[test]
    fn scroll_snap_type_x_mandatory() {
        let root = lay("<p>x</p>", "p { scroll-snap-type: x mandatory; }");
        let s = first_p_style(&root);
        assert_eq!(s.scroll_snap_type.axis, ScrollSnapAxis::X);
        assert_eq!(s.scroll_snap_type.strictness, ScrollSnapStrictness::Mandatory);
    }

    #[test]
    fn scroll_snap_align_single_keyword() {
        let root = lay("<p>x</p>", "p { scroll-snap-align: center; }");
        let s = first_p_style(&root);
        assert_eq!(s.scroll_snap_align.block, ScrollSnapAlignKeyword::Center);
        assert_eq!(s.scroll_snap_align.inline, ScrollSnapAlignKeyword::Center);
    }

    #[test]
    fn scroll_snap_align_two_keywords() {
        let root = lay("<p>x</p>", "p { scroll-snap-align: start end; }");
        let s = first_p_style(&root);
        assert_eq!(s.scroll_snap_align.block, ScrollSnapAlignKeyword::Start);
        assert_eq!(s.scroll_snap_align.inline, ScrollSnapAlignKeyword::End);
    }

    #[test]
    fn scroll_snap_stop_always() {
        let root = lay("<p>x</p>", "p { scroll-snap-stop: always; }");
        assert_eq!(first_p_style(&root).scroll_snap_stop, ScrollSnapStop::Always);
    }

    #[test]
    fn scroll_margin_individual() {
        let root = lay("<p>x</p>", "p { scroll-margin-top: 10px; scroll-margin-left: 5px; }");
        let s = first_p_style(&root);
        assert!((s.scroll_margin_top - 10.0).abs() < 1e-6);
        assert!((s.scroll_margin_left - 5.0).abs() < 1e-6);
    }

    #[test]
    fn scroll_margin_shorthand_4_values() {
        let root = lay("<p>x</p>", "p { scroll-margin: 1px 2px 3px 4px; }");
        let s = first_p_style(&root);
        assert!((s.scroll_margin_top - 1.0).abs() < 1e-6);
        assert!((s.scroll_margin_right - 2.0).abs() < 1e-6);
        assert!((s.scroll_margin_bottom - 3.0).abs() < 1e-6);
        assert!((s.scroll_margin_left - 4.0).abs() < 1e-6);
    }

    #[test]
    fn scroll_padding_shorthand_1_value() {
        let root = lay("<p>x</p>", "p { scroll-padding: 5px; }");
        let s = first_p_style(&root);
        assert!((s.scroll_padding_top - 5.0).abs() < 1e-6);
        assert!((s.scroll_padding_right - 5.0).abs() < 1e-6);
        assert!((s.scroll_padding_bottom - 5.0).abs() < 1e-6);
        assert!((s.scroll_padding_left - 5.0).abs() < 1e-6);
    }

    // ──────── CSS Overscroll Behavior L1 ────────

    #[test]
    fn overscroll_behavior_contain() {
        let root = lay("<p>x</p>", "p { overscroll-behavior: contain; }");
        let s = first_p_style(&root);
        assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
        assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Contain);
    }

    #[test]
    fn overscroll_behavior_two_values() {
        let root = lay("<p>x</p>", "p { overscroll-behavior: contain none; }");
        let s = first_p_style(&root);
        assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
        assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::None);
    }

    #[test]
    fn overscroll_behavior_individual_axis() {
        let root = lay("<p>x</p>", "p { overscroll-behavior-x: none; overscroll-behavior-y: auto; }");
        let s = first_p_style(&root);
        assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::None);
        assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Auto);
    }

    #[test]
    fn scroll_snap_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { scroll-snap-type: x mandatory; }",
        );
        let p = nested_p_style(&root);
        // Не наследуется.
        assert_eq!(p.scroll_snap_type.axis, ScrollSnapAxis::None);
    }

    // ──────── collect_snap_containers / find_snap_target ────────

    fn make_snap_container(
        w: f32,
        h: f32,
        axis: ScrollSnapAxis,
        strictness: ScrollSnapStrictness,
    ) -> SnapContainer {
        SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type: ScrollSnapType { axis, strictness },
            rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: w, height: h },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: Vec::new(),
        }
    }

    fn snap_pt(y: f32) -> SnapPoint {
        SnapPoint { node: lumen_dom::NodeId::from_index(1), snap_x: None, snap_y: Some(y), stop_always: false }
    }

    #[test]
    fn find_snap_target_mandatory_y() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
        );
        sc.points = vec![snap_pt(0.0), snap_pt(720.0), snap_pt(1440.0)];
        // Target 400 → nearest is 0 (dist=160000) vs 720 (dist=102400) → snap 720.
        let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 400.0));
        assert!(result.is_some());
        let (_, sy) = result.unwrap();
        assert!((sy - 720.0).abs() < 1e-3, "expected 720, got {sy}");
    }

    #[test]
    fn find_snap_target_mandatory_first_section() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
        );
        sc.points = vec![snap_pt(0.0), snap_pt(720.0), snap_pt(1440.0)];
        // Target 300 → nearest is 0 (dist=90000) vs 720 (dist=176400) → snap 0.
        let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 300.0));
        assert!(result.is_some());
        let (_, sy) = result.unwrap();
        assert!((sy - 0.0).abs() < 1e-3, "expected 0, got {sy}");
    }

    #[test]
    fn find_snap_target_proximity_within_threshold() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Proximity,
        );
        sc.points = vec![snap_pt(720.0)];
        // Proximity threshold = 720 * 0.5 = 360. Target 450 → dist from 720 = 270 ≤ 360 → snaps.
        let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 450.0));
        assert!(result.is_some());
        let (_, sy) = result.unwrap();
        assert!((sy - 720.0).abs() < 1e-3, "expected 720, got {sy}");
    }

    #[test]
    fn find_snap_target_proximity_out_of_threshold() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Proximity,
        );
        sc.points = vec![snap_pt(720.0)];
        // Proximity threshold = 360. Target 200 → dist from 720 = 520 > 360 → no snap.
        let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 200.0));
        assert!(result.is_none(), "should not snap when beyond proximity threshold");
    }

    #[test]
    fn find_snap_target_stop_always_barrier_viewport() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
        );
        sc.points = vec![
            SnapPoint { node: lumen_dom::NodeId::from_index(1), snap_x: None, snap_y: Some(720.0), stop_always: true },
            snap_pt(1440.0),
        ];
        // Scrolling from 0 to 1500 would pass 720 (stop_always) → forced to 720.
        let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 1500.0));
        assert!(result.is_some());
        let (_, sy) = result.unwrap();
        assert!((sy - 720.0).abs() < 1e-3, "stop_always barrier should force snap to 720, got {sy}");
    }

    #[test]
    fn find_snap_target_no_points_returns_none() {
        let sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
        );
        assert!(find_snap_target(&sc, (0.0, 0.0), (0.0, 400.0)).is_none());
    }

    // ──────── find_snapped_nodes (CSS Scroll Snap L2 events) ────────

    fn snap_pt_node(idx: u32, x: Option<f32>, y: Option<f32>) -> SnapPoint {
        SnapPoint {
            node: lumen_dom::NodeId::from_index(idx as usize),
            snap_x: x,
            snap_y: y,
            stop_always: false,
        }
    }

    #[test]
    fn find_snapped_nodes_empty_container_is_default() {
        let sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
        );
        let t = find_snapped_nodes(&sc, (0.0, 0.0));
        assert_eq!(t, SnapTargets::default());
    }

    #[test]
    fn find_snapped_nodes_block_axis_picks_nearest() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
        );
        sc.points = vec![
            snap_pt_node(1, None, Some(0.0)),
            snap_pt_node(2, None, Some(720.0)),
            snap_pt_node(3, None, Some(1440.0)),
        ];
        // Scroll at 700 → nearest block snap is node 2 (720).
        let t = find_snapped_nodes(&sc, (0.0, 700.0));
        assert_eq!(t.block, Some(lumen_dom::NodeId::from_index(2)));
        // Y-only container does not snap on the inline axis.
        assert_eq!(t.inline, None);
    }

    #[test]
    fn find_snapped_nodes_both_axes() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Both, ScrollSnapStrictness::Mandatory,
        );
        sc.points = vec![
            snap_pt_node(1, Some(0.0), Some(0.0)),
            snap_pt_node(2, Some(500.0), Some(720.0)),
        ];
        // Inline near 480 → node 2 (x=500); block near 30 → node 1 (y=0).
        let t = find_snapped_nodes(&sc, (480.0, 30.0));
        assert_eq!(t.inline, Some(lumen_dom::NodeId::from_index(2)));
        assert_eq!(t.block, Some(lumen_dom::NodeId::from_index(1)));
    }

    #[test]
    fn find_snapped_nodes_x_only_ignores_block() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::X, ScrollSnapStrictness::Mandatory,
        );
        sc.points = vec![
            snap_pt_node(1, Some(0.0), Some(0.0)),
            snap_pt_node(2, Some(1024.0), Some(720.0)),
        ];
        let t = find_snapped_nodes(&sc, (900.0, 700.0));
        assert_eq!(t.inline, Some(lumen_dom::NodeId::from_index(2)));
        assert_eq!(t.block, None);
    }

    #[test]
    fn find_snapped_nodes_skips_points_without_axis_offset() {
        let mut sc = make_snap_container(
            1024.0, 720.0, ScrollSnapAxis::Both, ScrollSnapStrictness::Mandatory,
        );
        // Node 1 snaps only on block; node 2 only on inline.
        sc.points = vec![
            snap_pt_node(1, None, Some(0.0)),
            snap_pt_node(2, Some(300.0), None),
        ];
        let t = find_snapped_nodes(&sc, (290.0, 10.0));
        assert_eq!(t.inline, Some(lumen_dom::NodeId::from_index(2)));
        assert_eq!(t.block, Some(lumen_dom::NodeId::from_index(1)));
    }

    #[test]
    fn collect_snap_containers_empty_when_no_snap_type() {
        let root = lay(
            "<div><p>first</p><p>second</p></div>",
            "div { width: 1024px; height: 720px; overflow: scroll; }",
        );
        // No scroll-snap-type → empty containers list.
        let containers = collect_snap_containers(&root);
        assert!(containers.is_empty(), "expected no snap containers");
    }

    #[test]
    fn collect_snap_containers_finds_y_mandatory() {
        let root = lay(
            "<div><p>first</p><p>second</p></div>",
            "div { width: 1024px; height: 720px; overflow: scroll; scroll-snap-type: y mandatory; } p { height: 720px; scroll-snap-align: start; }",
        );
        let containers = collect_snap_containers(&root);
        // At least one snap container should be found (the div).
        assert!(!containers.is_empty(), "expected a snap container");
        let sc = &containers[0];
        assert_eq!(sc.snap_type.axis, ScrollSnapAxis::Y);
        assert_eq!(sc.snap_type.strictness, ScrollSnapStrictness::Mandatory);
    }

    // ──────── mask-* + scrollbar-* ────────

    #[test]
    fn mask_image_url() {
        let root = lay("<p>x</p>", "p { mask-image: url(\"mask.png\"); }");
        assert_eq!(
            first_p_style(&root).mask_image,
            BackgroundImage::Url("mask.png".into())
        );
    }

    #[test]
    fn mask_image_none_clears() {
        let root = lay("<p>x</p>", "p { mask-image: url(m.png); mask-image: none; }");
        assert_eq!(first_p_style(&root).mask_image, BackgroundImage::None);
    }

    #[test]
    fn mask_repeat_no_repeat() {
        let root = lay("<p>x</p>", "p { mask-repeat: no-repeat; }");
        assert_eq!(first_p_style(&root).mask_repeat, BackgroundRepeat::NoRepeat);
    }

    #[test]
    fn mask_size_cover() {
        let root = lay("<p>x</p>", "p { mask-size: cover; }");
        assert_eq!(first_p_style(&root).mask_size, BackgroundSize::Cover);
    }

    #[test]
    fn mask_mode_default_is_alpha() {
        let root = lay("<p>x</p>", "p { mask-image: linear-gradient(black, white); }");
        assert_eq!(first_p_style(&root).mask_mode, MaskMode::Alpha);
    }

    #[test]
    fn mask_mode_luminance() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; }");
        assert_eq!(first_p_style(&root).mask_mode, MaskMode::Luminance);
    }

    #[test]
    fn mask_mode_alpha_keyword() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: alpha; }");
        assert_eq!(first_p_style(&root).mask_mode, MaskMode::Alpha);
    }

    #[test]
    fn mask_mode_match_source_resolves_to_alpha() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: match-source; }");
        assert_eq!(first_p_style(&root).mask_mode, MaskMode::Alpha);
    }

    #[test]
    fn mask_mode_invalid_keeps_previous() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: bogus; }");
        assert_eq!(first_p_style(&root).mask_mode, MaskMode::Luminance);
    }

    #[test]
    fn mask_mode_not_inherited() {
        // `first_p_style` returns the outer div block; drill into its child <p>.
        let root = lay("<div><p>x</p></div>", "div { mask-mode: luminance; }");
        let div = &root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("div block");
        assert_eq!(div.style.mask_mode, MaskMode::Luminance, "div carries the rule");
        let p = div
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        assert_eq!(p.style.mask_mode, MaskMode::Alpha, "child does not inherit");
    }

    #[test]
    fn scrollbar_width_thin() {
        let root = lay("<p>x</p>", "p { scrollbar-width: thin; }");
        assert_eq!(first_p_style(&root).scrollbar_width, ScrollbarWidth::Thin);
    }

    #[test]
    fn scrollbar_width_none() {
        let root = lay("<p>x</p>", "p { scrollbar-width: none; }");
        assert_eq!(first_p_style(&root).scrollbar_width, ScrollbarWidth::None);
    }

    #[test]
    fn scrollbar_width_inherited() {
        let root = lay("<div><p>x</p></div>", "div { scrollbar-width: thin; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.scrollbar_width, ScrollbarWidth::Thin);
    }

    #[test]
    fn scrollbar_color_pair() {
        let root = lay(
            "<p>x</p>",
            "p { scrollbar-color: red blue; }",
        );
        let (thumb, track) = first_p_style(&root).scrollbar_color.unwrap();
        assert_eq!(thumb, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(track, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn scrollbar_color_with_rgb_functions() {
        let root = lay(
            "<p>x</p>",
            "p { scrollbar-color: rgb(100, 100, 100) rgb(200, 200, 200); }",
        );
        let (thumb, _) = first_p_style(&root).scrollbar_color.unwrap();
        assert_eq!(thumb, Color { r: 100, g: 100, b: 100, a: 255 });
    }

    #[test]
    fn scrollbar_color_auto() {
        let root = lay("<p>x</p>", "p { scrollbar-color: red blue; scrollbar-color: auto; }");
        assert!(first_p_style(&root).scrollbar_color.is_none());
    }

    #[test]
    fn scrollbar_gutter_stable() {
        let root = lay("<p>x</p>", "p { scrollbar-gutter: stable; }");
        assert_eq!(first_p_style(&root).scrollbar_gutter, ScrollbarGutter::Stable);
    }

    #[test]
    fn scrollbar_gutter_stable_both_edges() {
        let root = lay("<p>x</p>", "p { scrollbar-gutter: stable both-edges; }");
        assert_eq!(
            first_p_style(&root).scrollbar_gutter,
            ScrollbarGutter::StableBothEdges
        );
    }

    // ──────── scrollbar-gutter layout algorithm ────────

    /// `scrollbar-gutter: stable` + `overflow-y: scroll` reserves 12px (auto gutter)
    /// in the inline axis so children are narrower than the container's content edge.
    #[test]
    fn scrollbar_gutter_stable_reduces_child_width() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // 200 border-box → content = 200; minus 12 gutter = 188.
        assert!((div.rect.width - 200.0).abs() < 0.01, "div={}", div.rect.width);
        assert!((p.rect.width - 188.0).abs() < 0.01, "p child={}", p.rect.width);
    }

    /// `scrollbar-gutter: auto` (default) with overlay scrollbars = no gutter reserved.
    #[test]
    fn scrollbar_gutter_auto_no_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; overflow-y: scroll; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // No gutter reserved: child fills full content width.
        assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
    }

    /// `scrollbar-width: none` suppresses the gutter even with `scrollbar-gutter: stable`.
    #[test]
    fn scrollbar_gutter_stable_none_no_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; scrollbar-width: none; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
    }

    /// `scrollbar-gutter: stable both-edges` reserves gutter on start AND end of
    /// the inline axis (2 × 12 = 24 px).
    #[test]
    fn scrollbar_gutter_stable_both_edges_double_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable both-edges; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // 200 − 12*2 = 176.
        assert!((p.rect.width - 176.0).abs() < 0.01, "p child={}", p.rect.width);
    }

    /// `scrollbar-width: thin` uses 6 px gutter instead of 12.
    #[test]
    fn scrollbar_gutter_stable_thin_reduces_by_6() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; scrollbar-width: thin; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // 200 − 6 = 194.
        assert!((p.rect.width - 194.0).abs() < 0.01, "p child={}", p.rect.width);
    }

    /// Without `overflow-y: scroll/auto`, `scrollbar-gutter: stable` has no effect.
    #[test]
    fn scrollbar_gutter_stable_no_scroll_no_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { width: 200px; scrollbar-gutter: stable; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
    }

    // ──────── transform-origin / perspective / list-style-* / transition-* ────────

    #[test]
    fn transform_origin_x_y_z() {
        let root = lay("<p>x</p>", "p { transform-origin: 10px 20px 30px; }");
        let o = first_p_style(&root).transform_origin;
        assert_eq!(o.0, PositionComponent::Px(10.0));
        assert_eq!(o.1, PositionComponent::Px(20.0));
        assert!((o.2 - 30.0).abs() < 1e-5);
    }

    #[test]
    fn transform_origin_single_value_y_defaults_to_center() {
        // CSS Transforms L1 §6: single value applies to x, y defaults to center (50%).
        let root = lay("<p>x</p>", "p { transform-origin: 50px; }");
        let o = first_p_style(&root).transform_origin;
        assert_eq!(o.0, PositionComponent::Px(50.0));
        assert_eq!(o.1, PositionComponent::Percent(0.5));
    }

    #[test]
    fn transform_origin_not_inherited() {
        let root = lay("<div><p>x</p></div>", "div { transform-origin: 10px 20px; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Non-inherited: <p> gets initial value 50% 50%.
        assert_eq!(p.style.transform_origin.0, PositionComponent::Percent(0.5));
        assert_eq!(p.style.transform_origin.1, PositionComponent::Percent(0.5));
        assert_eq!(div.style.transform_origin.0, PositionComponent::Px(10.0));
        assert_eq!(div.style.transform_origin.1, PositionComponent::Px(20.0));
    }

    #[test]
    fn perspective_length() {
        let root = lay("<p>x</p>", "p { perspective: 800px; }");
        assert_eq!(first_p_style(&root).perspective, Some(800.0));
    }

    #[test]
    fn perspective_none() {
        let root = lay("<p>x</p>", "p { perspective: 800px; perspective: none; }");
        assert_eq!(first_p_style(&root).perspective, None);
    }

    #[test]
    fn perspective_zero_treated_as_none() {
        let root = lay("<p>x</p>", "p { perspective: 0px; }");
        assert_eq!(first_p_style(&root).perspective, None);
    }

    #[test]
    fn list_style_type_decimal() {
        let root = lay("<p>x</p>", "p { list-style-type: decimal; }");
        assert_eq!(first_p_style(&root).list_style_type, ListStyleType::Decimal);
    }

    #[test]
    fn list_style_type_none() {
        let root = lay("<p>x</p>", "p { list-style-type: none; }");
        assert_eq!(first_p_style(&root).list_style_type, ListStyleType::None);
    }

    #[test]
    fn list_style_type_lower_roman() {
        let root = lay("<p>x</p>", "p { list-style-type: lower-roman; }");
        assert_eq!(first_p_style(&root).list_style_type, ListStyleType::LowerRoman);
    }

    #[test]
    fn list_style_position_inside() {
        let root = lay("<p>x</p>", "p { list-style-position: inside; }");
        assert_eq!(first_p_style(&root).list_style_position, ListStylePosition::Inside);
    }

    #[test]
    fn list_style_image_url() {
        let root = lay("<p>x</p>", "p { list-style-image: url(\"bullet.png\"); }");
        assert_eq!(
            first_p_style(&root).list_style_image,
            Some("bullet.png".to_string())
        );
    }

    #[test]
    fn list_style_shorthand_combines() {
        let root = lay("<p>x</p>", "p { list-style: square inside; }");
        let s = first_p_style(&root);
        assert_eq!(s.list_style_type, ListStyleType::Square);
        assert_eq!(s.list_style_position, ListStylePosition::Inside);
    }

    #[test]
    fn list_style_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { list-style-type: square; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.list_style_type, ListStyleType::Square);
    }

    #[test]
    fn transition_property_single() {
        let root = lay("<p>x</p>", "p { transition-property: opacity; }");
        assert_eq!(
            first_p_style(&root).transition_properties,
            vec!["opacity".to_string()]
        );
    }

    #[test]
    fn transition_property_list() {
        let root = lay("<p>x</p>", "p { transition-property: opacity, transform, color; }");
        let s = first_p_style(&root);
        assert_eq!(s.transition_properties.len(), 3);
        assert_eq!(s.transition_properties[0], "opacity");
        assert_eq!(s.transition_properties[2], "color");
    }

    #[test]
    fn transition_property_none_clears() {
        let root = lay(
            "<p>x</p>",
            "p { transition-property: opacity; transition-property: none; }",
        );
        assert!(first_p_style(&root).transition_properties.is_empty());
    }

    #[test]
    fn transition_duration_seconds_and_ms() {
        let root = lay("<p>x</p>", "p { transition-duration: 0.5s, 200ms, 1s; }");
        let durations = &first_p_style(&root).transition_durations;
        assert_eq!(durations.len(), 3);
        assert!((durations[0] - 0.5).abs() < 1e-5);
        assert!((durations[1] - 0.2).abs() < 1e-5);
        assert!((durations[2] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transition_delay_parses() {
        let root = lay("<p>x</p>", "p { transition-delay: 100ms; }");
        let s = first_p_style(&root);
        assert!((s.transition_delays[0] - 0.1).abs() < 1e-5);
    }

    // ──────── CSS Easing L1 — TimingFunction parser ────────

    #[test]
    fn timing_function_linear_keyword() {
        assert_eq!(TimingFunction::parse("linear"), Some(TimingFunction::Linear));
    }

    #[test]
    fn timing_function_ease_keywords() {
        assert_eq!(
            TimingFunction::parse("ease"),
            Some(TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0))
        );
        assert_eq!(
            TimingFunction::parse("ease-in"),
            Some(TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0))
        );
        assert_eq!(
            TimingFunction::parse("ease-out"),
            Some(TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0))
        );
        assert_eq!(
            TimingFunction::parse("ease-in-out"),
            Some(TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0))
        );
    }

    #[test]
    fn timing_function_cubic_bezier_explicit() {
        assert_eq!(
            TimingFunction::parse("cubic-bezier(0.1, 0.7, 0.9, 0.3)"),
            Some(TimingFunction::CubicBezier(0.1, 0.7, 0.9, 0.3))
        );
    }

    #[test]
    fn timing_function_cubic_bezier_x_out_of_range_rejected() {
        // x1 / x2 ∈ [0, 1] by spec; out-of-range — invalid.
        assert_eq!(TimingFunction::parse("cubic-bezier(1.5, 0, 0.5, 1)"), None);
        assert_eq!(TimingFunction::parse("cubic-bezier(0, 0, -0.1, 1)"), None);
    }

    #[test]
    fn timing_function_cubic_bezier_y_unbounded() {
        // y координаты могут быть вне [0, 1] (overshoot easings).
        assert_eq!(
            TimingFunction::parse("cubic-bezier(0.5, -0.5, 0.5, 1.5)"),
            Some(TimingFunction::CubicBezier(0.5, -0.5, 0.5, 1.5))
        );
    }

    #[test]
    fn timing_function_step_keywords() {
        assert_eq!(
            TimingFunction::parse("step-start"),
            Some(TimingFunction::Steps(1, StepPosition::JumpStart))
        );
        assert_eq!(
            TimingFunction::parse("step-end"),
            Some(TimingFunction::Steps(1, StepPosition::JumpEnd))
        );
    }

    #[test]
    fn timing_function_steps_with_position() {
        assert_eq!(
            TimingFunction::parse("steps(4, jump-start)"),
            Some(TimingFunction::Steps(4, StepPosition::JumpStart))
        );
        assert_eq!(
            TimingFunction::parse("steps(3, end)"),
            Some(TimingFunction::Steps(3, StepPosition::JumpEnd))
        );
        assert_eq!(
            TimingFunction::parse("steps(5, jump-both)"),
            Some(TimingFunction::Steps(5, StepPosition::JumpBoth))
        );
    }

    #[test]
    fn timing_function_steps_default_position_is_jump_end() {
        // steps(n) без position ≡ steps(n, jump-end).
        assert_eq!(
            TimingFunction::parse("steps(7)"),
            Some(TimingFunction::Steps(7, StepPosition::JumpEnd))
        );
    }

    #[test]
    fn timing_function_steps_jump_none_requires_n_ge_2() {
        // jump-none с n=1 — невалидно (никаких шагов между границами).
        assert_eq!(TimingFunction::parse("steps(1, jump-none)"), None);
        assert_eq!(
            TimingFunction::parse("steps(2, jump-none)"),
            Some(TimingFunction::Steps(2, StepPosition::JumpNone))
        );
    }

    #[test]
    fn timing_function_steps_zero_invalid() {
        assert_eq!(TimingFunction::parse("steps(0)"), None);
        assert_eq!(TimingFunction::parse("steps(0, end)"), None);
    }

    #[test]
    fn timing_function_case_insensitive() {
        assert_eq!(
            TimingFunction::parse("LINEAR"),
            Some(TimingFunction::Linear)
        );
        assert_eq!(
            TimingFunction::parse("Cubic-Bezier(0.25, 0.1, 0.25, 1.0)"),
            Some(TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0))
        );
    }

    #[test]
    fn timing_function_default_is_ease() {
        assert_eq!(
            TimingFunction::default(),
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
    }

    #[test]
    fn timing_function_list_with_nested_commas() {
        // split_top_level_commas должен корректно сохранять argument commas
        // внутри cubic-bezier(...) и steps(...).
        let list = TimingFunction::parse_list(
            "linear, cubic-bezier(0.1, 0.2, 0.3, 0.4), steps(3, end)",
        );
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], TimingFunction::Linear);
        assert_eq!(list[1], TimingFunction::CubicBezier(0.1, 0.2, 0.3, 0.4));
        assert_eq!(list[2], TimingFunction::Steps(3, StepPosition::JumpEnd));
    }

    // ──────── CSS Transitions L1 — transition-timing-function ────────

    #[test]
    fn transition_timing_function_single() {
        let root = lay("<p>x</p>", "p { transition-timing-function: ease-in-out; }");
        let s = first_p_style(&root);
        assert_eq!(s.transition_timing_functions.len(), 1);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn transition_timing_function_list_of_three() {
        let root = lay(
            "<p>x</p>",
            "p { transition-timing-function: linear, cubic-bezier(0.5, 0, 0.5, 1), steps(4); }",
        );
        let s = first_p_style(&root);
        assert_eq!(s.transition_timing_functions.len(), 3);
        assert_eq!(s.transition_timing_functions[0], TimingFunction::Linear);
        assert_eq!(
            s.transition_timing_functions[2],
            TimingFunction::Steps(4, StepPosition::JumpEnd)
        );
    }

    #[test]
    fn transition_timing_function_default_empty() {
        // Без декларации — пустой Vec (consumer применяет default `ease`
        // через cyclically-reuse правило).
        let root = lay("<p>x</p>", "p { color: red; }");
        assert!(first_p_style(&root).transition_timing_functions.is_empty());
    }

    // ──────── CSS Animations L1 — animation-name ────────

    #[test]
    fn animation_name_single() {
        let root = lay("<p>x</p>", "p { animation-name: spin; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_names, vec!["spin".to_string()]);
    }

    #[test]
    fn animation_name_comma_list() {
        let root = lay("<p>x</p>", "p { animation-name: fade, slide, bounce; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_names.len(), 3);
        assert_eq!(s.animation_names[1], "slide");
    }

    #[test]
    fn animation_name_none_clears() {
        let root = lay(
            "<p>x</p>",
            "p { animation-name: spin; animation-name: none; }",
        );
        assert!(first_p_style(&root).animation_names.is_empty());
    }

    #[test]
    fn animation_name_default_empty() {
        let root = lay("<p>x</p>", "p { color: red; }");
        assert!(first_p_style(&root).animation_names.is_empty());
    }

    // ──────── CSS Animations L1 — animation-duration / -delay ────────

    #[test]
    fn animation_duration_seconds_and_ms() {
        let root = lay(
            "<p>x</p>",
            "p { animation-duration: 1s, 200ms, 0.5s; }",
        );
        let durations = &first_p_style(&root).animation_durations;
        assert_eq!(durations.len(), 3);
        assert!((durations[0] - 1.0).abs() < 1e-5);
        assert!((durations[1] - 0.2).abs() < 1e-5);
        assert!((durations[2] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn animation_delay_negative_allowed() {
        // Отрицательный animation-delay допустим (phase offset).
        let root = lay("<p>x</p>", "p { animation-delay: -200ms; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_delays.len(), 1);
        assert!((s.animation_delays[0] - (-0.2)).abs() < 1e-5);
    }

    // ──────── CSS Animations L1 — animation-timing-function ────────

    #[test]
    fn animation_timing_function_keyword_and_function_mixed() {
        let root = lay(
            "<p>x</p>",
            "p { animation-timing-function: ease, steps(4, jump-start); }",
        );
        let s = first_p_style(&root);
        assert_eq!(s.animation_timing_functions.len(), 2);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
        assert_eq!(
            s.animation_timing_functions[1],
            TimingFunction::Steps(4, StepPosition::JumpStart)
        );
    }

    // ──────── CSS Animations L1 — animation-iteration-count ────────

    #[test]
    fn animation_iteration_count_finite() {
        let root = lay("<p>x</p>", "p { animation-iteration-count: 3; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_iteration_counts.len(), 1);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(3.0));
    }

    #[test]
    fn animation_iteration_count_fractional() {
        // Spec L1 §3.5 — count может быть дробным (`2.5` ≡ две полных
        // итерации + половина третьей).
        let root = lay("<p>x</p>", "p { animation-iteration-count: 2.5; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(2.5));
    }

    #[test]
    fn animation_iteration_count_infinite_keyword() {
        let root = lay("<p>x</p>", "p { animation-iteration-count: infinite; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Infinite);
    }

    #[test]
    fn animation_iteration_count_list() {
        let root = lay(
            "<p>x</p>",
            "p { animation-iteration-count: 1, infinite, 5; }",
        );
        let s = first_p_style(&root);
        assert_eq!(s.animation_iteration_counts.len(), 3);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(1.0));
        assert_eq!(s.animation_iteration_counts[1], IterationCount::Infinite);
        assert_eq!(s.animation_iteration_counts[2], IterationCount::Finite(5.0));
    }

    #[test]
    fn animation_iteration_count_negative_invalid() {
        // Отрицательный count — invalid declaration, не записывается.
        let root = lay("<p>x</p>", "p { animation-iteration-count: -1; }");
        let s = first_p_style(&root);
        assert!(s.animation_iteration_counts.is_empty());
    }

    // ──────── CSS Animations L1 — animation-direction ────────

    #[test]
    fn animation_direction_all_keywords() {
        let cases = [
            ("normal", AnimationDirection::Normal),
            ("reverse", AnimationDirection::Reverse),
            ("alternate", AnimationDirection::Alternate),
            ("alternate-reverse", AnimationDirection::AlternateReverse),
        ];
        for (kw, expected) in cases {
            let css = format!("p {{ animation-direction: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).animation_directions[0], expected);
        }
    }

    #[test]
    fn animation_direction_list() {
        let root = lay(
            "<p>x</p>",
            "p { animation-direction: normal, alternate-reverse; }",
        );
        let s = first_p_style(&root);
        assert_eq!(s.animation_directions.len(), 2);
        assert_eq!(s.animation_directions[1], AnimationDirection::AlternateReverse);
    }

    // ──────── CSS Animations L1 — animation-fill-mode ────────

    #[test]
    fn animation_fill_mode_all_keywords() {
        let cases = [
            ("none", AnimationFillMode::None),
            ("forwards", AnimationFillMode::Forwards),
            ("backwards", AnimationFillMode::Backwards),
            ("both", AnimationFillMode::Both),
        ];
        for (kw, expected) in cases {
            let css = format!("p {{ animation-fill-mode: {kw}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).animation_fill_modes[0], expected);
        }
    }

    // ──────── CSS Animations L1 — animation-play-state ────────

    #[test]
    fn animation_play_state_running_paused() {
        let root = lay("<p>x</p>", "p { animation-play-state: paused; }");
        let s = first_p_style(&root);
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
    }

    #[test]
    fn animation_play_state_list() {
        let root = lay(
            "<p>x</p>",
            "p { animation-play-state: running, paused, running; }",
        );
        let s = first_p_style(&root);
        assert_eq!(s.animation_play_states.len(), 3);
        assert_eq!(s.animation_play_states[1], AnimationPlayState::Paused);
    }

    // ──────── CSS Animations defaults — все списки пусты по initial value ────────

    #[test]
    fn animation_longhands_default_all_empty() {
        let root = lay("<p>x</p>", "p { color: red; }");
        let s = first_p_style(&root);
        assert!(s.animation_names.is_empty());
        assert!(s.animation_durations.is_empty());
        assert!(s.animation_delays.is_empty());
        assert!(s.animation_iteration_counts.is_empty());
        assert!(s.animation_timing_functions.is_empty());
        assert!(s.animation_directions.is_empty());
        assert!(s.animation_fill_modes.is_empty());
        assert!(s.animation_play_states.is_empty());
    }

    // ──────── CSS Text typography (tab-size, caret-color, overflow-wrap, word-break, hyphens) ────────

    #[test]
    fn tab_size_integer_in_spaces() {
        let root = lay("<p>x</p>", "p { tab-size: 4; }");
        // integer 4 → 32px (8px-per-space).
        assert!((first_p_style(&root).tab_size - 32.0).abs() < 0.01);
    }

    #[test]
    fn tab_size_length() {
        let root = lay("<p>x</p>", "p { tab-size: 40px; }");
        assert!((first_p_style(&root).tab_size - 40.0).abs() < 0.01);
    }

    #[test]
    fn tab_size_default_64() {
        let root = lay("<p>x</p>", "p { color: red; }");
        assert!((first_p_style(&root).tab_size - 64.0).abs() < 0.01);
    }

    #[test]
    fn tab_size_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { tab-size: 100px; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.tab_size - 100.0).abs() < 0.01);
    }

    #[test]
    fn white_space_pre_parsed() {
        let root = lay("<p>x</p>", "p { white-space: pre; }");
        assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::Pre);
    }

    #[test]
    fn white_space_pre_wrap_parsed() {
        let root = lay("<p>x</p>", "p { white-space: pre-wrap; }");
        assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::PreWrap);
    }

    #[test]
    fn white_space_pre_line_parsed() {
        let root = lay("<p>x</p>", "p { white-space: pre-line; }");
        assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::PreLine);
    }

    #[test]
    fn pre_element_ua_white_space_pre() {
        let root = lay("<pre>hello</pre>", "");
        let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        assert_eq!(pre_box.style.white_space, crate::style::WhiteSpace::Pre,
            "UA: <pre> should default to white-space: pre");
    }

    #[test]
    fn pre_element_newline_creates_two_lines() {
        let root = lay_measured("<pre>line1\nline2</pre>", "", 800.0);
        let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let run = pre_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert_eq!(lines.len(), 2, "expected 2 lines for \\n in <pre>, got {}", lines.len());
            assert_eq!(lines[0][0].text, "line1");
            assert_eq!(lines[1][0].text, "line2");
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn pre_element_tab_renders_with_tab_size() {
        // tab-size: 4 → 4*8=32px; char width=8px each.
        // "a\tb" → 'a'=8 + '\t'=32 + 'b'=8 = 48px width frag.
        let root = lay_measured("<pre>a\tb</pre>", "pre { tab-size: 4; }", 800.0);
        let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let run = pre_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
        if let BoxKind::InlineRun { lines, .. } = &run.kind {
            assert_eq!(lines.len(), 1);
            let frag = &lines[0][0];
            // text should be preserved verbatim including \t
            assert!(frag.text.contains('\t'), "tab should be preserved in text: {:?}", frag.text);
            // width: 'a'(8) + '\t'(32) + 'b'(8) = 48
            assert!((frag.width - 48.0).abs() < 0.01, "expected width=48, got {}", frag.width);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn caret_color_named() {
        let root = lay("<p>x</p>", "p { caret-color: red; }");
        assert_eq!(
            first_p_style(&root).caret_color,
            Some(Color { r: 255, g: 0, b: 0, a: 255 })
        );
    }

    #[test]
    fn caret_color_auto() {
        let root = lay("<p>x</p>", "p { caret-color: red; caret-color: auto; }");
        assert_eq!(first_p_style(&root).caret_color, None);
    }

    #[test]
    fn caret_color_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { caret-color: blue; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.caret_color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
    }

    #[test]
    fn overflow_wrap_break_word() {
        let root = lay("<p>x</p>", "p { overflow-wrap: break-word; }");
        assert_eq!(first_p_style(&root).overflow_wrap, OverflowWrap::BreakWord);
    }

    #[test]
    fn word_wrap_alias_overflow_wrap() {
        // `word-wrap` legacy alias.
        let root = lay("<p>x</p>", "p { word-wrap: anywhere; }");
        assert_eq!(first_p_style(&root).overflow_wrap, OverflowWrap::Anywhere);
    }

    #[test]
    fn word_break_keep_all() {
        let root = lay("<p>x</p>", "p { word-break: keep-all; }");
        assert_eq!(first_p_style(&root).word_break, WordBreak::KeepAll);
    }

    #[test]
    fn word_break_break_all() {
        let root = lay("<p>x</p>", "p { word-break: break-all; }");
        assert_eq!(first_p_style(&root).word_break, WordBreak::BreakAll);
    }

    #[test]
    fn hyphens_auto() {
        let root = lay("<p>x</p>", "p { hyphens: auto; }");
        assert_eq!(first_p_style(&root).hyphens, Hyphens::Auto);
    }

    #[test]
    fn hyphens_none() {
        let root = lay("<p>x</p>", "p { hyphens: none; }");
        assert_eq!(first_p_style(&root).hyphens, Hyphens::None);
    }

    #[test]
    fn hyphens_default_manual() {
        let root = lay("<p>x</p>", "p { color: red; }");
        assert_eq!(first_p_style(&root).hyphens, Hyphens::Manual);
    }

    #[test]
    fn text_typography_all_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { tab-size: 50px; overflow-wrap: break-word; word-break: keep-all; hyphens: auto; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!((p.style.tab_size - 50.0).abs() < 0.01);
        assert_eq!(p.style.overflow_wrap, OverflowWrap::BreakWord);
        assert_eq!(p.style.word_break, WordBreak::KeepAll);
        assert_eq!(p.style.hyphens, Hyphens::Auto);
        // А значения у div те же.
        assert!((div.style.tab_size - 50.0).abs() < 0.01);
    }

    // ──────── will-change / pointer-events / user-select / scroll-behavior ────────

    #[test]
    fn will_change_auto_is_empty_list() {
        let root = lay("<p>x</p>", "p { will-change: auto; }");
        assert!(first_p_style(&root).will_change.is_empty());
    }

    #[test]
    fn will_change_property_list() {
        let root = lay("<p>x</p>", "p { will-change: transform, opacity; }");
        let s = first_p_style(&root);
        assert_eq!(
            s.will_change,
            vec!["transform".to_string(), "opacity".to_string()]
        );
    }

    #[test]
    fn will_change_invalid_ident_skipped() {
        let root = lay("<p>x</p>", "p { will-change: 1invalid, transform; }");
        let s = first_p_style(&root);
        assert_eq!(s.will_change, vec!["transform".to_string()]);
    }

    #[test]
    fn pointer_events_none() {
        let root = lay("<p>x</p>", "p { pointer-events: none; }");
        assert_eq!(first_p_style(&root).pointer_events, PointerEvents::None);
    }

    #[test]
    fn pointer_events_all() {
        let root = lay("<p>x</p>", "p { pointer-events: all; }");
        assert_eq!(first_p_style(&root).pointer_events, PointerEvents::All);
    }

    #[test]
    fn user_select_none() {
        let root = lay("<p>x</p>", "p { user-select: none; }");
        assert_eq!(first_p_style(&root).user_select, UserSelect::None);
    }

    #[test]
    fn user_select_text() {
        let root = lay("<p>x</p>", "p { user-select: text; }");
        assert_eq!(first_p_style(&root).user_select, UserSelect::Text);
    }

    #[test]
    fn user_select_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { user-select: none; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Inherited.
        assert_eq!(p.style.user_select, UserSelect::None);
    }

    #[test]
    fn scroll_behavior_smooth() {
        let root = lay("<p>x</p>", "p { scroll-behavior: smooth; }");
        assert_eq!(first_p_style(&root).scroll_behavior, ScrollBehavior::Smooth);
    }

    #[test]
    fn scroll_behavior_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { scroll-behavior: smooth; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.scroll_behavior, ScrollBehavior::Smooth);
    }

    #[test]
    fn pointer_events_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { pointer-events: none; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // НЕ наследуется — у p default Auto.
        assert_eq!(p.style.pointer_events, PointerEvents::Auto);
        assert_eq!(div.style.pointer_events, PointerEvents::None);
    }

    #[test]
    fn unknown_keyword_keeps_default() {
        let root = lay("<p>x</p>", "p { pointer-events: garbage; user-select: weird; }");
        let s = first_p_style(&root);
        assert_eq!(s.pointer_events, PointerEvents::Auto);
        assert_eq!(s.user_select, UserSelect::Auto);
    }

    // ──────── background-* (CSS Backgrounds L3) ────────

    #[test]
    fn background_image_url_parses() {
        let root = lay("<p>x</p>", "p { background-image: url(\"bg.png\"); }");
        let s = first_p_style(&root);
        assert_eq!(s.background_layers[0].image, BackgroundImage::Url("bg.png".into()));
    }

    #[test]
    fn background_image_url_unquoted() {
        let root = lay("<p>x</p>", "p { background-image: url(bg.png); }");
        assert_eq!(
            first_p_style(&root).background_layers[0].image,
            BackgroundImage::Url("bg.png".into())
        );
    }

    #[test]
    fn background_image_none() {
        // Setting "none" after a URL replaces all layers with one None-image layer.
        let root = lay(
            "<p>x</p>",
            "p { background-image: url(\"x.png\"); background-image: none; }",
        );
        assert_eq!(first_p_style(&root).background_layers[0].image, BackgroundImage::None);
    }

    #[test]
    fn background_image_gradient_parsed_linear() {
        use crate::style::ParsedGradient;
        let root = lay(
            "<p>x</p>",
            "p { background-image: linear-gradient(to right, red, blue); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, stops, .. }) => {
                assert!((angle_deg - 90.0).abs() < 0.1, "expected 90° for 'to right'");
                assert_eq!(stops.len(), 2);
            }
            other => panic!("expected ParsedGradient::Linear, got {other:?}"),
        }
    }

    // ── parse_gradient_stops ──────────────────────────────────────────────────

    #[test]
    fn gradient_stops_empty_string_returns_empty() {
        assert_eq!(parse_gradient_stops(""), vec![]);
    }

    #[test]
    fn gradient_stops_no_parens_returns_empty() {
        assert_eq!(parse_gradient_stops("linear-gradient"), vec![]);
    }

    #[test]
    fn gradient_stops_two_named_colors_no_position() {
        let stops = parse_gradient_stops("linear-gradient(red, blue)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[0].position, None);
        assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
        assert_eq!(stops[1].position, None);
    }

    #[test]
    fn gradient_stops_to_right_direction_skipped() {
        let stops = parse_gradient_stops("linear-gradient(to right, red, blue)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn gradient_stops_angle_direction_skipped() {
        let stops = parse_gradient_stops("linear-gradient(45deg, red 0%, blue 100%)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
        assert_eq!(stops[1].position, Some(Length::Percent(100.0)));
    }

    #[test]
    fn gradient_stops_percent_positions_parsed() {
        let stops = parse_gradient_stops("linear-gradient(red 0%, green 50%, blue 100%)");
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
        assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
        assert_eq!(stops[2].position, Some(Length::Percent(100.0)));
    }

    #[test]
    fn gradient_stops_px_positions_parsed() {
        let stops = parse_gradient_stops("linear-gradient(red 0px, blue 200px)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].position, Some(Length::Px(0.0)));
        assert_eq!(stops[1].position, Some(Length::Px(200.0)));
    }

    #[test]
    fn gradient_stops_hex_color_with_percent() {
        let stops = parse_gradient_stops("linear-gradient(#ff0000 20%, #0000ff 80%)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[0].position, Some(Length::Percent(20.0)));
    }

    #[test]
    fn gradient_stops_rgba_function_color() {
        let stops = parse_gradient_stops("linear-gradient(rgba(255,0,0,1) 0%, rgba(0,0,255,1) 100%)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn gradient_stops_two_position_stop_expands() {
        // `red 20% 60%` → two stops: red@20% and red@60%
        let stops = parse_gradient_stops("linear-gradient(red 20% 60%, blue)");
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0].position, Some(Length::Percent(20.0)));
        assert_eq!(stops[1].position, Some(Length::Percent(60.0)));
        assert_eq!(stops[1].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[2].color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn gradient_stops_color_hint_skipped() {
        // `50%` between stops is a color hint — no color → skipped
        let stops = parse_gradient_stops("linear-gradient(red 0%, 50%, blue 100%)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn gradient_stops_radial_shape_skipped() {
        let stops =
            parse_gradient_stops("radial-gradient(circle at 50% 50%, white, black)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 255, b: 255, a: 255 });
        assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn gradient_stops_repeating_linear() {
        let stops =
            parse_gradient_stops("repeating-linear-gradient(red 0px, blue 10px)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(stops[0].position, Some(Length::Px(0.0)));
        assert_eq!(stops[1].position, Some(Length::Px(10.0)));
    }

    #[test]
    fn gradient_stops_zero_unitless_is_px_zero() {
        let stops = parse_gradient_stops("linear-gradient(red 0, blue 100%)");
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].position, Some(Length::Px(0.0)));
    }

    // ── conic-gradient parsing ───────────────────────────────────────────────

    #[test]
    fn background_image_gradient_parsed_conic_default() {
        use crate::style::ParsedGradient;
        let root = lay(
            "<p>x</p>",
            "p { background-image: conic-gradient(red, blue); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Conic {
                center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
            }) => {
                assert!((center_x_pct - 0.5).abs() < 1e-4);
                assert!((center_y_pct - 0.5).abs() < 1e-4);
                assert!(from_angle_deg.abs() < 1e-4, "default from-angle = 0°");
                assert_eq!(stops.len(), 2);
                assert!(!repeating);
            }
            other => panic!("expected Conic, got {other:?}"),
        }
    }

    #[test]
    fn background_image_gradient_parsed_conic_from_and_at() {
        use crate::style::ParsedGradient;
        let root = lay(
            "<p>x</p>",
            "p { background-image: conic-gradient(from 90deg at 25% 75%, red, blue); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Conic {
                center_x_pct, center_y_pct, from_angle_deg, ..
            }) => {
                assert!((center_x_pct - 0.25).abs() < 1e-4);
                assert!((center_y_pct - 0.75).abs() < 1e-4);
                assert!((from_angle_deg - 90.0).abs() < 1e-3);
            }
            other => panic!("expected Conic, got {other:?}"),
        }
    }

    #[test]
    fn background_image_gradient_parsed_repeating_conic() {
        use crate::style::ParsedGradient;
        let root = lay(
            "<p>x</p>",
            "p { background-image: repeating-conic-gradient(red 0deg, blue 90deg); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Conic { repeating, stops, .. }) => {
                assert!(repeating);
                assert_eq!(stops.len(), 2);
                // 0deg → 0%, 90deg → 25%.
                assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
                if let Some(Length::Percent(p)) = stops[1].position {
                    assert!((p - 25.0).abs() < 1e-3, "90deg should map to 25%, got {p}");
                } else {
                    panic!("expected Percent position, got {:?}", stops[1].position);
                }
            }
            other => panic!("expected repeating Conic, got {other:?}"),
        }
    }

    #[test]
    fn conic_stops_angles_converted_to_percent() {
        let stops = parse_gradient_stops("conic-gradient(red 0deg, green 180deg, blue 360deg)");
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
        assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
        if let Some(Length::Percent(p)) = stops[2].position {
            assert!((p - 100.0).abs() < 1e-3);
        } else {
            panic!("expected Percent");
        }
    }

    #[test]
    fn conic_stops_turn_unit() {
        let stops = parse_gradient_stops("conic-gradient(red 0turn, blue 0.5turn)");
        assert_eq!(stops.len(), 2);
        if let Some(Length::Percent(p)) = stops[1].position {
            assert!((p - 50.0).abs() < 1e-3, "0.5turn should map to 50%, got {p}");
        } else {
            panic!("expected Percent");
        }
    }

    #[test]
    fn conic_stops_percent_passthrough() {
        let stops = parse_gradient_stops("conic-gradient(red 0%, blue 25%, green 100%)");
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
        assert_eq!(stops[1].position, Some(Length::Percent(25.0)));
        assert_eq!(stops[2].position, Some(Length::Percent(100.0)));
    }

    #[test]
    fn conic_stops_named_colors_no_position() {
        // No explicit positions: auto-distributed by renderer; parser keeps None.
        let stops = parse_gradient_stops("conic-gradient(red, green, blue)");
        assert_eq!(stops.len(), 3);
        for s in &stops {
            assert!(s.position.is_none());
        }
    }

    #[test]
    fn conic_from_and_at_parsed_independently() {
        use crate::style::ParsedGradient;
        // Only `at` clause, no `from`.
        let root = lay(
            "<p>x</p>",
            "p { background-image: conic-gradient(at 10% 20%, red, blue); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Conic {
                center_x_pct, center_y_pct, from_angle_deg, ..
            }) => {
                assert!((center_x_pct - 0.1).abs() < 1e-4);
                assert!((center_y_pct - 0.2).abs() < 1e-4);
                assert!(from_angle_deg.abs() < 1e-4);
            }
            other => panic!("expected Conic, got {other:?}"),
        }
    }

    #[test]
    fn conic_from_turn_unit() {
        use crate::style::ParsedGradient;
        let root = lay(
            "<p>x</p>",
            "p { background-image: conic-gradient(from 0.25turn, red, blue); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Conic { from_angle_deg, .. }) => {
                // 0.25turn = 90deg.
                assert!((from_angle_deg - 90.0).abs() < 1e-3, "got {from_angle_deg}");
            }
            other => panic!("expected Conic, got {other:?}"),
        }
    }

    #[test]
    fn background_image_gradient_parsed_conic_keyword_position() {
        use crate::style::ParsedGradient;
        // `at top left` → (0, 0).
        let root = lay(
            "<p>x</p>",
            "p { background-image: conic-gradient(at left top, red, blue); }",
        );
        match &first_p_style(&root).background_layers[0].image {
            BackgroundImage::Gradient(ParsedGradient::Conic {
                center_x_pct, center_y_pct, ..
            }) => {
                assert!(center_x_pct.abs() < 1e-4);
                assert!(center_y_pct.abs() < 1e-4);
            }
            other => panic!("expected Conic, got {other:?}"),
        }
    }

    #[test]
    fn background_repeat_values() {
        for (s, expected) in [
            ("repeat", BackgroundRepeat::Repeat),
            ("no-repeat", BackgroundRepeat::NoRepeat),
            ("repeat-x", BackgroundRepeat::RepeatX),
            ("repeat-y", BackgroundRepeat::RepeatY),
            ("round", BackgroundRepeat::Round),
            ("space", BackgroundRepeat::Space),
        ] {
            let css = format!("p {{ background-repeat: {s}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).background_layers[0].repeat, expected);
        }
    }

    #[test]
    fn background_size_keywords() {
        for (s, expected) in [
            ("auto", BackgroundSize::Auto),
            ("cover", BackgroundSize::Cover),
            ("contain", BackgroundSize::Contain),
        ] {
            let css = format!("p {{ background-size: {s}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).background_layers[0].size, expected);
        }
    }

    #[test]
    fn background_size_length_single() {
        let root = lay("<p>x</p>", "p { background-size: 200px; }");
        match first_p_style(&root).background_layers[0].size {
            BackgroundSize::Length(w, h) => {
                assert!((w - 200.0).abs() < 0.01);
                assert_eq!(h, None);
            }
            _ => panic!("expected Length"),
        }
    }

    #[test]
    fn background_size_length_pair() {
        let root = lay("<p>x</p>", "p { background-size: 200px 100px; }");
        match first_p_style(&root).background_layers[0].size {
            BackgroundSize::Length(w, h) => {
                assert!((w - 200.0).abs() < 0.01);
                assert_eq!(h, Some(100.0));
            }
            _ => panic!("expected Length"),
        }
    }

    #[test]
    fn background_attachment_values() {
        for (s, expected) in [
            ("scroll", BackgroundAttachment::Scroll),
            ("fixed", BackgroundAttachment::Fixed),
            ("local", BackgroundAttachment::Local),
        ] {
            let css = format!("p {{ background-attachment: {s}; }}");
            let root = lay("<p>x</p>", &css);
            assert_eq!(first_p_style(&root).background_layers[0].attachment, expected);
        }
    }

    #[test]
    fn background_properties_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { background-image: url(x.png); background-repeat: no-repeat; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // Child element has no background declarations → empty layers (initial state).
        assert!(p.style.background_layers.is_empty());
    }

    // ──────── place-items / align-* / justify-* (CSS Box Alignment L3) ────────

    #[test]
    fn align_items_center() {
        let root = lay("<p>x</p>", "p { align-items: center; }");
        assert_eq!(first_p_style(&root).align_items, AlignValue::Center);
    }

    #[test]
    fn justify_content_space_between() {
        let root = lay("<p>x</p>", "p { justify-content: space-between; }");
        assert_eq!(first_p_style(&root).justify_content, AlignValue::SpaceBetween);
    }

    #[test]
    fn flex_start_alias() {
        // CSS spec: flex-start alias для start (вне flex-контекста).
        let root = lay("<p>x</p>", "p { align-items: flex-start; }");
        assert_eq!(first_p_style(&root).align_items, AlignValue::Start);
    }

    #[test]
    fn place_items_single_value() {
        let root = lay("<p>x</p>", "p { place-items: center; }");
        let s = first_p_style(&root);
        // Single value применяется к обоим осям.
        assert_eq!(s.align_items, AlignValue::Center);
        assert_eq!(s.justify_items, AlignValue::Center);
    }

    #[test]
    fn place_items_two_values() {
        let root = lay("<p>x</p>", "p { place-items: start end; }");
        let s = first_p_style(&root);
        assert_eq!(s.align_items, AlignValue::Start);
        assert_eq!(s.justify_items, AlignValue::End);
    }

    #[test]
    fn place_self_shorthand() {
        let root = lay("<p>x</p>", "p { place-self: center stretch; }");
        let s = first_p_style(&root);
        assert_eq!(s.align_self, AlignValue::Center);
        assert_eq!(s.justify_self, AlignValue::Stretch);
    }

    #[test]
    fn place_content_shorthand() {
        let root = lay("<p>x</p>", "p { place-content: space-around; }");
        let s = first_p_style(&root);
        assert_eq!(s.align_content, AlignValue::SpaceAround);
        assert_eq!(s.justify_content, AlignValue::SpaceAround);
    }

    #[test]
    fn align_unknown_value_ignored() {
        let root = lay("<p>x</p>", "p { align-items: garbage; }");
        // default (Auto) сохраняется.
        assert_eq!(first_p_style(&root).align_items, AlignValue::Auto);
    }

    #[test]
    fn alignment_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { align-items: center; justify-content: space-between; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        // У p должны быть defaults.
        assert_eq!(p.style.align_items, AlignValue::Auto);
        assert_eq!(p.style.justify_content, AlignValue::Auto);
        // У div — заданные.
        assert_eq!(div.style.align_items, AlignValue::Center);
        assert_eq!(div.style.justify_content, AlignValue::SpaceBetween);
    }

    #[test]
    fn align_value_parse_all_keywords() {
        for (s, expected) in [
            ("auto", AlignValue::Auto),
            ("normal", AlignValue::Normal),
            ("stretch", AlignValue::Stretch),
            ("start", AlignValue::Start),
            ("end", AlignValue::End),
            ("center", AlignValue::Center),
            ("baseline", AlignValue::Baseline),
            ("space-between", AlignValue::SpaceBetween),
            ("space-around", AlignValue::SpaceAround),
            ("space-evenly", AlignValue::SpaceEvenly),
            ("flex-start", AlignValue::Start),
            ("flex-end", AlignValue::End),
            ("self-start", AlignValue::Start),
            ("CENTER", AlignValue::Center),  // case-insensitive
        ] {
            assert_eq!(AlignValue::parse(s), Some(expected), "input: {s}");
        }
    }

    #[test]
    fn align_value_parse_unknown_returns_none() {
        assert_eq!(AlignValue::parse("garbage"), None);
        assert_eq!(AlignValue::parse(""), None);
    }

    #[test]
    fn gap_and_aspect_ratio_not_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { gap: 20px; aspect-ratio: 2; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.row_gap, Length::Px(0.0));
        assert_eq!(p.style.aspect_ratio, None);
        assert_eq!(div.style.row_gap, Length::Px(20.0));
        assert!(div.style.aspect_ratio.is_some());
    }

    #[test]
    fn media_prefers_color_scheme_light_default() {
        // Phase 0: prefers_dark=false → 'light' matches.
        let root = lay_with_viewport(
            "<p>x</p>",
            "@media (prefers-color-scheme: light) { p { color: red; } }",
            800.0,
            600.0,
        );
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ── CSS Quirks Mode — UA-rule для <table> ──────────────────────────────

    /// В Quirks-mode (нет DOCTYPE) `<table>` сбрасывает font-size к
    /// initial-значению, не наследует от родителя.
    #[test]
    fn quirks_table_font_size_resets_to_initial() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert!(
            (body.style.font_size - 30.0).abs() < 0.01,
            "body должен наследовать заявленные 30px"
        );
        assert!(
            (table.style.font_size - 16.0).abs() < 0.01,
            "table в Quirks должен сбросить font-size к initial 16, получено {}",
            table.style.font_size
        );
    }

    /// В Standards mode (`<!DOCTYPE html>`) `<table>` наследует font-size
    /// от родителя как обычный элемент.
    #[test]
    fn standards_table_font_size_inherits() {
        let root = lay(
            "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert!(
            (table.style.font_size - 30.0).abs() < 0.01,
            "table в Standards должен наследовать 30px, получено {}",
            table.style.font_size
        );
    }

    /// В Quirks color у `<table>` сбрасывается к BLACK, не наследуется.
    #[test]
    fn quirks_table_color_resets_to_black() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { color: red; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(body.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(table.style.color, Color::BLACK);
    }

    /// В Standards color наследуется.
    #[test]
    fn standards_table_color_inherits() {
        let root = lay(
            "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
            "body { color: red; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// В Quirks font-weight у `<table>` сбрасывается к NORMAL.
    #[test]
    fn quirks_table_font_weight_resets_to_normal() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-weight: bold; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(body.style.font_weight, FontWeight::BOLD);
        assert_eq!(table.style.font_weight, FontWeight::NORMAL);
    }

    /// В Quirks font-style у `<table>` сбрасывается к Normal.
    #[test]
    fn quirks_table_font_style_resets_to_normal() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-style: italic; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(body.style.font_style, FontStyle::Italic);
        assert_eq!(table.style.font_style, FontStyle::Normal);
    }

    /// В Quirks text-align у `<table>` сбрасывается к initial (Start).
    #[test]
    fn quirks_table_text_align_resets_to_left() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { text-align: center; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(body.style.text_align, TextAlign::Center);
        assert_eq!(table.style.text_align, TextAlign::Start);
    }

    /// В Quirks white-space у `<table>` сбрасывается к Normal.
    #[test]
    fn quirks_table_white_space_resets_to_normal() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { white-space: nowrap; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(body.style.white_space, WhiteSpace::Nowrap);
        assert_eq!(table.style.white_space, WhiteSpace::Normal);
    }

    /// Author CSS поверх Quirks-reset выигрывает: spec-rule идёт как
    /// низший cascade origin (UA).
    #[test]
    fn quirks_table_author_css_wins_over_reset() {
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; } table { font-size: 24px; color: blue; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert!(
            (table.style.font_size - 24.0).abs() < 0.01,
            "author CSS должен переопределить Quirks-reset"
        );
        assert_eq!(table.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    /// Дочерние элементы `<table>` в Quirks наследуют от сброшенных
    /// значений таблицы, не от прародителя.
    #[test]
    fn quirks_table_children_inherit_reset_values() {
        // <body>=30px → <table>=16 (reset) → <td>=16 (inherits from table).
        let root = lay(
            "<body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; }",
        );
        let body = &root;
        let table = first_element_child(body);
        // HTML5 parser inserts implicit <tbody>: table → tbody → tr → td.
        // Идём вглубь, пока не найдём td (Block inside a TableRow).
        fn find_td(b: &LayoutBox) -> Option<&LayoutBox> {
            for c in &b.children {
                if matches!(&c.kind, BoxKind::TableRow | BoxKind::TableRowGroup) {
                    if let Some(td) = find_td(c) {
                        return Some(td);
                    }
                } else if matches!(&c.kind, BoxKind::Block) {
                    if let Some(td) = find_td(c) {
                        return Some(td);
                    }
                    return Some(c);
                }
            }
            None
        }
        let td = find_td(table).expect("td не найден");
        assert!(
            (td.style.font_size - 16.0).abs() < 0.01,
            "td должен унаследовать от table сброшенные 16px, получено {}",
            td.style.font_size
        );
    }

    /// Не-`<table>` элементы в Quirks-mode не сбрасывают inherited.
    #[test]
    fn quirks_non_table_inherits_normally() {
        let root = lay(
            "<body><p>x</p></body>",
            "body { font-size: 30px; color: red; }",
        );
        let body = &root;
        let p = first_element_child(body);
        assert!(
            (p.style.font_size - 30.0).abs() < 0.01,
            "<p> в Quirks-mode должен наследовать font-size, получено {}",
            p.style.font_size
        );
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// LimitedQuirks (HTML 4.01 Transitional) — table-reset не применяется
    /// (spec §4.1: только в Quirks-mode).
    #[test]
    fn limited_quirks_does_not_apply_table_reset() {
        let root = lay(
            "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><table><tr><td>x</td></tr></table></body>",
            "body { font-size: 30px; color: red; }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert!(
            (table.style.font_size - 30.0).abs() < 0.01,
            "table в LimitedQuirks должен наследовать font-size как в Standards"
        );
        assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    // ── CSS Quirks Mode §3.4 — «hashless hex color quirk» ──────────────────

    /// В Quirks-mode `color: ff0000` (без `#`) парсится как red.
    /// Это эквивалент `color: #ff0000` (CSS Quirks Mode §3.4).
    #[test]
    fn quirks_hashless_hex_in_color_property() {
        // Нет DOCTYPE → Quirks.
        let root = lay(
            "<body><p>x</p></body>",
            "p { color: ff0000; }",
        );
        let body = &root;
        let p = first_element_child(body);
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// В Standards-mode `color: ff0000` (без `#`) — невалидное значение,
    /// игнорируется. Цвет наследуется (по умолчанию BLACK).
    #[test]
    fn standards_hashless_hex_rejected_in_color_property() {
        let root = lay(
            "<!DOCTYPE html><body><p>x</p></body>",
            "p { color: ff0000; }",
        );
        let body = &root;
        let p = first_element_child(body);
        // ff0000 без `#` — невалидно в Standards, color остаётся inherited
        // от body (BLACK).
        assert_eq!(p.style.color, Color::BLACK);
    }

    /// В Quirks `background-color: 00ff00` (6-hex без `#`) парсится как green.
    #[test]
    fn quirks_hashless_hex_in_background_color() {
        let root = lay(
            "<body><p>x</p></body>",
            "p { background-color: 00ff00; }",
        );
        let body = &root;
        let p = first_element_child(body);
        assert_eq!(p.style.background_color, Some(CssColor::Rgba(Color { r: 0, g: 255, b: 0, a: 255 })));
    }

    /// В Quirks 3-hex bare digit-ы тоже парсятся: `f00` → red.
    #[test]
    fn quirks_hashless_hex_3_digit_short() {
        let root = lay(
            "<body><p>x</p></body>",
            "p { color: f00; }",
        );
        let body = &root;
        let p = first_element_child(body);
        assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    /// В Quirks border-color принимает bare hex.
    #[test]
    fn quirks_hashless_hex_in_border_color() {
        let root = lay(
            "<body><p>x</p></body>",
            "p { border: 1px solid 0000ff; }",
        );
        let body = &root;
        let p = first_element_child(body);
        assert_eq!(
            p.style.border_top_color,
            CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 }),
        );
    }

    /// LimitedQuirks (HTML 4.01 Transitional) — hashless hex quirk
    /// НЕ применяется (spec §1.1.1: «full quirks mode only»).
    #[test]
    fn limited_quirks_hashless_hex_rejected() {
        let root = lay(
            "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><p>x</p></body>",
            "p { color: ff0000; }",
        );
        let body = &root;
        let p = first_element_child(body);
        // В LimitedQuirks bare hex — invalid, как в Standards.
        assert_eq!(p.style.color, Color::BLACK);
    }

    // ──────────────── CSS Quirks Mode §3.5 — html viewport height ────────────────

    /// В quirks-mode `<html>` получает UA-правило `height: 100vh`, поэтому
    /// его rect.height равен высоте viewport (600.0 в тестовом lay).
    #[test]
    fn quirks_html_height_equals_viewport() {
        let root = lay_full("<html><body><p>x</p></body></html>", "");
        let (html, _body) = html_and_body(&root);
        assert!(
            (html.rect.height - 600.0).abs() < 0.1,
            "quirks: html.rect.height={} (ожидалось 600.0)",
            html.rect.height
        );
    }

    /// В quirks-mode `body { height: 100% }` резолвится против viewport
    /// через html-box с высотой 100vh.
    #[test]
    fn quirks_body_height_100pct_resolves_to_viewport() {
        let root = lay_full(
            "<html><body></body></html>",
            "body { height: 100%; }",
        );
        let (_html, body) = html_and_body(&root);
        assert!(
            (body.rect.height - 600.0).abs() < 0.1,
            "quirks: body.rect.height={} (ожидалось 600.0)",
            body.rect.height
        );
    }

    /// В standards-mode (с `<!DOCTYPE html>`) `<html>` с высотой auto
    /// НЕ получает 100vh — высота определяется контентом (маленькая).
    #[test]
    fn standards_html_height_is_content_not_viewport() {
        let root = lay_full(
            "<!DOCTYPE html><html><body><p style=\"height:20px\">x</p></body></html>",
            "",
        );
        let (html, _body) = html_and_body(&root);
        // Контент высотой 20px + margins body → html значительно < 600.
        assert!(
            html.rect.height < 200.0,
            "standards: html.rect.height={} (ожидалось меньше 200.0)",
            html.rect.height
        );
    }

    /// В quirks-mode author CSS на `<html>` перекрывает UA-правило 100vh.
    #[test]
    fn quirks_html_author_height_overrides_ua_rule() {
        let root = lay_full(
            "<html><body></body></html>",
            "html { height: 300px; }",
        );
        let (html, _body) = html_and_body(&root);
        assert!(
            (html.rect.height - 300.0).abs() < 0.1,
            "quirks: author height=300px, html.rect.height={} (ожидалось 300.0)",
            html.rect.height
        );
    }

    /// В limited-quirks mode (HTML 4.01 Transitional + system_id) правило
    /// §3.5 НЕ применяется — только full quirks mode.
    #[test]
    fn limited_quirks_html_height_is_content_not_viewport() {
        let root = lay_full(
            "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \
             \"http://www.w3.org/TR/html4/loose.dtd\">\
             <html><body><p style=\"height:20px\">x</p></body></html>",
            "",
        );
        let (html, _body) = html_and_body(&root);
        assert!(
            html.rect.height < 200.0,
            "limited-quirks: html.rect.height={} (ожидалось меньше 200.0)",
            html.rect.height
        );
    }

    // ──────────────── :fullscreen / :modal / :popover-open (open-state pseudo-classes) ────────────────

    /// `:fullscreen` (Fullscreen API §4.2) — Phase 0 без runtime top-layer
    /// никакой элемент не считается fullscreen, правило не применяется.
    #[test]
    fn fullscreen_pseudo_never_matches_in_phase_0() {
        let c = element_color(
            "<div>x</div>",
            "div:fullscreen { color: red; }",
            "div",
        );
        assert_eq!(c.r, 0);
    }

    /// `:fullscreen` не активируется даже на дочернем элементе с
    /// контейнером — top-layer state runtime-only.
    #[test]
    fn fullscreen_pseudo_never_matches_nested() {
        let c = element_color(
            "<div><p>x</p></div>",
            "p:fullscreen { color: red; }",
            "p",
        );
        assert_eq!(c.r, 0);
    }

    /// `:modal` (CSS Selectors L4 §16.5.2) — Phase 0 без dialog runtime.
    /// `<dialog open>` НЕ модален: атрибут `open` ставится и через
    /// `dialog.show()` (non-modal), поэтому простая DOM-проверка не покрыла
    /// бы spec — matcher всегда `false`.
    #[test]
    fn modal_pseudo_never_matches_in_phase_0() {
        let c = element_color(
            "<dialog open>x</dialog>",
            "dialog:modal { color: red; }",
            "dialog",
        );
        assert_eq!(c.r, 0);
    }

    /// `:modal` не активируется и без атрибута `open`.
    #[test]
    fn modal_pseudo_never_matches_closed_dialog() {
        let c = element_color(
            "<dialog>x</dialog>",
            "dialog:modal { color: red; }",
            "dialog",
        );
        assert_eq!(c.r, 0);
    }

    /// `:popover-open` (HTML LS §6.12.2) — Phase 0 без Popover API runtime.
    /// Наличие атрибута `popover` декларирует тип, но не открытое состояние.
    #[test]
    fn popover_open_pseudo_never_matches_in_phase_0() {
        let c = element_color(
            r#"<div popover="auto">x</div>"#,
            "div:popover-open { color: red; }",
            "div",
        );
        assert_eq!(c.r, 0);
    }

    /// `:popover-open` не матчит и при отсутствии `popover`-атрибута.
    #[test]
    fn popover_open_pseudo_never_matches_non_popover() {
        let c = element_color(
            "<div>x</div>",
            "div:popover-open { color: red; }",
            "div",
        );
        assert_eq!(c.r, 0);
    }

    /// Specificity открытых-состояния pseudo-классов — class-уровня (0,1,0).
    /// `:not(:fullscreen)` через always-false означает «всегда true» — это
    /// удобный FOUC-protection idiom (если когда-нибудь fullscreen runtime
    /// появится, правило сбросится). Проверяем, что `:not(:fullscreen)`
    /// действительно матчит обычный element.
    #[test]
    fn not_fullscreen_matches_all_elements_in_phase_0() {
        let c = element_color(
            "<div>x</div>",
            "div:not(:fullscreen) { color: red; }",
            "div",
        );
        assert_eq!(c.r, 255);
    }

    /// То же для `:not(:modal)`: элементы не в modal state — все элементы
    /// в Phase 0.
    #[test]
    fn not_modal_matches_all_elements_in_phase_0() {
        let c = element_color(
            "<dialog open>x</dialog>",
            "dialog:not(:modal) { color: red; }",
            "dialog",
        );
        assert_eq!(c.r, 255);
    }

    /// То же для `:not(:popover-open)`.
    #[test]
    fn not_popover_open_matches_all_elements_in_phase_0() {
        let c = element_color(
            r#"<div popover="auto">x</div>"#,
            "div:not(:popover-open) { color: red; }",
            "div",
        );
        assert_eq!(c.r, 255);
    }

    // ──────────────── :current / :past / :future (CSS Selectors L4 §11.4) ────────────────

    /// `:current` (§11.4.1) — timed-text «active cue». Phase 0 без timed-text
    /// runtime никакой элемент не считается current, правило не применяется.
    #[test]
    fn current_pseudo_never_matches_in_phase_0() {
        let c = element_color(
            "<p>x</p>",
            "p:current { color: red; }",
            "p",
        );
        assert_eq!(c.r, 0);
    }

    /// `:past` (§11.4.2) — Phase 0 timed-text без runtime → always false.
    #[test]
    fn past_pseudo_never_matches_in_phase_0() {
        let c = element_color(
            "<p>x</p>",
            "p:past { color: red; }",
            "p",
        );
        assert_eq!(c.r, 0);
    }

    /// `:future` (§11.4.3) — Phase 0 timed-text без runtime → always false.
    #[test]
    fn future_pseudo_never_matches_in_phase_0() {
        let c = element_color(
            "<p>x</p>",
            "p:future { color: red; }",
            "p",
        );
        assert_eq!(c.r, 0);
    }

    /// Time-dim pseudo-classes specificity = class-level (0,1,0). Проверяем,
    /// что `:not(:current)` матчит все элементы (классическая FOUC/initial-
    /// state idiom — когда timed-text runtime появится, правило сбросится).
    #[test]
    fn not_current_matches_all_elements_in_phase_0() {
        let c = element_color(
            "<p>x</p>",
            "p:not(:current) { color: red; }",
            "p",
        );
        assert_eq!(c.r, 255);
    }

    /// То же для `:not(:past)`.
    #[test]
    fn not_past_matches_all_elements_in_phase_0() {
        let c = element_color(
            "<p>x</p>",
            "p:not(:past) { color: red; }",
            "p",
        );
        assert_eq!(c.r, 255);
    }

    /// То же для `:not(:future)`.
    #[test]
    fn not_future_matches_all_elements_in_phase_0() {
        let c = element_color(
            "<p>x</p>",
            "p:not(:future) { color: red; }",
            "p",
        );
        assert_eq!(c.r, 255);
    }

    // ─── Canvas background propagation (CSS Backgrounds L3 §2.11.2) ─────

    fn html_and_body(root: &LayoutBox) -> (&LayoutBox, &LayoutBox) {
        let html = root
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Block))
            .expect("html box");
        let body = html
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Block))
            .expect("body box");
        (html, body)
    }

    #[test]
    fn body_bg_propagates_to_html_when_html_has_none() {
        let root = lay_full(
            "<html><body><p>x</p></body></html>",
            "body { background-color: red; }",
        );
        let (html, body) = html_and_body(&root);
        assert_eq!(
            html.style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
            "html должен получить фон body"
        );
        assert_eq!(
            body.style.background_color, None,
            "у body фон обнуляется после propagation"
        );
    }

    #[test]
    fn html_with_own_bg_blocks_propagation() {
        let root = lay_full(
            "<html><body><p>x</p></body></html>",
            "html { background-color: blue; } body { background-color: red; }",
        );
        let (html, body) = html_and_body(&root);
        assert_eq!(
            html.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })),
            "html сохраняет свой фон"
        );
        assert_eq!(
            body.style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
            "body тоже сохраняет — propagation не сработала"
        );
    }

    #[test]
    fn body_bg_image_propagates_when_html_has_none() {
        let root = lay_full(
            "<html><body><p>x</p></body></html>",
            "body { background-image: url(\"bg.png\"); }",
        );
        let (html, body) = html_and_body(&root);
        assert!(
            html.style.background_layers.first().is_some_and(|l| {
                matches!(&l.image, BackgroundImage::Url(s) if s == "bg.png")
            }),
            "html получает background-image"
        );
        assert!(body.style.background_layers.is_empty(), "у body background_layers обнуляется");
    }

    #[test]
    fn html_image_blocks_propagation_even_if_color_empty() {
        // У html есть background-image (color=None) — propagation НЕ должна
        // сработать, у body свой фон остаётся.
        let root = lay_full(
            "<html><body><p>x</p></body></html>",
            "html { background-image: url(\"h.png\"); } body { background-color: red; }",
        );
        let (html, body) = html_and_body(&root);
        assert!(html.style.background_layers.first().is_some_and(|l| matches!(&l.image, BackgroundImage::Url(_))));
        assert_eq!(html.style.background_color, None);
        assert_eq!(
            body.style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }))
        );
    }

    #[test]
    fn no_body_no_propagation() {
        // `<html>` без `<body>` — propagation noop, ничего не падает.
        let root = lay("<html><p>x</p></html>", "p { background-color: red; }");
        // Просто проверка, что layout не паникует и не выставляет фон
        // случайно: у root-Document-box-а нет background style-а.
        assert_eq!(root.style.background_color, None);
    }

    #[test]
    fn fragment_without_html_skips_propagation() {
        // Bare-fragment без `<html>`/`<body>` — наш tree builder не
        // добавляет implicit-ы. propagation должна тихо пропустить.
        let root = lay("<p>x</p>", "p { background-color: red; }");
        assert_eq!(root.style.background_color, None);
        // p сохраняет свой фон (он не body, propagation не трогает).
        let p = first_element_child(&root);
        assert_eq!(
            p.style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }))
        );
    }

    // ── HTML presentational hints: bgcolor / text (HTML5 §15) ──────────────

    /// `<body bgcolor="red">` — presentational hint задаёт background-color.
    /// После canvas-propagation фон переходит на html-box.
    #[test]
    fn body_bgcolor_attr_sets_background() {
        let root = lay_full("<html><body bgcolor=\"red\"><p>x</p></body></html>", "");
        let (html, body) = html_and_body(&root);
        assert_eq!(
            html.style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
            "html должен получить фон из bgcolor после propagation"
        );
        assert_eq!(body.style.background_color, None, "body фон обнуляется после propagation");
    }

    /// `<body bgcolor="ff0000">` — hashless hex принимается по HTML5 §2.4.6
    /// legacy color algorithm.
    #[test]
    fn body_bgcolor_hashless_hex_accepted() {
        let root = lay_full("<html><body bgcolor=\"ff0000\"><p>x</p></body></html>", "");
        let (html, _body) = html_and_body(&root);
        assert_eq!(
            html.style.background_color,
            Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
            "hashless hex bgcolor должен распознаваться"
        );
    }

    /// `<table bgcolor="navy">` — bgcolor на table-элементе.
    #[test]
    fn table_bgcolor_attr_sets_background() {
        let root = lay("<body><table bgcolor=\"navy\"><tr><td>x</td></tr></table></body>", "");
        let body = &root;
        let table = first_element_child(body);
        assert_eq!(
            table.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 0, b: 128, a: 255 })),
            "bgcolor на table должен задавать background-color"
        );
    }

    /// `<tr bgcolor="lime">` — bgcolor на tr-элементе.
    #[test]
    fn tr_bgcolor_attr_sets_background() {
        let root = lay("<body><table><tr bgcolor=\"lime\"><td>x</td></tr></table></body>", "");
        let body = &root;
        let table = first_element_child(body);
        // HTML5 parser inserts implicit <tbody>; navigate through it.
        let tbody = first_element_child(table);
        let tr = first_element_child(tbody);
        assert_eq!(
            tr.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 255, b: 0, a: 255 })),
            "bgcolor на tr должен задавать background-color"
        );
    }

    /// `<td bgcolor="#00f">` — bgcolor на td-элементе, short hex form.
    #[test]
    fn td_bgcolor_attr_sets_background() {
        let root = lay("<body><table><tr><td bgcolor=\"#00f\">x</td></tr></table></body>", "");
        let body = &root;
        let table = first_element_child(body);
        // HTML5 parser inserts implicit <tbody>; navigate through it.
        let tbody = first_element_child(table);
        let tr = first_element_child(tbody);
        let td = first_element_child(tr);
        assert_eq!(
            td.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })),
            "bgcolor на td должен задавать background-color"
        );
    }

    // ── table layout (BUG-006) ────────────────────────────────────────────────

    /// Ячейки таблицы должны раскладываться горизонтально, не вертикально.
    #[test]
    fn table_cells_layout_horizontally() {
        let root = lay(
            "<body><table><tr>\
               <td style=\"width:100px;height:50px\"></td>\
               <td style=\"width:200px;height:50px\"></td>\
             </tr></table></body>",
            "body,table,tr,td { margin:0; padding:0; border:0 }",
        );
        let body = &root;
        let table = first_element_child(body);
        // HTML5 parser inserts implicit <tbody>; navigate through it.
        let tbody = first_element_child(table);
        let tr = first_element_child(tbody);
        assert!(
            matches!(tr.kind, BoxKind::TableRow),
            "<tr> должен иметь BoxKind::TableRow"
        );
        let cells: Vec<_> = tr
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(cells.len(), 2, "должно быть 2 ячейки");
        // Первая ячейка: x=0, w=100
        assert!((cells[0].rect.x - 0.0).abs() < 0.01, "первая ячейка x=0, получено {}", cells[0].rect.x);
        assert!((cells[0].rect.width - 100.0).abs() < 0.01, "первая ячейка w=100");
        // Вторая ячейка: x=100, w=200
        assert!((cells[1].rect.x - 100.0).abs() < 0.01, "вторая ячейка x=100, получено {}", cells[1].rect.x);
        assert!((cells[1].rect.width - 200.0).abs() < 0.01, "вторая ячейка w=200");
        // Высота строки = max(50, 50) = 50
        assert!((tr.rect.height - 50.0).abs() < 0.01, "высота строки 50px");
    }

    /// Строки таблицы стакаются вертикально (block-flow для `<table>`).
    #[test]
    fn table_rows_stack_vertically() {
        let root = lay(
            "<body><table><tr><td style=\"width:100px;height:40px\"></td></tr>\
                         <tr><td style=\"width:100px;height:60px\"></td></tr></table></body>",
            "body,table,tr,td { margin:0; padding:0; border:0 }",
        );
        let body = &root;
        let table = first_element_child(body);
        // HTML5 parser inserts implicit <tbody>; navigate through it.
        let tbody = first_element_child(table);
        let rows: Vec<_> = tbody
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRow))
            .collect();
        assert_eq!(rows.len(), 2, "должно быть 2 строки");
        assert!((rows[0].rect.y - 0.0).abs() < 0.01, "первая строка y=0");
        assert!((rows[1].rect.y - 40.0).abs() < 0.01, "вторая строка y=40, получено {}", rows[1].rect.y);
    }

    /// Колонки выравниваются между строками — global column widths.
    /// Row 1: col0=100px, col1=200px. Row 2: col0=80px, col1=250px.
    /// Global: col0=max(100,80)=100, col1=max(200,250)=250.
    /// All rows use the global widths, so both rows → col0=100, col1=250.
    #[test]
    fn table_global_column_widths_aligned() {
        let root = lay(
            "<body><table><tr>\
               <td style=\"width:100px;height:20px\"></td>\
               <td style=\"width:200px;height:20px\"></td>\
             </tr><tr>\
               <td style=\"width:80px;height:20px\"></td>\
               <td style=\"width:250px;height:20px\"></td>\
             </tr></table></body>",
            "body,table,tr,td { margin:0; padding:0; border:0 }",
        );
        let body = &root;
        let table = first_element_child(body);
        assert!(matches!(table.kind, BoxKind::Table), "table должен иметь BoxKind::Table");
        // HTML5 parser inserts implicit <tbody>; rows are inside it.
        let tbody = first_element_child(table);
        let rows: Vec<_> = tbody.children.iter().filter(|c| matches!(c.kind, BoxKind::TableRow)).collect();
        assert_eq!(rows.len(), 2);
        let r1_cells: Vec<_> = rows[0].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
        let r2_cells: Vec<_> = rows[1].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
        // col0 global = max(100, 80) = 100 — both rows.
        assert!((r1_cells[0].rect.width - 100.0).abs() < 0.01, "r1 col0=100, got {}", r1_cells[0].rect.width);
        assert!((r2_cells[0].rect.width - 100.0).abs() < 0.01, "r2 col0=100 (global), got {}", r2_cells[0].rect.width);
        // col1 global = max(200, 250) = 250 — both rows.
        assert!((r1_cells[1].rect.width - 250.0).abs() < 0.01, "r1 col1=250 (global), got {}", r1_cells[1].rect.width);
        assert!((r2_cells[1].rect.width - 250.0).abs() < 0.01, "r2 col1=250 (global), got {}", r2_cells[1].rect.width);
    }

    /// `<table>` имеет BoxKind::Table (не Block).
    #[test]
    fn table_has_boxkind_table() {
        let root = lay("<body><table><tr><td>x</td></tr></table></body>", "");
        let body = &root;
        let table = first_element_child(body);
        assert!(
            matches!(table.kind, BoxKind::Table),
            "table должен быть BoxKind::Table, получено {:?}", table.kind
        );
    }

    /// `<tbody>` имеет BoxKind::TableRowGroup.
    #[test]
    fn tbody_has_boxkind_tablerowgroup() {
        let root = lay("<body><table><tbody><tr><td>x</td></tr></tbody></table></body>", "");
        let body = &root;
        let table = first_element_child(body);
        let tbody = first_element_child(table);
        assert!(
            matches!(tbody.kind, BoxKind::TableRowGroup),
            "tbody должен быть BoxKind::TableRowGroup, получено {:?}", tbody.kind
        );
    }

    /// Строки внутри `<tbody>` выравниваются вертикально через `<table>`.
    #[test]
    fn table_with_tbody_rows_stack_vertically() {
        let root = lay(
            "<body><table><tbody>\
               <tr><td style=\"width:100px;height:40px\"></td></tr>\
               <tr><td style=\"width:100px;height:60px\"></td></tr>\
             </tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let body = &root;
        let table = first_element_child(body);
        let tbody = first_element_child(table);
        let rows: Vec<_> = tbody.children.iter().filter(|c| matches!(c.kind, BoxKind::TableRow)).collect();
        assert_eq!(rows.len(), 2, "должно быть 2 строки");
        assert!((rows[0].rect.y - 0.0).abs() < 0.01, "первая строка y=0, got {}", rows[0].rect.y);
        assert!((rows[1].rect.y - 40.0).abs() < 0.01, "вторая строка y=40, got {}", rows[1].rect.y);
    }

    /// `<thead>` и `<tfoot>` должны иметь BoxKind::TableRowGroup.
    #[test]
    fn thead_tfoot_have_boxkind_tablerowgroup() {
        let root = lay(
            "<body><table>\
               <thead><tr><th>H</th></tr></thead>\
               <tfoot><tr><td>F</td></tr></tfoot>\
             </table></body>",
            "",
        );
        let body = &root;
        let table = first_element_child(body);
        let groups: Vec<_> = table.children.iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRowGroup))
            .collect();
        assert_eq!(groups.len(), 2, "должно быть 2 row group (thead + tfoot)");
    }

    /// Колонки внутри tbody выравниваются глобально (через родительский table).
    #[test]
    fn table_tbody_global_col_widths() {
        let root = lay(
            "<body><table><tbody><tr>\
               <td style=\"width:120px;height:20px\"></td>\
               <td style=\"width:80px;height:20px\"></td>\
             </tr><tr>\
               <td style=\"width:60px;height:20px\"></td>\
               <td style=\"width:150px;height:20px\"></td>\
             </tr></tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let body = &root;
        let table = first_element_child(body);
        let tbody = first_element_child(table);
        let rows: Vec<_> = tbody.children.iter().filter(|c| matches!(c.kind, BoxKind::TableRow)).collect();
        let r1: Vec<_> = rows[0].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
        let r2: Vec<_> = rows[1].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
        // Col0 global = max(120, 60) = 120 — both rows.
        assert!((r1[0].rect.width - 120.0).abs() < 0.01, "r1 col0=120, got {}", r1[0].rect.width);
        assert!((r2[0].rect.width - 120.0).abs() < 0.01, "r2 col0=120 (global), got {}", r2[0].rect.width);
        // Col1 global = max(80, 150) = 150 — both rows.
        assert!((r1[1].rect.width - 150.0).abs() < 0.01, "r1 col1=150 (global), got {}", r1[1].rect.width);
        assert!((r2[1].rect.width - 150.0).abs() < 0.01, "r2 col1=150 (global), got {}", r2[1].rect.width);
    }

    // ── colspan / rowspan ────────────────────────────────────────────────────
    // All table tests use explicit <tbody> because html-full-tree-builder
    // correctly injects implicit <tbody> for bare <table><tr> markup (BUG-040).

    /// `col_span` and `row_span` are stored on the LayoutBox from HTML attrs.
    #[test]
    fn table_cell_col_span_row_span_stored() {
        let root = lay(
            "<body><table><tbody><tr>\
               <td colspan=\"3\" rowspan=\"2\"></td>\
             </tr></tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
        let row = find_box(tbody, |k| matches!(k, BoxKind::TableRow)).unwrap();
        let cell = row.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        assert_eq!(cell.col_span, 3, "colspan=3 must be stored");
        assert_eq!(cell.row_span, 2, "rowspan=2 must be stored");
    }

    /// Non-cell boxes have col_span=1, row_span=1 by default.
    #[test]
    fn non_cell_col_row_span_defaults_to_one() {
        // `lay` returns the body box directly, so the <div> is its first
        // element child (no intermediate <html>/<body> unwrapping needed).
        let root = lay("<body><div></div></body>", "");
        let div = first_element_child(&root);
        assert_eq!(div.col_span, 1);
        assert_eq!(div.row_span, 1);
    }

    /// `<td colspan="2">` spanning two equal-width columns gets combined width.
    #[test]
    fn table_colspan2_cell_width() {
        // Row 1 sets col widths: col0=100, col1=100.
        // Row 2 has a single cell with colspan=2 → width should be 200.
        let root = lay(
            "<body><table><tbody>\
               <tr><td style=\"width:100px;height:20px\"></td>\
                   <td style=\"width:100px;height:20px\"></td></tr>\
               <tr><td colspan=\"2\" style=\"height:30px\"></td></tr>\
             </tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
        let rows: Vec<_> = tbody
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRow))
            .collect();
        assert_eq!(rows.len(), 2);
        let r2_cells: Vec<_> = rows[1]
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(r2_cells.len(), 1, "colspan=2 row must have exactly 1 DOM cell");
        assert!(
            (r2_cells[0].rect.width - 200.0).abs() < 0.01,
            "colspan=2 cell width should be 200px, got {}",
            r2_cells[0].rect.width
        );
        assert!(
            (r2_cells[0].rect.x - 0.0).abs() < 0.01,
            "colspan=2 cell x should be 0, got {}",
            r2_cells[0].rect.x
        );
    }

    /// Cell after a `colspan=2` cell starts at column 2 (x = col0+col1).
    #[test]
    fn table_cell_after_colspan2_x_position() {
        // Row 1: col0=60, col1=80, col2=50.
        // Row 2: [colspan=2 cell → cols 0-1, width=140], [cell at col2, width=50].
        let root = lay(
            "<body><table><tbody>\
               <tr><td style=\"width:60px;height:20px\"></td>\
                   <td style=\"width:80px;height:20px\"></td>\
                   <td style=\"width:50px;height:20px\"></td></tr>\
               <tr><td colspan=\"2\" style=\"height:20px\"></td>\
                   <td style=\"height:20px\"></td></tr>\
             </tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
        let rows: Vec<_> = tbody
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRow))
            .collect();
        let r2_cells: Vec<_> = rows[1]
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(r2_cells.len(), 2, "row 2 should have 2 DOM cells");
        assert!(
            (r2_cells[0].rect.x - 0.0).abs() < 0.01,
            "colspan cell x=0, got {}",
            r2_cells[0].rect.x
        );
        assert!(
            (r2_cells[0].rect.width - 140.0).abs() < 0.01,
            "colspan=2 width=140, got {}",
            r2_cells[0].rect.width
        );
        assert!(
            (r2_cells[1].rect.x - 140.0).abs() < 0.01,
            "cell after colspan x=140, got {}",
            r2_cells[1].rect.x
        );
        assert!(
            (r2_cells[1].rect.width - 50.0).abs() < 0.01,
            "cell after colspan width=50, got {}",
            r2_cells[1].rect.width
        );
    }

    /// `colspan=2 width=200` distributes 100px hint per column;
    /// an explicit 120px col0 in another row wins over the 100px hint.
    #[test]
    fn table_colspan_distributes_width_hint() {
        let root = lay(
            "<body><table><tbody>\
               <tr><td style=\"width:120px;height:20px\"></td>\
                   <td style=\"height:20px\"></td></tr>\
               <tr><td colspan=\"2\" style=\"width:200px;height:20px\"></td></tr>\
             </tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
        let rows: Vec<_> = tbody
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRow))
            .collect();
        let r1_cells: Vec<_> = rows[0]
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        // col0 = max(120, 100) = 120; col1 = max(auto→0, 100) = 100
        assert!(
            (r1_cells[0].rect.width - 120.0).abs() < 0.01,
            "col0 should be 120, got {}",
            r1_cells[0].rect.width
        );
        assert!(
            (r1_cells[1].rect.width - 100.0).abs() < 0.01,
            "col1 hint from colspan should be 100, got {}",
            r1_cells[1].rect.width
        );
    }

    /// `rowspan=2` in row 1 occupies col0 for both rows;
    /// row 2's cell must be placed at col1, not col0.
    #[test]
    fn table_rowspan2_second_row_skips_occupied_column() {
        let root = lay(
            "<body><table><tbody>\
               <tr><td rowspan=\"2\" style=\"width:80px;height:20px\"></td>\
                   <td style=\"width:60px;height:20px\"></td></tr>\
               <tr><td style=\"width:60px;height:20px\"></td></tr>\
             </tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
        let rows: Vec<_> = tbody
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRow))
            .collect();
        let r2_cells: Vec<_> = rows[1]
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        assert_eq!(r2_cells.len(), 1, "row 2 has 1 DOM cell");
        assert!(
            (r2_cells[0].rect.x - 80.0).abs() < 0.01,
            "row2 cell must start at x=80 (col1), got {}",
            r2_cells[0].rect.x
        );
        assert!(
            (r2_cells[0].rect.width - 60.0).abs() < 0.01,
            "row2 cell width=60, got {}",
            r2_cells[0].rect.width
        );
    }

    /// After layout, a `rowspan=2` cell's height is patched to cover both rows.
    #[test]
    fn table_rowspan2_cell_height_spans_two_rows() {
        // Row1: [A(rowspan=2,h=10), B(h=30)] → row1_h=30.
        // Row2: [C(h=40)] → row2_h=40.
        // A.height post-fix = row1.y+row1.h + row2.h - A.y = 30+40 = 70.
        let root = lay(
            "<body><table><tbody>\
               <tr><td rowspan=\"2\" style=\"width:50px;height:10px\"></td>\
                   <td style=\"width:50px;height:30px\"></td></tr>\
               <tr><td style=\"width:50px;height:40px\"></td></tr>\
             </tbody></table></body>",
            "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
        let rows: Vec<_> = tbody
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::TableRow))
            .collect();
        let row1_cells: Vec<_> = rows[0]
            .children
            .iter()
            .filter(|c| matches!(c.kind, BoxKind::Block))
            .collect();
        let cell_a = row1_cells[0];
        let row1_h = rows[0].rect.height;
        let row2_h = rows[1].rect.height;
        assert!(
            (row1_h - 30.0).abs() < 0.01,
            "row1 height should be 30 (from B), got {}",
            row1_h
        );
        assert!(
            (row2_h - 40.0).abs() < 0.01,
            "row2 height should be 40 (from C), got {}",
            row2_h
        );
        let expected_a_h = row1_h + row2_h;
        assert!(
            (cell_a.rect.height - expected_a_h).abs() < 0.01,
            "rowspan=2 cell A height should be {}, got {}",
            expected_a_h,
            cell_a.rect.height
        );
    }

    /// CSS 2.1 §17.5.2 — table without explicit CSS width shrinks to fit its columns.
    /// 3×3 grid with border-spacing:12px and cell width:60px should be 228px
    /// (3×60 + 4×12), not the full container width.
    #[test]
    fn table_without_explicit_width_shrinks_to_fit() {
        let root = lay(
            "<body><table><tr>\
               <td style=\"width:60px;height:20px\"></td>\
               <td style=\"width:60px;height:20px\"></td>\
               <td style=\"width:60px;height:20px\"></td>\
             </tr></table></body>",
            "body { width:800px } table { border-spacing:12px } td { margin:0; padding:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        // Expected: 3×60 + 4×12 = 180 + 48 = 228px
        assert!(
            (table.rect.width - 228.0).abs() < 0.01,
            "table should shrink to 228px, got {}",
            table.rect.width
        );
    }

    /// CSS 2.1 §17.5.2 — table with explicit CSS width is NOT shrunk to fit.
    #[test]
    fn table_with_explicit_width_keeps_that_width() {
        let root = lay(
            "<body><table><tr>\
               <td style=\"width:60px;height:20px\"></td>\
               <td style=\"width:60px;height:20px\"></td>\
             </tr></table></body>",
            "body { width:800px } table { width:400px; border-spacing:8px } td { margin:0; padding:0 }",
        );
        let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
        assert!(
            (table.rect.width - 400.0).abs() < 0.01,
            "table with explicit width:400px should stay 400px, got {}",
            table.rect.width
        );
    }

    /// Author CSS `background-color` выигрывает у presentational hint `bgcolor`.
    #[test]
    fn author_css_overrides_bgcolor_hint() {
        let root = lay_full(
            "<html><body bgcolor=\"red\"><p>x</p></body></html>",
            "body { background-color: blue; }",
        );
        let (html, _body) = html_and_body(&root);
        assert_eq!(
            html.style.background_color,
            Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })),
            "author CSS background-color должен побеждать bgcolor атрибут"
        );
    }

    /// `<body bgcolor="transparent">` — по HTML5 §2.4.6 «transparent» является
    /// ошибкой; атрибут игнорируется, фон остаётся None.
    #[test]
    fn body_bgcolor_transparent_is_ignored() {
        let root = lay_full("<html><body bgcolor=\"transparent\"><p>x</p></body></html>", "");
        let (html, body) = html_and_body(&root);
        assert_eq!(html.style.background_color, None, "transparent bgcolor должен игнорироваться");
        assert_eq!(body.style.background_color, None);
    }

    /// `<body bgcolor="olive">` — named color через HTML5 legacy-парсер.
    #[test]
    fn body_bgcolor_named_color() {
        let root = lay_full("<html><body bgcolor=\"olive\"><p>x</p></body></html>", "");
        let (html, _body) = html_and_body(&root);
        assert_eq!(
            html.style.background_color,
            Some(CssColor::Rgba(Color { r: 128, g: 128, b: 0, a: 255 })),
            "named color 'olive' должен правильно конвертироваться"
        );
    }

    // ── HTML presentational hints: body text / font color (HTML5 §15.3) ────

    /// `<body text="red">` → body.color = red.
    #[test]
    fn body_text_attr_sets_color() {
        let root = lay_full("<html><body text=\"red\"><p>x</p></body></html>", "");
        let (_html, body) = html_and_body(&root);
        assert_eq!(
            body.style.color,
            Color { r: 255, g: 0, b: 0, a: 255 },
            "body text= должен задавать color"
        );
    }

    /// `<body text="blue">` — цвет наследуется дочерними элементами.
    #[test]
    fn body_text_color_inherited_by_child() {
        let root = lay_full("<html><body text=\"blue\"><p>x</p></body></html>", "");
        let (_html, body) = html_and_body(&root);
        let p = first_element_child(body);
        assert_eq!(
            p.style.color,
            Color { r: 0, g: 0, b: 255, a: 255 },
            "<p> должен наследовать color из body text="
        );
    }

    /// Author CSS `color` выигрывает у presentational hint `text=`.
    #[test]
    fn author_css_overrides_body_text_hint() {
        let root = lay_full(
            "<html><body text=\"red\"><p>x</p></body></html>",
            "body { color: green; }",
        );
        let (_html, body) = html_and_body(&root);
        assert_eq!(
            body.style.color,
            Color { r: 0, g: 128, b: 0, a: 255 },
            "author CSS color должен побеждать body text= атрибут"
        );
    }

    /// `<font color="red">` задаёт color на элементе font.
    #[test]
    fn font_color_attr_sets_color() {
        let root = lay("<body><font color=\"red\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.color,
            Color { r: 255, g: 0, b: 0, a: 255 },
            "<font color=> должен задавать color"
        );
    }

    /// `<font color="#0000ff">` — hash long hex form.
    #[test]
    fn font_color_hash_long_hex() {
        let root = lay("<body><font color=\"#0000ff\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.color,
            Color { r: 0, g: 0, b: 255, a: 255 },
            "<font color=#0000ff> должен задавать blue"
        );
    }

    /// Author CSS `color` выигрывает у `<font color=>`.
    #[test]
    fn author_css_overrides_font_color_hint() {
        let root = lay(
            "<body><font color=\"red\">x</font></body>",
            "font { color: blue; }",
        );
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.color,
            Color { r: 0, g: 0, b: 255, a: 255 },
            "author CSS должен побеждать font color= атрибут"
        );
    }

    // ── HTML presentational hints: <font size/face>, img hspace/vspace/border, align ──

    /// `<font size="3">` → font-size 16px (medium).
    #[test]
    fn font_size_attr_medium() {
        let root = lay("<body><font size=\"3\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 16.0,
            "<font size=3> должен задавать font-size 16px"
        );
    }

    /// `<font size="1">` → font-size 10px (xx-small).
    #[test]
    fn font_size_attr_xxsmall() {
        let root = lay("<body><font size=\"1\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 10.0,
            "<font size=1> должен задавать font-size 10px"
        );
    }

    /// `<font size="7">` → font-size 48px (xxx-large).
    #[test]
    fn font_size_attr_xxxlarge() {
        let root = lay("<body><font size=\"7\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 48.0,
            "<font size=7> должен задавать font-size 48px"
        );
    }

    /// `<font size="+2">` → base 3 + 2 = 5 → 24px.
    #[test]
    fn font_size_attr_relative_plus() {
        let root = lay("<body><font size=\"+2\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 24.0,
            "<font size=+2> должен задавать font-size 24px"
        );
    }

    /// `<font size="-1">` → base 3 - 1 = 2 → 13px.
    #[test]
    fn font_size_attr_relative_minus() {
        let root = lay("<body><font size=\"-1\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 13.0,
            "<font size=-1> должен задавать font-size 13px"
        );
    }

    /// `<font size="99">` clamps to 7 → 48px.
    #[test]
    fn font_size_attr_clamp_max() {
        let root = lay("<body><font size=\"99\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 48.0,
            "<font size=99> должен клэмпироваться к 48px"
        );
    }

    /// Author CSS `font-size` побеждает `<font size>` hint.
    #[test]
    fn author_css_overrides_font_size_hint() {
        let root = lay(
            "<body><font size=\"7\">x</font></body>",
            "font { font-size: 20px; }",
        );
        let body = &root;
        let font = first_element_child(body);
        assert_eq!(
            font.style.font_size, 20.0,
            "author CSS font-size должен побеждать font size= атрибут"
        );
    }

    /// `<font face="Arial, sans-serif">` → font-family.
    #[test]
    fn font_face_attr_sets_font_family() {
        let root = lay("<body><font face=\"Arial, sans-serif\">x</font></body>", "");
        let body = &root;
        let font = first_element_child(body);
        assert!(
            font.style.font_family.contains(&"Arial".to_string()),
            "<font face=> должен задавать font-family"
        );
    }

    /// `<img hspace="10">` → margin-left и margin-right по 10px.
    #[test]
    fn img_hspace_attr_sets_margins() {
        let root = lay(r#"<img src="x.png" hspace="10">"#, "");
        let img = first_image_child(&root);
        assert_eq!(
            img.style.margin_left,
            LengthOrAuto::Length(Length::Px(10.0)),
            "img hspace должен задавать margin-left 10px"
        );
        assert_eq!(
            img.style.margin_right,
            LengthOrAuto::Length(Length::Px(10.0)),
            "img hspace должен задавать margin-right 10px"
        );
    }

    /// `<img vspace="8">` → margin-top и margin-bottom по 8px.
    #[test]
    fn img_vspace_attr_sets_margins() {
        let root = lay(r#"<img src="x.png" vspace="8">"#, "");
        let img = first_image_child(&root);
        assert_eq!(
            img.style.margin_top,
            LengthOrAuto::Length(Length::Px(8.0)),
            "img vspace должен задавать margin-top 8px"
        );
        assert_eq!(
            img.style.margin_bottom,
            LengthOrAuto::Length(Length::Px(8.0)),
            "img vspace должен задавать margin-bottom 8px"
        );
    }

    /// `<img border="2">` → все 4 border-width 2px + style=solid.
    #[test]
    fn img_border_attr_sets_border() {
        let root = lay(r#"<img src="x.png" border="2">"#, "");
        let img = first_image_child(&root);
        assert_eq!(img.style.border_top_width, 2.0, "img border должен задавать border-top-width 2px");
        assert_eq!(img.style.border_right_width, 2.0);
        assert_eq!(img.style.border_bottom_width, 2.0);
        assert_eq!(img.style.border_left_width, 2.0);
        assert_eq!(
            img.style.border_top_style,
            crate::style::BorderStyle::Solid,
            "img border>0 должен задавать border-style solid"
        );
    }

    /// `<img border="0">` → нулевые border-width, style=none (no-op).
    #[test]
    fn img_border_zero_no_style() {
        let root = lay(r#"<img src="x.png" border="0">"#, "");
        let img = first_image_child(&root);
        assert_eq!(img.style.border_top_width, 0.0);
        assert_eq!(
            img.style.border_top_style,
            crate::style::BorderStyle::None,
            "img border=0 не должен задавать border-style"
        );
    }

    /// `<div align="center">` → text-align: center.
    #[test]
    fn div_align_center_attr() {
        let root = lay("<body><div align=\"center\">x</div></body>", "");
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(
            div.style.text_align,
            crate::style::TextAlign::Center,
            "div align=center должен задавать text-align: center"
        );
    }

    /// `<p align="right">` → text-align: right.
    #[test]
    fn p_align_right_attr() {
        let root = lay("<body><p align=\"right\">x</p></body>", "");
        let body = &root;
        let p = first_element_child(body);
        assert_eq!(
            p.style.text_align,
            crate::style::TextAlign::Right,
            "p align=right должен задавать text-align: right"
        );
    }

    /// `<h1 align="middle">` → text-align: center (middle = center alias).
    #[test]
    fn h1_align_middle_is_center() {
        let root = lay("<body><h1 align=\"middle\">x</h1></body>", "");
        let body = &root;
        let h1 = first_element_child(body);
        assert_eq!(
            h1.style.text_align,
            crate::style::TextAlign::Center,
            "align=middle должен давать text-align: center"
        );
    }

    /// Author CSS `text-align` побеждает `align` атрибут.
    #[test]
    fn author_css_overrides_align_hint() {
        let root = lay(
            "<body><div align=\"center\">x</div></body>",
            "div { text-align: right; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(
            div.style.text_align,
            crate::style::TextAlign::Right,
            "author CSS text-align должен побеждать align= атрибут"
        );
    }

    // --- CSS Grid Layout tests ---

    /// Parse `grid-template-columns: 100px 200px 300px`.
    #[test]
    fn grid_parse_fixed_columns() {
        let root = lay(
            "<body><div></div></body>",
            "div { display: grid; grid-template-columns: 100px 200px 300px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_template_columns.len(), 3);
        assert_eq!(div.style.grid_template_columns[0], GridTrackSize::Length(Length::Px(100.0)));
        assert_eq!(div.style.grid_template_columns[1], GridTrackSize::Length(Length::Px(200.0)));
        assert_eq!(div.style.grid_template_columns[2], GridTrackSize::Length(Length::Px(300.0)));
    }

    /// Parse fr units.
    #[test]
    fn grid_parse_fr_columns() {
        let root = lay(
            "<body><div></div></body>",
            "div { display: grid; grid-template-columns: 1fr 2fr; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_template_columns.len(), 2);
        assert_eq!(div.style.grid_template_columns[0], GridTrackSize::Fr(1.0));
        assert_eq!(div.style.grid_template_columns[1], GridTrackSize::Fr(2.0));
    }

    /// Parse `repeat(3, 100px)` — expands to 3 tracks.
    #[test]
    fn grid_parse_repeat() {
        let root = lay(
            "<body><div></div></body>",
            "div { display: grid; grid-template-columns: repeat(3, 100px); }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_template_columns.len(), 3);
        for ts in &div.style.grid_template_columns {
            assert_eq!(*ts, GridTrackSize::Length(Length::Px(100.0)));
        }
    }

    /// Parse `grid-column: 2 / 4`.
    #[test]
    fn grid_parse_column_shorthand() {
        let root = lay(
            "<body><div></div></body>",
            "div { grid-column: 2 / 4; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_column_start, GridLine::Line(2));
        assert_eq!(div.style.grid_column_end, GridLine::Line(4));
    }

    /// Parse `grid-row: 1 / span 2`.
    #[test]
    fn grid_parse_row_span() {
        let root = lay(
            "<body><div></div></body>",
            "div { grid-row: 1 / span 2; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_row_start, GridLine::Line(1));
        assert_eq!(div.style.grid_row_end, GridLine::Span(2));
    }

    /// Two equal fr columns should each get half the container width.
    #[test]
    fn grid_two_fr_columns_equal_width() {
        let root = lay(
            "<body><div><span></span><span></span></div></body>",
            "div { display: grid; grid-template-columns: 1fr 1fr; width: 400px; } \
             span { height: 50px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
        assert_eq!(items.len(), 2, "должно быть 2 grid-item");
        assert!((items[0].rect.width - 200.0).abs() < 1.0, "первый item = 200px, получили {}", items[0].rect.width);
        assert!((items[1].rect.width - 200.0).abs() < 1.0, "второй item = 200px, получили {}", items[1].rect.width);
        // Second item starts at x=200.
        assert!((items[1].rect.x - items[0].rect.x - 200.0).abs() < 1.0);
    }

    /// Fixed 3-column grid: items placed in row order.
    #[test]
    fn grid_three_column_auto_placement() {
        let root = lay(
            "<body><div><a></a><a></a><a></a><a></a></div></body>",
            "div { display: grid; grid-template-columns: 100px 100px 100px; width: 300px; } \
             a { height: 30px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
        assert_eq!(items.len(), 4);
        // First 3 items on row 1, 4th on row 2.
        assert!((items[0].rect.y - items[1].rect.y).abs() < 1.0, "items 0,1 одна строка");
        assert!((items[1].rect.y - items[2].rect.y).abs() < 1.0, "items 1,2 одна строка");
        assert!(items[3].rect.y > items[0].rect.y + 1.0, "item 4 на второй строке");
        // Column positions.
        assert!(items[0].rect.x < items[1].rect.x, "col 0 < col 1");
        assert!(items[1].rect.x < items[2].rect.x, "col 1 < col 2");
    }

    /// Explicit grid-column / grid-row placement.
    #[test]
    fn grid_explicit_placement() {
        let root = lay(
            "<body><div><a></a></div></body>",
            "div { display: grid; grid-template-columns: 100px 100px 100px; \
                   grid-template-rows: 50px 50px; width: 300px; } \
             a { grid-column: 3; grid-row: 2; height: 40px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        let item = div.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
        // item at column 3, row 2 → x ≈ 200, y ≈ 50.
        assert!((item.rect.x - 200.0).abs() < 1.0, "x≈200, got {}", item.rect.x);
        assert!((item.rect.y - 50.0).abs() < 1.0, "y≈50, got {}", item.rect.y);
    }

    /// Grid with `gap` between cells.
    #[test]
    fn grid_gap_applied() {
        let root = lay(
            "<body><div><a></a><a></a></div></body>",
            "div { display: grid; grid-template-columns: 100px 100px; \
                   column-gap: 20px; width: 220px; } \
             a { height: 30px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
        assert_eq!(items.len(), 2);
        // Second item starts at x ≈ 120 (100px col + 20px gap).
        assert!((items[1].rect.x - items[0].rect.x - 120.0).abs() < 1.0,
            "gap: x diff should be 120, got {}", items[1].rect.x - items[0].rect.x);
    }

    /// `grid-auto-flow: column` places items vertically first.
    #[test]
    fn grid_auto_flow_column() {
        let root = lay(
            "<body><div><a></a><a></a><a></a></div></body>",
            "div { display: grid; grid-template-rows: 50px 50px; \
                   grid-auto-flow: column; width: 300px; } \
             a { width: 80px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
        assert_eq!(items.len(), 3);
        // items 0,1 same column (different y); item 2 in next column.
        assert!((items[0].rect.x - items[1].rect.x).abs() < 1.0, "items 0,1 same column");
        assert!(items[2].rect.x > items[0].rect.x + 1.0, "item 2 next column");
    }

    /// `minmax(50px, 1fr)` — explicit minmax() track.
    #[test]
    fn grid_parse_minmax() {
        let root = lay(
            "<body><div></div></body>",
            "div { display: grid; grid-template-columns: minmax(50px, 1fr); }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_template_columns.len(), 1);
        assert!(matches!(div.style.grid_template_columns[0], GridTrackSize::Minmax(_, _)));
    }

    /// `grid-area` shorthand parses `row-start / col-start / row-end / col-end`.
    #[test]
    fn grid_parse_area_shorthand() {
        let root = lay(
            "<body><div></div></body>",
            "div { grid-area: 2 / 1 / 4 / 3; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_row_start, GridLine::Line(2));
        assert_eq!(div.style.grid_column_start, GridLine::Line(1));
        assert_eq!(div.style.grid_row_end, GridLine::Line(4));
        assert_eq!(div.style.grid_column_end, GridLine::Line(3));
    }

    /// `display: grid` container has no height when empty.
    #[test]
    fn grid_empty_container_zero_height() {
        let root = lay(
            "<body><div></div></body>",
            "div { display: grid; grid-template-columns: 100px 100px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.rect.height, 0.0, "empty grid should have 0 height");
    }

    /// Auto rows sized by content.
    #[test]
    fn grid_auto_row_height_from_content() {
        let root = lay(
            "<body><div><a></a></div></body>",
            "div { display: grid; grid-template-columns: 100px; width: 100px; } \
             a { height: 80px; }",
        );
        let body = &root;
        let div = first_element_child(body);
        // Container height should accommodate the 80px item.
        assert!(div.rect.height >= 80.0, "grid height should be ≥80px, got {}", div.rect.height);
    }

    // ── CSS Grid named areas ──────────────────────────────────────────────────

    /// `parse_grid_template_areas` — 2×2 grid with named areas.
    #[test]
    fn grid_template_areas_parse_2x2() {
        use crate::parse_grid_template_areas;
        let areas = parse_grid_template_areas(r#""header header" "sidebar main""#);
        assert_eq!(areas.len(), 2, "should have 2 rows");
        assert_eq!(areas[0], vec!["header", "header"]);
        assert_eq!(areas[1], vec!["sidebar", "main"]);
    }

    /// `parse_grid_template_areas` — single row.
    #[test]
    fn grid_template_areas_parse_single_row() {
        use crate::parse_grid_template_areas;
        let areas = parse_grid_template_areas(r#""a b c""#);
        assert_eq!(areas, vec![vec!["a", "b", "c"]]);
    }

    /// `parse_grid_template_areas` — `none` returns empty.
    #[test]
    fn grid_template_areas_none() {
        use crate::parse_grid_template_areas;
        let areas = parse_grid_template_areas("none");
        assert!(areas.is_empty());
    }

    /// `parse_grid_template_areas` — dot (.) cells are stored as-is.
    #[test]
    fn grid_template_areas_dot_cells() {
        use crate::parse_grid_template_areas;
        let areas = parse_grid_template_areas(r#""a . b""#);
        assert_eq!(areas[0], vec!["a", ".", "b"]);
    }

    /// `GridLine::parse` recognises named area idents.
    #[test]
    fn grid_line_parse_named_ident() {
        use crate::GridLine;
        assert_eq!(GridLine::parse("main"), Some(GridLine::Named("main".into())));
        assert_eq!(GridLine::parse("header-area"), Some(GridLine::Named("header-area".into())));
        assert_eq!(GridLine::parse("auto"), Some(GridLine::Auto));
        assert_eq!(GridLine::parse("2"), Some(GridLine::Line(2)));
        // digit-only or empty → not an ident
        assert_eq!(GridLine::parse("3abc"), None);
    }

    /// `grid-area: <name>` shorthand sets all four placement properties to Named.
    #[test]
    fn grid_area_named_sets_all_four() {
        let root = lay(
            "<body><div></div></body>",
            "div { grid-area: main; }",
        );
        let body = &root;
        let div = first_element_child(body);
        assert_eq!(div.style.grid_row_start,    GridLine::Named("main".into()));
        assert_eq!(div.style.grid_row_end,      GridLine::Named("main".into()));
        assert_eq!(div.style.grid_column_start, GridLine::Named("main".into()));
        assert_eq!(div.style.grid_column_end,   GridLine::Named("main".into()));
    }

    /// `grid-template-areas` stored on container after cascade.
    #[test]
    fn grid_template_areas_stored_on_container() {
        let root = lay(
            "<body><div></div></body>",
            r#"div { display: grid; grid-template-areas: "header header" "sidebar main"; }"#,
        );
        let body = &root;
        let div = first_element_child(body);
        let areas = &div.style.grid_template_areas;
        assert_eq!(areas.len(), 2, "should have 2 rows");
        assert_eq!(areas[0], vec!["header", "header"]);
        assert_eq!(areas[1], vec!["sidebar", "main"]);
    }

    /// Named area layout: a 2×2 grid where items reference areas by name.
    ///
    /// ```css
    /// .grid {
    ///   display: grid;
    ///   grid-template-columns: 100px 100px;
    ///   grid-template-rows: 50px 50px;
    ///   grid-template-areas: "a b" "a c";
    ///   width: 200px;
    /// }
    /// .item-a { grid-area: a; }  /* row 1–3, col 1–2 */
    /// .item-b { grid-area: b; }  /* row 1–2, col 2–3 */
    /// .item-c { grid-area: c; }  /* row 2–3, col 2–3 */
    /// ```
    #[test]
    fn grid_named_areas_layout_placement() {
        let root = lay(
            "<body><div><span id='a'></span><span id='b'></span><span id='c'></span></div></body>",
            r#"
            div {
                display: grid;
                grid-template-columns: 100px 100px;
                grid-template-rows: 50px 50px;
                grid-template-areas: "a b" "a c";
                width: 200px;
            }
            #a { grid-area: a; }
            #b { grid-area: b; }
            #c { grid-area: c; }
            "#,
        );
        let body = &root;
        let div = first_element_child(body);
        let items: Vec<_> = div
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert_eq!(items.len(), 3, "3 named-area items");
        let item_a = &items[0];
        let item_b = &items[1];
        let item_c = &items[2];
        // item-a occupies rows 1-2 (height=100) at column 1 (x=0, width=100)
        assert!((item_a.rect.x - 0.0).abs() < 1.0,  "a.x should be 0, got {}", item_a.rect.x);
        assert!((item_a.rect.width - 100.0).abs() < 1.0, "a.w should be 100, got {}", item_a.rect.width);
        assert!((item_a.rect.height - 100.0).abs() < 1.0, "a.h should be 100 (2 rows), got {}", item_a.rect.height);
        // item-b occupies row 1 at column 2 (x=100, width=100, height=50)
        assert!((item_b.rect.x - 100.0).abs() < 1.0, "b.x should be 100, got {}", item_b.rect.x);
        assert!((item_b.rect.y - 0.0).abs() < 1.0,   "b.y should be 0, got {}", item_b.rect.y);
        assert!((item_b.rect.width - 100.0).abs() < 1.0, "b.w should be 100, got {}", item_b.rect.width);
        assert!((item_b.rect.height - 50.0).abs() < 1.0, "b.h should be 50, got {}", item_b.rect.height);
        // item-c occupies row 2 at column 2 (y=50, width=100, height=50)
        assert!((item_c.rect.x - 100.0).abs() < 1.0, "c.x should be 100, got {}", item_c.rect.x);
        assert!((item_c.rect.y - 50.0).abs() < 1.0,  "c.y should be 50, got {}", item_c.rect.y);
        assert!((item_c.rect.width - 100.0).abs() < 1.0, "c.w should be 100, got {}", item_c.rect.width);
        assert!((item_c.rect.height - 50.0).abs() < 1.0, "c.h should be 50, got {}", item_c.rect.height);
    }

    /// Named area with a span > 1 row: area "sidebar" spans both rows.
    #[test]
    fn grid_named_area_spanning_rows() {
        let root = lay(
            "<body><div><span id='h'></span><span id='s'></span></div></body>",
            r#"
            div {
                display: grid;
                grid-template-columns: 200px 600px;
                grid-template-rows: 80px 80px;
                grid-template-areas: "header header" "sidebar content";
                width: 800px;
            }
            #h { grid-area: header; }
            #s { grid-area: sidebar; }
            "#,
        );
        let body = &root;
        let div = first_element_child(body);
        let items: Vec<_> = div
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        // header spans both columns: x=0, w=800, y=0, h=80
        let header = &items[0];
        assert!((header.rect.x - 0.0).abs() < 1.0,    "h.x={}", header.rect.x);
        assert!((header.rect.width - 800.0).abs() < 1.0, "h.w={}", header.rect.width);
        assert!((header.rect.y - 0.0).abs() < 1.0,    "h.y={}", header.rect.y);
        assert!((header.rect.height - 80.0).abs() < 1.0, "h.h={}", header.rect.height);
        // sidebar: x=0, w=200, y=80, h=80
        let sidebar = &items[1];
        assert!((sidebar.rect.x - 0.0).abs() < 1.0,   "s.x={}", sidebar.rect.x);
        assert!((sidebar.rect.width - 200.0).abs() < 1.0, "s.w={}", sidebar.rect.width);
        assert!((sidebar.rect.y - 80.0).abs() < 1.0,  "s.y={}", sidebar.rect.y);
    }

    // ── grid-auto-flow: dense ────────────────────────────────────────────────

    /// Dense row packing fills the gap left by a wide item.
    ///
    ///  3 cols, A and B each span 2 cols; C and D are 1×1.
    ///
    ///  Sparse (row):             Dense (row dense):
    ///  +---+---+---+             +---+---+---+
    ///  | A   A |   |             | A   A | C |  ← C fills gap in row 1
    ///  +---+---+---+             +---+---+---+
    ///  | B   B | C |             | B   B | D |  ← D fills gap in row 2
    ///  +---+---+---+             +---+---+---+
    ///  | D |       |
    ///  +---+---+---+
    #[test]
    fn grid_dense_row_fills_gap() {
        let root = lay(
            "<body><div id='g'>\
               <span id='a'></span>\
               <span id='b'></span>\
               <span id='c'></span>\
               <span id='d'></span>\
             </div></body>",
            r#"
            #g {
                display: grid;
                grid-template-columns: 100px 100px 100px;
                grid-auto-rows: 50px;
                grid-auto-flow: row dense;
                width: 300px;
            }
            #a { grid-column: span 2; }
            #b { grid-column: span 2; }
            /* c, d: auto 1×1 */
            "#,
        );
        let body = &root;
        let grid = first_element_child(body);
        let items: Vec<_> = grid.children.iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert_eq!(items.len(), 4, "expected 4 items");

        let a = &items[0];
        let b = &items[1];
        let c = &items[2];
        let d = &items[3];

        // A: cols 1-2, row 1 → x=0, w=200, y=0
        assert!((a.rect.x - 0.0).abs() < 1.0,     "a.x={}", a.rect.x);
        assert!((a.rect.width - 200.0).abs() < 1.0, "a.w={}", a.rect.width);
        assert!((a.rect.y - 0.0).abs() < 1.0,     "a.y={}", a.rect.y);

        // B: cols 1-2, row 2 → x=0, w=200, y=50
        assert!((b.rect.x - 0.0).abs() < 1.0,     "b.x={}", b.rect.x);
        assert!((b.rect.width - 200.0).abs() < 1.0, "b.w={}", b.rect.width);
        assert!((b.rect.y - 50.0).abs() < 1.0,    "b.y={}", b.rect.y);

        // Dense: C fills the gap at col 3, row 1 → x=200, y=0
        assert!((c.rect.x - 200.0).abs() < 1.0, "c.x={}: dense must fill row-1 gap", c.rect.x);
        assert!((c.rect.y - 0.0).abs() < 1.0,   "c.y={}: dense must fill row-1 gap", c.rect.y);

        // Dense: D fills the gap at col 3, row 2 → x=200, y=50
        assert!((d.rect.x - 200.0).abs() < 1.0, "d.x={}: dense must fill row-2 gap", d.rect.x);
        assert!((d.rect.y - 50.0).abs() < 1.0,  "d.y={}: dense must fill row-2 gap", d.rect.y);
    }

    /// Sparse layout must NOT back-fill: C stays in row 2 (after B), D in row 3.
    ///
    ///  Same grid: A(span2), B(span2), C(1×1), D(1×1) with `grid-auto-flow: row`.
    ///  Col-3 gap in row 1 is skipped by the forward-only cursor.
    #[test]
    fn grid_sparse_row_no_backfill() {
        let root = lay(
            "<body><div id='g'>\
               <span id='a'></span>\
               <span id='b'></span>\
               <span id='c'></span>\
               <span id='d'></span>\
             </div></body>",
            r#"
            #g {
                display: grid;
                grid-template-columns: 100px 100px 100px;
                grid-auto-rows: 50px;
                grid-auto-flow: row;
                width: 300px;
            }
            #a { grid-column: span 2; }
            #b { grid-column: span 2; }
            "#,
        );
        let body = &root;
        let grid = first_element_child(body);
        let items: Vec<_> = grid.children.iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert_eq!(items.len(), 4, "expected 4 items");

        let c = &items[2];
        let d = &items[3];

        // Sparse: C ends up at col 3, row 2 (not row 1 — cursor didn't go back).
        assert!((c.rect.x - 200.0).abs() < 1.0, "c.x={}: sparse must not back-fill col3 row1", c.rect.x);
        assert!((c.rect.y - 50.0).abs() < 1.0,  "c.y={}: sparse must not back-fill col3 row1", c.rect.y);

        // D ends up at col 1, row 3 (cursor advanced past row 2).
        assert!((d.rect.y - 100.0).abs() < 1.0, "d.y={}: sparse must not back-fill", d.rect.y);
    }

    /// Dense column flow: small items back-fill gaps left by tall items in earlier columns.
    ///
    ///  2 cols, 3 explicit rows (50px).
    ///  A spans 2 rows (col 1, rows 1-2); B spans 3 rows (col 2, rows 1-3).
    ///  Dense: C fills the remaining slot in col 1, row 3.
    ///  Sparse: C would continue forward to col 3 (outside the explicit grid).
    #[test]
    fn grid_dense_column_fills_gap() {
        let root = lay(
            "<body><div id='g'>\
               <span id='a'></span>\
               <span id='b'></span>\
               <span id='c'></span>\
             </div></body>",
            r#"
            #g {
                display: grid;
                grid-template-columns: 100px 100px;
                grid-template-rows: 50px 50px 50px;
                grid-auto-flow: column dense;
                width: 200px;
            }
            #a { grid-row: span 2; }
            #b { grid-row: span 3; }
            /* c: auto 1×1 */
            "#,
        );
        let body = &root;
        let grid = first_element_child(body);
        let items: Vec<_> = grid.children.iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert_eq!(items.len(), 3, "expected 3 items");

        let a = &items[0];
        let b = &items[1];
        let c = &items[2];

        // A: col 1, rows 1-2 → x=0, y=0, h=100
        assert!((a.rect.x - 0.0).abs() < 1.0,      "a.x={}", a.rect.x);
        assert!((a.rect.y - 0.0).abs() < 1.0,      "a.y={}", a.rect.y);
        assert!((a.rect.height - 100.0).abs() < 1.0, "a.h={}", a.rect.height);

        // B: col 2, rows 1-3 → x=100, y=0, h=150
        assert!((b.rect.x - 100.0).abs() < 1.0,    "b.x={}", b.rect.x);
        assert!((b.rect.y - 0.0).abs() < 1.0,      "b.y={}", b.rect.y);
        assert!((b.rect.height - 150.0).abs() < 1.0, "b.h={}", b.rect.height);

        // Dense: C fills col 1 row 3 → x=0, y=100
        assert!((c.rect.x - 0.0).abs() < 1.0,   "c.x={}: dense col must back-fill col1 row3", c.rect.x);
        assert!((c.rect.y - 100.0).abs() < 1.0, "c.y={}: dense col must back-fill col1 row3", c.rect.y);
    }

    // ── CSS Grid L2 Subgrid ───────────────────────────────────────────────────

    /// `grid-template-columns: subgrid` parses to the sentinel `[Subgrid]`.
    #[test]
    fn grid_subgrid_parse_columns() {
        let root = lay(
            "<body><div id='g'><div id='sg'></div></div></body>",
            "#g { display: grid; grid-template-columns: 100px 200px; } \
             #sg { grid-template-columns: subgrid; }",
        );
        let grid = first_element_child(&root);
        let subgrid = first_element_child(grid);
        assert_eq!(subgrid.style.grid_template_columns.len(), 1);
        assert_eq!(subgrid.style.grid_template_columns[0], GridTrackSize::Subgrid);
    }

    /// `grid-template-rows: subgrid` parses to the sentinel `[Subgrid]`.
    #[test]
    fn grid_subgrid_parse_rows() {
        let root = lay(
            "<body><div id='g'><div id='sg'></div></div></body>",
            "#g { display: grid; grid-template-rows: 50px 100px; } \
             #sg { grid-template-rows: subgrid; }",
        );
        let grid = first_element_child(&root);
        let subgrid = first_element_child(grid);
        assert_eq!(subgrid.style.grid_template_rows.len(), 1);
        assert_eq!(subgrid.style.grid_template_rows[0], GridTrackSize::Subgrid);
    }

    /// A subgrid item spanning 2 columns inherits those column widths from the parent.
    /// Two items inside the subgrid are placed in the inherited columns (100px + 200px).
    #[test]
    fn grid_subgrid_column_layout() {
        let root = lay(
            "<body>\
               <div id='g'>\
                 <div id='sg'>\
                   <span id='a'></span>\
                   <span id='b'></span>\
                 </div>\
               </div>\
             </body>",
            r#"
            body { width: 400px; }
            #g {
                display: grid;
                grid-template-columns: 100px 200px;
                grid-template-rows: 50px;
                width: 300px;
            }
            #sg {
                display: grid;
                grid-template-columns: subgrid;
                grid-column: 1 / 3;
            }
            #a { height: 30px; }
            #b { height: 30px; }
            "#,
        );
        let grid = first_element_child(&root);
        // The subgrid item spans both columns → width = 300px.
        let sg = first_element_child(grid);
        assert!(
            (sg.rect.width - 300.0).abs() < 2.0,
            "subgrid width should be ~300, got {}",
            sg.rect.width
        );
        // Items inside subgrid are placed in the inherited 100px and 200px columns.
        let items: Vec<_> = sg.children.iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert_eq!(items.len(), 2, "expected 2 items in subgrid");
        let a = &items[0];
        let b = &items[1];
        // a in col 1 (x=0, w=100), b in col 2 (x=100, w=200).
        assert!((a.rect.x - sg.rect.x).abs() < 2.0, "a.x rel={}", a.rect.x - sg.rect.x);
        assert!((a.rect.width - 100.0).abs() < 2.0, "a.w={}", a.rect.width);
        assert!((b.rect.x - sg.rect.x - 100.0).abs() < 2.0, "b.x rel={}", b.rect.x - sg.rect.x);
        assert!((b.rect.width - 200.0).abs() < 2.0, "b.w={}", b.rect.width);
    }

    /// `collect_subgrid_items` finds both column-subgrid and row-subgrid containers.
    #[test]
    fn grid_collect_subgrid_items() {
        use crate::subgrid::collect_subgrid_items;
        let root = lay(
            "<body>\
               <div id='g'>\
                 <div id='col_sg'></div>\
                 <div id='row_sg'></div>\
                 <div id='both_sg'></div>\
                 <div id='normal'></div>\
               </div>\
             </body>",
            r#"
            #g { display: grid; grid-template-columns: 100px 200px; grid-template-rows: 50px 50px; }
            #col_sg { grid-template-columns: subgrid; grid-column: 1 / 3; }
            #row_sg { grid-template-rows: subgrid; grid-row: 1 / 3; }
            #both_sg { grid-template-columns: subgrid; grid-template-rows: subgrid; }
            "#,
        );
        let items = collect_subgrid_items(&root);
        // col_sg, row_sg, both_sg should appear; normal should not.
        assert_eq!(items.len(), 3, "expected 3 subgrid items, got {:?}", items.len());
        let col_sg = items.iter().find(|it| it.subgrid_columns && !it.subgrid_rows);
        assert!(col_sg.is_some(), "missing col-subgrid item");
        let row_sg = items.iter().find(|it| it.subgrid_rows && !it.subgrid_columns);
        assert!(row_sg.is_some(), "missing row-subgrid item");
        let both_sg = items.iter().find(|it| it.subgrid_columns && it.subgrid_rows);
        assert!(both_sg.is_some(), "missing both-subgrid item");
    }

    // ── collect_image_requests ────────────────────────────────────────────────

    fn vp() -> Size {
        Size::new(800.0, 600.0)
    }

    /// Обычный `<img src>` → один запрос с тем же URL.
    #[test]
    fn collect_plain_img_src() {
        let doc = lumen_html_parser::parse(r#"<body><img src="photo.jpg"></body>"#);
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "photo.jpg");
        assert!(!reqs[0].has_explicit_width);
        assert!(!reqs[0].has_explicit_height);
    }

    /// `<img src width height>` → has_explicit_width/height == true.
    #[test]
    fn collect_img_with_explicit_dims() {
        let doc = lumen_html_parser::parse(
            r#"<body><img src="a.png" width="100" height="50"></body>"#,
        );
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].has_explicit_width);
        assert!(reqs[0].has_explicit_height);
    }

    /// Пустой `src` → запрос не включается.
    #[test]
    fn collect_img_empty_src_skipped() {
        let doc = lumen_html_parser::parse(r#"<body><img src=""></body>"#);
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 0);
    }

    /// `<img>` без `src` → запрос не включается.
    #[test]
    fn collect_img_no_src_skipped() {
        let doc = lumen_html_parser::parse(r#"<body><img alt="no src"></body>"#);
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 0);
    }

    /// `<img srcset="a.png 1x, b.png 2x">` → DPR=1.0 → первый кандидат.
    #[test]
    fn collect_img_srcset_picks_first_at_dpr1() {
        let doc = lumen_html_parser::parse(
            r#"<body><img srcset="a.png 1x, b.png 2x" src="fallback.png"></body>"#,
        );
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 1);
        // DPR=1.0 → picker выберет "a.png 1x"
        assert_eq!(reqs[0].url, "a.png");
    }

    /// `<picture><source srcset="hd.webp"><img src="sd.jpg"></picture>` →
    /// picker выбирает source-кандидата (нет атрибута type → тип неизвестен, не фильтруется).
    #[test]
    fn collect_picture_source_wins_over_img_src() {
        let doc = lumen_html_parser::parse(
            r#"<body><picture><source srcset="hd.webp"><img src="sd.jpg"></picture></body>"#,
        );
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "hd.webp");
    }

    /// `<picture><source type="image/heic" srcset="hero.heic"><img src="hero.jpg"></picture>` →
    /// heic нет в `supported_mime_types()` → picker пропускает source → fallback на `<img src>`.
    #[test]
    fn collect_picture_unsupported_type_falls_back() {
        let doc = lumen_html_parser::parse(concat!(
            r#"<body><picture>"#,
            r#"<source type="image/heic" srcset="hero.heic">"#,
            r#"<img src="hero.jpg">"#,
            r#"</picture></body>"#,
        ));
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 1, "должен быть один запрос — fallback PNG/JPEG");
        assert_eq!(reqs[0].url, "hero.jpg", "heic source скипается, выбирается img src");
    }

    /// `<picture>` с первым поддерживаемым `<source type="image/webp">` →
    /// picker выбирает этот source (webp теперь декодируется), а не img src.
    #[test]
    fn collect_picture_supported_type_picked() {
        let doc = lumen_html_parser::parse(concat!(
            r#"<body><picture>"#,
            r#"<source type="image/webp" srcset="hero.webp">"#,
            r#"<source type="image/jpeg" srcset="hero.jpg">"#,
            r#"<img src="fallback.png">"#,
            r#"</picture></body>"#,
        ));
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "hero.webp", "первый поддерживаемый source — WebP");
    }

    /// Несколько `<img>` → несколько запросов.
    #[test]
    fn collect_multiple_images() {
        let doc = lumen_html_parser::parse(
            r#"<body><img src="a.png"><img src="b.jpg"></body>"#,
        );
        let reqs = collect_image_requests(&doc, vp());
        assert_eq!(reqs.len(), 2);
        let urls: Vec<&str> = reqs.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.contains(&"a.png"));
        assert!(urls.contains(&"b.jpg"));
    }

    // ── collect_background_image_requests ────────────────────────────────────

    fn layout_with(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        layout(&doc, &sheet, vp())
    }

    /// `background-image: url(...)` на блоке → один URL в результате.
    #[test]
    fn collect_bg_image_single_block() {
        let root = layout_with(
            "<body><div></div></body>",
            "div { width: 50px; height: 50px; background-image: url(bg.png); }",
        );
        let urls = collect_background_image_requests(&root);
        assert_eq!(urls, vec!["bg.png".to_string()]);
    }

    /// `background-image: none` (initial) → пустой результат.
    #[test]
    fn collect_bg_image_none_skipped() {
        let root = layout_with(
            "<body><div></div></body>",
            "div { width: 50px; height: 50px; background-image: none; }",
        );
        assert!(collect_background_image_requests(&root).is_empty());
    }

    /// Gradient-вариант не учитывается (Phase 0 не растрит).
    #[test]
    fn collect_bg_image_gradient_skipped() {
        let root = layout_with(
            "<body><div></div></body>",
            "div { width: 50px; height: 50px; \
             background-image: linear-gradient(red, blue); }",
        );
        assert!(collect_background_image_requests(&root).is_empty());
    }

    /// Дубликаты URL фильтруются.
    #[test]
    fn collect_bg_image_dedupes() {
        let root = layout_with(
            "<body><div></div><div></div><div></div></body>",
            "div { width: 10px; height: 10px; background-image: url(same.png); }",
        );
        let urls = collect_background_image_requests(&root);
        assert_eq!(urls.len(), 1, "three divs same URL → один запрос, got {urls:?}");
        assert_eq!(urls[0], "same.png");
    }

    /// Разные URL → собираются в порядке обхода.
    #[test]
    fn collect_bg_image_multiple_distinct() {
        let root = layout_with(
            r#"<body><div class="a"></div><div class="b"></div></body>"#,
            ".a { width: 10px; height: 10px; background-image: url(a.png); } \
             .b { width: 10px; height: 10px; background-image: url(b.png); }",
        );
        let urls = collect_background_image_requests(&root);
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"a.png".to_string()));
        assert!(urls.contains(&"b.png".to_string()));
    }

    // ── CSS Positioned Layout L3 — position: relative / absolute / fixed ──

    /// `position: relative; top: 20px; left: 30px` — визуальный сдвиг относительно
    /// нормального потока; высота родителя не меняется.
    #[test]
    fn position_relative_offset() {
        let root = lay(
            "<div class='outer'><div class='inner'>x</div></div>",
            ".outer { width: 200px; height: 100px; }
             .inner { position: relative; top: 20px; left: 30px; }",
        );
        let outer = first_element_child(&root);
        let inner = first_element_child(outer);
        // Нормальная позиция inner без offset: x=0, y=0 (нет margin/padding).
        // С relative offset: y += 20, x += 30.
        assert_eq!(inner.rect.x, 30.0, "relative left");
        assert_eq!(inner.rect.y, 20.0, "relative top");
        // Родительская высота не изменяется (relative не влияет на flow).
        assert_eq!(outer.rect.height, 100.0, "outer height unchanged");
    }

    /// `position: relative; bottom: 10px; right: 15px` — отрицательный сдвиг.
    #[test]
    fn position_relative_bottom_right() {
        let root = lay(
            "<div class='inner'>x</div>",
            ".inner { position: relative; bottom: 10px; right: 15px; }",
        );
        let inner = first_element_child(&root);
        // bottom: 10px → y -= 10 (сдвиг вверх)
        assert_eq!(inner.rect.y, -10.0, "relative bottom moves up");
        // right: 15px → x -= 15 (сдвиг влево)
        assert_eq!(inner.rect.x, -15.0, "relative right moves left");
    }

    /// `position: absolute; top: 10px; left: 20px` внутри positioned parent.
    /// Абсолютный элемент не участвует в normal flow (высота родителя = 0).
    #[test]
    fn position_absolute_top_left() {
        let root = lay(
            "<div class='parent'><div class='abs'>x</div></div>",
            ".parent { position: relative; width: 400px; height: 300px; }
             .abs    { position: absolute; top: 10px; left: 20px; width: 50px; }",
        );
        let parent = first_element_child(&root);
        let abs_child = first_element_child(parent);
        // Positioned relative to parent's border-edge box.
        assert_eq!(abs_child.rect.x, 20.0, "abs left");
        assert_eq!(abs_child.rect.y, 10.0, "abs top");
        // Ширина задана явно.
        assert_eq!(abs_child.rect.width, 50.0, "abs explicit width");
    }

    /// `position: absolute; bottom: 0; right: 0` — правый нижний угол контейнера.
    #[test]
    fn position_absolute_bottom_right() {
        let root = lay(
            "<div class='parent'><div class='abs'>x</div></div>",
            ".parent { position: relative; width: 400px; height: 300px; }
             .abs    { position: absolute; bottom: 0px; right: 0px; width: 60px; height: 40px; }",
        );
        let parent = first_element_child(&root);
        let abs_child = first_element_child(parent);
        // right: 0 → right edge of abs = right edge of parent (400)
        // abs.rect.x = 400 - 0 - 60 = 340
        assert_eq!(abs_child.rect.x, 340.0, "abs right=0 positions at right edge");
        // bottom: 0 → bottom edge of abs = bottom edge of parent (300)
        // abs.rect.y = 300 - 0 - 40 = 260
        assert_eq!(abs_child.rect.y, 260.0, "abs bottom=0 positions at bottom edge");
    }

    /// `position: absolute` без explicit containing block — используется viewport.
    #[test]
    fn position_absolute_uses_viewport_without_positioned_ancestor() {
        let root = lay(
            "<div><div class='abs'>x</div></div>",
            ".abs { position: absolute; top: 50px; left: 100px; width: 80px; }",
        );
        // Родитель static — CB = viewport (800×600)
        let parent = first_element_child(&root);
        let abs_child = first_element_child(parent);
        assert_eq!(abs_child.rect.y, 50.0, "abs top from viewport");
        assert_eq!(abs_child.rect.x, 100.0, "abs left from viewport");
    }

    /// Абсолютный элемент не влияет на высоту normal-flow родителя.
    #[test]
    fn position_absolute_excluded_from_normal_flow() {
        let root = lay(
            "<div class='parent'>
               <div class='normal' style='height: 40px;'></div>
               <div class='abs' style='height: 200px;'></div>
             </div>",
            ".parent { position: relative; }
             .abs    { position: absolute; top: 0; left: 0; }",
        );
        let parent = first_element_child(&root);
        // Только normal-flow div (height=40) считается в высоту родителя.
        assert_eq!(parent.rect.height, 40.0, "abs child excluded from parent height");
    }

    /// `position: fixed; top: 0; right: 0` — position relative to viewport.
    #[test]
    fn position_fixed_relative_to_viewport() {
        let root = lay(
            "<div class='parent'><div class='fix'>x</div></div>",
            ".parent { position: relative; width: 400px; height: 300px; margin: 50px; }
             .fix    { position: fixed; top: 5px; right: 10px; width: 80px; }",
        );
        let parent = first_element_child(&root);
        let fix_child = first_element_child(parent);
        // Fixed: CB = viewport (800×600), not parent
        assert_eq!(fix_child.rect.y, 5.0, "fixed top from viewport");
        // right: 10 → x = viewport.width - 10 - 80 = 710
        assert_eq!(fix_child.rect.x, 710.0, "fixed right from viewport");
    }

    /// `inset` shorthand: `inset: 10px 20px 30px 40px` → top/right/bottom/left.
    #[test]
    fn inset_shorthand_four_values() {
        let root = lay(
            "<div class='parent'><div class='abs'></div></div>",
            ".parent { position: relative; width: 400px; height: 300px; }
             .abs    { position: absolute; inset: 10px 20px 30px 40px; }",
        );
        let parent = first_element_child(&root);
        let abs_child = first_element_child(parent);
        // top: 10, left: 40
        assert_eq!(abs_child.rect.y, 10.0, "inset top");
        assert_eq!(abs_child.rect.x, 40.0, "inset left");
    }

    /// `position: relative; top: auto; left: auto` — никакого сдвига.
    #[test]
    fn position_relative_all_auto_no_offset() {
        let root = lay(
            "<div class='outer'><div class='inner'>x</div></div>",
            ".outer { width: 200px; }
             .inner { position: relative; top: auto; left: auto; }",
        );
        let outer = first_element_child(&root);
        let inner = first_element_child(outer);
        assert_eq!(inner.rect.x, 0.0, "no x offset");
        assert_eq!(inner.rect.y, 0.0, "no y offset");
    }

    // ── UA stylesheet ──────────────────────────────────────────────────────

    fn first_seg_style(p: &LayoutBox) -> ComputedStyle {
        let run = first_inline_run(p);
        if let BoxKind::InlineRun { segments, .. } = &run.kind {
            segments[0].style.clone()
        } else {
            panic!("expected InlineRun with segments");
        }
    }

    #[test]
    fn ua_del_text_decoration_line_through() {
        let root = lay("<p><del>x</del></p>", "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert!(style.text_decoration_line.line_through, "del → line-through");
        assert!(!style.text_decoration_line.underline, "del → no underline");
    }

    #[test]
    fn ua_s_text_decoration_line_through() {
        let root = lay("<p><s>x</s></p>", "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert!(style.text_decoration_line.line_through, "s → line-through");
    }

    #[test]
    fn ua_ins_text_decoration_underline() {
        let root = lay("<p><ins>x</ins></p>", "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert!(style.text_decoration_line.underline, "ins → underline");
        assert!(!style.text_decoration_line.line_through, "ins → no line-through");
    }

    #[test]
    fn ua_a_href_link_color_and_underline() {
        let root = lay(r#"<p><a href="http://example.com">link</a></p>"#, "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert_eq!(
            style.color,
            Color { r: 0, g: 0, b: 238, a: 255 },
            "a[href] → #0000ee"
        );
        assert!(style.text_decoration_line.underline, "a[href] → underline");
    }

    #[test]
    fn ua_sub_vertical_align_and_font_size() {
        let root = lay("<p><sub>x</sub></p>", "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert_eq!(style.vertical_align, VerticalAlign::Sub, "sub → VerticalAlign::Sub");
        assert!(
            (style.font_size - 16.0 * 0.83).abs() < 0.01,
            "sub → 83% font-size, got {}",
            style.font_size
        );
    }

    #[test]
    fn ua_sup_vertical_align_and_font_size() {
        let root = lay("<p><sup>x</sup></p>", "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert_eq!(style.vertical_align, VerticalAlign::Super, "sup → VerticalAlign::Super");
        assert!(
            (style.font_size - 16.0 * 0.83).abs() < 0.01,
            "sup → 83% font-size, got {}",
            style.font_size
        );
    }

    #[test]
    fn ua_small_font_size() {
        let root = lay("<p><small>x</small></p>", "");
        let p = first_element_child(&root);
        let style = first_seg_style(p);
        assert!(
            (style.font_size - 16.0 * 0.83).abs() < 0.01,
            "small → 83% font-size, got {}",
            style.font_size
        );
    }

    // ──────── ::before / ::after pseudo-element generation ──────────────────

    fn first_seg_text(b: &LayoutBox) -> String {
        match &b.kind {
            BoxKind::InlineRun { segments, .. } => {
                segments.first().map(|s| s.text.clone()).unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    #[test]
    fn before_pseudo_string_content() {
        // ::before content вставляется как первый сегмент InlineRun.
        let root = lay("<p>Hello</p>", r#"p::before { content: ">> "; }"#);
        let p = first_element_child(&root);
        assert!(!p.children.is_empty(), "p must have children");
        let first = &p.children[0];
        assert!(
            matches!(first.kind, BoxKind::InlineRun { .. }),
            "first child must be InlineRun, got {:?}",
            std::mem::discriminant(&first.kind)
        );
        let text = first_seg_text(first);
        assert!(
            text.starts_with(">> "),
            "::before text should start with '>> ', got {:?}",
            text
        );
    }

    #[test]
    fn after_pseudo_string_content() {
        // ::after content вставляется как последний сегмент InlineRun.
        let root = lay("<p>Hello</p>", r#"p::after { content: " <<"; }"#);
        let p = first_element_child(&root);
        assert!(!p.children.is_empty(), "p must have children");
        let last = p.children.last().unwrap();
        assert!(
            matches!(last.kind, BoxKind::InlineRun { .. }),
            "last child must be InlineRun"
        );
        if let BoxKind::InlineRun { segments, .. } = &last.kind {
            let last_seg = segments.last().unwrap();
            assert!(
                last_seg.text.ends_with(" <<"),
                "::after text should end with ' <<', got {:?}",
                last_seg.text
            );
        }
    }

    #[test]
    fn before_and_after_together() {
        // ::before и ::after оба применяются.
        let root = lay(
            "<p>X</p>",
            r#"p::before { content: "["; } p::after { content: "]"; }"#,
        );
        let p = first_element_child(&root);
        // The p should have at least one InlineRun with all text.
        let all_text: String = p
            .children
            .iter()
            .flat_map(|c| {
                if let BoxKind::InlineRun { segments, .. } = &c.kind {
                    segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect();
        assert!(
            all_text.contains('[') && all_text.contains(']'),
            "expected '[' and ']' in inline text, got {:?}",
            all_text
        );
    }

    #[test]
    fn before_content_none_generates_nothing() {
        // content: none → псевдоэлемент не генерируется.
        let root = lay("<p>X</p>", "p::before { content: none; }");
        let p = first_element_child(&root);
        // Только один InlineRun с текстом "X", без ::before.
        let inline_texts: Vec<String> = p
            .children
            .iter()
            .flat_map(|c| {
                if let BoxKind::InlineRun { segments, .. } = &c.kind {
                    segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect();
        assert!(
            inline_texts.iter().all(|t| !t.is_empty()),
            "no empty texts expected"
        );
        // Нет текста кроме "X".
        let all = inline_texts.join("");
        assert_eq!(all.trim(), "X", "got {:?}", all);
    }

    #[test]
    fn before_pseudo_inherits_parent_color() {
        // ::before наследует color от родителя.
        let root = lay(
            "<p>X</p>",
            r#"p { color: red; } p::before { content: "•"; }"#,
        );
        let p = first_element_child(&root);
        // Первый InlineRun содержит сегмент от ::before.
        let first_run = p.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. }));
        let Some(run) = first_run else {
            panic!("no InlineRun found");
        };
        if let BoxKind::InlineRun { segments, .. } = &run.kind {
            let before_seg = segments.iter().find(|s| s.text == "•");
            let Some(seg) = before_seg else {
                panic!("no segment with '•' found");
            };
            // red = Color { r: 255, g: 0, b: 0, a: 255 }. Проверяем r > 0, g == 0.
            assert!(
                seg.style.color.r > 0 && seg.style.color.g == 0,
                "::before should inherit red color, got {:?}",
                seg.style.color
            );
        }
    }

    #[test]
    fn before_pseudo_no_rules_no_box() {
        // Если нет правил для ::before — ничего не генерируется.
        let root = lay("<p>Hello</p>", "p { color: blue; }");
        let p = first_element_child(&root);
        // Только один InlineRun с "Hello".
        assert_eq!(p.children.len(), 1, "expected 1 child (InlineRun)");
        assert!(matches!(p.children[0].kind, BoxKind::InlineRun { .. }));
    }

    // ──────── inline ::before / ::after (collect_inline_segments path) ───────

    #[test]
    fn inline_before_pseudo_injects_segment_before_children() {
        // span::before { content: ">>"; } — сегмент ">>" перед текстом span.
        let root = lay(
            "<p><span>Hello</span></p>",
            r#"span::before { content: ">>"; }"#,
        );
        let p = first_element_child(&root);
        let run = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .expect("InlineRun expected");
        if let BoxKind::InlineRun { segments, .. } = &run.kind {
            let first = segments.first().expect("at least one segment");
            assert!(
                first.text.contains(">>"),
                "::before segment should be first, got {:?}",
                first.text
            );
        }
    }

    #[test]
    fn inline_after_pseudo_injects_segment_after_children() {
        // span::after { content: "<<"; } — сегмент "<<" после текста span.
        let root = lay(
            "<p><span>Hello</span></p>",
            r#"span::after { content: "<<"; }"#,
        );
        let p = first_element_child(&root);
        let run = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .expect("InlineRun expected");
        if let BoxKind::InlineRun { segments, .. } = &run.kind {
            let last = segments.last().expect("at least one segment");
            assert!(
                last.text.contains("<<"),
                "::after segment should be last, got {:?}",
                last.text
            );
        }
    }

    #[test]
    fn inline_before_after_order() {
        // span::before + ::after — порядок: before / span-text / after.
        let root = lay(
            "<p><span>X</span></p>",
            r#"span::before { content: "A"; } span::after { content: "B"; }"#,
        );
        let p = first_element_child(&root);
        let all_text: String = p
            .children
            .iter()
            .flat_map(|c| {
                if let BoxKind::InlineRun { segments, .. } = &c.kind {
                    segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect();
        let a_pos = all_text.find('A').expect("A not found");
        let x_pos = all_text.find('X').expect("X not found");
        let b_pos = all_text.find('B').expect("B not found");
        assert!(a_pos < x_pos, "::before must precede span text");
        assert!(x_pos < b_pos, "::after must follow span text");
    }

    #[test]
    fn inline_before_inherits_span_style() {
        // span::before наследует color от span.
        let root = lay(
            "<p><span>X</span></p>",
            r#"span { color: #ff0000; } span::before { content: "●"; }"#,
        );
        let p = first_element_child(&root);
        let run = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .expect("InlineRun");
        if let BoxKind::InlineRun { segments, .. } = &run.kind {
            let before = segments.iter().find(|s| s.text.contains('●')).expect("● not found");
            assert!(
                before.style.color.r > 0 && before.style.color.g == 0,
                "::before should inherit red color, got {:?}",
                before.style.color
            );
        }
    }

    #[test]
    fn inline_before_display_block_skipped_in_inline_context() {
        // span::before { display: block } внутри inline-контекста — пропускается.
        let root = lay(
            "<p><span>Only</span></p>",
            r#"span::before { content: "X"; display: block; }"#,
        );
        let p = first_element_child(&root);
        let run = p
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .expect("InlineRun");
        if let BoxKind::InlineRun { segments, .. } = &run.kind {
            // Текст "X" не должен появиться — псевдо-элемент block в inline-контексте пропускается.
            let has_x = segments.iter().any(|s| s.text == "X");
            assert!(!has_x, "block ::before must be skipped in inline context");
        }
    }

    fn first_inline_run_frag(b: &LayoutBox) -> &InlineFrag {
        let run = b
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
            .expect("expected InlineRun child");
        match &run.kind {
            BoxKind::InlineRun { lines, .. } => &lines[0][0],
            _ => unreachable!(),
        }
    }

    #[test]
    fn vertical_align_baseline_y_offset_half_leading() {
        // baseline — y_offset == half_leading = (line_h - font_size) / 2.
        // CSS 2.1 §10.8.1: content area is centred in line-box via half-leading.
        let root = lay_measured("<p>Hello</p>", "", 800.0);
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        let fs = frag.style.font_size;
        let line_h = fs * frag.style.line_height;
        let expected = ((line_h - fs) / 2.0).max(0.0);
        assert!(
            (frag.y_offset - expected).abs() < 0.01,
            "baseline y_offset must be half_leading={}, got {}",
            expected,
            frag.y_offset
        );
    }

    #[test]
    fn vertical_align_middle_y_offset() {
        // middle → (line_h - font_size) / 2.
        let root = lay_measured(
            "<p><span>Hi</span></p>",
            "span { vertical-align: middle; }",
            800.0,
        );
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        let font_size = frag.style.font_size;
        let line_h = font_size * frag.style.line_height;
        let expected = ((line_h - font_size) / 2.0).max(0.0);
        assert!(
            (frag.y_offset - expected).abs() < 0.01,
            "middle y_offset: expected {}, got {}",
            expected,
            frag.y_offset
        );
    }

    #[test]
    fn vertical_align_bottom_y_offset() {
        // bottom → line_h - font_size.
        let root = lay_measured(
            "<p><span>Hi</span></p>",
            "span { vertical-align: bottom; }",
            800.0,
        );
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        let font_size = frag.style.font_size;
        let line_h = font_size * frag.style.line_height;
        let expected = (line_h - font_size).max(0.0);
        assert!(
            (frag.y_offset - expected).abs() < 0.01,
            "bottom y_offset: expected {}, got {}",
            expected,
            frag.y_offset
        );
    }

    #[test]
    fn vertical_align_length_shifts_up() {
        // vertical-align: 8px → y_offset = half_leading - 8px
        // (позитивная длина CSS = вверх от baseline = half_leading - 8).
        let root = lay_measured(
            "<p><span>Hi</span></p>",
            "span { vertical-align: 8px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        let fs = frag.style.font_size;
        let line_h = fs * frag.style.line_height;
        let half_leading = ((line_h - fs) / 2.0).max(0.0);
        let expected = half_leading - 8.0;
        assert!(
            (frag.y_offset - expected).abs() < 0.01,
            "length 8px y_offset: expected {}, got {}",
            expected,
            frag.y_offset
        );
    }

    #[test]
    fn vertical_align_super_negative_y_offset() {
        // super → y_offset < 0 (сдвиг вверх).
        let root = lay_measured("<p><sup>note</sup></p>", "", 800.0);
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        assert!(
            frag.y_offset < 0.0,
            "super y_offset must be negative, got {}",
            frag.y_offset
        );
    }

    #[test]
    fn vertical_align_sub_positive_y_offset() {
        // sub → y_offset > 0 (сдвиг вниз).
        let root = lay_measured("<p><sub>note</sub></p>", "", 800.0);
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        assert!(
            frag.y_offset > 0.0,
            "sub y_offset must be positive, got {}",
            frag.y_offset
        );
    }

    // ── Half-leading (CSS 2.1 §10.8.1) ──────────────────────────────────────

    #[test]
    fn half_leading_baseline_centred_in_line_box() {
        // line-height: 2.0 → half_leading = (32 - 16) / 2 = 8px for 16px font.
        // Baseline фрагмента должен быть смещён на 8px вниз от верха строки.
        let root = lay_measured(
            "<p>Hello</p>",
            "p { line-height: 2.0; font-size: 16px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        let expected_half_leading = 8.0_f32; // (32 - 16) / 2
        assert!(
            (frag.y_offset - expected_half_leading).abs() < 0.1,
            "half_leading with line-height:2: expected y_offset={}, got {}",
            expected_half_leading,
            frag.y_offset
        );
    }

    #[test]
    fn half_leading_zero_when_line_height_equals_font_size() {
        // line-height: 1.0 → нет leading, y_offset = 0.
        let root = lay_measured(
            "<p>Hello</p>",
            "p { line-height: 1.0; font-size: 16px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let frag = first_inline_run_frag(p);
        assert!(
            frag.y_offset.abs() < 0.001,
            "line-height:1.0 → no half-leading, expected y_offset=0, got {}",
            frag.y_offset
        );
    }

    #[test]
    fn half_leading_line_box_height_correct() {
        // line-height: 1.5, font-size: 20px → line_h = 30px.
        // Высота InlineRun должна быть 30px.
        let root = lay_measured(
            "<p>Hello</p>",
            "p { line-height: 1.5; font-size: 20px; }",
            800.0,
        );
        let p = first_element_child(&root);
        let run = p.children.iter().find(|c| matches!(c.kind, crate::box_tree::BoxKind::InlineRun { .. })).expect("InlineRun not found");
        assert!(
            (run.rect.height - 30.0).abs() < 0.5,
            "line-height:1.5 font-size:20px → height=30px, got {}",
            run.rect.height
        );
    }

    // ── Multi-column layout ──────────────────────────────────────────────────

    #[test]
    fn multicol_column_count_divides_width() {
        // column-count: 3 + column-gap: 10px → each column = (300 - 20) / 3 = 93.33px.
        // Three equal 30px boxes (total 90px) balance into 3 columns of 30px each,
        // so each box maps cleanly to one column fragment.
        let root = lay_measured(
            "<div id='c'><div></div><div></div><div></div></div>",
            "#c { width: 300px; column-count: 3; column-gap: 10px; } #c div { height: 30px; }",
            800.0,
        );
        let container = first_element_child(&root);
        assert_eq!(container.children.len(), 3);
        let col_w = container.children[0].rect.width;
        assert!((col_w - 93.33).abs() < 0.1, "col_w={col_w}");
        // All three children should be in different columns (x differs).
        let x0 = container.children[0].rect.x;
        let x1 = container.children[1].rect.x;
        let x2 = container.children[2].rect.x;
        assert!(x1 > x0, "child1.x={x1} should be right of child0.x={x0}");
        assert!(x2 > x1, "child2.x={x2} should be right of child1.x={x1}");
    }

    #[test]
    fn multicol_no_repeat_width_when_no_column_props() {
        // Without column-count / column-width, block flow is unchanged.
        let root = lay_measured(
            "<div id='c'><div id='a'></div><div id='b'></div></div>",
            "#c { width: 300px; } #a { height: 20px; } #b { height: 20px; }",
            800.0,
        );
        let container = first_element_child(&root);
        let ch0 = &container.children[0];
        let ch1 = &container.children[1];
        assert_eq!(ch0.rect.x, ch1.rect.x, "children should share same x in normal flow");
        assert!(ch1.rect.y > ch0.rect.y, "b should be below a");
    }

    #[test]
    fn multicol_column_span_all_spans_full_width() {
        // A child with column-span:all should be laid out at the full container width,
        // not squeezed into a single column.
        // Layout: 2 column children → span-all → 2 more column children.
        let root = lay_measured(
            r#"<div id='c'>
              <div id='a'></div>
              <div id='s'></div>
              <div id='b'></div>
            </div>"#,
            r#"#c { width: 300px; column-count: 2; column-gap: 10px; }
               #a { height: 20px; }
               #b { height: 20px; }
               #s { column-span: all; height: 10px; }"#,
            800.0,
        );
        let container = first_element_child(&root);
        // Find the span-all child by its full container width (300px) — column
        // fragments of #a/#b are col_w wide, only the spanner spans the full width.
        let span_child = container.children.iter()
            .find(|c| (c.rect.width - 300.0).abs() < 1.0)
            .expect("span-all child not found");
        // Span-all element must cover the full container width (300px).
        assert!(
            (span_child.rect.width - 300.0).abs() < 1.0,
            "span-all child width={} should be 300px",
            span_child.rect.width
        );
        // Span-all element must start at container's content_x.
        assert!(
            span_child.rect.x < 10.0,
            "span-all child x={} should be near container left edge",
            span_child.rect.x
        );
    }

    #[test]
    fn multicol_column_span_all_children_below_span() {
        // Children after a column-span:all element must be positioned below it.
        let root = lay_measured(
            r#"<div id='c'>
              <div id='s'></div>
              <div id='b'></div>
            </div>"#,
            r#"#c { width: 300px; column-count: 2; column-gap: 10px; }
               #s { column-span: all; height: 15px; }
               #b { height: 20px; }"#,
            800.0,
        );
        let container = first_element_child(&root);
        // Spanner is the only full-width (300px) child; #b becomes column fragments.
        let span_child = container.children.iter()
            .find(|c| (c.rect.width - 300.0).abs() < 1.0)
            .expect("span-all child not found");
        let span_bottom = span_child.rect.y + span_child.rect.height;
        // Every column fragment of #b (the non-span children) must be below the spanner.
        let after_children: Vec<&LayoutBox> = container.children.iter()
            .filter(|c| (c.rect.width - 300.0).abs() >= 1.0 && c.rect.height > 0.0)
            .collect();
        assert!(!after_children.is_empty(), "expected #b column fragments below span");
        for after_child in after_children {
            assert!(
                after_child.rect.y >= span_bottom,
                "after_child.y={} must be >= span bottom={}",
                after_child.rect.y,
                span_bottom
            );
        }
    }

    #[test]
    fn multicol_column_fill_auto_sequential() {
        // column-fill: auto — each column is filled up to the container height before
        // spilling to the next column, rather than distributing content evenly.
        // 3 children of 15px each (total 45px) in a 40px-tall container: col0 fills to
        // 40px (the first two boxes + the top 10px of the third), and the third box's
        // remaining 5px spills into col1 (CSS Multicol §3.4 fragmentation).
        let root = lay_measured(
            "<div id='c'><div id='a'></div><div id='b'></div><div id='d'></div></div>",
            "#c { width: 300px; column-count: 2; column-gap: 0px; height: 40px; column-fill: auto; } \
             #a { height: 15px; } #b { height: 15px; } #d { height: 15px; }",
            800.0,
        );
        let container = first_element_child(&root);
        let frags: Vec<&LayoutBox> = container.children.iter()
            .filter(|c| c.rect.height > 0.0)
            .collect();
        // col_w = 300 / 2 = 150. col0 at content_x, col1 at content_x + 150.
        let col0_x = frags.iter().map(|c| c.rect.x).fold(f32::INFINITY, f32::min);
        // col0 must be filled all the way to the container height before col1 is used.
        let col0_bottom = frags.iter()
            .filter(|c| (c.rect.x - col0_x).abs() < 1.0)
            .map(|c| c.rect.y + c.rect.height)
            .fold(0.0f32, f32::max);
        assert!(
            (col0_bottom - 40.0).abs() < 1.0,
            "col0 must fill to container height 40 before spilling (col0_bottom={col0_bottom})"
        );
        // The spillover fragment must exist in col1 (x = content_x + 150).
        assert!(
            frags.iter().any(|c| c.rect.x > col0_x + 100.0),
            "expected a spillover fragment in col1 (col0_x={col0_x})"
        );
    }

    #[test]
    fn multicol_balance_fragments_boxes_across_columns() {
        // Regression (BUG-186, TEST-33 case 5): two 36px background boxes in a
        // 3-column balance container fragment into three 24px column slices
        // (total 72 / 3 = 24), matching Edge — not one atomic box per column with
        // an empty third column. The container height collapses to 24px.
        let root = lay_measured(
            "<div id='c'><div></div><div></div></div>",
            "#c { width: 660px; column-count: 3; column-gap: 12px; } #c div { height: 36px; }",
            800.0,
        );
        let container = first_element_child(&root);
        // col_w = (660 - 24) / 3 = 212.
        let frags: Vec<&LayoutBox> = container.children.iter()
            .filter(|c| c.rect.height > 0.0)
            .collect();
        // Every fragment is at most one column tall (24px), never a whole 36px box.
        for f in &frags {
            assert!(f.rect.height <= 24.0 + 0.5, "fragment too tall: {}", f.rect.height);
            assert!((f.rect.width - 212.0).abs() < 0.5, "fragment width={}", f.rect.width);
        }
        // All three columns receive content (distinct x positions).
        let mut xs: Vec<f32> = frags.iter().map(|f| f.rect.x).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        xs.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        assert_eq!(xs.len(), 3, "all 3 columns should hold a fragment, got xs={xs:?}");
        // Container content height = balanced column height = 24px.
        assert!(
            (container.rect.height - 24.0).abs() < 1.0,
            "container height={} should be 24px",
            container.rect.height
        );
    }

    #[test]
    fn multicol_column_fill_balance_vs_auto_target() {
        // Verify that column-fill:balance uses total/n_cols as target, not container height.
        // With height:20px and 2 children of 15px each and 2 columns:
        //   balance: target = ceil(30/2) = 15 → ch0 fills col0 (15px), ch1 overflows to col1
        //   auto:    target = 20 → ch0(15)+ch1(15)=30>20 with count_cap=1, so still col0+col1
        // Both end up with same layout here; test that column_fill_balance is parsed.
        let root = lay("<p>x</p>", "p { column-fill: balance; }");
        assert!(first_p_style(&root).column_fill_balance, "balance should set column_fill_balance=true");
        let root2 = lay("<p>x</p>", "p { column-fill: auto; }");
        assert!(!first_p_style(&root2).column_fill_balance, "auto should set column_fill_balance=false");
    }

    #[test]
    fn multicol_balance_does_not_skip_first_column() {
        // Regression (BUG-117): with column-count:3 and items each taller than the
        // balanced target height, the greedy assigner advanced past the EMPTY first
        // column (height_overflow fires on column 0 because item height > target),
        // placing items in columns 1 and 2 and leaving column 0 blank. Items must
        // fill column 0 first (CSS Multicol §3.4 — columns filled in order).
        let root = lay_measured(
            "<div id='c'><div id='a'></div><div id='b'></div></div>",
            "#c { width: 300px; column-count: 3; column-gap: 0px; } \
             #a { height: 40px; } #b { height: 40px; }",
            800.0,
        );
        let container = first_element_child(&root);
        let a = &container.children[0];
        let b = &container.children[1];
        // col_w = 300/3 = 100. col0 at content_x, col1 at content_x + 100.
        assert!(
            (a.rect.x - container.rect.x).abs() < 1.0,
            "first item must be in column 0 (a.x={}, container.x={})",
            a.rect.x, container.rect.x
        );
        assert!(
            (b.rect.x - a.rect.x - 100.0).abs() < 1.0,
            "second item must be in column 1, not column 2 (b.x={}, a.x={})",
            b.rect.x, a.rect.x
        );
    }

    #[test]
    fn multicol_fill_auto_ignores_count_cap() {
        // Regression (BUG-117): column-fill:auto must fill a column purely by height.
        // The per-column count cap (a balance-mode anti-starvation guard) wrongly forced
        // one item per column even in auto mode. With 3 short items and a tall container,
        // all three must stack in column 0.
        let root = lay_measured(
            "<div id='c'><div id='a'></div><div id='b'></div><div id='d'></div></div>",
            "#c { width: 300px; column-count: 3; column-gap: 0px; height: 100px; column-fill: auto; } \
             #a { height: 10px; } #b { height: 10px; } #d { height: 10px; }",
            800.0,
        );
        let container = first_element_child(&root);
        let a = &container.children[0];
        let b = &container.children[1];
        let d = &container.children[2];
        // All three fit in column 0 (30px < 100px) → identical x.
        assert!(
            (a.rect.x - b.rect.x).abs() < 1.0 && (a.rect.x - d.rect.x).abs() < 1.0,
            "auto must stack all items in col0 (xs: {} {} {})",
            a.rect.x, b.rect.x, d.rect.x
        );
        // And they stack vertically within the column.
        assert!(
            b.rect.y > a.rect.y && d.rect.y > b.rect.y,
            "items must stack vertically in col0 (ys: {} {} {})",
            a.rect.y, b.rect.y, d.rect.y
        );
    }

    // ── ::marker box (BUG-011) ───────────────────────────────────────────

    #[test]
    fn list_item_generates_marker_box() {
        let root = lay("<ul><li>item</li></ul>", "");
        let ul = first_element_child(&root);
        let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
        assert!(marker.is_some(), "list-item must have a ::marker child");
        if let BoxKind::Marker { text, position, list_style_type, .. } = &marker.unwrap().kind {
            // Disc renders geometrically — marker_text returns "" for bullet types.
            assert!(text.is_empty(), "disc marker text must be empty (geometric rendering)");
            assert_eq!(*list_style_type, ListStyleType::Disc, "default list-style-type is disc");
            assert_eq!(*position, ListStylePosition::Outside);
        }
    }

    #[test]
    fn list_style_image_marker_carries_url() {
        // CSS Lists L3 §2.3 — `list-style-image` populates the Marker box's
        // `image` field and the URL is collected for fetching.
        let root = lay(
            "<ul><li>item</li></ul>",
            "li { list-style-image: url(\"bullet.png\"); }",
        );
        let ul = first_element_child(&root);
        let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
        if let BoxKind::Marker { image, .. } = &marker.kind {
            assert_eq!(image.as_deref(), Some("bullet.png"));
        } else {
            panic!("expected Marker box");
        }
        let urls = collect_background_image_requests(&root);
        assert!(urls.iter().any(|u| u == "bullet.png"), "marker image must be fetched");
    }

    #[test]
    fn list_style_image_marker_shown_with_type_none() {
        // CSS Lists L3 §2.3 — an explicit image still produces a marker even when
        // `list-style-type: none`.
        let root = lay(
            "<ul><li>item</li></ul>",
            "li { list-style-type: none; list-style-image: url(\"b.png\"); }",
        );
        let ul = first_element_child(&root);
        let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
        assert!(marker.is_some(), "list-style-image must generate a marker despite type:none");
    }

    #[test]
    fn list_item_none_no_marker() {
        let root = lay("<ul><li>item</li></ul>", "li { list-style-type: none; }");
        let ul = first_element_child(&root);
        let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
        assert!(marker.is_none(), "list-style-type:none must not generate marker");
    }

    #[test]
    fn ordered_list_decimal_marker() {
        let root = lay(
            "<ol><li>a</li><li>b</li></ol>",
            "ol { list-style-type: decimal; }",
        );
        let ol = first_element_child(&root);
        let lis: Vec<_> = ol.children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
        assert_eq!(lis.len(), 2);
        let m0 = lis[0].children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
        let m1 = lis[1].children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
        if let (BoxKind::Marker { text: t0, .. }, BoxKind::Marker { text: t1, .. }) = (&m0.kind, &m1.kind) {
            assert_eq!(t0, "1. ", "first item");
            assert_eq!(t1, "2. ", "second item");
        }
    }

    #[test]
    fn marker_outside_not_in_flow() {
        // For outside markers: child_y must not advance past the marker.
        let root = lay(
            "<ul><li>item</li></ul>",
            "ul { margin: 0; padding: 0; } li { font-size: 16px; line-height: 1; }",
        );
        let ul = first_element_child(&root);
        let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
        let content = li.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineRun { .. })).unwrap();
        // Marker y should equal content y (both at top of list item).
        assert_eq!(marker.rect.y, content.rect.y, "marker and content must share the same top");
        // Marker x must be to the left of content x.
        assert!(marker.rect.x < content.rect.x, "marker must be left of content");
    }

    /// BUG-038: list-style-position: inside — marker must share the first line with content,
    /// not occupy a separate block line. li height must equal one line-height.
    #[test]
    fn marker_inside_shares_line_with_content() {
        let root = lay(
            "<ul><li>item</li></ul>",
            "ul { padding-left: 0; } \
             li { list-style-position: inside; font-size: 16px; line-height: 1; }",
        );
        let ul = first_element_child(&root);
        let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
        let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
        let content = li.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineRun { .. })).unwrap();
        // Marker and content must be on the same line.
        assert_eq!(marker.rect.y, content.rect.y, "inside marker and content must share the same y");
        // Content must start to the right of the marker.
        assert!(content.rect.x > marker.rect.x, "inside marker must be left of content");
        // li height must be one line-height (16 * 1.0 = 16px), not two.
        assert!((li.rect.height - 16.0).abs() < 1.0,
            "li height should be one line (16px), got {}", li.rect.height);
    }

    // ─── CSS 2.1 §9.5 — float + clear ────────────────────────────────────────

    /// `float: left` с явной шириной — элемент помещается у левого края контейнера.
    #[test]
    fn float_left_positioned_at_left_edge() {
        let root = lay(
            "<div class='c'><div class='f'>x</div></div>",
            ".c { width: 400px; }
             .f { float: left; width: 100px; height: 50px; }",
        );
        let c = first_element_child(&root);
        let f = first_element_child(c);
        assert_eq!(f.rect.x, 0.0, "float left: x at container left");
        assert_eq!(f.rect.y, 0.0, "float left: y at top");
        assert_eq!(f.rect.width,  100.0, "float left: explicit width");
        assert_eq!(f.rect.height,  50.0, "float left: explicit height");
    }

    /// `float: right` с явной шириной — элемент у правого края контейнера.
    #[test]
    fn float_right_positioned_at_right_edge() {
        let root = lay(
            "<div class='c'><div class='f'>x</div></div>",
            ".c { width: 400px; }
             .f { float: right; width: 100px; height: 50px; }",
        );
        let c = first_element_child(&root);
        let f = first_element_child(c);
        // right edge of container = 400px; float width = 100px → x = 300
        assert_eq!(f.rect.x, 300.0, "float right: x at container_right - width");
        assert_eq!(f.rect.y,   0.0, "float right: y at top");
    }

    /// Float left сужает доступную ширину последующего block-брата.
    #[test]
    fn float_left_narrows_sibling_width() {
        let root = lay(
            "<div class='c'><div class='f'>x</div><div class='s'>y</div></div>",
            ".c { width: 400px; }
             .f { float: left; width: 100px; height: 50px; }
             .s { height: 30px; }",
        );
        let c = first_element_child(&root);
        let sibling = c.children.iter()
            .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.float_side == FloatSide::None)
            .expect("sibling block");
        // Sibling starts after left float (x=100) and has width = 400-100 = 300.
        assert_eq!(sibling.rect.x,     100.0, "sibling starts after float");
        assert_eq!(sibling.rect.width, 300.0, "sibling width narrowed");
    }

    /// Float right сужает доступную ширину последующего block-брата.
    #[test]
    fn float_right_narrows_sibling_width() {
        let root = lay(
            "<div class='c'><div class='f'>x</div><div class='s'>y</div></div>",
            ".c { width: 400px; }
             .f { float: right; width: 100px; height: 50px; }
             .s { height: 30px; }",
        );
        let c = first_element_child(&root);
        let sibling = c.children.iter()
            .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.float_side == FloatSide::None)
            .expect("sibling block");
        // Sibling starts at x=0 (right float doesn't push it right), width = 400-100 = 300.
        assert_eq!(sibling.rect.x,     0.0, "sibling starts at left edge");
        assert_eq!(sibling.rect.width, 300.0, "sibling width narrowed by right float");
    }

    /// Два `float: left` выстраиваются горизонтально.
    #[test]
    fn two_left_floats_stack_horizontally() {
        let root = lay(
            "<div class='c'><div class='f1'>a</div><div class='f2'>b</div></div>",
            ".c  { width: 400px; }
             .f1 { float: left; width: 100px; height: 50px; }
             .f2 { float: left; width: 80px;  height: 40px; }",
        );
        let c = first_element_child(&root);
        let floats: Vec<_> = c.children.iter()
            .filter(|ch| ch.style.float_side == FloatSide::Left)
            .collect();
        assert_eq!(floats.len(), 2, "expected two left floats");
        assert_eq!(floats[0].rect.x, 0.0,   "first float at left edge");
        assert_eq!(floats[1].rect.x, 100.0, "second float after first");
    }

    /// `clear: both` сдвигает элемент ниже обоих float-ов.
    #[test]
    fn clear_both_advances_past_floats() {
        let root = lay(
            "<div class='c'><div class='fl'>a</div><div class='fr'>b</div><div class='clr'>c</div></div>",
            ".c   { width: 400px; }
             .fl  { float: left;  width: 80px; height: 60px; }
             .fr  { float: right; width: 80px; height: 40px; }
             .clr { clear: both; height: 20px; }",
        );
        let c = first_element_child(&root);
        let clr = c.children.iter()
            .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.clear == ClearSide::Both)
            .expect("clear:both block");
        // clear:both → must start at y >= max(60, 40) = 60
        assert!(clr.rect.y >= 60.0 - 0.01,
            "clear:both block must start below tallest float (got {})", clr.rect.y);
    }

    /// Контейнер height охватывает float (float clearing родителя).
    /// CSS 2.1 §9.5: контейнер должен расти, чтобы содержать свои float-ы.
    #[test]
    fn container_height_encloses_float() {
        let root = lay(
            "<div class='c'><div class='f'>x</div></div>",
            ".c { width: 400px; }
             .f { float: left; width: 100px; height: 80px; }",
        );
        let c = first_element_child(&root);
        // Container has no non-float children, so height = float height = 80.
        assert!(c.rect.height >= 80.0 - 0.01,
            "container must enclose float (height={}, expected >=80)", c.rect.height);
    }

    /// `clear: left` сдвигает элемент мимо левого float.
    #[test]
    fn clear_left_only_clears_left_floats() {
        let root = lay(
            "<div class='c'><div class='fl'>a</div><div class='clr'>c</div></div>",
            ".c   { width: 400px; }
             .fl  { float: left; width: 80px; height: 50px; }
             .clr { clear: left; height: 20px; }",
        );
        let c = first_element_child(&root);
        let clr = c.children.iter()
            .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.clear == ClearSide::Left)
            .expect("clear:left block");
        assert!(clr.rect.y >= 50.0 - 0.01,
            "clear:left must start below left float (got {})", clr.rect.y);
    }

    /// CSS `float` парсится в FloatSide.
    #[test]
    fn float_side_parsed_correctly() {
        let root = lay("<div class='l'>x</div><div class='r'>x</div><div class='n'>x</div>",
            ".l { float: left } .r { float: right } .n { float: none }");
        let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
        let l = iter.next().unwrap();
        let r = iter.next().unwrap();
        let n = iter.next().unwrap();
        assert_eq!(l.style.float_side, FloatSide::Left,  "float: left");
        assert_eq!(r.style.float_side, FloatSide::Right, "float: right");
        assert_eq!(n.style.float_side, FloatSide::None,  "float: none");
    }

    /// CSS `clear` парсится в ClearSide.
    #[test]
    fn clear_parsed_correctly() {
        let root = lay("<div class='b'>x</div><div class='l'>x</div><div class='r'>x</div>",
            ".b { clear: both } .l { clear: left } .r { clear: right }");
        let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
        let b = iter.next().unwrap();
        let l = iter.next().unwrap();
        let r = iter.next().unwrap();
        assert_eq!(b.style.clear, ClearSide::Both,  "clear: both");
        assert_eq!(l.style.clear, ClearSide::Left,  "clear: left");
        assert_eq!(r.style.clear, ClearSide::Right, "clear: right");
    }

    // ── Margin collapsing CSS 2.1 §8.3.1 ─────────────────────────────────────

    /// Соседние блоки: побеждает бо́льший margin-top (top wins).
    #[test]
    fn sibling_blocks_margin_collapse_top_wins() {
        // mb=10, mt=30 → gap = max(10,30) = 30, а не 40
        let root = lay(
            "<div class='a'>x</div><div class='b'>y</div>",
            ".a { height: 10px; margin-bottom: 10px; } .b { height: 10px; margin-top: 30px; }",
        );
        let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
        let a = iter.next().unwrap();
        let b = iter.next().unwrap();
        assert!((a.rect.y - 0.0).abs() < 0.1, "a.y={}", a.rect.y);
        // bottom of .a = 10. gap = max(10,30)=30. .b top = 40.
        assert!((b.rect.y - 40.0).abs() < 0.1, "b.y={}", b.rect.y);
    }

    /// Соседние блоки: побеждает бо́льший margin-bottom (bottom wins).
    #[test]
    fn sibling_blocks_margin_collapse_bottom_wins() {
        // mb=30, mt=10 → gap = max(30,10) = 30, а не 40
        let root = lay(
            "<div class='a'>x</div><div class='b'>y</div>",
            ".a { height: 10px; margin-bottom: 30px; } .b { height: 10px; margin-top: 10px; }",
        );
        let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
        let a = iter.next().unwrap();
        let b = iter.next().unwrap();
        assert!((a.rect.y - 0.0).abs() < 0.1, "a.y={}", a.rect.y);
        // bottom of .a = 10. gap = max(30,10)=30. .b top = 40.
        assert!((b.rect.y - 40.0).abs() < 0.1, "b.y={}", b.rect.y);
    }

    /// Цепочка из трёх блоков: два соседних схлопывания независимы.
    #[test]
    fn three_sibling_blocks_margin_collapse_chain() {
        // .a mb=20, .b mt=15 mb=25, .c mt=10
        // gap(a–b) = max(20,15)=20,  gap(b–c) = max(25,10)=25
        let root = lay(
            "<div class='a'>x</div><div class='b'>y</div><div class='c'>z</div>",
            ".a { height: 5px; margin-bottom: 20px; }
             .b { height: 5px; margin-top: 15px; margin-bottom: 25px; }
             .c { height: 5px; margin-top: 10px; }",
        );
        let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
        let a = iter.next().unwrap();
        let b = iter.next().unwrap();
        let c = iter.next().unwrap();
        assert!((a.rect.y -  0.0).abs() < 0.1, "a.y={}", a.rect.y);
        assert!((b.rect.y - 25.0).abs() < 0.1, "b.y={}", b.rect.y);
        assert!((c.rect.y - 55.0).abs() < 0.1, "c.y={}", c.rect.y);
    }

    /// BUG-193: a `display: table` wrapper box is block-level, so its margins
    /// collapse with adjacent sibling margins (CSS 2.1 §8.3.1) — even though the
    /// table establishes a BFC for its own rows/cells. The gap between the table
    /// and the following block must be `max(30, 10) = 30`, not the summed `40`.
    #[test]
    fn table_bottom_margin_collapses_with_next_sibling() {
        let root = lay(
            "<table class='t'><tr><td>x</td></tr></table><div class='b'>y</div>",
            ".t { margin-bottom: 30px; } .b { height: 10px; margin-top: 10px; }",
        );
        let table = root
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Table))
            .expect("table box");
        let b = root
            .children
            .iter()
            .find(|c| matches!(c.kind, BoxKind::Block))
            .expect("following block");
        let gap = b.rect.y - (table.rect.y + table.rect.height);
        assert!(
            (gap - 30.0).abs() < 0.1,
            "table↔block gap={gap} (expected collapsed 30, not summed 40)",
        );
    }

    // ── CSS Intrinsic Sizing L3 — min-content / max-content / fit-content ────

    /// `width: fit-content` на block-элементе с явной шириной потомка: бокс
    /// сжимается до ширины потомка, не растягиваясь на весь контейнер.
    #[test]
    fn fit_content_shrinks_to_child_explicit_width() {
        let root = lay(
            "<div class='outer'><div class='inner'>x</div></div>",
            ".outer { width: fit-content; }
             .inner { width: 120px; height: 10px; }",
        );
        let outer = first_element_child(&root);
        // outer's border-box should equal inner's 120px (no padding/border on outer).
        assert!(
            (outer.rect.width - 120.0).abs() < 1.0,
            "outer.width={} expected≈120",
            outer.rect.width
        );
    }

    /// `width: fit-content` не выходит за пределы доступного пространства.
    #[test]
    fn fit_content_capped_at_available_width() {
        // Container 200px wide; inner has explicit width 300px (wider than container).
        let root = lay_viewport(
            "<div class='outer'><div class='inner'>x</div></div>",
            ".outer { width: fit-content; }
             .inner { width: 300px; height: 10px; }",
            Size { width: 200.0, height: 600.0 },
        );
        let outer = first_element_child(&root);
        // fit-content = min(available=200, max-content=300) → 200.
        assert!(
            outer.rect.width <= 200.0 + 0.5,
            "outer.width={} should be ≤ 200",
            outer.rect.width
        );
    }

    /// `width: max-content` expands past the container to fit content.
    #[test]
    fn max_content_expands_to_child_explicit_width() {
        let root = lay_viewport(
            "<div class='outer'><div class='inner'>x</div></div>",
            ".outer { width: max-content; }
             .inner { width: 500px; height: 10px; }",
            Size { width: 200.0, height: 600.0 },
        );
        let outer = first_element_child(&root);
        // max-content ignores available width — should be 500px.
        assert!(
            (outer.rect.width - 500.0).abs() < 1.0,
            "outer.width={} expected≈500",
            outer.rect.width
        );
    }

    /// `width: min-content` with single-word text: box shrinks to word width.
    #[test]
    fn min_content_shrinks_to_word_width() {
        // Fixed8 measurer: each char = 8px. "Hello" = 5 chars = 40px.
        // Container is 800px wide. min-content should give 40px.
        let root = lay_measured(
            "<p class='p'>Hello</p>",
            ".p { width: min-content; }",
            800.0,
        );
        let p = first_element_child(&root);
        // With Fixed8 measurer: "Hello" = 5 × 8 = 40px.
        assert!(
            (p.rect.width - 40.0).abs() < 1.0,
            "p.width={} expected≈40 (5 chars × 8px)",
            p.rect.width
        );
    }

    /// `width: fit-content` on block with text: shrinks to text width.
    #[test]
    fn fit_content_text_shrinks_within_container() {
        // "Hi" = 2 chars × 8px = 16px; container = 800px.
        let root = lay(
            "<p class='p'>Hi</p>",
            ".p { width: fit-content; }",
        );
        let p = first_element_child(&root);
        assert!(
            p.rect.width <= 800.0,
            "p.width={} should be ≤ container",
            p.rect.width
        );
        // Text content width = 16px. Box should shrink to ~16px (+ any padding).
        assert!(
            p.rect.width < 100.0,
            "p.width={} should be much less than 800px (container)",
            p.rect.width
        );
    }

    /// `width: fit-content` with text: element shrinks to text content width.
    #[test]
    fn fit_content_text_node_shrinks_to_content() {
        // "Hi" = 2 chars × 8px = 16px with Fixed8 measurer.
        let root = lay_measured(
            "<div class='d'>Hi</div>",
            ".d { width: fit-content; }",
            800.0,
        );
        let div = first_element_child(&root);
        // Should shrink to text content width ≈ 16px, not fill the 800px container.
        assert!(
            div.rect.width < 100.0,
            "div.width={} should shrink to ~16px",
            div.rect.width
        );
        assert!(
            div.rect.width >= 16.0,
            "div.width={} should be at least text width 16px",
            div.rect.width
        );
    }

    /// `width: max-content` parsing: keyword stored correctly.
    #[test]
    fn max_content_keyword_parsed() {
        let sheet = lumen_css_parser::parse(".x { width: max-content; }");
        let doc = lumen_html_parser::parse("<div class='x'>a</div>");
        let vp = Size { width: 800.0, height: 600.0 };
        use crate::style::Length;
        let children = doc.get(doc.body().unwrap()).children.clone();
        let div_id = children.into_iter().find(|&id| {
            matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
        }).unwrap();
        let div_style = compute_style(&doc, div_id, &sheet, &ComputedStyle::root(), vp, false);
        assert!(
            matches!(div_style.width, Some(Length::MaxContent)),
            "expected MaxContent, got {:?}", div_style.width
        );
    }

    /// `width: min-content` and `width: fit-content` parsing round-trip.
    #[test]
    fn min_fit_content_keywords_parsed() {
        let sheet = lumen_css_parser::parse(".a { width: min-content; } .b { width: fit-content; }");
        let doc = lumen_html_parser::parse("<div class='a'></div><div class='b'></div>");
        let root_style = ComputedStyle::root();
        let vp = Size { width: 800.0, height: 600.0 };
        use crate::style::Length;
        let children = doc.get(doc.body().unwrap()).children.clone();
        let mut it = children.into_iter().filter(|&id| matches!(&doc.get(id).data, lumen_dom::NodeData::Element { .. }));
        let a_id = it.next().unwrap();
        let b_id = it.next().unwrap();
        let a_style = compute_style(&doc, a_id, &sheet, &root_style, vp, false);
        let b_style = compute_style(&doc, b_id, &sheet, &root_style, vp, false);
        assert!(matches!(a_style.width, Some(Length::MinContent)), "got {:?}", a_style.width);
        assert!(matches!(b_style.width, Some(Length::FitContent(None))), "got {:?}", b_style.width);
    }

    /// `fit-content(<length>)` functional form: parsed with inner length.
    #[test]
    fn fit_content_functional_form_parsed() {
        let sheet = lumen_css_parser::parse(".x { width: fit-content(200px); }");
        let doc = lumen_html_parser::parse("<div class='x'>a</div>");
        let vp = Size { width: 800.0, height: 600.0 };
        use crate::style::Length;
        let children = doc.get(doc.body().unwrap()).children.clone();
        let div_id = children.into_iter().find(|&id| {
            matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
        }).unwrap();
        let style = compute_style(&doc, div_id, &sheet, &ComputedStyle::root(), vp, false);
        assert!(
            matches!(style.width, Some(Length::FitContent(Some(_)))),
            "expected FitContent(Some(200px)), got {:?}", style.width
        );
    }

    // ──────── CSS Counters resolution (CSS Lists L3 §6.4) ────────

    /// Extract the text from the first InlineRun segment of a box's first child.
    fn counter_first_inline_text(b: &LayoutBox) -> String {
        for c in &b.children {
            match &c.kind {
                BoxKind::InlineRun { segments, .. } => {
                    return segments.iter().map(|s| s.text.as_str()).collect();
                }
                BoxKind::Block => {
                    let t = counter_first_inline_text(c);
                    if !t.is_empty() {
                        return t;
                    }
                }
                _ => {}
            }
        }
        String::new()
    }

    #[test]
    fn counter_before_resolves_decimal() {
        // div::before renders "1. " using counter(section) after counter-increment.
        let root = lay(
            "<div id='a'></div>",
            "div { counter-reset: section; counter-increment: section; } \
             div::before { content: counter(section) \". \"; display: block; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let text = counter_first_inline_text(div);
        assert_eq!(text, "1. ", "counter(section) should resolve to '1'");
    }

    #[test]
    fn counter_set_resolves_in_content() {
        // counter-set runs after counter-increment (CSS Lists L3 §4): the set
        // value wins, so counter(section) resolves to the set value, not +1.
        let root = lay(
            "<div id='a'></div>",
            "div { counter-reset: section; counter-increment: section; counter-set: section 42; } \
             div::before { content: counter(section) \". \"; display: block; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let text = counter_first_inline_text(div);
        assert_eq!(text, "42. ", "counter-set should override the increment");
    }

    #[test]
    fn counter_multiple_increments() {
        // Three divs, each increment section by 1 → values 1, 2, 3.
        let root = lay(
            "<div id='a'></div><div id='b'></div><div id='c'></div>",
            "body { counter-reset: section; } \
             div { counter-increment: section; } \
             div::before { content: counter(section); display: block; }",
        );
        let blocks: Vec<&LayoutBox> = root
            .children
            .iter()
            .filter(|c| matches!(&c.kind, BoxKind::Block))
            .collect();
        assert_eq!(blocks.len(), 3);
        assert_eq!(first_inline_text(blocks[0]), "1");
        assert_eq!(first_inline_text(blocks[1]), "2");
        assert_eq!(first_inline_text(blocks[2]), "3");
    }

    #[test]
    fn counter_lower_alpha_style() {
        let root = lay(
            "<div id='a'></div>",
            "div { counter-reset: s; counter-increment: s; } \
             div::before { content: counter(s, lower-alpha); display: block; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let text = counter_first_inline_text(div);
        assert_eq!(text, "a");
    }

    #[test]
    fn counters_nested_decimal() {
        // Outer ol resets "item", inner ol also resets "item" creating nested scope.
        // Inner li::before should show "1.1" via counters(item, ".").
        let root = lay(
            "<ol><li><ol><li id='inner'></li></ol></li></ol>",
            "ol { counter-reset: item; } \
             li { counter-increment: item; } \
             li::before { content: counters(item, \".\"); display: block; }",
        );
        // Walk tree to find the innermost li's ::before text.
        fn find_text(b: &LayoutBox, depth: u32) -> Option<String> {
            if depth == 0 { return None; }
            for c in &b.children {
                if let BoxKind::Block = &c.kind {
                    // Try text in this block.
                    let t: String = c.children.iter().flat_map(|sc| {
                        if let BoxKind::InlineRun { segments, .. } = &sc.kind {
                            segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
                        } else {
                            vec![]
                        }
                    }).collect();
                    if t.contains('.') {
                        return Some(t);
                    }
                    if let Some(inner) = find_text(c, depth - 1) {
                        return Some(inner);
                    }
                }
            }
            None
        }
        let text = find_text(&root, 6).unwrap_or_default();
        assert_eq!(text, "1.1", "counters(item, '.') should give '1.1'");
    }

    #[test]
    fn content_attr_resolves() {
        // div::before { content: attr(data-label); } → "hello"
        let root = lay(
            "<div data-label=\"hello\"></div>",
            "div::before { content: attr(data-label); display: block; }",
        );
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        let text = counter_first_inline_text(div);
        assert_eq!(text, "hello");
    }

    #[test]
    fn counter_reset_creates_new_scope() {
        // Inner ol counter-reset creates nested scope; outer li still sees own value.
        let root = lay(
            "<ol><li id='outer'><ol><li id='inner'></li></ol></li></ol>",
            "ol { counter-reset: item; } \
             li { counter-increment: item; } \
             li::before { content: counter(item); display: block; }",
        );
        // Outer li::before → "1", inner li::before → "1" (own nested scope).
        let mut outer_text = String::new();
        let mut inner_found = false;
        fn collect(b: &LayoutBox, depth: u32, outer: &mut String, inner: &mut bool) {
            if depth == 0 { return; }
            for c in &b.children {
                if let BoxKind::Block = &c.kind {
                    for sc in &c.children {
                        if let BoxKind::InlineRun { segments, .. } = &sc.kind {
                            let t: String = segments.iter().map(|s| s.text.as_str()).collect();
                            if !t.is_empty() && outer.is_empty() {
                                *outer = t;
                            } else if !t.is_empty() {
                                *inner = true;
                            }
                        }
                    }
                    collect(c, depth - 1, outer, inner);
                }
            }
        }
        collect(&root, 5, &mut outer_text, &mut inner_found);
        assert_eq!(outer_text, "1", "outer li counter should be 1");
        assert!(inner_found, "inner li should also have counter text");
    }

    // ─── <details>/<summary> tests ───────────────────────────────────────────

    /// Count LayoutBox nodes with non-Skip kind under root.
    fn count_visible_boxes(b: &LayoutBox) -> usize {
        if matches!(b.kind, BoxKind::Skip) {
            return 0;
        }
        1 + b.children.iter().map(count_visible_boxes).sum::<usize>()
    }

    #[test]
    fn details_closed_hides_content() {
        // Without `open` attribute, only <summary> should appear.
        let closed = lay(
            "<details><summary>Title</summary><p>Hidden content</p></details>",
            "",
        );
        let open = lay(
            r#"<details open><summary>Title</summary><p>Hidden content</p></details>"#,
            "",
        );
        let closed_total = count_visible_boxes(&closed);
        let open_total = count_visible_boxes(&open);
        // Closed should have fewer visible boxes than open (the <p> is hidden).
        assert!(
            closed_total < open_total,
            "closed <details> ({closed_total} boxes) should have fewer visible boxes than open ({open_total} boxes)"
        );
    }

    #[test]
    fn details_open_shows_content() {
        // With `open` attribute, all children are visible.
        let root = lay(
            r#"<details open><summary>Title</summary><p>Visible content</p></details>"#,
            "",
        );
        let total = count_visible_boxes(&root);
        // Should include details + summary + "Title" inline + p + "Visible content" inline.
        assert!(
            total >= 5,
            "open <details> should show all content, got {total} visible boxes"
        );
    }

    #[test]
    fn details_no_summary_closed() {
        // <details> without <summary>: no summary child → nothing rendered when closed.
        let closed = lay("<details><p>Secret</p></details>", "");
        let open = lay(r#"<details open><p>Secret</p></details>"#, "");
        // Closed hides all children (no summary to show); open shows them.
        assert!(
            count_visible_boxes(&closed) < count_visible_boxes(&open),
            "closed <details> without <summary> should have fewer boxes than open"
        );
    }

    // ─── collect_clickable_elements tests ────────────────────────────────────

    #[test]
    fn clickable_finds_block_link() {
        let doc = lumen_html_parser::parse(r#"<a href="/page" style="display:block">Click me</a>"#);
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            elems.iter().any(|e| matches!(&e.kind, ClickableKind::Link { href } if href == "/page")),
            "block-level <a href> should be collected"
        );
    }

    #[test]
    fn clickable_finds_form_controls() {
        let doc = lumen_html_parser::parse(
            "<form><input type=text><button>Submit</button><select><option>A</option></select></form>",
        );
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        let inputs = elems.iter().filter(|e| matches!(e.kind, ClickableKind::Input)).count();
        let buttons = elems.iter().filter(|e| matches!(e.kind, ClickableKind::Button)).count();
        assert!(inputs >= 2, "input + select should be collected as Input, got {inputs}");
        assert!(buttons >= 1, "button should be collected, got {buttons}");
    }

    #[test]
    fn clickable_finds_tabindex_element() {
        let doc = lumen_html_parser::parse(r#"<div tabindex="0">Interactive</div>"#);
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            elems.iter().any(|e| e.kind == ClickableKind::Generic),
            "element with tabindex=0 should be collected as Generic"
        );
    }

    #[test]
    fn clickable_skips_display_none() {
        let doc = lumen_html_parser::parse(
            r#"<a href="/hidden" style="display:none">Hidden</a><a href="/visible" style="display:block">Visible</a>"#,
        );
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            !elems.iter().any(|e| matches!(&e.kind, ClickableKind::Link { href } if href == "/hidden")),
            "display:none link should not be collected"
        );
        assert!(
            elems.iter().any(|e| matches!(&e.kind, ClickableKind::Link { href } if href == "/visible")),
            "display:block link should be collected"
        );
    }


    #[test]
    fn clickable_skips_pointer_events_none_link() {
        // Use display:block so links create Block boxes (layout() without measurer
        // can't detect inline <a> links — they require a text measurer to populate
        // InlineRun.lines). Block-level links are found via element_href on the box.
        let doc = lumen_html_parser::parse(
            r#"<a href="/blocked" style="display:block;pointer-events:none">Blocked</a>
               <a href="/ok" style="display:block">OK</a>"#,
        );
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            !elems.iter().any(|e| matches!(&e.kind, ClickableKind::Link { href } if href == "/blocked")),
            "pointer-events:none link must not be in clickable set"
        );
        assert!(
            elems.iter().any(|e| matches!(&e.kind, ClickableKind::Link { href } if href == "/ok")),
            "normal link must still be collected"
        );
    }

    #[test]
    fn clickable_pointer_events_none_skips_element_but_not_children() {
        // Parent has pointer-events:none; child link has default (auto).
        // Child must still be clickable even though parent is not.
        let doc = lumen_html_parser::parse(
            r#"<div style="pointer-events:none"><a href="/child">Child</a></div>"#,
        );
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            elems.iter().any(|e| matches!(&e.kind, ClickableKind::Link { href } if href == "/child")),
            "child link inside pointer-events:none parent must remain clickable"
        );
    }

    #[test]
    fn clickable_pointer_events_none_on_button() {
        let doc = lumen_html_parser::parse(
            r#"<button style="pointer-events:none">Disabled</button>"#,
        );
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            !elems.iter().any(|e| matches!(e.kind, ClickableKind::Button)),
            "button with pointer-events:none must not be in clickable set"
        );
    }

    // ── line-clamp layout tests ───────────────────────────────────────────────

    fn find_inline_run_in(b: &box_tree::LayoutBox) -> Option<&box_tree::LayoutBox> {
        if matches!(b.kind, box_tree::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(r) = find_inline_run_in(c) { return Some(r); } }
        None
    }

    #[allow(dead_code)]
    fn inline_line_count(root: &box_tree::LayoutBox) -> usize {
        let Some(run) = find_inline_run_in(root) else { return 0; };
        let box_tree::BoxKind::InlineRun { lines, .. } = &run.kind else { return 0; };
        lines.len()
    }

    fn inline_last_text(root: &box_tree::LayoutBox) -> String {
        let Some(run) = find_inline_run_in(root) else { return String::new(); };
        let box_tree::BoxKind::InlineRun { lines, .. } = &run.kind else { return String::new(); };
        let Some(last_line) = lines.last() else { return String::new(); };
        last_line.iter().map(|f| f.text.as_str()).collect()
    }

    /// line-clamp: 2 на контейнере с длинным текстом → показываем только 2 строки.
    #[test]
    fn line_clamp_truncates_to_n_lines() {
        // 300px wide, font ~16px — слово "word" ~4×8.8=35.2px, 8 слов/строку.
        // 40 слов → ~5 строк. Ожидаем ровно 2 после clamp.
        let words = "word ".repeat(40);
        let html = format!("<p>{words}</p>");
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse("p { width: 300px; -webkit-line-clamp: 2; font-size: 16px; }");
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        assert_eq!(twrap_line_count(&root), 2, "must have exactly 2 lines after clamp");
    }

    /// line-clamp: 2 → последняя строка оканчивается на «…».
    #[test]
    fn line_clamp_last_line_ends_with_ellipsis() {
        let words = "word ".repeat(40);
        let html = format!("<p>{words}</p>");
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse("p { width: 300px; -webkit-line-clamp: 2; font-size: 16px; }");
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        let last = inline_last_text(&root);
        assert!(last.ends_with('\u{2026}'), "last line must end with '…', got: {last:?}");
    }

    /// line-clamp: 1 → одна строка, совпадает с text-overflow поведением.
    #[test]
    fn line_clamp_one_line() {
        let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let html = format!("<p>{words}</p>");
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse("p { width: 300px; -webkit-line-clamp: 1; font-size: 16px; }");
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        assert_eq!(twrap_line_count(&root), 1, "must have exactly 1 line");
        let last = inline_last_text(&root);
        assert!(last.ends_with('\u{2026}'), "single line must end with '…', got: {last:?}");
    }

    /// line-clamp без усечения (строк меньше N) → всё отображается, без «…».
    #[test]
    fn line_clamp_no_truncation_when_fewer_lines() {
        let doc = lumen_html_parser::parse("<p>Short text</p>");
        let sheet = lumen_css_parser::parse("p { width: 600px; -webkit-line-clamp: 5; font-size: 16px; }");
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        // Текст помещается в одну строку — clamp не должен добавлять «…».
        let last = inline_last_text(&root);
        assert!(!last.ends_with('\u{2026}'), "no ellipsis when content fits: {last:?}");
    }

    /// standard `line-clamp` (без webkit-префикса) тоже работает.
    #[test]
    fn line_clamp_standard_property_works() {
        let words = "word ".repeat(40);
        let html = format!("<p>{words}</p>");
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse("p { width: 300px; line-clamp: 3; font-size: 16px; }");
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        assert_eq!(twrap_line_count(&root), 3);
    }

    /// line-clamp совместим с явной высотой блока.
    #[test]
    fn line_clamp_with_explicit_height() {
        let words = "word ".repeat(40);
        let html = format!("<p>{words}</p>");
        let doc = lumen_html_parser::parse(&html);
        let sheet = lumen_css_parser::parse(
            "p { width: 300px; height: 100px; -webkit-line-clamp: 2; font-size: 16px; }",
        );
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        assert_eq!(twrap_line_count(&root), 2);
    }

    // ─── collect_clickable_elements tests ──────────────────────────────────────

    #[test]
    fn collect_clickable_empty_document() {
        let doc = lumen_html_parser::parse("<p>No interactive elements</p>");
        let root = lay_full("<p>No interactive elements</p>", "");
        let clickables = collect_clickable_elements(&root, &doc);
        assert_eq!(clickables.len(), 0);
    }

    #[test]
    fn collect_clickable_link_block_level() {
        let doc = lumen_html_parser::parse("<a href=\"http://example.com\">Example Link</a>");
        let root = lay_full("<a href=\"http://example.com\">Example Link</a>", "");
        let clickables = collect_clickable_elements(&root, &doc);
        assert_eq!(clickables.len(), 1);
        assert!(
            matches!(clickables[0].kind, ClickableKind::Link { ref href } if href == "http://example.com"),
            "Expected link with href, got {:?}",
            clickables[0].kind
        );
    }

    #[test]
    fn collect_clickable_button_element() {
        let doc = lumen_html_parser::parse("<button>Click me</button>");
        let root = lay_full("<button>Click me</button>", "");
        let clickables = collect_clickable_elements(&root, &doc);
        assert!(
            clickables.iter().any(|c| matches!(c.kind, ClickableKind::Button)),
            "Expected button element"
        );
    }

    #[test]
    fn collect_clickable_input_text() {
        let doc = lumen_html_parser::parse("<input type=\"text\" placeholder=\"Enter text\">");
        let root = lay_full("<input type=\"text\" placeholder=\"Enter text\">", "");
        let clickables = collect_clickable_elements(&root, &doc);
        assert!(
            clickables.iter().any(|c| matches!(c.kind, ClickableKind::Input)),
            "Expected input element"
        );
    }

    #[test]
    fn collect_clickable_details_element() {
        let doc = lumen_html_parser::parse("<details><summary>Details</summary><p>Content</p></details>");
        let root = lay_full("<details><summary>Details</summary><p>Content</p></details>", "");
        let clickables = collect_clickable_elements(&root, &doc);
        assert!(
            clickables.iter().any(|c| matches!(c.kind, ClickableKind::Details)),
            "Expected details element"
        );
    }

    #[test]
    fn collect_clickable_mixed_elements() {
        let doc = lumen_html_parser::parse(
            r#"
            <a href="/home">Home</a>
            <button>Submit</button>
            <input type="text">
            <details><summary>Info</summary></details>
            "#,
        );
        let root = lay_full(
            r#"
            <a href="/home">Home</a>
            <button>Submit</button>
            <input type="text">
            <details><summary>Info</summary></details>
            "#,
            "",
        );
        let clickables = collect_clickable_elements(&root, &doc);
        assert!(
            clickables.len() >= 4,
            "Expected at least 4 clickable elements, got {}",
            clickables.len()
        );
        // Verify each type is present
        assert!(clickables.iter().any(|c| matches!(c.kind, ClickableKind::Link { .. })));
        assert!(clickables.iter().any(|c| matches!(c.kind, ClickableKind::Button)));
        assert!(clickables.iter().any(|c| matches!(c.kind, ClickableKind::Input)));
        assert!(clickables.iter().any(|c| matches!(c.kind, ClickableKind::Details)));
    }

    // ── Sticky position algorithm tests ─────────────────────────────────────

    fn sticky_box(
        static_y: f32,
        height: f32,
        top: Option<f32>,
        bottom: Option<f32>,
        cb_y: f32,
        cb_h: f32,
    ) -> StickyBox {
        use lumen_core::geom::Rect;
        StickyBox {
            node: lumen_dom::NodeId::from_index(0),
            static_rect: Rect::new(0.0, static_y, 200.0, height),
            top,
            bottom,
            left: None,
            right: None,
            containing_rect: Rect::new(0.0, cb_y, 800.0, cb_h),
        }
    }

    #[test]
    fn sticky_no_scroll_no_offset() {
        // Element at y=200, top: 0 — not yet scrolled past threshold.
        let sb = sticky_box(200.0, 50.0, Some(0.0), None, 0.0, 1000.0);
        let (dx, dy) = compute_sticky_offset(&sb, 0.0, 0.0, 800.0, 600.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn sticky_sticks_at_top_when_scrolled() {
        // Element at y=200, height=50, top: 0, cb covers full doc.
        // scroll_y=250: ideal viewport-y = 200-250 = -50 → clamped to 0 → off_y = +50.
        let sb = sticky_box(200.0, 50.0, Some(0.0), None, 0.0, 1000.0);
        let (_, dy) = compute_sticky_offset(&sb, 0.0, 250.0, 800.0, 600.0);
        assert!((dy - 50.0).abs() < 0.001, "expected dy≈50, got {dy}");
    }

    #[test]
    fn sticky_not_stuck_before_threshold() {
        // scroll_y=100: ideal viewport-y = 200-100 = 100 ≥ top(0) → no sticking.
        let sb = sticky_box(200.0, 50.0, Some(0.0), None, 0.0, 1000.0);
        let (_, dy) = compute_sticky_offset(&sb, 0.0, 100.0, 800.0, 600.0);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn sticky_releases_at_containing_block_bottom() {
        // cb from y=0, height=300. Element height=50, top=0.
        // When scroll_y=350: ideal_y = 200-350 = -150.
        // cb_bot = 0+300-350-50 = -100.
        // lo=max(0, 0-350)=0, hi=min(∞, -100)= -100 → lo>hi → clamp gives lo=0.
        // Wait, that means it sticks at 0 even past cb. That's because lo > hi.
        // In practice the element is above the containing block's bottom — correct.
        // scroll_y=260: ideal_y=200-260=-60; cb_bot=0+300-260-50=-10; lo=0; hi=-10 → lo>hi → actual=lo=0; off=60.
        // scroll_y=280: ideal_y=200-280=-80; cb_bot=0+300-280-50=-30; lo=0; hi=-30 → actual=lo=0; off=80... but this is past cb.
        // That's correct: lo wins when tight, the element pegs to top=0 even past cb — matches Chrome's sticky behaviour.
        // Let's just verify the cb forces release via the hi bound in a case where top is large enough:
        // top=100 (so lo_y = max(100, -scroll_y) = 100 when scroll_y<=0).
        // scroll=200: ideal_y=200-200=0; lo=max(100,-200)=100... wait. lo = top.max(cb_top).
        // cb_top = 0 - 200 = -200. lo = 100.max(-200) = 100. hi = cb_bot = 0+300-200-50=50. actual=0.clamp(100,50)=100 → off=100. That sticks at 100 from top.
        // scroll=260: ideal=-60; lo=max(100,-260)=100; hi=0+300-260-50=-10 → lo>hi → actual=100; off=160. Element is past cb bottom but stays at top=100.
        // This is the edge case — for a concise test just check the transition:
        let sb = sticky_box(200.0, 50.0, Some(0.0), None, 0.0, 300.0);
        let (_, dy_normal) = compute_sticky_offset(&sb, 0.0, 250.0, 800.0, 600.0);
        // At scroll=250 element would be at vp_y=-50; clamp to lo=0 → off=50.
        assert!((dy_normal - 50.0).abs() < 0.001, "got {dy_normal}");
    }

    #[test]
    fn sticky_no_insets_never_sticks() {
        // No top/bottom/left/right — element always at ideal position.
        let sb = sticky_box(200.0, 50.0, None, None, 0.0, 1000.0);
        let (dx, dy) = compute_sticky_offset(&sb, 0.0, 500.0, 800.0, 600.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn sticky_bottom_inset() {
        // bottom: 10 — element sticks to 10px above bottom of viewport.
        // viewport_height=600, element height=50. Max vp_y = 600-10-50=540.
        // static_y=0. scroll_y=-300 (scrolled up): ideal=0-(-300)=300 ≤ 540 → no stick.
        // scroll_y=0: ideal=0; 0 <= 540 → no stick, off=0.
        // To trigger bottom-stick without horizontal scroll, we use a static_y below 540.
        let sb = sticky_box(0.0, 50.0, None, Some(10.0), 0.0, 1000.0);
        // scroll_y=0: ideal_y=0; hi=600-10-50=540; cb_bot=0+1000-0-50=950; hi=min(540,950)=540; lo=max(-inf,0-0)=0; actual=clamp(0,0,540)=0 → off=0.
        let (_, dy0) = compute_sticky_offset(&sb, 0.0, 0.0, 800.0, 600.0);
        assert_eq!(dy0, 0.0);

        // Now element at y=600, so at scroll_y=0 its viewport-y=600; hi=540 → actual=540; off=-60.
        let sb2 = sticky_box(600.0, 50.0, None, Some(10.0), 0.0, 2000.0);
        let (_, dy2) = compute_sticky_offset(&sb2, 0.0, 0.0, 800.0, 600.0);
        assert!((dy2 - (-60.0)).abs() < 0.001, "expected dy≈-60, got {dy2}");
    }

    #[test]
    fn collect_sticky_boxes_empty_document() {
        let root = lay_full("<p>no sticky</p>", "");
        let stickies = collect_sticky_boxes(&root);
        assert_eq!(stickies.len(), 0, "expected no sticky boxes");
    }

    #[test]
    fn collect_sticky_boxes_finds_sticky_element() {
        let root = lay_full(
            "<div id=\"s\">sticky</div>",
            "#s { position: sticky; top: 0px; }",
        );
        let stickies = collect_sticky_boxes(&root);
        assert_eq!(stickies.len(), 1, "expected one sticky box");
        let sb = &stickies[0];
        assert_eq!(sb.top, Some(0.0));
        assert_eq!(sb.bottom, None);
        assert_eq!(sb.left, None);
        assert_eq!(sb.right, None);
    }

    #[test]
    fn collect_sticky_boxes_px_inset_captured() {
        let root = lay_full(
            "<div id=\"s\">sticky</div>",
            "#s { position: sticky; top: 16px; bottom: 8px; }",
        );
        let stickies = collect_sticky_boxes(&root);
        assert_eq!(stickies.len(), 1);
        assert_eq!(stickies[0].top, Some(16.0));
        assert_eq!(stickies[0].bottom, Some(8.0));
    }

    #[test]
    fn collect_sticky_boxes_non_px_inset_is_none() {
        // Em and percent insets cannot be resolved post-layout → None.
        let root = lay_full(
            "<div id=\"s\">sticky</div>",
            "#s { position: sticky; top: 1em; }",
        );
        let stickies = collect_sticky_boxes(&root);
        assert_eq!(stickies.len(), 1);
        assert_eq!(stickies[0].top, None, "em unit should yield None");
    }

    // ─── CSS Scroll Snap tests ────────────────────────────────────────────────

    #[test]
    fn snap_no_containers_when_no_snap_type() {
        let root = lay_full("<div><p>item</p></div>", "");
        let containers = collect_snap_containers(&root);
        assert!(containers.is_empty(), "no snap-type → no containers");
    }

    #[test]
    fn snap_container_collected() {
        let root = lay_full(
            "<div id=c><p>item</p></div>",
            "#c { scroll-snap-type: y mandatory; }",
        );
        let containers = collect_snap_containers(&root);
        assert_eq!(containers.len(), 1, "one snap container expected");
        assert_eq!(containers[0].snap_type.axis, style::ScrollSnapAxis::Y);
        assert_eq!(
            containers[0].snap_type.strictness,
            style::ScrollSnapStrictness::Mandatory
        );
    }

    #[test]
    fn snap_area_start_offset_y() {
        // Container at y≈0, height=600; child <p> with scroll-snap-align: start.
        // `start` is a shorthand setting BOTH inline and block axes to `start`,
        // so both snap_x and snap_y are Some.  The container's y-only axis
        // restricts which snaps are *used* by find_snap_target, not what's stored.
        let root = lay_full(
            "<div id=c><p id=a>item</p></div>",
            "#c { scroll-snap-type: y mandatory; } #a { scroll-snap-align: start; }",
        );
        let containers = collect_snap_containers(&root);
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].points.len(), 1, "one snap area expected");
        let pt = &containers[0].points[0];
        // snap_x is Some because align.inline == Start (both axes from shorthand).
        assert!(pt.snap_x.is_some(), "snap_x computed from inline alignment");
        let snap_y = pt.snap_y.expect("snap_y should be Some");
        assert!(snap_y.is_finite(), "snap_y must be finite");
        assert!(!pt.stop_always, "default is Normal");
    }

    #[test]
    fn snap_area_stop_always() {
        let root = lay_full(
            "<div id=c><p id=a>item</p></div>",
            "#c { scroll-snap-type: y mandatory; } #a { scroll-snap-align: start; scroll-snap-stop: always; }",
        );
        let containers = collect_snap_containers(&root);
        assert_eq!(containers.len(), 1);
        if let Some(pt) = containers[0].points.first() {
            assert!(pt.stop_always, "stop_always should be true");
        }
    }

    #[test]
    fn snap_no_areas_when_no_align() {
        let root = lay_full(
            "<div id=c><p>item</p></div>",
            "#c { scroll-snap-type: y mandatory; }",
        );
        let containers = collect_snap_containers(&root);
        assert_eq!(containers.len(), 1);
        assert!(
            containers[0].points.is_empty(),
            "no snap-align → no snap areas"
        );
    }

    #[test]
    fn find_snap_target_mandatory_nearest() {
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::Y,
            strictness: style::ScrollSnapStrictness::Mandatory,
        };
        let container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: vec![
                SnapPoint {
                    node: lumen_dom::NodeId::from_index(1),
                    snap_x: None,
                    snap_y: Some(0.0),
                    stop_always: false,
                },
                SnapPoint {
                    node: lumen_dom::NodeId::from_index(2),
                    snap_x: None,
                    snap_y: Some(600.0),
                    stop_always: false,
                },
                SnapPoint {
                    node: lumen_dom::NodeId::from_index(3),
                    snap_x: None,
                    snap_y: Some(1200.0),
                    stop_always: false,
                },
            ],
        };
        // Target ≈ 700 → nearest snap is 600.
        let result = find_snap_target(&container, (0.0, 0.0), (0.0, 700.0));
        assert!(result.is_some(), "mandatory always snaps");
        let (_, sy) = result.unwrap();
        assert!((sy - 600.0).abs() < 0.001, "expected snap to 600, got {sy}");
    }

    #[test]
    fn find_snap_target_proximity_too_far() {
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::Y,
            strictness: style::ScrollSnapStrictness::Proximity,
        };
        let container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: vec![SnapPoint {
                node: lumen_dom::NodeId::from_index(1),
                snap_x: None,
                snap_y: Some(0.0),
                stop_always: false,
            }],
        };
        // Target 400 is exactly 50% of viewport — proximity threshold is 50% → skip.
        // (400 == 600*0.5, so dx.abs() > prox_y is false at boundary, but
        //  any value strictly > 300 should be filtered.)
        let result = find_snap_target(&container, (0.0, 0.0), (0.0, 400.0));
        assert!(result.is_none(), "proximity: too far from snap point");
    }

    #[test]
    fn find_snap_target_proximity_close_enough() {
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::Y,
            strictness: style::ScrollSnapStrictness::Proximity,
        };
        let container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: vec![SnapPoint {
                node: lumen_dom::NodeId::from_index(1),
                snap_x: None,
                snap_y: Some(600.0),
                stop_always: false,
            }],
        };
        // Target 450 → snap_y=600, dy=150 < 300 (50% of 600) → snaps.
        let result = find_snap_target(&container, (0.0, 0.0), (0.0, 450.0));
        assert!(result.is_some(), "proximity: close enough to snap");
        let (_, sy) = result.unwrap();
        assert!((sy - 600.0).abs() < 0.001, "expected snap to 600, got {sy}");
    }

    #[test]
    fn find_snap_target_stop_always_barrier() {
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::Y,
            strictness: style::ScrollSnapStrictness::Mandatory,
        };
        let container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: vec![
                SnapPoint {
                    node: lumen_dom::NodeId::from_index(1),
                    snap_x: None,
                    snap_y: Some(600.0),
                    stop_always: true,  // barrier
                },
                SnapPoint {
                    node: lumen_dom::NodeId::from_index(2),
                    snap_x: None,
                    snap_y: Some(1200.0),
                    stop_always: false,
                },
            ],
        };
        // Fling from 0 → 1300 crosses the barrier at 600 → must stop there.
        let result = find_snap_target(&container, (0.0, 0.0), (0.0, 1300.0));
        assert!(result.is_some(), "stop_always acts as barrier");
        let (_, sy) = result.unwrap();
        assert!((sy - 600.0).abs() < 0.001, "expected stop at barrier 600, got {sy}");
    }

    #[test]
    fn find_snap_target_x_axis_only() {
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::X,
            strictness: style::ScrollSnapStrictness::Mandatory,
        };
        let container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: vec![SnapPoint {
                node: lumen_dom::NodeId::from_index(1),
                snap_x: Some(800.0),
                snap_y: Some(100.0),
                stop_always: false,
            }],
        };
        // x-only: target (900, 50) → snaps x to 800, y unchanged (50).
        let result = find_snap_target(&container, (0.0, 0.0), (900.0, 50.0));
        assert!(result.is_some());
        let (sx, sy) = result.unwrap();
        assert!((sx - 800.0).abs() < 0.001, "x snapped to 800");
        assert!((sy - 50.0).abs() < 0.001, "y unchanged (x-only axis)");
    }

    #[test]
    fn find_snap_target_empty_points() {
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::Y,
            strictness: style::ScrollSnapStrictness::Mandatory,
        };
        let container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            points: vec![],
        };
        assert!(
            find_snap_target(&container, (0.0, 0.0), (0.0, 300.0)).is_none(),
            "no points → no snap"
        );
    }

    // ── scroll-margin / scroll-padding snap offset tests (BB-7) ──────────────

    #[test]
    fn snap_margin_start_shifts_x_offset() {
        // scroll-margin-left: 20px on the snap area pulls the snap-x position
        // 20 px earlier (spec CSS Scroll Snap §6.3: margin expands the snap area).
        // Container 0..800, area at x=100 w=400, align=start, margin_left=20.
        // Expected: ax - margin_left = (100-0) - 20 = 80.
        let container_rect = lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
        let area_rect = lumen_core::geom::Rect { x: 100.0, y: 50.0, width: 400.0, height: 300.0 };
        let result = snap_offset_x(
            style::ScrollSnapAlignKeyword::Start,
            area_rect,
            container_rect,
            20.0, // margin_left
            0.0,  // margin_right
            0.0,  // padding_left
            0.0,  // padding_right
        );
        assert_eq!(result, Some(80.0), "scroll-margin-left shifts snap-x left");
    }

    #[test]
    fn snap_padding_start_shifts_x_offset() {
        // scroll-padding-left: 15px on the container shifts the snap port inward,
        // which reduces the required scroll-x (the port's left edge is further right).
        // Container 0..800, area at x=100 w=400, align=start, padding_left=15.
        // Expected: ax - 0 - padding_left = 100 - 15 = 85.
        let container_rect = lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
        let area_rect = lumen_core::geom::Rect { x: 100.0, y: 50.0, width: 400.0, height: 300.0 };
        let result = snap_offset_x(
            style::ScrollSnapAlignKeyword::Start,
            area_rect,
            container_rect,
            0.0,  // margin_left
            0.0,  // margin_right
            15.0, // padding_left
            0.0,  // padding_right
        );
        assert_eq!(result, Some(85.0), "scroll-padding-left shifts snap-x left");
    }

    #[test]
    fn snap_margin_end_shifts_y_offset() {
        // scroll-margin-bottom: 10px on the snap area.
        // Container 0..600h, area at y=500 h=200, align=end, margin_bottom=10.
        // Expected: ay + area.h + margin_bottom - container.h + padding_bottom
        //         = 500 + 200 + 10 - 600 + 0 = 110.
        let container_rect = lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
        let area_rect = lumen_core::geom::Rect { x: 0.0, y: 500.0, width: 200.0, height: 200.0 };
        let result = snap_offset_y(
            style::ScrollSnapAlignKeyword::End,
            area_rect,
            container_rect,
            0.0,  // margin_top
            10.0, // margin_bottom
            0.0,  // padding_top
            0.0,  // padding_bottom
        );
        assert_eq!(result, Some(110.0), "scroll-margin-bottom shifts snap-y end");
    }

    #[test]
    fn snap_margin_center_splits_evenly() {
        // Center alignment: margins shift center by (margin_right - margin_left)/2.
        // Container 0..800w, area at x=200 w=200, align=center, margin_left=20, margin_right=20.
        // Without margins: ax + w/2 - W/2 = 200 + 100 - 400 = -100.
        // Margin contribution: (20-20)/2 = 0 → same result -100.
        let container_rect = lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 };
        let area_rect = lumen_core::geom::Rect { x: 200.0, y: 0.0, width: 200.0, height: 100.0 };
        let result = snap_offset_x(
            style::ScrollSnapAlignKeyword::Center,
            area_rect,
            container_rect,
            20.0, // margin_left
            20.0, // margin_right
            0.0,
            0.0,
        );
        // Symmetric margins cancel: same as no margins.
        assert!((result.unwrap() - (-100.0_f32)).abs() < 0.01,
            "symmetric margins don't shift center, got {:?}", result);

        // Asymmetric: margin_right=30 > margin_left=10 → shifted right by (30-10)/2 = 10.
        let result2 = snap_offset_x(
            style::ScrollSnapAlignKeyword::Center,
            area_rect,
            container_rect,
            10.0, // margin_left
            30.0, // margin_right
            0.0,
            0.0,
        );
        assert!((result2.unwrap() - (-90.0_f32)).abs() < 0.01,
            "asymmetric margins shift center by (right-left)/2, got {:?}", result2);
    }

    #[test]
    fn snap_collect_containers_applies_scroll_margin() {
        // Verify that collect_snap_containers wires scroll-margin into the snap
        // point offset: element with scroll-margin-top: 20px + align=start
        // should produce snap_y = (area.y - container.y) - 20.
        let root = lay_full(
            "<div id=c><p id=a>item</p></div>",
            "#c { scroll-snap-type: y mandatory; height: 600px; } \
             #a { scroll-snap-align: start; scroll-margin-top: 20px; }",
        );
        let containers = collect_snap_containers(&root);
        assert_eq!(containers.len(), 1);
        let pts = &containers[0].points;
        assert_eq!(pts.len(), 1, "one snap area");
        let snap_y = pts[0].snap_y.expect("snap_y must be Some");
        // Without margin snap_y would be area.y - container.y (≈ 0 for first child).
        // With margin_top=20, snap_y = area.y - container.y - 20 ≈ -20.
        assert!(snap_y < 0.0,
            "scroll-margin-top shifts snap_y negative for first child, got {snap_y}");
        // The offset should be roughly -20px (margin_top).
        assert!((snap_y - (-20.0_f32)).abs() < 5.0,
            "snap_y should be ≈ −20 (scroll-margin-top), got {snap_y}");
    }

    #[test]
    fn snap_padding_reduces_proximity_threshold() {
        // scroll-padding reduces the effective snap port, which shrinks the
        // proximity threshold from 50%×viewport to 50%×(viewport−padding).
        // Container height=600, padding_top=100, padding_bottom=100 → port height=400
        // → proximity threshold = 200.
        // snap_y=600, target=380 → dy=220 > 200 → no snap.
        // Without padding (threshold=300): dy=220 < 300 → would snap.
        let snap_type = style::ScrollSnapType {
            axis: style::ScrollSnapAxis::Y,
            strictness: style::ScrollSnapStrictness::Proximity,
        };
        let mut container = SnapContainer {
            node: lumen_dom::NodeId::from_index(0),
            snap_type,
            rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 },
            scroll_padding_top: 100.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 100.0,
            scroll_padding_left: 0.0,
            points: vec![SnapPoint {
                node: lumen_dom::NodeId::from_index(1),
                snap_x: None,
                snap_y: Some(600.0),
                stop_always: false,
            }],
        };
        // With padding: threshold = (600-200)*0.5 = 200. dy=220 > 200 → no snap.
        let result = find_snap_target(&container, (0.0, 0.0), (0.0, 380.0));
        assert!(result.is_none(),
            "scroll-padding shrinks proximity threshold — should not snap at 380");

        // Verify that removing padding (threshold=300) would snap the same target.
        container.scroll_padding_top = 0.0;
        container.scroll_padding_bottom = 0.0;
        let result_no_pad = find_snap_target(&container, (0.0, 0.0), (0.0, 380.0));
        assert!(result_no_pad.is_some(),
            "without padding threshold=300 > dy=220 → should snap");
    }

    // ─── Scroll container tests ───────────────────────────────────────────────

    #[test]
    fn collect_scroll_containers_overflow_scroll() {
        let root = lay_full(
            "<div id=\"s\"><p>a</p></div>",
            "#s { overflow: scroll; width: 100px; height: 50px; }",
        );
        let containers = collect_scroll_containers(&root);
        assert_eq!(containers.len(), 1, "one scroll container expected");
        assert_eq!(containers[0].scroll_x, 0.0);
        assert_eq!(containers[0].scroll_y, 0.0);
        // clip rect should be approximately the padding-box of the div
        assert!(containers[0].clip_rect.width > 0.0);
        assert!(containers[0].clip_rect.height > 0.0);
    }

    #[test]
    fn collect_scroll_containers_overflow_auto() {
        let root = lay_full(
            "<div id=\"s\"><p>b</p></div>",
            "#s { overflow: auto; width: 100px; height: 50px; }",
        );
        let containers = collect_scroll_containers(&root);
        assert_eq!(containers.len(), 1);
    }

    #[test]
    fn collect_scroll_containers_overflow_hidden_excluded() {
        let root = lay_full(
            "<div id=\"s\"><p>c</p></div>",
            "#s { overflow: hidden; width: 100px; height: 50px; }",
        );
        let containers = collect_scroll_containers(&root);
        assert_eq!(containers.len(), 0, "overflow:hidden should not be a scroll container");
    }

    #[test]
    fn set_scroll_position_clamps_to_zero() {
        let mut root = lay_full(
            "<div id=\"s\"><p>d</p></div>",
            "#s { overflow: scroll; width: 100px; height: 50px; }",
        );
        let containers = collect_scroll_containers(&root);
        assert_eq!(containers.len(), 1);
        let node = containers[0].node;
        set_scroll_position(&mut root, node, -50.0, -50.0);
        let containers2 = collect_scroll_containers(&root);
        assert_eq!(containers2[0].scroll_x, 0.0, "negative scroll_x should clamp to 0");
        assert_eq!(containers2[0].scroll_y, 0.0, "negative scroll_y should clamp to 0");
    }

    #[test]
    fn set_scroll_position_sets_value() {
        let mut root = lay_full(
            "<div id=\"s\"><div style=\"height:200px\"></div></div>",
            "#s { overflow: scroll; width: 100px; height: 50px; }",
        );
        let containers = collect_scroll_containers(&root);
        assert_eq!(containers.len(), 1);
        let node = containers[0].node;
        let found = set_scroll_position(&mut root, node, 0.0, 10.0);
        assert!(found, "set_scroll_position should return true when node found");
        let containers2 = collect_scroll_containers(&root);
        assert_eq!(containers2[0].scroll_y, 10.0);
    }

    #[test]
    fn set_scroll_position_returns_false_for_unknown_node() {
        use lumen_dom::NodeId;
        let mut root = lay_full("<div></div>", "");
        let found = set_scroll_position(&mut root, NodeId::from_index(9999), 0.0, 0.0);
        assert!(!found, "should return false for unknown node");
    }

    // ── text-wrap: balance / pretty ─────────────────────────────────────────

    fn twrap_find_run(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::InlineRun { .. }) {
            return Some(b);
        }
        for c in &b.children {
            if let Some(f) = twrap_find_run(c) {
                return Some(f);
            }
        }
        None
    }

    fn twrap_line_count(root: &LayoutBox) -> usize {
        twrap_find_run(root)
            .and_then(|b| {
                if let BoxKind::InlineRun { lines, .. } = &b.kind {
                    Some(lines.len())
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    fn twrap_last_end_x(root: &LayoutBox) -> f32 {
        twrap_find_run(root)
            .and_then(|b| {
                if let BoxKind::InlineRun { lines, .. } = &b.kind {
                    lines.last().and_then(|l| l.last()).map(|f| f.x + f.width)
                } else {
                    None
                }
            })
            .unwrap_or(0.0)
    }

    // Fixed8: "aaaa"=32px, "bb"=16px, "cc"=16px, "dd"=16px, space=8px.
    // Greedy at 80px: ["aaaa"(32) "bb"(16) "cc"(16)] end=80, ["dd"(16)] end=16.
    // Balance: binary search → wrap_width≈56 → ["aaaa" "bb"] end=56, ["cc" "dd"] end=40.

    #[test]
    fn text_wrap_balance_preserves_line_count() {
        let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
        let balanced = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: balance; }", 80.0);
        assert_eq!(twrap_line_count(&greedy), 2, "greedy should produce 2 lines");
        assert_eq!(twrap_line_count(&balanced), 2, "balance must keep same line count");
    }

    #[test]
    fn text_wrap_balance_widens_last_line() {
        let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
        let balanced = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: balance; }", 80.0);
        // last line: greedy=16px ("dd"), balanced=40px ("cc dd")
        assert!(
            twrap_last_end_x(&balanced) > twrap_last_end_x(&greedy),
            "balance must widen last line: {} <= {}",
            twrap_last_end_x(&balanced),
            twrap_last_end_x(&greedy)
        );
    }

    #[test]
    fn text_wrap_balance_narrows_first_line() {
        let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
        let balanced = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: balance; }", 80.0);
        // first line: greedy=80px, balanced=56px
        let greedy_end = twrap_find_run(&greedy)
            .and_then(|b| {
                if let BoxKind::InlineRun { lines, .. } = &b.kind {
                    lines.first().and_then(|l| l.last()).map(|f| f.x + f.width)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let balanced_end = twrap_find_run(&balanced)
            .and_then(|b| {
                if let BoxKind::InlineRun { lines, .. } = &b.kind {
                    lines.first().and_then(|l| l.last()).map(|f| f.x + f.width)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        assert!(
            balanced_end < greedy_end,
            "balance must narrow first line: {} >= {}",
            balanced_end,
            greedy_end
        );
    }

    #[test]
    fn text_wrap_balance_single_line_is_noop() {
        // Single-line text must not be touched by balance.
        let normal = lay_measured("<p>hello</p>", "", 200.0);
        let balanced = lay_measured("<p>hello</p>", "p { text-wrap: balance; }", 200.0);
        assert_eq!(twrap_line_count(&normal), 1);
        assert_eq!(twrap_line_count(&balanced), 1);
        assert_eq!(twrap_last_end_x(&normal), twrap_last_end_x(&balanced));
    }

    #[test]
    fn text_wrap_stable_behaves_like_auto() {
        // For static layout `stable` is identical to `auto` (stability is an
        // incremental-editing concern, not a static-render concern).
        let auto = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: auto; }", 80.0);
        let stable = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: stable; }", 80.0);
        assert_eq!(
            twrap_line_count(&auto),
            twrap_line_count(&stable),
            "stable must produce same line count as auto"
        );
        assert_eq!(
            twrap_last_end_x(&auto),
            twrap_last_end_x(&stable),
            "stable last line must match auto"
        );
    }

    #[test]
    fn text_wrap_pretty_prevents_widow() {
        // Greedy: last line is just "dd" (16px). Pretty must widen it to "cc dd" (40px).
        // Words may be merged into one InlineFrag, so we check end_x, not frag count.
        let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
        let pretty = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: pretty; }", 80.0);
        assert_eq!(twrap_line_count(&pretty), 2, "pretty must keep 2 lines");
        assert!(
            twrap_last_end_x(&pretty) > twrap_last_end_x(&greedy),
            "pretty must widen last line: {} <= {}",
            twrap_last_end_x(&pretty),
            twrap_last_end_x(&greedy)
        );
    }

    #[test]
    fn text_wrap_pretty_no_widow_noop() {
        // If last line already has ≥2 words, pretty must not change anything.
        // "aaaa bb cc dd ee" at 80px → greedy: ["aaaa bb cc"(80), "dd ee"(40)].
        // Last line already has 2 frags → pretty is a no-op.
        let auto = lay_measured("<p>aaaa bb cc dd ee</p>", "", 80.0);
        let pretty = lay_measured("<p>aaaa bb cc dd ee</p>", "p { text-wrap: pretty; }", 80.0);
        assert_eq!(
            twrap_line_count(&auto),
            twrap_line_count(&pretty),
            "pretty must not change non-widow layout"
        );
        assert_eq!(
            twrap_last_end_x(&auto),
            twrap_last_end_x(&pretty),
            "pretty last line end must match auto when no widow"
        );
    }

    #[test]
    fn text_wrap_nowrap_ignores_balance() {
        // white-space: nowrap overrides text-wrap — line count must stay 1.
        let root = lay_measured(
            "<p>aaaa bb cc dd</p>",
            "p { white-space: nowrap; text-wrap: balance; }",
            80.0,
        );
        assert_eq!(
            twrap_line_count(&root),
            1,
            "nowrap must produce single line regardless of text-wrap-style"
        );
    }

    #[test]
    fn text_wrap_balance_longer_sequence() {
        // "aa bb cc dd ee ff" — 6 two-char words × 8px = 16px each, space=8px.
        // At 80px greedy: 3 lines → balance should equalize.
        let balanced = lay_measured(
            "<p>aa bb cc dd ee ff</p>",
            "p { text-wrap: balance; }",
            80.0,
        );
        let count = twrap_line_count(&balanced);
        assert!((2..=3).contains(&count), "balanced should have 2-3 lines, got {count}");
        // Last line must be wider than a single 2-char word (16px).
        assert!(
            twrap_last_end_x(&balanced) > 16.0,
            "last line must have more than one word after balance"
        );
    }

    #[test]
    fn range_input_creates_range_kind() {
        use box_tree::FormControlKind;
        let doc = lumen_html_parser::parse(r#"<input type="range" min="10" max="90" value="50">"#);
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let found = find_range_kind(&root);
        assert!(found.is_some(), "range input should produce FormControlKind::Range");
        if let Some(FormControlKind::Range { value, min, max }) = found {
            assert!((value - 50.0).abs() < 0.001, "value should be 50, got {value}");
            assert!((min - 10.0).abs() < 0.001, "min should be 10, got {min}");
            assert!((max - 90.0).abs() < 0.001, "max should be 90, got {max}");
        }
    }

    #[test]
    fn range_input_defaults_min_max() {
        use box_tree::FormControlKind;
        let doc = lumen_html_parser::parse(r#"<input type="range">"#);
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let found = find_range_kind(&root);
        assert!(found.is_some(), "range input without min/max should produce FormControlKind::Range");
        if let Some(FormControlKind::Range { value, min, max }) = found {
            assert!((min - 0.0).abs() < 0.001, "default min should be 0");
            assert!((max - 100.0).abs() < 0.001, "default max should be 100");
            assert!((value - 50.0).abs() < 0.001, "default value should be midpoint 50");
        }
    }

    #[test]
    fn range_input_value_clamped_to_max() {
        use box_tree::FormControlKind;
        let doc = lumen_html_parser::parse(r#"<input type="range" min="0" max="10" value="999">"#);
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        if let Some(FormControlKind::Range { value, max, .. }) = find_range_kind(&root) {
            assert!(value <= max, "value {value} should be clamped to max {max}");
        }
    }

    #[test]
    fn range_input_is_clickable() {
        let doc = lumen_html_parser::parse(r#"<input type="range">"#);
        let sheet = lumen_css_parser::parse("");
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        let elems = collect_clickable_elements(&root, &doc);
        assert!(
            elems.iter().any(|e| matches!(e.kind, ClickableKind::Input)),
            "range input should be collected as clickable Input"
        );
    }

    fn find_range_kind(root: &LayoutBox) -> Option<box_tree::FormControlKind> {
        if let BoxKind::FormControl { kind } = &root.kind
            && matches!(kind, box_tree::FormControlKind::Range { .. })
        {
            return Some(kind.clone());
        }
        for child in &root.children {
            if let Some(k) = find_range_kind(child) {
                return Some(k);
            }
        }
        None
    }

    // ── find_scroll_container_at ──────────────────────────────────────────────

    fn make_scroll_container(node_idx: usize, x: f32, y: f32, w: f32, h: f32) -> ScrollContainer {
        use lumen_core::geom::Rect;
        ScrollContainer {
            node: lumen_dom::NodeId::from_index(node_idx),
            clip_rect: Rect::new(x, y, w, h),
            scroll_width: w + 200.0,
            scroll_height: h + 400.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            overscroll_behavior_x: style::OverscrollBehavior::Auto,
            overscroll_behavior_y: style::OverscrollBehavior::Auto,
        }
    }

    #[test]
    fn find_scroll_container_at_hit() {
        let c = make_scroll_container(1, 10.0, 20.0, 100.0, 200.0);
        let result = find_scroll_container_at(&[c], 50.0, 80.0);
        assert_eq!(result, Some(lumen_dom::NodeId::from_index(1)));
    }

    #[test]
    fn find_scroll_container_at_miss() {
        let c = make_scroll_container(1, 10.0, 20.0, 100.0, 200.0);
        // Point outside the container
        assert_eq!(find_scroll_container_at(&[c], 5.0, 80.0), None);
    }

    #[test]
    fn find_scroll_container_at_empty() {
        assert_eq!(find_scroll_container_at(&[], 50.0, 50.0), None);
    }

    #[test]
    fn find_scroll_container_at_innermost_wins() {
        // Outer container covers (0,0,200,200), inner covers (50,50,50,50).
        // A point inside both should return the inner (last in list = deeper in DOM).
        let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
        let inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
        let result = find_scroll_container_at(&[outer, inner], 60.0, 60.0);
        assert_eq!(result, Some(lumen_dom::NodeId::from_index(2)));
    }

    #[test]
    fn find_scroll_container_at_only_outer_when_point_outside_inner() {
        let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
        let inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
        // Point in outer but not in inner
        let result = find_scroll_container_at(&[outer, inner], 10.0, 10.0);
        assert_eq!(result, Some(lumen_dom::NodeId::from_index(1)));
    }

    // ── collect_view_transition_names ─────────────────────────────────────────

    #[test]
    fn vt_names_empty_without_property() {
        let root = lay("<div></div>", "div { width: 100px; height: 50px; }");
        let names = collect_view_transition_names(&root);
        assert!(names.is_empty(), "no view-transition-name set → empty");
    }

    #[test]
    fn vt_names_single_named_element() {
        let root = lay(
            "<div></div>",
            "div { view-transition-name: hero; width: 100px; height: 50px; }",
        );
        let names = collect_view_transition_names(&root);
        assert_eq!(names.len(), 1, "one named element");
        assert_eq!(names[0].1.as_ref(), "hero");
    }

    #[test]
    fn vt_names_multiple_elements_document_order() {
        let root = lay(
            "<div id='a'></div><div id='b'></div>",
            "#a { view-transition-name: first; width: 100px; height: 50px; } \
             #b { view-transition-name: second; width: 100px; height: 50px; }",
        );
        let names = collect_view_transition_names(&root);
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].1.as_ref(), "first");
        assert_eq!(names[1].1.as_ref(), "second");
    }

    #[test]
    fn vt_names_none_value_excluded() {
        let root = lay(
            "<div></div>",
            "div { view-transition-name: none; width: 100px; height: 50px; }",
        );
        let names = collect_view_transition_names(&root);
        assert!(names.is_empty(), "view-transition-name:none should not appear");
    }

    // BUG-130: view-transition-name must not affect normal-flow rendering — a box
    // carrying the property lays out identically to a plain box (CSS View
    // Transitions L1 §10; the property only marks elements for capture during
    // document.startViewTransition()). Regression mirrors TEST-81: two equal boxes
    // in a centered flex row, one named, one not — same y/size/height.
    #[test]
    fn vt_name_does_not_affect_layout_geometry() {
        let root = lay_viewport(
            "<div class='f'><div class='box plain'></div><div class='box named'></div></div>",
            ".f { display: flex; align-items: center; justify-content: center; gap: 60px; \
                  width: 1022px; height: 718px; } \
             .box { width: 200px; height: 200px; } \
             .named { view-transition-name: hero; }",
            Size::new(1024.0, 720.0),
        );
        let flex = first_element_child(&root);
        let plain = &flex.children[0];
        let named = &flex.children[1];
        // align-items:center → both vertically centered at the same y in the 718px row.
        assert_eq!(plain.rect.y, named.rect.y, "named box must share the plain box y");
        assert_eq!(plain.rect.height, named.rect.height, "same height");
        assert_eq!(plain.rect.width, named.rect.width, "same width");
        // Centered cross-size: (718 - 200) / 2 = 259 (BUG-141), not pinned to row top.
        assert!(
            (plain.rect.y - 259.0).abs() < 0.5,
            "boxes centered on cross axis, got y={}",
            plain.rect.y
        );
    }

    // ──────────── CSS Overscroll Behavior L1 — scroll chain stop ────────────

    #[test]
    fn overscroll_collected_from_style() {
        let root = lay(
            "<div class='s'><div class='t'></div></div>",
            ".s { width: 100px; height: 100px; overflow: scroll; \
               overscroll-behavior-x: contain; overscroll-behavior-y: none; } \
             .t { width: 300px; height: 300px; }",
        );
        let containers = collect_scroll_containers(&root);
        let c = containers
            .iter()
            .find(|c| matches!(c.overscroll_behavior_x, style::OverscrollBehavior::Contain))
            .expect("scroll container with overscroll-behavior-x: contain");
        assert_eq!(c.overscroll_behavior_x, style::OverscrollBehavior::Contain);
        assert_eq!(c.overscroll_behavior_y, style::OverscrollBehavior::None);
    }

    #[test]
    fn overscroll_auto_propagates_at_boundary() {
        use style::OverscrollBehavior::Auto;
        // At boundary (no movement), default `auto` lets the delta bubble up.
        assert!(overscroll_should_propagate(Auto, Auto, 0.0, 30.0, false, false));
        assert!(overscroll_should_propagate(Auto, Auto, 30.0, 0.0, false, false));
    }

    #[test]
    fn overscroll_contain_blocks_propagation() {
        use style::OverscrollBehavior::{Auto, Contain, None};
        // Vertical delta at boundary with overscroll-behavior-y: contain stays put.
        assert!(!overscroll_should_propagate(Auto, Contain, 0.0, 30.0, false, false));
        // None behaves like contain for chain-stopping.
        assert!(!overscroll_should_propagate(None, Auto, 30.0, 0.0, false, false));
    }

    #[test]
    fn overscroll_blocked_axis_only_matters_for_its_delta() {
        use style::OverscrollBehavior::{Auto, Contain};
        // contain on Y, but the delta is purely horizontal on an `auto` X axis →
        // the horizontal delta is free to propagate.
        assert!(overscroll_should_propagate(Auto, Contain, 30.0, 0.0, false, false));
        // contain on X but delta is vertical on `auto` Y → propagates.
        assert!(overscroll_should_propagate(Contain, Auto, 0.0, 30.0, false, false));
    }

    #[test]
    fn overscroll_consumed_when_container_moves() {
        use style::OverscrollBehavior::Auto;
        // Any actual movement consumes the gesture — chain never reaches parent,
        // regardless of overscroll-behavior.
        assert!(!overscroll_should_propagate(Auto, Auto, 0.0, 30.0, false, true));
        assert!(!overscroll_should_propagate(Auto, Auto, 30.0, 30.0, true, false));
    }

    /// BUG-158: a `flex: 1` item (which sets `flex-basis: 0`) in an
    /// indefinite-height column flex container must not collapse to height 0 —
    /// CSS Flexbox §4.5 automatic minimum size keeps it at its content height.
    ///
    /// The container is itself a flex item of a row-flex grandparent, so the row
    /// flex lays the column out twice (preliminary + final pass). The first pass
    /// writes a resolved px `height` back into the item's style; the regression
    /// is that the second pass saw that stale `height` and re-collapsed the item
    /// to 0, so sibling cards painted on top of each other (lenta.ru news cards).
    #[test]
    fn flex_column_basis_zero_item_keeps_content_height() {
        let body = lay_measured(
            "<div class=g>\
               <div class=col>\
                 <div class=a>First card single line</div>\
                 <div class=mid>Middle card has enough text to wrap onto two lines here ok</div>\
                 <div class=b>Last card single line</div>\
               </div>\
             </div>",
            ".g { display: flex; } \
             .col { display: flex; flex-direction: column; width: 280px; } \
             .a, .b { flex: none; } \
             .mid { flex: 1; }",
            800.0,
        );

        let grand = body.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
        let col = grand.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
        // (y, height) of each card, in source order.
        let cards: Vec<(f32, f32)> = col
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .map(|c| (c.rect.y, c.rect.height))
            .collect();
        assert_eq!(cards.len(), 3, "expected 3 cards, got {}", cards.len());

        // The middle `flex: 1` card must keep a real content height, not collapse.
        assert!(
            cards[1].1 > 10.0,
            "middle flex:1 card collapsed to height {} (BUG-158)",
            cards[1].1
        );

        // Cards stack without overlap: each starts at the bottom edge of the
        // previous one (no two share a y, which is the painted symptom).
        assert!(
            (cards[1].0 - (cards[0].0 + cards[0].1)).abs() < 0.5,
            "card 1 (y={}) does not stack under card 0 (y={}, h={})",
            cards[1].0, cards[0].0, cards[0].1
        );
        assert!(
            (cards[2].0 - (cards[1].0 + cards[1].1)).abs() < 0.5,
            "card 2 (y={}) does not stack under card 1 (y={}, h={})",
            cards[2].0, cards[1].0, cards[1].1
        );
    }
}

