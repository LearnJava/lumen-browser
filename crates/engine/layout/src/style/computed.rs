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
    AlignValue, AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationTimeline,
    Appearance, BackfaceVisibility, BackgroundLayer, BorderCollapse, BorderStyle, BoxShadow,
    BoxSizing, BreakValue, ClearSide, ClipPath, Color, ColorScheme, ContainerType, ContainFlags,
    Content, ContentVisibility, CssColor, Cursor, CustomProps, default_font_family, Direction,
    Display, EmptyCells, FieldSizing, FillRule, FilterFn, FlexBasis, FlexDirection, FlexWrap,
    FloatSide, FontFeatureSetting, FontOpticalSizing, FontPalette, FontSizeAdjust, FontStretch,
    FontStyle, FontVariantCaps, FontVariantEmoji, FontVariationSetting, FontWeight,
    ForcedColorAdjust, GridAutoFlow, GridLine, GridRepeat, GridTrackSize, Hyphens, ImageRendering,
    InterpolateSizeMode, Isolation, IterationCount, Length, LengthOrAuto, LineBreak,
    ListStylePosition, ListStyleType, MaskLayer, MasonryAutoFlow, MixBlendMode, ObjectFit,
    ObjectPosition, OffsetRotate, OutlineColor, OutlineStyle, Overflow, OverflowWrap,
    OverscrollBehavior, PointerEvents, Position, PositionComponent, PrintColorAdjust, Quotes,
    Resize, ScrollbarGutter, ScrollbarWidth, ScrollBehavior, ScrollSnapAlign, ScrollSnapStop,
    ScrollSnapType, ShapeOutside, StrokeLinecap, StrokeLinejoin, SvgPaint, SvgPaintOrder,
    TextAlign, TextAlignLast, TextDecorationLine, TextDecorationSkipInk, TextDecorationStyle,
    TextDecorationThickness, TextEmphasisPosition, TextEmphasisStyle, TextOrientation,
    TextOverflow, TextShadow, TextTransform, TextUnderlinePosition, TextWrapMode, TextWrapStyle,
    TimingFunction, TouchAction, TransformFn, TransformStyle, UnicodeBidi, UserSelect,
    VerticalAlign, Visibility, WhiteSpace, WhiteSpaceCollapse, WordBreak, WritingMode,
};

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
    /// CSS Writing Modes L4 §2.2 — участие бокса в UBA. NOT inherited.
    /// Initial: `Normal`. См. [`UnicodeBidi`].
    pub unicode_bidi: UnicodeBidi,
    pub color: Color,
    /// Цветовое пространство, в котором объявлен `color` (CSS Color L4 §10).
    /// Используется renderer-ом для точной передачи wide-gamut цветов в GPU.
    pub color_space: ColorSpace,
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
    /// CSS Rhythmic Sizing L1 §2 — `line-height-step` step unit in px.
    /// When `> 0`, each line box's used height is rounded up to the closest
    /// multiple of this value and the extra space is distributed as half-leading.
    /// `0.0` (initial) disables stepping. Resolved to absolute px at parse time
    /// (em/rem relative to the element's own font-size). Inherited.
    pub line_height_step: f32,
    pub font_style: FontStyle,
    pub font_weight: FontWeight,
    /// CSS Fonts L4 §6.2 — font-variant-caps (весь набор значений). Inherited.
    pub font_variant_caps: FontVariantCaps,
    /// CSS Fonts L4 §6.6 — font-variant-emoji. Inherited. На выбор глифа пока
    /// не влияет — см. [`FontVariantEmoji`].
    pub font_variant_emoji: FontVariantEmoji,
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
    /// CSS Text L3 §11.3: дополнительное расстояние **между словами**
    /// (resolved px). Inherited. В отличие от `letter-spacing`, добавляется
    /// только на word-boundary, не между всеми символами. Может быть
    /// отрицательным.
    pub word_spacing: f32,
    pub text_decoration_line: TextDecorationLine,
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
    /// CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Inherited.
    /// Initial: `OverRight` (horizontal writing-mode).
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
    /// CSS Overflow L3 §overflow-clip-margin — расширяет clip region при overflow:clip.
    /// Default: 0px. Не наследуется.
    pub overflow_clip_margin: Option<Length>,
    /// CSS UI L4 §10.1 — text-overflow. Не наследуется.
    pub text_overflow: TextOverflow,
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
    /// CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`.
    /// Phase 0: parse + store; применение при Forced Colors Mode — P2.
    pub forced_color_adjust: ForcedColorAdjust,
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
    /// Individual translate in px; composed BEFORE `transform` in the final matrix.
    /// `None` = `none` (identity). Not inherited.
    pub translate: Option<(f32, f32)>,
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
    /// CSS Multi-column L1 §3.2 — `column-count: <integer> | auto`. `None`
    /// = `auto`. Phase 0: parsing only.
    pub column_count: Option<u32>,
    /// CSS Multi-column L1 §3.3 — `column-width: <length> | auto`. Typed.
    /// `None` = `auto`. Phase 0: parsing only.
    pub column_width: Option<Length>,
    /// CSS Multi-column L1 §4.1 — `column-rule-width` (px). Default 0.
    pub column_rule_width: f32,
    /// CSS Multi-column L1 §4.2 — `column-rule-style`. Default `None`
    /// (без линии — линия рисуется только если style != None и width > 0).
    pub column_rule_style: BorderStyle,
    /// CSS Multi-column L1 §4.3 — `column-rule-color`. Initial = `CurrentColor`.
    pub column_rule_color: CssColor,
    /// CSS Gap Decorations L1 — `gap-rule-width` (px). Default 0. Non-inherited.
    /// Thickness of the visual rule drawn in flex/grid/multicol gaps.
    pub gap_rule_width: f32,
    /// CSS Gap Decorations L1 — `gap-rule-style`. Default `None`. Non-inherited.
    /// Rule is only visible when style != None and width > 0.
    pub gap_rule_style: BorderStyle,
    /// CSS Gap Decorations L1 — `gap-rule-color`. Default `CurrentColor`. Non-inherited.
    pub gap_rule_color: CssColor,
    /// CSS Tables L2 §17.6 — `border-collapse`. Inherited. Default `Separate`.
    /// When `Collapse`, `border-spacing` has no effect and adjacent cell borders merge.
    pub border_collapse: BorderCollapse,
    /// CSS Tables L2 §17.6.1.1 — `empty-cells`. Inherited. Default `Show`.
    /// When `Hide`, a table cell with no in-flow content draws neither borders nor
    /// background. No effect under `border-collapse: collapse`.
    pub empty_cells: EmptyCells,
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
    /// CSS Box Alignment L3 — alignment свойства для flex/grid items.
    /// Все не наследуются. Phase 0: parsing only.
    pub align_items: AlignValue,
    pub align_self: AlignValue,
    pub align_content: AlignValue,
    pub justify_items: AlignValue,
    pub justify_self: AlignValue,
    pub justify_content: AlignValue,
    /// CSS Backgrounds L3 §3 — стек фоновых слоёв. Первый элемент = верхний (рендерится поверх).
    /// Пустой Vec соответствует `background-image: none` без слоёв. `background-color` отдельно.
    pub background_layers: Vec<BackgroundLayer>,
    /// CSS Will Change L1. Список имён свойств для optimization hint.
    /// Пустой Vec = `auto` (default). Не наследуется.
    pub will_change: Vec<String>,
    /// CSS Pointer Events L1. Default `auto`. Не наследуется.
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
    /// CSS Text L3 §10.1 — `tab-size: <integer> | <length>`. Inherited.
    /// В пикселях если length; для integer хранится как число × 8 (default
    /// 8 spaces — стандартный default). Default 8 spaces = 64px при 8px-space.
    pub tab_size: f32,
    /// CSS UI L4 §6.3 — `caret-color: auto | <color>`. Inherited.
    /// `None` = auto (UA выбирает). `Some(color)` — явный цвет.
    pub caret_color: Option<Color>,
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
    /// CSS Scrollbars 1 — `scrollbar-color: auto | <color> <color>`
    /// (thumb-color + track-color).
    pub scrollbar_color: Option<(Color, Color)>,
    /// CSS Overflow L3 — `scrollbar-gutter: auto | stable | stable both-edges`.
    pub scrollbar_gutter: ScrollbarGutter,
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
    /// CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited. Default `Row`.
    pub grid_auto_flow: GridAutoFlow,
    /// CSS Masonry Layout §9 — `masonry-auto-flow`. Controls placement order in
    /// masonry containers. Non-inherited. Default `DefiniteFirst`.
    pub masonry_auto_flow: MasonryAutoFlow,
    /// CSS Grid Layout L1 §8.6 — `grid-auto-columns`. Non-inherited. Default `Auto`.
    pub grid_auto_columns: GridTrackSize,
    /// CSS Grid Layout L1 §8.6 — `grid-auto-rows`. Non-inherited. Default `Auto`.
    pub grid_auto_rows: GridTrackSize,
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
    /// CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`.
    /// Phase 0: parse + store; @container query matching — deferred.
    pub container_type: ContainerType,
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
    /// CSS Ruby L1 §4 — `ruby-position`. Inherited. Initial: `Over`.
    /// Drives `lay_out_ruby`; `<ruby>` box-tree integration — deferred.
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
    // CSS Logical Properties L1 §2 — temporary storage for logical properties.
    // These are resolved to physical properties in resolve_logical_properties().
    /// CSS Logical Properties L1 — `inline-size`. `None` = auto.
    pub inline_size: Option<Length>,
    /// CSS Logical Properties L1 — `block-size`. `None` = auto.
    pub block_size: Option<Length>,
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
    pub border_inline_start_width: f32,
    /// CSS Logical Properties L1 — `border-inline-end-width`.
    pub border_inline_end_width: f32,
    /// CSS Logical Properties L1 — `border-block-start-width`.
    pub border_block_start_width: f32,
    /// CSS Logical Properties L1 — `border-block-end-width`.
    pub border_block_end_width: f32,
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
    /// CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial `auto`.
    /// Supplies the glyph pairs for `content: open-quote` / `close-quote`.
    pub quotes: Quotes,
}

