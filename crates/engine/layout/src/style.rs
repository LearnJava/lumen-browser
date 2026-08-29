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
use lumen_core::ColorSpace;
use lumen_css_parser::{
    ComplexSelector,
    Declaration, MediaContext,
    PseudoElementKind, Rule, SimpleSelector, Specificity, Stylesheet, StylesheetRevision,
    SUPPORTED_PROPERTIES,
};
use lumen_dom::{Document, DocumentMode, NodeId};

// Разбор CSS-значений, применение значений и табличные справочники, вынесенные
// из этого файла батчами SPLIT-ST3…ST10 (docs/tasks/p1-monolith-split-queue.md §4).
// `matching` — матчинг селекторов (SPLIT-ST12): `matches_complex`/`matches_slotted_complex`
// вызываются отсюда, поэтому реэкспорт/импорт ниже, в блоке ST-12.
mod adjust;
mod apply;
mod calc;
mod cascade;
mod computed;
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
use parse::counters::is_css_ident;
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

/// CSS Content L3 — value свойства `content`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Content {
    /// `normal` (default) — поведение по умолчанию для каждого element.
    #[default]
    Normal,
    /// `none` — pseudo-element не генерируется.
    None,
    /// Список фрагментов: строки, counter()/counters(), attr(), url().
    /// Phase 0 хранит список typed-фрагментов; конкатенация для render —
    /// задача paint pipeline.
    Items(Vec<ContentItem>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentItem {
    /// Литеральная строка из CSS-string-literal (без кавычек).
    String(String),
    /// `attr(name)` — значение HTML-атрибута текущего element.
    Attr(String),
    /// `url("path")` — изображение / external resource.
    Url(String),
    /// `counter(name [, style])` — значение counter-а. `style` — пока
    /// сырая строка (Phase 0 разрешит только `decimal` etc.).
    Counter {
        name: String,
        style: Option<String>,
    },
    /// `counters(name, separator [, style])` — вложенные counters
    /// (`1.2.3` через `.`).
    Counters {
        name: String,
        separator: String,
        style: Option<String>,
    },
    /// `open-quote` / `close-quote` — quotation marks per `quotes` property.
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
}

/// CSS Generated Content L3 §3.2 — `quotes`. Inherited. Initial: `auto`.
///
/// Controls the quotation marks produced by `content: open-quote` /
/// `close-quote`. The nesting depth (which pair is used) is tracked in
/// document order by the counters pre-pass; this value only supplies the
/// glyph pairs to choose from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Quotes {
    /// `auto` — UA language-appropriate quotation marks. Lumen uses English
    /// curly quotes: primary “ ”, secondary ‘ ’.
    #[default]
    Auto,
    /// `none` — `open-quote` / `close-quote` produce no marks (depth still
    /// advances).
    None,
    /// Explicit `[<string> <string>]+` pairs — outermost (depth 0) first.
    /// Each tuple is `(open, close)`.
    Pairs(Vec<(String, String)>),
}

impl Quotes {
    /// Returns the `(open, close)` glyph strings for the given nesting `depth`.
    ///
    /// `Auto` uses the built-in English pairs; `Pairs` clamps `depth` to the
    /// last available pair (CSS Content L3 §3.2). Returns `None` for `quotes:
    /// none` or an empty explicit list — the caller emits nothing in that case.
    pub fn pair_for_depth(&self, depth: usize) -> Option<(&str, &str)> {
        const AUTO: &[(&str, &str)] = &[("\u{201C}", "\u{201D}"), ("\u{2018}", "\u{2019}")];
        match self {
            Quotes::None => None,
            Quotes::Auto => {
                let idx = depth.min(AUTO.len() - 1);
                Some(AUTO[idx])
            }
            Quotes::Pairs(pairs) => {
                if pairs.is_empty() {
                    return None;
                }
                let idx = depth.min(pairs.len() - 1);
                let (o, c) = &pairs[idx];
                Some((o.as_str(), c.as_str()))
            }
        }
    }
}

/// CSS Scrollbars 1 — `scrollbar-width`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    /// `thin` — тонкий scrollbar.
    Thin,
    /// `none` — без visible scrollbar (контент всё ещё скроллится через
    /// keyboard / touch / programmatic).
    None,
}

impl ScrollbarWidth {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "thin" => Some(Self::Thin),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scrollbar-gutter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarGutter {
    /// `auto` (default) — gutter появляется когда overflow:scroll.
    #[default]
    Auto,
    /// `stable` — gutter всегда зарезервирован (не двигает контент при scroll).
    Stable,
    /// `stable both-edges` — gutter на обоих краях для симметрии.
    StableBothEdges,
}

impl ScrollbarGutter {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        if lc == "auto" {
            return Some(Self::Auto);
        }
        if lc == "stable" {
            return Some(Self::Stable);
        }
        // `stable both-edges` — двухтокеновая форма.
        let tokens: Vec<&str> = lc.split_whitespace().collect();
        if tokens == ["stable", "both-edges"] {
            return Some(Self::StableBothEdges);
        }
        None
    }
}

/// CSS Lists L3 §2.1 — markers для list items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ListStyleType {
    /// `none` — без marker.
    None,
    /// `disc` — закрашенный кружок (default для ul).
    #[default]
    Disc,
    /// `circle` — пустой кружок.
    Circle,
    /// `square` — квадратик.
    Square,
    /// `decimal` — 1, 2, 3, ... (default для ol).
    Decimal,
    /// `decimal-leading-zero` — 01, 02, ..., 09, 10, ...
    DecimalLeadingZero,
    /// `lower-roman` — i, ii, iii, ...
    LowerRoman,
    /// `upper-roman` — I, II, III, ...
    UpperRoman,
    /// `lower-alpha` / `lower-latin` — a, b, c, ...
    LowerAlpha,
    /// `upper-alpha` / `upper-latin` — A, B, C, ...
    UpperAlpha,
    /// `lower-greek` — α, β, γ, ...
    LowerGreek,
    /// `<custom-ident>` — ссылка на именованный `@counter-style`.
    Custom(Box<str>),
}

impl ListStyleType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "disc" => Some(Self::Disc),
            "circle" => Some(Self::Circle),
            "square" => Some(Self::Square),
            "decimal" => Some(Self::Decimal),
            "decimal-leading-zero" => Some(Self::DecimalLeadingZero),
            "lower-roman" => Some(Self::LowerRoman),
            "upper-roman" => Some(Self::UpperRoman),
            "lower-alpha" | "lower-latin" => Some(Self::LowerAlpha),
            "upper-alpha" | "upper-latin" => Some(Self::UpperAlpha),
            "lower-greek" => Some(Self::LowerGreek),
            // Any unrecognised ident is a reference to a named @counter-style.
            s if !s.is_empty() => Some(Self::Custom(s.into())),
            _ => None,
        }
    }
}

/// CSS Lists L3 §2.3 — `list-style-position`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListStylePosition {
    /// `outside` (default) — marker вне content-area.
    #[default]
    Outside,
    /// `inside` — marker внутри content-area.
    Inside,
}

impl ListStylePosition {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "outside" => Some(Self::Outside),
            "inside" => Some(Self::Inside),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `overflow-wrap`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverflowWrap {
    #[default]
    Normal,
    /// `break-word` — разрешает перенос любого слова, чтобы не было overflow.
    BreakWord,
    /// `anywhere` — как `break-word`, но также влияет на intrinsic-width
    /// computation (CSS Text L3).
    Anywhere,
}

impl OverflowWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "break-word" => Some(Self::BreakWord),
            "anywhere" => Some(Self::Anywhere),
            _ => None,
        }
    }
}

/// CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`.
/// Управляет строгостью правил переноса CJK-текста по пробелам.
/// Phase 0: parse + store; реальный CJK-wrap — отдельная задача.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineBreak {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

/// CSS Text L3 §5.1 — `word-break`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WordBreak {
    #[default]
    Normal,
    /// `keep-all` — CJK не разбивается.
    KeepAll,
    /// `break-all` — разрыв в любом месте, кроме whitespace.
    BreakAll,
    /// `break-word` — legacy для `overflow-wrap: break-word`.
    BreakWord,
}

impl WordBreak {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "keep-all" => Some(Self::KeepAll),
            "break-all" => Some(Self::BreakAll),
            "break-word" => Some(Self::BreakWord),
            _ => None,
        }
    }
}

/// CSS Text L3 §6 — `hyphens`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hyphens {
    /// `none` — переносы запрещены.
    None,
    /// `manual` (default) — переносы только при явных hyphenation-точках
    /// (`&shy;` / U+00AD).
    #[default]
    Manual,
    /// `auto` — UA расставляет переносы по алгоритму (требует hyphenation
    /// dictionary).
    Auto,
}

impl Hyphens {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`.
/// Указывает, какими жестами UA управляет самостоятельно (pan/zoom).
/// Phase 0: parse + store; реальная обработка touch-жестов — P3 task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TouchAction {
    #[default]
    Auto,
    None,
    PanX,
    PanLeft,
    PanRight,
    PanY,
    PanUp,
    PanDown,
    PinchZoom,
    Manipulation,
}

/// CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`.
/// Контролирует отображение элемента согласно UA-теме (форм-виджеты).
/// Phase 0: parse + store; реальная стилизация форм-виджетов — P2/P3 task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Auto,
    None,
    /// `menulist-button` / `searchfield` / `textfield` / `button` и прочие
    /// platform-специфичные значения — хранятся как Compat.
    Compat,
    /// `base-select` (HTML/CSS «Customizable Select») — `<select>` рендерится
    /// как author-стилизуемое дерево (кнопка-триггер + `<selectedcontent>` +
    /// `::picker(select)` со списком опций) вместо непрозрачного нативного
    /// контрола. См. `box_tree.rs` (построение дерева) и `forms.rs` (поповер).
    BaseSelect,
}

/// CSS Basic UI L4 §4.4 — `field-sizing`. NOT inherited. Initial: `Fixed`.
/// `Fixed` — UA-specified dimensions apply (default browser behaviour).
/// `Content` — intrinsic size comes from the control's text content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FieldSizing {
    /// UA default dimensions (e.g. `<input>` is 174×21 px).
    #[default]
    Fixed,
    /// Size the control to fit its text content (CSS Basic UI L4 §4.4).
    Content,
}

/// CSS Pointer Events L1. Default `auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
    Visible,
    /// `painted` / `fill` / `stroke` / `all` — для SVG. В non-SVG
    /// контексте трактуются как `auto`.
    Painted,
    Fill,
    Stroke,
    All,
}

