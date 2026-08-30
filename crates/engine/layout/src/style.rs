//! Style cascade с поддержкой compound и complex selectors, attribute и
//! pseudo-class matching, specificity по CSS Selectors Level 3.
//!
//! Алгоритм каскада: для каждого правила в stylesheet проверяем, матчит ли оно
//! целевой элемент. Если матчит — для каждой декларации записываем «применять с
//! приоритетом (specificity, source_order)». В конце сортируем все
//! применимые декларации по этому ключу (по возрастанию) и применяем — так
//! правило с большей specificity перекрывает меньшую, а при равенстве выигрывает
//! более позднее.
//!
//! Matching complex selector-а — справа налево, жадно: для каждого combinator-а
//! берём первого подходящего предка/sibling-а без back-tracking. Для большинства
//! реальных страниц этого достаточно; патологические случаи `a b c` с
//! вложенными `a`-предками могут промахнуться — это известное упрощение, до
//! фазы со «честным» Selectors-движком.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::rule_index::RuleIndex;

use lumen_core::geom::Size;
use lumen_css_parser::{
    ComplexSelector,
    Declaration, MediaContext,
    PseudoElementKind, Rule, SimpleSelector, Stylesheet, StylesheetRevision,
    SUPPORTED_PROPERTIES,
};
use lumen_dom::NodeId;

// Разбор CSS-значений, применение значений и табличные справочники, вынесенные
// из этого файла батчами SPLIT-ST3…ST10 (docs/tasks/p1-monolith-split-queue.md §4).
// `matching` — матчинг селекторов (SPLIT-ST12): `matches_complex`/`matches_slotted_complex`
// вызываются отсюда, поэтому реэкспорт/импорт ниже, в блоке ST-12.
mod adjust;
mod apply;
mod calc;
mod cascade;
mod computed;
mod container;
mod env;
mod logical;
mod matching;
mod parse;
mod presentational;
mod property_syntax;
mod pseudo;
mod quirks;
mod restyle;
mod shorthand;
mod substitute;
mod ua;
mod values;

