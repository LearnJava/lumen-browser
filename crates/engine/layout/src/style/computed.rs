//! `ComputedStyle` — результат каскада для одного узла: ~375 полей на все
//! поддерживаемые свойства плюс `root()` (начальные значения корня),
//! `outline_used_width()` (CSS 2.1 §17.6.1 used value) и `text_rendering_eq()`
//! (сравнение подмножества полей, влияющих на растеризацию текста).
//!
//! Перенесено батчем SPLIT-ST15 из `crates/engine/layout/src/style.rs`
//! (анкеры `struct ComputedStyle` и `impl ComputedStyle`) без правок тел.
//! Регион батча разрывный, и разрыв обязателен: объявление и реализация
//! обязаны ехать вместе (прецедент `enum PageSource`/`impl PageSource`,
//! SPLIT-SH-3a/SH-3b), а в доноре между ними лежат 2 584 строки чужих
//! типов значений.

// Долг по документации переезжает вместе с кодом (правило §2.4
// docs/tasks/p1-monolith-split-queue.md): замер `#![warn(missing_docs)]` на
// этом файле — сама `struct ComputedStyle` плюс 50 из 299 её `pub`-полей без
// `///`. Область исключения — файл. Счётчики по крейтам — docs/lint-policy.md §10.
//
// ВНИМАНИЕ, лечится не здесь: в `lumen-layout` этот allow ничего не решает —
// такой же стоит в `lib.rs:18`, а lib.rs это КОРЕНЬ крейта, поэтому его
// внутренний атрибут действует на весь крейт, а не на файл, вопреки
// собственному комментарию рядом с ним. Пока он там стоит, `missing_docs` в
// этом крейте не сработает ни в одном новом файле.
#![allow(missing_docs)]

use crate::font_palette::ResolvedFontPalette;
use crate::mathml::MathStyle;
use crate::ruby::{RubyAlign, RubyMerge, RubyPosition};
use crate::scroll_timeline::ScrollAxis;
use lumen_core::ColorSpace;

// Типы значений и табличные дефолты остаются в `style.rs` и его потомках;
// путь `crate::style::<Имя>` работает и для тех имён, которые донор сам
// втянул реэкспортом из `style/values/*`, `style/parse/*` (правило §2.1).
use crate::style::{
    AlignValue, ContentAlignExtra, AnimationComposition, AnimationDirection, AnimationFillMode, TransitionBehavior, AnimationPlayState, AnimationTimeline,
    Appearance, BackfaceVisibility, BackgroundLayer, BackgroundSpecified, BorderImage, BlockStepAlign, BlockStepInsert,
    BlockStepRound, BorderCollapse, BorderStyle, BoxShadow, CaptionSide, CaretAnimation, CaretShape, InputSecurity, Interactivity, TableLayout,
    RuleBreak, RuleInsets, RuleList, RuleOverlap, RuleVisibilityItems,
    BoxSizing, BreakValue, ClearSide, ClipPath, Color, ColorScheme, ContainerType, ContainFlags,
    Content, ContentVisibility, CssColor, CssContinue, Cursor, CustomProps, default_font_family,
    Direction, Display, DynamicRangeLimit, EmptyCells, FieldSizing, FillRule, FilterFn, FlexBasis, FlexDirection,
    FlexWrap, FloatSide, FontFeatureSetting, FontOpticalSizing, FontPalette, FontSizeAdjust,
    FontStretch, FontStyle, FontVariantCaps, FontVariantEmoji, FontVariantLigatures,
    FontVariantNumeric, FontVariantPosition, FontVariationSetting, FontWeight,
    ForcedColorAdjust, GridAutoFlow, GridLanes, GridLine, GridRepeat, GridTrackSize, NameFill, Hyphens, ImageRendering,
    InterpolateSizeMode, Isolation, IterationCount, Length, LengthOrAuto, LineBreak,
    ListStylePosition, ListStyleType, MaskLayer, MasonryAutoFlow, MixBlendMode, ObjectFit,
    ObjectPosition, OffsetRotate, OutlineColor, OutlineStyle, Overflow, OverflowAnchor, OverflowClipMarginBox, OverflowWrap,
    OverscrollBehavior, PointerEvents, Position, PositionComponent, PrintColorAdjust, Quotes,
    Resize, ScrollMarkerGroup, ScrollTargetGroup, ScrollbarGutter, ScrollbarWidth, ScrollBehavior,
    ScrollInitialTarget, ScrollSnapAlign, ScrollSnapStop, ScrollSnapType, ShapeOutside, StrokeLinecap, StrokeLinejoin,
    SvgPaint, SvgPaintOrder,
    TextAlign, TextAlignLast, TextCssomExtra, TextDecorationLine, TextDecorationSkipInk, TextDecorationSkipSpaces, TextDecorationStyle,
    TextDecorationThickness, TextEmphasisPosition, TextEmphasisStyle, TextOrientation,
    TextOverflow, TextShadow, TextSizeAdjust, TextTransform, TextUnderlinePosition, TextWrapMode, TextWrapStyle,
    TimingFunction, TouchAction, TransformBox, TransformFn, TransformStyle, UnicodeBidi, UserSelect,
    VerticalAlign, Visibility, WebkitBoxAlign, WebkitBoxDirection, WebkitBoxOrient, WebkitBoxPack, WhiteSpace, WhiteSpaceCollapse, WordBreak,
    WritingMode,
};

mod constructors;

/// SVG 2 §Painting — `color-interpolation`. Inherited. Initial `sRGB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvgColorInterpolation {
    Auto,
    #[default]
    Srgb,
    LinearRgb,
}

/// Storage for the logical min/max size longhands, resolved onto
/// `min-`/`max-` `width`/`height` by `writing-mode` after the cascade.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LogicalMinMaxSizes {
    /// `min-inline-size`. `None` = auto.
    pub min_inline: Option<Length>,
    /// `max-inline-size`. `None` = none.
    pub max_inline: Option<Length>,
    /// `min-block-size`. `None` = auto.
    pub min_block: Option<Length>,
    /// `max-block-size`. `None` = none.
    pub max_block: Option<Length>,
}

