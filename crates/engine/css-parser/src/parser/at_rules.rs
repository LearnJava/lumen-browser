//! CSS at-rules: типы правил (`@property`, `@supports`, `@font-face`,
//! `@layer`, `@keyframes`, …) и разбор их прелюдий и тел.
//! Media Queries вынесены в [`super::media`].
//!
//! Вырезано из `parser.rs` (SPLIT-CP1 срез 2/2) без изменения поведения.

// Долг по документации: код перенесён из `parser.rs` как есть; файл
// написан до включения `missing_docs`. Счётчики — docs/lint-policy.md §10.
#![allow(missing_docs)]

use super::*;

/// CSS Properties and Values L1 §1.1 — регистрация custom property через
/// `@property --name { syntax: ...; inherits: ...; initial-value: ...; }`.
/// Обязательные descriptors: `syntax`, `inherits`. `initial-value`
/// обязателен, если syntax не universal (`*`). Имя хранится с ведущими
/// `--` для прямого сравнения с `custom_props` в layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyRule {
    pub name: String,
    pub syntax: String,
    pub inherits: bool,
    pub initial_value: Option<String>,
}

/// `@function <name>(<params>) [returns <type>]? { declarations }` — CSS
/// Functions and Mixins L1. Declares an author-defined custom function
/// invoked from property values as `<name>(<args>)`. `<name>` is a
/// dashed-ident (function-token grammar: no whitespace before `(`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRule {
    /// Dashed-ident name, e.g. `--double`. Matched against `<name>(...)` calls.
    pub name: String,
    /// Positional parameters in declared order.
    pub parameters: Vec<FunctionParameter>,
    /// Raw `returns <type>` descriptor, if present. Stored but not type-checked
    /// (call-site substitution is untyped string substitution, same as `var()`).
    pub returns: Option<String>,
    /// Body declarations in source order: local `--x: ...;` custom properties
    /// used to build up a value, plus the `result: <value>;` descriptor that
    /// gives the function's return value.
    pub declarations: Vec<Declaration>,
}

/// One parameter of an `@function` rule: `--name` or `--name: <default>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParameter {
    /// Dashed-ident parameter name, e.g. `--x`. Referenced inside the body via `var(--x)`.
    pub name: String,
    /// Optional default value, substituted when the call site omits this argument.
    pub default: Option<String>,
}

/// `@color-profile --name { src: url(...); rendering-intent: ...; }` — CSS
/// Color L5 §4. Declares a named custom colour profile referenced from
/// `color(--name c1 c2 c3)`. Phase 0: descriptors are parsed and stored;
/// actual ICC-based colour transform is deferred (layout treats the profile's
/// channels as already-sRGB once a matching name is found).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorProfileRule {
    /// Dashed-ident name, e.g. `--swop5c`. Used to match `color(--name ...)` values.
    pub name: String,
    /// `src` descriptor — URL of the ICC profile resource (loading deferred).
    pub src: Option<String>,
    /// `rendering-intent` descriptor — one of `relative-colorimetric` (default),
    /// `absolute-colorimetric`, `perceptual`, `saturation`.
    pub rendering_intent: Option<String>,
}

/// `@font-palette-values --name { font-family: ...; base-palette: N; override-colors: ... }`
/// CSS Fonts L4 §13. Defines a named custom color palette for a COLR color font.
/// Matched against an element's `font-palette` property value to resolve which
/// palette overrides apply at render time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontPaletteValuesRule {
    /// Dashed-ident name, e.g. `--my-palette`. Used to match `font-palette` property values.
    pub name: String,
    /// `font-family` descriptor — the font family this palette applies to (without quotes).
    pub font_family: Option<String>,
    /// `base-palette` descriptor — 0-based index of the built-in CPAL palette to start from.
    /// None means start from palette index 0 (the default palette).
    pub base_palette: Option<u16>,
    /// `override-colors` descriptor — raw `"<index> <color>"` pairs as strings.
    /// Stored raw for layout-side parsing via `parse_color`. Each entry is `(index, color_str)`.
    pub override_colors: Vec<(u16, String)>,
}

/// `@container <name>? <condition> { rules }` — CSS Containment L3 §3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRule {
    /// Имя container query (по умолчанию — None, match всех ancestor-ов
    /// с container-name / container-type).
    pub name: Option<String>,
    /// Сырая condition-строка типа `(min-width: 200px)` или `style(...)`.
    pub condition: String,
    pub rules: Vec<Rule>,
}

/// `@counter-style <name> { ... }` — CSS Counter Styles L3 §2.
/// Phase 0: parse+store. Descriptors (`system`, `symbols`, `suffix`,
/// `range`, `prefix`, `pad`, `negative`, ...) хранятся как declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterStyleRule {
    pub name: String,
    pub declarations: Vec<Declaration>,
}

/// `@page <selector>? { decls }` — CSS Paged Media L3 §3.
/// Selector — пустой (любая страница), `:first`, `:left`, `:right`,
/// `:blank`, named `page-name`. Phase 0: хранится сырая строка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRule {
    /// Pseudo-classes и/или page-name. Пустая строка = любой page.
    pub selector: String,
    pub declarations: Vec<Declaration>,
}

/// `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6.
/// `root` — селектор корня scope, `limit` — селектор upper boundary
/// (рекурсивный обход вниз останавливается на нём). Phase 0: оба
/// хранятся сырыми строками; реальный scope-matcher отложен.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRule {
    /// Селектор корня scope. Может быть пустым (`@scope { ... }`
    /// без явного root — implicit `:scope` = stylesheet root).
    pub root: String,
    /// Опциональный limit (`to (<selector>)`). None — без верхней границы.
    pub limit: Option<String>,
    pub rules: Vec<Rule>,
}

/// `@starting-style { rules }` — CSS Transitions L2 §3.4. Контейнер
/// rules, применяющихся как initial state при first match (для
/// transition-on-display-changes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartingStyleRule {
    pub rules: Vec<Rule>,
}

/// `@keyframes name { offset { decls } ... }` — CSS Animations L1 §3.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframesRule {
    pub name: String,
    /// Список frames в порядке появления в source. Один frame может
    /// иметь несколько offset-ов (selector-list типа `0%, 50%`) —
    /// разворачивается в отдельные записи.
    pub frames: Vec<Keyframe>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    /// Offset в долях `[0, 1]`. `from` → 0.0, `to` → 1.0. Невалидные
    /// (NaN или вне [0,1]) → пропускаются на этапе парсинга.
    pub offset: f32,
    pub declarations: Vec<Declaration>,
}

/// `@supports <condition> { rules }` блок — CSS Conditional Rules L3 §2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportsRule {
    pub condition: SupportsCondition,
    pub rules: Vec<Rule>,
}