// SPLIT-ST15. `ComputedStyle` (объявление + `impl`) уехала в `style::computed`.
// Это центральный тип публичной поверхности крейта: `lib.rs` реэкспортирует его
// наружу, а по репозиторию его зовут по старому пути `lumen_layout::style::
// ComputedStyle`, поэтому реэкспорт обязателен (правило §2.1).
pub use computed::ComputedStyle;
// SPLIT-ST8: сама `apply_declaration` вместе со своим `match prop` уехала в
// `style::apply`; вызыватели в этом файле остались на прежнем имени.
use apply::apply_declaration;
// SPLIT-ST14. Главный проход каскада уехал в `style::cascade`. `compute_style`
// и `take_compute_style_calls` — публичная поверхность крейта (`pub mod style`
// в `lib.rs`, реэкспорт `compute_style` из `lib.rs`; зовут `box_tree.rs`,
// `lumen-driver` и census-тест `lumen-shell`), поэтому реэкспорт обязателен
// даже без вызывателя внутри `style.rs` (правило §2.1). `parse_zoom` зовёт
// только `style::tests::cascade` через цепочку `use super::*` — отсюда
// `#[cfg(test)]` на её реэкспорте (прецедент SH-3a/SPLIT-ST11).
pub use cascade::{compute_style, take_compute_style_calls};
#[cfg(test)]
pub(in crate::style) use cascade::parse_zoom;
// `NodeData` перестала быть нужна производственному `style.rs` вместе с
// `compute_style`, но её по-прежнему читают `style::tests::{color,restyle}`
// через `use super::*` — отсюда `cfg`-импорт (тот же приём, что у
// `presentational::parse_legacy_color_html_attr` из SPLIT-ST11).
#[cfg(test)]
use lumen_dom::NodeData;
// SPLIT-ST17. `Document`/`DocumentMode` перестали быть нужны производственному
// `style.rs` вместе с регионом, что уехал в `style::container`, но их
// по-прежнему читает `style::tests::{cascade,node_fanout_tests}` через
// `use super::*` — та же ловушка SH-3a, седьмой раз подряд.
#[cfg(test)]
use lumen_dom::{Document, DocumentMode};
// SPLIT-ST15. `MathStyle` и `Ruby*` держали только поля `ComputedStyle`, но их
// по-прежнему читает `style::tests` (`mod.rs`, ruby- и mathml-кейсы) через `use super::*` — та же
// ловушка SH-3a, что и у `NodeData` выше, шестой раз подряд. `ResolvedFontPalette`
// вместе с полем ушла насовсем и импорта здесь больше не имеет.
#[cfg(test)]
use crate::mathml::MathStyle;
#[cfg(test)]
use crate::ruby::{RubyAlign, RubyMerge, RubyPosition};
// SPLIT-ST13. Пост-каскадные правки (`style::adjust`), псевдоэлементы
// (`style::pseudo`) и валидация `@property`-синтаксиса (`style::property_syntax`).
// Три `pub fn` из них — публичная поверхность крейта (`pub mod style` в `lib.rs`;
// `box_tree.rs`/`counters.rs`/`lib.rs` зовут их по старому пути), поэтому реэкспорт
// обязателен даже там, где вызывателя внутри `style.rs` не осталось (правило §2.1).
use adjust::{
    apply_forced_colors_mode, apply_webkit_scrollbar_pseudos, coerce_overflow_axes,
    resolve_system_colors_in_style,
};
pub use property_syntax::validate_against_syntax;
use property_syntax::apply_property_initial_values;
pub use pseudo::{compute_pseudo_element_style, compute_selection_style, merge_pseudo_inherited};
pub(in crate::style) use pseudo::pseudo_element_name;
// Пост-каскадная `resolve_logical_properties` дописана в созданный ST-7
// `style::logical` тем же батчем; её единственный вызыватель — `compute_style`.
use logical::resolve_logical_properties;
// Реэкспорт со старого пути: `resolve_logical_property` — публичный API крейта
// (`pub mod style` в `lib.rs`), вызывателей внутри `style.rs` у неё нет, поэтому
// без реэкспорта путь `lumen_layout::style::resolve_logical_property` пропал бы.
pub use logical::resolve_logical_property;
pub use parse::color::{parse_color, system_color};
// Реэкспорт со старого пути: этих троих зовут `lib.rs` и `animation.rs`, то есть
// потребитель вне `crate::style` (правило §2.1 очереди SPLIT).
pub use parse::image::{parse_background_gradient, parse_gradient_stops};
pub use parse::transform::parse_transform_list;
use parse::color::{named_color, parse_color_legacy};
// SPLIT-ST9. `CalcNode`/`MathFn`/`RoundStrategy`/`Length`/`LengthOrAuto`/
// `parse_length` — публичная поверхность крейта (`pub mod style` в `lib.rs`,
// обращения `lumen_layout::style::<Имя>` из шести крейтов), поэтому реэкспорт
// обязателен даже там, где вызывателя внутри `style.rs` уже нет (правило §2.1).
pub use calc::{CalcNode, MathFn, RoundStrategy};
pub use values::length::{parse_length, Length, LengthOrAuto};
// SPLIT-ST16. Типы значений — типографика/текст, цвет, бокс-модель, тайминг —
// уехали в `style::values::{typography,color,box_model,timing}`. Все четыре
// группы — публичная поверхность крейта (`pub mod style` в `lib.rs`; `box_tree.rs`,
// `lumen-paint`, `lumen-shell` и другие крейты зовут их по старому пути
// `lumen_layout::style::<Имя>`), поэтому реэкспорт обязателен даже там, где
// вызывателя внутри `style.rs` уже нет (правило §2.1).
pub use values::typography::{
    ColorScheme, Cursor, Direction, Display, FontFeatureSetting, FontOpticalSizing, FontStretch,
    FontStyle, FontVariantCaps, FontVariantEmoji, FontVariationSetting, FontWeight,
    ForcedColorAdjust, Overflow, TextAlign, TextAlignLast, TextDecorationLine,
    TextDecorationSkipInk, TextDecorationStyle, TextDecorationThickness, TextEmphasisPosition,
    TextEmphasisShape, TextEmphasisStyle, TextOverflow, TextShadow, TextTransform,
    TextUnderlinePosition, UnicodeBidi, Visibility, WhiteSpace, WhiteSpaceCollapse, BoxShadow,
    text_font_features,
};
pub use values::color::{Color, ColorFloat, CssColor, SystemColor};
pub use values::box_model::{
    BorderCollapse, BorderStyle, BoxSizing, BreakValue, ClearSide, EmptyCells, FillRule,
    FloatSide, Isolation, MixBlendMode, OutlineColor, OutlineStyle, PaintOrderSlot, Position,
    StrokeLinecap, StrokeLinejoin, SvgPaint, SvgPaintOrder, VerticalAlign,
};
pub use values::timing::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationTimeline, CssWideKeyword,
    CustomProps, IterationCount, LinearEasingPoint, StepPosition, TimingFunction,
    parse_css_wide_keyword,
};
// SPLIT-ST17. Хвост типов значений — содержимое/списки/перенос/интерактивность
// (`style::values::misc`), containment и контейнерные запросы (`style::container`,
// не тип значения, а вычисление), формы/мотион-путь/письмо/scroll-snap
// (`style::values::scroll`), градиенты/фон/маски (`style::values::background`),
// flex/grid/выравнивание (`style::values::flexgrid`), clip-path/transform/filter/mask
// (`style::values::transform`). Все шесть групп — публичная поверхность крейта
// (`pub mod style` в `lib.rs`; `style::apply::*` и другие крейты зовут их по
// старому пути `lumen_layout::style::<Имя>`), поэтому реэкспорт обязателен даже
// там, где вызывателя внутри `style.rs` уже нет (правило §2.1).
pub use values::misc::{
    Appearance, Content, ContentItem, FieldSizing, Hyphens, LineBreak, ListStylePosition,
    ListStyleType, OverflowWrap, PointerEvents, Quotes, Resize, ScrollbarGutter, ScrollbarWidth,
    TouchAction, WordBreak,
};
pub use container::{
    apply_container_rules, evaluate_container_condition, ContainFlags, ContainerContext,
    ContainerType, ContentVisibility, InterpolateSizeMode,
};
pub use values::scroll::{
    FontSizeAdjust, OffsetRotate, OverscrollBehavior, PrintColorAdjust, ScrollBehavior,
    ScrollSnapAlign, ScrollSnapAlignKeyword, ScrollSnapAxis, ScrollSnapStop, ScrollSnapStrictness,
    ScrollSnapType, ShapeOutside, TextOrientation, UserSelect, WritingMode,
};
pub use values::background::{
    BackgroundAttachment, BackgroundClip, BackgroundImage, BackgroundLayer, BackgroundOrigin,
    BackgroundRepeat, BackgroundSize, BgSizeAxis, GradientCorner, ImageRendering, MaskClip,
    ObjectFit, ParsedGradient, RadialShape, RadialSize, radial_gradient_radii,
};
pub use values::flexgrid::{
    AlignValue, FlexBasis, FlexDirection, FlexWrap, GridAutoFlow, GridLine, GridRepeat,
    GridTrackSize, MasonryAutoFlow, ObjectPosition, PositionComponent, RepeatCount,
    TextWrapMode, TextWrapStyle,
};
// `parse_auto_repeat` была `pub(crate)` в доноре (зовёт только `style::apply::layout`
// внутри крейта, не публичная поверхность наружу) — реэкспорт сужен так же.
pub(crate) use values::flexgrid::parse_auto_repeat;
// `parse_position_component` тоже была приватной в доноре, но её зовёт
// сосед `style::apply::motion` — реэкспорт сужен до `crate::style`.
pub(in crate::style) use values::flexgrid::parse_position_component;
pub use values::transform::{
    BackfaceVisibility, ClipPath, FilterFn, GradientStop, MaskComposite, MaskLayer, MaskMode,
    ShapeValue, TransformFn, TransformStyle,
};
// Вызывателей внутри производственного `style.rs` у них нет — только
// `style::tests::*` через цепочку `use super::*` (тот же приём, что у
// `NodeData`/`MathStyle` выше, SPLIT-ST16).
#[cfg(test)]
use values::color::srgb_gamma_decode;
#[cfg(test)]
use crate::scroll_timeline::ScrollAxis;
// `expand_vars_and_env` — `pub(crate)`, её зовёт `lib.rs`; остальные видны только
// внутри `style` и его потомков, поэтому и реэкспорт сужен до `crate::style`.
pub(crate) use substitute::expand_vars_and_env;
pub(in crate::style) use substitute::{expand_attr_val, expand_custom_functions, expand_vars};
pub(in crate::style) use values::length::{parse_length_q, parse_sizing_length};
use parse::font_size::{FontSizeBasis, apply_font_size};

// SPLIT-ST10. Окружение прохода уехало в `style::env`, фан-аут рестайла — в
// `style::restyle`. `pub`-функции обоих — публичная поверхность крейта
// (`pub mod style` в `lib.rs`; `box_tree.rs`/`incremental.rs`/`lib.rs` зовут их
// по старому пути), поэтому реэкспорт обязателен даже там, где вызывателя
// внутри `style.rs` нет (правило §2.1).
pub use env::{
    clear_cq_context, clear_interactive_state, cq_context_active, forced_colors_active,
    pop_ch_ex_context, print_media_active, push_ch_ex_context, set_cq_context, set_forced_colors,
    set_interactive_state, set_print_media, StyleEnvSnapshot,
};
pub use restyle::{
    restyle_node_index, restyle_root_set_for_node_change, restyle_root_set_for_state_change,
    restyle_state_index, NodeChange, NodeRestyleIndex, StateRestyleIndex,
};
// `CONTAINER_CQ`/`FONT_CH_EX` читает `style::values::length` по старому пути
// `crate::style::…` (SPLIT-ST9): это реэкспорт, а не импорт, — своих вызывателей
// в `style.rs` у обеих нет.
pub(in crate::style) use env::{CONTAINER_CQ, FONT_CH_EX};
use env::{media_context_from_viewport, node_in_scope};

