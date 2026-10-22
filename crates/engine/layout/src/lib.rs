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
pub mod highlight;
pub mod hyphenation;
pub mod counter_builtin;
#[cfg(test)]
mod counter_builtin_cases;
pub mod counters;
mod custom_flow;
mod invariants;
pub mod font_palette;
mod computed_font_mask;
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
mod grid_computed;
mod resolved_geometry;
mod pseudo_geometry;
pub use pseudo_geometry::{collect_pseudo_rects, PseudoGeometry};
mod clickable;
pub use clickable::{collect_clickable_elements, ClickableElement, ClickableKind};
use clickable::inline_element_ancestors;
mod sticky_snap;
pub use sticky_snap::{
    collect_snap_containers, collect_sticky_boxes, compute_sticky_offset, find_snap_target, find_snapped_nodes,
    SnapContainer, SnapPoint, SnapTargets, StickyBox,
};
#[cfg(test)]
use sticky_snap::{snap_offset_x, snap_offset_y};
mod transform_interp;
pub mod selection;
pub mod selector_query;
mod css_wide_value;
pub use css_wide_value::{initial_computed_value, is_inherited_property};
pub mod style_map;
pub use style_map::StyleMap;
pub mod scoped_collect;
mod scroll_rollup;
mod scroll_state;
pub use scroll_rollup::RollupCache;
pub mod paint_eq;
pub use paint_eq::subtree_paint_eq;
pub mod scroll_initial_target;
pub mod scroll_timeline;
pub mod snapshot;
pub mod inert;
pub mod stacking;
pub mod starting_style;
pub mod style;
pub mod masonry;
pub mod subgrid;
pub mod table;
pub mod text_geometry;
pub mod text_iter;
pub mod vertical;

pub use scoped_collect::{
    ChainBreaks, ChangedNodes, CustomPropMaps, PlanOptions, PseudoMaps, Reasons, ScopedCollection, StyleCollectStats,
};

pub use scroll_initial_target::{apply_scroll_initial_targets, InitialScroll};

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
pub use ruby::{
    RubyAlign, RubyBox, RubyLevel, RubyLevelShape, RubyMerge, RubyPosition, RubySegment,
    RubySegmentShape, RubyShape, RubySide, lay_out_ruby, lay_out_ruby_segments,
    resolve_level_sides,
};
pub use selector_query::{animated_marker_key, TRANSFORM_LIST_KEY};
pub use transform_interp::{interpolate_transform_value, resolve_transform_value};
pub use animation::{
    AnimValue, AnimatedStyle, AnimationFrame, AnimationInterpolator, interpolate_gap_rules,
    LinearInterpolator, NoopInterpolator, parse_keyframe_style, KeyframeStyle,
    CompositorAnimFrame, CompositorOverride,
    AnimationScheduler, TransitionScheduler, TransitionEventInfo, TransitionEventKind,
};
pub use box_tree::{
    apply_container_styles, apply_intrinsic_size, build_iframe_document, canvas_background_color,
    collect_background_image_requests, collect_cascade_background_image_requests, collect_image_requests,
    pick_image_request_url, collect_shape_image_requests,
    is_open_details, layout, layout_measured,
    layout_measured_hyp, layout_measured_hyp_with_counters, layout_measured_with_counters, layout_mutation_incremental,
    layout_mutation_incremental_with_counters, layout_streaming_incremental,
    lay_out_incremental, select_widget_arrow_width, select_widget_font_size, AnchorScroll, AnchorScroller, BoxKind, BoxOrigin,
    BoxRole, CrossOriginMode, FieldsetLegend, FormControlKind, ImageRequest, InlineFrag, InlineSegment, LayoutBox, MergedSource,
    PseudoKind, SvgMaskContent, SvgShapeKind, SvgTextAnchor, SvgDominantBaseline, SvgBaselineShift,
    ViewBox, SELECT_WIDGET_PAD_PX,
};
pub use incremental::{ReleasedIds, DirtyBits, mark_dirty, mark_dirty_set, clear_dirty, translate_subtree};
pub use page::{MarginBox, MarginBoxPosition, PageBox, PageProperties, MarginBoxTextFragment};
pub use pagination::{paginate, Page, PageFragment, PaginationContext};
pub use property_trees::{
    compute_local_transform, forward_box_transform, perspective_matrix, transform_fns_to_matrix,
    translate_lp_matrix, TransformRef,
    ClipNode, ClipTree, EffectNode, EffectTree,
    Mat4, PropertyTreeNodeId, PropertyTrees, ScrollNode, ScrollTree, TransformNode, TransformTree,
};
pub use highlight::{
    compute_highlight_style, originating_base, resolve_highlights, HighlightEnv, HighlightLayer, HighlightLook, HighlightPiece, Highlights,
    UA_SELECTION_BACKGROUND,
};
pub use selection::{
    caret_at_point, clamp_to_range, node_between, paragraph_range_at_point, select_scope_at_point, selection_rects,
    user_select_none_text_nodes, SelectScope,
};
pub use text_geometry::{collect_text_frag_rects, frag_source_spans, text_hits_at_point, FragSpan, TextFragRect};
pub use style::{
    is_highlight_pseudo, compute_selection_style, compute_style, compute_style_from_declarations, compute_target_text_style,
    compute_view_transition_pseudo_style, ViewTransitionPart,
};
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
    collect_named_scroll_timelines, collect_named_view_timelines, find_nearest_scroll_container,
    resolve_scroll_progress, resolve_view_progress,
    NamedScrollTimeline, NamedViewTimeline, ScrollAxis, ScrollTimeline, ViewTimeline, Viewport,
};
pub use snapshot::serialize_layout_tree;
pub use resolved_geometry::{contains_fixed_descendants, BOXLESS_KEY, COMPUTED_VALUE_KEY_PREFIX};
pub use inert::{collect_inert_regions, is_inert, InertRegion};
pub use starting_style::{resolve_starting_style, StartingStyleTracker};
pub use subgrid::{collect_subgrid_items, SubgridContext, SubgridItem, SubgridTracks};
pub use content_visibility::{
    cv_bottom_estimate, cv_is_skipped, set_cv_scroll, set_cv_relevant, take_cv_skipped, CV_SLACK_FACTOR,
};
pub use invariants::{count_geometry_violations, GeometryViolationCounts};
pub use stacking::{
    box_can_own_stacking_context, creates_stacking_context, is_positioned_layer_auto, owns_paint_layer,
    paint_child_order,
    PaintOrder, PaintPhase,
    StackingContext, StackingContextId, StackingTree,
};
pub use style::{
    apply_container_rules, evaluate_container_condition, evaluate_container_query,
    set_interactive_state, clear_interactive_state,
    set_animated_heights, clear_animated_heights,
    set_forced_colors, forced_colors_active,
    set_print_media, print_media_active,
    set_device_pixel_ratio, device_pixel_ratio,
    parse_background_gradient, parse_color, parse_color_function, parse_css_wide_keyword, parse_gradient_stops,
    parse_grid_template_areas, parse_transform_list,
    radial_gradient_radii, GradientCorner, RadialShape, RadialSize,
    AlignValue, FlexDirection, FlexWrap, AnimationDirection, Appearance, ContainerContext, ContainerKinds, Edge, EdgeFlags, ScrollStateInfo, RuleBreak, RuleInset, RuleInsets, RuleItem, RuleList, GapRuleOverride,
    RuleOverlap, RuleVisibilityItems,
    AnimationFillMode, AnimationPlayState, TransitionBehavior,
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin, BackgroundRepeat, RepeatStyle,
    BackgroundSize, BgSizeAxis, BorderCollapse, BorderStyle,
    BoxShadow, BoxSizing, BreakValue, CalcNode, ClipPath, GeometryBox, SvgClipPath, SvgClipPlacement, SvgClipSubpath, Color, ColorFloat,
    BackfaceVisibility, ClearSide, ContainFlags, ComputedStyle, Content, CustomProps,
    ContentItem, CssColor, CssWideKeyword, Cursor, Direction, Display, EmptyCells, CaptionSide, TableLayout, FilterFn, FilterUrl, FeBlendMode, FeColorMatrix, FeCompositeOp, FeInput, FePrimitive, FeTransfer, SvgFilter, SvgFilterPrimitive, FloatSide, FontOpticalSizing, FontStretch, PrintColorAdjust,
    FontStyle,
    FontVariantCaps, FontVariationSetting, FontWeight, GradientStop, GridAutoFlow, GridLine, GridRepeat, GridTrackSize, Hyphens, ImageRendering,
    MaskClip, MaskComposite, MaskLayer, MaskMode, MasonryAutoFlow,
    Isolation, IterationCount, Length,
    LengthOrAuto, ListStylePosition, ListStyleType, MixBlendMode, ObjectFit, ObjectPosition,
    OutlineColor, OutlineStyle, Overflow, OverflowWrap, OverscrollBehavior, ParsedGradient, Resize,
    PointerEvents,
    Position, PositionComponent, Quotes, ScrollBehavior, ScrollSnapAlign, ScrollSnapAlignKeyword,
    ScrollSnapAxis, ScrollSnapStop, ScrollSnapStrictness, ScrollSnapType, ScrollbarGutter,
    FillRule, ScrollbarWidth, ShapeValue, StepPosition, StrokeLinecap, StrokeLinejoin,
    SvgGradientDef, SvgGradientUnits, SvgPaint, SvgPatternDef, TextAlign, TextDecorationLine, TextDecorationStyle,
    TextDecorationSkipInk, TextDecorationThickness, TextEmphasisPosition, TextEmphasisShape, TextEmphasisStyle,
    TextOverflow, TextShadow, TextTransform, TextUnderlinePosition,
    TimingFunction, TransformFn, TransformStyle,
    UserSelect, Visibility,
    WhiteSpace, WordBreak,
};

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

    /// [`Self::descent_px`] с учётом CSS `font-family` каскада — тот же
    /// приём, что [`Self::char_width_with_families`]: перебирает `families`
    /// и берёт метрики первого резолвящегося шрифта (@font-face, затем
    /// системное имя), а не bundled-fallback (FONTLOAD-10, BUG-467).
    ///
    /// Реализации без доступа к нескольким шрифтам по умолчанию игнорируют
    /// `families` и делегируют к [`Self::descent_px`].
    fn descent_px_with_families(&self, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.descent_px(font_size_px)
    }

    /// [`Self::ascent_px`] с учётом CSS `font-family` каскада — см.
    /// [`Self::descent_px_with_families`].
    fn ascent_px_with_families(&self, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.ascent_px(font_size_px)
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

    /// [`Self::x_height_px`] с учётом CSS `font-family` каскада — см.
    /// [`Self::descent_px_with_families`]. Нужен для `ex`/`rex` (BUG-1340).
    fn x_height_px_with_families(&self, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.x_height_px(font_size_px)
    }

    /// Cap-height шрифта в пикселях при размере `font_size_px` — высота заглавных
    /// (`OS/2.sCapHeight`); источник единицы `cap`/`rcap` (CSS Values L4 §5.1.1,
    /// BUG-1429). Реализации без доступа к метрикам возвращают `0.7 × size`.
    fn cap_height_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 0.7
    }

    /// [`Self::cap_height_px`] с учётом CSS `font-family` каскада — см.
    /// [`Self::descent_px_with_families`].
    fn cap_height_px_with_families(&self, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.cap_height_px(font_size_px)
    }

    /// Line-gap шрифта в пикселях при размере `font_size_px` — рекомендуемый
    /// межстрочный зазор поверх content area (ascent + descent), источник —
    /// `OS/2.sTypoLineGap`/`hhea.lineGap` (FONTLOAD-13, BUG-467).
    ///
    /// Отдельный accessor для `BoxKind::InlineBlockRow`-strut'а (относительное
    /// baseline-выравнивание, `layout_dispatch.rs`) и предпосылка для
    /// `line-gap-override` (CSS Fonts L4 §14.3) — НЕ источник для
    /// `line-height: normal` (см. [`Self::normal_line_height_px`], у которого
    /// собственная, консистентно нормированная сумма ascent+descent+line-gap).
    /// Реализации без доступа к метрикам возвращают `0.0` (большинство шрифтов
    /// не декларируют line-gap).
    fn line_gap_px(&self, font_size_px: f32) -> f32 {
        let _ = font_size_px;
        0.0
    }

    /// [`Self::line_gap_px`] с учётом CSS `font-family` каскада — см.
    /// [`Self::descent_px_with_families`].
    fn line_gap_px_with_families(&self, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.line_gap_px(font_size_px)
    }

    /// CSS2 §10.8.1 `line-height: normal` — используемая line-height в px,
    /// когда автор не задал явное значение. Источник — рекомендуемые
    /// line-spacing метрики face-а (`ascent + descent [+ lineGap]`,
    /// OpenType `OS/2.fsSelection` бит `USE_TYPO_METRICS` выбирает
    /// `sTypoAscender`/`sTypoDescender`/`sTypoLineGap` против
    /// `usWinAscent`/`usWinDescent` — FONTLOAD-15, BUG-467).
    ///
    /// Намеренно ОТДЕЛЬНЫЙ метод, а не сумма [`Self::ascent_px`] +
    /// [`Self::descent_px`] + [`Self::line_gap_px`]: FONTLOAD-14 обнаружила,
    /// что `OwnedFontMetrics::ascent_px` нормирует ascent относительно
    /// `ascent_units + descent_units`, а `descent_px`/`line_gap_px` —
    /// относительно `units_per_em`, так что их сумма использует два разных
    /// знаменателя и не имеет чёткого смысла как абсолютная высота строки.
    /// `normal_line_height_px` — единая, консистентно нормированная (везде
    /// `units_per_em`) величина.
    ///
    /// Дефолт `1.2 × font_size_px` — тот же UA-фоллбек, что CSS2
    /// рекомендует для шрифтов без доступных метрик, и то же значение, на
    /// которое неявно полагался каждый mock `TextMeasurer` до появления
    /// этого метода.
    fn normal_line_height_px(&self, font_size_px: f32) -> f32 {
        font_size_px * 1.2
    }

    /// [`Self::normal_line_height_px`] с учётом CSS `font-family` каскада —
    /// см. [`Self::descent_px_with_families`].
    fn normal_line_height_px_with_families(&self, font_size_px: f32, families: &[String]) -> f32 {
        let _ = families;
        self.normal_line_height_px(font_size_px)
    }
}