impl PointerEvents {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "none" => Some(Self::None),
            "visible" | "visiblepainted" | "visiblefill" | "visiblestroke" => {
                Some(Self::Visible)
            }
            "painted" => Some(Self::Painted),
            "fill" => Some(Self::Fill),
            "stroke" => Some(Self::Stroke),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`.
/// Позволяет пользователю изменять размер элемента мышью.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Resize {
    /// `none` — resize запрещён.
    #[default]
    None,
    /// `both` — resize по обеим физическим осям.
    Both,
    /// `horizontal` — resize только по физической ширине.
    Horizontal,
    /// `vertical` — resize только по физической высоте.
    Vertical,
    /// `block` — resize вдоль block-оси (логическая, зависит от `writing-mode`).
    Block,
    /// `inline` — resize вдоль inline-оси (логическая, зависит от `writing-mode`).
    Inline,
}

impl Resize {
    /// Разрешает логическую ось `resize` (`Block`/`Inline`) в физическую пару
    /// `(разрешена ширина, разрешена высота)` с учётом `writing-mode`.
    ///
    /// В `horizontal-tb` block-ось — вертикальная, inline-ось — горизонтальная;
    /// в вертикальных режимах (`vertical-rl`/`vertical-lr`/`sideways-rl`) — наоборот.
    /// Используется драг-хендлером grip-а (`crates/shell/src/main.rs`), чтобы
    /// вложенный корректно гейтить, какую из осей (`width`/`height`) двигать.
    pub fn allowed_axes(self, writing_mode: WritingMode) -> (bool, bool) {
        let vertical_wm = matches!(
            writing_mode,
            WritingMode::VerticalRl
                | WritingMode::VerticalLr
                | WritingMode::SidewaysRl
                | WritingMode::SidewaysLr
        );
        match self {
            Resize::None => (false, false),
            Resize::Both => (true, true),
            Resize::Horizontal => (true, false),
            Resize::Vertical => (false, true),
            Resize::Block => (vertical_wm, !vertical_wm),
            Resize::Inline => (!vertical_wm, vertical_wm),
        }
    }
}

/// CSS Containment L3 §3 — `contain` property.
/// Bitflags: bit0=size, bit1=inline-size, bit2=layout, bit3=style, bit4=paint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContainFlags(pub u8);

impl ContainFlags {
    pub const NONE: Self = Self(0);
    pub const SIZE: Self = Self(1 << 0);
    pub const INLINE_SIZE: Self = Self(1 << 1);
    pub const LAYOUT: Self = Self(1 << 2);
    pub const STYLE: Self = Self(1 << 3);
    pub const PAINT: Self = Self(1 << 4);
    /// `strict` = size + layout + style + paint
    pub const STRICT: Self = Self(1 | (1 << 2) | (1 << 3) | (1 << 4));
    /// `content` = layout + style + paint
    pub const CONTENT: Self = Self((1 << 2) | (1 << 3) | (1 << 4));
}

/// CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentVisibility {
    #[default]
    Visible,
    Auto,
    Hidden,
}

/// CSS Sizing L4 §4.5 — `interpolate-size` property value.
///
/// Controls whether keyword sizes (`auto`, `min-content`, `max-content`,
/// `fit-content`) can participate in CSS transitions and animations.
/// When `AllowKeywords` is active, the layout engine resolves keyword sizes
/// to their px equivalent at transition start, enabling smooth
/// `height: 0 → height: auto` transitions.
///
/// # CSS: interpolate-size
/// P4 wires this enum via `apply_declaration("interpolate-size", ...)` and
/// stores the result in `ComputedStyle::interpolate_size`. The engine reads
/// it in `TransitionScheduler::sync()` to decide whether to allow keyword
/// size interpolation for a given element.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InterpolateSizeMode {
    /// CSS Sizing L4 §4.5.1 initial value — keyword sizes are discrete.
    /// Transitions that start or end at a keyword size snap at `t = 0.5`.
    #[default]
    NumericOnly,
    /// CSS Sizing L4 §4.5 `allow-keywords` value — keyword sizes resolve
    /// to their px value at transition start, enabling smooth animations.
    AllowKeywords,
}

/// CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContainerType {
    #[default]
    Normal,
    Size,
    InlineSize,
}

/// Resolved container dimensions, passed during style re-computation for container queries.
/// CSS Container Queries L1 §3: size features (width, height) evaluated against this context.
#[derive(Debug, Clone)]
pub struct ContainerContext {
    /// Content width of the container element in pixels.
    pub width: f32,
    /// Content height if definite (None when auto/unknown).
    pub height: Option<f32>,
    /// The container's `container-name` values (for named queries).
    pub names: Vec<String>,
    /// Custom properties (`--*`) контейнера — для style() queries (CSS Containment L3 §4).
    /// Shares the container's own [`CustomProps`] allocation, so building a
    /// `ContainerContext` costs no map copy.
    pub custom_props: CustomProps,
    /// Container's own computed style, serialized the same way as
    /// `window.getComputedStyle()` (`selector_query::computed_style_to_map`) —
    /// used to resolve `style()` queries against standard (non-custom) properties.
    pub style_props: HashMap<String, String>,
    /// Container's own font-size — the `em` basis when resolving relative
    /// units in a `style()` query's declared value.
    pub font_size: f32,
    /// Viewport size — the `vw`/`vh`/`vmin`/`vmax` basis when resolving
    /// relative units in a `style()` query's declared value.
    pub viewport: Size,
    /// Height of the *container's own* containing block (its immediate
    /// parent's content box) — the CSS2.1 §10.5 basis for resolving `%` in
    /// the container's own `height`/`top`/`bottom`/`min-height`/
    /// `max-height` during a `style()` query. Distinct from `height` above,
    /// which is the container's own (already-resolved) content height
    /// exposed to descendants for `(min-height: …)`-style size queries.
    /// Always a concrete pixel value: by the time `ContainerContext` is
    /// built, the whole tree has already been laid out to definite rects,
    /// so this doesn't distinguish an explicitly-sized parent from one whose
    /// own height was itself content-derived (CSS2.1 §10.5's "if the height
    /// of the containing block is not specified explicitly… the percentage
    /// value computes to auto" is not modeled here).
    pub own_containing_block_height: f32,
}

/// Evaluates a raw @container condition string against a `ContainerContext`.
///
/// Phase 0: handles `(min-width: Npx)`, `(max-width: Npx)`, `(min-height: Npx)`,
/// `(max-height: Npx)`, `(width: Npx)`, `(height: Npx)`, and `and`/`or`/`not` operators.
/// Also supports `style(--prop: value)` and boolean `style(--prop)` forms
/// (CSS Containment L3 §4). Custom-property style queries compare the container's
/// value against the query value as *normalized* token streams — internal runs of
/// whitespace collapse to a single space and whitespace around commas is removed,
/// so `style(--gap: 1px 2px)` matches a container declaring `--gap: 1px  2px` or
/// `--gap:1px 2px` (CSS Custom Properties L1 §2 «computed value is the specified
/// value with whitespace trimmed»). The container's declared value is `var()`-expanded
/// against its own `custom_props` map before comparison — e.g. a container with
/// `--base: 8px; --gap: var(--base);` matches `style(--gap: 8px)` — mirroring how
/// `var()` is substituted when a custom property is consumed elsewhere in the cascade.
/// Standard (non-custom) properties are compared against the container's own
/// computed style (`ctx.style_props`, same serialization as `getComputedStyle()`):
/// `style(display: flex)` matches a container computed to `display: flex`. The
/// comparison is case-insensitive after the same whitespace/comma normalization
/// used for custom properties, so it works for keyword and length values whose
/// author-written form matches the serialized form (`style(width: 100px)` against
/// a computed `100px`); if that normalized comparison fails, both sides are also
/// tried as CSS colors and as lengths (`style_query_value_matches`), so
/// `style(color: red)` matches a computed `rgb(255, 0, 0)`, `style(border-width:
/// 2pt)` matches a computed `2.6667px`, and relative lengths (`em`, `%`,
/// viewport units) resolve against the container's own `font_size`/`viewport`
/// (`style(width: 1em)` matches a computed `16px` on a container whose
/// font-size is `16px`) — the same `em`/viewport basis `cq*` units use,
/// since a `style()` query's declared value is evaluated as if specified on
/// the container element itself (CSS Containment L3 §4). The `%` basis is
/// picked per queried property by `style_query_percent_basis` — the
/// container's width by default, but its own font-size for `line-height` and
/// its own containing block's height for `height`/`top`/`bottom`/
/// `min-height`/`max-height`.
/// Boolean form (`style(--prop)` / `style(prop)` without a value) is true when the
/// container has any value for that property — for custom properties this checks
/// `custom_props`, for standard properties `style_props` (a standard property never
/// computes to the custom-property-only guaranteed-invalid value, so in practice
/// this is true whenever the container's computed style was resolved for it).
/// A single `style()` call may itself combine multiple property queries with
/// `and`/`or`/`not`, each wrapped in its own parentheses — e.g.
/// `style((--a: 1) and (--b: 2))` or `style(not (display: none))` — per the
/// formal grammar (`<style-query> = <style-condition> | <style-feature>`,
/// CSS Containment L3 §5.2); see `evaluate_style_query`.
/// Phase 0 limitations:
/// - `state()` container queries: not a Lumen gap — the CSS Containment L3
///   spec itself removed/deferred state query features, so there is nothing
///   to implement against.
/// - Vertical box-model properties (`margin-top`/`margin-bottom`/
///   `padding-top`/`padding-bottom`) resolve `%` against the container's
///   width per CSS2.1 §8.3/§10.3 (correct — the containing block width is
///   the basis for *all four* margin/padding sides).
/// - `height`/`top`/`bottom`/`min-height`/`max-height` resolve `%` against
///   `ContainerContext::own_containing_block_height` — the container's own
///   immediate parent's content height, correctly distinct from the
///   container's own size or width (see that field's doc). The one
///   remaining approximation: this value is always treated as definite,
///   since Lumen's post-layout box tree no longer distinguishes a parent
///   whose height was explicitly specified from one whose height was itself
///   content-derived (CSS2.1 §10.5 would compute the `%` as `auto` in the
///   latter case).
///
/// Unknown features → false (safe fallback).
pub fn evaluate_container_condition(condition: &str, ctx: &ContainerContext) -> bool {
    let s = condition.trim();
    // Handle `not (...)` and `not style(...)`.
    if let Some(rest) = s.strip_prefix("not") {
        let rest = rest.trim();
        if rest.starts_with('(') || rest.to_ascii_lowercase().starts_with("style(") {
            return !evaluate_container_condition(rest, ctx);
        }
    }
    // Split on top-level `and` / `or`.
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return evaluate_container_condition(lhs, ctx) && evaluate_container_condition(rhs, ctx);
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return evaluate_container_condition(lhs, ctx) || evaluate_container_condition(rhs, ctx);
    }
    // Handle `style(...)` queries.
    let s_lower = s.to_ascii_lowercase();
    if s_lower.starts_with("style(") && s.ends_with(')') {
        // Extract content between `style(` and the final `)`.
        let inner = s[6..s.len() - 1].trim();
        return evaluate_style_query(inner, ctx);
    }
    // Feature: `(feature: value)`.
    let inner = s.strip_prefix('(').and_then(|x| x.strip_suffix(')'));
    let inner = match inner {
        Some(i) => i.trim(),
        None => return false,
    };
    // Parse `feature: value`.
    let colon = inner.find(':');
    let (feature, value) = if let Some(pos) = colon {
        (inner[..pos].trim(), inner[pos + 1..].trim())
    } else {
        // Boolean feature (e.g. `(color)`) — unsupported in Phase 0.
        return false;
    };
    let px = parse_css_length_to_px(value);
    match (feature, px) {
        ("min-width", Some(v))  => ctx.width >= v,
        ("max-width", Some(v))  => ctx.width <= v,
        ("width", Some(v))      => (ctx.width - v).abs() < 0.5,
        ("min-height", Some(v)) => ctx.height.is_some_and(|h| h >= v),
        ("max-height", Some(v)) => ctx.height.is_none_or(|h| h <= v),
        ("height", Some(v))     => ctx.height.is_some_and(|h| (h - v).abs() < 0.5),
        _ => false,
    }
}

/// Evaluates the content of a `style()` container query — CSS Containment L3
/// §5.2. Per the formal grammar (`<style-query> = <style-condition> |
/// <style-feature>`, `<style-condition> = not <style-in-parens> |
/// <style-in-parens> [and <style-in-parens>]* | [or <style-in-parens>]*`,
/// `<style-in-parens> = (<style-condition>) | (<style-feature>)`), a single
/// `style()` call may combine multiple property queries with `and`/`or`/`not`,
/// each wrapped in its own parentheses (e.g. `style((--a: 1) and (--b: 2))`,
/// `style(not (display: none))`). `<style-feature>` itself always queries
/// exactly one property — the grammar has no comma-separated multi-declaration
/// form — handled by `evaluate_style_feature`.
fn evaluate_style_query(s: &str, ctx: &ContainerContext) -> bool {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("not") {
        let rest = rest.trim();
        if rest.starts_with('(') {
            return !evaluate_style_query(rest, ctx);
        }
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " and ") {
        return evaluate_style_query(lhs, ctx) && evaluate_style_query(rhs, ctx);
    }
    if let Some((lhs, rhs)) = split_top_level_logical(s, " or ") {
        return evaluate_style_query(lhs, ctx) || evaluate_style_query(rhs, ctx);
    }
    // `<style-in-parens>` grouping: strip one layer and recurse.
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return evaluate_style_query(inner, ctx);
    }
    // Leaf: a bare `<style-feature>` (boolean `prop` or declaration `prop: value`).
    evaluate_style_feature(s, ctx)
}

