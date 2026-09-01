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

use lumen_core::geom::Size;
use lumen_css_parser::Declaration;
// Вызывателей внутри производственного `style.rs` у этой четвёрки нет —
// только `style::tests::*` через цепочку `use super::*` (тот же приём, что у
// `NodeData`/`MathStyle` ниже), с уходом региона SPLIT-ST18: `Cell`/`HashMap`
// читают `style::tests::{restyle,anchor_positioning_tests}`,
// `NodeId`/`Stylesheet` — `style::tests::{node_fanout_tests,shadow_dom_selectors}`.
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use lumen_css_parser::Stylesheet;
#[cfg(test)]
use lumen_dom::NodeId;

// Разбор CSS-значений, применение значений и табличные справочники, вынесенные
// из этого файла батчами SPLIT-ST3…ST10 (docs/tasks/p1-monolith-split-queue.md §4).
// `matching` — матчинг селекторов (SPLIT-ST12): `matches_complex`/`matches_slotted_complex`
// вызываются отсюда, поэтому реэкспорт/импорт ниже, в блоке ST-12.
mod adjust;
mod apply;
mod calc;
mod cascade;
mod cascade_index;
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
// SPLIT-ST18. Кэш индекса каскада и его BUG-341 диагностика уехали в
// `style::cascade_index`. Восемь имён ниже — публичная поверхность крейта
// (`pub mod style` в `lib.rs`; `box_tree.rs`/`counters.rs`/
// `crates/shell/src/tests/*` зовут их по старому пути), реэкспорт обязателен
// даже там, где вызывателя внутри `style.rs` нет (правило §2.1).
pub use cascade_index::{
    add_cascade_index_stats, add_pseudo_cascade_sites, add_pseudo_cascade_stats,
    clear_rule_idx_cache, clear_shadow_sheets, set_pseudo_cascade_diagnostics, set_shadow_sheets,
    sheet_has_quote_content, sheet_targets_pseudo, take_cascade_index_stats,
    take_pseudo_cascade_sites, take_pseudo_cascade_stats, CascadeIndexStats, PseudoCascadeStats,
};
// Видны только сиблингам `style::cascade_index` — `style::cascade`/
// `style::pseudo`/`style::adjust`/`style::env`/`style::matching::forms` (не
// потомкам, иначе видимость была бы даром), поэтому сужено до
// `pub(in crate::style)`, приём SPLIT-ST3 (`encode_srgb`).
pub(in crate::style) use cascade_index::{
    ensure_cascade_index, note_pseudo_cascade, with_cascade_index, with_front_cascade_index,
    PSEUDO_STATS_ON, SHADOW_HOST_SCOPE, SHADOW_SHEETS,
};
// `reset_pseudo_base_builds`/`pseudo_base_builds` были `pub(crate)` в доноре —
// реэкспорт сохраняет ту же видимость. Вызывателя внутри производственного
// `style.rs` у этой пятёрки и `CASCADE_INDEX_SLOTS`/
// `reset_scrollbar_pseudo_cascades`/`scrollbar_pseudo_cascades`/
// `SCROLLBAR_PSEUDO_CASCADES` нет — только `style::tests::restyle` через
// цепочку `use super::*` и (двух статиков) `style::adjust`/`style::pseudo`
// напрямую, отсюда `#[cfg(test)]`-импорт прямо здесь (приём SPLIT-ST3b/ST9
// «пользуются только тестами»). `all_rules` вызывателя нигде вне
// `cascade_index.rs` не имеет (единственное внешнее упоминание — комментарий
// в `restyle.rs`, ловушка «grep считает комментарии», SPLIT-ST3) — реэкспорта
// не получила и осталась приватной.
#[cfg(test)]
pub(crate) use cascade_index::{pseudo_base_builds, reset_pseudo_base_builds};
#[cfg(test)]
pub(in crate::style) use cascade_index::{
    reset_scrollbar_pseudo_cascades, scrollbar_pseudo_cascades, CASCADE_INDEX_SLOTS,
    PSEUDO_BASE_BUILDS, SCROLLBAR_PSEUDO_CASCADES,
};
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
pub use parse::color::{canonical_specified_color, parse_color, system_color};
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
    StrokeLinecap, StrokeLinejoin, SvgGradientDef, SvgGradientUnits, SvgPaint, SvgPaintOrder,
    VerticalAlign,
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
// `media_context_from_viewport` перестала быть нужна и производственному
// `style.rs`, и его реэкспорту — `style::cascade_index` (SPLIT-ST18) зовёт её
// напрямую по пути `crate::style::env::media_context_from_viewport`, минуя это
// место; здесь остался только `node_in_scope` (нужен `style::cascade`).
use env::node_in_scope;
// `media_context_from_viewport` перестала быть нужна производственному
// `style.rs` вместе с регионом SPLIT-ST18, но её по-прежнему читает
// `style::tests` (`mod.rs`) через `use super::*` — та же ловушка SH-3a.
#[cfg(test)]
use env::media_context_from_viewport;

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