// ---------------------------------------------------------------------------
// Scroll container infrastructure
// CSS: overflow — P4 wires: check style.overflow_x/overflow_y == Overflow::Scroll | Auto,
// call collect_scroll_containers() to enumerate regions, set_scroll_position() on wheel.
// ---------------------------------------------------------------------------

/// A scrollable overflow container collected from the layout tree.
/// Shell uses this to route wheel events and update scroll offsets.
#[derive(Clone, Debug, PartialEq)]
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
/// the wheel-event hit-test map — `hidden`/`clip` containers are deliberately
/// excluded here (not wheel-scrollable, CSS Overflow L3 §3.3/§3.4), see
/// [`collect_scroll_containers_for_js_state`] for the JS-visible counterpart.
///
/// # CSS: overflow
/// P4 wires: after adding `overflow: scroll` parsing, this function will naturally
/// include those boxes (LayoutBox.style.overflow_x/y already parsed by P4).
pub fn collect_scroll_containers(root: &LayoutBox) -> Vec<ScrollContainer> {
    let mut out = Vec::new();
    collect_scroll_containers_inner(root, &mut out, false);
    out
}

/// Same enumeration as [`collect_scroll_containers`], but also includes
/// `overflow: hidden`/`clip` containers.
///
/// CSSOM View defines `scrollLeft`/`scrollTop`/`scrollWidth`/`scrollHeight`
/// for any element establishing a scroll container, not only the
/// wheel-scrollable ones: `hidden` is still programmatically scrollable
/// (`set_scroll_position` already honors it) and `clip` still reports a
/// nonzero scrollable-overflow size even though it rejects every scroll
/// write. Before this split, the shell fed the *wheel-routable* list
/// (`collect_scroll_containers`) straight into `update_scroll_states`, so a
/// `hidden`/`clip` container's real, correctly-clamped `scroll_x`/`scroll_y`
/// (set by [`set_scroll_position`]) never reached the JS-visible cache and
/// every read fell back to the shim's zero default (BUG-504, part 7). Use
/// this function at every call site that feeds `update_scroll_states`;
/// wheel/hit-test call sites must keep using [`collect_scroll_containers`].
pub fn collect_scroll_containers_for_js_state(root: &LayoutBox) -> Vec<ScrollContainer> {
    scroll_rollup::collect_for_js_state(&[root])
}

/// BUG-1211 (post-collectors): [`collect_scroll_containers_for_js_state`],
/// scoped to a handful of subtrees. Self-contained per box (no ancestor
/// dependency), so a plain per-root re-walk is correct without threading any
/// context down, unlike [`collect_computed_styles_scoped`].
pub fn collect_scroll_containers_for_js_state_scoped(roots: &[&LayoutBox]) -> Vec<ScrollContainer> {
    scroll_rollup::collect_for_js_state(roots)
}

/// [`collect_scroll_containers`] для корня страницы (`html`-бокс): без
/// контейнеров, чей `overflow` ушёл во вьюпорт.
///
/// CSS Overflow L3 §3.5 — `overflow` корня (а при `visible` у корня — `body`)
/// относится к вьюпорту и самостоятельным контейнером не остаётся. Без этой
/// фильтрации `body{overflow-y:scroll}` (lenta.ru) делался «контейнером» размером
/// с документ, перехватывал каждый щелчок колеса и не отдавал его странице.
pub fn collect_page_scroll_containers(root: &LayoutBox) -> Vec<ScrollContainer> {
    // `root` — бокс документа, его единственный не-Skip ребёнок — `html`,
    // у `html` — `body`.
    fn first_box(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, box_tree::BoxKind::Skip))
    }
    let mut out = collect_scroll_containers(root);
    out.retain(|c| c.node != root.node);
    if let Some(html) = first_box(root) {
        out.retain(|c| c.node != html.node);
        let html_visible = matches!(html.style.overflow_x, style::Overflow::Visible)
            && matches!(html.style.overflow_y, style::Overflow::Visible);
        if html_visible && let Some(body) = first_box(html) {
            out.retain(|c| c.node != body.node);
        }
    }
    out
}

/// Scrollable overflow area of the page — `(width, height)` in CSS px, the
/// extent the viewport can be scrolled over (CSSOM View §5.2: what
/// `document.scrollingElement.scrollWidth`/`scrollHeight` report).
///
/// The shell derives the page's scroll range from the rects the display list
/// paints, so a box that paints nothing (a transparent spacer, an empty
/// `height: 5000px` block) never extended it; this is the layout-side answer
/// (the same BUG-960 scrollable-overflow walk the JS-visible `scrollHeight`
/// uses) and the shell takes the larger of the two. An axis whose viewport
/// `overflow` is `hidden`/`clip` reports `0.0`: the user cannot scroll it, and
/// the shell keeps its paint-based range there as before (CSS Overflow L3
/// §3.5: the viewport takes the root's `overflow`, or the `body`'s when the
/// root's is `visible`).
pub fn page_scrollable_extent(root: &LayoutBox) -> (f32, f32) {
    fn first_box(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, box_tree::BoxKind::Skip))
    }
    let Some(html) = first_box(root) else { return (0.0, 0.0) };
    let html_visible = matches!(html.style.overflow_x, style::Overflow::Visible)
        && matches!(html.style.overflow_y, style::Overflow::Visible);
    let viewport_style = match first_box(html) {
        Some(body) if html_visible => &body.style,
        _ => &html.style,
    };
    let clipped = |o: style::Overflow| matches!(o, style::Overflow::Hidden | style::Overflow::Clip);
    let w = if clipped(viewport_style.overflow_x) { 0.0 } else { content_width(html) };
    let h = if clipped(viewport_style.overflow_y) { 0.0 } else { content_height(html) };
    (w, h)
}

fn collect_scroll_containers_inner(b: &LayoutBox, out: &mut Vec<ScrollContainer>, include_non_wheel: bool) {
    scroll_container_into(b, out, include_non_wheel);
    for child in &b.children {
        collect_scroll_containers_inner(child, out, include_non_wheel);
    }
}

