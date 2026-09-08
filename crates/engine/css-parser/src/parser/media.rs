//! Media Queries L4: типы `@media`-запросов, [`MediaContext`] и разбор
//! media-query-list, media-условий и media-фич.
//!
//! Вырезано из `parser.rs` (SPLIT-CP1 срез 2/2) без изменения поведения.

// Долг по документации: код перенесён из `parser.rs` как есть; файл
// написан до включения `missing_docs`. Счётчики — docs/lint-policy.md §10.
#![allow(missing_docs)]

use super::*;

/// Группа CSS-правил, вложенных в `@media`-блок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRule {
    pub query: MediaQuery,
    pub rules: Vec<Rule>,
}

/// Media query — OR-список AND-clauses (Media Queries L4 §3). Пустой
/// `clauses` (нет условий) трактуется как «всегда true» (= `@media all`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaQuery {
    /// Comma-separated OR-список. При пустом `clauses` query всегда
    /// матчит (`@media all`).
    pub clauses: Vec<MediaQueryClause>,
    /// Исходный текст между `@media`/`@import`-URL и `{`/`;`, до trim.
    /// Заполняется [`parse_media_query`]. Источник `CSSMediaRule.media.
    /// mediaText`/`conditionText` (CSSOM View §6.2) — сериализация из
    /// структурного `clauses` не нужна, раз есть точный исходный текст.
    pub raw: String,
}

/// Одна clause в media query — AND-список feature/media-type условий
/// с опциональным `not`-модификатором.
///
/// Media Queries L4 §3.2: `not <media-query>` инвертирует результат
/// _всей_ clause. `only <media-type>` — L3-совместимый no-op-модификатор
/// (использовался для скрытия media-query от старых браузеров, для
/// современных парсеров значимого эффекта не несёт).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaQueryClause {
    /// Истина для `not screen and (min-width: 600px)` — инвертирует
    /// итоговый результат clause целиком. Per §3.2 unknown-условия
    /// внутри negated clause не дают `true`: clause с любым
    /// `Unsupported` оценивается как unknown и не матчит.
    pub negated: bool,
    /// Истина для `only screen and (...)`. L3-совместимый no-op-модификатор
    /// (не влияет на [`MediaQueryClause::matches`]) — хранится только ради
    /// точной сериализации ([`MediaQueryClause::serialize`]).
    pub only: bool,
    /// AND-list. Пустой — clause-error (например, `not` без feature),
    /// `matches()` отдаст `false`.
    pub conditions: Vec<MediaCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaCondition {
    /// `screen`, `print`, `all`, `handheld`, etc. — media type.
    /// Хранится lower-case. `all` всегда match. Прочие имена match
    /// если совпадают с `MediaContext::media_type` (lower-case).
    MediaType(String),
    /// `(min-width: 600px)` и подобные. Phase 0 поддерживает:
    /// min/max-width, min/max-height, orientation, prefers-color-scheme.
    Feature(MediaFeature),
    /// Любая `(unknown-feature: value)` — никогда не матчит (forward-compat).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    // Viewport dimensions — exact and range
    Width(f32),
    MinWidth(f32),
    MaxWidth(f32),
    Height(f32),
    MinHeight(f32),
    MaxHeight(f32),
    // Aspect ratio: numerator/denominator kept separate (not pre-divided)
    // так, чтобы serialize() мог отдать `1 / 3`, а не отгаданную из float
    // дробь — см. Media Queries L4 aspect-ratio-serialization.html.
    AspectRatio(f32, f32),
    MinAspectRatio(f32, f32),
    MaxAspectRatio(f32, f32),
    // Display
    Orientation(MediaOrientation),
    // Resolution (CSS Values L4 `<resolution>`, canonical unit `dppx`) —
    // `x`/`dppx`/`dpi`/`dpcm` units plus a minimal `calc()` (BUG-1019).
    /// `(resolution: <resolution>)` — exact device resolution match.
    Resolution(ResolutionValue),
    /// `(min-resolution: <resolution>)`.
    MinResolution(ResolutionValue),
    /// `(max-resolution: <resolution>)`.
    MaxResolution(ResolutionValue),
    // User preferences (MQ L5, commonly used)
    PrefersColorScheme(ColorScheme),
    PrefersReducedMotion(bool),
    // CSS Forced Colors Mode (Forced Colors L1) — опубликована (active/none)
    ForcedColors(bool),
    // Interaction media features (Media Queries L4 §5.3-5.6)
    /// `(hover: none | hover)` — hover-способность основного указателя.
    Hover(MediaHover),
    /// `(any-hover: none | hover)` — hover-способность любого указателя.
    AnyHover(MediaHover),
    /// `(pointer: none | coarse | fine)` — точность основного указателя.
    Pointer(MediaPointer),
    /// `(any-pointer: none | coarse | fine)` — точность любого указателя.
    AnyPointer(MediaPointer),
    // User-preference media features (Media Queries L5 §5.5/§5.6)
    /// `(prefers-contrast: no-preference | more | less | custom)` —
    /// предпочтение пользователя по контрастности интерфейса.
    PrefersContrast(MediaContrast),
    /// `(prefers-reduced-data: no-preference | reduce)` —
    /// предпочтение пользователя по экономии сетевого трафика.
    PrefersReducedData(MediaReducedData),
    /// `(prefers-reduced-transparency: no-preference | reduce)` —
    /// предпочтение пользователя по уменьшению полупрозрачности UI
    /// (Media Queries L5 §5.7).
    PrefersReducedTransparency(MediaReducedTransparency),
    /// `(scripting: none | initial-only | enabled)` — доступность скриптов
    /// при рендеринге документа (Media Queries L5 §6.2).
    Scripting(MediaScripting),
    /// `(inverted-colors: none | inverted)` — инвертирует ли ОС/UA выводимые
    /// цвета (например, режим «инверсия цветов» доступности) (Media Queries
    /// L5 §5.8).
    InvertedColors(MediaInvertedColors),
    /// `(color)` — boolean-context форма (CSS Values L4 §Boolean Context):
    /// «true, если фича поддерживается устройством и её значение не равно
    /// нулю». У Lumen цветовая глубина фиксирована, поэтому она всегда
    /// «поддерживается». Общий boolean-context механизм для range-фич
    /// (`width`/`resolution`/…) — их собственная, более широкая grammar
    /// (сравнение `<`/`<=`/`>`/`>=`), не затронут этой правкой (BUG-527
    /// закрывает только discrete-фичи — см. `parse_media_feature`).
    Color,
    /// `(display-mode: standalone | fullscreen | minimal-ui |
    /// picture-in-picture | browser)` (Media Queries L5 §6.4). У Lumen нет
    /// PWA-режима отображения — всегда `browser`.
    DisplayMode(MediaDisplayMode),
    /// `(display-state: normal | fullscreen | maximized | minimized)`
    /// (tentative, additional-windowing-controls explainer).
    DisplayState(MediaDisplayState),
    /// `(resizable: true | false)` (tentative, additional-windowing-controls
    /// explainer) — может ли пользователь менять размер окна вывода.
    Resizable(bool),
    /// `(dynamic-range: standard | high)` (Media Queries L5 §6.5).
    DynamicRange(MediaDynamicRange),
    /// `(video-dynamic-range: standard | high)` (Media Queries L5 §6.6).
    VideoDynamicRange(MediaDynamicRange),
    /// `(update: none | slow | fast)` (Media Queries L4 §6.1) — как часто
    /// окружение способно отражать изменения после первичного рендера.
    Update(MediaUpdate),
    /// `(navigation-controls: none | back-button)` (tentative,
    /// backbutton-mediaquery explainer).
    NavigationControls(MediaNavigationControls),
    /// `(overflow-inline: none | scroll)` (Media Queries L4 §6.2).
    OverflowInline(MediaOverflowInline),
    /// `(overflow-block: none | scroll | paged)` (Media Queries L4 §6.3).
    OverflowBlock(MediaOverflowBlock),
    /// Boolean-context form (bare `(feature)`, no `: value`) of a discrete
    /// feature whose value form is one of the arms above. See
    /// [`BooleanFeature`] — kept as its own small enum rather than one
    /// variant per feature here, since its `matches()` re-derives the
    /// answer from `MediaContext` directly instead of carrying a parsed
    /// value literal.
    BooleanContext(BooleanFeature),
}