/// Условие в `@supports (...)`. Грамматика:
/// `<condition> = <negation> | <conjunction> | <disjunction> | <test>`
/// `<negation>  = "not" <inside-parens>`
/// `<conjunction> = <test> ("and" <test>)+`
/// `<disjunction> = <test> ("or" <test>)+`
/// `<test>       = "(" <property>: <value> ")" | "(" <condition> ")"`.
///
/// Phase 0: парсер также распознаёт `selector(<simple>)` (CSS Conditional
/// L4) и сохраняет селектор как сырую строку.
/// Функциональные тесты `font-tech(<font-tech>)` и
/// `font-format(<font-format>)` (CSS Conditional L4 §4 / CSS Fonts L4 §4.3)
/// тоже типизированы — evaluator сверяет аргумент со списком технологий и
/// форматов шрифтов, поддержанных движком `lumen-font`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportsCondition {
    /// `(prop: value)` — declaration test. Текущий supports-evaluator
    /// проверяет, что `property` есть в списке known-property-имён,
    /// не валидируя value (для Phase 0 этого достаточно — мы поддерживаем
    /// конкретный набор properties, и tests типа `(display: grid)`
    /// возвращают true, потому что мы парсим `display`, даже если
    /// реального grid layout-а нет).
    Decl { property: String, value: String },
    Not(Box<SupportsCondition>),
    And(Vec<SupportsCondition>),
    Or(Vec<SupportsCondition>),
    /// `selector(<sel>)` — CSS Conditional L4. Phase 0 не оценивает.
    Selector(String),
    /// `font-tech(<font-tech>)` — CSS Conditional L4 §4 / CSS Fonts L4 §4.3.
    /// Хранит lowercase-ключевое слово технологии шрифта (например,
    /// `variations`, `color-colrv1`, `features-opentype`). Evaluator
    /// возвращает `true`, если технология реализована в `lumen-font`.
    FontTech(String),
    /// `font-format(<font-format>)` — CSS Conditional L4 §4 / CSS Fonts L4 §4.3.
    /// Хранит lowercase-ключевое слово формата шрифта (например, `woff2`,
    /// `opentype`, `truetype`). Кавычки legacy-строкового синтаксиса
    /// (`font-format("woff2")`) снимаются при разборе. Evaluator возвращает
    /// `true`, если формат декодируется движком `lumen-font`.
    FontFormat(String),
    /// Невалидный или нераспознанный тест — evaluator возвращает false.
    Unknown,
}

/// Технологии шрифтов (`<font-tech>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` реально реализует: OpenType-фичи (GSUB/GPOS) и вариативные
/// шрифты (fvar/gvar/avar/HVAR/MVAR). Цветные глифы (COLR/CPAL, sbix, CBDT,
/// SVG-in-OpenType), палитры, AAT/Graphite-фичи и инкрементальная загрузка
/// пока не поддержаны — см. `crates/engine/font/src/lib.rs` (заголовок).
pub(crate) const SUPPORTED_FONT_TECH: &[&str] = &["features-opentype", "variations"];

/// Форматы шрифтов (`<font-format>`, CSS Fonts L4 §4.3), которые
/// `lumen-font` умеет декодировать: TrueType (glyf), OpenType (CFF/glyf +
/// OT layout), WOFF1 (`decode_woff1`) и WOFF2 (`decode_woff2`). Контейнеры
/// `collection` (.ttc), `embedded-opentype` (EOT) и `svg`-шрифты не
/// поддержаны — см. `crates/engine/font/src/woff2.rs` и `lib.rs`.
pub(crate) const SUPPORTED_FONT_FORMAT: &[&str] = &["opentype", "truetype", "woff", "woff2"];

impl SupportsCondition {
    /// Вычислить условие: вернуть `true`, если потребитель поддерживает
    /// все объявления в условии. `known_properties` — список property-
    /// имён, которые css-parser/layout распознают (например, `display`,
    /// `color`, `grid-template-columns`).
    ///
    /// `Selector(<sel>)` (CSS Conditional L4 §4.2 `selector()`) парсится и
    /// признаётся поддержанным, если каждая его часть распознаётся движком —
    /// см. [`ComplexSelector::is_supported`]. Пустой/невалидный селектор → `false`.
    /// `FontTech`/`FontFormat` сверяются со списками технологий и форматов,
    /// которые реально реализует `lumen-font` ([`SUPPORTED_FONT_TECH`] /
    /// [`SUPPORTED_FONT_FORMAT`]). `Unknown` → `false`.
    pub fn evaluate(&self, known_properties: &[&str]) -> bool {
        match self {
            Self::Decl { property, .. } => known_properties
                .iter()
                .any(|p| p.eq_ignore_ascii_case(property)),
            Self::Not(c) => !c.evaluate(known_properties),
            Self::And(cs) => cs.iter().all(|c| c.evaluate(known_properties)),
            Self::Or(cs) => cs.iter().any(|c| c.evaluate(known_properties)),
            Self::Selector(sel) => {
                let list = parse_selector_list(sel);
                !list.is_empty() && list.iter().all(ComplexSelector::is_supported)
            }
            Self::FontTech(tech) => SUPPORTED_FONT_TECH
                .iter()
                .any(|t| t.eq_ignore_ascii_case(tech)),
            Self::FontFormat(fmt) => SUPPORTED_FONT_FORMAT
                .iter()
                .any(|f| f.eq_ignore_ascii_case(fmt)),
            Self::Unknown => false,
        }
    }
}

/// `@layer name { rules }` блок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerRule {
    /// Имя layer-а. Анонимный блок (`@layer { ... }`) получает имя
    /// `__anon_<n>__` где `n` — порядковый номер.
    pub name: String,
    pub rules: Vec<Rule>,
}

/// `@import` декларация. Per CSS Cascade L4 §6.5 + Media Queries L4:
/// `@import url("path");` или `@import url("path") <media-query>;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRule {
    /// URL для загрузки. Хранится как есть (без resolve относительно base).
    pub url: String,
    /// Опциональный media query — стиль применим только если query
    /// matches. Пустой Vec в `clauses` (=default) трактуется как
    /// «всегда применять» (= `@import url("...")` без media-фильтра).
    pub media: MediaQuery,
}