fn scroll_container_into(b: &LayoutBox, out: &mut Vec<ScrollContainer>, include_non_wheel: bool) {
    use style::Overflow;
    let s = &b.style;
    let is_scroll_x = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto)
        || (include_non_wheel && matches!(s.overflow_x, Overflow::Hidden | Overflow::Clip));
    let is_scroll_y = matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto)
        || (include_non_wheel && matches!(s.overflow_y, Overflow::Hidden | Overflow::Clip));
    if is_scroll_x || is_scroll_y {
        let clip = padding_box(b);
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
    } else if include_non_wheel
        && matches!(s.overflow_x, Overflow::Visible)
        && matches!(s.overflow_y, Overflow::Visible)
    {
        // BUG-960: `overflow: visible` doesn't clip and isn't a scroll
        // container, but CSSOM View still defines `scrollWidth`/
        // `scrollHeight` for it as the exact scrollable-overflow-area
        // magnitude (not just "at least padding-box", the BUG-475 floor).
        // Publish an entry only when the box actually overflows its own
        // padding box on some axis — the common non-overflowing case stays
        // off this list and keeps using the (identical, cheaper) border-box
        // fallback in the JS shim.
        let clip = padding_box(b);
        let scroll_width = content_width(b);
        let scroll_height = content_height(b);
        if scroll_width > clip.width + 0.01 || scroll_height > clip.height + 0.01 {
            out.push(ScrollContainer {
                node: b.node,
                clip_rect: clip,
                scroll_width,
                scroll_height,
                scroll_x: 0.0,
                scroll_y: 0.0,
                overscroll_behavior_x: s.overscroll_behavior_x,
                overscroll_behavior_y: s.overscroll_behavior_y,
            });
        }
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

/// A box's padding-box rectangle, in the same document-relative coordinate
/// space as [`LayoutBox::rect`] (border-box).
///
/// CSS Overflow L3 §3.3/§3.4 defines `scrollWidth`/`scrollHeight` and the
/// scrollable-overflow region relative to the padding edge, not the border
/// edge — the two coincide only when all border widths are zero. Shared by
/// [`collect_scroll_containers_inner`]'s clip rect and by
/// [`content_width`]/[`content_height`]'s floor and origin (BUG-504).
fn padding_box(b: &LayoutBox) -> lumen_core::geom::Rect {
    let s = &b.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    lumen_core::geom::Rect::new(
        b.rect.x + bl,
        b.rect.y + bt,
        (b.rect.width - bl - br).max(0.0),
        (b.rect.height - bt - bb).max(0.0),
    )
}

/// Sides of a scroll container's padding box beyond which overflow is *unreachable*: the start
/// sides of its inline and block axes (CSS Overflow L3 §3.3 — content there can never be scrolled
/// into view, so it adds nothing to the scrollable area). The mirror sides are reachable.
#[derive(Clone, Copy)]
pub(crate) struct UnreachableSides {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl UnreachableSides {
    pub(crate) fn of(s: &style::ComputedStyle) -> Self {
        use style::{Direction, WritingMode};
        let rtl = s.direction == Direction::Rtl;
        match s.writing_mode {
            WritingMode::HorizontalTb => Self { left: !rtl, right: rtl, top: true, bottom: false },
            WritingMode::VerticalRl | WritingMode::SidewaysRl => Self { left: false, right: true, top: !rtl, bottom: rtl },
            WritingMode::VerticalLr => Self { left: true, right: false, top: !rtl, bottom: rtl },
            WritingMode::SidewaysLr => Self { left: true, right: false, top: rtl, bottom: !rtl },
        }
    }

    /// Whether `bounds` lies wholly beyond an unreachable side of padding box `pb`.
    pub(crate) fn hides(self, bounds: &lumen_core::geom::Rect, pb: &lumen_core::geom::Rect) -> bool {
        (self.left && bounds.x + bounds.width <= pb.x)
            || (self.right && bounds.x >= pb.x + pb.width)
            || (self.top && bounds.y + bounds.height <= pb.y)
            || (self.bottom && bounds.y >= pb.y + pb.height)
    }
}

/// Whether a child must be folded into its container's scrollable-overflow
/// computation.
///
/// CSS Overflow L3 §3.3: an absolutely/fixed positioned descendant whose
/// border box lies wholly in the *unreachable* scrollable-overflow region of
/// the containing block's padding box ([`UnreachableSides`]) contributes
/// nothing at all — not even on the axis where it does overlap (BUG-504,
/// `overflow-outside-padding.html`). A box wholly beyond the *reachable* side
/// (a dropdown below the padding box, `line-clamp-with-abspos-019`) counts in
/// full. This exclusion is specific to out-of-flow boxes placed via
/// `top`/`right`/`bottom`/`left`: an in-flow box pushed outside by `transform`
/// must still count in full (§3.4, already covered by
/// [`child_scrollable_bounds`]) — that provision is unconditional, so only
/// abspos/fixed children are ever excluded here.
fn contributes_to_scrollable_overflow(
    c: &LayoutBox,
    bounds: &lumen_core::geom::Rect,
    padding_box: &lumen_core::geom::Rect,
    unreachable: UnreachableSides,
) -> bool {
    if !matches!(c.style.position, style::Position::Absolute | style::Position::Fixed) {
        return true;
    }
    !unreachable.hides(bounds, padding_box)
}

/// Axis-aligned bounding box of `rect`'s four corners after applying `m`.
///
/// Shared by [`child_scrollable_bounds`] (scrollable-overflow, BUG-504) and
/// [`collect_layout_rects_rec`] (`getBoundingClientRect`, BUG-540): both need
/// the same "transform a box, then re-flatten to an AABB" step, just applied
/// to a different rect and consumed differently.
fn transformed_aabb(rect: &lumen_core::geom::Rect, m: &Mat4) -> lumen_core::geom::Rect {
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    let corners = [
        m.transform_point_2d(x0, y0),
        m.transform_point_2d(x1, y0),
        m.transform_point_2d(x0, y1),
        m.transform_point_2d(x1, y1),
    ];
    let min_x = corners.iter().fold(f32::INFINITY, |acc, p| acc.min(p.0));
    let max_x = corners.iter().fold(f32::NEG_INFINITY, |acc, p| acc.max(p.0));
    let min_y = corners.iter().fold(f32::INFINITY, |acc, p| acc.min(p.1));
    let max_y = corners.iter().fold(f32::NEG_INFINITY, |acc, p| acc.max(p.1));
    lumen_core::geom::Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// A child's border-box, expanded by its own CSS `transform` (BUG-504).
///
/// CSS Overflow L3 §3.4: `transform` doesn't move a box for flow purposes
/// (`LayoutBox::rect` stays untouched), but it *does* contribute to the
/// scrollable overflow rectangle — a translated/rotated/scaled child can
/// force a scroll container to grow its scroll range even though the
/// child's flow position and size are unaffected. Returns `c.rect` itself
/// when the child carries no transform (the common case, zero-cost).
fn child_scrollable_bounds(c: &LayoutBox) -> lumen_core::geom::Rect {
    // CSS Overflow L4 §line-clamp: the part of a float below the clamp container's content edge
    // is clipped away and is not scrollable overflow either.
    let mut rect = c.rect;
    if let Some(clip) = c.clamp_clip {
        rect.height = rect.height.min(clip.y + clip.height).max(0.0);
    }
    match forward_box_transform(c) {
        Some(m) => transformed_aabb(&rect, &m),
        None => rect,
    }
}

/// Whether a box establishes its own scroll/clip boundary for the purposes
/// of scrollable-overflow rollup (CSS Overflow L3 §3.3/§3.4, BUG-960): any
/// value of `overflow-x`/`overflow-y` other than `visible` clips its own
/// content, so a box's *own* border box still contributes to its
/// container's scrollable-overflow region, but that box's *descendants* do
/// not — their overflow is contained (and separately reported by that box's
/// own `scrollWidth`/`scrollHeight`), not folded further into the ancestor
/// chain.
///
/// CSS Containment L1 §3.2 / §3.3: `contain: layout` and `contain: paint` turn the box's
/// descendants' overflow into ink overflow, so the scrollable area of an ancestor never grows
/// for it either.
fn box_clips_own_overflow(b: &LayoutBox) -> bool {
    !matches!(b.style.overflow_x, style::Overflow::Visible)
        || !matches!(b.style.overflow_y, style::Overflow::Visible)
        || b.style.contain.0 & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0) != 0
}

/// The horizontal scrollable-overflow span of a box, as `(min_x, max_x)`
/// relative to the padding edge (`pb.x` = 0).
///
/// `max_x` is the existing rightward-extension edge (children's right edge
/// minus the padding-box left edge, floored at `pb.width`); `min_x` is its
/// mirror on the other side — 0.0 unless a child's scrollable bounds
/// ([`child_scrollable_bounds`]) start left of the padding edge, which
/// happens for in-flow content whose block-progression direction is
/// physically right-to-left (`writing-mode: vertical-rl`, CSS Overflow L3
/// §3.3/§3.4 — the box's normal-flow child is positioned flush with the
/// *right* edge by the writing-mode-aware block layout pass and extends left
/// under it, not right). A child whose bounds don't overlap the padding box
/// on either axis at all is excluded entirely — CSS Overflow L3 §3.3:
/// "blocks wholly outside padding edges [do] not contribute to overflow"
/// (BUG-504). [`set_scroll_position`] clamps against this range directly;
/// [`content_width`] reduces it to the single-number `scrollWidth` magnitude.
fn scrollable_extent_x(b: &LayoutBox) -> (f32, f32) {
    let pb = padding_box(b);
    let (mut min_x, mut max_x) = (0.0_f32, pb.width);
    let u = UnreachableSides::of(&b.style);
    let mut inflow = (f32::INFINITY, f32::NEG_INFINITY);
    scrollable_extent_x_rec(b, &pb, u, &mut min_x, &mut max_x, &mut inflow);
    // CSS Overflow L3 §3.3: overflow past an unreachable (start) side never joins the scrollable
    // area, so it cannot widen `scrollWidth` either (BUG-1415, `justify-content: center` tracks).
    if u.left {
        min_x = min_x.max(0.0);
    } else if u.right {
        max_x = max_x.min(pb.width);
    }
    // CSS Overflow L3 §2.2: the padding at the reachable end of the inline
    // axis follows the in-flow content, so a scroller whose content overflows
    // keeps its end padding visible (BUG-1389). `abspos` boxes are placed
    // against the padding box and carry no such padding.
    let [_, pad_r, _, pad_l] = box_padding(b);
    if u.left && inflow.1.is_finite() {
        max_x = max_x.max(inflow.1 + pad_r);
    } else if u.right && inflow.0.is_finite() {
        min_x = min_x.min(inflow.0 - pad_l);
    }
    (min_x, max_x)
}

/// A box's used padding `[top, right, bottom, left]` in px.
pub(crate) fn box_padding(b: &LayoutBox) -> [f32; 4] {
    b.used_padding.unwrap_or_else(|| {
        let s = &b.style;
        let vp = lumen_core::geom::Size::new(0.0, 0.0);
        let em = s.font_size;
        [
            s.padding_top.resolve_or_zero(em, 0.0, vp),
            s.padding_right.resolve_or_zero(em, 0.0, vp),
            s.padding_bottom.resolve_or_zero(em, 0.0, vp),
            s.padding_left.resolve_or_zero(em, 0.0, vp),
        ]
    })
}

/// The margins `[top, right, bottom, left]` (non-negative px) that CSS Overflow L3
/// §2.2 adds to the scrollable overflow of `c`: those of a flex or grid item
/// (a direct child of such a container), zero for everything else — block
/// children contribute their border box alone, out-of-flow boxes are placed
/// against the padding box (BUG-1389).
pub(crate) fn item_margins(parent: &LayoutBox, c: &LayoutBox) -> [f32; 4] {
    if !matches!(
        parent.style.display,
        style::Display::Flex | style::Display::InlineFlex | style::Display::Grid | style::Display::InlineGrid
    ) || matches!(c.style.position, style::Position::Absolute | style::Position::Fixed)
    {
        return [0.0; 4];
    }
    let vp = lumen_core::geom::Size::new(0.0, 0.0);
    let em = c.style.font_size;
    let cb_w = parent.rect.width;
    let m = |l: &style::LengthOrAuto| l.resolve_or_zero(em, cb_w, vp).max(0.0);
    [m(&c.style.margin_top), m(&c.style.margin_right), m(&c.style.margin_bottom), m(&c.style.margin_left)]
}

/// A child's scrollable bounds, grown by [`item_margins`] on the reachable side
/// of each axis only (`u`): the start-side margin of an unreachable side can
/// never be scrolled into view.
fn item_scrollable_bounds(
    parent: &LayoutBox,
    c: &LayoutBox,
    mut bounds: lumen_core::geom::Rect,
    u: UnreachableSides,
) -> lumen_core::geom::Rect {
    let [mt, mr, mb, ml] = item_margins(parent, c);
    if u.left {
        bounds.width += mr;
    } else if u.right {
        bounds.x -= ml;
        bounds.width += ml;
    }
    if u.top {
        bounds.height += mb;
    } else if u.bottom {
        bounds.y -= mt;
        bounds.height += mt;
    }
    bounds
}

/// Recursive worker for [`scrollable_extent_x`] (BUG-960): folds in every
/// descendant that isn't behind a clipping ancestor (per
/// [`box_clips_own_overflow`]), not just direct children, so overflow from
/// e.g. a doubly-nested negative-margin box still reaches the outer scroll
/// container's `scrollWidth`. `inflow` collects the `(min, max)` of the
/// non-out-of-flow contributors for the end-padding rule.
fn scrollable_extent_x_rec(
    b: &LayoutBox,
    pb: &lumen_core::geom::Rect,
    u: UnreachableSides,
    min_x: &mut f32,
    max_x: &mut f32,
    inflow: &mut (f32, f32),
) {
    for c in &b.children {
        let own = child_scrollable_bounds(c);
        if contributes_to_scrollable_overflow(c, &own, pb, u) {
            let bounds = item_scrollable_bounds(b, c, own, u);
            *min_x = min_x.min(bounds.x - pb.x);
            *max_x = max_x.max(bounds.x + bounds.width - pb.x);
            if !matches!(c.style.position, style::Position::Absolute | style::Position::Fixed) {
                inflow.0 = inflow.0.min(bounds.x - pb.x);
                inflow.1 = inflow.1.max(bounds.x + bounds.width - pb.x);
            }
        }
        if !box_clips_own_overflow(c) {
            scrollable_extent_x_rec(c, pb, u, min_x, max_x, inflow);
        }
    }
}

/// The vertical counterpart of [`scrollable_extent_x`] — `(min_y, max_y)`
/// relative to the padding edge (`pb.y` = 0).
fn scrollable_extent_y(b: &LayoutBox) -> (f32, f32) {
    let pb = padding_box(b);
    let (mut min_y, mut max_y) = (0.0_f32, pb.height);
    let u = UnreachableSides::of(&b.style);
    let mut inflow = (f32::INFINITY, f32::NEG_INFINITY);
    scrollable_extent_y_rec(b, &pb, u, &mut min_y, &mut max_y, &mut inflow);
    if u.top {
        min_y = min_y.max(0.0);
    } else if u.bottom {
        max_y = max_y.min(pb.height);
    }
    let [pad_t, _, pad_b, _] = box_padding(b);
    if u.top && inflow.1.is_finite() {
        max_y = max_y.max(inflow.1 + pad_b);
    } else if u.bottom && inflow.0.is_finite() {
        min_y = min_y.min(inflow.0 - pad_t);
    }
    (min_y, max_y)
}

/// Recursive worker for [`scrollable_extent_y`] — vertical counterpart of
/// [`scrollable_extent_x_rec`] (BUG-960).
fn scrollable_extent_y_rec(
    b: &LayoutBox,
    pb: &lumen_core::geom::Rect,
    u: UnreachableSides,
    min_y: &mut f32,
    max_y: &mut f32,
    inflow: &mut (f32, f32),
) {
    for c in &b.children {
        let own = child_scrollable_bounds(c);
        if contributes_to_scrollable_overflow(c, &own, pb, u) {
            let bounds = item_scrollable_bounds(b, c, own, u);
            *min_y = min_y.min(bounds.y - pb.y);
            *max_y = max_y.max(bounds.y + bounds.height - pb.y);
            if !matches!(c.style.position, style::Position::Absolute | style::Position::Fixed) {
                inflow.0 = inflow.0.min(bounds.y - pb.y);
                inflow.1 = inflow.1.max(bounds.y + bounds.height - pb.y);
            }
        }
        if !box_clips_own_overflow(c) {
            scrollable_extent_y_rec(c, pb, u, min_y, max_y, inflow);
        }
    }
}

/// Compute the content scroll-width of a box (`scrollWidth`'s magnitude):
/// the full horizontal span of [`scrollable_extent_x`], regardless of which
/// side it extends toward.
fn content_width(b: &LayoutBox) -> f32 {
    let (min_x, max_x) = scrollable_extent_x(b);
    max_x - min_x
}

/// Compute the content scroll-height of a box (`scrollHeight`'s magnitude):
/// the full vertical span of [`scrollable_extent_y`], regardless of which
/// side it extends toward.
fn content_height(b: &LayoutBox) -> f32 {
    let (min_y, max_y) = scrollable_extent_y(b);
    max_y - min_y
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
///
/// `display: contents` elements are in the map too when `counters` is `Some`
/// (BUG-489): `flatten_contents` eliminates such an element's own `LayoutBox`
/// from the tree entirely (its children are spliced into its place), so —
/// unlike a plain inline element — there is no surviving box anywhere to
/// approximate its style from, and no descendant carries it either (a
/// `display:contents` element can wrap block-level children, own
/// non-inherited properties, and any `display` value). Its real, distinct
/// cascade result is still cached in `counters` — [`precompute_counters`]
/// resolves every element's style up front, regardless of `display` — so it
/// is looked up there instead of recomputed. `counters` is `None` at call
/// sites that reuse an already-built `LayoutBox` without a matching fresh
/// [`CounterMap`] (e.g. `relayout_scoped`'s incremental pass); such sites keep
/// today's behaviour (an empty entry) for this one element shape.
///
/// `viewport` is the initial containing block: CSSOM-9 (BUG-472) replaces the
/// computed `width`/`height`/`margin-*`/`padding-*`/inset values of every
/// element's principal box with their used px values (CSSOM §6.7.2
/// resolved value) — see [`resolved_geometry`] — and percentages / viewport
/// units resolve against it.
pub fn collect_computed_styles(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
    counters: Option<&CounterMap>,
    viewport: lumen_core::geom::Size,
) -> std::collections::HashMap<u32, StyleMap> {
    let mut out = std::collections::HashMap::new();
    collect_computed_styles_rec(doc, root, resolved_geometry::GeomCtx::root(viewport), viewport, &mut out);
    if let Some(counters) = counters {
        // CQ-SCROLL-STATE: box-less elements inside a query container, restyled by its
        // `@container` rules — they replace the segment-derived approximation too.
        for (id, style) in counters.cq_boxless() {
            let idx = id.index() as u32;
            let mut m = StyleMap::of_style(style);
            if !out.contains_key(&idx) && style.display != Display::Contents {
                m.set(resolved_geometry::BOXLESS_KEY, "1".to_owned());
            }
            out.insert(idx, m);
        }
        for i in 0..doc.len() {
            let idx = i as u32;
            if out.contains_key(&idx) {
                continue;
            }
            publish_boxless_computed_style(counters, idx, &mut out);
        }
    }
    out
}

/// BUG-1191: publishes the cascaded style of node `idx` — an element with no box
/// (empty inline, `display: none` / `content-visibility: hidden` descendant) —
/// from `counters`, marked ([`resolved_geometry::BOXLESS_KEY`]) so «rendered»
/// checks can skip it. A no-op for a node `counters` holds no style for.
///
/// BUG-1480/BUG-1684: also the incremental flush's backfill — it evicts the
/// entries of the boxes it rebuilt, and an element that *lost* its box in that
/// restyle (`div.style.display = 'inline'`, a closed `<details>`' content) is
/// not walked by the box collector, so nothing would refill its entry.
pub fn publish_boxless_computed_style(
    counters: &CounterMap,
    idx: u32,
    out: &mut std::collections::HashMap<u32, StyleMap>,
) {
    let Some(style) = counters.style_arc(lumen_dom::NodeId::from_index(idx as usize)) else { return };
    let mut m = StyleMap::of_style(&style);
    if style.display != Display::Contents {
        m.set(resolved_geometry::BOXLESS_KEY, "1".to_owned());
    }
    out.insert(idx, m);
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

// LAYOUT-1 срез 3: явный стек вместо рекурсии — этот обход исполняется на
// каждом `getComputedStyle`, т.е. потенциально на каждый relayout, а не
// только на первичном layout (BUG-987). Чистый pre-order без пост-обработки
// после цикла по детям — LIFO-стек с детьми в обратном порядке сохраняет
// тот же порядок посещения, что и рекурсия ("первый бокс в tree order
// побеждает" остаётся верным).
fn collect_computed_styles_rec(
    doc: &lumen_dom::Document,
    root: &LayoutBox,
    root_ctx: resolved_geometry::GeomCtx,
    viewport: lumen_core::geom::Size,
    out: &mut std::collections::HashMap<u32, StyleMap>,
) {
    let mut stack: Vec<(&LayoutBox, resolved_geometry::GeomCtx)> = vec![(root, root_ctx)];
    while let Some((b, ctx)) = stack.pop() {
        collect_computed_styles_box(doc, b, &ctx, viewport, out);
        let child_ctx = resolved_geometry::child_ctx(b, &ctx, viewport);
        stack.extend(b.children.iter().rev().map(|c| (c, child_ctx)));
    }
}

/// One box's contribution to [`collect_computed_styles`]: its own entry (first box
/// in tree order wins) and the entries of the text and plain inline elements an
/// `InlineRun` flattens. `ctx` is the geometry context `b` itself is resolved in.
fn collect_computed_styles_box(
    doc: &lumen_dom::Document,
    b: &LayoutBox,
    ctx: &resolved_geometry::GeomCtx,
    viewport: lumen_core::geom::Size,
    out: &mut std::collections::HashMap<u32, StyleMap>,
) {
    collect_computed_styles_parts(doc, b, ctx, viewport, true, true, out);
}

/// [`collect_computed_styles_box`] with either half optional (BUG-935 срез 59):
/// `own` is the box's own entry, `segments` the entries of the text and inline
/// elements an `InlineRun` flattens. The caller passes `false` for a half whose
/// published entries it has proved unchanged.
fn collect_computed_styles_parts(
    doc: &lumen_dom::Document,
    b: &LayoutBox,
    ctx: &resolved_geometry::GeomCtx,
    viewport: lumen_core::geom::Size,
    own: bool,
    segments: bool,
    out: &mut std::collections::HashMap<u32, StyleMap>,
) {
    if own {
        out.entry(b.node.index() as u32).or_insert_with(|| {
            let mut m = StyleMap::of_style(&b.style);
            resolved_geometry::apply_used_geometry(&mut m, b, ctx, viewport);
            m
        });
    }
    if !segments {
        return;
    }
    if let box_tree::BoxKind::InlineRun { segments, .. } = &b.kind {
        for seg in segments {
            // `NodeId(0)` is the document root, which `InlineSegment::source_node`
            // uses for generated content with no DOM origin.
            if seg.source_node.index() == 0 {
                continue;
            }
            out.entry(seg.source_node.index() as u32)
                .or_insert_with(|| selector_query::inline_segment_style_map(&seg.style).into());
            // BUG-488: publish the full property map for every plain inline
            // element this segment is nested inside — see `collect_computed_styles`'s
            // doc comment for the approximation this relies on.
            for anc in inline_element_ancestors(doc, seg.source_node, b.node) {
                out.entry(anc.index() as u32)
                    .or_insert_with(|| StyleMap::of_style_copy(&seg.style));
            }
        }
    }
}

/// Sibling of [`collect_computed_styles`] for CSS Pseudo-Elements L4 boxes and
/// segments (`::before`, `::after`, `::first-line`, `::first-letter` —
/// CSSOM-6/BUG-490). Keyed by `(owner NodeId index, pseudo name)`, using the
/// same lowercase no-colon spelling `compute_pseudo_element_style` already
/// takes ("before"/"after"/"first-line"/"first-letter"), so a JS shim that
/// normalizes its `pseudoElt` argument the same way needs no translation table.
///
/// Reusing `collect_computed_styles`'s map is not an option: that map is keyed
/// by `NodeId` alone, "first box in tree order wins", so an element and its own
/// `::before` — same `node`, different box — would collide.
///
/// `display` is overridden to the CSS Pseudo-elements L4 / CSS Display L3
/// §placement *used* value instead of the raw cascaded one: `::first-line`
/// always reports `inline` (it can never be floated or out-of-flow), and
/// `::first-letter` reports `block` when floated or absolutely/fixed
/// positioned and `inline` otherwise — neither ever exposes the author's own
/// `display`. `::before`/`::after` are ordinary boxes and report their
/// cascaded `display` as-is.
pub fn collect_pseudo_computed_styles(
    root: &LayoutBox,
) -> std::collections::HashMap<(u32, String), std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    collect_pseudo_computed_styles_rec(root, root.node, &mut out);
    out
}

fn pseudo_kind_name(kind: PseudoKind) -> Option<&'static str> {
    match kind {
        PseudoKind::Before => Some("before"),
        PseudoKind::After => Some("after"),
        PseudoKind::FirstLine => Some("first-line"),
        PseudoKind::FirstLetter => Some("first-letter"),
        PseudoKind::ScrollMarker => Some("scroll-marker"),
        PseudoKind::ScrollMarkerGroup => Some("scroll-marker-group"),
        PseudoKind::ScrollButtonBlockStart => Some("scroll-button(block-start)"),
        PseudoKind::ScrollButtonInlineStart => Some("scroll-button(inline-start)"),
        PseudoKind::ScrollButtonInlineEnd => Some("scroll-button(inline-end)"),
        PseudoKind::ScrollButtonBlockEnd => Some("scroll-button(block-end)"),
        PseudoKind::None | PseudoKind::Marker => None,
    }
}

/// The *used* `display` CSS Pseudo-elements L4 mandates for `kind`, or `None`
/// to keep the box's own cascaded value (`::before`/`::after`).
fn pseudo_used_display(kind: PseudoKind, style: &ComputedStyle) -> Option<&'static str> {
    match kind {
        PseudoKind::FirstLine => Some("inline"),
        PseudoKind::FirstLetter => {
            let out_of_flow =
                style.float_side != FloatSide::None || matches!(style.position, Position::Absolute | Position::Fixed);
            Some(if out_of_flow { "block" } else { "inline" })
        }
        PseudoKind::None
        | PseudoKind::Before
        | PseudoKind::After
        | PseudoKind::Marker
        | PseudoKind::ScrollMarker
        | PseudoKind::ScrollMarkerGroup
        | PseudoKind::ScrollButtonBlockStart
        | PseudoKind::ScrollButtonInlineStart
        | PseudoKind::ScrollButtonInlineEnd
        | PseudoKind::ScrollButtonBlockEnd => None,
    }
}

/// The published style of a `::marker` box (BUG-1446): its own cascaded style plus the used
/// values CSSOM reports for it — `display` is `inline` for an inside marker and a block
/// container (`block`) for an outside one (CSS Lists L3 §3.1), `width`/`height` the marker
/// box's resolved size.
fn marker_style_map(b: &LayoutBox, position: ListStylePosition) -> std::collections::HashMap<String, String> {
    let mut map = computed_style_to_map(&b.style);
    let display = if position == ListStylePosition::Outside { "block" } else { "inline" };
    map.insert("display".to_string(), display.to_string());
    map.insert("width".to_string(), selector_query::px_str(b.rect.width));
    map.insert("height".to_string(), selector_query::px_str(b.rect.height));
    map
}

fn pseudo_style_map(style: &ComputedStyle, kind: PseudoKind) -> std::collections::HashMap<String, String> {
    let mut map = computed_style_to_map(style);
    if let Some(used_display) = pseudo_used_display(kind, style) {
        map.insert("display".to_string(), used_display.to_string());
    }
    map
}

/// `container_owner` is the nearest ancestor box's own `node` — the box whose
/// `children` list `b` lives in. Needed because `::first-letter`'s box/segment
/// (`extract_first_letter_float`/`apply_first_letter_pseudo`) carries the
/// *text node's* `NodeId` in `origin.node`/`source_node` (Selection/Range
/// needs that identity), not the block element CSS attributes `::first-letter`
/// to — that owner is whichever box directly contains it, i.e. `container_owner`.
/// `::before`/`::after`/`::first-line` boxes already carry the correct owner
/// in `origin.node`/`source_node` and ignore this parameter.
/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion: nothing after the loop over `children` reads a value the walk
/// produced, so this is the same safe mechanical class LAYOUT-1 already
/// converted. `container_owner` is threaded down like a plain parameter
/// (each node passes its own `b.node` to its children, never reads it back),
/// so a `(&LayoutBox, NodeId)` stack entry reproduces it exactly. `out` uses
/// `or_insert_with` (first write wins), so document order only matters
/// within that guarantee — preserved anyway by pushing children in reverse.
fn collect_pseudo_computed_styles_rec(
    root: &LayoutBox,
    root_owner: lumen_dom::NodeId,
    out: &mut std::collections::HashMap<(u32, String), std::collections::HashMap<String, String>>,
) {
    let mut stack: Vec<(&LayoutBox, lumen_dom::NodeId)> = vec![(root, root_owner)];
    while let Some((b, container_owner)) = stack.pop() {
        collect_pseudo_computed_styles_box(b, container_owner, None, out);
        for child in b.children.iter().rev() {
            stack.push((child, b.node));
        }
    }
}

/// One box's contribution to [`collect_pseudo_computed_styles`]: its own entry when it is a
/// pseudo-element box, and the entries of the pseudo-element segments an `InlineRun` flattens.
/// `container_owner` is the node of the box whose `children` list `b` lives in. `held` — the
/// entries already published, which are left as they are (BUG-935 срез 96: a box the flush did
/// not restyle has the entry it had).
pub(crate) fn collect_pseudo_computed_styles_box(
    b: &LayoutBox,
    container_owner: lumen_dom::NodeId,
    held: Option<&scoped_collect::PseudoMaps>,
    out: &mut std::collections::HashMap<(u32, String), std::collections::HashMap<String, String>>,
) {
    // `::marker` — the list item's marker box (`BoxKind::Marker`, not a `BoxRole::Pseudo`).
    if let box_tree::BoxKind::Marker { position, .. } = &b.kind {
        let key = (b.node.index() as u32, "marker".to_string());
        if !held.is_some_and(|h| h.contains_key(&key)) {
            out.entry(key).or_insert_with(|| marker_style_map(b, *position));
        }
    }
    if let BoxRole::Pseudo(kind) = b.origin.role
        && let Some(name) = pseudo_kind_name(kind)
    {
        let owner = if kind == PseudoKind::FirstLetter {
            container_owner
        } else {
            b.origin.node.unwrap_or(b.node)
        };
        let key = (owner.index() as u32, name.to_string());
        if !held.is_some_and(|h| h.contains_key(&key)) {
            out.entry(key).or_insert_with(|| pseudo_style_map(&b.style, kind));
        }
        // `::scroll-button(down)` etc.: the physical spelling is the same box (the writing mode
        // of its originating element, which the button inherits, picks the keyword).
        if matches!(
            kind,
            PseudoKind::ScrollButtonBlockStart
                | PseudoKind::ScrollButtonInlineStart
                | PseudoKind::ScrollButtonInlineEnd
                | PseudoKind::ScrollButtonBlockEnd
        ) {
            let (_, physical) = box_tree::button_keywords(kind, &b.style);
            let key = (owner.index() as u32, format!("scroll-button({physical})"));
            if !held.is_some_and(|h| h.contains_key(&key)) {
                out.entry(key).or_insert_with(|| pseudo_style_map(&b.style, kind));
            }
        }
    }
    if let box_tree::BoxKind::InlineRun { segments, .. } = &b.kind {
        for seg in segments {
            // `NodeId(0)` is the "no DOM origin" sentinel `make_content_image_segment`
            // uses for a pseudo-element whose sole content is `url(...)` — no owner
            // to key on, so it is silently skipped (rare: image-only generated content).
            if seg.source_node.index() == 0 {
                continue;
            }
            if let Some(name) = pseudo_kind_name(seg.pseudo_kind) {
                let owner = if seg.pseudo_kind == PseudoKind::FirstLetter {
                    container_owner
                } else {
                    seg.source_node
                };
                let key = (owner.index() as u32, name.to_string());
                if !held.is_some_and(|h| h.contains_key(&key)) {
                    out.entry(key).or_insert_with(|| pseudo_style_map(&seg.style, seg.pseudo_kind));
                }
            }
        }
    }
}

/// The pseudo-element names [`collect_pseudo_computed_styles`] keys by.
const PSEUDO_NAMES: [&str; 15] = [
    "before",
    "marker",
    "after",
    "first-line",
    "first-letter",
    "scroll-marker",
    "scroll-marker-group",
    "scroll-button(block-start)",
    "scroll-button(inline-start)",
    "scroll-button(inline-end)",
    "scroll-button(block-end)",
    "scroll-button(up)",
    "scroll-button(down)",
    "scroll-button(left)",
    "scroll-button(right)",
];

/// Drops every pseudo-element entry owned by the node with `NodeId::index` `node`
/// (BUG-935 срез 95: the scoped flush evicts the entries of what it rebuilds).
pub fn forget_pseudo_computed_styles(
    out: &mut std::collections::HashMap<(u32, String), std::collections::HashMap<String, String>>,
    node: u32,
) {
    if out.is_empty() {
        return;
    }
    for name in PSEUDO_NAMES {
        out.remove(&(node, name.to_string()));
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
///
/// `viewport` and each node's own resolved `font_size` are the `em`/`vw`/`vh`
/// basis for a `var(ident(...), ...)` (BUG-500) numeric argument. Since the
/// resolved map is cached by the source allocation's identity, not by
/// `font_size`, a value that depends on `1em` and is shared by two nodes with
/// different font-sizes resolves against whichever of them is visited first —
/// the same trade-off BUG-732's own memoization already makes for every other
/// substitution, just newly reachable through this one draft function.
pub fn collect_custom_properties(
    root: &LayoutBox,
    viewport: lumen_core::geom::Size,
) -> std::collections::HashMap<u32, std::sync::Arc<std::collections::HashMap<String, String>>> {
    collect_custom_properties_for(root, None, viewport)
}

/// [`collect_custom_properties`] that also publishes the custom properties of the
/// elements that own no box — plain inline elements, `display: none` / `contents`
/// ones — below a query container (CQ-SCROLL-STATE: where `@container` rules set a
/// `--x` that `getComputedStyle` has to read back; [`CounterMap::cq_boxless`]).
/// Without `counters`, and outside query containers, those elements stay absent.
pub fn collect_custom_properties_for(
    root: &LayoutBox,
    counters: Option<&CounterMap>,
    viewport: lumen_core::geom::Size,
) -> std::collections::HashMap<u32, std::sync::Arc<std::collections::HashMap<String, String>>> {
    let mut out = std::collections::HashMap::new();
    // Keyed by the address of the source allocation — identity, never read.
    let mut resolved: std::collections::HashMap<
        usize,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    > = std::collections::HashMap::new();
    collect_custom_properties_rec(root, viewport, &mut out, &mut resolved);
    if let Some(counters) = counters {
        for (id, style) in counters.cq_boxless() {
            let idx = id.index() as u32;
            if !out.contains_key(&idx) && !style.custom_props.is_empty() {
                out.insert(idx, resolve_custom_props(style, viewport, &mut resolved));
            }
        }
    }
    out
}

/// `style.custom_props` with every `var()` expanded, one resolution per distinct
/// allocation (`resolved`, keyed by the allocation's address).
fn resolve_custom_props(
    style: &ComputedStyle,
    viewport: lumen_core::geom::Size,
    resolved: &mut std::collections::HashMap<
        usize,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    >,
) -> std::sync::Arc<std::collections::HashMap<String, String>> {
    let key = style.custom_props.as_ptr() as usize;
    if let Some(m) = resolved.get(&key) {
        return std::sync::Arc::clone(m);
    }
    let raw = style.custom_props.shared();
    let em_basis = style.font_size;
    let m = std::sync::Arc::new(
        raw.iter()
            .map(|(name, value)| {
                let computed = crate::style::expand_vars_and_env(value, &raw, em_basis, viewport).unwrap_or_default();
                (name.clone(), computed.trim().to_string())
            })
            .collect::<std::collections::HashMap<String, String>>(),
    );
    resolved.insert(key, std::sync::Arc::clone(&m));
    m
}

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion: nothing after the loop over `children` reads a value the walk
/// produced, so this is the same safe mechanical class LAYOUT-1 already
/// converted. "First box in tree order wins" (below) depends on left-to-right
/// pre-order — preserved by pushing children in reverse so the LIFO stack
/// pops them in source order.
fn collect_custom_properties_rec(
    root: &LayoutBox,
    viewport: lumen_core::geom::Size,
    out: &mut std::collections::HashMap<
        u32,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    >,
    resolved: &mut std::collections::HashMap<
        usize,
        std::sync::Arc<std::collections::HashMap<String, String>>,
    >,
) {
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        collect_custom_properties_box(b, viewport, None, out, resolved);
        for child in b.children.iter().rev() {
            stack.push(child);
        }
    }
}

/// One box's contribution to [`collect_custom_properties`]. `held` — the entries already
/// published, which are left as they are (BUG-935 срез 96).
pub(crate) fn collect_custom_properties_box(
    b: &LayoutBox,
    viewport: lumen_core::geom::Size,
    held: Option<&scoped_collect::CustomPropMaps>,
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
    if b.style.custom_props.is_empty() || held.is_some_and(|h| h.contains_key(&(b.node.index() as u32))) {
        return;
    }
    let map = resolve_custom_props(&b.style, viewport, resolved);
    out.entry(b.node.index() as u32).or_insert(map);
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

/// GAP-LAYOUTSHIFT срез 5 (BUG-809): geometry snapshot for
/// `compute_layout_shift_score`, deliberately *not* [`collect_layout_rects`].
///
/// Two approximation gaps that formula's doc-comment flagged as follow-up
/// work, both traced to reusing `collect_layout_rects`'s gBCR-flavoured
/// geometry for a spec that wants different inputs:
///
/// * **Own-node CSS `transform`/`translate` must not count.** L1 §3.1 scores
///   *layout* shifts; a composited transform moves the paint output without
///   touching the border box a reflow would recompute (`translate-change.html`
///   expects score `0` for a `translate` edit). `collect_layout_rects` applies
///   [`forward_box_transform`](crate::property_trees::forward_box_transform)
///   because `getBoundingClientRect()` *should* report the transformed box —
///   right for that caller, wrong for this one. This walk uses the raw
///   `b.rect` instead.
/// * **`visibility: hidden` elements must not count.** L1 §5.2.4 only scores
///   elements that were actually rendered; a hidden element moving is invisible
///   by definition (`visibility-hidden.html` expects score `0` for a `top`
///   edit on a `visibility: hidden` box). Skipped here by omission — an
///   element hidden in both the previous and current snapshot is absent from
///   both maps, so [`compute_layout_shift_score`](crate) 's
///   `prev.get(node)` miss treats it exactly like "entered/left the tree":
///   not a shift.
///
/// Inline fragments (BUG-488's `InlineRun` union) are out of scope here —
/// neither probe exercises them, and the plain-block walk below is what the
/// filed approximation gaps need.
///
/// GAP-LAYOUTSHIFT срез 6 (BUG-809): a third exclusion, `opacity: 0`
/// (L1 §5.2.4, same "not actually rendered" clause `visibility: hidden`
/// already used). Unlike `visibility`, CSS `opacity` does not inherit — a
/// node's own computed value says nothing about whether an *ancestor's*
/// zero opacity already collapses the whole subtree to transparent paint
/// output (`opacity-zero.html`: parent `opacity: 0`, child `opacity: 0.5`,
/// child's own shift still must not count). So this walk threads an
/// `ancestor_opacity_zero` flag down explicitly instead of trusting each
/// node's own field, mirroring how the cascade would have propagated
/// `visibility: hidden` if `opacity` inherited too.
///
/// GAP-LAYOUTSHIFT срез 7 (BUG-809): a fourth exclusion, `position: fixed`/
/// `sticky` (`ignore-fixed-and-sticky.html`). Their containing block is the
/// viewport, so a relayout while the page is scrolled recomputes their page-
/// space `b.rect` by the scroll delta even though nothing moved on screen —
/// without this exclusion `compute_layout_shift_score` would score every
/// scroll-triggered relayout as a shift of the fixed/sticky element's full
/// travel distance. Excluded the same way `visibility: hidden` already is
/// (omission from the map, not a flag threaded to children — unlike
/// `opacity`, `position` does not cascade a "not rendered" state to
/// descendants, so their own boxes still participate normally).
pub fn collect_layout_shift_rects(root: &LayoutBox) -> std::collections::HashMap<u32, [f32; 4]> {
    let mut out = std::collections::HashMap::new();
    collect_layout_shift_rects_rec(root, false, &mut out);
    out
}

fn collect_layout_shift_rects_rec(
    root: &LayoutBox,
    ancestor_opacity_zero: bool,
    out: &mut std::collections::HashMap<u32, [f32; 4]>,
) {
    // (box, an ancestor has opacity 0, an ancestor is the containing block of `fixed` boxes)
    let mut stack: Vec<(&LayoutBox, bool, bool)> = vec![(root, ancestor_opacity_zero, false)];
    while let Some((b, opacity_zero, in_fixed_cb)) = stack.pop() {
        let opacity_zero = opacity_zero || b.style.opacity <= 0.0;
        let not_rendered = !matches!(b.style.visibility, Visibility::Visible) || opacity_zero;
        // A `fixed` box under a transformed/filtered ancestor scrolls with it
        // (css-transforms-1 §2), so only a viewport-pinned one is skipped.
        let scroll_pinned = match b.style.position {
            Position::Fixed => !in_fixed_cb,
            Position::Sticky => true,
            _ => false,
        };
        let in_fixed_cb = in_fixed_cb || resolved_geometry::contains_fixed_descendants(&b.style);
        if not_rendered || scroll_pinned {
            stack.extend(b.children.iter().rev().map(|c| (c, opacity_zero, in_fixed_cb)));
            continue;
        }
        let r = b.rect;
        out.entry(b.node.index() as u32)
            .or_insert([r.x, r.y, r.width, r.height]);
        stack.extend(b.children.iter().rev().map(|c| (c, opacity_zero, in_fixed_cb)));
    }
}

// LAYOUT-1 срез 3: явный стек вместо рекурсии — `getBoundingClientRect`
// пересчитывается на каждый relayout (BUG-987). Pre-order без пост-обработки
// после цикла по детям, LIFO-стек с детьми в обратном порядке сохраняет
// порядок посещения — "первый бокс в tree order побеждает" остаётся верным.
fn collect_layout_rects_rec(
    doc: &lumen_dom::Document,
    root: &LayoutBox,
    out: &mut std::collections::HashMap<u32, [f32; 4]>,
) {
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        collect_layout_rects_box(doc, b, out);
        // `content-visibility: hidden` (BUG-1400): the skipped subtree, laid out as a probe.
        stack.extend(b.cv_hidden.iter().rev());
        stack.extend(b.children.iter().rev());
    }
}

/// The rect `getBoundingClientRect` publishes for `b` itself: its border box, a
/// `<ruby>`'s base-level box, and the AABB of its own transform (BUG-540).
fn box_published_rect(b: &LayoutBox) -> lumen_core::geom::Rect {
    let own = match &b.kind {
        BoxKind::Ruby { .. } => ruby::ruby_base_rect(b).unwrap_or(b.rect),
        _ => b.rect,
    };
    match forward_box_transform(b) {
        Some(m) => transformed_aabb(&own, &m),
        None => own,
    }
}

/// One box's contribution to [`collect_layout_rects`] — its own entry and, for an
/// `InlineRun`, the union entries of the plain inline elements inside it. Shared
/// by the whole-tree walk and the spine boxes of [`ScopedCollection`].
fn collect_layout_rects_box(
    doc: &lumen_dom::Document,
    b: &LayoutBox,
    out: &mut std::collections::HashMap<u32, [f32; 4]>,
) {
    // A single `NodeId` can own more than one box: an element with inline content
    // gets an anonymous block/line box for that content, and the box tree keeps the
    // element's own `NodeId` on it. The traversal visits the principal box before
    // its descendants, so `or_insert` keeps the element's own border box; plain
    // `insert` used to hand JS the last (inner) box instead — `getBoundingClientRect`
    // on `<div style="height:20px">x</div>` answered the 19.2px line box (BUG-382).
    //
    // BUG-540: `forward_box_transform` (own `transform`/individual transform
    // properties/`offset-path`, same matrix paint composites) is applied here
    // too — a real browser's `getBoundingClientRect()` reports the *transformed*
    // box, not the flow-position `rect` paint starts from. Ancestor transforms
    // are not accumulated (out of scope of the filed repro, which transforms the
    // queried box itself, not a container above it).
    // GAP-RUBYBOX-2: a `<ruby>` answers with its base-level box only — see
    // `box_published_rect`.
    let r = box_published_rect(b);
    out.entry(b.node.index() as u32)
        .or_insert([r.x, r.y, r.width, r.height]);
    // BUG-488: plain inline elements (`<span>`, `<em>`, …) own no `LayoutBox` of
    // their own — accumulate the union of every laid-out `InlineFrag` nested
    // inside them, keyed by DOM ancestor via `inline_element_ancestors`. Line
    // y-position uses the same `font_size * line_height` uniform-line-height
    // model `selection.rs` uses to turn `lines[line_idx]` into a pixel rect.
    for_each_inline_frag_ancestor(doc, b, |anc, [fx1, fy1, fx2, fy2]| {
        out.entry(anc.index() as u32)
            .and_modify(|cur| {
                let cx1 = cur[0].min(fx1);
                let cy1 = cur[1].min(fy1);
                let cx2 = (cur[0] + cur[2]).max(fx2);
                let cy2 = (cur[1] + cur[3]).max(fy2);
                *cur = [cx1, cy1, cx2 - cx1, cy2 - cy1];
            })
            .or_insert([fx1, fy1, fx2 - fx1, fy2 - fy1]);
    });
}

/// For an `InlineRun` box: calls `f(element, [x1, y1, x2, y2])` for every plain inline element
/// (see [`inline_element_ancestors`]) once per laid-out fragment span inside it — the pieces
/// whose union is the element's `getBoundingClientRect` (BUG-488), also what an `anchor-name`
/// on such an element anchors to (BUG-1477). Other boxes: no calls.
pub(crate) fn for_each_inline_frag_ancestor(
    doc: &lumen_dom::Document,
    b: &LayoutBox,
    mut f: impl FnMut(lumen_dom::NodeId, [f32; 4]),
) {
    let BoxKind::InlineRun { lines, .. } = &b.kind else { return };
    let line_h = b.used_line_height;
    for (line_idx, line) in lines.iter().enumerate() {
        let line_y = b.rect.y + line_idx as f32 * line_h;
        // GAP-HLHITTEST: per source span, so an inline element whose
        // words `wrap_inline_run` merged into its same-style
        // neighbour's fragment still gets its own geometry.
        for (frag, span) in line.iter().flat_map(|f| {
            text_geometry::frag_source_spans(f).into_iter().map(move |s| (f, s))
        }) {
            let fx1 = b.rect.x + frag.x + span.x;
            let rect = [fx1, line_y, fx1 + span.width, line_y + line_h];
            for anc in inline_element_ancestors(doc, span.source_node, b.node) {
                f(anc, rect);
            }
        }
    }
}

// ──────────────── collect_client_rects ────────────────

/// Nodes that own at least one real `LayoutBox` of their own — as opposed to a
/// plain inline element (`<span>`, `<em>`, …) that BUG-488 flattens into the
/// enclosing `InlineRun`'s fragments and never gives a box to. Used by
/// [`collect_client_rects_rec`] to tell "this owner already has its one true
/// rect from its own box" (an inline-block nested inside a line, still an
/// `InlineFrag` for line-height purposes) apart from "this owner has no box
/// and needs one rect per line it appears on".
// LAYOUT-1 срез 3: явный стек вместо рекурсии — см. `collect_layout_rects_rec`.
fn collect_boxed_node_ids(root: &LayoutBox, out: &mut std::collections::HashSet<u32>) {
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        out.insert(b.node.index() as u32);
        stack.extend(b.cv_hidden.iter().rev());
        stack.extend(b.children.iter().rev());
    }
}

/// Walks the layout tree and returns a map of `NodeId index → one rect per CSS
/// fragment` (border-box, viewport-relative CSS px) — the per-fragment geometry
/// `Element.prototype.getClientRects()`/`getBoxQuads()` need (BUG-1007) and
/// [`collect_layout_rects`] deliberately does not give them: that function
/// unions every fragment of a node into a single summarising rect (BUG-488),
/// right for `getBoundingClientRect` but spec-wrong for `getClientRects`, which
/// must answer one `DOMRect` per line a multi-line inline element spans (CSSOM
/// View §6).
///
/// A node that owns a real `LayoutBox` (the common case — block, inline-block,
/// replaced, …) always gets exactly one rect, same as [`collect_layout_rects`].
/// A plain inline element that owns no box of its own (BUG-488) gets one rect
/// per line it appears on, each the union of every `InlineFrag` reaching it on
/// that line (bidi runs, a nested inline-block splitting it mid-line, …) via
/// [`inline_element_ancestors`] — the same union [`collect_layout_rects_rec`]
/// computes, just kept apart per line instead of merged across all of them.
pub fn collect_client_rects(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
) -> std::collections::HashMap<u32, Vec<[f32; 4]>> {
    let mut boxed = std::collections::HashSet::new();
    collect_boxed_node_ids(root, &mut boxed);
    let mut out = std::collections::HashMap::new();
    collect_client_rects_rec(doc, root, &boxed, &mut out);
    out
}

// LAYOUT-1 срез 3: явный стек вместо рекурсии — `getClientRects`/`getBoxQuads`
// пересчитываются на каждый relayout (BUG-987). Pre-order без пост-обработки
// после цикла по детям, LIFO-стек с детьми в обратном порядке сохраняет
// порядок посещения — "первый бокс в tree order побеждает" остаётся верным.
fn collect_client_rects_rec(
    doc: &lumen_dom::Document,
    root: &LayoutBox,
    boxed: &std::collections::HashSet<u32>,
    out: &mut std::collections::HashMap<u32, Vec<[f32; 4]>>,
) {
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        collect_client_rects_box(doc, b, boxed, out);
        stack.extend(b.cv_hidden.iter().rev());
        stack.extend(b.children.iter().rev());
    }
}

