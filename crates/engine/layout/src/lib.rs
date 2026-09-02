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

/// DOM element ancestors of an `InlineSegment`/`InlineFrag::source_node` that the
/// box tree never gives their own `LayoutBox` (BUG-488): plain inline elements
/// (`<span>`, `<em>`, …) are flattened into the enclosing `InlineRun`'s segments,
/// so `collect_computed_styles`/`collect_layout_rects` — which key their output by
/// walking `LayoutBox.node`, not DOM structure — never see the element's own
/// `NodeId` at all.
///
/// Returns every element strictly between `source_node` and `stop_at` (both
/// exclusive), innermost first, plus `source_node` itself when it names an
/// element rather than a text node (`content_to_inline_segments`'s generated-text
/// segments use `source_node = owner_id`, the element being styled — its own
/// generated content must count toward its own box). `stop_at` is normally the
/// `InlineRun`'s own `node` (the containing block/inline-block), which already
/// gets an entry from the ordinary per-`LayoutBox` walk.
fn inline_element_ancestors(
    doc: &lumen_dom::Document,
    source_node: lumen_dom::NodeId,
    stop_at: lumen_dom::NodeId,
) -> Vec<lumen_dom::NodeId> {
    let mut out = Vec::new();
    let mut cur = if matches!(doc.get(source_node).data, lumen_dom::NodeData::Element { .. }) {
        source_node
    } else {
        match doc.get(source_node).parent {
            Some(p) => p,
            None => return out,
        }
    };
    while cur != stop_at {
        out.push(cur);
        cur = match doc.get(cur).parent {
            Some(p) => p,
            None => break,
        };
    }
    out
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
///
/// Plain inline elements (`<span>`, `<em>`, …) are in the map too (BUG-488):
/// they own no `LayoutBox` of their own, so their entry is approximated from
/// the nearest descendant `InlineSegment`'s already-cascaded style via
/// [`inline_element_ancestors`] — exact for every inherited property unless an
/// element *between* the element and that segment overrides it, and exact for
/// `display` (always `inline`, or the flattening in `collect_inline_segments`
/// would not have happened).
pub fn collect_computed_styles(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
) -> std::collections::HashMap<u32, std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    collect_computed_styles_rec(doc, root, &mut out);
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
    doc: &lumen_dom::Document,
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
            // BUG-488: publish the full property map for every plain inline
            // element this segment is nested inside — see `collect_computed_styles`'s
            // doc comment for the approximation this relies on.
            for anc in inline_element_ancestors(doc, seg.source_node, b.node) {
                out.entry(anc.index() as u32)
                    .or_insert_with(|| computed_style_to_map(&seg.style));
            }
        }
    }
    for child in &b.children {
        collect_computed_styles_rec(doc, child, out);
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
///
/// Plain inline elements (`<span>`, `<em>`, …) are in the map too (BUG-488):
/// their rect is the union of every laid-out `InlineFrag` nested inside them
/// (line y-position via the same `font_size * line_height` uniform-line-height
/// model `selection.rs` uses), reached by walking DOM ancestors of each frag's
/// `source_node` up to the owning `InlineRun`'s own container via
/// [`inline_element_ancestors`].
pub fn collect_layout_rects(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
) -> std::collections::HashMap<u32, [f32; 4]> {
    let mut out = std::collections::HashMap::new();
    collect_layout_rects_rec(doc, root, &mut out);
    out
}

fn collect_layout_rects_rec(
    doc: &lumen_dom::Document,
    b: &LayoutBox,
    out: &mut std::collections::HashMap<u32, [f32; 4]>,
) {
    // A single `NodeId` can own more than one box: an element with inline content
    // gets an anonymous block/line box for that content, and the box tree keeps the
    // element's own `NodeId` on it. The recursion visits the principal box before
    // its descendants, so `or_insert` keeps the element's own border box; plain
    // `insert` used to hand JS the last (inner) box instead — `getBoundingClientRect`
    // on `<div style="height:20px">x</div>` answered the 19.2px line box (BUG-382).
    let r = &b.rect;
    out.entry(b.node.index() as u32)
        .or_insert([r.x, r.y, r.width, r.height]);
    // BUG-488: plain inline elements (`<span>`, `<em>`, …) own no `LayoutBox` of
    // their own — accumulate the union of every laid-out `InlineFrag` nested
    // inside them, keyed by DOM ancestor via `inline_element_ancestors`. Line
    // y-position uses the same `font_size * line_height` uniform-line-height
    // model `selection.rs` uses to turn `lines[line_idx]` into a pixel rect.
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        let line_h = b.style.font_size * b.style.line_height;
        for (line_idx, line) in lines.iter().enumerate() {
            let line_y = b.rect.y + line_idx as f32 * line_h;
            for frag in line {
                let fx1 = b.rect.x + frag.x;
                let fy1 = line_y;
                let fx2 = fx1 + frag.width;
                let fy2 = fy1 + line_h;
                for anc in inline_element_ancestors(doc, frag.source_node, b.node) {
                    out.entry(anc.index() as u32)
                        .and_modify(|cur| {
                            let cx1 = cur[0].min(fx1);
                            let cy1 = cur[1].min(fy1);
                            let cx2 = (cur[0] + cur[2]).max(fx2);
                            let cy2 = (cur[1] + cur[3]).max(fy2);
                            *cur = [cx1, cy1, cx2 - cx1, cy2 - cy1];
                        })
                        .or_insert([fx1, fy1, fx2 - fx1, fy2 - fy1]);
                }
            }
        }
    }
    for child in &b.children {
        collect_layout_rects_rec(doc, child, out);
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

    /// Like `lay_full`, but also returns the `Document` — needed by callers of
    /// `collect_layout_rects`/`collect_computed_styles` (BUG-488), which walk
    /// DOM ancestry to attribute inline-element boxes.
    fn lay_full_with_doc(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        (doc, root)
    }

    /// Like `lay_full_with_doc`, but lays out with `Fixed8` instead of `layout()`'s
    /// no-op measurer — needed by any test asserting on the *width* of wrapped
    /// text (`layout()` never measures a glyph, so every text frag is 0-wide).
    fn lay_full_measured_with_doc(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
        (doc, root)
    }

    /// BUG-382: an element with inline content owns two boxes with the same
    /// `NodeId` — its principal block box and the anonymous run holding the text.
    /// Both snapshot collectors must answer with the principal one; the earlier
    /// `insert` handed JS the inner run, so `getBoundingClientRect().height` was
    /// the line height and `getComputedStyle().width` was `auto`.
    #[test]
    fn snapshot_collectors_keep_the_principal_box() {
        let (doc, root) = lay_full_with_doc(
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

        let rects = collect_layout_rects(&root, &doc);
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

        let styles = collect_computed_styles(&root, &doc);
        assert_eq!(
            styles[&div_nid].get("width").map(String::as_str),
            Some("50px"),
            "style snapshot must describe the element's own box"
        );
    }

    /// BUG-488: a plain inline-level element (`<span>`, `<em>`, …) owns no
    /// `LayoutBox` of its own — its content is flattened into the enclosing
    /// `InlineRun`'s segments, so neither collector's key set ever contained
    /// the inline element's own `NodeId`. `getComputedStyle()`/
    /// `getBoundingClientRect()` on such an element answered `""` / an
    /// all-zero rect regardless of content, indistinguishable from "element
    /// doesn't exist".
    #[test]
    fn snapshot_collectors_cover_plain_inline_elements() {
        let (doc, root) = lay_full_measured_with_doc(
            "<html><body><div>before <span id=s style=\"color:red\">hi</span> after</div></body></html>",
            "body{margin:0}",
        );
        let span_nid = find_first_dom_node_by_selector(&doc, "#s")
            .expect("span must be findable in the DOM")
            .index() as u32;

        let rects = collect_layout_rects(&root, &doc);
        let rect = rects
            .get(&span_nid)
            .expect("inline element must have a rect entry");
        assert!(rect[2] > 0.0, "span must have a nonzero width: {rect:?}");
        assert!(rect[3] > 0.0, "span must have a nonzero height: {rect:?}");

        let styles = collect_computed_styles(&root, &doc);
        let style = styles
            .get(&span_nid)
            .expect("inline element must have a computed-style entry");
        assert_eq!(
            style.get("color").map(String::as_str),
            Some("rgb(255, 0, 0)"),
            "inline element's own declared style must be visible"
        );
        assert_eq!(style.get("display").map(String::as_str), Some("inline"));
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
    fn font_family_default_is_document_serif() {
        // BUG-128: дефолт документа — UA `serif`, а не пустой список (пустой
        // зарезервирован за chrome UI, см. `style::DEFAULT_FONT_FAMILY`).
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_family, vec!["serif".to_string()]);
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

    // ── font-variant-caps (CSS Fonts L4 §6.2) ───────────────────────────────

    #[test]
    fn font_variant_default_normal() {
        let root = lay("<p>x</p>", "");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::Normal);
    }

    #[test]
    fn font_variant_small_caps_parsed() {
        let root = lay("<p>x</p>", "p { font-variant: small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::SmallCaps);
    }

    #[test]
    fn font_variant_caps_full_value_set_parsed() {
        // CSS Fonts L4 §6.2 — longhand принимает все шесть не-initial значений.
        for (css, want) in [
            ("small-caps", FontVariantCaps::SmallCaps),
            ("all-small-caps", FontVariantCaps::AllSmallCaps),
            ("petite-caps", FontVariantCaps::PetiteCaps),
            ("all-petite-caps", FontVariantCaps::AllPetiteCaps),
            ("unicase", FontVariantCaps::Unicase),
            ("titling-caps", FontVariantCaps::TitlingCaps),
            ("normal", FontVariantCaps::Normal),
        ] {
            let root = lay("<p>x</p>", &format!("p {{ font-variant-caps: {css}; }}"));
            let p = first_element_child(&root);
            assert_eq!(p.style.font_variant_caps, want, "font-variant-caps: {css}");
        }
    }

    #[test]
    fn font_variant_caps_invalid_keyword_ignored() {
        // Невалидное значение longhand-а не отменяет унаследованное
        // (CSS Cascade L4 §4.4 — declaration отбрасывается).
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant-caps: small-caps; } p { font-variant-caps: nope; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::SmallCaps);
    }

    #[test]
    fn font_variant_shorthand_picks_caps_component() {
        let root = lay("<p>x</p>", "p { font-variant: all-small-caps; }");
        let p = first_element_child(&root);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::AllSmallCaps);
    }

    #[test]
    fn font_variant_shorthand_resets_caps_to_initial() {
        // CSS Cascade L4 §3.1: shorthand выставляет ВСЕ свои longhand-ы.
        // `font-variant: common-ligatures` (лигатурная компонента, у нас не
        // реализована) обязан вернуть caps в initial, а не сохранить
        // унаследованное small-caps.
        for css in ["common-ligatures", "none"] {
            let root = lay(
                "<div><p>x</p></div>",
                &format!("div {{ font-variant: small-caps; }} p {{ font-variant: {css}; }}"),
            );
            let p = first_element_child(first_element_child(&root));
            assert_eq!(p.style.font_variant_caps, FontVariantCaps::Normal, "font-variant: {css}");
        }
    }

    #[test]
    fn font_variant_normal_keyword_resets() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; } p { font-variant: normal; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(div.style.font_variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::Normal);
    }

    #[test]
    fn font_variant_inherited() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { font-variant: small-caps; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert_eq!(p.style.font_variant_caps, FontVariantCaps::SmallCaps);
    }

    #[test]
    fn font_variant_caps_synthesized_into_frags() {
        // End-to-end: small-caps доезжает до фрагментов — строчные подняты в
        // верхний регистр и нарисованы уменьшенным кеглем.
        let root = lay_measured("<p>Hi</p>", "p { font-variant-caps: small-caps; font-size: 20px; }", 400.0);
        let run = first_inline_run(first_element_child(&root));
        let BoxKind::InlineRun { lines, .. } = &run.kind else { panic!("expected InlineRun") };
        let frags: Vec<(&str, f32)> = lines
            .iter()
            .flatten()
            .map(|f| (f.text.as_str(), f.style.font_size))
            .collect();
        assert_eq!(frags, vec![("H", 20.0), ("I", 16.0)]);
    }

    #[test]
    fn font_variant_caps_does_not_break_word_at_case_boundary() {
        // Разрез «H|ELLO» проходит внутри слова: перенос по нему запрещён,
        // иначе узкий контейнер разорвал бы слово пополам.
        let root = lay_measured(
            "<p>Hello</p>",
            "p { font-variant-caps: small-caps; font-size: 20px; }",
            24.0,
        );
        let run = first_inline_run(first_element_child(&root));
        let BoxKind::InlineRun { lines, .. } = &run.kind else { panic!("expected InlineRun") };
        let non_empty = lines.iter().filter(|l| !l.is_empty()).count();
        assert_eq!(non_empty, 1, "слово разорвано на {non_empty} строк: {lines:?}");
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

    #[test]
    fn font_stretch_as_percent_matches_os2_width_class_scale() {
        // `as_percent` — единицы matcher-а (`FaceRecord::stretch`,
        // `usWidthClass`). Дробные keyword-ы округляются к ближайшему целому:
        // шкала usWidthClass целочисленная, полуступеней у неё нет.
        assert_eq!(FontStretch::NORMAL.as_percent(), 100);
        assert_eq!(FontStretch(500).as_percent(), 50); // ultra-condensed
        assert_eq!(FontStretch(750).as_percent(), 75); // condensed
        assert_eq!(FontStretch(875).as_percent(), 88); // semi-condensed 87.5%
        assert_eq!(FontStretch(1125).as_percent(), 113); // semi-expanded 112.5%
        assert_eq!(FontStretch(2000).as_percent(), 200); // ultra-expanded
        // Округление к ближайшему, а не вверх: 80.4% → 80.
        assert_eq!(FontStretch(804).as_percent(), 80);
    }

    #[test]
    fn font_stretch_parse_keyword_and_percentage() {
        assert_eq!(FontStretch::parse("condensed"), Some(FontStretch(750)));
        assert_eq!(FontStretch::parse("80%"), Some(FontStretch(800)));
        // Диапазон из двух значений (синтаксис @font-face) → первое значение.
        assert_eq!(FontStretch::parse("75% 125%"), Some(FontStretch(750)));
        // Кламп в [50%, 200%] — держит значение в u16 без переполнения.
        assert_eq!(FontStretch::parse("10%"), Some(FontStretch(500)));
        assert_eq!(FontStretch::parse("300%"), Some(FontStretch(2000)));
        assert_eq!(FontStretch::parse("nonsense"), None);
        assert_eq!(FontStretch::parse(""), None);
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

    // ── CSS Container Style Queries (Phase 0) ──────────────────────────────

    fn style_ctx(props: &[(&str, &str)]) -> crate::ContainerContext {
        style_ctx_with_style_props(props, &[])
    }

    fn style_ctx_with_style_props(
        custom_props: &[(&str, &str)],
        style_props: &[(&str, &str)],
    ) -> crate::ContainerContext {
        let mut custom = std::collections::HashMap::new();
        for (k, v) in custom_props {
            custom.insert(k.to_string(), v.to_string());
        }
        let mut style = std::collections::HashMap::new();
        for (k, v) in style_props {
            style.insert(k.to_string(), v.to_string());
        }
        crate::ContainerContext {
            width: 200.0,
            height: Some(100.0),
            names: vec![],
            custom_props: custom.into(),
            style_props: style,
            font_size: 16.0,
            viewport: lumen_core::Size::new(1024.0, 768.0),
            own_containing_block_height: 100.0,
        }
    }

    #[test]
    fn style_query_with_value_true() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_with_value_false() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(!crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_with_value_missing() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_boolean_true() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_boolean_false() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_with_extra_spaces() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style(--theme:  dark )", &ctx));
    }

    #[test]
    fn style_query_not() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(crate::evaluate_container_condition("not style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_combined_with_size() {
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("(min-width: 150px) and style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_combined_with_size_false() {
        let ctx = style_ctx(&[("--theme", "light")]);
        assert!(!crate::evaluate_container_condition("(min-width: 150px) and style(--theme: dark)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_unset_returns_false() {
        let ctx = style_ctx(&[]);
        assert!(!crate::evaluate_container_condition("style(width: 100px)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_matches() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display: flex)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(!crate::evaluate_container_condition("style(display: block)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_keyword_case_insensitive() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display: FLEX)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_boolean_form_true_when_set() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition("style(display)", &ctx));
    }

    #[test]
    fn style_query_non_custom_property_boolean_form_false_when_unset() {
        let ctx = style_ctx_with_style_props(&[], &[]);
        assert!(!crate::evaluate_container_condition("style(display)", &ctx));
    }

    #[test]
    fn style_query_color_keyword_matches_computed_rgb() {
        // Container's computed style is already serialized as `rgb(...)`
        // (getComputedStyle form); the query uses the author's keyword.
        let ctx = style_ctx_with_style_props(&[], &[("color", "rgb(255, 0, 0)")]);
        assert!(crate::evaluate_container_condition("style(color: red)", &ctx));
    }

    #[test]
    fn style_query_color_hex_matches_computed_rgb() {
        let ctx = style_ctx_with_style_props(&[], &[("background-color", "rgb(0, 0, 255)")]);
        assert!(crate::evaluate_container_condition(
            "style(background-color: #0000ff)",
            &ctx
        ));
    }

    #[test]
    fn style_query_color_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("color", "rgb(255, 0, 0)")]);
        assert!(!crate::evaluate_container_condition("style(color: blue)", &ctx));
    }

    #[test]
    fn style_query_non_color_value_mismatch_still_returns_false() {
        // A non-color, non-matching value must not be coerced into matching.
        let ctx = style_ctx_with_style_props(&[], &[("display", "flex")]);
        assert!(!crate::evaluate_container_condition("style(display: grid)", &ctx));
    }

    #[test]
    fn style_query_length_pt_matches_computed_px() {
        // Container's computed style is serialized in px; the query uses `pt`.
        let ctx = style_ctx_with_style_props(&[], &[("border-top-width", "2.6667px")]);
        assert!(crate::evaluate_container_condition(
            "style(border-top-width: 2pt)",
            &ctx
        ));
    }

    #[test]
    fn style_query_length_in_matches_computed_px() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "96px")]);
        assert!(crate::evaluate_container_condition("style(width: 1in)", &ctx));
    }

    #[test]
    fn style_query_length_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "96px")]);
        assert!(!crate::evaluate_container_condition("style(width: 2in)", &ctx));
    }

    #[test]
    fn style_query_em_matches_computed_px() {
        // `style_ctx` has font_size: 16.0 → `1em` resolves to 16px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "16px")]);
        assert!(crate::evaluate_container_condition("style(width: 1em)", &ctx));
    }

    #[test]
    fn style_query_em_mismatch_returns_false() {
        let ctx = style_ctx_with_style_props(&[], &[("width", "16px")]);
        assert!(!crate::evaluate_container_condition("style(width: 2em)", &ctx));
    }

    #[test]
    fn style_query_percent_matches_computed_px() {
        // `style_ctx` has width: 200.0 → `50%` resolves to 100px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "100px")]);
        assert!(crate::evaluate_container_condition("style(width: 50%)", &ctx));
    }

    #[test]
    fn style_query_line_height_percent_uses_font_size_basis_not_width() {
        // `style_ctx` has font_size: 16.0, width: 200.0. `line-height: 150%`
        // must resolve against font-size (24px), not width (300px).
        let ctx = style_ctx_with_style_props(&[], &[("line-height", "24px")]);
        assert!(crate::evaluate_container_condition("style(line-height: 150%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(line-height: 50%)", &ctx));
    }

    #[test]
    fn style_query_height_percent_uses_height_basis_not_width() {
        // `style_ctx` has height: Some(100.0), width: 200.0. `height: 50%`
        // must resolve against height (50px), not width (100px).
        let ctx = style_ctx_with_style_props(&[], &[("height", "50px")]);
        assert!(crate::evaluate_container_condition("style(height: 50%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(height: 100%)", &ctx));
    }

    #[test]
    fn style_query_top_percent_uses_height_basis() {
        let ctx = style_ctx_with_style_props(&[], &[("top", "25px")]);
        assert!(crate::evaluate_container_condition("style(top: 25%)", &ctx));
    }

    #[test]
    fn style_query_height_basis_is_own_containing_block_not_own_height() {
        // Even when the container is `container-type: inline-size` (its own
        // `height` is unknown, mirroring `ctx.height: None`), `%` in a
        // `height`/`top` style() query must resolve against the container's
        // *own* containing block height — not fall back to width like the
        // Phase 0 gap used to (see CSS-SPECS.md T3 Container Queries).
        let mut ctx = style_ctx_with_style_props(&[], &[("height", "60px")]);
        ctx.height = None;
        ctx.own_containing_block_height = 300.0;
        assert!(crate::evaluate_container_condition("style(height: 20%)", &ctx));
        assert!(!crate::evaluate_container_condition("style(height: 30%)", &ctx),
            "30% of the containing block (300px) is 90px, not 60px — must not fall back to width (200px)");
    }

    #[test]
    fn style_query_margin_top_percent_still_uses_width_basis() {
        // CSS2.1 §8.3: vertical margin/padding percentages resolve against
        // the containing block *width*, not height — unlike `height`/`top`.
        // `style_ctx` has width: 200.0 → `50%` resolves to 100px.
        let ctx = style_ctx_with_style_props(&[], &[("margin-top", "100px")]);
        assert!(crate::evaluate_container_condition("style(margin-top: 50%)", &ctx));
    }

    #[test]
    fn style_query_viewport_unit_matches_computed_px() {
        // `style_ctx` has viewport: 1024x768 → `10vw` resolves to 102.4px.
        let ctx = style_ctx_with_style_props(&[], &[("width", "102.4px")]);
        assert!(crate::evaluate_container_condition("style(width: 10vw)", &ctx));
    }

    #[test]
    fn style_query_value_internal_whitespace_normalized() {
        // Container declares `1px  2px` (two spaces); query uses a single space.
        let ctx = style_ctx(&[("--gap", "1px  2px")]);
        assert!(crate::evaluate_container_condition("style(--gap: 1px 2px)", &ctx));
    }

    #[test]
    fn style_query_value_no_space_matches_spaced() {
        // Query without a space after the colon matches a spaced container value.
        let ctx = style_ctx(&[("--gap", "1px 2px")]);
        assert!(crate::evaluate_container_condition("style(--gap:1px 2px)", &ctx));
    }

    #[test]
    fn style_query_value_comma_whitespace_normalized() {
        // `a, b` (container) equals `a,b` (query) after comma-space normalization.
        let ctx = style_ctx(&[("--list", "a, b")]);
        assert!(crate::evaluate_container_condition("style(--list: a,b)", &ctx));
    }

    #[test]
    fn style_query_value_whitespace_difference_still_distinguishes_tokens() {
        // Normalization must not merge distinct tokens: `1px2px` != `1px 2px`.
        let ctx = style_ctx(&[("--gap", "1px2px")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 1px 2px)", &ctx));
    }

    #[test]
    fn style_query_var_chain_resolves() {
        // Container's `--gap` references `--base` via var() — resolved before compare.
        let ctx = style_ctx(&[("--base", "8px"), ("--gap", "var(--base)")]);
        assert!(crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    #[test]
    fn style_query_var_chain_mismatch() {
        let ctx = style_ctx(&[("--base", "8px"), ("--gap", "var(--base)")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 4px)", &ctx));
    }

    #[test]
    fn style_query_var_unresolved_reference_is_false() {
        // `--gap` references an undeclared custom property with no fallback.
        let ctx = style_ctx(&[("--gap", "var(--missing)")]);
        assert!(!crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    #[test]
    fn style_query_var_boolean_form_resolves() {
        let ctx = style_ctx(&[("--base", "dark"), ("--theme", "var(--base)")]);
        assert!(crate::evaluate_container_condition("style(--theme)", &ctx));
    }

    #[test]
    fn style_query_var_fallback_used() {
        // `--gap` references an undeclared property, but with a fallback value.
        let ctx = style_ctx(&[("--gap", "var(--missing, 8px)")]);
        assert!(crate::evaluate_container_condition("style(--gap: 8px)", &ctx));
    }

    // ── CSS Container Style Queries — nested and/or/not inside a single
    // style() call (CSS Containment L3 §5.2 <style-condition> grammar) ────

    #[test]
    fn style_query_nested_and_both_true() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "2")]);
        assert!(crate::evaluate_container_condition("style((--a: 1) and (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_and_one_false() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "3")]);
        assert!(!crate::evaluate_container_condition("style((--a: 1) and (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_or_one_true() {
        let ctx = style_ctx(&[("--a", "9"), ("--b", "2")]);
        assert!(crate::evaluate_container_condition("style((--a: 1) or (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_or_both_false() {
        let ctx = style_ctx(&[("--a", "9"), ("--b", "9")]);
        assert!(!crate::evaluate_container_condition("style((--a: 1) or (--b: 2))", &ctx));
    }

    #[test]
    fn style_query_nested_not() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "block")]);
        assert!(crate::evaluate_container_condition("style(not (display: none))", &ctx));
    }

    #[test]
    fn style_query_nested_not_false() {
        let ctx = style_ctx_with_style_props(&[], &[("display", "none")]);
        assert!(!crate::evaluate_container_condition("style(not (display: none))", &ctx));
    }

    #[test]
    fn style_query_nested_and_chain_of_three() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "2"), ("--c", "3")]);
        assert!(crate::evaluate_container_condition(
            "style((--a: 1) and (--b: 2) and (--c: 3))",
            &ctx
        ));
    }

    #[test]
    fn style_query_nested_and_chain_of_three_last_false() {
        let ctx = style_ctx(&[("--a", "1"), ("--b", "2"), ("--c", "9")]);
        assert!(!crate::evaluate_container_condition(
            "style((--a: 1) and (--b: 2) and (--c: 3))",
            &ctx
        ));
    }

    #[test]
    fn style_query_nested_mixed_custom_and_standard() {
        let ctx = style_ctx_with_style_props(&[("--theme", "dark")], &[("display", "flex")]);
        assert!(crate::evaluate_container_condition(
            "style((--theme: dark) and (display: flex))",
            &ctx
        ));
    }

    #[test]
    fn style_query_single_feature_extra_grouping_paren() {
        // A single <style-feature> wrapped in one redundant grouping paren layer.
        let ctx = style_ctx(&[("--theme", "dark")]);
        assert!(crate::evaluate_container_condition("style((--theme: dark))", &ctx));
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

    /// @container style(--prop: value) — rule applies when container has the custom property.
    #[test]
    fn container_style_query_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --theme: dark; }
             @container style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container style(--theme: dark) should apply, got height={}",
            p.rect.height,
        );
    }

    /// @container style(--prop: value) — rule does not apply when value differs.
    #[test]
    fn container_style_query_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --theme: light; }
             @container style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "container style(--theme: dark) should NOT apply with --theme: light, got height={}",
            p.rect.height,
        );
    }

    /// @container style(prop: value) — standard (non-custom) property, resolved
    /// against the container's own computed style.
    #[test]
    fn container_style_query_standard_property_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; position: relative; }
             @container style(position: relative) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container style(position: relative) should apply, got height={}",
            p.rect.height,
        );
    }

    /// @container style(prop: value) — standard property query does not apply
    /// when the container's computed value differs.
    #[test]
    fn container_style_query_standard_property_not_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; position: static; }
             @container style(position: relative) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            p.rect.height < 1.0,
            "container style(position: relative) should NOT apply with position:static, got height={}",
            p.rect.height,
        );
    }

    /// @container style(--prop: value) — matches when the container's custom
    /// property is declared via `var()` chained to another custom property.
    #[test]
    fn container_style_query_var_chain_applies() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --base: dark; --theme: var(--base); }
             @container style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "container style(--theme: dark) should apply via var() chain, got height={}",
            p.rect.height,
        );
    }

    /// @container (min-width) and style(...) — combined condition.
    #[test]
    fn container_style_query_combined_with_size() {
        let root = lay_measured(
            "<div><p></p></div>",
            "div { container-type: size; width: 200px; height: 100px; --theme: dark; }
             @container (min-width: 150px) and style(--theme: dark) { p { height: 40px; } }",
            400.0,
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "combined (min-width: 150px) and style(--theme: dark) should apply, got height={}",
            p.rect.height,
        );
    }

    /// @container style(height: %) — the `%` in the query's declared value
    /// must resolve against the container's *own* containing block height
    /// (its parent's content box), not the container's own height or width.
    /// `outer` is 300px tall; `container`'s own `height: 150px` is exactly
    /// 50% of that — a basis of the container's own height (150) or its
    /// width (200) would both give a mismatch instead.
    #[test]
    fn container_style_query_height_percent_uses_parent_containing_block() {
        let root = lay_measured(
            "<div class=\"outer\"><div class=\"container\"><p></p></div></div>",
            "div.outer { height: 300px; }
             div.container { container-type: size; width: 200px; height: 150px; }
             @container style(height: 50%) { p { height: 40px; } }",
            400.0,
        );
        let outer = first_element_child(&root);
        let container = first_element_child(outer);
        let p = first_element_child(container);
        assert!(
            (p.rect.height - 40.0).abs() < 0.5,
            "style(height: 50%) should apply (50% of the 300px parent height == the \
             container's own 150px height), got p.height={}",
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

    /// Первый `BoxKind::Image` в поддереве. Поиск рекурсивный, потому что с
    /// IFC-2 `<img>` — atomic inline-level бокс и лежит не прямым ребёнком
    /// блока, а внутри анонимного `InlineBlockRow`, собирающего его строку.
    fn first_image_child(b: &LayoutBox) -> &LayoutBox {
        fn walk(b: &LayoutBox) -> Option<&LayoutBox> {
            for c in &b.children {
                if matches!(c.kind, BoxKind::Image { .. }) {
                    return Some(c);
                }
                if let Some(found) = walk(c) {
                    return Some(found);
                }
            }
            None
        }
        walk(b).expect("expected at least one image child")
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
    fn img_is_atomic_inline_not_inline_content() {
        // IFC-2: <img> делит строку с текстом, но НЕ вливается в него
        // сегментом — у сегмента нет собственной высоты (BUG-728). Значит один
        // анонимный `InlineBlockRow` на всю строку, а внутри — три куска:
        // прогон «before», картинка, прогон «after».
        let root = lay(r#"<div>before<img src="x" width="10" height="10">after</div>"#, "");
        let div = first_element_child(&root);
        assert_eq!(div.children.len(), 1, "строка должна быть одна, а не {}", div.children.len());
        let row = &div.children[0];
        assert!(
            matches!(row.kind, BoxKind::InlineBlockRow),
            "картинка с текстом обязана собраться в InlineBlockRow"
        );
        assert_eq!(row.children.len(), 3, "got {}", row.children.len());
        assert!(matches!(row.children[0].kind, BoxKind::InlineRun { .. }));
        assert!(matches!(row.children[1].kind, BoxKind::Image { .. }));
        assert!(matches!(row.children[2].kind, BoxKind::InlineRun { .. }));
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
    fn css_revert_falls_back_to_inherited_without_ua_hint() {
        // `color` has no UA-stylesheet hint on `<p>`, so `revert` rolls back to
        // the same value `unset` would give: the inherited value. Cases where
        // `revert` differs from `unset` (a UA hint applies) are covered in
        // `style.rs`'s `revert_*_ua_hint_*` tests.
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
        // BUG-739: inline-flex — atomic inline-level бокс (CSS Display L3 §2.1),
        // а не inline-family: он получает СВОЙ бокс внутри InlineBlockRow, а не
        // уплощается в сегменты родительского InlineRun (так было до фикса).
        let root = lay("<div><span>x</span></div>", "span { display: inline-flex; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(matches!(&div.children[0].kind, BoxKind::InlineBlockRow));
        let item = &div.children[0].children[0];
        assert_eq!(item.style.display, Display::InlineFlex);
    }

    #[test]
    fn display_grid_parses_as_block_family() {
        let root = lay("<p>x</p>", "p { display: grid; }");
        let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert_eq!(p.style.display, Display::Grid);
    }

    #[test]
    fn display_inline_grid_creates_its_own_box() {
        // BUG-739, симметрично `display_inline_flex_parses_and_stores`.
        let root = lay("<div><span>x</span></div>", "span { display: inline-grid; }");
        let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
        assert!(matches!(&div.children[0].kind, BoxKind::InlineBlockRow));
        let item = &div.children[0].children[0];
        assert_eq!(item.style.display, Display::InlineGrid);
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
        // CSS §10.8 / Edge-верификация (TEST-11/TEST-12/TEST-34):
        // ряд из baseline-aligned inline-block-ов получает strut_descent.
        // ряд из bottom-aligned inline-block-ов strut НЕ получает.
        //
        // Strut — content area шрифта ряда без half-leading (descent 0.2em у
        // тестового измерителя). Почему без него — в `box_tree.rs`, ветка
        // `BoxKind::InlineBlockRow`: `line-height: normal` здесь 1.2em, и
        // half-leading от него делает строку выше, чем в Edge (IFC-1, A/B на
        // TEST-02/04/21/56).
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

    /// BUG-188 / TEST-46 regression: individual transform properties compose with
    /// the `transform` property in the spec order (translate → rotate → scale →
    /// transform), all wrapped by the shared `transform-origin` pivot
    /// (CSS Transforms L2 §3). For the TEST-46 `t-individual-plus-transform` box
    /// (`translate: 15px 0; scale: 0.9; transform: rotate(15deg)`) this means:
    /// the box centre — which is also the default `50% 50%` pivot — must map to
    /// `centre + (15, 0)` (scale/rotate keep the pivot fixed, only the leading
    /// translate moves it), and the linear part must be `scale(0.9)·rotate(15deg)`.
    /// Locks the composition so a future refactor can't silently reorder it; the
    /// remaining TEST-46 pixel diff is font-parity (BUG-128), not transform math.
    #[test]
    fn individual_plus_transform_composes_translate_then_scale_then_rotate() {
        let root = lay(
            "<div>x</div>",
            "div { width: 80px; height: 80px; translate: 15px 0px; scale: 0.9; \
                   transform: rotate(15deg); }",
        );
        let div = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("div box");
        let m = forward_box_transform(div).expect("transformed box has a matrix");

        // Box centre = default transform-origin pivot.
        let cx = div.rect.x + div.rect.width / 2.0;
        let cy = div.rect.y + div.rect.height / 2.0;
        let (mx, my) = m.transform_point_2d(cx, cy);
        // Centre moves by exactly the individual `translate` (scale+rotate pivot
        // about the centre, so they leave it fixed). Wrong order/pivot would shift it.
        assert!(
            (mx - (cx + 15.0)).abs() < 0.05 && (my - cy).abs() < 0.05,
            "centre must map to centre+(15,0); got ({mx}, {my}) vs ({}, {cy})",
            cx + 15.0
        );

        // Linear part = scale(0.9) · rotate(15deg). cos15≈0.96593, sin15≈0.25882.
        let (lx, ly) = m.transform_point_2d(cx + 1.0, cy);
        let a = lx - mx; // d(x')/dx
        let b = ly - my; // d(y')/dx
        assert!(
            (a - 0.9 * 0.96593).abs() < 1e-3 && (b - 0.9 * 0.25882).abs() < 1e-3,
            "linear column must be scale(0.9)·rotate(15deg); got a={a}, b={b}"
        );
    }

    /// BUG-125 / TEST-76 regression: CSS Motion Path L1 places the box's
    /// `offset-anchor` (default `auto` = `transform-origin` = centre) ONTO the
    /// path point — not the box's top-left corner. The path coordinate origin is
    /// the box's normal position, so the centre of a box on
    /// `offset-path: path("M 0 0 L 960 0")` at `offset-distance: 480px` must map
    /// to `rect_topleft + (480, 0)`. Without the `T(-anchor)` term the box sat
    /// half-a-box down-and-right of Edge (the original 3.18% TEST-76 diff).
    #[test]
    fn motion_path_centres_anchor_on_path_point() {
        let root = lay(
            "<div>x</div>",
            r#"div { width: 40px; height: 40px; offset-path: path("M 0 0 L 960 0"); offset-distance: 480px; offset-rotate: 0deg; }"#,
        );
        let div = root
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("div box");
        let m = forward_box_transform(div).expect("motion-path box has a matrix");

        // Box centre (= default anchor) must land on the path point, which is
        // `rect_topleft + (480, 0)` — NOT `rect_topleft + centre + (480, 0)`.
        let cx = div.rect.x + div.rect.width / 2.0;
        let cy = div.rect.y + div.rect.height / 2.0;
        let (mx, my) = m.transform_point_2d(cx, cy);
        let (ex, ey) = (div.rect.x + 480.0, div.rect.y);
        assert!(
            (mx - ex).abs() < 0.05 && (my - ey).abs() < 0.05,
            "anchor must map to path point ({ex}, {ey}); got ({mx}, {my})"
        );
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

    /// Топовый (первый) слой маски `<p>` — все mask-longhand-ы теперь живут
    /// в `mask_layers` (CSS Masking L1 §4.9).
    fn first_p_mask(root: &LayoutBox) -> MaskLayer {
        first_p_style(root)
            .mask_layers
            .first()
            .cloned()
            .expect("mask layer")
    }

    #[test]
    fn mask_image_url() {
        let root = lay("<p>x</p>", "p { mask-image: url(\"mask.png\"); }");
        assert_eq!(
            first_p_mask(&root).image,
            BackgroundImage::Url("mask.png".into())
        );
    }

    #[test]
    fn mask_image_none_clears() {
        let root = lay("<p>x</p>", "p { mask-image: url(m.png); mask-image: none; }");
        assert_eq!(first_p_mask(&root).image, BackgroundImage::None);
    }

    #[test]
    fn mask_repeat_no_repeat() {
        let root = lay("<p>x</p>", "p { mask-repeat: no-repeat; }");
        assert_eq!(first_p_mask(&root).repeat, BackgroundRepeat::NoRepeat);
    }

    #[test]
    fn mask_size_cover() {
        let root = lay("<p>x</p>", "p { mask-size: cover; }");
        assert_eq!(first_p_mask(&root).size, BackgroundSize::Cover);
    }

    #[test]
    fn mask_mode_default_is_alpha() {
        let root = lay("<p>x</p>", "p { mask-image: linear-gradient(black, white); }");
        assert_eq!(first_p_mask(&root).mode, MaskMode::Alpha);
    }

    #[test]
    fn mask_mode_luminance() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; }");
        assert_eq!(first_p_mask(&root).mode, MaskMode::Luminance);
    }

    #[test]
    fn mask_mode_alpha_keyword() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: alpha; }");
        assert_eq!(first_p_mask(&root).mode, MaskMode::Alpha);
    }

    #[test]
    fn mask_mode_match_source_resolves_to_alpha() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: match-source; }");
        assert_eq!(first_p_mask(&root).mode, MaskMode::Alpha);
    }

    #[test]
    fn mask_mode_invalid_keeps_previous() {
        let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: bogus; }");
        assert_eq!(first_p_mask(&root).mode, MaskMode::Luminance);
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
        assert_eq!(
            div.style.mask_layers.first().expect("div mask layer").mode,
            MaskMode::Luminance,
            "div carries the rule"
        );
        let p = div
            .children
            .iter()
            .find(|c| matches!(&c.kind, BoxKind::Block))
            .expect("p block");
        assert!(
            p.style.mask_layers.is_empty(),
            "child does not inherit the mask"
        );
    }

    // ──────── CSS Masking L1 §4.9 — multi-layer masks + `mask` shorthand ────────

    #[test]
    fn mask_image_list_creates_one_layer_per_image() {
        let root = lay(
            "<p>x</p>",
            "p { mask-image: url(a.png), linear-gradient(black, white), none; }",
        );
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].image, BackgroundImage::Url("a.png".into()));
        assert!(matches!(layers[1].image, BackgroundImage::Gradient(_)));
        assert_eq!(layers[2].image, BackgroundImage::None);
    }

    #[test]
    fn mask_longhands_cycle_over_layers() {
        // 3 слоя, 2 значения repeat → cycling: no-repeat, repeat-x, no-repeat.
        let root = lay(
            "<p>x</p>",
            "p { mask-image: url(a.png), url(b.png), url(c.png);
                 mask-repeat: no-repeat, repeat-x; }",
        );
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].repeat, BackgroundRepeat::NoRepeat);
        assert_eq!(layers[1].repeat, BackgroundRepeat::RepeatX);
        assert_eq!(layers[2].repeat, BackgroundRepeat::NoRepeat);
    }

    #[test]
    fn mask_composite_list_per_layer() {
        let root = lay(
            "<p>x</p>",
            "p { mask-image: url(a.png), url(b.png);
                 mask-composite: intersect, subtract; }",
        );
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers[0].composite, MaskComposite::Intersect);
        assert_eq!(layers[1].composite, MaskComposite::Subtract);
    }

    #[test]
    fn mask_composite_default_is_add() {
        let root = lay("<p>x</p>", "p { mask-image: url(a.png); }");
        assert_eq!(first_p_mask(&root).composite, MaskComposite::Add);
    }

    #[test]
    fn mask_clip_and_origin_lists() {
        let root = lay(
            "<p>x</p>",
            "p { mask-image: url(a.png), url(b.png);
                 mask-origin: content-box, padding-box;
                 mask-clip: no-clip, fill-box; }",
        );
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers[0].origin, BackgroundOrigin::ContentBox);
        assert_eq!(layers[1].origin, BackgroundOrigin::PaddingBox);
        assert_eq!(layers[0].clip, MaskClip::NoClip);
        assert_eq!(layers[1].clip, MaskClip::FillBox);
    }

    #[test]
    fn mask_longhand_without_image_creates_a_layer() {
        // Longhand без `mask-image` не должен теряться: создаётся один слой
        // с initial-значениями и применённым longhand-ом.
        let root = lay("<p>x</p>", "p { mask-repeat: no-repeat; }");
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].image, BackgroundImage::None);
        assert_eq!(layers[0].repeat, BackgroundRepeat::NoRepeat);
    }

    #[test]
    fn mask_shorthand_single_layer() {
        let root = lay(
            "<p>x</p>",
            "p { mask: url(m.png) center / cover no-repeat content-box luminance intersect; }",
        );
        let m = first_p_mask(&root);
        assert_eq!(m.image, BackgroundImage::Url("m.png".into()));
        assert_eq!(m.size, BackgroundSize::Cover);
        assert_eq!(m.repeat, BackgroundRepeat::NoRepeat);
        assert_eq!(m.origin, BackgroundOrigin::ContentBox);
        // Один <geometry-box> задаёт и origin, и clip.
        assert_eq!(m.clip, MaskClip::ContentBox);
        assert_eq!(m.mode, MaskMode::Luminance);
        assert_eq!(m.composite, MaskComposite::Intersect);
    }

    #[test]
    fn mask_shorthand_two_geometry_boxes() {
        let root = lay("<p>x</p>", "p { mask: url(m.png) padding-box no-clip; }");
        let m = first_p_mask(&root);
        assert_eq!(m.origin, BackgroundOrigin::PaddingBox);
        assert_eq!(m.clip, MaskClip::NoClip);
    }

    #[test]
    fn mask_shorthand_no_clip_before_geometry_box() {
        // `||` — порядок свободный: `no-clip` занимает слот clip, поэтому
        // следующий <geometry-box> обязан попасть в origin, а не затереть clip.
        let root = lay("<p>x</p>", "p { mask: url(m.png) no-clip padding-box; }");
        let m = first_p_mask(&root);
        assert_eq!(m.origin, BackgroundOrigin::PaddingBox);
        assert_eq!(m.clip, MaskClip::NoClip);
    }

    #[test]
    fn mask_shorthand_two_geometry_boxes_fill_origin_then_clip() {
        let root = lay("<p>x</p>", "p { mask: url(m.png) padding-box content-box; }");
        let m = first_p_mask(&root);
        assert_eq!(m.origin, BackgroundOrigin::PaddingBox);
        assert_eq!(m.clip, MaskClip::ContentBox);
    }

    #[test]
    fn mask_shorthand_resets_unspecified_longhands() {
        let root = lay(
            "<p>x</p>",
            "p { mask-repeat: no-repeat; mask-mode: luminance; mask: url(m.png); }",
        );
        let m = first_p_mask(&root);
        assert_eq!(m.repeat, BackgroundRepeat::Repeat, "reset to initial");
        assert_eq!(m.mode, MaskMode::Alpha, "reset to initial");
    }

    #[test]
    fn mask_shorthand_multi_layer() {
        let root = lay(
            "<p>x</p>",
            "p { mask: url(a.png) no-repeat, linear-gradient(black, white) subtract; }",
        );
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].image, BackgroundImage::Url("a.png".into()));
        assert_eq!(layers[0].repeat, BackgroundRepeat::NoRepeat);
        assert_eq!(layers[0].composite, MaskComposite::Add);
        assert!(matches!(layers[1].image, BackgroundImage::Gradient(_)));
        assert_eq!(layers[1].composite, MaskComposite::Subtract);
    }

    #[test]
    fn mask_shorthand_none_clears_the_image() {
        let root = lay("<p>x</p>", "p { mask-image: url(a.png); mask: none; }");
        let layers = &first_p_style(&root).mask_layers;
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].image, BackgroundImage::None);
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

    /// Block-axis gutter: `overflow-x: scroll` + `scrollbar-gutter: stable` reserves
    /// space for the horizontal scrollbar, so a `%`-height child shrinks by 12 px
    /// while the container's own border-box height stays put.
    #[test]
    fn scrollbar_gutter_block_stable_reduces_child_height() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable; } p { height: 100%; }",
        );
        let div = first_element_child(&root);
        let p = first_element_child(div);
        // 200 content-box → minus 12 block gutter = 188.
        assert!((div.rect.height - 200.0).abs() < 0.01, "div={}", div.rect.height);
        assert!((p.rect.height - 188.0).abs() < 0.01, "p child={}", p.rect.height);
    }

    /// `both-edges` is undefined for the block axis: only one gutter unit reserved
    /// (unlike the inline axis, which doubles it). 200 − 12 = 188.
    #[test]
    fn scrollbar_gutter_block_both_edges_single_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable both-edges; } p { height: 100%; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert!((p.rect.height - 188.0).abs() < 0.01, "p child={}", p.rect.height);
    }

    /// `scrollbar-width: thin` uses a 6 px block-axis gutter. 200 − 6 = 194.
    #[test]
    fn scrollbar_gutter_block_thin_reduces_by_6() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable; scrollbar-width: thin; } p { height: 100%; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert!((p.rect.height - 194.0).abs() < 0.01, "p child={}", p.rect.height);
    }

    /// Without `overflow-x: scroll/auto`, block-axis `scrollbar-gutter: stable` has
    /// no effect: the `%`-height child fills the full content height.
    #[test]
    fn scrollbar_gutter_block_no_scroll_no_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { height: 200px; scrollbar-gutter: stable; } p { height: 100%; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert!((p.rect.height - 200.0).abs() < 0.01, "p child={}", p.rect.height);
    }

    /// `scrollbar-width: none` suppresses the block-axis gutter even with
    /// `overflow-x: scroll` + `scrollbar-gutter: stable`.
    #[test]
    fn scrollbar_gutter_block_width_none_no_reduction() {
        let root = lay(
            "<div><p>x</p></div>",
            "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable; scrollbar-width: none; } p { height: 100%; }",
        );
        let p = first_element_child(first_element_child(&root));
        assert!((p.rect.height - 200.0).abs() < 0.01, "p child={}", p.rect.height);
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