/// `@font-face { font-family: ...; src: url(...) format(...); ... }`
/// — CSS Fonts L4 §4. Регистрация webfont-ресурса для font-matcher-а.
/// Phase 0: парсер собирает основные descriptors; реальный fetch и
/// font-loading — задача font-matcher / shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFaceRule {
    /// `font-family: "Roboto"` — имя без кавычек.
    pub family: String,
    /// `src: url("..."), url("..."), local("...")` — список источников.
    pub sources: Vec<FontFaceSource>,
    /// `font-weight: 400 | bold | 100 200 ...` — хранится сырой строкой
    /// (font-matcher парсит keyword/число/диапазон по контексту). `None` = default (400).
    pub weight: Option<String>,
    /// `font-style: normal | italic | oblique`. `None` = default.
    pub style: Option<String>,
    /// `font-stretch: condensed | expanded | 75% 125% ...` — сырая строка. `None` = default (normal).
    pub stretch: Option<String>,
    /// `font-display: auto | block | swap | fallback | optional`. `None` = default (auto).
    pub display: Option<String>,
    /// `unicode-range: U+0000-FFFF, U+10000-1FFFF` — сырая строка.
    pub unicode_range: Option<String>,
    /// `font-variant: small-caps | ...` — CSS Fonts L3/L4 §7. Сырая строка.
    pub variant: Option<String>,
    /// `font-feature-settings: "liga" 1, "kern" 0` — CSS Fonts L3 §6. Сырая строка.
    pub feature_settings: Option<String>,
    /// `font-variation-settings: "wght" 400, "ital" 1` — CSS Fonts L4 §6 (variable fonts). Сырая строка.
    pub variation_settings: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFaceSource {
    pub kind: FontFaceSourceKind,
    /// Значение url или local — без кавычек.
    pub value: String,
    /// `format("woff2")` — hint о формате. None если не указан.
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFaceSourceKind {
    /// `src: url("...")` — внешний font-файл.
    Url,
    /// `src: local("...")` — системный шрифт по имени.
    Local,
}

pub(crate) enum AtRuleOutcome {
    Property(PropertyRule),
    Media(MediaRule),
    Import(ImportRule),
    FontFace(FontFaceRule),
    FontPaletteValues(FontPaletteValuesRule),
    LayerNames(Vec<String>),
    LayerBlock {
        name: Option<String>,
        rules: Vec<Rule>,
    },
    Supports(SupportsRule),
    Keyframes(KeyframesRule),
    CounterStyle(CounterStyleRule),
    Page(PageRule),
    Scope(ScopeRule),
    StartingStyle(StartingStyleRule),
    Container(ContainerRule),
    ColorProfile(ColorProfileRule),
    Function(FunctionRule),
    None,
}

/// Парсит keyframe-селектор: `from` / `to` / `<percentage>` / списки
/// через запятую (`0%, 50%`). Возвращает offset-ы в [0, 1]; невалидные
/// токены пропускаются.
pub(crate) fn parse_keyframe_selectors(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    for tok in s.split(',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        if t.eq_ignore_ascii_case("from") {
            out.push(0.0);
            continue;
        }
        if t.eq_ignore_ascii_case("to") {
            out.push(1.0);
            continue;
        }
        if let Some(num_str) = t.strip_suffix('%')
            && let Ok(n) = num_str.trim().parse::<f32>()
            && n.is_finite()
            && (0.0..=100.0).contains(&n)
        {
            out.push(n / 100.0);
        }
    }
    out
}

/// Layer-имя — CSS-ident, опционально с точками (sub-layers через
/// `base.text`, CSS Cascade L5 §6.4.1). Phase 0 поддерживает простые
/// имена (без точек) и dotted-имена как одну строку, не разбивая иерархию.
pub(crate) fn is_layer_name(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    s.split('.').all(|part| {
        let mut chars = part.chars();
        let Some(first) = chars.next() else { return false };
        if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
            return false;
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Парсит значение `src:` из `@font-face`: comma-separated список
/// `url("path") format("fmt")` или `local("name")`. Игнорирует
/// невалидные элементы (best-effort).
pub(crate) fn parse_font_face_src(src: &str) -> Vec<FontFaceSource> {
    let mut out = Vec::new();
    for item in split_top_level_commas(src) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        // Найти `url(` или `local(`.
        let (kind, after) = if let Some(rest) = item.strip_prefix("url(") {
            (FontFaceSourceKind::Url, rest)
        } else if let Some(rest) = item.strip_prefix("local(") {
            (FontFaceSourceKind::Local, rest)
        } else {
            continue;
        };
        let Some(close) = after.find(')') else {
            continue;
        };
        let inner = after[..close].trim().trim_matches(['"', '\''].as_ref());
        let tail = after[close + 1..].trim();
        // Опциональный `format("...")`.
        let format = if let Some(fmt_rest) = tail.strip_prefix("format(") {
            fmt_rest
                .find(')')
                .map(|end| fmt_rest[..end].trim().trim_matches(['"', '\''].as_ref()).to_string())
        } else {
            None
        };
        out.push(FontFaceSource {
            kind,
            value: inner.to_string(),
            format,
        });
    }
    out
}

/// Делит строку по top-level запятым (игнорирует запятые внутри `(...)`
/// и строковых литералов). Используется для `src:` value
/// (`url(a), url(b) format(c)`) и подобных list-значений.
pub(crate) fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string: Option<u8> = None;
    let mut start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = in_string {
            if b == q {
                in_string = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => in_string = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < bytes.len() {
        out.push(&s[start..]);
    }
    out
}

/// Парсит `@supports`-условие из строки между `@supports` и `{`.
///
/// Грамматика (упрощённая): `<expr> = <term> (("and"|"or") <term>)*`,
/// `<term> = "not"? <atom>`, `<atom> = "(" <inner> ")" | "selector(" sel ")"`,
/// `<inner> = <expr> | <prop ":" value>`.
///
/// Phase 0 ограничения:
/// - Mixing `and` и `or` на одном уровне не разрешено (per spec), но
///   парсер lenient — берёт первый встретившийся combinator и применяет
///   его ко всем term-ам этого уровня. Реалистичные tests этого не
///   нарушают (`(a) and (b) and (c)` или `(a) or (b)`); смешанные — UB.
/// - Нерекурсивный `selector(...)` хранит сырой селектор; реальный
///   match — отложенная задача.
pub fn parse_supports_condition(s: &str) -> SupportsCondition {
    let s = s.trim();
    if s.is_empty() {
        return SupportsCondition::Unknown;
    }
    let bytes = s.as_bytes();
    let mut pos = 0usize;
    let result = parse_supports_expr(bytes, &mut pos);
    skip_ws(bytes, &mut pos);
    if pos < bytes.len() {
        // Если что-то осталось — это синтаксическая ошибка; возвращаем
        // частично разобранное (lenient).
    }
    result
}

/// Парсит значение `override-colors` из `@font-palette-values`.
/// Формат: comma-separated `<u16-index> <color-string>` пары.
/// CSS Fonts L4 §13.3. Хранит color как raw string — resolve через
/// `parse_color` выполняется в layout при использовании palette.
pub(crate) fn parse_override_colors(s: &str) -> Vec<(u16, String)> {
    let mut result = Vec::new();
    for pair in s.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, char::is_whitespace);
        if let (Some(idx_str), Some(color_str)) = (parts.next(), parts.next())
            && let Ok(idx) = idx_str.trim().parse::<u16>()
        {
            let color = color_str.trim().to_string();
            if !color.is_empty() {
                result.push((idx, color));
            }
        }
    }
    result
}

pub(crate) fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && b[*p].is_ascii_whitespace() {
        *p += 1;
    }
}