/// `(inline-axis, block-axis)` radii of the four flow-relative corners, in the order
/// start-start, start-end, end-start, end-end (CSS Logical Properties L1 §3.3).
pub type LogicalCornerRadii = [Option<(Length, Length)>; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub text_align: TextAlign,
    /// CSS Text L3 §7.2 — `text-align-last`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; применение при line layout — deferred.
    pub text_align_last: TextAlignLast,
    /// CSS Writing Modes L3 §2.1 — направление inline-потока. Inherited.
    /// Задаёт paragraph embedding level для UBA и разрешает логические
    /// `text-align: start|end`. См. [`Direction`] для подробностей.
    pub direction: Direction,
    /// CSS Logical L1 §3.2 — `direction` ближайшего предка-блочного контейнера (включая сам бокс, если он
    /// блочный контейнер): относительно него `float`/`clear: inline-start|inline-end` раскрываются в
    /// физические стороны. Inherited; пересчитывается каскадом после применения деклараций.
    pub block_container_direction: Direction,
    /// `float: inline-start` (`Some(true)`) / `inline-end` (`Some(false)`) до раскрытия в `float_side`.
    /// NOT inherited; сбрасывается физическим значением. Каскад раскрывает его по `block_container_direction`.
    pub float_logical: Option<bool>,
    /// То же для `clear: inline-start|inline-end`.
    pub clear_logical: Option<bool>,
    /// CSS Writing Modes L4 §2.2 — участие бокса в UBA. NOT inherited.
    /// Initial: `Normal`. См. [`UnicodeBidi`].
    pub unicode_bidi: UnicodeBidi,
    pub color: Color,
    /// Цветовое пространство, в котором объявлен `color` (CSS Color L4 §10).
    /// Используется renderer-ом для точной передачи wide-gamut цветов в GPU.
    pub color_space: ColorSpace,
    /// CSSOM-запись computed `color`, когда её нельзя восстановить из `color: Color`
    /// (`lab()`/`oklch()`/`color(display-p3 …)`/`color-mix()`/относительные цвета, BUG-1425).
    /// `None` — `getComputedStyle` печатает `rgb()` из `color`. Наследуется вместе с `color`.
    pub color_css: Option<std::sync::Arc<str>>,
    pub background_color: Option<CssColor>,
    pub font_size: f32,
    /// CSS Viewport L1 §5 — the element's *effective* `zoom`: its own declared
    /// `zoom` multiplied by every ancestor's, since `zoom` compounds down the
    /// tree. Initial (and the value for a page that never declares `zoom`):
    /// `1.0`, which makes every scaling site below a no-op.
    ///
    /// Unlike `transform: scale()`, `zoom` affects *layout*: it multiplies the
    /// computed value of the element's absolute lengths and font-size, so the
    /// box genuinely occupies less space rather than merely being painted
    /// smaller. Applied by [`cascade::apply_zoom_to_lengths`] after the main cascade
    /// pass; relative units need no separate handling because they resolve
    /// against bases (`font_size`, the containing block) that are themselves
    /// already zoomed.
    pub effective_zoom: f32,
    /// Computed (zoomed) `font-size` of the document element, inherited by the
    /// whole tree so `rem` follows `html { font-size }` (CSS Values L4 §5.1.2).
    /// `16` until the root has been cascaded.
    pub root_font_size: f32,
    /// `effective_zoom` of the document element (already inside `root_font_size`);
    /// `rem` in a deeper zoomed element is rescaled by `effective_zoom / root_zoom`.
    pub root_zoom: f32,
    pub line_height: f32,
    /// CSS2 §10.8.1 / CSS Fonts L5 §4 — whether `line-height` was specified as a
    /// relative value (`normal` or a unitless `<number>`) that scales with the
    /// used font-size, vs. an absolute `<length>`/`<percentage>` whose computed
    /// line box is frozen. `line_height` is always stored as a ratio (×font-size);
    /// this flag records which kind it was so `font-size-adjust` (which mutates the
    /// used font-size post-cascade) can keep an absolute line box constant instead
    /// of re-scaling it. Initial: `true` (`normal` is relative). Inherited with
    /// `line_height`.
    pub line_height_is_relative: bool,
    /// CSS2 §10.8.1 / CSS Fonts L4 §14.3 — `true` when `line-height` resolved to
    /// the `normal` keyword specifically, as opposed to an author-specified
    /// unitless `<number>` that happens to also be relative. Both set
    /// `line_height_is_relative = true` and start `line_height` at the same
    /// `1.2` fallback ratio, so this is the only signal that distinguishes them.
    /// FONTLOAD-14 (BUG-467): layout's whole-tree resolve pass
    /// (`box_tree::entry::resolve_used_line_height`) uses this flag to replace
    /// the `1.2` approximation with the box's real font metrics (`ascent +
    /// descent + lineGap`) into [`LayoutBox::used_line_height`] — `line_height`
    /// itself is left untouched (would force an `Arc::make_mut` deep-copy of
    /// nearly every box's style, `normal` being the default). Initial: `true`.
    /// Inherited with `line_height`.
    pub line_height_is_normal: bool,
    /// CSS Rhythmic Sizing L1 §2 — `line-height-step` step unit in px.
    /// When `> 0`, each line box's used height is rounded up to the closest
    /// multiple of this value and the extra space is distributed as half-leading.
    /// `0.0` (initial) disables stepping. Resolved to absolute px at parse time
    /// (em/rem relative to the element's own font-size). Inherited.
    pub line_height_step: f32,
    pub font_style: FontStyle,
    /// CSS Fonts L4 §2.3 — угол `oblique <angle>` в тысячных градуса (`[-90000, 90000]`).
    /// Осмысленен только при `font_style == Oblique`; `None` — голое `oblique`
    /// (синтетический наклон по умолчанию, 14deg). Inherited вместе с `font_style`.
    pub font_oblique_angle: Option<i32>,
    pub font_weight: FontWeight,
    /// CSS Fonts L4 §6.2 — font-variant-caps (весь набор значений). Inherited.
    pub font_variant_caps: FontVariantCaps,
    /// CSS Fonts L4 §6.6 — font-variant-emoji. Inherited. На выбор глифа пока
    /// не влияет — см. [`FontVariantEmoji`].
    pub font_variant_emoji: FontVariantEmoji,
    /// CSS Fonts L4 §6.5 — font-variant-ligatures. Inherited. Попадает в
    /// OpenType-фичи через [`text_font_features`].
    pub font_variant_ligatures: FontVariantLigatures,
    /// CSS Fonts L4 §6.7 — font-variant-numeric. Inherited.
    pub font_variant_numeric: FontVariantNumeric,
    /// CSS Fonts L4 §6.8 — font-variant-position. Inherited.
    pub font_variant_position: FontVariantPosition,
    /// CSS Fonts L4 §2.5 — font-stretch (десятые доли процента; normal = 1000).
    /// Inherited.
    pub font_stretch: FontStretch,
    /// CSS Fonts L4 §3.1 — font-family как приоритизированный список имён.
    /// Inherited. Phase 0: рендерер пока всегда использует Inter, но layout
    /// уже хранит и распространяет список — задел под будущий font matcher.
    /// Generic-family имена (`serif`, `sans-serif`, `monospace`, `cursive`,
    /// `fantasy`, `system-ui`) сохраняются в этом же списке как обычные строки.
    /// Пустой Vec = inherited / default.
    pub font_family: Vec<String>,
    /// CSS Fonts L4 §7 — `font-variation-settings`. Inherited.
    /// Initial: пустой Vec (эквивалентно `normal`). Renderer нормализует
    /// через fvar + avar при растеризации глифов.
    pub font_variation_settings: Vec<FontVariationSetting>,
    /// CSS Fonts L3 §6 — `font-feature-settings`. Inherited.
    /// Initial: пустой Vec (эквивалентно `normal`). Шейпер (lumen-font)
    /// накладывает записи поверх default-набора OpenType-фич: value 0
    /// выключает фичу, ≥1 включает.
    pub font_feature_settings: Vec<FontFeatureSetting>,
    /// CSS Fonts L4 §11.3 — `font-palette`. Inherited. Initial: `Normal`
    /// (default CPAL palette). Selects the color palette for COLR color
    /// glyphs; no effect on monochrome text.
    pub font_palette: FontPalette,
    /// Resolved `@font-palette-values` data when [`Self::font_palette`] is
    /// `Custom` — resolved at the end of `compute_style` against the
    /// stylesheet (paint builds the display list from `ComputedStyle` alone
    /// and has no stylesheet access). `None` for keyword values and unknown
    /// palette names (spec: behaves as `normal`).
    pub font_palette_resolved: Option<ResolvedFontPalette>,
    /// CSS Fonts L4 §7.12 — `font-optical-sizing: auto | none`. Inherited.
    /// `auto` (initial): renderer injects `opsz = font_size` variation axis.
    pub font_optical_sizing: FontOpticalSizing,
    pub text_transform: TextTransform,
    /// Компоненты `text-transform` / `text-indent` / `tab-size`, которых layout не читает
    /// (BUG-1325). Inherited.
    pub text_extra: TextCssomExtra,
    pub white_space: WhiteSpace,
    /// CSS Text L4 §3.1 — `white-space-collapse`. Inherited. Longhand-компонента
    /// `white-space`; хранится для каскада/наследования, layout читает
    /// эффективное `white_space` (пересчитывается через [`WhiteSpace::combine`]).
    pub white_space_collapse: WhiteSpaceCollapse,
    /// CSS Text L3 §7.1: отступ перед первой строкой inline-content.
    /// Inherited. Typed `Length`; `%` = % cb_width, резолвится при layout.
    pub text_indent: Length,
    /// CSS Text L3 §11.2: дополнительное расстояние между каждой парой
    /// символов и между словами (resolved px). Inherited. Может быть
    /// отрицательным (сжимает текст). Применяется в wrap_inline_run при
    /// расчёте ширин.
    pub letter_spacing: f32,
    /// Процентная часть `letter-spacing` (доля `font-size` элемента; 0.1 = 10%, BUG-1438).
    /// `letter_spacing` уже включает её (`abs + pct * font_size`, дописывается в конце каскада);
    /// потомок пересчитывает долю от своего `font-size`.
    pub letter_spacing_pct: f32,
    /// CSS Text L3 §11.3: дополнительное расстояние **между словами**
    /// (resolved px). Inherited. В отличие от `letter-spacing`, добавляется
    /// только на word-boundary, не между всеми символами. Может быть
    /// отрицательным.
    pub word_spacing: f32,
    /// Процентная часть `word-spacing` (доля ширины U+0020 шрифта; 0.25 = 25%).
    /// Резолвится в раскладке, где известна метрика пробела.
    pub word_spacing_pct: f32,
    pub text_decoration_line: TextDecorationLine,
    /// The element itself declared `text-decoration[-line]` (or the UA sheet did, `<a href>`/`<u>`…):
    /// not inherited. A float, an absolutely positioned box and an atomic inline (`inline-block`,
    /// `inline-flex`, …) drop the line propagated from an ancestor (Text Decoration L3 §2.2) but
    /// keep the one declared on itself.
    pub text_decoration_line_declared: bool,
    /// CSS Text Decoration L3 §3 — `text-decoration-color`. `CurrentColor`
    /// означает «использовать style.color при рендеринге» (initial value по
    /// spec). Inherited через каскад (как и `text-decoration-line` в Phase 0
    /// — см. decisions log).
    pub text_decoration_color: CssColor,
    /// CSS Text Decoration L3 §2.2 — `text-decoration-style`. Initial: Solid.
    /// Inherited через каскад (Phase 0; см. doc на [`TextDecorationStyle`]).
    pub text_decoration_style: TextDecorationStyle,
    /// CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Initial: Auto.
    /// Inherited через каскад (Phase 0; см. doc на [`TextDecorationThickness`]).
    pub text_decoration_thickness: TextDecorationThickness,
    /// CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Inherited.
    /// Initial: `None` (нет emphasis marks). Phase 0 layout: parse+store;
    /// real rendering поверх каждого глифа — задача P2.
    pub text_emphasis_style: TextEmphasisStyle,
    /// CSS Text Decoration L4 §5.4 — `text-emphasis-color`. Inherited.
    /// Initial: `CurrentColor` — разрешается в `style.color` при рендере.
    pub text_emphasis_color: CssColor,
    /// WHATWG Compat §3.1 — `-webkit-text-fill-color`. Inherited.
    /// Initial: `CurrentColor` (глиф красится `color`); хранится как задано,
    /// `currentcolor` резолвится в `color` самого фрагмента (`used_text_fill_color`).
    pub text_fill_color: CssColor,
    /// WHATWG Compat §3.2 — `-webkit-text-stroke-width`, px (≥ 0). Inherited.
    /// Initial `0` (обводки нет); `thin`/`medium`/`thick` = 1/3/5 px.
    pub text_stroke_width: f32,
    /// WHATWG Compat §3.2 — `-webkit-text-stroke-color`. Inherited.
    /// Initial `currentcolor`, резолвится в `color` фрагмента (`used_text_stroke_color`).
    pub text_stroke_color: CssColor,
    /// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Inherited.
    /// Initial: `Auto` (в горизонтальном письме — `over right`).
    pub text_emphasis_position: TextEmphasisPosition,
    /// CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`.
    /// Inherited. Initial: `Auto`. Controls whether underline is placed below
    /// the alphabetic baseline (`Under`) or uses font metrics (`FromFont`).
    pub text_underline_position: TextUnderlinePosition,
    /// CSS Text Decoration L4 §5.3 — `text-underline-offset`. Inherited.
    /// Initial: `None` (auto ≡ 0). `Some(px)` = additional offset added to
    /// the intrinsic underline position. Positive shifts down (away from text).
    pub text_underline_offset: Option<f32>,
    /// CSS Text Decoration L4 §3.5 — `text-decoration-skip-ink`. Inherited.
    /// Initial: `Auto`. Controls whether underlines skip over glyph descenders.
    pub text_decoration_skip_ink: TextDecorationSkipInk,
    /// Доля `%` у `text-underline-offset` (`calc(200% - 8px)` → абсолют в `text_underline_offset`,
    /// `2.0` здесь). База процента — `font-size` элемента; в раскладке пока не участвует.
    pub text_underline_offset_pct: f32,
    /// CSS Text Decoration L4 §3.6 — `text-decoration-skip-spaces`. Inherited. Initial: `start end`.
    pub text_decoration_skip_spaces: TextDecorationSkipSpaces,
    /// CSS Text Decoration L4 §4.2 — `text-decoration-inset`. Not inherited. `None` = `auto`;
    /// `Some([start, end])`, каждый — `(px, доля %)` (`calc(10% - 20px)` → `(-20, 0.1)`).
    pub text_decoration_inset: Option<[(f32, f32); 2]>,
    /// Явная ширина (CSS `width`). `None` = auto. Typed `Length`; `%`
    /// резолвится при layout с known cb_width.
    pub width: Option<Length>,
    /// Явная высота (CSS `height`). `None` = auto.
    pub height: Option<Length>,
    /// CSS 2.1 §10.4 — `min-width`. `None` = initial/auto (≡ 0).
    pub min_width: Option<Length>,
    /// CSS 2.1 §10.4 — `max-width`. `None` = `none` (без ограничения).
    pub max_width: Option<Length>,
    /// CSS 2.1 §10.4 — `min-height`. `None` = initial/auto (≡ 0).
    pub min_height: Option<Length>,
    /// CSS 2.1 §10.4 — `max-height`. `None` = `none`.
    pub max_height: Option<Length>,
    /// CSS 2.1 §8.3 — внешние отступы. `Auto` для `margin: auto` (centering).
    /// `%` = % cb_width, резолвится при layout. Initial = `Length(Px(0.0))`.
    pub margin_top: LengthOrAuto,
    pub margin_right: LengthOrAuto,
    pub margin_bottom: LengthOrAuto,
    pub margin_left: LengthOrAuto,
    /// CSS 2.1 §8.4 — внутренние отступы. `%` = % cb_width, резолв при
    /// layout. Initial = `Px(0.0)`.
    pub padding_top: Length,
    pub padding_right: Length,
    pub padding_bottom: Length,
    pub padding_left: Length,
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
    /// Initial = `CurrentColor` (spec: initial value of border-color IS currentColor).
    pub border_top_color: CssColor,
    pub border_right_color: CssColor,
    pub border_bottom_color: CssColor,
    pub border_left_color: CssColor,
    pub box_sizing: BoxSizing,
    /// CSS Rhythmic Sizing L1 §3.2 — `block-step-size`. NOT inherited.
    /// Initial: `none` (`None`). Phase 0: parse + store, resolved eagerly to
    /// px (same scope as `line_height_step`) — no layout algorithm effect yet
    /// (BUG-517).
    pub block_step_size: Option<f32>,
    /// CSS Rhythmic Sizing L1 §3.3 — `block-step-insert`. NOT inherited.
    /// Initial: `MarginBox` (BUG-517).
    pub block_step_insert: BlockStepInsert,
    /// CSS Rhythmic Sizing L1 §3.4 — `block-step-align`. NOT inherited.
    /// Initial: `Auto` (BUG-517).
    pub block_step_align: BlockStepAlign,
    /// CSS Rhythmic Sizing L1 §3.5 — `block-step-round`. NOT inherited.
    /// Initial: `Up` (BUG-517).
    pub block_step_round: BlockStepRound,
    /// CSS Positioned Layout L3 §3 — `position`. Не наследуется.
    /// Default `Static`. Используется для stacking context (§9.10) и layout.
    pub position: Position,
    /// CSS Positioned Layout L3 §4 — inset properties. Не наследуются.
    /// `auto` = не задано (участвует в shrink-to-fit или оставляет edge на месте).
    pub top: LengthOrAuto,
    pub right: LengthOrAuto,
    pub bottom: LengthOrAuto,
    pub left: LengthOrAuto,
    /// CSS Positioned Layout L3 §9.3 — `z-index: auto | <integer>`. Не
    /// наследуется. `None` = `auto` (stacking context создаётся только если
    /// другие триггеры в §9.10 совпали). `Some(n)` = явный integer; для
    /// positioned- и flex/grid-item элементов это запускает создание
    /// stacking context.
    pub z_index: Option<i32>,
    /// CSS Flexbox L1 §4.3 / Grid L1 §6.4 — the box is an in-flow child of a
    /// flex or grid container (`inherited.display` at cascade time), so a
    /// `z-index` other than `auto` applies to it even with `position: static`
    /// and makes it a stacking context (`creates_stacking_context`). Not
    /// inherited; set by the cascade, never by a declaration.
    pub is_flex_grid_item: bool,
    /// CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
    /// элемент из нормального потока. `None` — нормальный поток.
    pub float_side: FloatSide,
    /// CSS 2.1 §9.5.2 — `clear`. Не наследуется. Определяет, мимо каких
    /// float-ов блок обязан «пройти» перед размещением в потоке.
    pub clear: ClearSide,
    /// CSS Inline Layout L3 §5 — `initial-letter` высота буквицы в строках.
    /// Не наследуется. `1.0` = `normal` (без эффекта); `> 1.0` активирует
    /// буквицу высотой в это число строк (значение может быть дробным).
    pub initial_letter_size: f32,
    /// CSS Inline Layout L3 §5 — `initial-letter` «утопление» (sink): сколько
    /// строк нормального потока занимает буквица сбоку. Не наследуется.
    /// `0` = `auto`, выводится как `floor(initial_letter_size)`.
    pub initial_letter_sink: u32,
    /// CSS Inline Layout L3 §5.1 — ключевое слово `initial-letter`: `0` — не записано,
    /// `1` — `drop`, `2` — `raise`. Раскладку не меняет (`drop` — это `sink = floor(size)`,
    /// `raise` ставит `sink = 1`); нужно, чтобы `getComputedStyle` вернул записанную форму.
    pub initial_letter_keyword: u8,
    /// CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется.
    /// `Isolate` создаёт stacking context.
    pub isolation: Isolation,
    /// CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется.
    /// Любое значение, отличное от `Normal`, создаёт stacking context.
    pub mix_blend_mode: MixBlendMode,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. Stored as `Length::Px` for
    /// absolute/em/rem values (resolved at cascade time) or `Length::Percent` for `%`
    /// (resolved at paint time against border-box width). Not inherited.
    pub border_top_left_radius: Length,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. See `border_top_left_radius`.
    pub border_top_right_radius: Length,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. See `border_top_left_radius`.
    pub border_bottom_right_radius: Length,
    /// CSS Backgrounds L3 §5.5: horizontal (x) corner radius. See `border_top_left_radius`.
    pub border_bottom_left_radius: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. Equals x-radius for circular corners
    /// (`border-radius: 10px`); differs for elliptical (`border-radius: 10px / 20px`). `%` is
    /// resolved against border-box **height** at paint time. Not inherited.
    pub border_top_left_radius_y: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. See `border_top_left_radius_y`.
    pub border_top_right_radius_y: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. See `border_top_left_radius_y`.
    pub border_bottom_right_radius_y: Length,
    /// CSS Backgrounds L3 §5.5: vertical (y) corner radius. See `border_top_left_radius_y`.
    pub border_bottom_left_radius_y: Length,
    /// CSS Display L3 §4 — visibility. Inherited.
    pub visibility: Visibility,
    /// CSS UI L4 §8.1 — cursor. Inherited.
    pub cursor: Cursor,
    /// CSS Backgrounds L3 §4.6 — список теней. Не наследуется. Пустой Vec
    /// = `none`.
    pub box_shadow: Vec<BoxShadow>,
    /// CSS Text Decoration L3 §4 — список теней текста. Inherited
    /// (отличается от box-shadow!). Пустой Vec = `none`.
    pub text_shadow: Vec<TextShadow>,
    /// CSS Overflow L3 — отдельные поля для X и Y. Не наследуются.
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// CSS Overflow L3 §logical (BUG-505) — специфицированные значения
    /// `overflow-block`/`overflow-inline` до разрешения в физическую пару
    /// `overflow_x`/`overflow_y` по `writing_mode`
    /// (`resolve_overflow_logical_properties`, `style/logical.rs`). Не
    /// наследуются, как и физическая пара. Default `Visible` совпадает с
    /// initial value — тот же эвристический приём «поле != default
    /// означает явную установку», что у остальных логических свойств в
    /// этом файле (см. `margin_inline_start` и соседей).
    pub overflow_block: Overflow,
    /// CSS Overflow L3 §3.5 — `overflow` of `<html>` (or of `<body>` when `<html>`'s is `visible`)
    /// goes to the viewport and the element's *used* value is `visible`: paint must not clip to the
    /// element's own box. Not inherited; the computed `overflow_x/y` stay as authored (the page-scroll
    /// filter reads them). Set by the cascade.
    pub overflow_to_viewport: bool,
    pub overflow_inline: Overflow,
    /// CSS Overflow L3 §overflow-clip-margin — расширяет clip region при
    /// overflow:clip. `(box, length)`, default `(padding-box, 0px)`
    /// (BUG-505 срез 4: was a bare `Option<Length>` — Phase 0 only
    /// supported the `<length>` half of `[<visual-box> || <length [0,∞]>]`).
    /// `None` means unset (initial). Не наследуется.
    pub overflow_clip_margin: Option<(OverflowClipMarginBox, Length)>,
    /// CSS Overflow L5 §scroll-marker-group-property (BUG-505 срез 6).
    /// `None` = initial `none`. Не наследуется.
    pub scroll_marker_group: Option<ScrollMarkerGroup>,
    /// CSS Overflow L5 §scroll-target-group. `ScrollTargetGroup::None` =
    /// initial `none` (BUG-505 срез 6). Не наследуется.
    pub scroll_target_group: ScrollTargetGroup,
    /// CSS UI L4 §10.1 — text-overflow. Не наследуется.
    pub text_overflow: TextOverflow,
    /// Второе значение `text-overflow` (правая сторона строки); одно значение задаёт обе.
    pub text_overflow_right: TextOverflow,
    /// CSS Text Size Adjustment L1 §2 — `text-size-adjust` / legacy alias
    /// `-webkit-text-size-adjust`. Наследуется. Initial `Auto`. BUG-513.
    /// Phase 0: parse + store + interpolate — нет мобильного пайплайна
    /// авто-инфляции текста, эффекта на рендер нет (тот же класс, что
    /// `dynamic_range_limit`/BUG-508).
    pub text_size_adjust: TextSizeAdjust,
    /// CSS Color L3 §3.2 — opacity (0.0..=1.0). Не наследуется. Работает
    /// как alpha всего слоя (включая фон, бордер, текст и потомков). В
    /// Phase 0 layout только хранит — paint пока не применяет alpha
    /// blending этого уровня; индивидуальные альфы в `color`/`background`
    /// продолжают работать.
    pub opacity: f32,
    /// CSS Basic UI L4 §5: outline. В отличие от border не сдвигает соседей
    /// и не учитывается в width/height (рисуется поверх / снаружи коробки).
    /// Не наследуется.
    ///
    /// Initial computed `outline-width` = `medium` (3 px по UA convention);
    /// **used** value становится 0 при `outline-style: none` (CSS 2.1
    /// §17.6.1 / Basic UI L4 §5.2) — см. `outline_used_width()`. Поэтому
    /// «outline по умолчанию невидим» обеспечивается style=None, а не
    /// width=0.
    pub outline_width: f32,
    pub outline_style: OutlineStyle,
    pub outline_color: OutlineColor,
    /// CSS Basic UI L4 §5.5 — outline-offset. Положительное — дальше от
    /// бокса, отрицательное — внутрь. Typed `Length`; резолвится при paint.
    pub outline_offset: Length,
    /// CSS UI L4 §6.1 — accent-color. Цвет встроенных form widgets
    /// (checkbox, radio, range, progress). `None` = `auto` (UA default).
    /// Inherited. В Phase 0 layout только хранит — real применение появится
    /// вместе с form-widget рендерингом.
    pub accent_color: Option<Color>,
    /// CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`.
    /// Phase 0: parse + store; реальное переключение SystemColor / UA-тем — P2.
    pub color_scheme: ColorScheme,
    /// CSS Color Adjustment L1 §4 — `forced-color-adjust`. Inherited. Initial: `Auto`.
    /// Применяется в `style::adjust::apply_forced_colors_mode`.
    pub forced_color_adjust: ForcedColorAdjust,
    /// CSS Color HDR L1 §2 — `dynamic-range-limit`. Inherited. Initial:
    /// `no-limit` (`DynamicRangeLimit::default()`). BUG-508. Phase 0: parse
    /// + store + interpolate — no HDR display pipeline, no rendering effect.
    pub dynamic_range_limit: DynamicRangeLimit,
    /// CSS Variables L1 — custom properties (`--name`). Все custom properties
    /// inherited (спека: `all custom properties are inherited by default`).
    /// Ключ — полное имя с ведущими `--`, значение — сырой текст из source.
    /// Substitution `var(--name [, fallback])` делается lazy при применении
    /// обычных деклараций (см. `apply_declaration`).
    ///
    /// Shared copy-on-write (BUG-341 S9) — see [`CustomProps`] for why.
    pub custom_props: CustomProps,
    /// CSS Lists L3 §3 — `counter-reset: name [N]?`. Каждый element задаёт
    /// (имя-счётчика, начальное-значение). Не наследуется. Пустой `Vec`
    /// при отсутствии декларации или `counter-reset: none`. Реальное
    /// разрешение counter() в `content` pseudo-elements — отдельная задача
    /// (требует layout-time counter scoping walker).
    pub counter_reset: Vec<(String, i32)>,
    /// CSS Lists L3 §3 — `counter-increment: name [N]?`. Каждый element
    /// инкрементирует названный counter на N (default +1). Не наследуется.
    pub counter_increment: Vec<(String, i32)>,
    /// CSS Lists L3 §4 — `counter-set: name [N]?`. Каждый element
    /// устанавливает названный counter в N (default 0). Не наследуется.
    /// Применяется ПОСЛЕ counter-reset и counter-increment (порядок по spec).
    /// Если счётчика с таким именем нет в области видимости — создаёт его.
    pub counter_set: Vec<(String, i32)>,
    /// CSS Masking L1 §3 — `clip-path: <basic-shape> | none`. Не
    /// наследуется. Phase 0: parsing only — real geometric clipping
    /// в paint pipeline отложен.
    pub clip_path: Option<ClipPath>,
    /// CSS Transforms L1 §2 — `transform: <transform-list> | none`.
    /// Список функций — каждая `TransformFn` хранит параметры. Не
    /// наследуется.
    pub transform: Vec<TransformFn>,
    /// CSS Transforms L2 §2 — `translate: none | <tx> [<ty>]`.
    /// Individual translate (`<length-percentage>` по X и Y; процент — от border
    /// box, BUG-1292); composed BEFORE `transform` in the final matrix.
    /// `None` = `none` (identity). Not inherited.
    pub translate: Option<(Length, Length)>,
    /// CSS Transforms L2 §2 — `rotate: none | <angle>`.
    /// Individual 2D rotation in radians; composed BEFORE `transform`.
    /// `None` = `none` (identity). Not inherited.
    pub rotate: Option<f32>,
    /// CSS Transforms L2 §2 — `scale: none | <sx> [<sy>]`.
    /// Individual scale factors; composed BEFORE `transform`.
    /// `None` = `none` (identity). Not inherited.
    pub scale: Option<(f32, f32)>,
    /// CSS Filter Effects L1 §3 — `filter: <filter-function-list> | none`.
    /// Список функций — blur/brightness/contrast/grayscale/etc. Не
    /// наследуется. Phase 0: parsing only.
    pub filter: Vec<FilterFn>,
    /// CSS Box Alignment L3 §8 — `row-gap` / `column-gap` для
    /// flex/grid container-ов. В пикселях (resolved). Default 0.
    /// Не наследуется. Phase 0: parsing only — real flex/grid algorithm
    /// не реализован, гарантированно gap не применяется.
    /// CSS Box Alignment L3 §8 — `row-gap` / `column-gap` для flex/grid.
    /// Typed `Length`; `%` = % cb_size. Default `Px(0)`. Phase 0: parse only.
    pub row_gap: Length,
    pub column_gap: Length,
    /// CSS Box Alignment L3 §8.1 — `column-gap: normal` (the initial value) is `0` in flex/grid
    /// but `1em` in a multicol container (Multicol L1 §3.3). `true` until a `<length-percentage>`
    /// is declared; read through [`ComputedStyle::multicol_column_gap`].
    pub column_gap_normal: bool,
    /// `row-gap: normal` (the initial value, `0` for rows) was not overridden by a length. Only a
    /// subgrid looks at it: an explicit `row-gap` replaces the parent's gutter inside the subgrid
    /// (Grid L2 §9), `normal` keeps it.
    pub row_gap_normal: bool,
    /// CSS Multi-column L1 §3.2 — `column-count: <integer> | auto`. `None`
    /// = `auto`. Phase 0: parsing only.
    pub column_count: Option<u32>,
    /// CSS Multi-column L1 §3.3 — `column-width: <length> | auto`. Typed.
    /// `None` = `auto`. Phase 0: parsing only.
    pub column_width: Option<Length>,
    /// CSS Multi-column L2 §4.2 — `column-height: auto | <length>`. `None` = `auto`.
    pub column_height: Option<Length>,
    /// CSS Multi-column L2 §4.4 — `column-wrap: nowrap`. `false` = `auto`/`wrap`: with a
    /// non-auto `column-height`, overflow columns open a new row in the block direction.
    pub column_wrap_nowrap: bool,
    /// `column-wrap: wrap` written explicitly: rows open on forced breaks even with an `auto`
    /// `column-height` (Multicol L2 §4.4), where `auto` stays a single row.
    pub column_wrap_explicit: bool,
    /// CSS Multi-column L1 §4.1 / CSS Gap Decorations L1 §4.5 — `column-rule-width`: список
    /// px-значений по щелям (`repeat()` сохранён). Default `[3]` (`medium`).
    pub column_rule_width: RuleList<f32>,
    /// CSS Multi-column L1 §4.2 — `column-rule-style`. Default `None`
    /// (без линии — линия рисуется только если style != None и width > 0).
    pub column_rule_style: RuleList<BorderStyle>,
    /// CSS Multi-column L1 §4.3 — `column-rule-color`. Initial = `CurrentColor`.
    pub column_rule_color: RuleList<CssColor>,
    /// CSS Gap Decorations L1 §3 — `row-rule-width` (px). Default 3 (`medium`). Non-inherited.
    /// Горизонтальные линии между строками flex-wrap/grid-контейнера; ось колонок
    /// использует `column_rule_*` выше (одни и те же поля для multicol и gap decorations).
    pub row_rule_width: RuleList<f32>,
    /// CSS Gap Decorations L1 §3 — `row-rule-style`. Default `None`. Non-inherited.
    pub row_rule_style: RuleList<BorderStyle>,
    /// CSS Gap Decorations L1 §3 — `row-rule-color`. Default `CurrentColor`. Non-inherited.
    pub row_rule_color: RuleList<CssColor>,
    /// CSS Gap Decorations L1 §3.2 — `column-rule-break`. Initial `Normal`. Non-inherited.
    pub column_rule_break: RuleBreak,
    /// CSS Gap Decorations L1 §3.2 — `row-rule-break`. Initial `Normal`. Non-inherited.
    pub row_rule_break: RuleBreak,
    /// CSS Gap Decorations L1 §3.4 — `column-rule-visibility-items`. Initial `Normal`. Non-inherited.
    pub column_rule_visibility_items: RuleVisibilityItems,
    /// CSS Gap Decorations L1 §3.4 — `row-rule-visibility-items`. Initial `Normal`. Non-inherited.
    pub row_rule_visibility_items: RuleVisibilityItems,
    /// CSS Gap Decorations L1 §3.5 — `rule-overlap`. Initial `RowOverColumn`. Non-inherited.
    pub rule_overlap: RuleOverlap,
    /// CSS Gap Decorations L1 §3.3 — `column-rule-inset-{cap,junction}-{start,end}`. Initial `0`.
    /// Non-inherited.
    pub column_rule_inset: RuleInsets,
    /// CSS Gap Decorations L1 §3.3 — `row-rule-inset-{cap,junction}-{start,end}`. Initial `0`.
    /// Non-inherited.
    pub row_rule_inset: RuleInsets,
    /// CSS Tables L2 §17.6 — `border-collapse`. Inherited. Default `Separate`.
    /// When `Collapse`, `border-spacing` has no effect and adjacent cell borders merge.
    pub border_collapse: BorderCollapse,
    /// CSS Tables L2 §17.6.1.1 — `empty-cells`. Inherited. Default `Show`.
    /// When `Hide`, a table cell with no in-flow content draws neither borders nor
    /// background. No effect under `border-collapse: collapse`.
    pub empty_cells: EmptyCells,
    /// CSS Tables L2 §17.4.1 — `caption-side`. Inherited. Default `Top`.
    pub caption_side: CaptionSide,
    /// CSS Tables L2 §17.5.2 — `table-layout`. Not inherited. Default `Auto`.
    pub table_layout: TableLayout,
    /// CSS 2.1 §17.6 — `border-spacing: <length> [<length>]?`. Inherited. Default 0.
    /// Horizontal gap (px) between adjacent table cells in separate-border mode.
    /// Only applies when `border-collapse: separate` (CSS 2.1 default).
    pub border_spacing_h: f32,
    /// CSS 2.1 §17.6 — `border-spacing` vertical component (px). Inherited.
    /// Vertical gap between adjacent table rows in separate-border mode.
    pub border_spacing_v: f32,
    /// CSS Multi-column L1 §6.1 — `column-span: none | all`. По умолчанию
    /// `None` (False), `Some(true)` = `all` (элемент растягивается через
    /// все колонки). Не наследуется. Phase 0: parse+store.
    pub column_span_all: bool,
    /// CSS Multi-column L1 §6.2 — `column-fill: balance | auto`. `true`
    /// = balance (spec default — распределяет содержимое поровну между
    /// колонками), `false` = auto (заполняет колонки последовательно до
    /// высоты контейнера). Не наследуется.
    pub column_fill_balance: bool,
    /// `column-fill: balance-all` (Multicol L2 §6.2) — раскладывается как `balance`, но
    /// `getComputedStyle` отдаёт исходное ключевое слово.
    pub column_fill_balance_all: bool,
    /// CSS Fragmentation L3 §5.1 — `box-decoration-break: slice | clone`. `true` = `clone` (every
    /// fragment of the box keeps its own full border, padding and margin), `false` = `slice` (the
    /// default: the edges a break cuts lose them). Read by the nested-multicol cut in
    /// `multicol_trampoline`; not inherited.
    pub box_decoration_clone: bool,
    /// The box is one fragment of a multicol container that its multicol parent cut across its own
    /// columns (`multicol_nested`): the rows of columns of the last band end where the fragment
    /// does, not at a full `column-height`. Set by the layout, never by the cascade.
    pub multicol_fragment: bool,
    /// CSS Fragmentation L3 §3.1 — `break-before`. Phase 0 — enum со
    /// значениями auto/avoid/always/page/column/region. Не наследуется.
    pub break_before: BreakValue,
    pub break_after: BreakValue,
    pub break_inside: BreakValue,
    /// CSS Sizing L4 §6.1 — `aspect-ratio: auto | <ratio> | auto <ratio>`.
    /// `None` = `auto` (UA выбирает). `Some((w, h))` = явное отношение
    /// W:H (например, 16:9 → (16.0, 9.0)). Не наследуется.
    /// Phase 0: parsing — real intrinsic-aspect-ratio enforcement
    /// требует layout-time pass.
    pub aspect_ratio: Option<(f32, f32)>,
    /// `aspect-ratio: auto <ratio>` / `<ratio> auto`: `aspect_ratio` is only the fallback for a
    /// box without a natural ratio — a replaced element with one (decoded image, canvas) keeps
    /// the natural ratio instead.
    pub aspect_ratio_auto: bool,
    /// Не CSS-свойство — внутренняя пометка `box_tree::build`: `true`, если
    /// `width` заполнен не автором, а presentational-hint'ом из декодированных
    /// intrinsic-пикселей `<img>` (оба `width`/`height` были `auto` — см. ветку
    /// `is_image_element` в `build.rs`). Отличает «автор явно задал ширину» от
    /// «ширина — заглушка под intrinsic-размер», что нужно flex-раскладке
    /// (BUG-736): подставленную ширину нельзя трактовать как разрешающий
    /// flex-basis `width`, иначе алгоритм CSS Flexbox L1 §9.2/§4.5
    /// (transferred size через `aspect_ratio`) никогда не срабатывает и
    /// растянутый/сжатый по main-оси элемент замещения не пересчитывает
    /// вторую сторону по соотношению. Не наследуется, не сбрасывается кроме
    /// как через новый `compute_style`.
    pub width_is_intrinsic_hint: bool,
    /// Same as [`Self::width_is_intrinsic_hint`], for `height`. Both an
    /// `<img>` element's own `width`/`height` content attributes (whether the
    /// author wrote them or the shell filled them in after decode — see
    /// `image_requests::apply_intrinsic_size`) go through the SAME
    /// presentational-hint path (`apply_image_presentational_hints`), so both
    /// axes need this flag independently: a replaced element commonly has
    /// both attributes at once, and BUG-736's row-direction flex case is
    /// gated on the CROSS axis (height) hint, not the main axis (width) one.
    pub height_is_intrinsic_hint: bool,
    /// `width` задан UA-стилем нативного контрола (`apply_ua_form_controls`), а не автором:
    /// grid `normal`/`stretch` растягивает такую ось как `auto` (BUG-1609). Авторское
    /// `width` (в т.ч. `initial`/`inherit`) сбрасывает флаг, как `width_is_intrinsic_hint`.
    pub width_is_ua_default: bool,
    /// То же для `height`.
    pub height_is_ua_default: bool,
    /// CSS Box Alignment L3 — alignment свойства для flex/grid items.
    /// Все не наследуются. Phase 0: parsing only.
    pub align_items: AlignValue,
    pub align_self: AlignValue,
    pub align_content: AlignValue,
    pub justify_items: AlignValue,
    pub justify_self: AlignValue,
    pub justify_content: AlignValue,
    /// `safe`/`left`/`right` of `justify-content`/`align-content` — see [`ContentAlignExtra`].
    pub content_align_extra: ContentAlignExtra,
    /// CSS Backgrounds L3 §3 — стек фоновых слоёв. Первый элемент = верхний (рендерится поверх).
    /// Пустой Vec соответствует `background-image: none` без слоёв. `background-color` отдельно.
    pub background_layers: Vec<BackgroundLayer>,
    /// Списки per-layer-свойств как заданы автором — для `getComputedStyle` (BUG-1297).
    pub background_specified: Option<Box<BackgroundSpecified>>,
    /// CSS Backgrounds L3 §6 — `border-image` (источник + slice/width/outset/repeat). Не наследуется.
    pub border_image: BorderImage,
    /// CSS Will Change L1. Список имён свойств для optimization hint.
    /// Пустой Vec = `auto` (default). Не наследуется.
    pub will_change: Vec<String>,
    /// CSS UI L4 §6.1 / Pointer Events L1. Default `auto`. **Наследуется**
    /// (`Inherited: yes`): потомок `pointer-events: none` тоже не цель, пока
    /// сам не вернёт `auto`/`all`.
    pub pointer_events: PointerEvents,
    /// CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; обработка touch-жестов — P3 task.
    pub touch_action: TouchAction,
    /// CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; form-widget styling — P2/P3 task.
    pub appearance: Appearance,
    /// CSS Basic UI L4 §4.4 — `field-sizing`. NOT inherited. Initial: `Fixed`.
    /// When `Content`, UA-default dimensions are suppressed and `lay_out_box` calls
    /// `field_sizing_content_intrinsic()` to size the control from its text.
    pub field_sizing: FieldSizing,
    /// CSS UI L4 §6.2 — `user-select`. Inherited (по спеке).
    pub user_select: UserSelect,
    /// CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`.
    /// Drag-resize UI (grip hit-test, apply): `crates/shell/src/main.rs` (CC-CSS-4).
    pub resize: Resize,
    /// CSS Overflow L3 — `scroll-behavior`. Inherited.
    pub scroll_behavior: ScrollBehavior,
    /// CSS Scroll Snap L1 §3.1 — `scroll-snap-type`. Не наследуется.
    pub scroll_snap_type: ScrollSnapType,
    /// CSS Scroll Snap L1 §6.1 — `scroll-snap-align`. Не наследуется.
    pub scroll_snap_align: ScrollSnapAlign,
    /// CSS Scroll Snap L1 §6.2 — `scroll-snap-stop`. Не наследуется.
    pub scroll_snap_stop: ScrollSnapStop,
    /// CSS Scroll Snap L2 §4 — `scroll-initial-target`. Не наследуется.
    pub scroll_initial_target: ScrollInitialTarget,
    /// CSS Scroll Snap L1 §4 — `scroll-margin-*` (resolved px).
    pub scroll_margin_top: f32,
    pub scroll_margin_right: f32,
    pub scroll_margin_bottom: f32,
    pub scroll_margin_left: f32,
    /// CSS Scroll Snap L1 §4 — `scroll-padding-*` (resolved px).
    pub scroll_padding_top: f32,
    pub scroll_padding_right: f32,
    pub scroll_padding_bottom: f32,
    pub scroll_padding_left: f32,
    /// CSS Overscroll Behavior L1 §2 — `overscroll-behavior-x`. Не наследуется.
    pub overscroll_behavior_x: OverscrollBehavior,
    pub overscroll_behavior_y: OverscrollBehavior,
    /// CSS Overscroll Behavior L1 §2 — специфицированные значения
    /// `overscroll-behavior-block`/`overscroll-behavior-inline` до
    /// разрешения в физическую пару `overscroll_behavior_x`/`_y` по
    /// `writing_mode` (`resolve_overscroll_behavior_logical_properties`,
    /// `style/logical.rs`). Не наследуются, как и физическая пара. Default
    /// `Auto` совпадает с initial value — тот же эвристический приём, что у
    /// `overflow_block`/`overflow_inline` (BUG-505) и остальных логических
    /// свойств в этом файле.
    pub overscroll_behavior_block: OverscrollBehavior,
    pub overscroll_behavior_inline: OverscrollBehavior,
    /// CSS Text L3 §10.1 — `tab-size: <integer> | <length>`. Inherited.
    /// В пикселях если length; для integer хранится как число × 8 (default
    /// 8 spaces — стандартный default). Default 8 spaces = 64px при 8px-space.
    pub tab_size: f32,
    /// CSS UI L4 §6.3 — `caret-color: auto | <color>`. Inherited.
    /// `None` = auto (каретка следует `color`). `Some(CssColor)` — явный цвет:
    /// `currentcolor` / `color()` / системные цвета сохраняются как есть и
    /// разрешаются в `used_caret_color` (системные — ещё и постпассом).
    pub caret_color: Option<CssColor>,
    /// CSS UI L4 §6.2 — `caret-shape: auto | bar | block | underscore`. Inherited.
    pub caret_shape: CaretShape,
    /// CSS UI L4 §6.2 — `caret-animation: auto | manual`. Inherited. `manual` —
    /// каретка не мигает (анимацию ведёт автор).
    pub caret_animation: CaretAnimation,
    /// CSS UI L4 §6.4 — `interactivity: auto | inert`. Inherited. `inert` — элемент и его
    /// потомки не становятся целью указателя (UA-правила `[inert]`, `dialog:modal`, `:fullscreen`).
    pub interactivity: Interactivity,
    /// CSS UI L4 §6.5 — `input-security: auto | none`. Inherited. `auto` — `<input type=password>`
    /// скрывает символы; `none` — показывает текст.
    pub input_security: InputSecurity,
    /// CSS Text L3 §5.2 — `overflow-wrap: normal | break-word | anywhere`.
    /// Inherited. Default `Normal`.
    pub overflow_wrap: OverflowWrap,
    /// CSS Text L3 §5.1 — `word-break: normal | keep-all | break-all |
    /// break-word`. Inherited. Default `Normal`.
    pub word_break: WordBreak,
    /// CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`.
    /// Phase 0: parse + store; CJK line-breaking — отдельная задача.
    pub line_break: LineBreak,
    /// CSS Text L3 §6 — `hyphens: none | manual | auto`. Inherited.
    /// Default `Manual`.
    pub hyphens: Hyphens,
    /// CSS Transforms L1 §6 — `transform-origin: <x> <y> <z>?`.
    /// Default `50% 50% 0` — центр бокса. Percentages resolved at display-list
    /// time against border-box width/height (box dimensions known only after layout).
    pub transform_origin: (PositionComponent, PositionComponent, f32),
    /// CSS Transforms L2 §4 — `perspective: <length> | none`.
    /// `None` = no perspective; `Some(px)` = distance to camera.
    pub perspective: Option<f32>,
    /// CSS Transforms L2 §4 — `perspective-origin: <x> <y>`.
    /// Default `50% 50%`. Resolved at display-list time against border-box.
    pub perspective_origin: (PositionComponent, PositionComponent),
    /// CSS Transforms L2 §6 — `transform-style: flat | preserve-3d`.
    /// `Preserve3d` makes children participate in 3D rendering context.
    pub transform_style: TransformStyle,
    /// CSS Transforms L1 §transform-box — reference box of `transform-origin`
    /// and percentage `translate`. Initial `view-box`; not inherited.
    pub transform_box: TransformBox,
    /// CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`.
    /// Stored on ComputedStyle; actual back-face culling deferred until
    /// a 3D rendering context exists (3D projection ⬜).
    pub backface_visibility: BackfaceVisibility,
    /// CSS Lists L3 §2.1 — `list-style-type`.
    pub list_style_type: ListStyleType,
    /// CSS Lists L3 §2.3 — `list-style-position`.
    pub list_style_position: ListStylePosition,
    /// CSS Lists L3 §2.2 — `list-style-image: url(...) | none`.
    pub list_style_image: Option<String>,
    /// CSS Transitions L1 §3 — `transition-property: none | all | <ident>+`.
    pub transition_properties: Vec<String>,
    /// `transition-property: none` was declared (the list is empty either way: the scheduler
    /// treats both as «no transition»). Only the computed value differs — `none` against the
    /// initial `all` (CSSANIM-MODEL).
    pub transition_property_none: bool,
    /// CSS Transitions L1 §3 — `transition-duration: <time>+` в секундах.
    pub transition_durations: Vec<f32>,
    /// CSS Transitions L1 §3 — `transition-delay: <time>+` в секундах.
    pub transition_delays: Vec<f32>,
    /// CSS Transitions L1 §3 — `transition-timing-function: <easing-function>+`.
    /// Per-property list; если длина короче `transition_properties`, при
    /// resolve-time spec велит cyclically reuse последний элемент.
    pub transition_timing_functions: Vec<TimingFunction>,
    /// CSS Transitions L2 §3 — `transition-fill-mode: <single-animation-fill-mode>#`.
    /// Parallels animation-fill-mode; используется для сохранения значений
    /// в delay-периоде (backwards) и после завершения (forwards).
    pub transition_fill_modes: Vec<AnimationFillMode>,
    /// CSS Transitions L2 §3.1 — `transition-behavior: <transition-behavior-value>#`.
    /// Пустой список = `normal`; читается через `getComputedStyle` и JS-шимом переходов.
    pub transition_behaviors: Vec<TransitionBehavior>,
    /// CSS Animations L1 §3.1 — `animation-name: none | <keyframes-name>#`.
    /// `none` хранится как пустой `Vec` (нет анимаций); иначе список имён.
    /// Имя соответствует `@keyframes name { ... }` в [`Stylesheet`].
    pub animation_names: Vec<String>,
    /// CSS Animations L1 §3.2 — `animation-duration: <time>#`. Секунды.
    /// Параллельный список к `animation_names`; cyclically reuse при
    /// несовпадении длины (resolve в P1 п.3A scheduler).
    pub animation_durations: Vec<f32>,
    /// CSS Animations L1 §3.3 — `animation-timing-function: <easing-function>#`.
    pub animation_timing_functions: Vec<TimingFunction>,
    /// CSS Animations L1 §3.4 — `animation-delay: <time>#`. Секунды.
    /// Отрицательные значения допустимы и означают «анимация началась
    /// в прошлом» (используется для phase-offset нескольких анимаций).
    pub animation_delays: Vec<f32>,
    /// CSS Animations L1 §3.5 — `animation-iteration-count: <single-iteration-count>#`.
    pub animation_iteration_counts: Vec<IterationCount>,
    /// CSS Animations L1 §3.6 — `animation-direction: <single-animation-direction>#`.
    pub animation_directions: Vec<AnimationDirection>,
    /// CSS Animations L1 §3.7 — `animation-fill-mode: <single-animation-fill-mode>#`.
    pub animation_fill_modes: Vec<AnimationFillMode>,
    /// CSS Animations L1 §3.8 — `animation-play-state: <single-animation-play-state>#`.
    pub animation_play_states: Vec<AnimationPlayState>,
    /// CSS Animations L2 §3.9 — `animation-composition: <single-animation-composition>#`.
    /// Non-inherited, not reset by the `animation` shorthand. Empty = all `replace`.
    pub animation_compositions: Vec<AnimationComposition>,
    /// CSS Scroll-Driven Animations L1 §3.3 — `animation-timeline: auto | scroll() | view() | <ident>#`.
    /// Non-inherited. Parallel list to `animation_names`. Default empty = all `Auto`.
    pub animation_timelines: Vec<AnimationTimeline>,
    /// CSS Scroll-Driven Animations L1 §3.1 — `scroll-timeline-name: none | <custom-ident>`.
    /// Non-inherited. Names this element as a scroll container for a named scroll timeline.
    pub scroll_timeline_name: Option<String>,
    /// CSS Scroll-Driven Animations L1 §3.2 — `scroll-timeline-axis: block | inline | x | y`.
    /// Non-inherited. Which axis drives the named scroll timeline. Default `Block`.
    pub scroll_timeline_axis: ScrollAxis,
    /// CSS Scroll-Driven Animations L1 §3.3 — `view-timeline-name: none | <custom-ident>`.
    /// Non-inherited. Names this element as a view-timeline subject.
    pub view_timeline_name: Option<String>,
    /// CSS Scroll-Driven Animations L1 §3.4 — `view-timeline-axis: block | inline | x | y`.
    /// Non-inherited. Which axis drives the named view timeline. Default `Block`.
    pub view_timeline_axis: ScrollAxis,
    /// CSS Masking L1 §4.9 — список слоёв маски (`mask-image` и все
    /// сопутствующие per-layer longhand-ы). Первый элемент = верхний слой,
    /// последний = нижний (тот же порядок, что у [`Self::background_layers`]).
    /// Пустой Vec = `mask: none`, маска не применяется. Не наследуется.
    pub mask_layers: Vec<MaskLayer>,
    /// CSS Scrollbars 1 — `scrollbar-width: auto | thin | none`.
    pub scrollbar_width: ScrollbarWidth,
    /// `::-webkit-scrollbar { height: 0 }` — горизонтальная полоса скрыта (gutter 0,
    /// оверлей не рисуется), вертикальная живёт по `scrollbar_width`. У стандартного
    /// `scrollbar-width` одно значение на обе оси, поэтому ось-ноль из legacy-
    /// псевдоэлемента хранится отдельно. Не наследуется.
    pub scrollbar_hidden_x: bool,
    /// То же для вертикальной полосы: `::-webkit-scrollbar { width: 0 }`. Не наследуется.
    pub scrollbar_hidden_y: bool,
    /// CSS Scrollbars 1 — `scrollbar-color: auto | <color> <color>`
    /// (thumb-color + track-color).
    pub scrollbar_color: Option<(Color, Color)>,
    /// CSS Overflow L3 — `scrollbar-gutter: auto | stable | stable both-edges`.
    pub scrollbar_gutter: ScrollbarGutter,
    /// CSS Scroll Anchoring 1 — `overflow-anchor: auto | none`. Не
    /// наследуется. Phase 0: только парсинг/CSSOM — сам алгоритм якорения
    /// не реализован (BUG-524).
    pub overflow_anchor: OverflowAnchor,
    /// CSS Content L3 §2.1 — `content`. Используется в pseudo-elements
    /// (`::before` / `::after`) и для counter()-разрешения. Phase 0:
    /// parsing + storage; реальные pseudo-elements в layout — отдельная
    /// большая задача. Default `Normal` (use element's box-tree).
    pub content: Content,
    /// CSS Images L3 §5.5 — `object-fit`. Применяется только к replaced
    /// elements (`<img>` и пр.). Не наследуется. Default `Fill`.
    pub object_fit: ObjectFit,
    /// CSS Images L3 §5.5 — `object-position`. Не наследуется. Default
    /// `50% 50%` (центр коробки).
    pub object_position: ObjectPosition,
    /// CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется.
    /// Default `Baseline`. Phase 0: parsing + storage; реальное применение
    /// (y_offset фрагмента в inline-flow и DrawText в paint) — отдельная
    /// задача с согласованием P2 (см. doc-comment на [`VerticalAlign`]).
    pub vertical_align: VerticalAlign,
    /// CSS Images L3 §6.1 — `image-rendering`. Inherited. Default `Auto`.
    /// Phase 0: parsing + storage; реальное переключение GPU sampler filter
    /// в `lumen-paint` (linear vs nearest-neighbour для `<img>` и background)
    /// — отдельная задача с согласованием P2.
    pub image_rendering: ImageRendering,
    /// CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited. Default `Row`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_direction: FlexDirection,
    /// CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited. Default `Nowrap`.
    /// Phase 0: parsing + storage; реальный multi-line flex — задача 4B.5.
    pub flex_wrap: FlexWrap,
    /// CSS Flexbox L2 `flex-wrap: balance` — `flex_wrap` is then `Wrap`/`WrapReverse`.
    pub flex_wrap_balance: bool,
    /// CSS Flexbox L2 `flex-line-count` — minimum line count of a balanced container. Initial 1.
    pub flex_line_count: u32,
    /// CSS Flexbox L1 §7.1 — `flex-grow`. Non-inherited. Default `0`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_grow: f32,
    /// CSS Flexbox L1 §7.2 — `flex-shrink`. Non-inherited. Default `1`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_shrink: f32,
    /// CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited. Default `Auto`.
    /// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
    pub flex_basis: FlexBasis,
    /// CSS Flexbox L1 §5.4 — `order`. Non-inherited. Initial: `0`.
    /// Управляет порядком отображения flex-элементов внутри контейнера.
    pub order: i32,
    /// CSS Grid Layout L1 §7.2 — `grid-template-columns`. Non-inherited.
    /// Default `[]` (no explicit tracks). Parsed track-list.
    pub grid_template_columns: Vec<GridTrackSize>,
    /// CSS Grid Layout L1 §7.2 — `grid-template-rows`. Non-inherited.
    /// Default `[]` (no explicit tracks). Parsed track-list.
    pub grid_template_rows: Vec<GridTrackSize>,
    /// CSS Grid Layout L1 §7.2 — auto-fill/auto-fit repeat metadata for columns.
    /// `Some` when `grid-template-columns` contains `repeat(auto-fill|auto-fit, ...)`.
    /// Resolved at layout time via `resolve_auto_fill_fit_count`. Non-inherited. Default `None`.
    pub grid_template_col_auto_repeat: Option<GridRepeat>,
    /// CSS Grid Layout L1 §7.2 — auto-fill/auto-fit repeat metadata for rows.
    /// `Some` when `grid-template-rows` contains `repeat(auto-fill|auto-fit, ...)`.
    /// Resolved at layout time via `resolve_auto_fill_fit_count`. Non-inherited. Default `None`.
    pub grid_template_row_auto_repeat: Option<GridRepeat>,
    /// CSS Grid Layout L1 §7.3 — `grid-template-areas`. Non-inherited.
    /// Default `[]` (none). Outer vec = rows (top-to-bottom), inner vec = columns
    /// (left-to-right). Each string is a cell name; `"."` means unnamed cell.
    pub grid_template_areas: Vec<Vec<String>>,
    /// CSS Grid Layout L1 §7.2.2 — имена линий `grid-template-columns`
    /// (`[a b] 100px [c]`). Индекс `i` — линия номер `i + 1`; пусто, если имён
    /// нет. Non-inherited. Default `[]`.
    pub grid_template_col_line_names: Vec<Vec<String>>,
    /// То же для `grid-template-rows`.
    pub grid_template_row_line_names: Vec<Vec<String>>,
    /// CSS Grid Layout L2 §9 — `repeat(auto-fill, <line-names>+)` в `subgrid <line-name-list>`
    /// столбцов: какие из `grid_template_col_line_names` — одно повторение. Non-inherited.
    pub grid_template_col_subgrid_fill: Option<NameFill>,
    /// То же для `grid-template-rows`.
    pub grid_template_row_subgrid_fill: Option<NameFill>,
    /// CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited. Default `Row`.
    pub grid_auto_flow: GridAutoFlow,
    /// CSS Masonry Layout §9 — `masonry-auto-flow`. Controls placement order in
    /// masonry containers. Non-inherited. Default `DefiniteFirst`.
    pub masonry_auto_flow: MasonryAutoFlow,
    /// CSS Grid Layout L3 §2–3 — `display: grid-lanes` и его `flow-tolerance`,
    /// `grid-lanes-direction`, `grid-lanes-pack`. Non-inherited.
    pub grid_lanes: GridLanes,
    /// CSS Grid Layout L1 §8.6 — `grid-auto-columns` (`<track-size>+`, the pattern repeats over the implicit tracks).
    /// Non-inherited. Default empty = a single `auto`.
    pub grid_auto_columns: Vec<GridTrackSize>,
    /// CSS Grid Layout L1 §8.6 — `grid-auto-rows` (`<track-size>+`, the pattern repeats over the implicit tracks).
    /// Non-inherited. Default empty = a single `auto`.
    pub grid_auto_rows: Vec<GridTrackSize>,
    /// CSS Grid Layout L1 §8.3 — `grid-column-start`. Non-inherited. Default `Auto`.
    pub grid_column_start: GridLine,
    /// CSS Grid Layout L1 §8.3 — `grid-column-end`. Non-inherited. Default `Auto`.
    pub grid_column_end: GridLine,
    /// CSS Grid Layout L1 §8.3 — `grid-row-start`. Non-inherited. Default `Auto`.
    pub grid_row_start: GridLine,
    /// CSS Grid Layout L1 §8.3 — `grid-row-end`. Non-inherited. Default `Auto`.
    pub grid_row_end: GridLine,
    /// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
    /// Default `Wrap`. Phase 0: parsing + storage; реальная связка с
    /// inline-flow line-breaker-ом (когда `Nowrap` подавляет soft wraps
    /// и эмитит overflowing line, эквивалентно legacy `white-space: nowrap`)
    /// — отдельная задача рядом с типизацией white-space (P1 1B).
    pub text_wrap_mode: TextWrapMode,
    /// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
    /// Default `Auto`. Phase 0: parsing + storage; реальная интерпретация
    /// `balance` / `pretty` / `stable` требует Knuth–Plass-style breaker-а
    /// и Unicode line-break tables — отложено до интеграции `UnicodeProvider`
    /// (provisional `icu4x`, P1 п.5).
    pub text_wrap_style: TextWrapStyle,
    /// CSS Overflow L4 / compat `-webkit-line-clamp` — максимальное число
    /// строк до обрезки текста. `None` = `none` (нет ограничения). Не
    /// наследуется. Phase 0: parsing + storage; реальное применение (truncate
    /// inline-flow после N-й строки и добавить ellipsis) — отдельная задача.
    pub line_clamp: Option<u32>,
    /// CSS Overflow L4 §line-clamp: `line-clamp: auto` — число строк задаёт высота (`height`/
    /// `max-height`), а не целое. Не наследуется; `line_clamp` при этом `None`, кроме формы
    /// `<n> auto` — там заданы оба, и действует то ограничение, что наступает раньше.
    pub line_clamp_auto: bool,
    /// Значение записано через `-webkit-line-clamp`: усечение действует только на
    /// `display: -webkit-box` с `-webkit-box-orient: vertical` (WPT `webkit-line-clamp-001/002`).
    pub line_clamp_legacy: bool,
    /// WHATWG Compat §2.1 — `-webkit-box-orient`. Не наследуется. Initial
    /// `Horizontal`. Phase 0: используется только для computed-value quirk
    /// `display: -webkit-box`/`-webkit-inline-box` (см. `webkit_box_computed_
    /// display`, `selector_query.rs`) — реального legacy-flexbox layout нет.
    pub box_orient: WebkitBoxOrient,
    /// WHATWG Compat §2.1 — `-webkit-box-pack` / `-webkit-box-align` / `-webkit-box-direction`:
    /// читаются пост-проходом каскада для `display: -webkit-box` (→ `justify-content`,
    /// `align-items`, `flex-direction`). Не наследуются.
    pub box_pack: WebkitBoxPack,
    pub box_align: WebkitBoxAlign,
    pub box_direction: WebkitBoxDirection,
    /// WHATWG Compat §2.1 — `-webkit-box-flex` (≥ 0, initial 0): flex-grow и flex-shrink
    /// элемента legacy-бокса. Не наследуется.
    pub box_flex: f32,
    /// WHATWG Compat §2.1 — `-webkit-box-ordinal-group` (≥ 1, initial 1): порядок элемента
    /// legacy-бокса (`order = group - 1`). Не наследуется.
    pub box_ordinal_group: u32,
    /// WHATWG Compat §2.1 — исходное значение `display: -webkit-box`/`-webkit-inline-box`,
    /// когда пост-проход каскада заменил его на `Flex`/`InlineFlex`, чтобы бокс раскладывался
    /// flex-алгоритмом (ось — из `-webkit-box-orient`), а `getComputedStyle` по-прежнему
    /// отдавал `-webkit-box` (`webkit_box_computed_display`). `None` — обычный `display`
    /// (в том числе зажатый `-webkit-line-clamp` бокс, он остаётся блоком). Не наследуется.
    pub legacy_box_display: Option<Display>,
    /// CSS Display 3 §2 — каноническая строка `display`, которую `Display` не выражает
    /// (`inline list-item`, `ruby`, `block math`, `run-in flex`, `ruby-text`), вместе с тем
    /// `Display`, в который её свёл разбор. Сериализатор верит строке, пока `style.display` равен
    /// этому `Display`: блокификация или каскад, поменявшие его, обесценивают форму. Не наследуется.
    pub display_form: Option<(Display, &'static str)>,
    /// CSS Overflow L4 §continue — `continue`. Не наследуется. Initial
    /// `Normal`. Phase 0: parsing + storage, участвует в том же computed-
    /// value quirk что и `box_orient`; фрагментационное поведение
    /// (`discard`/`collapse`) не реализовано.
    pub continue_value: CssContinue,
    /// CSS Fragmentation L3 §3.3 — `orphans`: минимальное число строк в конце
    /// фрагмента перед page/column break (сколько строк должно остаться «внизу»
    /// после разрыва). Inherited. Initial: 2. Phase 0: parsing + storage;
    /// реальная фрагментация — отдельная задача.
    pub orphans: u32,
    /// CSS Fragmentation L3 §3.3 — `widows`: минимальное число строк в начале
    /// фрагмента после page/column break (сколько строк должно перенестись
    /// «наверх» нового фрагмента). Inherited. Initial: 2. Phase 0: parsing + storage.
    pub widows: u32,
    /// CSS Containment L3 §3 — `contain`. NOT inherited. Initial: `NONE`.
    /// Phase 0: parse + store; containment enforcement in layout/paint — deferred.
    pub contain: ContainFlags,
    /// CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`.
    /// Phase 0: parse + store; skip-content optimization — deferred.
    pub content_visibility: ContentVisibility,
    /// CSS Box Sizing L4 §5 — `contain-intrinsic-width`. NOT inherited. Initial: `None`.
    /// Placeholder inline-size used as the box's intrinsic width when the element
    /// is subject to size containment (`contain: size`, `content-visibility: hidden`,
    /// or `content-visibility: auto` while skipped off-screen). `None` = the CSS
    /// keyword `none` (no placeholder; content-based width collapses). The optional
    /// `auto` keyword (last-remembered size) is parsed but treated as the length —
    /// its presence is kept in [`Self::contain_intrinsic_width_auto`] for
    /// serialization only, since the computed value the CSSOM must report is the
    /// specified `auto? [none | <length>]`, not just the length.
    /// Stored as a content-box `Length`, resolved against the font-size at layout.
    pub contain_intrinsic_width: Option<Length>,
    /// CSS Box Sizing L4 §5 — the `auto` keyword of `contain-intrinsic-width`.
    /// Behaviourally ignored (no last-remembered size); kept so
    /// `getComputedStyle` can serialise `auto 1px` rather than `1px`
    /// (BUG-852).
    pub contain_intrinsic_width_auto: bool,
    /// CSS Box Sizing L4 §5 — `contain-intrinsic-height`. NOT inherited. Initial: `None`.
    /// Placeholder block-size under size containment. See `contain_intrinsic_width`.
    pub contain_intrinsic_height: Option<Length>,
    /// CSS Box Sizing L4 §5 — the `auto` keyword of `contain-intrinsic-height`.
    /// See [`Self::contain_intrinsic_width_auto`].
    pub contain_intrinsic_height_auto: bool,
    /// CSS Sizing L4 §4.5 — `interpolate-size`. **Inherited.** Initial: `NumericOnly`.
    /// Controls whether keyword sizes (`auto`, `min-content`, …) participate in
    /// transitions/animations. Read by `TransitionScheduler::sync()` to gate
    /// `height: auto` interpolation.
    pub interpolate_size: InterpolateSizeMode,
    /// Interest Invokers — `interest-delay-start`. NOT inherited. Initial:
    /// `normal` (`None`); `Some(s)` — задержка в секундах. Читается JS-шимом
    /// через getComputedStyle, на раскладку не влияет.
    pub interest_delay_start: Option<f32>,
    /// Interest Invokers — `interest-delay-end`. См. [`Self::interest_delay_start`].
    pub interest_delay_end: Option<f32>,
    /// CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`.
    /// Phase 0: parse + store; @container query matching — deferred.
    pub container_type: ContainerType,
    /// CSS Conditional 5 §3 — `container-type: … scroll-state`: the box is a scroll-state
    /// query container (`@container scroll-state(…)`). Independent of the size axis in
    /// [`Self::container_type`]; NOT inherited. Initial: `false`.
    pub container_scroll_state: bool,
    /// CSS Container Queries L1 §3.2 — `container-name`. NOT inherited. Initial: empty.
    /// `none` = empty vec. Each name is a `<custom-ident>`.
    pub container_name: Vec<String>,
    /// CSS Filter Effects L2 §2 — `backdrop-filter`. NOT inherited. Initial: empty Vec (none).
    /// Same filter functions as `filter`. Phase 0: parse + store; backdrop blur in paint — deferred.
    pub backdrop_filter: Vec<FilterFn>,
    /// CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`.
    /// Phase 0: parse + store; print rendering path — deferred.
    pub print_color_adjust: PrintColorAdjust,
    /// CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`.
    /// Wired: `box_tree::apply_font_size_adjust` rescales the used `font_size`
    /// to `size · adjust / aspect` (aspect = font x-height / em) before layout.
    pub font_size_adjust: FontSizeAdjust,
    /// CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`.
    /// Phase 0: parse + store; vertical layout — deferred.
    pub writing_mode: WritingMode,
    /// CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`.
    /// Phase 0: parse + store; glyph rotation — deferred.
    pub text_orientation: TextOrientation,
    /// CSS Ruby L1 §3.4 — `ruby-position`. Inherited. Initial: `alternate` (`AlternateOver`).
    /// Read per annotation container by `build_ruby_box` (GAP-RUBYBOX-2).
    pub ruby_position: RubyPosition,
    /// CSS Ruby L1 §4 — `ruby-align`. Inherited. Initial: `SpaceAround`.
    pub ruby_align: RubyAlign,
    /// CSS Ruby L1 §4 — `ruby-merge`. Inherited. Initial: `Separate`.
    pub ruby_merge: RubyMerge,
    /// MathML Core §2.1.1 — `math-style`. Inherited. Initial: `Normal`.
    /// Drives `lay_out_mathml` (compact scales mfrac children); `<math>` box-tree integration — deferred.
    pub math_style: MathStyle,
    /// MathML Core §2.1.2 — `math-depth`. Inherited. Computed value: integer. Initial: `0`.
    /// `auto-add` / `add(<integer>)` resolve against the inherited depth at compute time.
    pub math_depth: i32,
    /// CSS Shapes L1 §3 — `shape-outside`. NOT inherited. Initial: `None`.
    /// Phase 0: parse + store; float-wrap shape application — deferred.
    pub shape_outside: ShapeOutside,
    /// CSS Shapes L1 §4 — `shape-margin: <length-percentage>`. NOT inherited. Initial: 0.
    pub shape_margin: Length,
    /// CSS Shapes L1 §5 — `shape-image-threshold: <number>`. NOT inherited. Initial: 0.0.
    pub shape_image_threshold: f32,
    /// CSS Motion Path L1 §3 — `offset-path`. NOT inherited. Initial: `None` (no motion path).
    /// Phase 0: parse + store; path-based motion animation — deferred.
    pub offset_path: Option<String>,
    /// CSS Motion Path L1 §3 — `offset-distance: <length-percentage>`. NOT inherited. Initial: `0`.
    pub offset_distance: Length,
    /// CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`.
    pub offset_rotate: OffsetRotate,
    /// CSS Motion Path L1 §3 — `offset-anchor`: auto | `<position>`. NOT inherited. Initial: `Auto` (None).
    pub offset_anchor: Option<ObjectPosition>,
    /// SVG §11.2 — `fill`. Inherited. Initial: `Color(Color::BLACK)` per SVG spec.
    /// Overrides the SVG default presentation fill for shape elements.
    pub svg_fill: SvgPaint,
    /// SVG §11.3 — `fill-opacity`. Inherited. Range 0.0–1.0. Initial: 1.0.
    pub svg_fill_opacity: f32,
    /// SVG §11.2 — `stroke`. Inherited. Initial: `None` (no stroke per SVG spec).
    pub svg_stroke: SvgPaint,
    /// SVG §11.3 — `stroke-opacity`. Inherited. Range 0.0–1.0. Initial: 1.0.
    pub svg_stroke_opacity: f32,
    /// SVG §11.4 — `stroke-width`. Inherited. In resolved px. Initial: 1.0.
    pub svg_stroke_width: f32,
    /// SVG §11.3 — `fill-rule`. Inherited. Initial: `NonZero`.
    pub svg_fill_rule: FillRule,
    /// SVG §14.3.4 — `clip-rule`. Inherited. Initial: `NonZero`.
    /// Fill rule used for the interior of a `<clipPath>` child shape (reuses
    /// [`FillRule`]). Parsed and cascaded; consumed once SVG `clip-path:
    /// url(#id)` references land.
    pub svg_clip_rule: FillRule,
    /// SVG §11.4 — `stroke-linecap`. Inherited. Initial: `Butt`.
    pub svg_stroke_linecap: StrokeLinecap,
    /// SVG §11.4 — `stroke-linejoin`. Inherited. Initial: `Miter`.
    pub svg_stroke_linejoin: StrokeLinejoin,
    /// SVG §11.4 — `stroke-miterlimit`. Inherited. Range ≥ 1.0. Initial: 4.0.
    pub svg_stroke_miterlimit: f32,
    /// SVG §11.4 — `stroke-dasharray`. Inherited. Empty = solid line (none).
    /// Resolved dash/gap lengths in px, repeated cyclically.
    pub svg_stroke_dasharray: Vec<f32>,
    /// SVG §11.4 — `stroke-dashoffset`. Inherited. In resolved px. Initial: 0.0.
    pub svg_stroke_dashoffset: f32,
    /// CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — `paint-order`. Inherited.
    /// Order fill/stroke/markers are painted; initial `normal` = fill, stroke,
    /// markers (fill drawn first, markers on top).
    pub paint_order: SvgPaintOrder,
    /// SVG 2 §11.6 / CSS — `text-anchor`. Inherited. `None` = not set by CSS
    /// (the SVG presentation attribute, then the `start` initial, applies at box
    /// build time). `Some(_)` = an author CSS rule won the cascade and overrides
    /// any presentation attribute (presentation attributes have specificity 0).
    pub text_anchor: Option<crate::box_tree::SvgTextAnchor>,
    /// SVG 2 §11.10.2 / CSS — `dominant-baseline`. Inherited. `None` = not set by
    /// CSS (the presentation attribute, then the `auto` initial, applies at box
    /// build time). `Some(_)` = author CSS rule overrides the attribute.
    pub dominant_baseline: Option<crate::box_tree::SvgDominantBaseline>,
    /// SVG 1.1 §10.9.2 / CSS Inline L3 §5.2 — `baseline-shift`. NOT inherited.
    /// Initial `baseline` (no shift). Positive lengths/percentages raise the text.
    pub baseline_shift: crate::box_tree::SvgBaselineShift,
    /// SVG 2 §Geometry — `cx`/`cy`/`x`/`y` (`<length-percentage>`, initial 0)
    /// and `r` (`<length-percentage [0,∞]>`). Not inherited. `em`-relative
    /// values are resolved to `Px` at cascade time; `%`/`calc(%)` stay typed.
    pub svg_cx: Length,
    pub svg_cy: Length,
    pub svg_r: Length,
    pub svg_x: Length,
    pub svg_y: Length,
    /// SVG 2 §Geometry — `rx`/`ry` (`auto | <length-percentage [0,∞]>`). Not inherited.
    pub svg_rx: LengthOrAuto,
    pub svg_ry: LengthOrAuto,
    /// SVG 2 §Painting — `color-interpolation`. Inherited. Initial `sRGB`.
    pub svg_color_interpolation: SvgColorInterpolation,
    /// SVG 2 §Geometry — `path-length` (`none | <number>`). Not inherited.
    pub svg_path_length: Option<f32>,
    // CSS Logical Properties L1 §2 — temporary storage for logical properties.
    // These are resolved to physical properties in resolve_logical_properties().
    /// CSS Logical Properties L1 — `inline-size`. `None` = auto.
    pub inline_size: Option<Length>,
    /// CSS Logical Properties L1 — `block-size`. `None` = auto.
    pub block_size: Option<Length>,
    /// CSS Logical Properties L1 §2 — `min-`/`max-` `inline-size`/`block-size`.
    /// Boxed and `None` unless one of the four is declared: four inline
    /// `Option<Length>`s would grow `ComputedStyle` enough to overflow the stack
    /// in deeply nested layout recursion (`deep_grid_chain` test).
    pub logical_min_max_sizes: Option<Box<LogicalMinMaxSizes>>,
    /// CSS Logical Properties L1 — `inset-inline-start`.
    pub inset_inline_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `inset-inline-end`.
    pub inset_inline_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `inset-block-start`.
    pub inset_block_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `inset-block-end`.
    pub inset_block_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-inline-start`.
    pub margin_inline_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-inline-end`.
    pub margin_inline_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-block-start`.
    pub margin_block_start: LengthOrAuto,
    /// CSS Logical Properties L1 — `margin-block-end`.
    pub margin_block_end: LengthOrAuto,
    /// CSS Logical Properties L1 — `padding-inline-start`.
    pub padding_inline_start: Length,
    /// CSS Logical Properties L1 — `padding-inline-end`.
    pub padding_inline_end: Length,
    /// CSS Logical Properties L1 — `padding-block-start`.
    pub padding_block_start: Length,
    /// CSS Logical Properties L1 — `padding-block-end`.
    pub padding_block_end: Length,
    /// CSS Logical Properties L1 — `border-inline-start-width`.
    pub border_inline_start_width: Option<f32>,
    /// CSS Logical Properties L1 — `border-inline-end-width`.
    pub border_inline_end_width: Option<f32>,
    /// CSS Logical Properties L1 — `border-block-start-width`.
    pub border_block_start_width: Option<f32>,
    /// CSS Logical Properties L1 — `border-block-end-width`.
    pub border_block_end_width: Option<f32>,
    /// CSS Logical Properties L1 §3.3 — `border-{start-start,start-end,end-start,end-end}-radius`
    /// as `(inline-axis, block-axis)` radii, in that order. `None` until one is declared; the
    /// post-cascade `resolve_logical_properties` maps each onto the physical corner the element's
    /// own `writing-mode` + `direction` select.
    pub border_logical_radii: Option<Box<LogicalCornerRadii>>,
    /// CSS Anchor Positioning L1 §2 — `anchor-name`. Custom-ident with `--` prefix.
    /// Non-inherited. When set, this element is registered as an anchor for positioned elements.
    pub anchor_name: Option<Box<str>>,
    /// CSS Anchor Positioning L1 §3 — `position-anchor`. Custom-ident with `--` prefix.
    /// Non-inherited. Names the default anchor element for `inset-area` and `anchor()` resolution.
    pub position_anchor: Option<Box<str>>,
    /// CSS Anchor Positioning L1 §5 — `inset-area` row (vertical axis) keyword. Non-inherited.
    pub inset_area_row: crate::anchor::InsetAreaKeyword,
    /// CSS Anchor Positioning L1 §5 — `inset-area` column (horizontal axis) keyword. Non-inherited.
    pub inset_area_col: crate::anchor::InsetAreaKeyword,
    /// CSS Anchor Positioning L1 §2.1 — `anchor-scope`. Non-inherited. Initial: `None`.
    /// Restricts which named anchors from this element's subtree are visible outside.
    pub anchor_scope: crate::anchor::AnchorScope,
    /// CSS Anchor Positioning L1 §3.1 — computed `position-area` text as
    /// `getComputedStyle` reports it (`inset_area_row/col` lose the keyword
    /// family). `None` = `none`.
    pub position_area_text: Option<Box<str>>,
    /// CSS Anchor Positioning L1 — `position-visibility`, canonical computed text.
    /// Non-inherited. `None` = the initial `anchors-visible`.
    pub position_visibility: Option<&'static str>,
    /// CSS Anchor Positioning L1 §4 — `anchor-size()` in `width`. Non-inherited. Initial: `None`.
    /// When set, the element's used width is the referenced anchor's dimension.
    pub anchor_size_w: Option<crate::anchor::AnchorSizeFunc>,
    /// CSS Anchor Positioning L1 §4 — `anchor-size()` in `height`. Non-inherited. Initial: `None`.
    /// When set, the element's used height is the referenced anchor's dimension.
    pub anchor_size_h: Option<crate::anchor::AnchorSizeFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `top`. Non-inherited. Initial: `None`.
    pub anchor_top: Option<crate::anchor::AnchorFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `right`. Non-inherited. Initial: `None`.
    pub anchor_right: Option<crate::anchor::AnchorFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `bottom`. Non-inherited. Initial: `None`.
    pub anchor_bottom: Option<crate::anchor::AnchorFunc>,
    /// CSS Anchor Positioning L1 §3.1 — `anchor()` in `left`. Non-inherited. Initial: `None`.
    pub anchor_left: Option<crate::anchor::AnchorFunc>,

    /// CSS View Transitions L1 §10 — `view-transition-name`. Non-inherited. Initial: `None` (none).
    /// Custom-ident that marks this element as a named view-transition capture target.
    /// During a `document.startViewTransition()` call the shell matches old/new snapshots
    /// by name and cross-fades them. `None` means the property is `none`.
    pub view_transition_name: Option<Box<str>>,
    /// CSS View Transitions L2 §2.2 — `view-transition-class: none | <custom-ident>+`.
    /// Non-inherited. Initial `None` (none); otherwise the idents joined by single spaces.
    pub view_transition_class: Option<Box<str>>,
    /// CSS View Transitions L2 §2.3 — `view-transition-group: normal | contain | nearest |
    /// <custom-ident>`. Non-inherited. Initial `None` (normal).
    pub view_transition_group: Option<Box<str>>,
    /// CSS View Transitions L2 §2.4 — `view-transition-scope: none | all`. Non-inherited.
    /// Initial `false` (none); `true` for `all`, which stops the name discovery of an
    /// enclosing transition at this element.
    pub view_transition_scope_all: bool,
    /// CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial `auto`.
    /// Supplies the glyph pairs for `content: open-quote` / `close-quote`.
    pub quotes: Quotes,
}

impl ComputedStyle {
    /// CSS Multi-column L1 §3.3 — the used `column-gap` of a multicol container in px:
    /// `normal` is `1em`, any other value resolves against `basis` (the content-box width).
    pub fn multicol_column_gap(&self, em: f32, basis: f32, viewport: lumen_core::geom::Size) -> f32 {
        if self.column_gap_normal {
            em.max(0.0)
        } else {
            self.column_gap.resolve_or_zero(em, basis, viewport).max(0.0)
        }
    }

    /// CSS Multi-column L2 §4.2 — the `column-height` in px (`None` for `auto`). `%` has no
    /// basis (the property takes no percentages), so only absolute and font-relative lengths
    /// resolve.
    pub fn column_height_px(&self, em: f32, viewport: lumen_core::geom::Size) -> Option<f32> {
        self.column_height.as_ref()?.resolve(em, None, viewport).filter(|h| *h >= 0.0)
    }

    /// CSS Multi-column L2 §4.4 — overflow columns start a new multicol row in the block
    /// direction: a non-auto `column-height` and `column-wrap` other than `nowrap`.
    pub fn column_rows_wrap(&self, em: f32, viewport: lumen_core::geom::Size) -> bool {
        !self.column_wrap_nowrap && self.column_height_px(em, viewport).is_some_and(|h| h > 0.0)
    }

    /// Used value `-webkit-text-stroke-color` (WHATWG Compat §3.2): цвет
    /// обводки глифов; `currentcolor` (initial) — это `color`.
    #[must_use]
    pub fn used_text_stroke_color(&self) -> Color {
        self.text_stroke_color.resolve(self.color)
    }

    /// Used value `-webkit-text-fill-color` (WHATWG Compat §3.1): цвет заливки
    /// глифов; `currentcolor` (initial) — это `color`. Декорации, тень и
    /// emphasis-marks остаются на `color`.
    pub fn used_text_fill_color(&self) -> Color {
        self.text_fill_color.resolve(self.color)
    }

    /// Used `pointer-events` для hit-test (CSS UI L4 §6.1/§6.4): `interactivity: inert`
    /// превращает любое значение в `none` — inert-бокс не бывает целью указателя.
    pub fn effective_pointer_events(&self) -> PointerEvents {
        if self.interactivity == Interactivity::Inert { PointerEvents::None } else { self.pointer_events }
    }

    /// Used value `caret-color` (CSS UI L4 §6.3): `auto` и `currentcolor`
    /// дают `color` элемента, остальное — явный цвет в sRGB.
    pub fn used_caret_color(&self) -> Color {
        self.caret_color.map_or(self.color, |c| c.resolve(self.color))
    }

    /// CSS 2.1 §17.6.1 / Basic UI L4 §5.2 — **used** value `outline-width`
    /// равно 0, если `outline-style` равен `none` (это spec, не аппроксимация).
    /// Computed `outline_width` хранится как есть (medium = 3 по UA convention),
    /// чтобы `outline-style: solid` без явного width давал видимый outline.
    pub fn outline_used_width(&self) -> f32 {
        if matches!(self.outline_style, OutlineStyle::None) {
            0.0
        } else {
            self.outline_width
        }
    }

    /// Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
    /// насыщенность, letter/word-spacing, декорация, фон, `user-select`). Используется для слияния
    /// inline-фрагментов в wrap_inline_run.
    pub fn text_rendering_eq(&self, other: &Self) -> bool {
        self.color == other.color
            && self.background_color == other.background_color
            && (self.font_size - other.font_size).abs() < f32::EPSILON
            && (self.line_height - other.line_height).abs() < f32::EPSILON
            && self.font_style == other.font_style
            && self.font_oblique_angle == other.font_oblique_angle
            && self.font_weight == other.font_weight
            && self.font_variant_caps == other.font_variant_caps
            && self.font_variant_ligatures == other.font_variant_ligatures
            && self.font_variant_numeric == other.font_variant_numeric
            && self.font_variant_position == other.font_variant_position
            && self.font_stretch == other.font_stretch
            && self.font_feature_settings == other.font_feature_settings
            && (self.letter_spacing - other.letter_spacing).abs() < f32::EPSILON
            && (self.letter_spacing_pct - other.letter_spacing_pct).abs() < f32::EPSILON
            && (self.word_spacing - other.word_spacing).abs() < f32::EPSILON
            && (self.word_spacing_pct - other.word_spacing_pct).abs() < f32::EPSILON
            && self.text_decoration_line == other.text_decoration_line
            && self.text_decoration_color == other.text_decoration_color
            && self.text_decoration_style == other.text_decoration_style
            && self.text_decoration_thickness == other.text_decoration_thickness
            // CSS UI L4 §6.2: selectability is decided per fragment (caret,
            // highlight, copy), so `user-select: none` text must not be glued
            // to a selectable neighbour.
            && self.user_select == other.user_select
            // CSS 2.1 §10.8: каждый фрагмент встаёт по своему `vertical-align`,
            // слитый с соседом остался бы на уровне первого.
            && self.vertical_align == other.vertical_align
    }
}