/// Which already-implemented discrete feature a bare `(feature)` boolean
/// context (CSS Values L4 §Boolean Context) refers to. Per Media Queries L4
/// §4.1 the bare form evaluates true unless the feature's current value is
/// its defined off/no-preference/normal keyword; a few features below have
/// no such keyword at all (`orientation`, `prefers-color-scheme`,
/// `display-mode`, `display-state`) and are unconditionally true once
/// recognized. Range features (`width`/`resolution`/`aspect-ratio`/…) are
/// out of scope — see the note on [`MediaFeature::Color`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanFeature {
    Scripting,
    PrefersColorScheme,
    ForcedColors,
    InvertedColors,
    PrefersReducedData,
    PrefersContrast,
    PrefersReducedMotion,
    PrefersReducedTransparency,
    Orientation,
    Hover,
    AnyHover,
    Pointer,
    AnyPointer,
    DisplayMode,
    DisplayState,
    Resizable,
    DynamicRange,
    VideoDynamicRange,
    Update,
    NavigationControls,
    OverflowInline,
    OverflowBlock,
}

impl BooleanFeature {
    fn matches(self, ctx: &MediaContext) -> bool {
        match self {
            Self::Scripting => ctx.scripting != MediaScripting::None,
            Self::PrefersColorScheme | Self::Orientation | Self::DisplayMode | Self::DisplayState => true,
            Self::ForcedColors => ctx.forced_colors,
            Self::InvertedColors => ctx.inverted_colors == MediaInvertedColors::Inverted,
            Self::PrefersReducedData => ctx.prefers_reduced_data == MediaReducedData::Reduce,
            Self::PrefersContrast => ctx.prefers_contrast != MediaContrast::NoPreference,
            Self::PrefersReducedMotion => ctx.prefers_reduced_motion,
            Self::PrefersReducedTransparency => {
                ctx.prefers_reduced_transparency == MediaReducedTransparency::Reduce
            }
            Self::Hover => ctx.hover != MediaHover::None,
            Self::AnyHover => ctx.any_hover != MediaHover::None,
            Self::Pointer => ctx.pointer != MediaPointer::None,
            Self::AnyPointer => ctx.any_pointer != MediaPointer::None,
            Self::Resizable => ctx.resizable,
            // Per spec (confirmed by `dynamic-range.html`): the boolean form
            // tests whether HDR is actually available, not merely whether
            // the feature is recognized — `standard` is the "off" state
            // here, same status as `none`/`no-preference` elsewhere.
            Self::DynamicRange => ctx.dynamic_range == MediaDynamicRange::High,
            Self::VideoDynamicRange => ctx.video_dynamic_range == MediaDynamicRange::High,
            Self::Update => ctx.update != MediaUpdate::None,
            Self::NavigationControls => ctx.navigation_controls != MediaNavigationControls::None,
            Self::OverflowInline => ctx.overflow_inline != MediaOverflowInline::None,
            Self::OverflowBlock => ctx.overflow_block != MediaOverflowBlock::None,
        }
    }

    const fn feature_name(self) -> &'static str {
        match self {
            Self::Scripting => "scripting",
            Self::PrefersColorScheme => "prefers-color-scheme",
            Self::ForcedColors => "forced-colors",
            Self::InvertedColors => "inverted-colors",
            Self::PrefersReducedData => "prefers-reduced-data",
            Self::PrefersContrast => "prefers-contrast",
            Self::PrefersReducedMotion => "prefers-reduced-motion",
            Self::PrefersReducedTransparency => "prefers-reduced-transparency",
            Self::Orientation => "orientation",
            Self::Hover => "hover",
            Self::AnyHover => "any-hover",
            Self::Pointer => "pointer",
            Self::AnyPointer => "any-pointer",
            Self::DisplayMode => "display-mode",
            Self::DisplayState => "display-state",
            Self::Resizable => "resizable",
            Self::DynamicRange => "dynamic-range",
            Self::VideoDynamicRange => "video-dynamic-range",
            Self::Update => "update",
            Self::NavigationControls => "navigation-controls",
            Self::OverflowInline => "overflow-inline",
            Self::OverflowBlock => "overflow-block",
        }
    }

    /// Parses a bare feature name (already trimmed/lower-cased) into its
    /// boolean-context representation. `None` for anything not in this
    /// list — either an unimplemented feature or a range feature (`color`
    /// is handled separately by the caller, `MediaFeature::Color`).
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "scripting" => Self::Scripting,
            "prefers-color-scheme" => Self::PrefersColorScheme,
            "forced-colors" => Self::ForcedColors,
            "inverted-colors" => Self::InvertedColors,
            "prefers-reduced-data" => Self::PrefersReducedData,
            "prefers-contrast" => Self::PrefersContrast,
            "prefers-reduced-motion" => Self::PrefersReducedMotion,
            "prefers-reduced-transparency" => Self::PrefersReducedTransparency,
            "orientation" => Self::Orientation,
            "hover" => Self::Hover,
            "any-hover" => Self::AnyHover,
            "pointer" => Self::Pointer,
            "any-pointer" => Self::AnyPointer,
            "display-mode" => Self::DisplayMode,
            "display-state" => Self::DisplayState,
            "resizable" => Self::Resizable,
            "dynamic-range" => Self::DynamicRange,
            "video-dynamic-range" => Self::VideoDynamicRange,
            "update" => Self::Update,
            "navigation-controls" => Self::NavigationControls,
            "overflow-inline" => Self::OverflowInline,
            "overflow-block" => Self::OverflowBlock,
            _ => return None,
        })
    }
}

/// Media Queries L5 §6.4 — `display-mode`: режим отображения top-level
/// browsing context (обычное окно браузера / установленное PWA-окно / …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaDisplayMode {
    /// Обычная вкладка/окно браузера — desktop-дефолт Lumen (нет PWA-режима).
    Browser,
    Standalone,
    MinimalUi,
    Fullscreen,
    PictureInPicture,
}

