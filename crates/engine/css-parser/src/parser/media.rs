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
    // Aspect ratio: numerator/denominator stored as f32 ratio
    AspectRatio(f32),
    MinAspectRatio(f32),
    MaxAspectRatio(f32),
    // Display
    Orientation(MediaOrientation),
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
}

impl Eq for MediaFeature {}

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
}

impl Default for MediaContext {
    fn default() -> Self {
        // Desktop-дефолты: есть мышь → hover-способность и точный указатель.
        Self {
            media_type: "screen".into(),
            width: 0.0,
            height: 0.0,
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
            Self::AspectRatio(ratio) => {
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { f32::INFINITY };
                (actual - ratio).abs() < 0.01
            }
            Self::MinAspectRatio(ratio) => {
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { f32::INFINITY };
                actual >= *ratio
            }
            Self::MaxAspectRatio(ratio) => {
                let actual = if ctx.height > 0.0 { ctx.width / ctx.height } else { 0.0 };
                actual <= *ratio
            }
            Self::Orientation(o) => {
                let actual = if ctx.width >= ctx.height {
                    MediaOrientation::Landscape
                } else {
                    MediaOrientation::Portrait
                };
                actual == *o
            }
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
        }
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
    MediaQuery { clauses }
}

pub(crate) fn parse_media_clause(s: &str) -> MediaQueryClause {
    let mut input = s.trim();

    // Per L4 §3.2 ведущие `not`/`only` — модификаторы query. `only`
    // используется для скрытия от L3-without-media-queries браузеров —
    // для нас семантически no-op. `not` инвертирует clause.
    let mut negated = false;
    if let Some(rest) = strip_leading_keyword(input, "not") {
        negated = true;
        input = rest;
    } else if let Some(rest) = strip_leading_keyword(input, "only") {
        input = rest;
    }

    let mut conditions = Vec::new();
    while !input.is_empty() {
        input = input.trim_start();
        if input.starts_with('(') {
            // Найти match `)`.
            if let Some(end) = input.find(')') {
                let inner = &input[1..end];
                conditions.push(parse_media_feature(inner.trim()));
                input = &input[end + 1..];
            } else {
                return MediaQueryClause {
                    negated,
                    conditions: vec![MediaCondition::Unsupported],
                };
            }
        } else {
            let end = input
                .find(|c: char| c.is_whitespace() || c == '(' || c == ',')
                .unwrap_or(input.len());
            let word = &input[..end];
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

    MediaQueryClause { negated, conditions }
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

/// Парсит значение aspect-ratio: `N/M` или просто `N`.
pub(crate) fn parse_aspect_ratio(val: &str) -> Option<f32> {
    if let Some((n, d)) = val.split_once('/') {
        let n: f32 = n.trim().parse().ok()?;
        let d: f32 = d.trim().parse().ok()?;
        if d == 0.0 { return None; }
        Some(n / d)
    } else {
        val.trim().parse::<f32>().ok()
    }
}

pub(crate) fn parse_media_feature(s: &str) -> MediaCondition {
    // `feature: value` или просто `feature` (boolean feature, не поддерживаем).
    let Some((key, val)) = s.split_once(':') else {
        return MediaCondition::Unsupported;
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
            let Some(ratio) = parse_aspect_ratio(val) else {
                return MediaCondition::Unsupported;
            };
            let feature = match key.as_str() {
                "aspect-ratio" => MediaFeature::AspectRatio(ratio),
                "min-aspect-ratio" => MediaFeature::MinAspectRatio(ratio),
                "max-aspect-ratio" => MediaFeature::MaxAspectRatio(ratio),
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
        _ => MediaCondition::Unsupported,
    }
}