/// One box's contribution to [`collect_client_rects`]. `boxed` must cover every
/// node owning a real box among `b`'s descendants (see [`collect_boxed_node_ids`]).
fn collect_client_rects_box(
    doc: &lumen_dom::Document,
    b: &LayoutBox,
    boxed: &std::collections::HashSet<u32>,
    out: &mut std::collections::HashMap<u32, Vec<[f32; 4]>>,
) {
    // Same first-box-wins ordering as `collect_layout_rects_rec` (BUG-382): the
    // traversal visits a node's own box before descending into its children.
    let r = match &b.kind {
        BoxKind::Ruby { .. } => ruby::ruby_base_rect(b).unwrap_or(b.rect),
        _ => b.rect,
    };
    out.entry(b.node.index() as u32)
        .or_insert_with(|| vec![[r.x, r.y, r.width, r.height]]);
    if let BoxKind::InlineRun { lines, .. } = &b.kind {
        let line_h = b.style.font_size * b.style.line_height;
        for (line_idx, line) in lines.iter().enumerate() {
            let line_y = b.rect.y + line_idx as f32 * line_h;
            // Frags on the SAME line belonging to the same owner (a bidi split,
            // an inline element reopening around a nested box, …) merge into
            // one rect — one rect per CSS *fragment*, and a fragment is a
            // line, not a frag.
            let mut per_owner_this_line: std::collections::HashMap<u32, [f32; 4]> =
                std::collections::HashMap::new();
            for (frag, span) in line.iter().flat_map(|f| {
                text_geometry::frag_source_spans(f).into_iter().map(move |s| (f, s))
            }) {
                let fx1 = b.rect.x + frag.x + span.x;
                let fy1 = line_y;
                let fx2 = fx1 + span.width;
                let fy2 = fy1 + line_h;
                for anc in inline_element_ancestors(doc, span.source_node, b.node) {
                    per_owner_this_line
                        .entry(anc.index() as u32)
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
            for (owner, rect) in per_owner_this_line {
                // An owner with a real box of its own (an inline-block nested
                // in this line, still an `InlineFrag` here for line-height
                // purposes) already has its one true rect from that box — do
                // not also append this approximate line-derived rect on top
                // of it.
                if !boxed.contains(&owner) {
                    out.entry(owner).or_default().push(rect);
                }
            }
        }
    }
}

/// Update the scroll position of a node in the layout tree.
///
/// Walks the tree to find the box with `node`, clamps `(x, y)` to the valid
/// scroll range and updates `LayoutBox.scroll_x / scroll_y`. Returns `true`
/// if found.
///
/// The range is `[min_x, max_x - clip_width] × [min_y, max_y - clip_height]`,
/// where `(min_x, max_x)`/`(min_y, max_y)` come from [`scrollable_extent_x`]/
/// [`scrollable_extent_y`] — `min_x`/`min_y` are 0.0 in the common case but
/// go negative when in-flow content extends left/up of the padding edge
/// (`writing-mode: vertical-rl`, BUG-504). `clip_w`/`clip_h` are the
/// *padding*-box size (not `LayoutBox::rect`'s border-box — a nonzero border
/// previously let the clamp's upper bound run `2 × border` short of the true
/// maximum). Per axis, `overflow: clip` (CSS Overflow L3 §3.4, "disables the
/// scrolling machinery outright") overrides the computed range entirely and
/// pins the offset to exactly `0.0`, regardless of content size or the
/// requested `x`/`y` — `clip` still establishes a scroll container's
/// geometry (`scrollWidth` etc. via [`content_width`]) but accepts no scroll
/// offset at all, unlike `hidden`/`scroll`/`auto` which are still
/// programmatically scrollable.
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

/// BUG-1211: locate every box in `root`'s tree whose `NodeId` is a member of
/// `roots`, without descending further once one is found — its whole subtree
/// is the caller's unit of work, same "stop at the topmost member" rule
/// [`crate::incremental::extract_clean_subtrees`] already uses for the same
/// reason. A single combined pass shared across every same-tick collector
/// that needs to visit exactly the touched subtrees (`FlushHandles::
/// maybe_flush`'s incremental branch) — cheaper than one [`find_box_by_node`]
/// call per root (each of which independently re-walks from the document
/// root), and, more importantly, the search cost is paid once instead of
/// once per collector.
///
/// Same first-match-in-pre-order ambiguity as [`find_box_by_node`] when a
/// `NodeId` labels more than one box (an anonymous wrapper and the element's
/// own box) — pre-order visits the outer/element box first, matching
/// `collect_layout_rects_rec`'s BUG-382 "first box in tree order wins" rule.
pub fn find_dirty_root_boxes<'a, S: std::hash::BuildHasher>(
    root: &'a LayoutBox,
    roots: &std::collections::HashSet<lumen_dom::NodeId, S>,
) -> Vec<&'a LayoutBox> {
    let mut out = Vec::new();
    if roots.is_empty() {
        return out;
    }
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        if roots.contains(&b.node) {
            out.push(b);
            continue;
        }
        stack.extend(b.children.iter().rev());
    }
    out
}