/// `display-state` (tentative) — состояние top-level окна вывода.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaDisplayState {
    /// Обычное, не свёрнутое/развёрнутое/полноэкранное окно.
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
}

/// Media Queries L5 §6.5/§6.6 — уровень динамического диапазона,
/// поддерживаемый устройством вывода (`dynamic-range`) или видео-плоскостью
/// (`video-dynamic-range`). Lumen не поддерживает HDR — всегда `Standard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaDynamicRange {
    Standard,
    High,
}

/// Media Queries L4 §6.1 — `update`: как часто окружение способно отражать
/// изменения контента после первичного рендера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaUpdate {
    /// Изменения не отражаются вовсе (статический снимок — печать/PDF).
    None,
    /// Изменения отражаются, но медленно/дорого (e-ink-подобные устройства).
    Slow,
    /// Изменения отражаются быстро — desktop-дефолт Lumen для непечатного
    /// документа (per spec note: «update» практически всегда `fast` вне
    /// печати).
    Fast,
}

/// `navigation-controls` (tentative) — какие встроенные средства навигации
/// «назад» доступны пользователю в UA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaNavigationControls {
    None,
    /// Есть кнопка «назад» — desktop-дефолт Lumen (вкладка с историей).
    BackButton,
}

/// Media Queries L4 §6.2 — `overflow-inline`: как окружение обрабатывает
/// содержимое, переполняющее viewport вдоль inline-оси.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOverflowInline {
    None,
    /// Скроллится — desktop-дефолт Lumen для непечатного документа.
    Scroll,
}

/// Media Queries L4 §6.3 — `overflow-block`: как окружение обрабатывает
/// содержимое, переполняющее viewport вдоль block-оси.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOverflowBlock {
    None,
    /// Скроллится — desktop-дефолт Lumen для непечатного документа.
    Scroll,
    /// Разбивается на страницы (печать/PDF).
    Paged,
}

impl Eq for MediaFeature {}

/// A resolved `<resolution>` value (canonical unit `dppx`), plus whether the
/// source used `calc(...)` — Media Queries L4's serialization keeps the
/// `calc(...)` wrapper even once the expression collapses to one number
/// (`match-media-parsing.html::test_resolution_parsing`: `(resolution:
/// calc(1x))` serializes as `(resolution: calc(1dppx))`, not `(resolution:
/// 1dppx)`), so a bare `f32` would lose that distinction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolutionValue {
    /// Written directly, e.g. `2dppx`/`600dpi`.
    Literal(f32),
    /// Written as `calc(...)`; the `f32` is the already-simplified `dppx`
    /// result.
    Calc(f32),
}

impl Eq for ResolutionValue {}

impl ResolutionValue {
    /// The canonical `dppx` amount, regardless of source form.
    pub(crate) fn dppx(self) -> f32 {
        match self {
            Self::Literal(v) | Self::Calc(v) => v,
        }
    }

    fn serialize(self) -> String {
        match self {
            Self::Literal(v) => format!("{v}dppx"),
            Self::Calc(v) => format!("calc({v}dppx)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaOrientation {
    Portrait,
    Landscape,
}

/// Media Queries L4 §5.3/§5.5 — hover-способность указателя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaHover {
    /// Указатель не может наводиться без активации (тач-экран).
    None,
    /// Указатель может удобно наводиться (мышь).
    Hover,
}

/// Media Queries L4 §5.4/§5.6 — точность указателя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPointer {
    /// Указывающего устройства нет.
    None,
    /// Грубый указатель (палец на тач-экране).
    Coarse,
    /// Точный указатель (мышь, стилус).
    Fine,
}

/// Media Queries L5 §5.5 — `prefers-contrast`: запрошенный пользователем
/// уровень контрастности интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaContrast {
    /// Пользователь не выразил предпочтения (значение по умолчанию).
    NoPreference,
    /// Пользователь запросил больший контраст.
    More,
    /// Пользователь запросил меньший контраст.
    Less,
    /// Активирована пользовательская цветовая схема (forced colors и т.п.).
    Custom,
}

/// Media Queries L5 §5.6 — `prefers-reduced-data`: запрос на экономию
/// сетевого трафика.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaReducedData {
    /// Пользователь не выразил предпочтения (значение по умолчанию).
    NoPreference,
    /// Пользователь запросил режим экономии трафика.
    Reduce,
}

/// Media Queries L5 §5.7 — `prefers-reduced-transparency`: запрос на
/// уменьшение полупрозрачных/blur-эффектов в интерфейсе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaReducedTransparency {
    /// Пользователь не выразил предпочтения (значение по умолчанию).
    NoPreference,
    /// Пользователь запросил уменьшение полупрозрачности.
    Reduce,
}

/// Media Queries L5 §6.2 — `scripting`: доступность JavaScript в текущем
/// окружении рендеринга.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaScripting {
    /// Скрипты полностью недоступны (например, отключены пользователем).
    None,
    /// Скрипты исполняются только при первичной загрузке, но не далее
    /// (например, статический снимок страницы для печати).
    InitialOnly,
    /// Скрипты доступны и исполняются на протяжении всей жизни документа.
    Enabled,
}

/// Media Queries L5 §5.8 — `inverted-colors`: инвертирует ли пользовательское
/// окружение (ОС/UA) выводимые цвета.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaInvertedColors {
    /// Цвета выводятся как есть (значение по умолчанию).
    None,
    /// Цвета инвертируются окружением.
    Inverted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    Light,
    Dark,
}

/// Контекст, против которого матчатся media queries. Заполняется
/// shell-ом / layout-ом из текущего viewport-а и пользовательских
/// настроек.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaContext {
    /// «screen» / «print» / «all» / прочее.
    pub media_type: String,
    pub width: f32,
    pub height: f32,
    /// Device resolution in `dppx` (`resolution`/`min-resolution`/
    /// `max-resolution`). No dynamic per-window scale-factor plumbing exists
    /// yet anywhere in the engine — `window.devicePixelRatio` itself is a
    /// hardcoded `1` (`crates/js/src/window_management.rs`) outside the
    /// multi-window API — so `1.0` here matches that same desktop default
    /// rather than adding a second, disagreeing source of truth (BUG-1019).
    pub resolution_dppx: f32,
    pub prefers_dark: bool,
    /// Соответствует `prefers-reduced-motion: reduce`.
    pub prefers_reduced_motion: bool,
    /// CSS Forced Colors: соответствует `(forced-colors: active)` media feature.
    pub forced_colors: bool,
    /// hover-способность основного указателя (`hover` media feature).
    pub hover: MediaHover,
    /// hover-способность любого указателя (`any-hover` media feature).
    pub any_hover: MediaHover,
    /// Точность основного указателя (`pointer` media feature).
    pub pointer: MediaPointer,
    /// Точность любого указателя (`any-pointer` media feature).
    pub any_pointer: MediaPointer,
    /// Предпочтение контрастности (`prefers-contrast` media feature).
    pub prefers_contrast: MediaContrast,
    /// Предпочтение экономии трафика (`prefers-reduced-data` media feature).
    pub prefers_reduced_data: MediaReducedData,
    /// Предпочтение уменьшения полупрозрачности
    /// (`prefers-reduced-transparency` media feature).
    pub prefers_reduced_transparency: MediaReducedTransparency,
    /// Доступность скриптов (`scripting` media feature). У Lumen есть
    /// встроенный JS-движок (QuickJS), поэтому desktop-дефолт — `Enabled`.
    pub scripting: MediaScripting,
    /// Инверсия цветов окружением (`inverted-colors` media feature).
    pub inverted_colors: MediaInvertedColors,
    /// Режим отображения top-level browsing context (`display-mode`).
    pub display_mode: MediaDisplayMode,
    /// Состояние top-level окна вывода (`display-state`).
    pub display_state: MediaDisplayState,
    /// Может ли пользователь менять размер окна вывода (`resizable`).
    pub resizable: bool,
    /// Динамический диапазон устройства вывода (`dynamic-range`).
    pub dynamic_range: MediaDynamicRange,
    /// Динамический диапазон видео-плоскости (`video-dynamic-range`).
    pub video_dynamic_range: MediaDynamicRange,
    /// Частота отражения изменений после первичного рендера (`update`).
    pub update: MediaUpdate,
    /// Доступные средства навигации «назад» (`navigation-controls`).
    pub navigation_controls: MediaNavigationControls,
    /// Обработка переполнения вдоль inline-оси (`overflow-inline`).
    pub overflow_inline: MediaOverflowInline,
    /// Обработка переполнения вдоль block-оси (`overflow-block`).
    pub overflow_block: MediaOverflowBlock,
}