// SPLIT-ST12. Матчинг селекторов уехал в `style::matching`; `matches_complex`
// уже был `pub(crate)` — сохраняем путь `crate::style::matches_complex`
// (сторонние читатели вне `style`: `selector_query.rs`, `starting_style.rs`,
// `style::env`). `matches_simple` был приватным, но её путь `crate::style::
// matches_simple` читает `style::restyle` (сосед `style::matching`, не его
// потомок) — тот же реэкспорт, суженный до `crate::style`.
// `complex_has_host`/`matches_slotted_complex` вызывает только сам `style.rs`.
pub(crate) use matching::matches_complex;
pub(in crate::style) use matching::matches_simple;
use matching::{complex_has_host, matches_slotted_complex};

// SPLIT-ST11. UA-таблица (`style::ua`), презентационные HTML-атрибуты
// (`style::presentational`), quirks-режим (`style::quirks`) и шрифтовые
// парсеры (`style::parse::font`) уехали из этого файла; вызыватели в этом
// файле и в `style::tests::*` (через цепочку `use super::*`) остались на
// прежних именах, поэтому реэкспорт нужен даже там, где `pub` не требуется
// снаружи крейта (правило §2.1 очереди SPLIT).
use presentational::{
    apply_align_presentational_hint, apply_background_image_presentational_hint,
    apply_bgcolor_presentational_hint, apply_bordercolor_presentational_hint,
    apply_cellspacing_presentational_hint, apply_font_element_presentational_hints,
    apply_image_presentational_hints, apply_svg_presentational_hints,
    apply_table_cell_width_hint, apply_text_color_presentational_hint,
};
// Вызывателя внутри производственного `style.rs` у неё нет — только
// `style::tests::ua` (правило SH-3a про cfg-реэкспорт «пользуются только
// тестами», подтверждено уже на SPLIT-ST3b/ST9). `hex_digit_value`/
// `is_svg_presentational_element`/`parse_html_dimension`/
// `parse_html_length_attr` зовутся только изнутри `presentational.rs` самой
// же, реэкспорт им не нужен вовсе.
#[cfg(test)]
use presentational::parse_legacy_color_html_attr;
use quirks::{apply_quirks_html_height, apply_quirks_line_height, apply_quirks_table_reset};
use ua::{
    apply_ua_body_margin, apply_ua_dialog_display, apply_ua_form_controls,
    apply_ua_form_controls_field_sizing_clear, apply_ua_heading_style, apply_ua_hr_style,
    apply_ua_inert, apply_ua_table_cell_padding, apply_ua_text_decoration, default_display,
    strip_ua_appearance_box_styling, ua_font_family, ua_font_size_factor, ua_font_style,
    ua_font_weight, ua_link_color, ua_vertical_align, ua_white_space,
};
pub use ua::ua_form_element_colors;
// Восемь `pub` item-ов — публичная поверхность крейта (`pub mod style` в
// `lib.rs`; `font_palette.rs`/`animation.rs`/`lib.rs` и другие крейты зовут их
// по старому пути `lumen_layout::style::<Имя>`), реэкспорт обязателен даже там,
// где вызывателя внутри `style.rs` нет (правило §2.1).
pub use parse::font::{
    default_font_family, parse_font_family, parse_font_feature_settings, parse_font_palette,
    parse_font_variation_settings, FontPalette, DEFAULT_FONT_FAMILY, ROOT_FONT_SIZE,
};
use parse::font::parse_font_weight;

/// BUG-284: cascade-wide rule index — the top-level [`RuleIndex`] plus one
/// per-block index for every `@layer`/`@media`/`@supports` block, in the same
/// order as `Stylesheet.layers`/`media_rules`/`supports_rules`.
///
/// Before this, only the top-level `rules` were indexed; the `@layer`/`@media`/
/// `@supports` loops in `compute_style` brute-force scanned every rule in
/// every block for every node. Real-world stylesheets often put the bulk of
/// their rules inside `@media` breakpoints, which made that brute-force scan
/// the dominant cascade cost (observed: ~1.1ms/node on a page with ~1100
/// styled nodes and ~3000 rules, most inside `@media`).
struct CascadeIndex {
    rules: RuleIndex,
    layers: Vec<RuleIndex>,
    media: Vec<RuleIndex>,
    supports: Vec<RuleIndex>,
    /// Perf (docs/tasks/p3-cascade-perf.md Задача 1): whether each
    /// `sheet.media_rules[i]`/`sheet.supports_rules[i]` block is currently
    /// active, precomputed once per (sheet, viewport, dark_mode) instead of
    /// re-evaluating `media.query.matches(..)`/`supports.condition.evaluate(..)`
    /// for every block on every node. This loop is node-independent — the
    /// per-node re-evaluation (×2 call sites: `compute_style` and
    /// `compute_pseudo_element_style`, the latter running twice per element
    /// for `::before`/`::after`) was the dominant cascade cost on stylesheets
    /// that put most rules inside `@media` breakpoints (profiled: ~60% of
    /// `compute_style`'s matching phase on a 3000-rule real-world sheet).
    active_media: Vec<bool>,
    active_supports: Vec<bool>,
    /// BUG-341 S10 — whether the sheet contains *any* rule whose subject is a
    /// `::-webkit-scrollbar*` pseudo-element. `compute_style` translates those
    /// onto `scrollbar-width`/`scrollbar-color` (CC-CSS-1) by running three
    /// extra pseudo-element cascades on **every** element; on a sheet with no
    /// such rule — every page that is not Lumen's own chrome — all three were
    /// pure waste. Node-independent, so it is decided once per sheet here.
    has_webkit_scrollbar_rules: bool,
    /// BUG-341 S10 — whether any declaration in the sheet mentions `quote`
    /// (`content: open-quote`, `quotes: …`). `counters::walk` probes
    /// `::before`/`::after` on every node solely to keep the CSS Generated
    /// Content L3 §3.2 quote-nesting counter continuous; with no quote
    /// anywhere in the sheet that probe cannot produce a depth, so it is
    /// skipped. Deliberately a substring test over raw declaration values: it
    /// over-approximates (a `--quote-color` custom property arms it) and must,
    /// because a `var()` can smuggle `open-quote` in from anywhere. `attr()`
    /// arms it too: that value comes from the DOM, which a sheet-level
    /// predicate cannot see.
    has_quote_content: bool,
    /// BUG-341 S23 — every pseudo-element name the sheet uses as the **subject**
    /// of a selector, lowercased and deduplicated.
    ///
    /// `matches_complex_for_pseudo` only ever looks at the subject compound, so
    /// a name absent from this list cannot match anywhere in the sheet and
    /// `compute_pseudo_element_style` for it is guaranteed to return `None`.
    /// That guarantee is what lets the callers skip whole traversals rather than
    /// individual cascades: `apply_first_line_pseudo_styles` walks the laid-out
    /// tree and probes `::first-line` on every block box — 123 probes with zero
    /// hits per cycle on `chrome.html`, which has no `::first-line` rule at all,
    /// and the largest single item of an interaction frame's pseudo stage.
    ///
    /// A `Vec` rather than a `HashSet`: real sheets use a handful of distinct
    /// pseudo-elements, and a linear `eq_ignore_ascii_case` scan over ≤10 short
    /// names beats hashing a `&str` (and needs no allocation at the call site).
    pseudo_subjects: Vec<Box<str>>,
}