pub(crate) fn match_keyword_ci(b: &[u8], p: &mut usize, kw: &[u8]) -> bool {
    skip_ws(b, p);
    if *p + kw.len() > b.len() {
        return false;
    }
    if !b[*p..*p + kw.len()].eq_ignore_ascii_case(kw) {
        return false;
    }
    // Граница: следующий символ — не ident-char.
    let after = *p + kw.len();
    if after < b.len() {
        let c = b[after];
        if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' {
            return false;
        }
    }
    *p = after;
    true
}

pub(crate) fn parse_supports_expr(b: &[u8], p: &mut usize) -> SupportsCondition {
    let first = parse_supports_term(b, p);
    skip_ws(b, p);
    // Определяем combinator (если есть).
    let saved = *p;
    if match_keyword_ci(b, p, b"and") {
        let mut terms = vec![first];
        loop {
            terms.push(parse_supports_term(b, p));
            skip_ws(b, p);
            let save = *p;
            if !match_keyword_ci(b, p, b"and") {
                *p = save;
                break;
            }
        }
        return SupportsCondition::And(terms);
    }
    *p = saved;
    if match_keyword_ci(b, p, b"or") {
        let mut terms = vec![first];
        loop {
            terms.push(parse_supports_term(b, p));
            skip_ws(b, p);
            let save = *p;
            if !match_keyword_ci(b, p, b"or") {
                *p = save;
                break;
            }
        }
        return SupportsCondition::Or(terms);
    }
    first
}

pub(crate) fn parse_supports_term(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    if match_keyword_ci(b, p, b"not") {
        let inner = parse_supports_atom(b, p);
        return SupportsCondition::Not(Box::new(inner));
    }
    parse_supports_atom(b, p)
}

/// Если ввод в позиции `*p` начинается с функции `name` (case-insensitive),
/// продвинуть `*p` за закрывающую `)` и вернуть содержимое скобок как строку.
/// Иначе оставить `*p` без изменений и вернуть `None`. Учитывает вложенные
/// скобки в аргументе (хотя для `font-tech`/`font-format` они не нужны).
pub(crate) fn match_func_arg(b: &[u8], p: &mut usize, name: &[u8]) -> Option<String> {
    let n = name.len();
    if *p + n > b.len() || !b[*p..*p + n].eq_ignore_ascii_case(name) {
        return None;
    }
    let start = *p + n;
    let mut q = start;
    let mut depth: i32 = 1;
    while q < b.len() && depth > 0 {
        match b[q] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            break;
        }
        q += 1;
    }
    let arg = std::str::from_utf8(&b[start..q]).unwrap_or("").to_string();
    if q < b.len() && b[q] == b')' {
        q += 1;
    }
    *p = q;
    Some(arg)
}