impl Default for MediaContext {
    fn default() -> Self {
        // Desktop-дефолты: есть мышь → hover-способность и точный указатель.
        Self {
            media_type: "screen".into(),
            width: 0.0,
            height: 0.0,
            resolution_dppx: 1.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            forced_colors: false,
            hover: MediaHover::Hover,
            any_hover: MediaHover::Hover,
            pointer: MediaPointer::Fine,
            any_pointer: MediaPointer::Fine,
            // Desktop-дефолты: пользователь не запрашивал особый контраст
            // или экономию трафика.
            prefers_contrast: MediaContrast::NoPreference,
            prefers_reduced_data: MediaReducedData::NoPreference,
            prefers_reduced_transparency: MediaReducedTransparency::NoPreference,
            // Lumen исполняет JS (QuickJS) → скрипты включены, как в Edge.
            scripting: MediaScripting::Enabled,
            // Desktop-дефолт: ОС не инвертирует цвета.
            inverted_colors: MediaInvertedColors::None,
            // Desktop-дефолты: обычная вкладка браузера, не PWA/e-ink/print.
            display_mode: MediaDisplayMode::Browser,
            display_state: MediaDisplayState::Normal,
            resizable: true,
            dynamic_range: MediaDynamicRange::Standard,
            video_dynamic_range: MediaDynamicRange::Standard,
            update: MediaUpdate::Fast,
            navigation_controls: MediaNavigationControls::BackButton,
            overflow_inline: MediaOverflowInline::Scroll,
            overflow_block: MediaOverflowBlock::Scroll,
        }
    }
}

impl MediaQuery {
    /// Пустой query (= `@media all`) — true. Иначе хотя бы одна
    /// OR-clause должна быть истиной; внутри clause — все AND-условия.
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        self.clauses.iter().any(|clause| clause.matches(ctx))
    }
}

impl MediaQueryClause {
    /// Per Media Queries L4 §3.2: пустая `conditions` — clause invalid
    /// (например, `@media not` без media-type / feature) → false.
    /// `Unsupported` в любом условии делает clause «unknown» → false
    /// даже под `not` (spec: «If the result is unknown, then the
    /// negation also evaluates to unknown»). При known-результате
    /// `negated` инвертирует исход AND-conjunction.
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        if self.conditions.is_empty() {
            return false;
        }
        if self
            .conditions
            .iter()
            .any(|c| matches!(c, MediaCondition::Unsupported))
        {
            return false;
        }
        let all_match = self.conditions.iter().all(|c| c.matches(ctx));
        if self.negated { !all_match } else { all_match }
    }
}

impl MediaCondition {
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        match self {
            Self::MediaType(t) => t == "all" || t == &ctx.media_type,
            Self::Feature(f) => f.matches(ctx),
            Self::Unsupported => false,
        }
    }
}

impl MediaFeature {
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        match self {
            Self::Width(px) => (ctx.width - px).abs() < 0.5,
            Self::MinWidth(px) => ctx.width >= *px,
            Self::MaxWidth(px) => ctx.width <= *px,
            Self::Height(px) => (ctx.height - px).abs() < 0.5,
            Self::MinHeight(px) => ctx.height >= *px,
            Self::MaxHeight(px) => ctx.height <= *px,
            Self::AspectRatio(n, d) => {
                let ratio = n / d;
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { f32::INFINITY };
                (actual - ratio).abs() < 0.01
            }
            Self::MinAspectRatio(n, d) => {
                let ratio = n / d;
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { f32::INFINITY };
                actual >= ratio
            }
            Self::MaxAspectRatio(n, d) => {
                let ratio = n / d;
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { 0.0 };
                actual <= ratio
            }
            Self::Orientation(o) => {
                let actual = if ctx.width >= ctx.height {
                    MediaOrientation::Landscape
                } else {
                    MediaOrientation::Portrait
                };
                actual == *o
            }
            Self::Resolution(v) => (ctx.resolution_dppx - v.dppx()).abs() < 0.001,
            Self::MinResolution(v) => ctx.resolution_dppx >= v.dppx(),
            Self::MaxResolution(v) => ctx.resolution_dppx <= v.dppx(),
            Self::PrefersColorScheme(scheme) => match scheme {
                ColorScheme::Dark => ctx.prefers_dark,
                ColorScheme::Light => !ctx.prefers_dark,
            },
            Self::PrefersReducedMotion(reduce) => ctx.prefers_reduced_motion == *reduce,
            Self::ForcedColors(active) => ctx.forced_colors == *active,
            Self::Hover(h) => ctx.hover == *h,
            Self::AnyHover(h) => ctx.any_hover == *h,
            Self::Pointer(p) => ctx.pointer == *p,
            Self::AnyPointer(p) => ctx.any_pointer == *p,
            Self::PrefersContrast(c) => ctx.prefers_contrast == *c,
            Self::PrefersReducedData(d) => ctx.prefers_reduced_data == *d,
            Self::PrefersReducedTransparency(t) => ctx.prefers_reduced_transparency == *t,
            Self::Scripting(s) => ctx.scripting == *s,
            Self::InvertedColors(i) => ctx.inverted_colors == *i,
            Self::Color => true,
            Self::DisplayMode(m) => ctx.display_mode == *m,
            Self::DisplayState(s) => ctx.display_state == *s,
            Self::Resizable(r) => ctx.resizable == *r,
            Self::DynamicRange(d) => ctx.dynamic_range == *d,
            Self::VideoDynamicRange(d) => ctx.video_dynamic_range == *d,
            Self::Update(u) => ctx.update == *u,
            Self::NavigationControls(n) => ctx.navigation_controls == *n,
            Self::OverflowInline(o) => ctx.overflow_inline == *o,
            Self::OverflowBlock(o) => ctx.overflow_block == *o,
            Self::BooleanContext(b) => b.matches(ctx),
        }
    }
}