impl CascadeIndex {
    fn empty() -> Self {
        Self {
            rules: RuleIndex::empty(),
            layers: Vec::new(),
            media: Vec::new(),
            supports: Vec::new(),
            active_media: Vec::new(),
            active_supports: Vec::new(),
            has_webkit_scrollbar_rules: false,
            has_quote_content: false,
            pseudo_subjects: Vec::new(),
        }
    }

    /// Builds the index, timing each of its four phases into the returned
    /// [`CascadeIndexStats`] (BUG-341 S21 census — four clock reads per rebuild,
    /// against a rebuild that walks every rule in the sheet several times).
    fn build(sheet: &Stylesheet, media_ctx: &MediaContext) -> (Self, CascadeIndexStats) {
        let t = std::time::Instant::now();
        let rules = RuleIndex::build(sheet);
        let rules_ns = t.elapsed().as_nanos() as u64;

        let t = std::time::Instant::now();
        let layers: Vec<RuleIndex> =
            sheet.layers.iter().map(|l| RuleIndex::build_from_rules(&l.rules)).collect();
        let media: Vec<RuleIndex> =
            sheet.media_rules.iter().map(|m| RuleIndex::build_from_rules(&m.rules)).collect();
        let supports: Vec<RuleIndex> =
            sheet.supports_rules.iter().map(|s| RuleIndex::build_from_rules(&s.rules)).collect();
        let blocks_ns = t.elapsed().as_nanos() as u64;

        let t = std::time::Instant::now();
        let active_media: Vec<bool> =
            sheet.media_rules.iter().map(|m| m.query.matches(media_ctx)).collect();
        let active_supports: Vec<bool> = sheet
            .supports_rules
            .iter()
            .map(|s| s.condition.evaluate(SUPPORTED_PROPERTIES))
            .collect();
        let active_ns = t.elapsed().as_nanos() as u64;

        let t = std::time::Instant::now();
        let has_webkit_scrollbar_rules =
            all_rules(sheet).any(|r| r.selectors.iter().any(selector_targets_webkit_scrollbar));
        let has_quote_content = all_rules(sheet).any(|r| {
            r.declarations
                .iter()
                .any(|d| value_mentions_quote(&d.value) || d.value.contains("attr("))
        });
        let mut pseudo_subjects: Vec<Box<str>> = Vec::new();
        for rule in all_rules(sheet) {
            for selector in &rule.selectors {
                for name in selector_pseudo_subjects(selector) {
                    if !pseudo_subjects.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                        pseudo_subjects.push(name.to_ascii_lowercase().into_boxed_str());
                    }
                }
            }
        }
        let predicates_ns = t.elapsed().as_nanos() as u64;

        let idx = Self {
            rules,
            layers,
            media,
            supports,
            active_media,
            active_supports,
            has_webkit_scrollbar_rules,
            has_quote_content,
            pseudo_subjects,
        };
        let stats = CascadeIndexStats {
            builds: 1,
            build_ns: rules_ns + blocks_ns + active_ns + predicates_ns,
            rules_ns,
            blocks_ns,
            active_ns,
            predicates_ns,
        };
        (idx, stats)
    }
}

/// Every `Rule` in `sheet`, whichever container it sits in (top level,
/// `@media`, `@supports`, `@layer`, `@scope`). Used for the node-independent
/// sheet-wide predicates on [`CascadeIndex`]; a rule's container decides
/// *whether* it applies, which is irrelevant to "does this sheet mention X at
/// all" — over-approximating there only costs the fast path, never correctness.
fn all_rules(sheet: &Stylesheet) -> impl Iterator<Item = &Rule> {
    sheet
        .rules
        .iter()
        .chain(sheet.media_rules.iter().flat_map(|m| m.rules.iter()))
        .chain(sheet.supports_rules.iter().flat_map(|s| s.rules.iter()))
        .chain(sheet.layers.iter().flat_map(|l| l.rules.iter()))
        .chain(sheet.scope_rules.iter().flat_map(|s| s.rules.iter()))
}

/// Whether `selector`'s subject compound carries a `::-webkit-scrollbar*`
/// pseudo-element (`::-webkit-scrollbar`, `-thumb`, `-track`), i.e. whether
/// `apply_webkit_scrollbar_pseudos` could ever find something to apply.
fn selector_targets_webkit_scrollbar(selector: &ComplexSelector) -> bool {
    let subject = selector.tail.last().map_or(&selector.head, |(_, c)| c);
    subject.parts.iter().any(|p| match p {
        SimpleSelector::PseudoElement(PseudoElementKind::Unknown(name)) => {
            name.to_ascii_lowercase().starts_with("-webkit-scrollbar")
        }
        _ => false,
    })
}

/// Every pseudo-element name carried by `selector`'s **subject** compound.
///
/// The subject is the only compound `matches_complex_for_pseudo` inspects, so
/// this is exactly the set of `pseudo` arguments for which that function can
/// return `Some` on this selector. See [`CascadeIndex::pseudo_subjects`].
fn selector_pseudo_subjects(selector: &ComplexSelector) -> impl Iterator<Item = &str> {
    let subject = selector.tail.last().map_or(&selector.head, |(_, c)| c);
    subject.parts.iter().filter_map(|p| match p {
        SimpleSelector::PseudoElement(kind) => Some(pseudo_element_name(kind)),
        _ => None,
    })
}

/// Case-insensitive `value.contains("quote")` without allocating — see
/// [`CascadeIndex::has_quote_content`].
fn value_mentions_quote(value: &str) -> bool {
    value
        .as_bytes()
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"quote"))
}

/// Perf cache key fields beyond (sheet pointer, rules count) that affect
/// which `@media` blocks are active — a viewport resize or dark-mode/
/// print/forced-colors toggle must invalidate `active_media` just like a
/// stylesheet swap does. `f32` compared via `to_bits()` (no NaN in practice
/// for a viewport size, and bit-equality avoids float-equality pitfalls).
#[derive(Clone, Copy, PartialEq)]
struct CascadeMediaKey {
    width_bits: u32,
    height_bits: u32,
    dark_mode: bool,
    print_active: bool,
    forced_colors: bool,
}

impl CascadeMediaKey {
    fn current(viewport: Size, dark_mode: bool) -> Self {
        Self {
            width_bits: viewport.width.to_bits(),
            height_bits: viewport.height.to_bits(),
            dark_mode,
            print_active: print_media_active(),
            forced_colors: forced_colors_active(),
        }
    }
}