/// BUG-1211: every `NodeId` owning a box anywhere in the subtrees [`find_dirty_root_boxes`] finds
/// for `roots`, as the sets the JS-visible caches evict by — the eviction-side twin of that search:
/// taken against the *previous* flush's tree before an incremental restyle discards it, so a
/// same-tick collector can remove exactly the entries a touched subtree owned (including ones for
/// nodes since removed from the DOM, which own no box in the *fresh* tree and so would otherwise
/// never be evicted) before re-inserting from the fresh subtree.
///
/// Returns `(indices, raw_ids)`: [`lumen_dom::NodeId::index`] for `layout_rects`/`client_rects`/
/// `computed_styles`, [`lumen_dom::NodeId::raw`] (index + generation) for `scroll_states`. The
/// asymmetry is pre-existing in those maps and the caller must match it exactly, not "fix" it: an
/// `.index()`-keyed set against a `.raw()`-keyed map silently fails to evict a stale entry whenever
/// a node's generation byte is nonzero (its arena slot was reused — GAP-P3GCJSDOM).
///
/// BUG-935 срез 74: one search and one walk of each subtree for both sets (they were built by four
/// walks over the same boxes: 0,36 мс of a 5 мс flush on `lenta.ru`, where the dirty root is `body`).
pub fn collect_dirty_subtree_ids<S: std::hash::BuildHasher>(
    root: &LayoutBox,
    roots: &std::collections::HashSet<lumen_dom::NodeId, S>,
) -> (lumen_core::id_hash::IdSet<u32>, lumen_core::id_hash::IdSet<u32>) {
    let mut indices = lumen_core::id_hash::IdSet::default();
    let mut raws = lumen_core::id_hash::IdSet::default();
    let mut stack: Vec<&LayoutBox> = find_dirty_root_boxes(root, roots);
    while let Some(b) = stack.pop() {
        indices.insert(b.node.index() as u32);
        raws.insert(b.node.raw());
        stack.extend(b.children.iter());
    }
    (indices, raws)
}