impl MediaQuery {
    /// Media Queries L4 §Serializing a media query list: пустой список —
    /// пустая строка (`window.matchMedia('').media === ''`), иначе каждая
    /// comma-separated clause сериализуется независимо и join'ится `", "`
    /// (точное исходное разделение/пробелы не сохраняются — спека требует
    /// канонической формы, не эха входа, отсюда и сам баг [BUG-526]).
    pub fn serialize(&self) -> String {
        self.clauses
            .iter()
            .map(MediaQueryClause::serialize)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl MediaQueryClause {
    /// Клауза, которая не распозналась целиком (пустой `conditions`) или
    /// содержит хотя бы один `Unsupported` (неизвестная фича, синтаксическая
    /// ошибка внутри `()`, лишний `not`/`only`) сериализуется как литеральная
    /// `not all` — Media Queries L4 требует заменить каждый невалидный член
    /// списка этой строкой целиком, независимо от собственных `not`/`only`.
    pub fn serialize(&self) -> String {
        if self.conditions.is_empty()
            || self
                .conditions
                .iter()
                .any(|c| matches!(c, MediaCondition::Unsupported))
        {
            return "not all".to_string();
        }
        let body = self
            .conditions
            .iter()
            .map(MediaCondition::serialize)
            .collect::<Vec<_>>()
            .join(" and ");
        if self.negated {
            format!("not {body}")
        } else if self.only {
            format!("only {body}")
        } else {
            body
        }
    }
}

impl MediaCondition {
    fn serialize(&self) -> String {
        match self {
            Self::MediaType(t) => t.clone(),
            Self::Feature(f) => format!("({})", f.serialize()),
            // Клаузы с Unsupported перехватываются на уровне
            // MediaQueryClause::serialize раньше, чем мы сюда доходим.
            Self::Unsupported => "not all".to_string(),
        }
    }
}

impl MediaFeature {
    fn serialize(&self) -> String {
        match self {
            Self::Width(px) => format!("width: {px}px"),
            Self::MinWidth(px) => format!("min-width: {px}px"),
            Self::MaxWidth(px) => format!("max-width: {px}px"),
            Self::Height(px) => format!("height: {px}px"),
            Self::MinHeight(px) => format!("min-height: {px}px"),
            Self::MaxHeight(px) => format!("max-height: {px}px"),
            Self::AspectRatio(n, d) => format!("aspect-ratio: {n} / {d}"),
            Self::MinAspectRatio(n, d) => format!("min-aspect-ratio: {n} / {d}"),
            Self::MaxAspectRatio(n, d) => format!("max-aspect-ratio: {n} / {d}"),
            Self::Orientation(o) => format!(
                "orientation: {}",
                match o {
                    MediaOrientation::Portrait => "portrait",
                    MediaOrientation::Landscape => "landscape",
                }
            ),
            Self::Resolution(v) => format!("resolution: {}", v.serialize()),
            Self::MinResolution(v) => format!("min-resolution: {}", v.serialize()),
            Self::MaxResolution(v) => format!("max-resolution: {}", v.serialize()),
            Self::PrefersColorScheme(s) => format!(
                "prefers-color-scheme: {}",
                match s {
                    ColorScheme::Light => "light",
                    ColorScheme::Dark => "dark",
                }
            ),
            Self::PrefersReducedMotion(reduce) => format!(
                "prefers-reduced-motion: {}",
                if *reduce { "reduce" } else { "no-preference" }
            ),
            Self::ForcedColors(active) => format!(
                "forced-colors: {}",
                if *active { "active" } else { "none" }
            ),
            Self::Hover(h) => format!("hover: {}", hover_str(*h)),
            Self::AnyHover(h) => format!("any-hover: {}", hover_str(*h)),
            Self::Pointer(p) => format!("pointer: {}", pointer_str(*p)),
            Self::AnyPointer(p) => format!("any-pointer: {}", pointer_str(*p)),
            Self::PrefersContrast(c) => format!(
                "prefers-contrast: {}",
                match c {
                    MediaContrast::NoPreference => "no-preference",
                    MediaContrast::More => "more",
                    MediaContrast::Less => "less",
                    MediaContrast::Custom => "custom",
                }
            ),
            Self::PrefersReducedData(d) => format!(
                "prefers-reduced-data: {}",
                match d {
                    MediaReducedData::NoPreference => "no-preference",
                    MediaReducedData::Reduce => "reduce",
                }
            ),
            Self::PrefersReducedTransparency(t) => format!(
                "prefers-reduced-transparency: {}",
                match t {
                    MediaReducedTransparency::NoPreference => "no-preference",
                    MediaReducedTransparency::Reduce => "reduce",
                }
            ),
            Self::Scripting(s) => format!(
                "scripting: {}",
                match s {
                    MediaScripting::None => "none",
                    MediaScripting::InitialOnly => "initial-only",
                    MediaScripting::Enabled => "enabled",
                }
            ),
            Self::InvertedColors(i) => format!(
                "inverted-colors: {}",
                match i {
                    MediaInvertedColors::None => "none",
                    MediaInvertedColors::Inverted => "inverted",
                }
            ),
            // Boolean context — no `: value` part, just the feature name
            // (`(color)`, not `(color: true)`).
            Self::Color => "color".to_string(),
            Self::DisplayMode(m) => format!(
                "display-mode: {}",
                match m {
                    MediaDisplayMode::Standalone => "standalone",
                    MediaDisplayMode::Browser => "browser",
                    MediaDisplayMode::MinimalUi => "minimal-ui",
                    MediaDisplayMode::Fullscreen => "fullscreen",
                    MediaDisplayMode::PictureInPicture => "picture-in-picture",
                }
            ),
            Self::DisplayState(s) => format!(
                "display-state: {}",
                match s {
                    MediaDisplayState::Normal => "normal",
                    MediaDisplayState::Minimized => "minimized",
                    MediaDisplayState::Maximized => "maximized",
                    MediaDisplayState::Fullscreen => "fullscreen",
                }
            ),
            Self::Resizable(r) => format!("resizable: {}", if *r { "true" } else { "false" }),
            Self::DynamicRange(d) => format!("dynamic-range: {}", dynamic_range_str(*d)),
            Self::VideoDynamicRange(d) => format!("video-dynamic-range: {}", dynamic_range_str(*d)),
            Self::Update(u) => format!(
                "update: {}",
                match u {
                    MediaUpdate::None => "none",
                    MediaUpdate::Slow => "slow",
                    MediaUpdate::Fast => "fast",
                }
            ),
            Self::NavigationControls(n) => format!(
                "navigation-controls: {}",
                match n {
                    MediaNavigationControls::None => "none",
                    MediaNavigationControls::BackButton => "back-button",
                }
            ),
            Self::OverflowInline(o) => format!(
                "overflow-inline: {}",
                match o {
                    MediaOverflowInline::None => "none",
                    MediaOverflowInline::Scroll => "scroll",
                }
            ),
            Self::OverflowBlock(o) => format!(
                "overflow-block: {}",
                match o {
                    MediaOverflowBlock::None => "none",
                    MediaOverflowBlock::Scroll => "scroll",
                    MediaOverflowBlock::Paged => "paged",
                }
            ),
            // Boolean context — bare feature name, no `: value` (same shape
            // as `Self::Color` above).
            Self::BooleanContext(b) => b.feature_name().to_string(),
        }
    }
}

fn dynamic_range_str(d: MediaDynamicRange) -> &'static str {
    match d {
        MediaDynamicRange::Standard => "standard",
        MediaDynamicRange::High => "high",
    }
}

fn hover_str(h: MediaHover) -> &'static str {
    match h {
        MediaHover::None => "none",
        MediaHover::Hover => "hover",
    }
}