/// Evaluates a single `<style-feature>` (CSS Containment L3 §5.2): the boolean
/// form `prop` (true iff the container has any value for it) or the
/// declaration form `prop: value` (true iff the container's own value matches,
/// with the same custom-property/var()-expansion and standard-property
/// canonicalization as `evaluate_container_condition`'s `style()` handling).
fn evaluate_style_feature(feature: &str, ctx: &ContainerContext) -> bool {
    let inner = feature.trim();
    // Boolean form: `--prop` or `prop`.
    if !inner.contains(':') {
        let name = inner.trim();
        if name.starts_with("--") {
            return resolve_container_custom_prop(ctx, name).is_some_and(|v| !v.trim().is_empty());
        }
        // Standard property: true if the container's computed style has
        // any value for it (a standard property never computes to the
        // custom-property-only guaranteed-invalid value).
        return ctx
            .style_props
            .get(&name.to_ascii_lowercase())
            .is_some_and(|v| !v.trim().is_empty());
    }
    // Declaration form: `--prop: value` or `prop: value`.
    if let Some((name, value)) = inner.split_once(':') {
        let name = name.trim();
        let want = normalize_style_value(value);
        if name.starts_with("--") {
            return resolve_container_custom_prop(ctx, name).map(|v| normalize_style_value(&v))
                == Some(want);
        }
        // Standard property: compare against the container's own computed
        // style (case-insensitive — CSS keywords are ASCII case-insensitive).
        let name_lower = name.to_ascii_lowercase();
        return ctx
            .style_props
            .get(&name_lower)
            .is_some_and(|v| style_query_value_matches(v, &want, &name_lower, ctx));
    }
    false
}

/// Resolves a container's custom property for a `style()` query: looks up `name`
/// in `ctx.custom_props` and expands any `var()` references against that same map
/// (CSS Variables L1 §3), so a chain like `--base: 8px; --gap: var(--base);`
/// resolves `--gap` to `8px` before comparison. Returns `None` if the property is
/// absent or its `var()` chain fails to resolve (unknown reference, no fallback,
/// or recursion past `VAR_EXPAND_MAX_DEPTH`).
fn resolve_container_custom_prop(ctx: &ContainerContext, name: &str) -> Option<String> {
    let raw = ctx.custom_props.get(name)?;
    expand_vars(raw, &ctx.custom_props, 0)
}

/// Normalizes a custom-property value for `style()` query comparison.
///
/// Collapses each run of ASCII whitespace to a single space, trims the ends, and
/// removes whitespace immediately around commas. This mirrors how a custom
/// property's computed value drops insignificant whitespace between tokens
/// (CSS Custom Properties L1 §2), so equivalent declarations compare equal
/// regardless of the author's spacing (`1px 2px` == `1px  2px`, `a,b` == `a, b`).
fn normalize_style_value(s: &str) -> String {
    // First collapse internal whitespace runs to single spaces.
    let collapsed: String = {
        let mut out = String::with_capacity(s.len());
        let mut prev_ws = false;
        for ch in s.trim().chars() {
            if ch.is_ascii_whitespace() {
                if !prev_ws {
                    out.push(' ');
                }
                prev_ws = true;
            } else {
                out.push(ch);
                prev_ws = false;
            }
        }
        out
    };
    // Then strip the spaces that sit directly around commas.
    let mut out = String::with_capacity(collapsed.len());
    let bytes = collapsed.as_bytes();
    for (i, ch) in collapsed.char_indices() {
        if ch == ' ' {
            let next_is_comma = bytes.get(i + 1) == Some(&b',');
            let prev_is_comma = i > 0 && bytes[i - 1] == b',';
            if next_is_comma || prev_is_comma {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Compares a container's serialized computed-style value against a `style()`
/// query's declared value for a standard (non-custom) property.
///
/// First tries the normalized token comparison (`normalize_style_value`,
/// case-insensitive). If that fails, falls back to two context-free
/// canonicalizations, tried in order:
/// 1. CSS colors: both sides parsed and compared by resolved RGBA channels —
///    so `style(color: red)` matches a container computed to
///    `color: rgb(255, 0, 0)` (CSS Color L4 §4, equivalent notations denote
///    the same color).
/// 2. CSS lengths: both sides parsed via `parse_length`, then resolved to px
///    using `ctx` as the basis — `ctx.font_size` for `em`, `ctx.viewport` for
///    `vw`/`vh`/`vmin`/`vmax` (CSS Values L3 §5.2/§6.1; absolute units like
///    `pt` resolve independent of any basis) — so `style(border-width: 2pt)`
///    matches a computed `2.6667px`, and `style(width: 1em)` matches a
///    computed `16px` on a container whose font-size is `16px`. The `%`
///    basis is picked per `prop_name` by `style_query_percent_basis` — e.g.
///    `line-height`'s is the container's own font-size, not its width.
///    Values that need layout context beyond `ctx` (`min-content`, unresolved
///    `cq*` outside a re-layout pass) don't resolve and fall through to the
///    textual comparison's `false`.
///
/// `want` must already be normalized by the caller. `prop_name` must already
/// be lowercased by the caller.
fn style_query_value_matches(computed: &str, want: &str, prop_name: &str, ctx: &ContainerContext) -> bool {
    if normalize_style_value(computed).eq_ignore_ascii_case(want) {
        return true;
    }
    if let (Some(a), Some(b)) = (parse_color(computed), parse_color(want)) {
        return a == b;
    }
    if let (Some(a), Some(b)) = (parse_length(computed), parse_length(want)) {
        let basis = Some(style_query_percent_basis(prop_name, ctx));
        if let (Some(pa), Some(pb)) = (
            a.resolve(ctx.font_size, basis, ctx.viewport),
            b.resolve(ctx.font_size, basis, ctx.viewport),
        ) {
            return (pa - pb).abs() < 0.01;
        }
    }
    false
}

/// Picks the `%` reference basis (in px) for a `style()` query's declared
/// value, based on which standard property is being queried — CSS Values L3
/// §5.2's «the percentage is calculated with respect to X» is per-property,
/// not a single container size. Mirrors the handful of properties whose
/// basis differs from the common "containing block width" default:
/// - `line-height`: the element's own font-size (CSS Inline L3 §4.6.2),
///   which for a `style()` query is the container's own `font_size`.
/// - Vertical box-model properties (`height`, `top`/`bottom`, vertical
///   `min-`/`max-height`): the *container's own* containing block's height
///   (CSS2.1 §10.5) — `ctx.own_containing_block_height`, i.e. the height of
///   the container's parent content box, not the container's own height
///   (`ctx.height` is a different quantity: the container's own resolved
///   size, exposed to descendants for `(min-height: …)`-style size queries).
///
/// Every other property (including `margin-top`/`margin-bottom`/
/// `padding-top`/`padding-bottom`, which CSS2.1 §8.3/§10.3 defines against
/// the containing block *width* despite being vertical) falls back to the
/// container's width, unchanged from before this function existed.
fn style_query_percent_basis(prop_name: &str, ctx: &ContainerContext) -> f32 {
    match prop_name {
        "line-height" => ctx.font_size,
        "height" | "min-height" | "max-height" | "top" | "bottom" => {
            ctx.own_containing_block_height
        }
        _ => ctx.width,
    }
}

/// Parses a CSS length value to pixels (px / em not supported — just px for Phase 0).
fn parse_css_length_to_px(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("px") {
        n.trim().parse::<f32>().ok()
    } else if let Some(n) = s.strip_suffix("em") {
        // Phase 0: treat em as px (approximate).
        n.trim().parse::<f32>().ok()
    } else {
        s.parse::<f32>().ok()
    }
}

/// Splits `s` on the first occurrence of `sep` that is not inside parentheses.
fn split_top_level_logical<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let sep_bytes = sep.as_bytes();
    let s_bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0;
    while i + sep.len() <= s.len() {
        match s_bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && s_bytes[i..].starts_with(sep_bytes) {
            return Some((&s[..i], &s[i + sep.len()..]));
        }
        i += 1;
    }
    None
}

/// Applies matching `@container` rules from `sheet` to `style`.
/// Called during the second layout pass for descendants of container elements.
/// `ctx` — resolved size of the nearest container ancestor.
pub fn apply_container_rules(
    style: &mut ComputedStyle,
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
    dark_mode: bool,
) {
    let is_quirks = doc.mode() == DocumentMode::Quirks;
    for container_rule in &sheet.container_rules {
        // Name filter: if the rule has a name, the context must include that name.
        if container_rule.name.as_ref().is_some_and(|rule_name| {
            !ctx.names.iter().any(|n| n == rule_name)
        }) {
            continue;
        }
        if !evaluate_container_condition(&container_rule.condition, ctx) {
            continue;
        }
        // Apply declarations from matching rules.
        for rule in &container_rule.rules {
            let mut best: Option<Specificity> = None;
            for complex in &rule.selectors {
                if matches_complex(complex, doc, node) {
                    let spec = complex.specificity();
                    best = Some(match best {
                        Some(prev) if prev >= spec => prev,
                        _ => spec,
                    });
                }
            }
            if best.is_some() {
                let em = style.font_size;
                let pw = style.font_weight;
                let inherited = style.clone();
                for decl in &rule.declarations {
                    let attr_buf;
                    let effective_decl: &Declaration = if decl.value.contains("attr(") {
                        let Some(v) = expand_attr_val(&decl.value, doc, node) else { continue };
                        attr_buf = Declaration { property: decl.property.clone(), value: v, important: decl.important };
                        &attr_buf
                    } else {
                        decl
                    };
                    apply_declaration(style, effective_decl, em, viewport, pw, &inherited, &inherited, is_quirks, dark_mode);
                }
            }
        }
    }
}

/// CSS Shapes L1 §3 — `shape-outside` value. NOT inherited. Initial: `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ShapeOutside {
    #[default]
    None,
    /// `<basic-shape>` or `<url>` or `<box-value>` — stored as raw string for Phase 0.
    Value(String),
}

/// CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum OffsetRotate {
    #[default]
    Auto,
    /// `auto <angle>` — auto direction plus a fixed rotation offset.
    AutoAngle(f32),
    Reverse,
    Angle(f32),
}

/// CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrintColorAdjust {
    #[default]
    Economy,
    Exact,
}

/// CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum FontSizeAdjust {
    #[default]
    None,
    Auto,
    Value(f32),
}

/// CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WritingMode {
    /// `horizontal-tb` — left-to-right horizontal, top-to-bottom block.
    #[default]
    HorizontalTb,
    /// `vertical-rl` — top-to-bottom vertical, right-to-left block.
    VerticalRl,
    /// `vertical-lr` — top-to-bottom vertical, left-to-right block.
    VerticalLr,
    /// `sideways-rl` — same as vertical-rl but glyphs rotated 90° CW.
    SidewaysRl,
    /// `sideways-lr` — same as vertical-lr but glyphs rotated 90° CCW.
    SidewaysLr,
}

/// CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`.
/// Only meaningful in vertical writing modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextOrientation {
    /// `mixed` — rotate CJK upright, rotate others 90° CW.
    #[default]
    Mixed,
    /// `upright` — all glyphs upright; implies `direction: ltr`.
    Upright,
    /// `sideways` — all glyphs rotated 90° CW (like vertical-rl inline).
    Sideways,
}

/// CSS UI L4 §6.2 — `user-select`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserSelect {
    #[default]
    Auto,
    Text,
    None,
    Contain,
    All,
}

impl UserSelect {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "text" => Some(Self::Text),
            "none" => Some(Self::None),
            "contain" => Some(Self::Contain),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// CSS Overflow L3 — `scroll-behavior`. Inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollBehavior {
    #[default]
    Auto,
    Smooth,
}

/// CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapType {
    pub axis: ScrollSnapAxis,
    pub strictness: ScrollSnapStrictness,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAxis {
    #[default]
    None,
    X,
    Y,
    Block,
    Inline,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStrictness {
    #[default]
    Proximity,
    Mandatory,
}

/// CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollSnapAlign {
    pub block: ScrollSnapAlignKeyword,
    pub inline: ScrollSnapAlignKeyword,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapAlignKeyword {
    #[default]
    None,
    Start,
    End,
    Center,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollSnapStop {
    #[default]
    Normal,
    Always,
}

/// CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverscrollBehavior {
    #[default]
    Auto,
    Contain,
    None,
}

impl ScrollBehavior {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            _ => None,
        }
    }
}

/// CSS Images L3/L4 §3.3/§3.7 — parsed linear / radial / conic gradient.
///
/// Stored instead of the raw CSS string once `parse_background_gradient`
/// has tokenised the gradient function. `Unknown` is kept as fallback for
/// future / malformed variants so they round-trip without information loss.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedGradient {
    /// `linear-gradient(angle, stop, ...)` — angle in CSS degrees measured
    /// clockwise from "to top" (0° = top, 90° = right, 180° = bottom).
    Linear {
        /// Gradient line angle in CSS degrees (0° = to top, 90° = to right).
        /// For a `to <corner>` keyword this is only the square-box (45/135/
        /// 225/315°) placeholder — `corner` carries the keyword so a paint-time
        /// consumer that knows the actual box size can resolve the true,
        /// aspect-ratio-dependent angle via [`GradientCorner::angle_deg`].
        angle_deg: f32,
        /// `Some` when the direction was written as `to <corner>` (CSS Images
        /// L3 §3.1) rather than an explicit `<angle>` — the true gradient-line
        /// angle for a corner keyword depends on the gradient box's aspect
        /// ratio, which is not known at style-parse time.
        corner: Option<GradientCorner>,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-linear-gradient`.
        repeating: bool,
    },
    /// `radial-gradient(...)` — radial gradient centred at `(cx, cy)`.
    Radial {
        /// Centre as fraction of box width/height ([0, 1] = [left/top, right/bottom]).
        center_x_pct: f32,
        center_y_pct: f32,
        /// Ending shape — `circle` or `ellipse` (CSS Images L3 §3.5). The radii
        /// are resolved against the box at paint time via [`radial_gradient_radii`].
        shape: RadialShape,
        /// Sizing keyword for the ending shape (default `farthest-corner`).
        size: RadialSize,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-radial-gradient`.
        repeating: bool,
    },
    /// CSS Images L4 §3.7 — `conic-gradient([from <angle>]? [at <pos>]?, <stops>)`.
    /// Angular gradient revolving around `(center_x_pct, center_y_pct)` (fraction of
    /// box width/height). `from_angle_deg` is the starting angle in CSS degrees
    /// (0° = top, 90° = right), clockwise. Stops' positions are stored as
    /// `Length::Percent` where 100% corresponds to a full revolution
    /// (angle units `<angle>` are pre-converted to percent on parse).
    Conic {
        center_x_pct: f32,
        center_y_pct: f32,
        /// Starting angle in CSS degrees (0° = top, 90° = right, clockwise).
        from_angle_deg: f32,
        stops: Vec<GradientStop>,
        /// True when the original function was `repeating-conic-gradient`.
        repeating: bool,
    },
    /// Fallback for any future gradient variant not yet rendered.
    Unknown(String),
}

/// CSS Images L3 §3.1 — `to <corner>` keyword of a `linear-gradient`'s
/// direction. Unlike the four side keywords (`to top`/`to right`/…), a corner
/// keyword's true gradient-line angle depends on the gradient box's aspect
/// ratio: the line is defined to pass exactly through the two opposite
/// corners, so on a non-square box it tilts away from the naive 45°
/// diagonal toward whichever side the box is longer along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientCorner {
    /// `to top right` / `to right top`.
    TopRight,
    /// `to bottom right` / `to right bottom`.
    BottomRight,
    /// `to bottom left` / `to left bottom`.
    BottomLeft,
    /// `to top left` / `to left top`.
    TopLeft,
}

impl GradientCorner {
    /// Resolves the keyword to a true gradient-line angle (CSS degrees,
    /// 0° = to top, clockwise) for a box of the given size.
    ///
    /// Per CSS Images L3 §3.1 the gradient line is defined to be
    /// *perpendicular* to the diagonal connecting the two corners the
    /// keyword does *not* name — e.g. for `to bottom right` that diagonal
    /// runs between the top-right and bottom-left corners, direction
    /// `(-width, height)`, so the gradient line itself runs along
    /// `(height, width)`. That makes the base angle `atan2(height, width)`
    /// (note: height first), not `atan2(width, height)` — on a box much
    /// wider than it is tall this angle is *small* (the line tilts toward
    /// vertical, "to bottom"/"to top"), which is the opposite of the naive
    /// "tilts toward the long axis" guess. Verified against a real Edge
    /// render of a 960×160 box: predicted 170.5°, measured ~170.5°.
    /// Reduces to the familiar 45/135/225/315° only when `width == height`.
    pub fn angle_deg(self, width: f32, height: f32) -> f32 {
        let base = height.max(0.0).atan2(width.max(0.0)).to_degrees();
        match self {
            GradientCorner::TopRight => base,
            GradientCorner::BottomRight => 180.0 - base,
            GradientCorner::BottomLeft => 180.0 + base,
            GradientCorner::TopLeft => 360.0 - base,
        }
    }
}

/// CSS Images L3 §3.5 — ending-shape of a `radial-gradient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialShape {
    /// `circle` — isotropic, a single radius along every direction.
    Circle,
    /// `ellipse` (also the default when no shape keyword is given) — independent
    /// horizontal and vertical radii.
    Ellipse,
}

/// CSS Images L3 §3.5 — sizing keyword controlling the radii of a
/// `radial-gradient`'s ending shape. Explicit `<length>` radii are not yet
/// modelled; they fall back to [`RadialSize::FarthestCorner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialSize {
    /// Ending shape meets the side(s) nearest the centre.
    ClosestSide,
    /// Ending shape passes through the corner nearest the centre.
    ClosestCorner,
    /// Ending shape meets the side(s) farthest from the centre.
    FarthestSide,
    /// Ending shape passes through the corner farthest from the centre (default).
    FarthestCorner,
}

/// CSS Images L3 §3.5.1 — resolves a `radial-gradient` ending shape to concrete
/// `(radius_x, radius_y)` in CSS px for a box `w×h` with centre at
/// `(cx_pct·w, cy_pct·h)`. For [`RadialShape::Circle`] both radii are equal.
/// Corner sizes use the aspect ratio of the matching side size and scale the
/// ellipse to pass through the chosen corner (CSS Images L3 §3.5.1, last list
/// item). Radii are clamped to ≥ 1 px to avoid a degenerate gradient.
#[must_use]
pub fn radial_gradient_radii(
    shape: RadialShape, size: RadialSize, cx_pct: f32, cy_pct: f32, w: f32, h: f32,
) -> (f32, f32) {
    let cx = cx_pct * w;
    let cy = cy_pct * h;
    let near_x = cx.abs().min((w - cx).abs());
    let far_x = cx.abs().max((w - cx).abs());
    let near_y = cy.abs().min((h - cy).abs());
    let far_y = cy.abs().max((h - cy).abs());
    // Ellipse with aspect ratio `sx:sy` scaled to pass through corner (cdx, cdy).
    let through_corner = |sx: f32, sy: f32, cdx: f32, cdy: f32| -> (f32, f32) {
        let a = (sx / sy.max(1e-6)).max(1e-6); // rx / ry
        let ry = ((cdx / a).powi(2) + cdy * cdy).sqrt().max(1.0);
        ((a * ry).max(1.0), ry)
    };
    match shape {
        RadialShape::Circle => {
            let r = match size {
                RadialSize::ClosestSide => near_x.min(near_y),
                RadialSize::FarthestSide => far_x.max(far_y),
                RadialSize::ClosestCorner => near_x.hypot(near_y),
                RadialSize::FarthestCorner => far_x.hypot(far_y),
            }
            .max(1.0);
            (r, r)
        }
        RadialShape::Ellipse => match size {
            RadialSize::ClosestSide => (near_x.max(1.0), near_y.max(1.0)),
            RadialSize::FarthestSide => (far_x.max(1.0), far_y.max(1.0)),
            RadialSize::ClosestCorner => through_corner(near_x, near_y, near_x, near_y),
            RadialSize::FarthestCorner => through_corner(far_x, far_y, far_x, far_y),
        },
    }
}

/// CSS Backgrounds L3 §3.1 / CSS Images L4 §4 — `background-image` value.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BackgroundImage {
    #[default]
    None,
    /// `url("path")` or raw `image-set(…)` / `-webkit-image-set(…)` string.
    ///
    /// `image-set()` strings are stored verbatim — paint resolves them to the
    /// best URL for the current DPR via `select_image_set_url` (CSS Images L4 §5).
    Url(String),
    /// Parsed gradient. Phase 0 renders linear / radial / conic.
    Gradient(ParsedGradient),
    /// CSS Images L4 §4 — `cross-fade(<image-a>, <image-b>, <percentage>)`.
    ///
    /// `t` is the blend factor in `[0.0, 1.0]`: `0.0` = fully `a`, `1.0` = fully `b`.
    CrossFade {
        /// First image (`t = 0.0`).
        a: Box<BackgroundImage>,
        /// Second image (`t = 1.0`).
        b: Box<BackgroundImage>,
        /// Blend factor clamped to `[0.0, 1.0]`.
        t: f32,
    },
    /// CSS Paint API (Houdini) — `paint(name)` generates dynamic image via registered worklet.
    /// Phase 0: stored as placeholder grey `DrawImage`; Phase 1: calls worklet `paint()` callback.
    Paint(String),
}