pub fn set_scroll_position(root: &mut LayoutBox, node: lumen_dom::NodeId, x: f32, y: f32) -> bool {
    let found = set_scroll_position_rec(root, node, x, y);
    if found {
        refresh_anchor_scroll(root);
    }
    found
}

/// CSS Anchor Positioning L1 §4.3 — copies the current `scroll_x` / `scroll_y` of every scroll
/// container an anchor-positioned box follows ([`LayoutBox::anchor_scroll`]) into that box, so
/// the next paint (or patch of a built display list) moves it with its anchor. Call after any
/// change of a container's scroll offset made without a relayout; a tree without such boxes
/// costs one read-only walk.
pub fn refresh_anchor_scroll(root: &mut LayoutBox) {
    let mut any = false;
    let mut offsets: std::collections::HashMap<lumen_dom::NodeId, (f32, f32)> = std::collections::HashMap::new();
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        any |= b.anchor_scroll.is_some();
        if b.scroll_x != 0.0 || b.scroll_y != 0.0 {
            // The principal box of a node comes first in pre-order and owns the offset.
            offsets.entry(b.node).or_insert((b.scroll_x, b.scroll_y));
        }
        stack.extend(b.children.iter());
    }
    if !any {
        return;
    }
    let mut stack: Vec<&mut LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        if let Some(a) = b.anchor_scroll.as_mut() {
            for sc in &mut a.scrollers {
                (sc.x, sc.y) = offsets.get(&sc.node).copied().unwrap_or((0.0, 0.0));
            }
        }
        stack.extend(b.children.iter_mut());
    }
}