fn pointer_str(p: MediaPointer) -> &'static str {
    match p {
        MediaPointer::None => "none",
        MediaPointer::Coarse => "coarse",
        MediaPointer::Fine => "fine",
    }
}

/// Распарсить media query из строки между `@media` и `{`. Принимает
/// строку без обрамляющих whitespace. Грамматика (упрощённая, Media
/// Queries L4 §3):
/// ```text
/// query-list    = query [ "," query ]*
/// query         = [ "not" | "only" ]? primary [ "and" primary ]*
/// primary       = ident | "(" feature ")"
/// ```
///
/// Возвращает `MediaQuery` с `clauses.len() == 0` если строка пустая
/// (= `@media all`). Неизвестные feature-имена дают `Unsupported` (не
/// матчат) — это lenient parser для forward-compat.
pub fn parse_media_query(s: &str) -> MediaQuery {
    let s = s.trim();
    if s.is_empty() {
        return MediaQuery::default();
    }
    let clauses = s.split(',').map(parse_media_clause).collect();
    MediaQuery { clauses, raw: s.to_string() }
}

pub(crate) fn parse_media_clause(s: &str) -> MediaQueryClause {
    let mut input = s.trim();

    // Per L4 §3.2 ведущие `not`/`only` — модификаторы query. `only`
    // используется для скрытия от L3-without-media-queries браузеров —
    // для нас семантически no-op. `not` инвертирует clause.
    let mut negated = false;
    let mut only = false;
    if let Some(rest) = strip_leading_keyword(input, "not") {
        negated = true;
        input = rest;
    } else if let Some(rest) = strip_leading_keyword(input, "only") {
        only = true;
        input = rest;
    }

    let mut conditions = Vec::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.starts_with('(') {
            // Найти БАЛАНСИРОВАННУЮ закрывающую `)` — не первую попавшуюся:
            // с появлением `calc(...)` внутри значения фичи (`resolution`,
            // BUG-1019) `input.find(')')` закрывал бы calc-скобку, а не
            // внешнюю фичевую, обрезая значение на середине.
            //
            // Отсутствующая закрывающая `)` — не ошибка (CSS Syntax L3
            // "consume a component value": незакрытый блок молча
            // домысливается закрытым в конце входа, BUG-1020) — остаток
            // строки становится содержимым фичи, а не роняет clause целиком.
            let (inner_end, next_start) = match find_matching_close_paren(input) {
                Some(end) => (end, end + 1),
                None => (input.len(), input.len()),
            };
            let inner = &input[1..inner_end];
            conditions.push(parse_media_feature(inner.trim()));
            input = &input[next_start..];
        } else {
            // `)` в разделителях: одиночный `)` без парной `(` в этой же
            // clause — синтаксическая ошибка (BUG-1020), а не буквальный
            // media-type/лишний хвост у предыдущего слова.
            let end = input
                .find(|c: char| c.is_whitespace() || c == '(' || c == ',' || c == ')')
                .unwrap_or(input.len());
            let word = &input[..end];
            let stray_close_paren = input[end..].starts_with(')');
            input = &input[end..];
            if word.eq_ignore_ascii_case("and") {
                continue;
            }
            // Дополнительные `not`/`only` внутри clause — синтаксически
            // невалидны (L4 разрешает их только в позиции query-prefix
            // или внутри `(not (...))`-conditions, которые мы пока не
            // парсим). Считаем clause unknown, чтобы не сматчить случайно.
            if word.eq_ignore_ascii_case("not") || word.eq_ignore_ascii_case("only") {
                return MediaQueryClause {
                    negated,
                    only,
                    conditions: vec![MediaCondition::Unsupported],
                };
            }
            if stray_close_paren {
                return MediaQueryClause {
                    negated,
                    only,
                    conditions: vec![MediaCondition::Unsupported],
                };
            }
            conditions.push(MediaCondition::MediaType(word.to_ascii_lowercase()));
        }
    }

    if conditions.is_empty() {
        // `@media not` без feature / media-type — invalid query
        // (Media Queries L4 §3.2 «not <media-query>» требует body).
        conditions.push(MediaCondition::Unsupported);
    }

    MediaQueryClause { negated, only, conditions }
}

/// Индекс закрывающей `)`, парная открывающей в позиции `0` (`s` должна
/// начинаться с `(`) — считает вложенность, а не берёт первую попавшуюся
/// `)`, так что `(resolution: calc(1x))`'s внешняя скобка не обрезается на
/// закрытии `calc(`'s собственной (BUG-1019).
fn find_matching_close_paren(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Если строка начинается с `keyword` (ASCII case-insensitive) и за ним
/// следует whitespace или `(` — отрезает префикс и возвращает остаток.
/// Иначе возвращает `None`. Нужно, чтобы `notebook` / `only-child` не
/// принимались за keyword.
pub(crate) fn strip_leading_keyword<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = input.trim_start();
    let lower = trimmed.as_bytes();
    let kw = keyword.as_bytes();
    if lower.len() < kw.len() + 1 {
        return None;
    }
    if !trimmed.is_char_boundary(kw.len()) {
        return None;
    }
    if !trimmed[..kw.len()].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let next = trimmed.as_bytes()[kw.len()];
    if !(next == b' ' || next == b'\t' || next == b'\n' || next == b'\r' || next == b'(') {
        return None;
    }
    Some(&trimmed[kw.len()..])
}

/// Парсит значение длины в px: `Npx`, `Nem` (1em=16px), `Nrem` (1rem=16px).
/// Используется только для media features, где viewport context недоступен.
pub(crate) fn parse_media_length_px(val: &str) -> Option<f32> {
    const ROOT_EM: f32 = 16.0;
    if let Some(n) = val.strip_suffix("px") {
        n.trim().parse::<f32>().ok()
    } else if let Some(n) = val.strip_suffix("rem") {
        n.trim().parse::<f32>().ok().map(|v| v * ROOT_EM)
    } else if let Some(n) = val.strip_suffix("em") {
        n.trim().parse::<f32>().ok().map(|v| v * ROOT_EM)
    } else {
        None
    }
}

/// Парсит значение aspect-ratio: `N/M` или просто `N` (= `N/1`).
/// Числитель/знаменатель сохраняются раздельно (не делятся сразу) —
/// нужно для точной сериализации (`1/3` → `1 / 3`, не `0.33333334`).
pub(crate) fn parse_aspect_ratio(val: &str) -> Option<(f32, f32)> {
    if let Some((n, d)) = val.split_once('/') {
        let n: f32 = n.trim().parse().ok()?;
        let d: f32 = d.trim().parse().ok()?;
        if d == 0.0 { return None; }
        Some((n, d))
    } else {
        let n: f32 = val.trim().parse().ok()?;
        Some((n, 1.0))
    }
}