/// How many (sheet, media key) pairs the per-thread index cache keeps.
///
/// Two, because a browser frame lays out two documents on one thread — its own
/// chrome and the page — and a single slot would make them evict each other
/// every frame, which is exactly the per-pass rebuild BUG-341 S21 removed. A
/// third document per thread per frame does not exist, and each slot retains an
/// index for the process's life, so the count is deliberately not larger.
const CASCADE_INDEX_SLOTS: usize = 2;

thread_local! {
    /// Per-thread rule-index cache, most recently used first.
    ///
    /// Keyed by ([`Stylesheet::revision`], media key): the revision changes
    /// whenever the sheet's rules do, and the media key covers a viewport
    /// resize or a dark-mode/print/forced-colors toggle, which decide which
    /// `@media` blocks are active.
    ///
    /// The key used to be the sheet's **address** plus its rule count, and an
    /// address is recycled the moment its sheet is freed — so the cache had to
    /// be invalidated at the top of every layout pass and on every rayon worker
    /// to avoid serving a freed sheet's index. That reduced it to a within-pass
    /// cache: a census (BUG-341 S21) measured one rebuild per incremental pass
    /// (0.14-0.22ms, 7-19% of the pass) and 33 per full pass across the worker
    /// pool. A revision is never recycled, so no invalidation is needed and the
    /// index now survives for as long as the sheet does.
    static RULE_IDX_CACHE: RefCell<Vec<(StylesheetRevision, CascadeMediaKey, CascadeIndex)>> =
        const { RefCell::new(Vec::new()) };
}

/// Makes the [`CascadeIndex`] for `sheet` under the current media conditions the
/// front slot of [`RULE_IDX_CACHE`], building it if this thread has not got it.
///
/// Every lookup site calls this first and then reads slot 0, so the borrow is
/// released between the two — a `candidates` call must not run while the cache
/// is mutably borrowed.
fn ensure_cascade_index(sheet: &Stylesheet, viewport: Size, dark_mode: bool) {
    let key = (sheet.revision(), CascadeMediaKey::current(viewport, dark_mode));
    let hit = RULE_IDX_CACHE.with(|cell| {
        let mut slots = cell.borrow_mut();
        match slots.iter().position(|s| (s.0, s.1) == key) {
            Some(0) => true,
            Some(i) => {
                slots.swap(0, i);
                true
            }
            None => false,
        }
    });
    if hit {
        return;
    }
    // Built outside the borrow: `CascadeIndex::build` is long, and holding a
    // `RefCell` across it would panic on any re-entrant cache read.
    let media_ctx = media_context_from_viewport(viewport, dark_mode);
    let idx = build_cascade_index_timed(sheet, &media_ctx);
    RULE_IDX_CACHE.with(|cell| {
        let mut slots = cell.borrow_mut();
        slots.truncate(CASCADE_INDEX_SLOTS - 1);
        slots.insert(0, (key.0, key.1, idx));
    });
}

/// Hands the front slot's index to `f`. Call [`ensure_cascade_index`] first —
/// with an empty cache this falls back to a scratch empty index, which matches
/// nothing rather than matching wrongly.
fn with_front_cascade_index<R>(f: impl FnOnce(&CascadeIndex) -> R) -> R {
    RULE_IDX_CACHE.with(|cell| match cell.borrow().first() {
        Some((_, _, idx)) => f(idx),
        None => f(&CascadeIndex::empty()),
    })
}

#[cfg(test)]
thread_local! {
    /// BUG-341 S10 test instrumentation — counts elements for which
    /// `apply_webkit_scrollbar_pseudos` actually ran its three pseudo-element
    /// cascades (i.e. took neither the "sheet has no such rule" nor the
    /// "same inheritance base as the parent" fast path).
    ///
    /// Gated by count rather than by output on purpose: the values these
    /// cascades produce are identical either way — that is the whole point of
    /// the fast path — so a regression that silently reinstates the per-element
    /// work is invisible to every differential test and shows up only as
    /// wall-clock, where machine noise hides it (BUG-341 "S8").
    static SCROLLBAR_PSEUDO_CASCADES: Cell<u32> = const { Cell::new(0) };

    /// BUG-341 S10 test instrumentation — counts [`pseudo_inherited_style`]
    /// calls, i.e. how often a pseudo-element's 302-field starting style was
    /// actually built. Same reasoning as `SCROLLBAR_PSEUDO_CASCADES`: building
    /// it and then discarding it produces exactly the same output.
    static PSEUDO_BASE_BUILDS: Cell<u32> = const { Cell::new(0) };
}

/// Resets the BUG-341 S10 pseudo-base counter for the current thread.
#[cfg(test)]
pub(crate) fn reset_pseudo_base_builds() {
    PSEUDO_BASE_BUILDS.with(|c| c.set(0));
}

/// Pseudo-element starting styles built since [`reset_pseudo_base_builds`].
#[cfg(test)]
pub(crate) fn pseudo_base_builds() -> u32 {
    PSEUDO_BASE_BUILDS.with(|c| c.get())
}

/// Resets the BUG-341 S10 scrollbar-cascade counter for the current thread.
#[cfg(test)]
fn reset_scrollbar_pseudo_cascades() {
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.set(0));
}

/// Elements that ran the full `::-webkit-scrollbar*` cascade since the last
/// [`reset_scrollbar_pseudo_cascades`].
#[cfg(test)]
fn scrollbar_pseudo_cascades() -> u32 {
    SCROLLBAR_PSEUDO_CASCADES.with(|c| c.get())
}

/// BUG-341 S20 — tally of [`CascadeIndex`] rebuilds.
///
/// Building the index walks every rule in the sheet four times over
/// ([`RuleIndex::build`], the `@layer`/`@media`/`@supports` blocks, the two
/// sheet-wide predicates). The counter exists because that rebuild is invisible
/// in output and lands *inside* `precompute_counters`' profile scope, where it
/// reads as cascade cost — it is what showed the index being rebuilt once per
/// pass and 33 times per full pass before S21 keyed the cache by revision.
/// It is also the gate: an index that is silently rebuilt every frame produces
/// exactly the same styles, just slower.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CascadeIndexStats {
    /// [`CascadeIndex::build`] calls since the last drain.
    pub builds: u32,
    /// Nanoseconds those calls took (the sum of the four fields below).
    pub build_ns: u64,
    /// Of `build_ns`: the top-level [`RuleIndex::build`].
    pub rules_ns: u64,
    /// Of `build_ns`: one [`RuleIndex`] per `@layer`/`@media`/`@supports` block.
    pub blocks_ns: u64,
    /// Of `build_ns`: evaluating which `@media`/`@supports` blocks are active.
    pub active_ns: u64,
    /// Of `build_ns`: the two sheet-wide predicate scans
    /// (`has_webkit_scrollbar_rules`, `has_quote_content`).
    pub predicates_ns: u64,
}

impl CascadeIndexStats {
    /// Folds `other` into `self` field by field.
    pub fn add(&mut self, other: CascadeIndexStats) {
        self.builds += other.builds;
        self.build_ns += other.build_ns;
        self.rules_ns += other.rules_ns;
        self.blocks_ns += other.blocks_ns;
        self.active_ns += other.active_ns;
        self.predicates_ns += other.predicates_ns;
    }
}