fn set_scroll_position_rec(root: &mut LayoutBox, node: lumen_dom::NodeId, x: f32, y: f32) -> bool {
    if root.node == node {
        let pb = padding_box(root);
        root.scroll_x = if root.style.overflow_x == style::Overflow::Clip {
            0.0
        } else {
            let (min_x, max_x) = scrollable_extent_x(root);
            x.clamp(min_x, (max_x - pb.width).max(min_x))
        };
        root.scroll_y = if root.style.overflow_y == style::Overflow::Clip {
            0.0
        } else {
            let (min_y, max_y) = scrollable_extent_y(root);
            y.clamp(min_y, (max_y - pb.height).max(min_y))
        };
        return true;
    }
    for child in &mut root.children {
        if set_scroll_position_rec(child, node, x, y) {
            return true;
        }
    }
    false
}

/// Apply every `(node raw id → [scroll_x, scroll_y, ..])` of `states` to the
/// tree in ONE walk — what a loop of [`set_scroll_position`] over the map did in
/// `O(containers × boxes)` (a DFS per container, ~2 ms per flush on lenta.ru).
///
/// Same result as the loop: a node's *first* box in pre-order takes the offset
/// (a node shared by several boxes is not applied twice), clamped by
/// [`set_scroll_position`]'s own rule. The one shortcut: a request for `(0, 0)`
/// on a box already at `(0, 0)` is skipped — the clamp range always contains 0
/// (`min ≤ 0 ≤ max`, and `clip` yields 0), so it would write the same value, but
/// computing the scrollable extent walks the container's whole subtree.
pub fn restore_scroll_positions(root: &mut LayoutBox, states: &std::collections::HashMap<u32, [f32; 4]>) {
    if states.is_empty() {
        return;
    }
    let mut pending = states.len();
    let mut applied = std::collections::HashSet::new();
    let mut moved = false;
    let mut stack: Vec<&mut LayoutBox> = vec![&mut *root];
    while let Some(b) = stack.pop() {
        let node = b.node;
        if let Some(s) = states.get(&node.raw())
            && applied.insert(node.raw())
        {
            pending -= 1;
            let at_origin = b.scroll_x == 0.0 && b.scroll_y == 0.0;
            if !(at_origin && s[0] == 0.0 && s[1] == 0.0) {
                set_scroll_position_rec(b, node, s[0], s[1]);
                moved = true;
            }
            if pending == 0 {
                break;
            }
        }
        stack.extend(b.children.iter_mut().rev());
    }
    drop(stack);
    if moved {
        refresh_anchor_scroll(root);
    }
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

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion: nothing after the loop over `children` reads a value the walk
/// produced, so this is the same safe mechanical class LAYOUT-1 already
/// converted. Document order (caller-documented) is preserved by pushing
/// children in reverse so the LIFO stack pops them in source order.
fn collect_vt_names_rec(root: &LayoutBox, out: &mut Vec<(lumen_dom::NodeId, Box<str>)>) {
    use box_tree::BoxKind;
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        if matches!(b.kind, BoxKind::Skip) {
            continue;
        }
        if let Some(ref name) = b.style.view_transition_name {
            out.push((b.node, name.clone()));
        }
        for child in b.children.iter().rev() {
            stack.push(child);
        }
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
// The `::view-transition-*` pseudo-elements are parsed and cascaded
// ([`compute_view_transition_pseudo_style`]); the shell reads the whole-page
// (`root`) `animation-duration`/`-timing-function` from them. Per-name group
// morphing is not driven by this collector yet.
pub fn collect_view_transition_groups(
    root: &LayoutBox,
) -> Vec<(lumen_dom::NodeId, Box<str>, lumen_core::geom::Rect)> {
    let mut out = Vec::new();
    collect_vt_groups_rec(root, &mut out);
    out
}

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion — same class as [`collect_vt_names_rec`] above. Children are
/// pushed in reverse to keep the LIFO stack's pop order identical to the old
/// left-to-right recursion (document order is caller-documented).
fn collect_vt_groups_rec(
    root: &LayoutBox,
    out: &mut Vec<(lumen_dom::NodeId, Box<str>, lumen_core::geom::Rect)>,
) {
    use box_tree::BoxKind;
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        if matches!(b.kind, BoxKind::Skip) {
            continue;
        }
        if let Some(ref name) = b.style.view_transition_name {
            out.push((b.node, name.clone(), b.rect));
        }
        for child in b.children.iter().rev() {
            stack.push(child);
        }
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

/// Result of walking the scroll chain for one wheel delta
/// ([`resolve_scroll_chain_target`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollChainTarget {
    /// The container that takes the gesture.
    pub node: lumen_dom::NodeId,
    /// Its new horizontal offset (CSS px, clamped).
    pub new_x: f32,
    /// Its new vertical offset (CSS px, clamped).
    pub new_y: f32,
    /// `false` ⇒ the container is at its boundary and the gesture ends here
    /// without moving anything (`overscroll-behavior: contain | none`).
    pub moved: bool,
}

/// CSS Overscroll Behavior L1 §3 — walk the scroll chain under the point
/// `(x, y)` (CSS px, document-relative) from the innermost scroll container
/// outwards and pick the one that takes the gesture.
///
/// A container takes the gesture when it can move on a delta-bearing axis or
/// when its `overscroll-behavior` stops propagation at its boundary
/// ([`overscroll_should_propagate`]). A container at its boundary with `auto`
/// hands the delta to the next enclosing container. `None` ⇒ no container in
/// the chain takes it, the caller scrolls the viewport / frame.
#[must_use]
pub fn resolve_scroll_chain_target(
    containers: &[ScrollContainer],
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
) -> Option<ScrollChainTarget> {
    // Later entries are deeper in the tree, so reverse order is innermost-first.
    for c in containers.iter().rev() {
        let r = &c.clip_rect;
        if !(x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) {
            continue;
        }
        let new_x = (c.scroll_x + dx).clamp(0.0, (c.scroll_width - r.width).max(0.0));
        let new_y = (c.scroll_y + dy).clamp(0.0, (c.scroll_height - r.height).max(0.0));
        let moved_x = (new_x - c.scroll_x).abs() > f32::EPSILON;
        let moved_y = (new_y - c.scroll_y).abs() > f32::EPSILON;
        if overscroll_should_propagate(
            c.overscroll_behavior_x,
            c.overscroll_behavior_y,
            dx,
            dy,
            moved_x,
            moved_y,
        ) {
            continue;
        }
        return Some(ScrollChainTarget { node: c.node, new_x, new_y, moved: moved_x || moved_y });
    }
    None
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
use crate::style::VerticalAlign;
#[cfg(test)]
use lumen_core::geom::Size;
#[cfg(test)]
use table_grid_presentational::html_and_body;

#[cfg(test)]
#[path = "tests/fixtures_and_core_selectors.rs"]
mod fixtures_and_core_selectors;
#[cfg(test)]
pub(crate) use fixtures_and_core_selectors::{
    body_layout_box, find_box, find_by_tag, first_element_child, lay, lay_full, lay_measured,
    lay_viewport, lay_with_doc, Fixed8,
};

#[cfg(test)]
#[path = "tests/counter_style_markers.rs"]
mod counter_style_markers;
#[cfg(test)]
#[path = "tests/grid_computed_style.rs"]
mod grid_computed_style;
#[cfg(test)]
#[path = "tests/state_pseudo_classes.rs"]
mod state_pseudo_classes;
#[cfg(test)]
pub(crate) use state_pseudo_classes::element_color;
#[cfg(test)]
#[path = "tests/functional_pseudo_classes.rs"]
mod functional_pseudo_classes;

#[cfg(test)]
#[path = "tests/text_justify.rs"]
mod text_justify;
#[cfg(test)]
#[path = "tests/box_sizing_text_props.rs"]
mod box_sizing_text_props;
#[cfg(test)]
pub(crate) use box_sizing_text_props::{first_inline_run, first_inline_text};

#[cfg(test)]
#[path = "tests/visual_props_pseudo_selectors.rs"]
mod visual_props_pseudo_selectors;
#[cfg(test)]
#[path = "tests/containment_style_queries.rs"]
mod containment_style_queries;
#[cfg(test)]
pub(crate) use containment_style_queries::{style_ctx, style_ctx_with_style_props};

#[cfg(test)]
#[path = "tests/container_queries_replaced.rs"]
mod container_queries_replaced;
#[cfg(test)]
pub(crate) use container_queries_replaced::{first_image_child, lay_with_viewport, nested_p_style};

#[cfg(test)]
#[path = "tests/filter_transform_snap_mask.rs"]
mod filter_transform_snap_mask;
#[cfg(test)]
pub(crate) use filter_transform_snap_mask::first_p_style;

#[cfg(test)]
#[path = "tests/scroll_snap_masking.rs"]
mod scroll_snap_masking;

#[cfg(test)]
#[path = "tests/animation_gradient_quirks.rs"]
mod animation_gradient_quirks;

#[cfg(test)]
#[path = "tests/table_grid_presentational.rs"]
mod table_grid_presentational;
#[cfg(test)]
#[path = "tests/grid_layout.rs"]
mod grid_layout;

#[cfg(test)]
#[path = "tests/layout_generation_misc.rs"]
mod layout_generation_misc;

#[cfg(test)]
#[path = "tests/half_leading_columns_floats.rs"]
mod half_leading_columns_floats;

#[cfg(test)]
#[path = "tests/scroll_interaction_misc.rs"]
mod scroll_interaction_misc;

#[cfg(test)]
#[path = "tests/scroll_container.rs"]
mod scroll_container;

#[cfg(test)]
#[path = "tests/scroll_state_queries.rs"]
mod scroll_state_queries;

#[cfg(test)]
#[path = "tests/scroll_initial_target.rs"]
mod scroll_initial_target_tests;

#[cfg(test)]
#[path = "tests/deep_traversal_stress.rs"]
mod deep_traversal_stress;

#[cfg(test)]
#[path = "tests/heading_level_pseudo.rs"]
mod heading_level_pseudo;