pub(crate) fn parse_supports_atom(b: &[u8], p: &mut usize) -> SupportsCondition {
    skip_ws(b, p);
    // `font-tech( <font-tech> )` / `font-format( <font-format> )`
    // (CSS Conditional L4 §4 / CSS Fonts L4 §4.3). Один ident-аргумент;
    // у `font-format` допустим legacy-строковый синтаксис (кавычки снимаем).
    if let Some(arg) = match_func_arg(b, p, b"font-tech(") {
        return SupportsCondition::FontTech(arg.trim().to_ascii_lowercase());
    }
    if let Some(arg) = match_func_arg(b, p, b"font-format(") {
        let unquoted = arg.trim().trim_matches(['"', '\'']).trim();
        return SupportsCondition::FontFormat(unquoted.to_ascii_lowercase());
    }
    // `selector( ... )`
    let saved = *p;
    if *p + 9 <= b.len() && b[*p..*p + 9].eq_ignore_ascii_case(b"selector(") {
        *p += 9;
        let start = *p;
        let mut depth: i32 = 1;
        while *p < b.len() && depth > 0 {
            match b[*p] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            *p += 1;
        }
        let sel_str = std::str::from_utf8(&b[start..*p]).unwrap_or("").trim().to_string();
        if *p < b.len() && b[*p] == b')' {
            *p += 1;
        }
        return SupportsCondition::Selector(sel_str);
    }
    *p = saved;
    if *p < b.len() && b[*p] == b'(' {
        *p += 1;
        // Содержимое: может быть `<expr>` (nested condition) или
        // `<prop>: <value>`. Различаем по наличию `:` на верхнем уровне.
        let inner_start = *p;
        let mut depth: i32 = 1;
        while *p < b.len() && depth > 0 {
            match b[*p] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            *p += 1;
        }
        let inner = std::str::from_utf8(&b[inner_start..*p]).unwrap_or("");
        if *p < b.len() && b[*p] == b')' {
            *p += 1;
        }
        // Determine: declaration or nested condition. Top-level `:`?
        let inner_t = inner.trim();
        let mut colon_pos: Option<usize> = None;
        let inner_bytes = inner_t.as_bytes();
        let mut d: i32 = 0;
        for (i, &c) in inner_bytes.iter().enumerate() {
            match c {
                b'(' => d += 1,
                b')' => d -= 1,
                b':' if d == 0 => {
                    colon_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(idx) = colon_pos {
            let property = inner_t[..idx].trim().to_string();
            let value = inner_t[idx + 1..].trim().to_string();
            if property.is_empty() {
                return SupportsCondition::Unknown;
            }
            return SupportsCondition::Decl { property, value };
        }
        return parse_supports_condition(inner_t);
    }
    SupportsCondition::Unknown
}

impl<'a> Parser<'a> {
    /// Распознаёт `@property --name { ... }` (CSS Properties and Values L1
    /// §1.1) и `@media <query> { <rules> }` (Media Queries L4).
    /// Все прочие @-правила синтаксически пропускает. Сама съедает
    /// либо `;`, либо полный `{ ... }`-блок.
    pub(crate) fn parse_at_rule(&mut self) -> AtRuleOutcome {
        let start = self.pos;
        self.consume(); // '@'
        let name = self.parse_ident().unwrap_or_default();
        if name.eq_ignore_ascii_case("property") {
            return self.parse_property_body().map_or(AtRuleOutcome::None, AtRuleOutcome::Property);
        }
        if name.eq_ignore_ascii_case("media") {
            return self.parse_media_rule().map_or(AtRuleOutcome::None, AtRuleOutcome::Media);
        }
        if name.eq_ignore_ascii_case("import") {
            return self.parse_import_body().map_or(AtRuleOutcome::None, AtRuleOutcome::Import);
        }
        if name.eq_ignore_ascii_case("font-face") {
            return self
                .parse_font_face_body()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::FontFace);
        }
        if name.eq_ignore_ascii_case("font-palette-values") {
            return self
                .parse_font_palette_values_body()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::FontPaletteValues);
        }
        if name.eq_ignore_ascii_case("layer") {
            return self.parse_layer_at_rule();
        }
        if name.eq_ignore_ascii_case("supports") {
            return self
                .parse_supports_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Supports);
        }
        if name.eq_ignore_ascii_case("keyframes")
            || name.eq_ignore_ascii_case("-webkit-keyframes")
        {
            return self
                .parse_keyframes_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Keyframes);
        }
        if name.eq_ignore_ascii_case("counter-style") {
            return self
                .parse_counter_style_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::CounterStyle);
        }
        if name.eq_ignore_ascii_case("page") {
            return self
                .parse_page_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Page);
        }
        if name.eq_ignore_ascii_case("scope") {
            return self
                .parse_scope_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Scope);
        }
        if name.eq_ignore_ascii_case("starting-style") {
            return self
                .parse_starting_style_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::StartingStyle);
        }
        if name.eq_ignore_ascii_case("container") {
            return self
                .parse_container_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Container);
        }
        if name.eq_ignore_ascii_case("color-profile") {
            return self
                .parse_color_profile_body()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::ColorProfile);
        }
        if name.eq_ignore_ascii_case("function") {
            return self
                .parse_function_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Function);
        }
        // Прочее @-правило: откатимся к '@' и пропустим как раньше.
        self.pos = start;
        self.skip_at_rule();
        AtRuleOutcome::None
    }

    /// Парсит `@layer` — две формы:
    /// - **Statement-form**: `@layer base, components;` — список имён,
    ///   закрывается `;`. Регистрирует layer-имена без rules.
    /// - **Block-form**: `@layer name { rules }` или `@layer { rules }`
    ///   (анонимный). Содержит обычные rules внутри. Имя опционально.
    ///
    /// Различие — что встречается раньше: `;` (statement) или `{` (block).
    pub(crate) fn parse_layer_at_rule(&mut self) -> AtRuleOutcome {
        self.skip_ws_and_comments();
        // Собираем токены имени до `;` или `{`.
        let names_start = self.pos;
        while let Some(c) = self.peek() {
            if c == ';' || c == '{' || c == '}' {
                break;
            }
            self.consume();
        }
        let prelude = self.input[names_start..self.pos].trim();
        match self.peek() {
            Some(';') => {
                self.consume();
                // Statement-form: список имён через запятую.
                let names: Vec<String> = prelude
                    .split(',')
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty() && is_layer_name(n))
                    .collect();
                AtRuleOutcome::LayerNames(names)
            }
            Some('{') => {
                self.consume();
                // Block-form: name опционально (может быть пустым для anon),
                // парсим rules до `}`.
                let name = if prelude.is_empty() {
                    None
                } else if is_layer_name(prelude) {
                    Some(prelude.to_string())
                } else {
                    // Невалидное имя (например, со скобками или невалидными
                    // символами) — пропустим как анонимный.
                    None
                };
                let mut rules = Vec::new();
                loop {
                    self.skip_ws_and_comments();
                    match self.peek() {
                        None => break,
                        Some('}') => {
                            self.consume();
                            break;
                        }
                        Some('@') => {
                            // Nested @-правила внутри layer пока не
                            // поддерживаем — skip.
                            self.skip_at_rule();
                        }
                        Some(_) => {
                            let before = self.pos;
                            if let Some((rule, nested, _)) = self.parse_rule() {
                                rules.push(rule);
                                rules.extend(nested);
                            } else if self.pos == before {
                                self.consume();
                            }
                        }
                    }
                }
                AtRuleOutcome::LayerBlock { name, rules }
            }
            _ => AtRuleOutcome::None,
        }
    }

    /// Парсит тело `@font-face { ... }` — обычный block declarations,
    /// но с font-face-specific descriptors (font-family / src / weight /
    /// style / stretch / display / unicode-range / variant /
    /// feature-settings / variation-settings). Прочие имена игнорируются.
    pub(crate) fn parse_font_face_body(&mut self) -> Option<FontFaceRule> {
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let declarations = self.parse_declaration_block();

        let mut family: String = String::new();
        let mut src_str: Option<String> = None;
        let mut weight: Option<String> = None;
        let mut style: Option<String> = None;
        let mut stretch: Option<String> = None;
        let mut display: Option<String> = None;
        let mut unicode_range: Option<String> = None;
        let mut variant: Option<String> = None;
        let mut feature_settings: Option<String> = None;
        let mut variation_settings: Option<String> = None;

        for d in &declarations {
            let prop = d.property.to_ascii_lowercase();
            match prop.as_str() {
                "font-family" => {
                    let v = d.value.trim();
                    family = strip_css_string(v).map_or_else(|| v.to_string(), str::to_string);
                }
                "src" => src_str = Some(d.value.clone()),
                "font-weight" => weight = Some(d.value.trim().to_string()),
                "font-style" => style = Some(d.value.trim().to_string()),
                "font-stretch" => stretch = Some(d.value.trim().to_string()),
                "font-display" => display = Some(d.value.trim().to_string()),
                "unicode-range" => unicode_range = Some(d.value.trim().to_string()),
                "font-variant" => variant = Some(d.value.trim().to_string()),
                "font-feature-settings" => feature_settings = Some(d.value.trim().to_string()),
                "font-variation-settings" => variation_settings = Some(d.value.trim().to_string()),
                _ => {}
            }
        }
        if family.is_empty() {
            return None;
        }
        let sources = src_str.as_deref().map(parse_font_face_src).unwrap_or_default();
        Some(FontFaceRule {
            family,
            sources,
            weight,
            style,
            stretch,
            display,
            unicode_range,
            variant,
            feature_settings,
            variation_settings,
        })
    }

    /// Парсит `@font-palette-values --name { font-family: …; base-palette: N; override-colors: … }`.
    /// CSS Fonts L4 §13. Prelude — dashed-ident (e.g. `--cool`). Block contains
    /// descriptors: `font-family`, `base-palette` (u16 index), `override-colors`
    /// (comma-separated `<index> <color>` pairs). Returns `None` if the
    /// name is missing or no `{` follows.
    pub(crate) fn parse_font_palette_values_body(&mut self) -> Option<FontPaletteValuesRule> {
        self.skip_ws_and_comments();
        // Prelude: dashed-ident starting with '--'
        let name = self.parse_ident()?;
        if !name.starts_with("--") {
            self.skip_until_block_end();
            return None;
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();

        let mut font_family: Option<String> = None;
        let mut base_palette: Option<u16> = None;
        let mut override_colors: Vec<(u16, String)> = Vec::new();

        for d in &declarations {
            match d.property.to_ascii_lowercase().as_str() {
                "font-family" => {
                    let v = d.value.trim();
                    font_family =
                        Some(strip_css_string(v).map_or_else(|| v.to_string(), str::to_string));
                }
                "base-palette" => {
                    base_palette = d.value.trim().parse::<u16>().ok();
                }
                "override-colors" => {
                    override_colors = parse_override_colors(d.value.trim());
                }
                _ => {}
            }
        }
        Some(FontPaletteValuesRule {
            name,
            font_family,
            base_palette,
            override_colors,
        })
    }

    /// Парсит `@color-profile --name { src: url(...); rendering-intent: ...; }`.
    /// CSS Color L5 §4. Prelude — dashed-ident (e.g. `--swop5c`). Block contains
    /// descriptors: `src` (URL, via `parse_import_url`), `rendering-intent`
    /// (keyword, stored raw). Returns `None` if the name is missing or no `{`
    /// follows.
    pub(crate) fn parse_color_profile_body(&mut self) -> Option<ColorProfileRule> {
        self.skip_ws_and_comments();
        // Prelude: dashed-ident starting with '--'
        let name = self.parse_ident()?;
        if !name.starts_with("--") {
            self.skip_until_block_end();
            return None;
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();

        let mut src: Option<String> = None;
        let mut rendering_intent: Option<String> = None;

        for d in &declarations {
            match d.property.to_ascii_lowercase().as_str() {
                "src" => {
                    src = Parser::new(d.value.trim()).parse_import_url();
                }
                "rendering-intent" => {
                    rendering_intent = Some(d.value.trim().to_ascii_lowercase());
                }
                _ => {}
            }
        }
        Some(ColorProfileRule {
            name,
            src,
            rendering_intent,
        })
    }

    /// Парсит `@function <name>(<params>) [returns <type>]? { decls }` — CSS
    /// Functions and Mixins L1. Prelude — dashed-ident сразу (без пробела,
    /// function-token grammar) за которым следует `(`. Параметры — список
    /// `--param [: <default>]` через запятую (`--foo()` — пустой список).
    /// Опциональный `returns <type>` перед `{` хранится сырой строкой, без
    /// типизации. Возвращает `None`, если prelude не dashed-ident-function-
    /// token или блок `{ ... }` отсутствует.
    pub(crate) fn parse_function_rule(&mut self) -> Option<FunctionRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        if !name.starts_with("--") || self.peek() != Some('(') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '('
        let params_str = self.read_balanced_parens()?;
        let parameters: Vec<FunctionParameter> = split_top_level_commas(&params_str)
            .into_iter()
            .filter_map(|raw| {
                let raw = raw.trim();
                if raw.is_empty() {
                    return None;
                }
                let param = match raw.split_once(':') {
                    Some((n, default)) => FunctionParameter {
                        name: n.trim().to_string(),
                        default: Some(default.trim().to_string()),
                    },
                    None => FunctionParameter { name: raw.to_string(), default: None },
                };
                param.name.starts_with("--").then_some(param)
            })
            .collect();

        self.skip_ws_and_comments();
        let mut returns = None;
        if self.skip_optional_returns_keyword() {
            self.skip_ws_and_comments();
            let type_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' {
                    break;
                }
                self.consume();
            }
            let raw_type = self.input[type_start..self.pos].trim();
            if !raw_type.is_empty() {
                returns = Some(raw_type.to_string());
            }
        }

        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        Some(FunctionRule { name, parameters, returns, declarations })
    }

    /// Читает содержимое между уже открытой `(` (позиция парсера сразу
    /// после неё) и парной закрывающей скобкой, съедая закрывающую. Учитывает
    /// вложенные `(...)` и строковые литералы (`)`/`(` внутри строк не меняют
    /// depth). Возвращает `None`, если EOF наступил раньше закрывающей скобки.
    pub(crate) fn read_balanced_parens(&mut self) -> Option<String> {
        let mut depth = 1u32;
        let mut in_string: Option<char> = None;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            match (in_string, c) {
                (Some(q), ch) if ch == q => {
                    in_string = None;
                    out.push(ch);
                    self.consume();
                }
                (None, '"') | (None, '\'') => {
                    in_string = Some(c);
                    out.push(c);
                    self.consume();
                }
                (None, '(') => {
                    depth += 1;
                    out.push(c);
                    self.consume();
                }
                (None, ')') => {
                    depth -= 1;
                    self.consume();
                    if depth == 0 {
                        return Some(out);
                    }
                    out.push(')');
                }
                _ => {
                    out.push(c);
                    self.consume();
                }
            }
        }
    }

    /// Если позиция стоит на слове `returns` (case-insensitive), за которым
    /// НЕ следует ident-continuation байт, продвигает позицию за это слово
    /// и возвращает `true`. Иначе — не трогает позицию, возвращает `false`.
    pub(crate) fn skip_optional_returns_keyword(&mut self) -> bool {
        let bytes = self.input.as_bytes();
        let p = self.pos;
        if p + 7 > bytes.len() || !bytes[p..p + 7].eq_ignore_ascii_case(b"returns") {
            return false;
        }
        if let Some(&c) = bytes.get(p + 7)
            && (c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return false;
        }
        self.pos += 7;
        true
    }

    /// Парсит тело `@import url("...") [<media-query>];` или
    /// `@import "..." [<media-query>];`. Заканчивается на `;` (имеет
    /// statement-form, не блочную). Возвращает None если синтаксис
    /// нарушен; в любом случае съедает до `;` (или EOF).
    pub(crate) fn parse_import_body(&mut self) -> Option<ImportRule> {
        self.skip_ws_and_comments();
        // URL: либо `url("...")` / `url('...')` / `url(...)`, либо просто `"..."` / `'...'`.
        let url = self.parse_import_url()?;
        self.skip_ws_and_comments();
        // Опциональный media-query до `;`.
        let media_start = self.pos;
        while let Some(c) = self.peek() {
            if c == ';' || c == '}' || c == '{' {
                break;
            }
            self.consume();
        }
        let media_str = self.input[media_start..self.pos].trim();
        let media = parse_media_query(media_str);
        // Сжираем `;` если есть.
        if self.peek() == Some(';') {
            self.consume();
        }
        Some(ImportRule { url, media })
    }

    /// Парсит URL для `@import` — `url("...")`, `url(...)`, или `"..."`/`'...'`.
    /// Позиция после успешного парсинга стоит ПОСЛЕ закрывающей кавычки/скобки.
    pub(crate) fn parse_import_url(&mut self) -> Option<String> {
        let rest = self.rest();
        if let Some(after) = rest.strip_prefix("url(") {
            // Внутри parentheses: опц. quoted-string или unquoted-URL.
            let close_idx = after.find(')')?;
            let inner = &after[..close_idx];
            let url = inner.trim().trim_matches(['"', '\''].as_ref()).to_string();
            self.pos += 4 + close_idx + 1;
            return Some(url);
        }
        // Plain string без url().
        match self.peek()? {
            '"' | '\'' => {
                let quote = self.consume()?;
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == quote {
                        break;
                    }
                    self.consume();
                }
                if self.peek() != Some(quote) {
                    return None;
                }
                let url = self.input[start..self.pos].to_string();
                self.consume();
                Some(url)
            }
            _ => None,
        }
    }

    /// Парсит тело `@media <query> { <rules> }`. Грамматика query
    /// упрощённая: type-or-feature [and type-or-feature]* [, ...].
    /// Type-or-feature — ident (`screen`/`print`/...) или
    /// `(feature: value)`. Возвращает None если синтаксис не позволяет
    /// дойти до `{`; в этом случае откатывает позицию до конца блока
    /// чтобы стабильно продолжить парсинг stylesheet.
    pub(crate) fn parse_media_rule(&mut self) -> Option<MediaRule> {
        self.skip_ws_and_comments();
        // Собираем query-string до `{`.
        let query_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '{' {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            return None;
        }
        let query_str = self.input[query_start..self.pos].trim();
        let query = parse_media_query(query_str);
        // Тело: рекурсивно парсим как обычные rules.
        self.consume(); // '{'
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила в media пока не поддерживаем — skip.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(MediaRule { query, rules })
    }

    /// Парсит тело `@supports <condition> { rules }` — CSS Conditional Rules L3 §2.
    /// Берёт сырую condition-строку до `{` (с балансировкой `(`/`)`),
    /// затем парсит её через [`parse_supports_condition`]. Тело — обычные
    /// rules до `}`. Возвращает `None` если структура нарушена.
    pub(crate) fn parse_supports_rule(&mut self) -> Option<SupportsRule> {
        self.skip_ws_and_comments();
        let cond_start = self.pos;
        let mut depth: i32 = 0;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            } else if c == '{' && depth == 0 {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            return None;
        }
        let cond_str = self.input[cond_start..self.pos].trim();
        let condition = parse_supports_condition(cond_str);
        self.consume(); // '{'
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила внутри @supports пока skip.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(SupportsRule { condition, rules })
    }

    /// Парсит тело `@keyframes <name> { <frame>* }` — CSS Animations L1 §3.
    /// Frame-selector: `from` / `to` / `<percentage>`. Поддерживается
    /// `0%, 50% { ... }` (одна frame с несколькими offset-ами,
    /// разворачивается в две записи). `name` — CSS-ident.
    pub(crate) fn parse_keyframes_rule(&mut self) -> Option<KeyframesRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let mut frames: Vec<Keyframe> = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    // Nested @-правила внутри @keyframes по spec не разрешены.
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    let frame_selector_start = self.pos;
                    while let Some(c) = self.peek() {
                        if c == '{' || c == '}' {
                            break;
                        }
                        self.consume();
                    }
                    if self.peek() != Some('{') {
                        if self.pos == before {
                            self.consume();
                        }
                        continue;
                    }
                    let selector_str = self.input[frame_selector_start..self.pos].trim();
                    self.consume(); // '{'
                    let declarations = self.parse_declaration_block();
                    let offsets = parse_keyframe_selectors(selector_str);
                    for offset in offsets {
                        frames.push(Keyframe {
                            offset,
                            declarations: declarations.clone(),
                        });
                    }
                }
            }
        }
        Some(KeyframesRule { name, frames })
    }

    /// Парсит `@counter-style <name> { <descriptors> }` — CSS Counter Styles L3 §2.
    /// Descriptors хранятся как обычные declarations.
    pub(crate) fn parse_counter_style_rule(&mut self) -> Option<CounterStyleRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let declarations = self.parse_declaration_block();
        Some(CounterStyleRule { name, declarations })
    }

    /// Парсит `@page <selector>? { <decls> }` — CSS Paged Media L3 §3.
    /// Selector сохраняется как сырая строка (`:first`, `:left`, имя
    /// страницы, и т.д.). Пустой selector — любая страница.
    pub(crate) fn parse_page_rule(&mut self) -> Option<PageRule> {
        self.skip_ws_and_comments();
        let sel_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '{' || c == ';' {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            // `@page <prelude>;` без блока — не валидно для CSS Paged Media.
            if self.peek() == Some(';') {
                self.consume();
            }
            return None;
        }
        let selector = self.input[sel_start..self.pos].trim().to_string();
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        Some(PageRule {
            selector,
            declarations,
        })
    }

    /// Парсит `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6.
    /// Root и limit — сырые строки селекторов (без обрамляющих `(`/`)`).
    /// Без `(<root>)` — implicit scope (root = пустая строка).
    /// Парсит прелюдию `@scope` — `(<root>)? [to (<limit>)]?` (CSS Cascade L6 §3).
    /// Возвращает сырой селектор корня (`String`; пустая строка = отсутствует
    /// `(<root>)`, implicit `:scope`) и опциональный сырой селектор limit из
    /// `to (<limit>)`. Курсор остаётся на первом токене после прелюдии (обычно
    /// `{`). Общий код для [`Self::parse_scope_rule`] (top-level) и ветки
    /// `@scope` в [`Self::parse_nested_at_rule`] (nested).
    pub(crate) fn parse_scope_prelude(&mut self) -> (String, Option<String>) {
        self.skip_ws_and_comments();
        let mut root = String::new();
        let mut limit: Option<String> = None;
        // Опциональный `(<root>)`.
        if self.peek() == Some('(') {
            self.consume();
            let start = self.pos;
            let mut depth: i32 = 1;
            while let Some(c) = self.peek() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                self.consume();
            }
            root = self.input[start..self.pos].trim().to_string();
            if self.peek() == Some(')') {
                self.consume();
            }
        }
        self.skip_ws_and_comments();
        // Опциональный `to (<limit>)`.
        if self.rest().to_ascii_lowercase().starts_with("to") {
            // Граница: следующий после `to` — не ident-char.
            let after = self.pos + 2;
            let ok = self.input.as_bytes().get(after).is_none_or(|&c| {
                !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            });
            if ok {
                self.pos = after;
                self.skip_ws_and_comments();
                if self.peek() == Some('(') {
                    self.consume();
                    let start = self.pos;
                    let mut depth: i32 = 1;
                    while let Some(c) = self.peek() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        self.consume();
                    }
                    limit = Some(self.input[start..self.pos].trim().to_string());
                    if self.peek() == Some(')') {
                        self.consume();
                    }
                }
            }
        }
        (root, limit)
    }

    pub(crate) fn parse_scope_rule(&mut self) -> Option<ScopeRule> {
        let (root, limit) = self.parse_scope_prelude();
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            return None;
        }
        self.consume();
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(ScopeRule {
            root,
            limit,
            rules,
        })
    }

    /// Парсит прелюдию `@container` — `<name>? <condition>` (CSS Containment L3
    /// §3). Имя — опциональный CSS-ident перед условием (только если дальше не
    /// `(` и не `style(`). Condition — сырая балансированная строка до `{`.
    /// Курсор остаётся на `{`. Возвращает `None`, если `{` не найден (структура
    /// нарушена). Общий код для [`Self::parse_container_rule`] (top-level) и
    /// ветки `@container` в [`Self::parse_nested_at_rule`] (nested).
    pub(crate) fn parse_container_prelude(&mut self) -> Option<(Option<String>, String)> {
        self.skip_ws_and_comments();
        // Опциональное имя: CSS-ident **только если** дальше не `(` —
        // если сразу `(`, это начало condition без имени. `style(...)` — тоже
        // condition, а не имя.
        let name = if self.peek() != Some('(') && !self.starts_with_keyword("style") {
            self.parse_ident()
        } else {
            None
        };
        self.skip_ws_and_comments();
        // Condition: всё до `{` с учётом баланса `()`.
        let cond_start = self.pos;
        let mut depth: i32 = 0;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
            } else if c == '{' && depth == 0 {
                break;
            }
            self.consume();
        }
        if self.peek() != Some('{') {
            return None;
        }
        let condition = self.input[cond_start..self.pos].trim().to_string();
        Some((name, condition))
    }

    /// Парсит `@container <name>? <condition> { rules }` — CSS Containment L3 §3.
    /// Name — опциональный CSS-ident перед условием. Condition — балансированная
    /// строка до `{` (хранится сырой). Rules — обычные правила внутри. Вложенные
    /// at-rules в теле (`@media`, `@supports`, `@layer`, `@container`, `@scope`)
    /// парсятся рекурсивно и всплывают в stylesheet через [`Self::bubbled`]
    /// (плоская модель — container-condition к ним не привязывается, как и для
    /// at-rule-in-at-rule в [`Self::parse_declaration_block_with_nesting`]).
    pub(crate) fn parse_container_rule(&mut self) -> Option<ContainerRule> {
        let (name, condition) = self.parse_container_prelude()?;
        self.consume(); // '{'
        let (rules, bubbled) = self.parse_bare_group_body();
        self.bubbled.extend(bubbled);
        Some(ContainerRule {
            name,
            condition,
            rules,
        })
    }

    /// Проверяет, начинается ли остаток с ключевого слова (case-insensitive)
    /// + не-ident разделитель. Используется для container `style(...)`.
    pub(crate) fn starts_with_keyword(&self, kw: &str) -> bool {
        let rest = self.rest();
        if !rest.to_ascii_lowercase().starts_with(kw) {
            return false;
        }
        rest.as_bytes()
            .get(kw.len())
            .is_none_or(|&c| !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
    }

    /// Парсит `@starting-style { rules }` — CSS Transitions L2 §3.4.
    pub(crate) fn parse_starting_style_rule(&mut self) -> Option<StartingStyleRule> {
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let mut rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    self.skip_at_rule();
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, _)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        Some(StartingStyleRule { rules })
    }

    /// Парсит тело `@property`: имя `--name`, блок `{ ... }`, обязательные
    /// дескрипторы. Возвращает None если синтаксис нарушен или нет
    /// обязательных полей. В любом исходе позиция остаётся после `}`
    /// (или после `;` если блока не было, или EOF).
    pub(crate) fn parse_property_body(&mut self) -> Option<PropertyRule> {
        self.skip_ws_and_comments();
        // Имя должно начинаться с `--`.
        if !self.rest().starts_with("--") {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        self.consume();
        let tail = self.parse_ident().unwrap_or_default();
        if tail.is_empty() {
            self.skip_until_block_end();
            return None;
        }
        let name = format!("--{tail}");
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume();
        let declarations = self.parse_declaration_block();

        // Извлекаем три обязательных дескриптора. Любые другие имена в теле
        // @property спецификацией не определены; их игнорируем (forward-compat).
        let mut syntax: Option<String> = None;
        let mut inherits: Option<bool> = None;
        let mut initial_value: Option<String> = None;
        for d in &declarations {
            let prop = d.property.to_ascii_lowercase();
            match prop.as_str() {
                "syntax" => {
                    // value — CSS-string в одиночных или двойных кавычках.
                    if let Some(stripped) = strip_css_string(d.value.trim()) {
                        syntax = Some(stripped.to_string());
                    }
                }
                "inherits" => {
                    let v = d.value.trim().to_ascii_lowercase();
                    if v == "true" {
                        inherits = Some(true);
                    } else if v == "false" {
                        inherits = Some(false);
                    }
                }
                "initial-value" => {
                    initial_value = Some(d.value.trim().to_string());
                }
                _ => {}
            }
        }

        let syntax = syntax?;
        let inherits = inherits?;
        // CSS Properties and Values L1 §1.1: если syntax не universal,
        // initial-value обязателен. В Phase 0 поддерживаем только syntax="*",
        // но валидируем по спеке — чужой syntax без initial-value invalid.
        if syntax != "*" && initial_value.is_none() {
            return None;
        }
        Some(PropertyRule {
            name,
            syntax,
            inherits,
            initial_value,
        })
    }

    /// Пропускает до конца `@-rule`-тела: либо `;`, либо `{ ... }` целиком.
    /// Используется при синтаксической ошибке внутри @property — потребитель
    /// не должен ловить declarations этого правила.
    pub(crate) fn skip_until_block_end(&mut self) {
        while let Some(c) = self.peek() {
            if c == '{' {
                self.consume();
                self.skip_block();
                return;
            }
            if c == ';' {
                self.consume();
                return;
            }
            self.consume();
        }
    }

    pub(crate) fn skip_at_rule(&mut self) {
        self.consume(); // '@'
        while let Some(c) = self.peek() {
            match c {
                ';' => {
                    self.consume();
                    return;
                }
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    pub(crate) fn skip_block(&mut self) {
        let mut depth = 1;
        while let Some(c) = self.peek() {
            match c {
                '{' => {
                    self.consume();
                    depth += 1;
                }
                '}' => {
                    self.consume();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

}

/// Снимает с CSS-string значения (`"..."` или `'...'`) обрамляющие кавычки.
/// Возвращает None если значение не строковый литерал. Используется для
/// дескриптора `syntax` в `@property` (он обязан быть строкой по spec L1 §1.1).
/// Внутренние escape-последовательности (`\xNN`, `\<newline>`) не
/// поддерживаются — в Phase 0 syntax всегда `"*"`, и более сложные формы
/// (`"<length>"`, `"<color>"`) будут идти через тот же путь без escape-ов.
pub(crate) fn strip_css_string(v: &str) -> Option<&str> {
    let bytes = v.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0];
    if (q == b'"' || q == b'\'') && bytes[bytes.len() - 1] == q {
        Some(&v[1..v.len() - 1])
    } else {
        None
    }
}