thread_local! {
    /// BUG-341 S20 instrumentation, see [`CascadeIndexStats`]. Always compiled:
    /// the timer runs once per rebuild, not per node, and a rebuild already
    /// costs orders of magnitude more than reading the clock.
    static CASCADE_INDEX_STATS: Cell<CascadeIndexStats> = const {
        Cell::new(CascadeIndexStats {
            builds: 0,
            build_ns: 0,
            rules_ns: 0,
            blocks_ns: 0,
            active_ns: 0,
            predicates_ns: 0,
        })
    };
}

/// Returns the accumulated [`CascadeIndexStats`] and resets the tally.
///
/// Thread-local, like [`crate::box_tree::take_box_build_stats`] and for the
/// same two reasons. It has to *see* the rayon workers, because the cache it
/// counts is per-thread and `build_box` fans flex/grid containers out (the S15
/// trap — a naively thread-local tally reported one rebuild per pass where a
/// full pass makes 33). And it must not see *other* tests, because a gate that
/// asserts "this pass rebuilt nothing" against a process-wide counter fails
/// whenever a concurrent test builds an index. Both hold because the fan-out
/// closure drains each worker's tally back into the parent thread through
/// [`add_cascade_index_stats`], exactly as the box-build tally does.
pub fn take_cascade_index_stats() -> CascadeIndexStats {
    CASCADE_INDEX_STATS.with(|s| s.replace(CascadeIndexStats::default()))
}

/// Folds a rayon worker's drained [`CascadeIndexStats`] into this thread's
/// tally — see [`take_cascade_index_stats`].
pub fn add_cascade_index_stats(other: CascadeIndexStats) {
    CASCADE_INDEX_STATS.with(|s| {
        let mut v = s.get();
        v.add(other);
        s.set(v);
    });
}

/// BUG-341 S20 — per-pass tally of [`compute_pseudo_element_style`] calls.
///
/// A pseudo-element cascade is a full candidate match plus, on a hit, a
/// 302-field starting style. S10 found three of them per element at the
/// *cascade* stage and removed them; the same shape survives at the *box-build*
/// stage, where every flex/grid container unconditionally asks for its own
/// `::before` and `::after`. Counted because the answer is almost always `None`
/// and therefore invisible in the produced tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PseudoCascadeStats {
    /// [`compute_pseudo_element_style`] calls that got as far as matching (i.e.
    /// the node was an element).
    pub calls: u32,
    /// Of those, the ones that produced a style rather than `None`.
    pub hits: u32,
    /// Nanoseconds those calls took.
    pub ns: u64,
}

impl PseudoCascadeStats {
    /// Folds another tally into this one.
    pub fn add(&mut self, other: PseudoCascadeStats) {
        self.calls += other.calls;
        self.hits += other.hits;
        self.ns += other.ns;
    }
}

thread_local! {
    /// BUG-341 S20 instrumentation, see [`PseudoCascadeStats`]. Always compiled:
    /// one clock read against a call that walks the sheet's candidate buckets.
    ///
    /// Thread-local, and `build_box` fans flex/grid containers out over rayon
    /// workers (the S15 trap). **S23**: the fan-out closure now drains each
    /// worker's tally back into the parent through [`add_pseudo_cascade_stats`],
    /// exactly as the box-build and cascade-index tallies do — before that the
    /// census saw only the containers built on the layout thread, which is what
    /// made S20's reading move from 139 to 160 for no visible reason.
    static PSEUDO_CASCADE_STATS: Cell<PseudoCascadeStats> =
        const { Cell::new(PseudoCascadeStats { calls: 0, hits: 0, ns: 0 }) };
}