/// CSS Backgrounds L3 §3.4 — `background-repeat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundRepeat {
    #[default]
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
    Round,
    Space,
}

impl BackgroundRepeat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "repeat" => Some(Self::Repeat),
            "no-repeat" => Some(Self::NoRepeat),
            "repeat-x" => Some(Self::RepeatX),
            "repeat-y" => Some(Self::RepeatY),
            "round" => Some(Self::Round),
            "space" => Some(Self::Space),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.5 — one axis of an explicit `background-size` value.
///
/// `Px`/`Percent` are resolved against the positioning area extent along this
/// axis at paint time; `Auto` derives the extent from the other axis (preserving
/// the image's intrinsic aspect ratio).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BgSizeAxis {
    /// Derive this axis from the other axis / the image's intrinsic ratio.
    Auto,
    /// Fixed length in CSS px.
    Px(f32),
    /// Percentage of the positioning area along this axis (fraction `0.0..`).
    Percent(f32),
}

impl BgSizeAxis {
    /// Resolve to a concrete px extent against `area` (the positioning-area
    /// size along this axis). Returns `None` for `Auto` (caller derives it from
    /// the other axis / intrinsic ratio).
    #[must_use]
    pub fn resolve(self, area: f32) -> Option<f32> {
        match self {
            BgSizeAxis::Auto => None,
            BgSizeAxis::Px(v) => Some(v),
            BgSizeAxis::Percent(p) => Some(p * area),
        }
    }
}

/// CSS Backgrounds L3 §3.5 — `background-size`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum BackgroundSize {
    #[default]
    Auto,
    Cover,
    Contain,
    /// Explicit width / height, each `auto` | `<length>` | `<percentage>`.
    /// Percentages resolve against the positioning area at paint time.
    Length(BgSizeAxis, BgSizeAxis),
}

/// CSS Backgrounds L3 §3.6 — `background-attachment`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundAttachment {
    #[default]
    Scroll,
    Fixed,
    Local,
}

impl BackgroundAttachment {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "scroll" => Some(Self::Scroll),
            "fixed" => Some(Self::Fixed),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited.
///
/// Определяет, к какому **краю box-а** привязана позиционная система
/// для `background-image` (initial = padding edge). На `background-color`
/// не влияет (тот всегда заливает border-edge независимо от origin).
///
/// **Phase 0 ограничение:** parsing + storage only. Реальное смещение
/// origin-у в paint pipeline (выбор `border_box` / `padding_box` /
/// `content_box` rect при расчёте начала tile-тиления) — отдельная
/// задача с согласованием P2 (crate-ownership matrix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundOrigin {
    /// `border-box` — позиционная система начинается с border-edge.
    BorderBox,
    /// `padding-box` (initial) — с padding-edge (= внутренний край border-а).
    #[default]
    PaddingBox,
    /// `content-box` — с content-edge (= внутренний край padding-а).
    ContentBox,
}

impl BackgroundOrigin {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited.
///
/// Определяет, к какому **краю box-а** обрезается `background-color`
/// и `background-image` (initial = border edge, т.е. фон видно даже
/// сквозь полупрозрачную рамку).
///
/// Variant `Text` (CSS Backgrounds L4) клипает фон по форме глифов —
/// классический паттерн «gradient text» через `background-clip: text`
/// и `color: transparent`. Реализация в paint требует подмаски через
/// glyph-cache mask-image — отдельная задача с согласованием P2.
///
/// **Phase 0 ограничение:** parsing + storage only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundClip {
    /// `border-box` (initial) — фон под border-ом виден.
    #[default]
    BorderBox,
    /// `padding-box` — фон обрезается до внутреннего края border-а.
    PaddingBox,
    /// `content-box` — фон только в content-area.
    ContentBox,
    /// `text` (CSS Backgrounds L4) — фон клипается по форме текста
    /// внутри box-а. Phase 0 хранит как atom, реальный clip — P2.
    Text,
}

impl BackgroundClip {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// CSS Masking L1 §4.6 — `mask-clip: <coord-box> | no-clip`.
///
/// `<coord-box>` = `content-box | padding-box | border-box | fill-box |
/// stroke-box | view-box`. Unlike `background-clip`, `mask-clip` also accepts
/// the SVG reference boxes and the `no-clip` keyword. For elements laid out
/// with the CSS box model (non-SVG HTML boxes) the SVG-specific boxes fall
/// back to their box-model equivalents (CSS Box 4 §1 "Choosing the layout
/// box"): `fill-box` → content box, `stroke-box`/`view-box` → border box.
/// `no-clip` disables the mask painting-area clip entirely. Non-inherited,
/// initial `border-box`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaskClip {
    /// `border-box` (initial) — mask painting area is the border box.
    #[default]
    BorderBox,
    /// `padding-box` — clip to the inner border edge.
    PaddingBox,
    /// `content-box` — clip to the content area.
    ContentBox,
    /// `fill-box` — object bounding box; for CSS boxes equals the content box.
    FillBox,
    /// `stroke-box` — stroke bounding box; for CSS boxes equals the border box.
    StrokeBox,
    /// `view-box` — nearest SVG viewport; for CSS boxes equals the border box.
    ViewBox,
    /// `no-clip` — the mask painting area is not clipped.
    NoClip,
}

impl MaskClip {
    /// Parses a single `mask-clip` keyword (CSS Masking L1 §4.6).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "border-box" => Some(Self::BorderBox),
            "padding-box" => Some(Self::PaddingBox),
            "content-box" => Some(Self::ContentBox),
            "fill-box" => Some(Self::FillBox),
            "stroke-box" => Some(Self::StrokeBox),
            "view-box" => Some(Self::ViewBox),
            "no-clip" => Some(Self::NoClip),
            _ => None,
        }
    }
}

/// CSS Backgrounds L3 §3 — один фоновый слой. Первый в Vec = верхний (рисуется последним).
///
/// Все поля — initial values из спецификации. `background_color` не входит
/// в слой — он всегда одиночный и хранится в `ComputedStyle.background_color`.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundLayer {
    /// `background-image` для этого слоя.
    pub image: BackgroundImage,
    /// `background-repeat` для этого слоя.
    pub repeat: BackgroundRepeat,
    /// `background-size` для этого слоя.
    pub size: BackgroundSize,
    /// `background-position` для этого слоя.
    pub position: ObjectPosition,
    /// `background-attachment` для этого слоя.
    pub attachment: BackgroundAttachment,
    /// `background-origin` для этого слоя.
    pub origin: BackgroundOrigin,
    /// `background-clip` для этого слоя.
    pub clip: BackgroundClip,
    /// CSS Compositing L1 §8.3 — `background-blend-mode` для этого слоя.
    /// Initial: normal. Не наследуется. Применяется при слиянии background
    /// layers между собой (не с контентом элемента).
    pub blend_mode: MixBlendMode,
}

impl Default for BackgroundLayer {
    fn default() -> Self {
        Self {
            image: BackgroundImage::None,
            repeat: BackgroundRepeat::Repeat,
            size: BackgroundSize::Auto,
            position: ObjectPosition::background_initial(),
            attachment: BackgroundAttachment::Scroll,
            origin: BackgroundOrigin::PaddingBox,
            clip: BackgroundClip::BorderBox,
            blend_mode: MixBlendMode::Normal,
        }
    }
}

/// CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
/// (`<img>`, `<video>`, `<canvas>` и т.д.) и определяет, как «коробка»
/// заливается содержимым с учётом intrinsic-размеров. Не наследуется.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObjectFit {
    /// `fill` (default) — растянуть на размер коробки без сохранения
    /// aspect ratio. Картинка может быть искажена.
    #[default]
    Fill,
    /// `contain` — максимально большой размер с сохранением aspect ratio,
    /// при котором изображение **умещается** целиком (letterbox / pillarbox).
    Contain,
    /// `cover` — минимально большой размер с сохранением aspect ratio,
    /// при котором изображение **покрывает** коробку. Излишки клипятся
    /// по `object-position`.
    Cover,
    /// `none` — без масштабирования (intrinsic-размер 1:1). Излишки
    /// клипятся; недостаток заполняется по `object-position`.
    None,
    /// `scale-down` — `min(none, contain)`: если intrinsic-размер меньше
    /// коробки, ведёт себя как `none`; иначе как `contain`.
    ScaleDown,
}

impl ObjectFit {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fill" => Some(Self::Fill),
            "contain" => Some(Self::Contain),
            "cover" => Some(Self::Cover),
            "none" => Some(Self::None),
            "scale-down" => Some(Self::ScaleDown),
            _ => None,
        }
    }
}

/// CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
/// масштабировать растровое изображение (применимо к `<img>`, background-image,
/// canvas, и т.д.). Inherited.
///
/// Phase 0: parsing + storage. Реальное переключение GPU sampler filter
/// (`Linear` для `auto`/`smooth`/`high-quality`, `Nearest` для `pixelated`/
/// `crisp-edges`) в `lumen-paint` — отдельная задача с согласованием P2.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageRendering {
    /// `auto` (default) — UA выбирает алгоритм. Обычно — bilinear.
    #[default]
    Auto,
    /// `smooth` — high-quality scaling, оптимизирован для smooth gradient.
    /// На практике в современных движках = `auto`.
    Smooth,
    /// `high-quality` — высочайшее качество масштабирования (тяжелее `smooth`).
    /// Спецификация добавлена в CSS Images L4; считается переименованием
    /// `optimizeQuality` из L3 (которое теперь deprecated).
    HighQuality,
    /// `crisp-edges` — сохраняет контраст и резкость границ (pixel art /
    /// vector graphics). UA может использовать nearest-neighbour или
    /// edge-preserving алгоритм.
    CrispEdges,
    /// `pixelated` — nearest-neighbour. Полезно для масштабирования pixel art.
    Pixelated,
}

impl ImageRendering {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "smooth" => Some(Self::Smooth),
            "high-quality" => Some(Self::HighQuality),
            "crisp-edges" => Some(Self::CrispEdges),
            "pixelated" => Some(Self::Pixelated),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited.
///
/// Управляет тем, переносятся ли строки внутри блока. `wrap` — нормальный
/// перенос по soft wrap opportunities (initial). `nowrap` — текст растягивается
/// в одну линию, до явного break-control (`<br>`, preserved newline).
///
/// Является non-shorthand-частью `text-wrap` (§6.4.3) и одновременно
/// частью legacy `white-space` shorthand (§2.1 — `white-space-collapse` ||
/// `text-wrap-mode` || `white-space-trim`). В этой кодовой базе `white-space`
/// исторически хранится отдельным [`WhiteSpace`] enum-ом — связка двух полей
/// уйдёт в типизацию декрараций (P1 1B).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    /// `wrap` (initial) — обычный перенос строк.
    #[default]
    Wrap,
    /// `nowrap` — без переноса, текст в одну линию.
    Nowrap,
}

impl TextWrapMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wrap" => Some(Self::Wrap),
            "nowrap" => Some(Self::Nowrap),
            _ => None,
        }
    }
}

/// CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited.
///
/// Расширенные стратегии перевода строк. `auto` — UA выбирает по умолчанию
/// (обычно greedy first-fit). Остальные значения — типографические
/// улучшения, требующие реального line-breaker-а (Knuth–Plass / Latin
/// last-line orphan-prevention) — Phase 0 хранит как atom, применение
/// откладывается до интеграции с `UnicodeProvider` (provisional `icu4x`,
/// P1 п.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWrapStyle {
    /// `auto` (initial) — UA-default стратегия (обычно greedy).
    #[default]
    Auto,
    /// `balance` — балансировать длины строк короткого блока (≤ ~10 строк).
    Balance,
    /// `stable` — стабильные break-points при редактировании (для contenteditable).
    Stable,
    /// `pretty` — улучшенный last-line (без orphan / висячих слов).
    Pretty,
}