/// Парсит `<resolution>` в канонический `dppx`: `x`/`dppx` (1:1), `dpi`
/// (÷96 — 96dpi = 1dppx), `dpcm` (×2.54/96 — 1dpcm = 2.54/96 dppx, поскольку
/// 1px = 1/96in = 2.54/96cm). `dppx`/`dpcm` должны проверяться раньше
/// голого `x` — оба тоже кончаются на `x`.
pub(crate) fn parse_resolution_dppx(val: &str) -> Option<f32> {
    let val = val.trim();
    if let Some(n) = val.strip_suffix("dppx") {
        n.trim().parse::<f32>().ok()
    } else if let Some(n) = val.strip_suffix("dpcm") {
        n.trim().parse::<f32>().ok().map(|v| v * 2.54 / 96.0)
    } else if let Some(n) = val.strip_suffix("dpi") {
        n.trim().parse::<f32>().ok().map(|v| v / 96.0)
    } else if let Some(n) = val.strip_suffix('x') {
        n.trim().parse::<f32>().ok()
    } else {
        None
    }
}

/// Парсит значение `<resolution>` media-фичи — литерал (`2dppx`) или
/// `calc(...)` (BUG-1019). Регистр `calc(`/юнитов не важен по спеке —
/// сравнение по lower-case копии, сами числа регистр не имеют.
pub(crate) fn parse_resolution_value(val: &str) -> Option<ResolutionValue> {
    let lower = val.trim().to_ascii_lowercase();
    if let Some(inner) = lower.strip_prefix("calc(") {
        let inner = inner.strip_suffix(')')?;
        return eval_resolution_calc(inner).map(ResolutionValue::Calc);
    }
    parse_resolution_dppx(&lower).map(ResolutionValue::Literal)
}

/// Вычисляет содержимое `calc(...)` для `<resolution>` — `+`/`-`/`*`/`/` с
/// одноуровневыми операндами (`<resolution>` для `+`/`-`, `<resolution>` и
/// голое число для `*`/`/`), без вложенных `calc()`/скобок — единственная
/// форма, которую заводит Media Queries L4's `match-media-parsing.html`
/// (`calc(1x + 2x)`, `calc(5x - 2x)`, `calc(1x * 3)`, `calc(6x / 2)`).
fn eval_resolution_calc(expr: &str) -> Option<f32> {
    let tokens = tokenize_calc(expr);
    if tokens.is_empty() {
        return None;
    }
    // Multiplicative terms first (лево-ассоциативно), затем сложение —
    // терм по терму, знак перед термом храним отдельно от `*`/`/`
    // внутри него.
    let mut total = 0.0_f32;
    let mut term_sign = 1.0_f32;
    let mut idx = 0;
    loop {
        let mut value = parse_calc_operand(tokens.get(idx)?)?;
        idx += 1;
        while matches!(tokens.get(idx).map(String::as_str), Some("*") | Some("/")) {
            let op = tokens[idx].clone();
            idx += 1;
            let rhs = parse_calc_operand(tokens.get(idx)?)?;
            idx += 1;
            if op == "*" {
                value *= rhs;
            } else {
                if rhs == 0.0 {
                    return None;
                }
                value /= rhs;
            }
        }
        total += term_sign * value;
        match tokens.get(idx).map(String::as_str) {
            None => return Some(total),
            Some("+") => {
                term_sign = 1.0;
                idx += 1;
            }
            Some("-") => {
                term_sign = -1.0;
                idx += 1;
            }
            _ => return None,
        }
    }
}

/// Токенизирует `calc()`-содержимое: числа-с-юнитом (`1x`, `600dpi`, голое
/// `3`) как один токен, `+`/`-`/`*`/`/` как отдельные однобуквенные токены.
fn tokenize_calc(expr: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in expr.chars() {
        match c {
            '+' | '-' | '*' | '/' => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    tokens.push(trimmed.to_string());
                }
                current.clear();
                tokens.push(c.to_string());
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        tokens.push(trimmed.to_string());
    }
    tokens
}

/// Операнд `calc()`: `<resolution>` (конвертируется в `dppx`) либо голое
/// число (множитель/делитель у `*`/`/`).
fn parse_calc_operand(tok: &str) -> Option<f32> {
    parse_resolution_dppx(tok).or_else(|| tok.trim().parse::<f32>().ok())
}