/// Gate for [`PseudoCascadeStats`]. Off by default.
///
/// `compute_pseudo_element_style` runs twice per element on a full pass, so an
/// unconditional pair of clock reads here would be a per-element cost added to
/// the very path this track exists to make cheaper — a census must not be one
/// of the things it measures. Off, the hook costs one relaxed load.
static PSEUDO_STATS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enables/disables the BUG-341 S20 pseudo-cascade census — see
/// [`PseudoCascadeStats`]. Process-wide, like the box-build censuses.
pub fn set_pseudo_cascade_diagnostics(on: bool) {
    PSEUDO_STATS_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Returns the accumulated [`PseudoCascadeStats`] and resets the tally.
pub fn take_pseudo_cascade_stats() -> PseudoCascadeStats {
    PSEUDO_CASCADE_STATS.with(|s| s.replace(PseudoCascadeStats::default()))
}

/// Folds a rayon worker's drained [`PseudoCascadeStats`] into this thread's
/// tally — see [`take_pseudo_cascade_stats`].
pub fn add_pseudo_cascade_stats(other: PseudoCascadeStats) {
    PSEUDO_CASCADE_STATS.with(|s| {
        let mut v = s.get();
        v.add(other);
        s.set(v);
    });
}

thread_local! {
    /// BUG-341 S23 — the same tally, split by pseudo-element name.
    ///
    /// "160 calls" does not say which of the ~14 call sites made them, and the
    /// fix for each is different. Keyed by the `pseudo` argument, which is the
    /// one thing every call site already carries. Only filled while
    /// [`set_pseudo_cascade_diagnostics`] is on, so the `String` keys never cost
    /// a production pass anything.
    static PSEUDO_CASCADE_SITES: RefCell<HashMap<String, PseudoCascadeStats>> =
        RefCell::new(HashMap::new());
}

/// Returns the per-pseudo split of [`PseudoCascadeStats`] and resets it.
pub fn take_pseudo_cascade_sites() -> HashMap<String, PseudoCascadeStats> {
    PSEUDO_CASCADE_SITES.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// Folds a rayon worker's drained per-pseudo split into this thread's map.
pub fn add_pseudo_cascade_sites(other: HashMap<String, PseudoCascadeStats>) {
    PSEUDO_CASCADE_SITES.with(|m| {
        let mut m = m.borrow_mut();
        for (k, v) in other {
            m.entry(k).or_default().add(v);
        }
    });
}

/// Folds one [`compute_pseudo_element_style`] call into the tally.
fn note_pseudo_cascade(pseudo: &str, ns: u64, hit: bool) {
    PSEUDO_CASCADE_STATS.with(|s| {
        let mut v = s.get();
        v.calls += 1;
        v.hits += u32::from(hit);
        v.ns += ns;
        s.set(v);
    });
    PSEUDO_CASCADE_SITES.with(|m| {
        let mut m = m.borrow_mut();
        let e = m.entry(pseudo.to_string()).or_default();
        e.calls += 1;
        e.hits += u32::from(hit);
        e.ns += ns;
    });
}

/// [`CascadeIndex::build`], tallied into [`CascadeIndexStats`]. Every cache
/// refill goes through here, so the counter cannot drift from reality.
fn build_cascade_index_timed(sheet: &Stylesheet, media_ctx: &MediaContext) -> CascadeIndex {
    let (idx, stats) = CascadeIndex::build(sheet, media_ctx);
    add_cascade_index_stats(stats);
    idx
}

/// Drops every cached [`CascadeIndex`] on the current thread.
///
/// Not needed for correctness — the cache is keyed by
/// [`Stylesheet::revision`], which is never recycled — and deliberately not
/// called on the layout path. Tests use it to measure a cold build.
pub fn clear_rule_idx_cache() {
    RULE_IDX_CACHE.with(|cell| cell.borrow_mut().clear());
}

/// Refreshes the thread-local [`CascadeIndex`] for `sheet` if this thread has
/// not got it, then hands it to `f`.
///
/// The ensure-then-borrow dance is spelled out inline at every candidate lookup
/// in `compute_style`; new call sites should use this instead. Do not call
/// anything that touches `RULE_IDX_CACHE` from inside `f` — the borrow is live
/// for its duration.
fn with_cascade_index<R>(
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    f: impl FnOnce(&CascadeIndex) -> R,
) -> R {
    ensure_cascade_index(sheet, viewport, dark_mode);
    with_front_cascade_index(f)
}

/// CSS Generated Content L3 §3.2 — whether `sheet` can produce quote content
/// at all, i.e. whether the `::before`/`::after` probe in `counters::walk` can
/// yield a quote depth. See [`CascadeIndex::has_quote_content`].
pub fn sheet_has_quote_content(sheet: &Stylesheet, viewport: Size, dark_mode: bool) -> bool {
    with_cascade_index(sheet, viewport, dark_mode, |idx| idx.has_quote_content)
}

/// BUG-341 S23 — whether `sheet` uses `pseudo` (name without the `::`) as the
/// subject of any selector, i.e. whether
/// [`compute_pseudo_element_style`] for it can return anything but `None`.
///
/// Lets a caller skip a whole traversal instead of a single cascade: see
/// [`CascadeIndex::pseudo_subjects`]. **`::marker` is exempt** — CSS Lists L3
/// §2.1 synthesizes a marker style with no rule at all, so a `false` here says
/// nothing about it; callers must not gate `marker` on this predicate.
pub fn sheet_targets_pseudo(
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    pseudo: &str,
) -> bool {
    with_cascade_index(sheet, viewport, dark_mode, |idx| {
        idx.pseudo_subjects.iter().any(|s| s.eq_ignore_ascii_case(pseudo))
    })
}

thread_local! {
    /// CSS Scoping L1 — per-shadow-tree author stylesheets, keyed by shadow-host
    /// `NodeId`. Built once per layout pass from the `<style>` elements inside each
    /// shadow root (`set_shadow_sheets`). Their `:host`/`:host()`/`::slotted()`
    /// rules participate in the cascade only for the host (and its slotted light
    /// children) of the tree they belong to. Document-scope `:host`/`::slotted`
    /// rules are no-ops (CSS Scoping L1 §6.1-6.2).
    static SHADOW_SHEETS: RefCell<std::collections::HashMap<NodeId, Stylesheet>> =
        RefCell::new(std::collections::HashMap::new());

    /// Index of the shadow host whose own shadow-tree stylesheet is currently
    /// being matched, or `u32::MAX` when matching in document scope. The `:host`
    /// pseudo-class matches only when this equals the candidate node's index —
    /// this is what makes document-scope `:host` a no-op while shadow-scope
    /// `:host` matches its host.
    static SHADOW_HOST_SCOPE: Cell<u32> = const { Cell::new(u32::MAX) };
}

/// Install the per-shadow-host author stylesheets for the current layout pass.
///
/// Called once at the start of each layout entry point (after `build_flat_tree`),
/// replacing any sheets from the previous pass. Keyed by shadow-host `NodeId`.
pub fn set_shadow_sheets(map: std::collections::HashMap<NodeId, Stylesheet>) {
    SHADOW_SHEETS.with(|cell| *cell.borrow_mut() = map);
}

/// Drop all installed shadow-tree stylesheets (used by tests to avoid leaking
/// per-host sheets between cases that reuse `NodeId` indices).
pub fn clear_shadow_sheets() {
    SHADOW_SHEETS.with(|cell| cell.borrow_mut().clear());
}

/// Build a `ComputedStyle` from a flat list of declarations with neutral context.
///
/// Used by `@starting-style` cascade wiring: converts the declarations returned by
/// `resolve_starting_style()` into a `ComputedStyle` that serves as the
/// *before-change* style for CSS entry transitions (CSS Transitions L2 §3.4).
///
/// Context defaults: em_basis = 16 px, inherited = default, non-quirks mode.
/// Suitable for transition value extraction (`opacity`, `color`, `background-color`,
/// `transform`).
pub fn compute_style_from_declarations(decls: &[Declaration], viewport: Size) -> ComputedStyle {
    let inherited = ComputedStyle::root();
    let mut style = inherited.clone();
    for decl in decls {
        apply_declaration(&mut style, decl, 16.0, viewport, FontWeight::NORMAL, &inherited, &inherited, false, false);
    }
    style
}

/// Разбивает строку на куски по запятым, не пересекая `(...)` (для
/// shadow-list, где цвет может быть `rgba(0, 0, 0, 0.5)` с запятыми).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Парсит одну box-shadow спецификацию. Формат:
/// `[inset]? <length>{2,4} <color>?` — токены произвольно перемешаны.
fn parse_box_shadow_one(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<BoxShadow> {
    // Сложность: цветовые функции (`rgba(...)`) содержат пробелы — наивный
    // split_whitespace их разорвёт. Восстанавливаем токены, балансируя `()`.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => { depth += 1; buf.push(c); }
            ')' => { depth -= 1; buf.push(c); }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }

    let mut inset = false;
    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::new();

    for tok in tokens {
        if tok.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if let Some(c) = parse_color_legacy(&tok, is_quirks) {
            color = Some(c);
        } else if let Some(len) = parse_length(&tok)
            && let Some(px) = match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }
        {
            lengths.push(px);
        }
    }

    // Должно быть 2-4 длины (offset-x, offset-y, blur?, spread?).
    let (offset_x, offset_y, blur, spread) = match lengths.as_slice() {
        [x, y] => (*x, *y, 0.0, 0.0),
        [x, y, b] => (*x, *y, *b, 0.0),
        [x, y, b, sp] => (*x, *y, *b, *sp),
        _ => return None,
    };

    Some(BoxShadow { offset_x, offset_y, blur, spread, color, inset })
}

/// Парсит одну text-shadow спецификацию. Формат:
/// `<length>{2,3} <color>?` (без inset, без spread).
fn parse_text_shadow_one(s: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<TextShadow> {
    // Тот же tokenization-трюк, что у box-shadow — балансируем `()`,
    // чтобы цветовые функции не разрывались.
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => { depth += 1; buf.push(c); }
            ')' => { depth -= 1; buf.push(c); }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }

    let mut color: Option<Color> = None;
    let mut lengths: Vec<f32> = Vec::new();

    for tok in tokens {
        if let Some(c) = parse_color_legacy(&tok, is_quirks) {
            color = Some(c);
        } else if let Some(len) = parse_length(&tok)
            && let Some(px) = match len {
                Length::Percent(_) => None,
                other => other.resolve(em_basis, None, viewport),
            }
        {
            lengths.push(px);
        }
    }

    let (offset_x, offset_y, blur) = match lengths.as_slice() {
        [x, y] => (*x, *y, 0.0),
        [x, y, b] => (*x, *y, *b),
        _ => return None,
    };

    Some(TextShadow { offset_x, offset_y, blur, color })
}

/// CSS UI L4 §8.1: парсит keyword в `Cursor`. None = неизвестное.
fn parse_cursor_kw(s: &str) -> Option<Cursor> {
    Some(match s {
        "auto" => Cursor::Auto,
        "default" => Cursor::Default,
        "none" => Cursor::None,
        "context-menu" => Cursor::ContextMenu,
        "help" => Cursor::Help,
        "pointer" => Cursor::Pointer,
        "progress" => Cursor::Progress,
        "wait" => Cursor::Wait,
        "cell" => Cursor::Cell,
        "crosshair" => Cursor::Crosshair,
        "text" => Cursor::Text,
        "vertical-text" => Cursor::VerticalText,
        "alias" => Cursor::Alias,
        "copy" => Cursor::Copy,
        "move" => Cursor::Move,
        "no-drop" => Cursor::NoDrop,
        "not-allowed" => Cursor::NotAllowed,
        "grab" => Cursor::Grab,
        "grabbing" => Cursor::Grabbing,
        "all-scroll" => Cursor::AllScroll,
        "col-resize" => Cursor::ColResize,
        "row-resize" => Cursor::RowResize,
        "n-resize" => Cursor::NResize,
        "e-resize" => Cursor::EResize,
        "s-resize" => Cursor::SResize,
        "w-resize" => Cursor::WResize,
        "ne-resize" => Cursor::NeResize,
        "nw-resize" => Cursor::NwResize,
        "se-resize" => Cursor::SeResize,
        "sw-resize" => Cursor::SwResize,
        "ew-resize" => Cursor::EwResize,
        "ns-resize" => Cursor::NsResize,
        "nesw-resize" => Cursor::NeswResize,
        "nwse-resize" => Cursor::NwseResize,
        "zoom-in" => Cursor::ZoomIn,
        "zoom-out" => Cursor::ZoomOut,
        _ => return None,
    })
}

/// CSS Overflow L3: парсит keyword в `Overflow`. None = неизвестное.
fn parse_overflow_kw(s: &str) -> Option<Overflow> {
    match s {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "clip" => Some(Overflow::Clip),
        "scroll" => Some(Overflow::Scroll),
        "auto" => Some(Overflow::Auto),
        _ => None,
    }
}















































fn expand_grouped_transition_property(prop: &str) -> Vec<String> {
    let lower = prop.to_ascii_lowercase();
    match lower.as_str() {
        "margin" => vec!["margin-top", "margin-right", "margin-bottom", "margin-left"]
            .into_iter().map(|s| s.to_string()).collect(),
        "padding" => vec!["padding-top", "padding-right", "padding-bottom", "padding-left"]
            .into_iter().map(|s| s.to_string()).collect(),
        "border" => vec!["border-top", "border-right", "border-bottom", "border-left"]
            .into_iter().map(|s| s.to_string()).collect(),
        "border-radius" => vec!["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]
            .into_iter().map(|s| s.to_string()).collect(),
        _ => vec![prop.to_string()],
    }
}

#[allow(clippy::too_many_arguments)]
/// CSS Inline Layout L3 §5 — parse `initial-letter: normal | <number> <integer>?`.
///
/// Returns `(size, sink)`: `size` is the cap height in lines (≥ 1, may be
/// fractional) and `sink` is the number of in-flow lines the letter occupies
/// (`0` = `auto`, later resolved to `floor(size)`). `normal` → `(1.0, 0)`.
/// Returns `None` on malformed input, so the declaration is ignored and the
/// previous value is kept (CSS error recovery).
fn parse_initial_letter(val: &str) -> Option<(f32, u32)> {
    let v = val.trim();
    if v.eq_ignore_ascii_case("normal") {
        return Some((1.0, 0));
    }
    let mut it = v.split_whitespace();
    let size: f32 = it.next()?.parse().ok()?;
    if !size.is_finite() || size < 1.0 {
        return None;
    }
    let sink = match it.next() {
        Some(tok) => {
            let n: i64 = tok.parse().ok()?;
            if n < 1 {
                return None;
            }
            u32::try_from(n).ok()?
        }
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    Some((size, sink))
}


/// CSS Grid L1 §7.3 — parse `grid-template-areas` value.
///
/// Input: a CSS string value like `'"header header" "sidebar main"'`.
/// Each quoted string defines one row; tokens within the string are cell
/// names. `"."` (dot) is the null cell token (unnamed). Returns a 2D grid:
/// outer vec = rows top-to-bottom, inner vec = column names left-to-right.
///
/// Malformed rows (different column count) are silently dropped to keep the
/// grid rectangular.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn parse_grid_template_areas(val: &str) -> Vec<Vec<String>> {
    // Extract all quoted strings in order.
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut s = val.trim();
    while !s.is_empty() {
        s = s.trim_start();
        if s.starts_with('"') || s.starts_with('\'') {
            let quote = s.chars().next().unwrap();
            s = &s[1..];
            let end = s.find(quote).unwrap_or(s.len());
            let row_str = &s[..end];
            s = if end < s.len() { &s[end + 1..] } else { "" };
            let cells: Vec<String> = row_str
                .split_whitespace()
                .map(|t| t.to_string())
                .collect();
            if !cells.is_empty() {
                rows.push(cells);
            }
        } else {
            // Skip unexpected token.
            let next = s.find(|c: char| c.is_whitespace() || c == '"' || c == '\'').unwrap_or(s.len());
            s = &s[next..];
        }
    }
    // Ensure all rows have the same column count (take minimum; drop trailing).
    if rows.is_empty() {
        return rows;
    }
    let cols = rows.iter().map(Vec::len).min().unwrap_or(0);
    rows.retain(|r| r.len() == cols);
    rows
}

/// CSS Sizing L4 §6.1 — парсит `<ratio>`: либо одно положительное
/// число (трактуется как W:1), либо `W / H` пара. Phase 0 не
/// поддерживает `auto <ratio>` форму (она бы хранилась как fallback,
/// но требует расширения структуры).
fn parse_aspect_ratio_value(s: &str) -> Option<(f32, f32)> {
    let s = s.trim();
    if let Some((w_str, h_str)) = s.split_once('/') {
        let w = w_str.trim().parse::<f32>().ok()?;
        let h = h_str.trim().parse::<f32>().ok()?;
        if w > 0.0 && h > 0.0 {
            return Some((w, h));
        }
        return None;
    }
    // Single number — W:1.
    let v = s.parse::<f32>().ok()?;
    if v > 0.0 {
        Some((v, 1.0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