impl TextWrapStyle {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "balance" => Some(Self::Balance),
            "stable" => Some(Self::Stable),
            "pretty" => Some(Self::Pretty),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited.
///
/// Задаёт направление главной оси flex-контейнера. Phase 0: parsing + storage;
/// реальный flex-layout pass — задача 4B.3.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexDirection {
    /// `row` (initial) — горизонтально, слева направо.
    #[default]
    Row,
    /// `row-reverse` — горизонтально, справа налево.
    RowReverse,
    /// `column` — вертикально, сверху вниз.
    Column,
    /// `column-reverse` — вертикально, снизу вверх.
    ColumnReverse,
}

impl FlexDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "row-reverse" => Some(Self::RowReverse),
            "column" => Some(Self::Column),
            "column-reverse" => Some(Self::ColumnReverse),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited.
///
/// Разрешает или запрещает перенос flex-элементов на новые строки/столбцы.
/// Phase 0: parsing + storage; реальный multi-line flex — задача 4B.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexWrap {
    /// `nowrap` (initial) — все элементы в одну строку.
    #[default]
    Nowrap,
    /// `wrap` — перенос вперёд (вниз или вправо).
    Wrap,
    /// `wrap-reverse` — перенос назад.
    WrapReverse,
}

impl FlexWrap {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "nowrap" => Some(Self::Nowrap),
            "wrap" => Some(Self::Wrap),
            "wrap-reverse" => Some(Self::WrapReverse),
            _ => None,
        }
    }
}

/// CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited.
///
/// Размер flex-элемента вдоль главной оси до применения grow/shrink.
/// Phase 0: parsing + storage; реальный flex-layout — задача 4B.3.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum FlexBasis {
    /// `auto` (initial) — использовать width/height элемента.
    #[default]
    Auto,
    /// `content` — intrinsic content-size (CSS Flexbox L1 §7.3.2).
    Content,
    /// Explicit length/percentage.
    Length(Length),
}

impl FlexBasis {
    pub fn parse(s: &str, is_quirks: bool) -> Option<Self> {
        let trimmed = s.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "content" => Some(Self::Content),
            _ => parse_length_q(trimmed, is_quirks).map(Self::Length),
        }
    }
}

/// CSS Grid Layout L3 §9 — `repeat(auto-fill | auto-fit | <count>, <track-list>)`.
/// Stored in grid_template_columns/rows during Phase 0 to preserve repeat information
/// until resolution time (lay_out_grid). Expanded via `resolve_grid_template` before layout.
#[derive(Debug, Clone, PartialEq)]
pub struct GridRepeat {
    /// `Count::Fixed(N)` for `repeat(N, ...)`, `AutoFill` for auto-fill, `AutoFit` for auto-fit.
    pub count: RepeatCount,
    /// The track sizing functions inside the parentheses, e.g. `minmax(100px, 1fr)`.
    pub tracks: Vec<GridTrackSize>,
}

/// Count type for grid-template-columns/rows `repeat()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatCount {
    /// Fixed count: `repeat(3, ...)`.
    Fixed(usize),
    /// Auto-fill: `repeat(auto-fill, ...)` — fill available space, prefer empty tracks over overflow.
    AutoFill,
    /// Auto-fit: `repeat(auto-fit, ...)` — fill available space, collapse empty tracks.
    AutoFit,
}

/// CSS Grid Layout L1 §7.2 — sizing function for a grid track.
/// Non-inherited. Appears in `grid-template-columns` / `grid-template-rows`
/// and `grid-auto-columns` / `grid-auto-rows`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum GridTrackSize {
    /// `auto` — sized by content (min-content as min, max-content as max).
    #[default]
    Auto,
    /// Fixed length (px, em, rem, %).
    Length(Length),
    /// `<number>fr` — fractional unit of remaining free space.
    Fr(f32),
    /// `min-content` — minimum content size.
    MinContent,
    /// `max-content` — maximum content size.
    MaxContent,
    /// `minmax(min, max)` — track between min and max sizing functions.
    Minmax(Box<GridTrackSize>, Box<GridTrackSize>),
    /// `fit-content(N)` — track sized to fit content with max limit (CSS Grid L3 §9.1).
    /// Equivalent to `minmax(auto, max(auto, min(N, max-content)))`.
    FitContent(Box<GridTrackSize>),
    /// `subgrid` — inherit track sizes from the spanning tracks of the parent grid
    /// (CSS Grid Layout L2 §9). The grid item must itself be a grid container;
    /// its column/row tracks are replaced by the parent's resolved track sizes
    /// for the cells it spans. Stored as a sentinel `vec![GridTrackSize::Subgrid]`
    /// in `grid_template_columns` or `grid_template_rows`.
    Subgrid,
    /// `masonry` — CSS Grid L3 §14 waterfall layout axis sentinel.
    /// Stored as `vec![GridTrackSize::Masonry]` in `grid_template_columns` or
    /// `grid_template_rows` to signal that the axis uses masonry placement.
    /// The perpendicular axis defines track sizes; `masonry.rs` handles placement.
    /// P4 handoff: `masonry-auto-flow`, `align-tracks`, `justify-tracks` in ComputedStyle.
    Masonry,
}

impl GridTrackSize {
    /// Resolve to a concrete pixel size given container width, em, viewport.
    /// For `fr`, `auto`, `fit-content`, `subgrid`, and `masonry` returns `None` — caller handles those specially.
    pub fn resolve_fixed(&self, em: f32, cb: f32, viewport: Size) -> Option<f32> {
        match self {
            Self::Length(l) => l.resolve(em, Some(cb), viewport),
            Self::Fr(_) | Self::Auto | Self::MinContent | Self::MaxContent | Self::FitContent(_) | Self::Subgrid | Self::Masonry => None,
            Self::Minmax(min, _max) => min.resolve_fixed(em, cb, viewport),
        }
    }

    /// True for fractional tracks.
    pub fn is_fr(&self) -> bool {
        matches!(self, Self::Fr(_))
    }

    /// Extract fr value.
    pub fn fr(&self) -> Option<f32> {
        if let Self::Fr(v) = self { Some(*v) } else { None }
    }

    /// True when this track inherits its size from the parent grid (subgrid axis).
    pub fn is_subgrid(&self) -> bool {
        matches!(self, Self::Subgrid)
    }

    /// True when this axis uses masonry placement (CSS Grid L3 §14).
    pub fn is_masonry(&self) -> bool {
        matches!(self, Self::Masonry)
    }

    /// Parse a single track sizing keyword / value (no `repeat()`).
    fn parse_single(s: &str, is_quirks: bool) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => return Some(Self::Auto),
            "min-content" => return Some(Self::MinContent),
            "max-content" => return Some(Self::MaxContent),
            // `subgrid` / `masonry` as single tokens are handled in parse_track_list;
            // reaching here means they appeared inside a repeat() context — treat as auto.
            "subgrid" | "masonry" => return Some(Self::Auto),
            _ => {}
        }
        // `<number>fr`
        if let Some(n) = lc.strip_suffix("fr")
            && let Ok(v) = n.trim().parse::<f32>()
        {
            return Some(Self::Fr(v.max(0.0)));
        }
        // `minmax(min, max)`
        if lc.starts_with("minmax(") && lc.ends_with(')') {
            let inner = &s.trim()[7..s.trim().len() - 1];
            if let Some((a, b)) = split_paren_aware_comma(inner) {
                let min = Self::parse_single(a.trim(), is_quirks)?;
                let max = Self::parse_single(b.trim(), is_quirks)?;
                return Some(Self::Minmax(Box::new(min), Box::new(max)));
            }
        }
        // `fit-content(<length-percentage>)` (CSS Grid L3 §9.1)
        if lc.starts_with("fit-content(") && lc.ends_with(')') {
            let inner = &s.trim()[12..s.trim().len() - 1];
            if let Some(limit) = Self::parse_single(inner.trim(), is_quirks) {
                return Some(Self::FitContent(Box::new(limit)));
            }
        }
        // length / percentage
        parse_length_q(s.trim(), is_quirks).map(Self::Length)
    }

    /// Parse a track-list value string into a Vec of GridTrackSize.
    /// Handles `repeat(N, <track-list>)` by expanding.
    /// `subgrid` as the entire value returns `vec![Subgrid]` (sentinel for the whole axis).
    /// `masonry` as the entire value returns `vec![Masonry]` (CSS Grid L3 §14 sentinel).
    pub fn parse_track_list(s: &str, is_quirks: bool) -> Vec<Self> {
        let trimmed = s.trim();
        // CSS Grid L2 §9: `subgrid` replaces the entire track list for that axis.
        if trimmed.eq_ignore_ascii_case("subgrid") {
            return vec![Self::Subgrid];
        }
        // CSS Grid L3 §14: `masonry` replaces the entire track list — waterfall placement axis.
        if trimmed.eq_ignore_ascii_case("masonry") {
            return vec![Self::Masonry];
        }
        let mut result = Vec::new();
        for token in split_track_list_tokens(trimmed) {
            let t = token.trim();
            let lc = t.to_ascii_lowercase();
            if lc.starts_with("repeat(") && lc.ends_with(')') {
                let inner = &t[7..t.len() - 1];
                if let Some((count_s, rest)) = split_paren_aware_comma(inner) {
                    let count_s_trim = count_s.trim();
                    let count_lc = count_s_trim.to_ascii_lowercase();
                    let count = if count_lc == "auto-fill" {
                        RepeatCount::AutoFill
                    } else if count_lc == "auto-fit" {
                        RepeatCount::AutoFit
                    } else if let Ok(n) = count_s_trim.parse::<usize>() {
                        RepeatCount::Fixed(n)
                    } else {
                        continue; // Invalid repeat count, skip
                    };

                    let tracks = Self::parse_track_list(rest.trim(), is_quirks);
                    if count == RepeatCount::Fixed(0) {
                        // zero repeat, add nothing
                    } else if matches!(count, RepeatCount::Fixed(_)) {
                        // Expand fixed repeat immediately
                        let n = match count {
                            RepeatCount::Fixed(n) => n,
                            _ => unreachable!(),
                        };
                        for _ in 0..n {
                            result.extend(tracks.iter().cloned());
                        }
                    } else {
                        // For auto-fill / auto-fit, store GridRepeat sentinel for resolution at layout time
                        // Phase 1: Add first track as GridRepeat sentinel. Caller (lay_out_grid) resolves count.
                        // For now, expand as single "repeat" marker that resolver can recognize and expand.
                        if !tracks.is_empty() {
                            // Store info in a way resolver can find: mark with a sentinel or new enum variant.
                            // Currently: add the first track once, and store GridRepeat in ComputedStyle separately.
                            // Phase 1 simplified: treat as auto (no expansion) until resolver wire-up
                            result.extend(tracks.iter().cloned());
                        }
                    }
                }
            } else if let Some(ts) = Self::parse_single(t, is_quirks) {
                result.push(ts);
            }
        }
        result
    }
}

