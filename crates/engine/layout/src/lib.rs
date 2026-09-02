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

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

pub use lumen_core::ColorSpace;

pub mod anchor;
pub mod animation;
pub mod bidi;
pub mod box_tree;
pub mod color_mix;
pub mod incremental;
pub mod content_visibility;
pub mod field_sizing;
pub mod hyphenation;
pub mod counters;
mod invariants;
pub mod font_palette;
pub mod image_gating;
pub mod image_set;
pub mod line_break;
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
pub mod vertical;

pub use counters::{
    format_counter, format_counter_with_registry, precompute_counters,
    build_counter_style_registry, build_list_marker_text, resolve_counter_value,
    CascadeStyles, CounterMap, CounterSnapshot, CounterStyleDef, CounterStyleRegistry,
    CounterSystem, CounterRange, PrevCascade, QuoteSlot, RangeBound,
};
pub use color_mix::{HueInterpolationMethod, MixColorSpace, mix_colors, mix_colors_hue};
pub use field_sizing::field_sizing_content_intrinsic;
pub use hyphenation::{collect_hyphen_points, SoftHyphenPoint};
pub use image_gating::gate_image_requests;
pub use image_set::{
    parse_image_set, select_image_set_candidate, select_image_set_url,
    ImageSetOption, SupportedTypes,
};
pub use mathml::{
    MATH_SCRIPT_SCALE, MathStyle, MathmlBox, MathmlElementKind, collect_mathml_structure,
    lay_out_mathml, math_depth_scale,
};
pub use ruby::{RubyAlign, RubyBox, RubyMerge, RubyPosition, lay_out_ruby};
pub use animation::{
    AnimValue, AnimatedStyle, AnimationFrame, AnimationInterpolator,
    LinearInterpolator, NoopInterpolator, parse_keyframe_style, KeyframeStyle,
    CompositorAnimFrame, CompositorOverride,
    AnimationScheduler, TransitionScheduler,
};
pub use box_tree::{
    apply_container_styles, apply_intrinsic_size, build_iframe_document, canvas_background_color,
    collect_background_image_requests, collect_image_requests, is_open_details, layout, layout_measured,
    layout_measured_hyp, layout_measured_hyp_with_counters, layout_mutation_incremental,
    layout_streaming_incremental,
    lay_out_incremental, BoxKind, BoxOrigin, BoxRole, FormControlKind, ImageRequest, InlineFrag, InlineSegment, LayoutBox,
    PseudoKind, SvgMaskContent, SvgShapeKind, SvgTextAnchor, SvgDominantBaseline, SvgBaselineShift, ViewBox,
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
pub use style::{compute_selection_style, compute_style, compute_style_from_declarations};
pub use selector_query::{
    computed_style_by_selector, computed_style_json, computed_style_json_by_selector,
    computed_style_to_map, find_all_by_selector, find_box_by_selector, find_first_dom_node_by_selector,
    matched_rules_for_node, matches_selector, query_all, query_all_scoped, query_all_within,
    ComputedStyleSnapshot, MatchedRule,
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
pub use content_visibility::{
    cv_is_skipped, set_cv_scroll, set_cv_relevant, take_cv_skipped, CV_SLACK_FACTOR,
};
pub use invariants::{count_geometry_violations, GeometryViolationCounts};
pub use stacking::{
    box_can_own_stacking_context, creates_stacking_context, PaintOrder, PaintPhase,
    StackingContext, StackingContextId, StackingTree,
};
pub use style::{
    apply_container_rules, evaluate_container_condition,
    set_interactive_state, clear_interactive_state,
    set_forced_colors, forced_colors_active,
    set_print_media, print_media_active,
    parse_background_gradient, parse_color, parse_css_wide_keyword, parse_gradient_stops,
    parse_grid_template_areas, parse_transform_list,
    radial_gradient_radii, GradientCorner, RadialShape, RadialSize,
    AlignValue, AnimationDirection, Appearance, ContainerContext,
    AnimationFillMode, AnimationPlayState,
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin, BackgroundRepeat,
    BackgroundSize, BgSizeAxis, BorderCollapse, BorderStyle,
    BoxShadow, BoxSizing, BreakValue, CalcNode, ClipPath, Color, ColorFloat,
    BackfaceVisibility, ClearSide, ContainFlags, ComputedStyle, Content, CustomProps,
    ContentItem, CssColor, CssWideKeyword, Cursor, Direction, Display, EmptyCells, FilterFn, FloatSide, FontOpticalSizing, FontStretch,
    FontStyle,
    FontVariantCaps, FontVariationSetting, FontWeight, GradientStop, GridAutoFlow, GridLine, GridTrackSize, Hyphens, ImageRendering,
    MaskClip, MaskComposite, MaskLayer, MaskMode, MasonryAutoFlow,
    Isolation, IterationCount, Length,
    LengthOrAuto, ListStylePosition, ListStyleType, MixBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, OverflowWrap, OverscrollBehavior, ParsedGradient, Resize,
    PointerEvents,
    Position, PositionComponent, Quotes, ScrollBehavior, ScrollSnapAlign, ScrollSnapAlignKeyword,
    ScrollSnapAxis, ScrollSnapStop, ScrollSnapStrictness, ScrollSnapType, ScrollbarGutter,
    FillRule, ScrollbarWidth, ShapeValue, StepPosition, StrokeLinecap, StrokeLinejoin,
    SvgGradientDef, SvgGradientUnits, SvgPaint, TextAlign, TextDecorationLine, TextDecorationStyle,
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

    // The UA rule `[inert] { pointer-events: none; }` is applied by
    // `apply_ua_inert` in style.rs. This guard provides the complementary
    // layout-level filter: inert elements are never included in the clickable
    // set regardless of an author `pointer-events` override.
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
        {
            let p = doc.get(id).parent?;
            id = p
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

fn collect_sticky_rec(
    b: &LayoutBox,
    containing_rect: lumen_core::geom::Rect,
    viewport: lumen_core::geom::Size,
    seen: &mut std::collections::HashSet<lumen_dom::NodeId>,
    out: &mut Vec<StickyBox>,
) {
    use box_tree::BoxKind;
    use style::Position;

    if matches!(b.kind, BoxKind::Skip) {
        return;
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

    for child in &b.children {
        collect_sticky_rec(child, next_cb, viewport, seen, out);
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
///
/// Text nodes are in the map too, but with a much smaller entry — see
/// [`INLINE_SEGMENT_PROPERTIES`] for which properties and why.
pub fn collect_computed_styles(
    root: &LayoutBox,
) -> std::collections::HashMap<u32, std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    collect_computed_styles_rec(root, &mut out);
    out
}

/// The properties published for a text node that ended up inside an inline run.
///
/// An inline element owns no [`LayoutBox`] — its content is flattened into the
/// enclosing `InlineRun`'s segments — so `<b>`, `<span>` and friends have no
/// entry in this map at all, and neither does anything inside a `display: none`
/// subtree. That makes the *text node* the only place where «this text was laid
/// out, with this style» can be observed, which is exactly what
/// `HTMLElement.innerText` needs ([BUG-413](../../../bugs/BUG-413-FIXED.md)):
/// the presence of the entry answers «is it rendered» and the three properties
/// are the ones the HTML LS §3.2.7 collection steps consult.
///
/// Deliberately not the full [`computed_style_to_map`] set: a text-heavy page
/// has as many text nodes as boxes, and republishing ~55 serialised properties
/// for each of them after every relayout would roughly double the cost of a
/// snapshot that is already rebuilt on the layout path.
pub const INLINE_SEGMENT_PROPERTIES: [&str; 3] = ["visibility", "white-space", "text-transform"];

fn collect_computed_styles_rec(
    b: &LayoutBox,
    out: &mut std::collections::HashMap<u32, std::collections::HashMap<String, String>>,
) {
    // First box in tree order wins — see `collect_layout_rects_rec` for why
    // several boxes can carry the same `NodeId`.
    out.entry(b.node.index() as u32)
        .or_insert_with(|| computed_style_to_map(&b.style));
    if let box_tree::BoxKind::InlineRun { segments, .. } = &b.kind {
        for seg in segments {
            // `NodeId(0)` is the document root, which `InlineSegment::source_node`
            // uses for generated content with no DOM origin.
            if seg.source_node.index() == 0 {
                continue;
            }
            out.entry(seg.source_node.index() as u32)
                .or_insert_with(|| selector_query::inline_segment_style_map(&seg.style));
        }
    }
    for child in &b.children {
        collect_computed_styles_rec(child, out);
    }
}

// ──────────────── collect_custom_properties ────────────────

/// Walks the layout tree and returns a map of `NodeId index → resolved custom
/// properties` (CSS Variables L1 §3), keys carrying their `--` prefix.
///
/// Values are the *computed* ones: every `var()` / `env()` reference is
/// substituted against the same node's map, so a chain like
/// `--base: 8px; --gap: var(--base)` publishes `--gap: 8px`. A reference that
/// cannot be resolved makes the property guaranteed-invalid, which
/// `getPropertyValue()` reports as the empty string — that is what the empty
/// value in the map means.
///
/// Published alongside — not inside — [`collect_computed_styles`], and behind
/// an `Arc`, on purpose: custom properties inherit, so one `:root`-declared set
/// is a single copy-on-write allocation shared by every node in the document
/// ([`crate::style::CustomProps`]). Folding them into the per-node standard
/// property map would re-materialise every variable on every node and multiply
/// the cost of the snapshot the shell rebuilds after each relayout by the
/// number of declared variables — on a design-system page (hundreds of
/// `:root` variables, thousands of nodes) that is the whole snapshot again,
/// several times over. Here each *distinct* map is resolved exactly once and
/// every node inheriting it gets the same `Arc` (BUG-732).
pub fn collect_custom_properties(
    root: &LayoutBox,
) -> std::collections::HashMap<u32, std::sync::Arc<std::collections::HashMap<String, String>>> {
    let mut out = std::collections::HashMap::new();
    // Keyed by the address of the source allocation — identity, never read.
    let mut resolved: std::collections::HashMap<
        usize,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    > = std::collections::HashMap::new();
    collect_custom_properties_rec(root, &mut out, &mut resolved);
    out
}

fn collect_custom_properties_rec(
    b: &LayoutBox,
    out: &mut std::collections::HashMap<
        u32,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    >,
    resolved: &mut std::collections::HashMap<
        usize,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    >,
) {
    // First box in tree order wins — see `collect_layout_rects_rec` for why
    // several boxes can carry the same `NodeId`.
    if !b.style.custom_props.is_empty() {
        let key = b.style.custom_props.as_ptr() as usize;
        let map = match resolved.get(&key) {
            Some(m) => std::sync::Arc::clone(m),
            None => {
                let raw = b.style.custom_props.shared();
                let m = std::sync::Arc::new(
                    raw.iter()
                        .map(|(name, value)| {
                            let computed =
                                crate::style::expand_vars_and_env(value, &raw).unwrap_or_default();
                            (name.clone(), computed.trim().to_string())
                        })
                        .collect::<std::collections::HashMap<String, String>>(),
                );
                resolved.insert(key, std::sync::Arc::clone(&m));
                m
            }
        };
        out.entry(b.node.index() as u32).or_insert(map);
    }
    for child in &b.children {
        collect_custom_properties_rec(child, out, resolved);
    }
}

// ──────────────── collect_layout_rects ────────────────

/// Walks the layout tree and returns a map of `NodeId index → [x, y, width, height]`
/// (border-box, viewport-relative CSS px).
///
/// The geometry counterpart of [`collect_computed_styles`]: embedders push the
/// result into the JS runtime so that `getBoundingClientRect`, `ResizeObserver`
/// and `IntersectionObserver` can answer without a round-trip to the layout
/// engine. Both maps must be published together and from every path that
/// produces a layout tree — publishing from the relayout path alone left a
/// freshly loaded page answering `""` / all-zeros (BUG-382).
pub fn collect_layout_rects(root: &LayoutBox) -> std::collections::HashMap<u32, [f32; 4]> {
    let mut out = std::collections::HashMap::new();
    collect_layout_rects_rec(root, &mut out);
    out
}

fn collect_layout_rects_rec(b: &LayoutBox, out: &mut std::collections::HashMap<u32, [f32; 4]>) {
    // A single `NodeId` can own more than one box: an element with inline content
    // gets an anonymous block/line box for that content, and the box tree keeps the
    // element's own `NodeId` on it. The recursion visits the principal box before
    // its descendants, so `or_insert` keeps the element's own border box; plain
    // `insert` used to hand JS the last (inner) box instead — `getBoundingClientRect`
    // on `<div style="height:20px">x</div>` answered the 19.2px line box (BUG-382).
    let r = &b.rect;
    out.entry(b.node.index() as u32)
        .or_insert([r.x, r.y, r.width, r.height]);
    for child in &b.children {
        collect_layout_rects_rec(child, out);
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
/// Находит layout-бокс по DOM-узлу (первое совпадение в порядке дерева).
///
/// Используется шеллом для точечных операций над конкретным боксом —
/// например, быстрый патч скролл-слоя в display list без полной пересборки
/// (`lumen_paint::patch_scroll_layer`).
pub fn find_box_by_node(root: &LayoutBox, node: lumen_dom::NodeId) -> Option<&LayoutBox> {
    if root.node == node {
        return Some(root);
    }
    root.children.iter().find_map(|c| find_box_by_node(c, node))
}

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

/// CSS View Transitions L1 §4 — collect each `view-transition-name` element with
/// the geometry the morph engine animates from/to.
///
/// Like [`collect_view_transition_names`] but additionally returns the element's
/// **border-box rectangle** (document-relative CSS px; includes padding + border,
/// excludes margin — see [`box_tree::LayoutBox::rect`]). One entry per named
/// element in document order; `display: none` boxes (no layout) are skipped.
///
/// Per the spec, a `view-transition-name` must be unique on a page: when a name
/// repeats, only the **first** occurrence is a valid capture source. This
/// collector returns every occurrence as-is (mirroring
/// [`collect_view_transition_names`]); the shell deduplicates by name, keeping
/// the first, when it pairs old↔new snapshots.
// CSS: ::view-transition / ::view-transition-group(name) / -image-pair / -old / -new —
// P4 to add PseudoElementKind variants + functional-pseudo parsing
// (css-parser/src/parser.rs:345) so author animation-duration /
// animation-timing-function on these pseudos can override the morph's hardcoded
// 300 ms per group. Until then the shell uses the default duration for every group.
pub fn collect_view_transition_groups(
    root: &LayoutBox,
) -> Vec<(lumen_dom::NodeId, Box<str>, lumen_core::geom::Rect)> {
    let mut out = Vec::new();
    collect_vt_groups_rec(root, &mut out);
    out
}

fn collect_vt_groups_rec(
    b: &LayoutBox,
    out: &mut Vec<(lumen_dom::NodeId, Box<str>, lumen_core::geom::Rect)>,
) {
    use box_tree::BoxKind;
    if matches!(b.kind, BoxKind::Skip) {
        return;
    }
    if let Some(ref name) = b.style.view_transition_name {
        out.push((b.node, name.clone(), b.rect));
    }
    for child in &b.children {
        collect_vt_groups_rec(child, out);
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

/// Find the nearest scrolling ancestor of `node` (inclusive of `node` itself),
/// walking up the DOM parent chain.
///
/// BUG-338: mirrors [`find_scroll_container_at`] but resolves by node identity
/// instead of a hit-tested point — used by automation surfaces (MCP `scroll`
/// `target`, fragment navigation) that already know which element they want
/// scrolled rather than a screen coordinate under a cursor.
pub fn find_scroll_container_for_node(
    containers: &[ScrollContainer],
    doc: &lumen_dom::Document,
    node: lumen_dom::NodeId,
) -> Option<lumen_dom::NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if containers.iter().any(|c| c.node == n) {
            return Some(n);
        }
        cur = doc.get(n).parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{compute_style, VerticalAlign};
    use lumen_core::geom::Size;
    use table_grid_presentational::html_and_body;

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

    /// UA `body { margin: 8px }` (HTML Rendering §14.3.3, BUG-204) shifts all
    /// normal-flow content by 8px. These layout-unit helpers test child geometry
    /// in isolation, so they neutralise the UA body margin with an explicit reset
    /// — exactly as real pages do via `* { margin: 0 }`. Tests that specifically
    /// verify the UA body margin use the `cascade_at` style helper instead.
    const BODY_RESET: &str = "body{margin:0}";

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
        body_layout_box(layout(&doc, &sheet, Size::new(800.0, 600.0)))
    }

    fn lay_viewport(html: &str, css: &str, vp: Size) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
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
        let sheet = lumen_css_parser::parse(&format!("{BODY_RESET}{css}"));
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

    /// BUG-382: an element with inline content owns two boxes with the same
    /// `NodeId` — its principal block box and the anonymous run holding the text.
    /// Both snapshot collectors must answer with the principal one; the earlier
    /// `insert` handed JS the inner run, so `getBoundingClientRect().height` was
    /// the line height and `getComputedStyle().width` was `auto`.
    #[test]
    fn snapshot_collectors_keep_the_principal_box() {
        let root = lay_full(
            "<html><body><div id=a>x</div></body></html>",
            "body{margin:0} #a{width:50px;height:20px}",
        );
        // Walk in the same order the collectors do, recording every box per node.
        fn walk<'a>(b: &'a LayoutBox, out: &mut Vec<(u32, &'a LayoutBox)>) {
            out.push((b.node.index() as u32, b));
            for c in &b.children {
                walk(c, out);
            }
        }
        let mut boxes = Vec::new();
        walk(&root, &mut boxes);

        let div_nid = boxes
            .iter()
            .find(|(nid, _)| boxes.iter().filter(|(n, _)| n == nid).count() > 1)
            .map(|(nid, _)| *nid)
            .expect("expected a NodeId owning more than one box");
        let principal = boxes
            .iter()
            .find(|(nid, _)| *nid == div_nid)
            .map(|(_, b)| *b)
            .expect("principal box");
        assert_eq!(principal.rect.height, 20.0, "specified height must win");

        let rects = collect_layout_rects(&root);
        assert_eq!(
            rects[&div_nid],
            [
                principal.rect.x,
                principal.rect.y,
                principal.rect.width,
                principal.rect.height
            ],
            "rect snapshot must describe the element's own box"
        );

        let styles = collect_computed_styles(&root);
        assert_eq!(
            styles[&div_nid].get("width").map(String::as_str),
            Some("50px"),
            "style snapshot must describe the element's own box"
        );
    }

    /// BUG-732: `getComputedStyle(el).getPropertyValue('--x')` answered `""`
    /// because nothing published custom properties at all. The snapshot carries
    /// *computed* values — `var()` chains substituted, an unresolvable
    /// reference reported as the empty string (guaranteed-invalid).
    #[test]
    fn custom_property_snapshot_resolves_var_chains() {
        let root = lay_full(
            "<html><body><div id=a>x</div></body></html>",
            ":root{--base:8px;--gap:var(--base)} #a{--own:2px;--broken:var(--nope)}",
        );
        let props = collect_custom_properties(&root);
        let div_nid = {
            fn find(b: &LayoutBox, out: &mut Option<u32>) {
                if b.style.custom_props.contains_key("--own") && out.is_none() {
                    *out = Some(b.node.index() as u32);
                }
                for c in &b.children {
                    find(c, out);
                }
            }
            let mut found = None;
            find(&root, &mut found);
            found.expect("the div declaring --own must own a box")
        };
        let div = &props[&div_nid];
        assert_eq!(div.get("--own").map(String::as_str), Some("2px"));
        assert_eq!(
            div.get("--gap").map(String::as_str),
            Some("8px"),
            "inherited var() chain must be substituted, not published raw"
        );
        assert_eq!(
            div.get("--broken").map(String::as_str),
            Some(""),
            "an unresolvable var() is guaranteed-invalid, i.e. the empty string"
        );
    }

    /// The whole point of the separate, `Arc`-shared snapshot: nodes that only
    /// inherit a set of variables must share one allocation with the node that
    /// declared it, not carry a copy each (see `collect_custom_properties`).
    #[test]
    fn custom_property_snapshot_shares_one_allocation_per_distinct_map() {
        let root = lay_full(
            "<html><body><div id=a><p>x</p></div></body></html>",
            ":root{--base:8px}",
        );
        let props = collect_custom_properties(&root);
        assert!(props.len() > 1, "every node inherits --base");
        let mut maps = props.values();
        let first = maps.next().expect("at least one node");
        for other in maps {
            assert!(
                std::sync::Arc::ptr_eq(first, other),
                "one declared set must stay one allocation across the document"
            );
        }
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

    #[cfg(test)]
    #[path = "state_pseudo_classes.rs"]
    mod state_pseudo_classes;
    #[cfg(test)]
    pub(crate) use state_pseudo_classes::element_color;

    #[cfg(test)]
    #[path = "box_sizing_text_props.rs"]
    mod box_sizing_text_props;
    #[cfg(test)]
    pub(crate) use box_sizing_text_props::{first_inline_run, first_inline_text};

    #[cfg(test)]
    #[path = "visual_props_pseudo_selectors.rs"]
    mod visual_props_pseudo_selectors;
    #[cfg(test)]
    pub(crate) use visual_props_pseudo_selectors::{style_ctx, style_ctx_with_style_props};

    #[cfg(test)]
    #[path = "container_queries_replaced.rs"]
    mod container_queries_replaced;
    #[cfg(test)]
    pub(crate) use container_queries_replaced::{first_image_child, lay_with_viewport, nested_p_style};



    #[cfg(test)]
    #[path = "filter_transform_snap_mask.rs"]
    mod filter_transform_snap_mask;
    #[cfg(test)]
    pub(crate) use filter_transform_snap_mask::first_p_style;

    #[cfg(test)]
    #[path = "animation_gradient_quirks.rs"]
    mod animation_gradient_quirks;

    #[cfg(test)]
    #[path = "table_grid_presentational.rs"]
    mod table_grid_presentational;

    #[cfg(test)]
    #[path = "layout_generation_misc.rs"]
    mod layout_generation_misc;

    #[cfg(test)]
    #[path = "scroll_interaction_misc.rs"]
    mod scroll_interaction_misc;
}