pub(crate) fn parse_media_feature(s: &str) -> MediaCondition {
    // `feature: value` или просто `feature` — CSS Values L4's boolean
    // context. `color` — особый случай без реальной range-фичи за ним
    // (BUG-1020); каждая уже реализованная discrete-фича получает свою
    // boolean-форму через `BooleanFeature` (BUG-527) — общий механизм для
    // range-фич (`width`/`resolution`/…, требуют `<`/`<=`/`>`/`>=`-grammar)
    // по-прежнему не затронут.
    let Some((key, val)) = s.split_once(':') else {
        let bare = s.trim().to_ascii_lowercase();
        return match bare.as_str() {
            "color" => MediaCondition::Feature(MediaFeature::Color),
            _ => match BooleanFeature::from_name(&bare) {
                Some(b) => MediaCondition::Feature(MediaFeature::BooleanContext(b)),
                None => MediaCondition::Unsupported,
            },
        };
    };
    let key = key.trim().to_ascii_lowercase();
    let val = val.trim();
    match key.as_str() {
        "width" | "min-width" | "max-width" | "height" | "min-height" | "max-height" => {
            let Some(px) = parse_media_length_px(val) else {
                return MediaCondition::Unsupported;
            };
            let feature = match key.as_str() {
                "width" => MediaFeature::Width(px),
                "min-width" => MediaFeature::MinWidth(px),
                "max-width" => MediaFeature::MaxWidth(px),
                "height" => MediaFeature::Height(px),
                "min-height" => MediaFeature::MinHeight(px),
                "max-height" => MediaFeature::MaxHeight(px),
                _ => unreachable!(),
            };
            MediaCondition::Feature(feature)
        }
        "aspect-ratio" | "min-aspect-ratio" | "max-aspect-ratio" => {
            let Some((n, d)) = parse_aspect_ratio(val) else {
                return MediaCondition::Unsupported;
            };
            let feature = match key.as_str() {
                "aspect-ratio" => MediaFeature::AspectRatio(n, d),
                "min-aspect-ratio" => MediaFeature::MinAspectRatio(n, d),
                "max-aspect-ratio" => MediaFeature::MaxAspectRatio(n, d),
                _ => unreachable!(),
            };
            MediaCondition::Feature(feature)
        }
        "resolution" | "min-resolution" | "max-resolution" => {
            let Some(value) = parse_resolution_value(val) else {
                return MediaCondition::Unsupported;
            };
            let feature = match key.as_str() {
                "resolution" => MediaFeature::Resolution(value),
                "min-resolution" => MediaFeature::MinResolution(value),
                "max-resolution" => MediaFeature::MaxResolution(value),
                _ => unreachable!(),
            };
            MediaCondition::Feature(feature)
        }
        "orientation" => match val.to_ascii_lowercase().as_str() {
            "portrait" => MediaCondition::Feature(MediaFeature::Orientation(MediaOrientation::Portrait)),
            "landscape" => MediaCondition::Feature(MediaFeature::Orientation(MediaOrientation::Landscape)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-color-scheme" => match val.to_ascii_lowercase().as_str() {
            "light" => MediaCondition::Feature(MediaFeature::PrefersColorScheme(ColorScheme::Light)),
            "dark" => MediaCondition::Feature(MediaFeature::PrefersColorScheme(ColorScheme::Dark)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-reduced-motion" => match val.to_ascii_lowercase().as_str() {
            "reduce" => MediaCondition::Feature(MediaFeature::PrefersReducedMotion(true)),
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersReducedMotion(false)),
            _ => MediaCondition::Unsupported,
        },
        "forced-colors" => match val.to_ascii_lowercase().as_str() {
            "active" => MediaCondition::Feature(MediaFeature::ForcedColors(true)),
            "none" => MediaCondition::Feature(MediaFeature::ForcedColors(false)),
            _ => MediaCondition::Unsupported,
        },
        "hover" | "any-hover" => {
            let h = match val.to_ascii_lowercase().as_str() {
                "none" => MediaHover::None,
                "hover" => MediaHover::Hover,
                _ => return MediaCondition::Unsupported,
            };
            MediaCondition::Feature(if key == "hover" {
                MediaFeature::Hover(h)
            } else {
                MediaFeature::AnyHover(h)
            })
        }
        "pointer" | "any-pointer" => {
            let p = match val.to_ascii_lowercase().as_str() {
                "none" => MediaPointer::None,
                "coarse" => MediaPointer::Coarse,
                "fine" => MediaPointer::Fine,
                _ => return MediaCondition::Unsupported,
            };
            MediaCondition::Feature(if key == "pointer" {
                MediaFeature::Pointer(p)
            } else {
                MediaFeature::AnyPointer(p)
            })
        }
        "prefers-contrast" => match val.to_ascii_lowercase().as_str() {
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::NoPreference)),
            "more" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::More)),
            "less" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::Less)),
            "custom" => MediaCondition::Feature(MediaFeature::PrefersContrast(MediaContrast::Custom)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-reduced-data" => match val.to_ascii_lowercase().as_str() {
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersReducedData(MediaReducedData::NoPreference)),
            "reduce" => MediaCondition::Feature(MediaFeature::PrefersReducedData(MediaReducedData::Reduce)),
            _ => MediaCondition::Unsupported,
        },
        "prefers-reduced-transparency" => match val.to_ascii_lowercase().as_str() {
            "no-preference" => MediaCondition::Feature(MediaFeature::PrefersReducedTransparency(MediaReducedTransparency::NoPreference)),
            "reduce" => MediaCondition::Feature(MediaFeature::PrefersReducedTransparency(MediaReducedTransparency::Reduce)),
            _ => MediaCondition::Unsupported,
        },
        "scripting" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::Scripting(MediaScripting::None)),
            "initial-only" => MediaCondition::Feature(MediaFeature::Scripting(MediaScripting::InitialOnly)),
            "enabled" => MediaCondition::Feature(MediaFeature::Scripting(MediaScripting::Enabled)),
            _ => MediaCondition::Unsupported,
        },
        "inverted-colors" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::InvertedColors(MediaInvertedColors::None)),
            "inverted" => MediaCondition::Feature(MediaFeature::InvertedColors(MediaInvertedColors::Inverted)),
            _ => MediaCondition::Unsupported,
        },
        "display-mode" => match val.to_ascii_lowercase().as_str() {
            "standalone" => MediaCondition::Feature(MediaFeature::DisplayMode(MediaDisplayMode::Standalone)),
            "browser" => MediaCondition::Feature(MediaFeature::DisplayMode(MediaDisplayMode::Browser)),
            "minimal-ui" => MediaCondition::Feature(MediaFeature::DisplayMode(MediaDisplayMode::MinimalUi)),
            "fullscreen" => MediaCondition::Feature(MediaFeature::DisplayMode(MediaDisplayMode::Fullscreen)),
            "picture-in-picture" => {
                MediaCondition::Feature(MediaFeature::DisplayMode(MediaDisplayMode::PictureInPicture))
            }
            _ => MediaCondition::Unsupported,
        },
        "display-state" => match val.to_ascii_lowercase().as_str() {
            "normal" => MediaCondition::Feature(MediaFeature::DisplayState(MediaDisplayState::Normal)),
            "minimized" => MediaCondition::Feature(MediaFeature::DisplayState(MediaDisplayState::Minimized)),
            "maximized" => MediaCondition::Feature(MediaFeature::DisplayState(MediaDisplayState::Maximized)),
            "fullscreen" => MediaCondition::Feature(MediaFeature::DisplayState(MediaDisplayState::Fullscreen)),
            _ => MediaCondition::Unsupported,
        },
        "resizable" => match val.to_ascii_lowercase().as_str() {
            "true" => MediaCondition::Feature(MediaFeature::Resizable(true)),
            "false" => MediaCondition::Feature(MediaFeature::Resizable(false)),
            _ => MediaCondition::Unsupported,
        },
        "dynamic-range" | "video-dynamic-range" => {
            let range = match val.to_ascii_lowercase().as_str() {
                "standard" => MediaDynamicRange::Standard,
                "high" => MediaDynamicRange::High,
                _ => return MediaCondition::Unsupported,
            };
            MediaCondition::Feature(if key == "dynamic-range" {
                MediaFeature::DynamicRange(range)
            } else {
                MediaFeature::VideoDynamicRange(range)
            })
        }
        "update" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::Update(MediaUpdate::None)),
            "slow" => MediaCondition::Feature(MediaFeature::Update(MediaUpdate::Slow)),
            "fast" => MediaCondition::Feature(MediaFeature::Update(MediaUpdate::Fast)),
            _ => MediaCondition::Unsupported,
        },
        "navigation-controls" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::NavigationControls(MediaNavigationControls::None)),
            "back-button" => {
                MediaCondition::Feature(MediaFeature::NavigationControls(MediaNavigationControls::BackButton))
            }
            _ => MediaCondition::Unsupported,
        },
        "overflow-inline" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::OverflowInline(MediaOverflowInline::None)),
            "scroll" => MediaCondition::Feature(MediaFeature::OverflowInline(MediaOverflowInline::Scroll)),
            _ => MediaCondition::Unsupported,
        },
        "overflow-block" => match val.to_ascii_lowercase().as_str() {
            "none" => MediaCondition::Feature(MediaFeature::OverflowBlock(MediaOverflowBlock::None)),
            "scroll" => MediaCondition::Feature(MediaFeature::OverflowBlock(MediaOverflowBlock::Scroll)),
            "paged" => MediaCondition::Feature(MediaFeature::OverflowBlock(MediaOverflowBlock::Paged)),
            _ => MediaCondition::Unsupported,
        },
        _ => MediaCondition::Unsupported,
    }
}