/// Extracts auto-fill/auto-fit repeat metadata from a track-list string.
/// Returns `Some(GridRepeat)` when the string is exactly `repeat(auto-fill|auto-fit, ...)`.
/// Used in Phase 2 of CSS Grid auto-repeat expansion (CSS Grid L1 §7.2.3.4).
pub(crate) fn parse_auto_repeat(s: &str) -> Option<GridRepeat> {
    let trimmed = s.trim();
    // Must start with "repeat(" (case-insensitive) and end with ")"
    let lc = trimmed.to_ascii_lowercase();
    let inner = lc.strip_prefix("repeat(")?.strip_suffix(')')?;
    let (count_s, rest) = split_paren_aware_comma(inner)?;
    let count = match count_s.trim() {
        "auto-fill" => RepeatCount::AutoFill,
        "auto-fit" => RepeatCount::AutoFit,
        _ => return None,
    };
    // Re-parse from original string to preserve case in track sizes
    let orig_inner = trimmed
        .get("repeat(".len()..trimmed.len() - 1)?;
    let (_, orig_rest) = split_paren_aware_comma(orig_inner)?;
    let tracks = GridTrackSize::parse_track_list(orig_rest.trim(), false);
    if tracks.is_empty() {
        return None;
    }
    let _ = rest; // suppress unused warning from lc version
    Some(GridRepeat { count, tracks })
}

/// Split a comma inside a track-list token that may contain nested parens.
fn split_paren_aware_comma(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Tokenize a track-list string into individual track tokens,
/// respecting parentheses (so `minmax(...)` stays as one token).
fn split_track_list_tokens(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b' ' | b'\t' | b'\n' if depth == 0 => {
                let tok = s[start..i].trim();
                if !tok.is_empty() {
                    tokens.push(tok);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        tokens.push(last);
    }
    tokens
}

/// CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GridAutoFlow {
    /// `row` (initial) — fill rows, add new rows as needed.
    #[default]
    Row,
    /// `column` — fill columns, add new columns as needed.
    Column,
    /// `row dense` — row flow with dense packing.
    RowDense,
    /// `column dense` — column flow with dense packing.
    ColumnDense,
}

impl GridAutoFlow {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "row" => Some(Self::Row),
            "column" => Some(Self::Column),
            "row dense" | "dense row" => Some(Self::RowDense),
            "column dense" | "dense column" => Some(Self::ColumnDense),
            _ => None,
        }
    }
}

/// CSS Masonry Layout §9 — `masonry-auto-flow`. Controls the placement order
/// of auto-placed items in a masonry container. Non-inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MasonryAutoFlow {
    /// `definite-first` (initial) — items with an explicit grid-axis position are
    /// placed first, then auto items in source order.
    #[default]
    DefiniteFirst,
    /// `next` — all items placed in source order, no definite-first prioritisation.
    Next,
    /// `ordered` — items sorted by their CSS `order` property before placement.
    Ordered,
}

impl MasonryAutoFlow {
    /// Parse a CSS `masonry-auto-flow` value string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "definite-first" => Some(Self::DefiniteFirst),
            "next" => Some(Self::Next),
            "ordered" => Some(Self::Ordered),
            _ => None,
        }
    }
}

/// CSS Grid Layout L1 §8.3 — a grid-line reference for grid-column-start,
/// grid-column-end, grid-row-start, grid-row-end. Non-inherited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GridLine {
    /// `auto` — automatic placement.
    #[default]
    Auto,
    /// Integer line number (1-based from start, negative from end).
    Line(i32),
    /// `span <integer>` — span N tracks.
    Span(u32),
    /// Named grid area reference (CSS Grid L1 §8.3). Resolved at layout time
    /// by looking up the name in the containing grid's `grid-template-areas`.
    Named(String),
}

impl GridLine {
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        // `span N` or `span`
        if let Some(rest) = trimmed.to_ascii_lowercase().strip_prefix("span") {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Self::Span(1));
            }
            if let Ok(n) = rest.parse::<u32>() {
                return Some(Self::Span(n.max(1)));
            }
        }
        // integer line number
        if let Ok(n) = trimmed.parse::<i32>() && n != 0 {
            return Some(Self::Line(n));
        }
        // CSS custom-ident: named grid area or named line.
        // Only accept valid CSS idents (letters, digits, hyphens, underscores;
        // cannot start with a digit or two hyphens without a letter).
        if is_css_ident(trimmed) {
            return Some(Self::Named(trimmed.to_string()));
        }
        None
    }
}

/// Одна компонента `object-position`. Length-варианты резолвятся в px
/// относительно края коробки (positive = от left/top); percentage —
/// относительно **свободного места** `box_size - content_size` (может быть
/// отрицательным, тогда излишек уходит за противоположный край). См.
/// CSS Images L3 §5.5 «object-position».
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionComponent {
    /// Length в px (после resolve em/rem/vw/...).
    Px(f32),
    /// Percentage в долях 1.0 (`50%` → 0.5). Резолвится на paint-стадии
    /// против свободного места: `offset = free_space * percent`.
    Percent(f32),
}

impl PositionComponent {
    /// Резолв в финальный px-offset относительно левого/верхнего края
    /// коробки. `free_space = box_size - content_size`; может быть
    /// отрицательным (content > box) — тогда offset тоже отрицательный,
    /// и излишек уезжает за противоположный край.
    pub fn resolve(self, free_space: f32) -> f32 {
        match self {
            Self::Px(px) => px,
            Self::Percent(p) => free_space * p,
        }
    }
}

/// CSS Images L3 §5.5 — `object-position` (две компоненты, x + y).
/// Default — `50% 50%` (центр).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectPosition {
    pub x: PositionComponent,
    pub y: PositionComponent,
}

impl Default for ObjectPosition {
    fn default() -> Self {
        Self {
            x: PositionComponent::Percent(0.5),
            y: PositionComponent::Percent(0.5),
        }
    }
}

impl ObjectPosition {
    /// CSS Backgrounds L3 §3.5 — initial value `background-position: 0% 0%`
    /// (top-left). Отличается от Object Position default (`50% 50%`, центр)
    /// специально потому, что `background-image` обычно anchored к top-left
    /// при первой укладке (см. CSS 2.1 §14.2.1).
    pub const fn background_initial() -> Self {
        Self {
            x: PositionComponent::Percent(0.0),
            y: PositionComponent::Percent(0.0),
        }
    }
}

impl ObjectPosition {
    /// CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
    /// поддерживает:
    ///   - keyword `center` (= 50%),
    ///   - axis-keywords `left|right|top|bottom`,
    ///   - один token (`50%`, `10px`, keyword) — второй = `center`,
    ///   - два token-а — первый x, второй y.
    ///
    /// Tri- и quad-форма (`<keyword> <length> <keyword> <length>` для
    /// сторон-якорей) — отложены: на современных страницах редкость.
    pub fn parse(s: &str, em_basis: f32, viewport: Size) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 2 {
            return None;
        }
        // Single-token: применяется к horizontal оси; вертикальная = center.
        // Если token — vertical keyword (`top`/`bottom`), то horizontal = center.
        if tokens.len() == 1 {
            let t = tokens[0];
            if t.eq_ignore_ascii_case("top") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(0.0),
                });
            }
            if t.eq_ignore_ascii_case("bottom") {
                return Some(Self {
                    x: PositionComponent::Percent(0.5),
                    y: PositionComponent::Percent(1.0),
                });
            }
            let x = parse_position_component(t, em_basis, viewport, /*vertical*/ false)?;
            return Some(Self {
                x,
                y: PositionComponent::Percent(0.5),
            });
        }
        // Two-token form: <x> <y>. Swap, если порядок инвертирован
        // (`top left` ≡ `left top`).
        let (t0, t1) = (tokens[0], tokens[1]);
        let (xtok, ytok) = if is_vertical_keyword(t0) || is_horizontal_keyword(t1) {
            (t1, t0)
        } else {
            (t0, t1)
        };
        let x = parse_position_component(xtok, em_basis, viewport, false)?;
        let y = parse_position_component(ytok, em_basis, viewport, true)?;
        Some(Self { x, y })
    }
}

fn is_vertical_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("top") || t.eq_ignore_ascii_case("bottom")
}

fn is_horizontal_keyword(t: &str) -> bool {
    t.eq_ignore_ascii_case("left") || t.eq_ignore_ascii_case("right")
}

fn parse_position_component(
    t: &str,
    em_basis: f32,
    viewport: Size,
    vertical: bool,
) -> Option<PositionComponent> {
    // Keyword-формы.
    if t.eq_ignore_ascii_case("center") {
        return Some(PositionComponent::Percent(0.5));
    }
    if !vertical {
        if t.eq_ignore_ascii_case("left") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("right") {
            return Some(PositionComponent::Percent(1.0));
        }
        // top/bottom в horizontal-позиции — недопустимо.
        if is_vertical_keyword(t) {
            return None;
        }
    } else {
        if t.eq_ignore_ascii_case("top") {
            return Some(PositionComponent::Percent(0.0));
        }
        if t.eq_ignore_ascii_case("bottom") {
            return Some(PositionComponent::Percent(1.0));
        }
        if is_horizontal_keyword(t) {
            return None;
        }
    }
    // Length / percentage. Percent-форма `50%` сохраняется как доля 0..=1
    // (без clamp — отрицательные и >100% валидны по спеке и используются
    // художниками для художественных смещений).
    if let Some(pct) = t.strip_suffix('%')
        && let Ok(n) = pct.trim().parse::<f32>()
    {
        return Some(PositionComponent::Percent(n / 100.0));
    }
    let len = parse_length(t)?;
    let px = len.resolve(em_basis, None, viewport)?;
    Some(PositionComponent::Px(px))
}

/// CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств.
/// Phase 0: основной набор keyword-ов. `Auto` — default (resolve в
/// `Normal` или specific behavior контекстом).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlignValue {
    /// CSS keyword `auto` — default. Behavior зависит от контекста
    /// (parent layout type). Для absolute-positioned — `normal`.
    #[default]
    Auto,
    /// `normal` — default-behavior для conteneur'а (stretch for grid,
    /// start for flex).
    Normal,
    /// `stretch` — растянуть на доступное место (default для grid).
    Stretch,
    /// `start` / `flex-start` — выровнять к началу cross/main axis.
    Start,
    /// `end` / `flex-end` — выровнять к концу.
    End,
    /// `center` — выровнять по центру.
    Center,
    /// `baseline` — выровнять text-baseline (для align-items).
    Baseline,
    /// `space-between` — равные промежутки между items, по краям нет.
    SpaceBetween,
    /// `space-around` — промежутки между + половинные по краям.
    SpaceAround,
    /// `space-evenly` — все промежутки одинаковые, включая края.
    SpaceEvenly,
}

impl AlignValue {
    pub fn parse(s: &str) -> Option<Self> {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "auto" => Some(Self::Auto),
            "normal" => Some(Self::Normal),
            "stretch" => Some(Self::Stretch),
            "start" | "flex-start" | "self-start" => Some(Self::Start),
            "end" | "flex-end" | "self-end" => Some(Self::End),
            "center" => Some(Self::Center),
            "baseline" | "first baseline" | "last baseline" => Some(Self::Baseline),
            "space-between" => Some(Self::SpaceBetween),
            "space-around" => Some(Self::SpaceAround),
            "space-evenly" => Some(Self::SpaceEvenly),
            _ => None,
        }
    }
}