impl ComputedStyle {
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
    /// насыщенность, letter/word-spacing, декорация). Используется для слияния
    /// inline-фрагментов в wrap_inline_run.
    pub fn text_rendering_eq(&self, other: &Self) -> bool {
        self.color == other.color
            && (self.font_size - other.font_size).abs() < f32::EPSILON
            && (self.line_height - other.line_height).abs() < f32::EPSILON
            && self.font_style == other.font_style
            && self.font_weight == other.font_weight
            && self.font_variant_caps == other.font_variant_caps
            && self.font_stretch == other.font_stretch
            && self.font_feature_settings == other.font_feature_settings
            && (self.letter_spacing - other.letter_spacing).abs() < f32::EPSILON
            && (self.word_spacing - other.word_spacing).abs() < f32::EPSILON
            && self.text_decoration_line == other.text_decoration_line
            && self.text_decoration_color == other.text_decoration_color
            && self.text_decoration_style == other.text_decoration_style
            && self.text_decoration_thickness == other.text_decoration_thickness
    }

    /// Стартовые значения для корня документа.
    pub fn root() -> Self {
        Self {
            display: Display::Block,
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            color: Color::BLACK,
            color_space: ColorSpace::Srgb,
            background_color: None,
            font_size: 16.0,
            // No ancestor and no declaration yet — `zoom` starts neutral.
            effective_zoom: 1.0,
            line_height: 1.2,
            line_height_is_relative: true,
            line_height_step: 0.0,
            font_style: FontStyle::Normal,
            font_weight: FontWeight::NORMAL,
            font_variant_caps: FontVariantCaps::Normal,
            font_variant_emoji: FontVariantEmoji::Normal,
            font_stretch: FontStretch::NORMAL,
            font_family: default_font_family(),
            font_variation_settings: Vec::new(),
            font_feature_settings: Vec::new(),
            font_palette: FontPalette::Normal,
            font_palette_resolved: None,
            font_optical_sizing: FontOpticalSizing::Auto,
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            text_indent: Length::Px(0.0),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_decoration_line: TextDecorationLine::default(),
            text_decoration_color: CssColor::CurrentColor,
            text_decoration_style: TextDecorationStyle::Solid,
            text_decoration_thickness: TextDecorationThickness::Auto,
            text_emphasis_style: TextEmphasisStyle::None,
            text_emphasis_color: CssColor::CurrentColor,
            text_emphasis_position: TextEmphasisPosition::OverRight,
            text_underline_position: TextUnderlinePosition::Auto,
            text_underline_offset: None,
            text_decoration_skip_ink: TextDecorationSkipInk::Auto,
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            margin_top: LengthOrAuto::ZERO,
            margin_right: LengthOrAuto::ZERO,
            margin_bottom: LengthOrAuto::ZERO,
            margin_left: LengthOrAuto::ZERO,
            padding_top: Length::Px(0.0),
            padding_right: Length::Px(0.0),
            padding_bottom: Length::Px(0.0),
            padding_left: Length::Px(0.0),
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
            border_top_color: CssColor::CurrentColor,
            border_right_color: CssColor::CurrentColor,
            border_bottom_color: CssColor::CurrentColor,
            border_left_color: CssColor::CurrentColor,
            box_sizing: BoxSizing::ContentBox,
            position: Position::Static,
            top: LengthOrAuto::Auto,
            right: LengthOrAuto::Auto,
            bottom: LengthOrAuto::Auto,
            left: LengthOrAuto::Auto,
            z_index: None,
            float_side: FloatSide::None,
            clear: ClearSide::None,
            initial_letter_size: 1.0,
            initial_letter_sink: 0,
            isolation: Isolation::Auto,
            mix_blend_mode: MixBlendMode::Normal,
            border_top_left_radius: Length::Px(0.0),
            border_top_right_radius: Length::Px(0.0),
            border_bottom_right_radius: Length::Px(0.0),
            border_bottom_left_radius: Length::Px(0.0),
            border_top_left_radius_y: Length::Px(0.0),
            border_top_right_radius_y: Length::Px(0.0),
            border_bottom_right_radius_y: Length::Px(0.0),
            border_bottom_left_radius_y: Length::Px(0.0),
            visibility: Visibility::Visible,
            cursor: Cursor::Auto,
            box_shadow: Vec::new(),
            text_shadow: Vec::new(),
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            overflow_clip_margin: None,
            text_overflow: TextOverflow::Clip,
            opacity: 1.0,
            outline_width: 3.0,
            outline_style: OutlineStyle::None,
            outline_color: OutlineColor::Auto,
            outline_offset: Length::Px(0.0),
            accent_color: None,
            color_scheme: ColorScheme::Normal,
            forced_color_adjust: ForcedColorAdjust::Auto,
            custom_props: CustomProps::default(),
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            counter_set: Vec::new(),
            clip_path: None,
            transform: Vec::new(),
            translate: None,
            rotate: None,
            scale: None,
            filter: Vec::new(),
            row_gap: Length::Px(0.0),
            column_gap: Length::Px(0.0),
            column_count: None,
            column_width: None,
            column_rule_width: 0.0,
            column_rule_style: BorderStyle::None,
            column_rule_color: CssColor::CurrentColor,
            gap_rule_width: 0.0,
            gap_rule_style: BorderStyle::None,
            gap_rule_color: CssColor::CurrentColor,
            border_collapse: BorderCollapse::Separate,
            empty_cells: EmptyCells::Show,
            border_spacing_h: 0.0,
            border_spacing_v: 0.0,
            column_span_all: false,
            column_fill_balance: true,
            break_before: BreakValue::Auto,
            break_after: BreakValue::Auto,
            break_inside: BreakValue::Auto,
            aspect_ratio: None,
            align_items: AlignValue::Auto,
            align_self: AlignValue::Auto,
            align_content: AlignValue::Auto,
            justify_items: AlignValue::Auto,
            justify_self: AlignValue::Auto,
            justify_content: AlignValue::Auto,
            background_layers: Vec::new(),
            will_change: Vec::new(),
            pointer_events: PointerEvents::Auto,
            touch_action: TouchAction::Auto,
            appearance: Appearance::Auto,
            field_sizing: FieldSizing::Fixed,
            user_select: UserSelect::Auto,
            resize: Resize::None,
            scroll_behavior: ScrollBehavior::Auto,
            // CSS Scroll Snap / Overscroll defaults.
            scroll_snap_type: ScrollSnapType::default(),
            scroll_snap_align: ScrollSnapAlign::default(),
            scroll_snap_stop: ScrollSnapStop::default(),
            scroll_margin_top: 0.0,
            scroll_margin_right: 0.0,
            scroll_margin_bottom: 0.0,
            scroll_margin_left: 0.0,
            scroll_padding_top: 0.0,
            scroll_padding_right: 0.0,
            scroll_padding_bottom: 0.0,
            scroll_padding_left: 0.0,
            overscroll_behavior_x: OverscrollBehavior::Auto,
            overscroll_behavior_y: OverscrollBehavior::Auto,
            // CSS Text typography defaults.
            tab_size: 64.0,  // 8 spaces × 8px-space-width default.
            caret_color: None,  // `auto`.
            overflow_wrap: OverflowWrap::Normal,
            word_break: WordBreak::Normal,
            line_break: LineBreak::Auto,
            hyphens: Hyphens::Manual,
            transform_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5), 0.0),
            perspective: None,
            perspective_origin: (PositionComponent::Percent(0.5), PositionComponent::Percent(0.5)),
            transform_style: TransformStyle::Flat,
            backface_visibility: BackfaceVisibility::Visible,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: None,
            transition_properties: Vec::new(),
            transition_durations: Vec::new(),
            transition_delays: Vec::new(),
            transition_timing_functions: Vec::new(),
            transition_fill_modes: Vec::new(),
            animation_names: Vec::new(),
            animation_durations: Vec::new(),
            animation_timing_functions: Vec::new(),
            animation_delays: Vec::new(),
            animation_iteration_counts: Vec::new(),
            animation_directions: Vec::new(),
            animation_fill_modes: Vec::new(),
            animation_play_states: Vec::new(),
            animation_timelines: Vec::new(),
            scroll_timeline_name: None,
            scroll_timeline_axis: ScrollAxis::Block,
            view_timeline_name: None,
            view_timeline_axis: ScrollAxis::Block,
            mask_layers: Vec::new(),
            scrollbar_width: ScrollbarWidth::Auto,
            scrollbar_color: None,
            scrollbar_gutter: ScrollbarGutter::Auto,
            content: Content::Normal,
            object_fit: ObjectFit::Fill,
            object_position: ObjectPosition::default(),
            vertical_align: VerticalAlign::Baseline,
            image_rendering: ImageRendering::Auto,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: FlexBasis::Auto,
            order: 0,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_template_col_auto_repeat: None,
            grid_template_row_auto_repeat: None,
            grid_template_areas: Vec::new(),
            grid_auto_flow: GridAutoFlow::Row,
            masonry_auto_flow: MasonryAutoFlow::DefiniteFirst,
            grid_auto_columns: GridTrackSize::Auto,
            grid_auto_rows: GridTrackSize::Auto,
            grid_column_start: GridLine::Auto,
            grid_column_end: GridLine::Auto,
            grid_row_start: GridLine::Auto,
            grid_row_end: GridLine::Auto,
            text_wrap_mode: TextWrapMode::Wrap,
            text_wrap_style: TextWrapStyle::Auto,
            line_clamp: None,
            orphans: 2,
            widows: 2,
            contain: ContainFlags::NONE,
            content_visibility: ContentVisibility::Visible,
            contain_intrinsic_width: None,
            contain_intrinsic_width_auto: false,
            contain_intrinsic_height: None,
            contain_intrinsic_height_auto: false,
            interpolate_size: InterpolateSizeMode::NumericOnly,
            container_type: ContainerType::Normal,
            container_name: Vec::new(),
            backdrop_filter: Vec::new(),
            print_color_adjust: PrintColorAdjust::Economy,
            font_size_adjust: FontSizeAdjust::None,
            writing_mode: WritingMode::HorizontalTb,
            text_orientation: TextOrientation::Mixed,
            ruby_position: RubyPosition::Over,
            ruby_align: RubyAlign::SpaceAround,
            ruby_merge: RubyMerge::Separate,
            math_style: MathStyle::Normal,
            math_depth: 0,
            shape_outside: ShapeOutside::None,
            shape_margin: Length::Px(0.0),
            shape_image_threshold: 0.0,
            offset_path: None,
            offset_distance: Length::Px(0.0),
            offset_rotate: OffsetRotate::Auto,
            offset_anchor: None,
            // SVG presentation attributes — inherited. Initial per SVG §11.2/11.3/11.4.
            svg_fill: SvgPaint::Color(Color::BLACK),
            svg_fill_opacity: 1.0,
            svg_stroke: SvgPaint::None,
            svg_stroke_opacity: 1.0,
            svg_stroke_width: 1.0,
            svg_fill_rule: FillRule::NonZero,
            svg_clip_rule: FillRule::NonZero,
            svg_stroke_linecap: StrokeLinecap::Butt,
            svg_stroke_linejoin: StrokeLinejoin::Miter,
            svg_stroke_miterlimit: 4.0,
            svg_stroke_dasharray: Vec::new(),
            svg_stroke_dashoffset: 0.0,
            paint_order: SvgPaintOrder::default(),
            text_anchor: None,
            dominant_baseline: None,
            baseline_shift: crate::box_tree::SvgBaselineShift::Baseline,
            // CSS Logical Properties L1 — initial values.
            inline_size: None,
            block_size: None,
            inset_inline_start: LengthOrAuto::Auto,
            inset_inline_end: LengthOrAuto::Auto,
            inset_block_start: LengthOrAuto::Auto,
            inset_block_end: LengthOrAuto::Auto,
            margin_inline_start: LengthOrAuto::ZERO,
            margin_inline_end: LengthOrAuto::ZERO,
            margin_block_start: LengthOrAuto::ZERO,
            margin_block_end: LengthOrAuto::ZERO,
            padding_inline_start: Length::Px(0.0),
            padding_inline_end: Length::Px(0.0),
            padding_block_start: Length::Px(0.0),
            padding_block_end: Length::Px(0.0),
            border_inline_start_width: 0.0,
            border_inline_end_width: 0.0,
            border_block_start_width: 0.0,
            border_block_end_width: 0.0,
            anchor_name: None,
            position_anchor: None,
            inset_area_row: crate::anchor::InsetAreaKeyword::None,
            inset_area_col: crate::anchor::InsetAreaKeyword::None,
            anchor_scope: crate::anchor::AnchorScope::None,
            anchor_size_w: None,
            anchor_size_h: None,
            anchor_top: None,
            anchor_right: None,
            anchor_bottom: None,
            anchor_left: None,
            view_transition_name: None,
            quotes: Quotes::Auto,
        }
    }
}