/// CSS Masking L1 §3.5 — `<length-percentage>` значение координаты/размера
/// basic-shape для `clip-path`. Проценты резолвятся на этапе paint
/// относительно reference box (border-box элемента): горизонтальные — по
/// width, вертикальные — по height, радиус `circle()` — по
/// `sqrt(w²+h²)/√2` (CSS Shapes L1 §5.1, BUG-140).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeValue {
    /// Абсолютное значение в px (em/rem уже приведены к px при парсинге).
    Px(f32),
    /// Процент соответствующего базиса reference box (0–100).
    Pct(f32),
}

impl ShapeValue {
    /// Резолвит значение в px. `basis` — размер reference box по
    /// соответствующей оси (px); для `Px` игнорируется.
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Self::Px(v) => v,
            Self::Pct(p) => p / 100.0 * basis,
        }
    }
}

/// CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
/// поддерживает: `inset(...)`, `circle(...)`, `ellipse(...)`,
/// `polygon(...)`. URL / `path()` / `none` отложены.
/// Координаты — `ShapeValue` (px или %), проценты резолвятся по
/// border-box на этапе paint (BUG-140: `circle(40% at 50% 50%)` раньше
/// молча отбрасывался целиком).
#[derive(Debug, Clone, PartialEq)]
pub enum ClipPath {
    /// `inset(top right bottom left)` — 1..=4 length-percentage значения
    /// (top/bottom — % от height, left/right — % от width).
    Inset(Vec<ShapeValue>),
    /// `circle(radius at cx cy)` — radius и center (опц.).
    Circle {
        /// Радиус: % резолвится по `sqrt(w²+h²)/√2`.
        radius: ShapeValue,
        /// Центр (cx — % от width, cy — % от height); `None` = 50% 50%.
        center: Option<(ShapeValue, ShapeValue)>,
    },
    /// `ellipse(rx ry at cx cy)` — rx — % от width, ry — % от height.
    Ellipse {
        /// Горизонтальный радиус.
        rx: ShapeValue,
        /// Вертикальный радиус.
        ry: ShapeValue,
        /// Центр; `None` = 50% 50%.
        center: Option<(ShapeValue, ShapeValue)>,
    },
    /// `polygon([<fill-rule>,]? x1 y1, x2 y2, ...)` — список вершин (x — % от
    /// width, y — % от height) + правило заливки. `FillRule` (CSS Shapes L1
    /// §3) управляет самопересекающимися полигонами: `EvenOdd` оставляет
    /// «дырки» в местах перекрытия, `NonZero` (default) заливает их.
    Polygon(Vec<(ShapeValue, ShapeValue)>, FillRule),
    /// `path([<fill-rule>,]? "<svg-path>")` — CSS Shapes L1 §4. Хранит
    /// предварительно флэттенный полигон в px-координатах системы пути
    /// (origin = верхний левый угол reference box; проценты в `path()`
    /// недопустимы по спецификации). Кривые разбиты на отрезки на этапе
    /// парсинга через `motion_path::flatten_path_to_polygon`. Второе поле —
    /// `FillRule` (default `NonZero`); `EvenOdd` делает дырки в
    /// самопересекающихся путях (звёзды-пентаграммы и т. п.).
    Path(Vec<(f32, f32)>, FillRule),
}

/// CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
/// translate/translateX/translateY, rotate, scale/scaleX/scaleY,
/// CSS Transforms L2 §6 — `transform-style: flat | preserve-3d`.
/// `Flat` = children are flattened into the parent plane (default).
/// `Preserve3d` = children participate in the parent 3D rendering context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformStyle {
    #[default]
    Flat,
    Preserve3d,
}

/// CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`.
/// `Hidden` = element is invisible when its back face is oriented toward
/// the viewer (requires a 3D rendering context to have any effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackfaceVisibility {
    /// Back face is visible (initial value).
    #[default]
    Visible,
    /// Back face is hidden.
    Hidden,
}

/// CSS transform functions — translate/scale/rotate/skew/skewX/skewY/matrix
/// and all 3D variants (CSS Transforms L2).
#[derive(Debug, Clone, PartialEq)]
pub enum TransformFn {
    Translate(f32, f32),
    TranslateX(f32),
    TranslateY(f32),
    /// `translateZ(<length>)` — translate along Z axis in px.
    TranslateZ(f32),
    /// `translate3d(<tx>, <ty>, <tz>)` — all three axes in px.
    Translate3d(f32, f32, f32),
    /// Угол в радианах (нормализован парсером из deg/rad/turn/grad).
    Rotate(f32),
    /// `rotateX(<angle>)` — angle in radians.
    RotateX(f32),
    /// `rotateY(<angle>)` — angle in radians.
    RotateY(f32),
    /// `rotateZ(<angle>)` — alias for 2D rotate, angle in radians.
    RotateZ(f32),
    /// `rotate3d(<x>, <y>, <z>, <angle>)` — arbitrary axis rotation.
    Rotate3d(f32, f32, f32, f32),
    Scale(f32, f32),
    ScaleX(f32),
    ScaleY(f32),
    /// `scaleZ(<s>)` — scale along Z axis.
    ScaleZ(f32),
    /// `scale3d(<sx>, <sy>, <sz>)` — all three axes.
    Scale3d(f32, f32, f32),
    SkewX(f32),
    SkewY(f32),
    Matrix([f32; 6]),
    /// `matrix3d(<16 values>)` — column-major 4×4 matrix.
    Matrix3d([f32; 16]),
    /// `perspective(<length>)` — perspective distance in px (> 0).
    Perspective(f32),
}

/// CSS Filter Effects L1 §3 — функции `filter`. Phase 0 поддерживает
/// все 9 стандартных функций кроме `drop-shadow` (требует rendering
/// pass — отложено).
#[derive(Debug, Clone, PartialEq)]
pub enum FilterFn {
    /// `blur(<length>)` — радиус gaussian blur.
    Blur(f32),
    /// `brightness(<number-percentage>)`. 1.0 = unchanged.
    Brightness(f32),
    /// `contrast(<number-percentage>)`. 1.0 = unchanged.
    Contrast(f32),
    /// `grayscale(<number-percentage>)`. 0.0 = unchanged, 1.0 = full grayscale.
    Grayscale(f32),
    /// `hue-rotate(<angle>)` — угол в радианах.
    HueRotate(f32),
    /// `invert(<number-percentage>)`. 0.0 = unchanged, 1.0 = inverted.
    Invert(f32),
    /// `opacity(<number-percentage>)`. 1.0 = unchanged.
    Opacity(f32),
    /// `saturate(<number-percentage>)`. 1.0 = unchanged.
    Saturate(f32),
    /// `sepia(<number-percentage>)`. 0.0 = unchanged, 1.0 = full sepia.
    Sepia(f32),
}

/// CSS Images L3 §3.4 — единичный `<color-stop>` градиента.
///
/// `position == None` означает auto-распределение: при resolve до used-value
/// auto-stops равномерно разносятся между фиксированными соседями (spec §3.4.3
/// "Color stop processing"). Здесь типизация специфицированного значения —
/// auto хранится как `None`, без раскрытия.
///
/// Только цвет и позиция (length / percentage). Hint-stops (`<color-stop>,
/// <length-percentage>, <color-stop>`) — без позиции цвета, чисто
/// midpoint-маркер — пока не моделируем: они отрабатывают на интерполяции
/// между соседями и не имеют animation-смысла на уровне per-stop pair.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GradientStop {
    pub color: Color,
    /// Source color space of this stop. `Srgb` for legacy `<color>` values;
    /// `DisplayP3` / `Rec2020` when the stop was written as `color(display-p3 …)`
    /// / `color(rec2020 …)`. Carried through the display list so the renderer
    /// can apply the correct output transform (ph3-color-management Step 3).
    pub color_space: ColorSpace,
    pub position: Option<Length>,
}

/// CSS Masking L1 §6.4 — `mask-mode`. Selects which channel of the mask image
/// is used as the per-pixel mask value when compositing the masked element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskMode {
    /// Use the alpha channel of the mask image directly. Initial behaviour for
    /// `<image>` mask sources (`match-source` resolves here for gradients/URLs).
    #[default]
    Alpha,
    /// Use luminance (`0.2126·R + 0.7152·G + 0.0722·B`, sRGB) multiplied by the
    /// source alpha as the mask value. A dark mask pixel hides the element even
    /// when fully opaque.
    Luminance,
}

/// CSS Masking L1 §4.7 — `mask-composite`. Determines how a mask layer is
/// combined with the mask already assembled from the layers **below** it
/// (Porter-Duff on the mask channel: `add` = source-over, `subtract` =
/// source-out, `intersect` = source-in, `exclude` = xor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaskComposite {
    /// `add` (initial) — Porter-Duff source-over on the mask channel.
    #[default]
    Add,
    /// `subtract` — Porter-Duff source-out: the layer below is removed where
    /// this layer paints.
    Subtract,
    /// `intersect` — Porter-Duff source-in: only the overlap survives.
    Intersect,
    /// `exclude` — Porter-Duff xor: the overlap is removed.
    Exclude,
}

impl MaskComposite {
    /// Parses a single `mask-composite` keyword (CSS Masking L1 §4.7).
    /// Case-insensitive; returns `None` on an unrecognised keyword.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "add" => Some(Self::Add),
            "subtract" => Some(Self::Subtract),
            "intersect" => Some(Self::Intersect),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }
}

/// CSS Masking L1 §4.9 — один слой маски.
///
/// `mask-image` задаёт количество слоёв; остальные longhand-ы циклически
/// повторяются по этому количеству («Layering Multiple Mask Layers»). Тот же
/// приём, что и у [`BackgroundLayer`], поэтому типы значений переиспользуются
/// из background (`mask-repeat` / `mask-size` / `mask-position` имеют ту же
/// грамматику, `mask-clip` — надмножество `background-clip`).
///
/// Порядок в [`ComputedStyle::mask_layers`]: первый = верхний слой. Слои
/// собираются в одну маску снизу вверх, каждый — оператором своего
/// [`MaskLayer::composite`].
#[derive(Debug, Clone, PartialEq)]
pub struct MaskLayer {
    /// `mask-image` этого слоя. `BackgroundImage` переиспользуется как тип
    /// (та же структура: None / Url / Gradient).
    pub image: BackgroundImage,
    /// `mask-repeat` этого слоя (§4.3). Initial `repeat`.
    pub repeat: BackgroundRepeat,
    /// `mask-size` этого слоя (§4.2). Initial `auto`.
    pub size: BackgroundSize,
    /// `mask-position` этого слоя (§4.4). Initial `center`.
    pub position: ObjectPosition,
    /// `mask-origin` этого слоя (§4.5). Initial `border-box`.
    pub origin: BackgroundOrigin,
    /// `mask-clip` этого слоя (§4.6). Initial `border-box`.
    pub clip: MaskClip,
    /// `mask-mode` этого слоя (§6.4). Initial `match-source`, который для
    /// поддерживаемых `<image>`-источников резолвится в `alpha`.
    pub mode: MaskMode,
    /// `mask-composite` этого слоя (§4.7) — оператор смешивания с уже
    /// собранными слоями ниже. Initial `add`.
    pub composite: MaskComposite,
}

impl Default for MaskLayer {
    fn default() -> Self {
        Self {
            image: BackgroundImage::None,
            repeat: BackgroundRepeat::Repeat,
            size: BackgroundSize::Auto,
            position: ObjectPosition::default(),
            origin: BackgroundOrigin::BorderBox,
            clip: MaskClip::BorderBox,
            mode: MaskMode::Alpha,
            composite: MaskComposite::Add,
        }
    }
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
