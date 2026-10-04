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
/// `color(--name c1 c2 c3)`. The descriptors are parsed here; the profile bytes
/// are fetched by the embedder (the parser does no I/O) and attached with
/// [`crate::Stylesheet::load_color_profiles`], after which layout compiles an
/// ICC transform from them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorProfileRule {
    /// Dashed-ident name, e.g. `--swop5c`. Used to match `color(--name ...)` values.
    pub name: String,
    /// `src` descriptor — URL of the ICC profile resource.
    pub src: Option<String>,
    /// `rendering-intent` descriptor — one of `relative-colorimetric` (default),
    /// `absolute-colorimetric`, `perceptual`, `saturation`. Parsed and stored;
    /// the transform always uses the profile's colorimetric path.
    pub rendering_intent: Option<String>,
    /// Raw bytes of the fetched ICC profile. `None` until the embedder loads
    /// it (or when the fetch failed) — CSS Color L5 §5.3: a colour referencing
    /// a profile that "has not loaded" is an invalid colour.
    pub data: Option<std::sync::Arc<Vec<u8>>>,
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

/// `navigation` descriptor of `@view-transition` — CSS View Transitions
/// Module Level 2 §3. `Auto` opts the document in to cross-document (MPA)
/// view transitions; the initial/default value is `None` (no opt-in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewTransitionNavigation {
    #[default]
    None,
    Auto,
}

/// `@view-transition { navigation: auto | none; }` — CSS View Transitions
/// Module Level 2 §3. No prelude (unlike `@page`/`@counter-style`); the
/// block holds a single `navigation` descriptor consumed by the shell's
/// cross-document navigation pipeline (`docs/tasks/ph3-view-transitions-mpa.md`
/// срез 2) to decide whether a same-origin navigation should snapshot the
/// departing document and cross-fade into the arriving one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewTransitionRule {
    pub navigation: ViewTransitionNavigation,
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

/// `@layer name { rules }` блок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerRule {
    /// Имя layer-а. Анонимный блок (`@layer { ... }`) получает имя
    /// `__anon_<n>__` где `n` — порядковый номер. Вложенный layer хранится
    /// под полным dotted-именем (`outer.inner`, Cascade L5 §6.4.2).
    pub name: String,
    pub rules: Vec<Rule>,
    /// `Some` — это правила условной группы (`@media`/`@supports`) внутри
    /// layer-а: они участвуют в каскаде этого layer-а, только пока условие
    /// истинно. `None` — безусловные правила блока.
    pub condition: Option<LayerCondition>,
}

/// Условие группы правил внутри `@layer` — см. [`LayerRule::condition`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerCondition {
    Media(MediaQuery),
    Supports(SupportsCondition),
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
    /// `layer` / `layer(<name>)` — CSS Cascade L5 §6.5: правила импортируемого
    /// листа попадают в указанный (или анонимный) cascade layer. `None` — без
    /// модификатора, правила остаются unlayered.
    pub layer: Option<ImportLayer>,
    /// `supports(<condition>)` — CSS Cascade L5 §6.5: импорт применяется,
    /// только если условие истинно (вычисляет вызывающая сторона через
    /// [`SupportsCondition::evaluate`]). `None` — модификатора нет.
    pub supports: Option<SupportsCondition>,
}

/// Значение модификатора `layer` у `@import` (CSS Cascade L5 §6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportLayer {
    /// Голый `layer` — анонимный layer.
    Anonymous,
    /// `layer(<name>)` — именованный layer (имя может быть dotted).
    Named(String),
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
    /// `ascent-override: normal | <percentage>` — CSS Fonts L4 §14.1. Сырая строка.
    pub ascent_override: Option<String>,
    /// `descent-override: normal | <percentage>` — CSS Fonts L4 §14.2. Сырая строка.
    pub descent_override: Option<String>,
    /// `line-gap-override: normal | <percentage>` — CSS Fonts L4 §14.3. Сырая строка.
    pub line_gap_override: Option<String>,
    /// `size-adjust: <percentage>` — CSS Fonts L4 §14.4. Сырая строка.
    pub size_adjust: Option<String>,
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
    FontFace(Box<FontFaceRule>),
    FontPaletteValues(FontPaletteValuesRule),
    LayerNames(Vec<String>),
    LayerBlock {
        name: Option<String>,
        rules: Vec<Rule>,
        /// `@mixin` rules found directly inside this `@layer` block's body
        /// (BUG-518 срез 3, `mixin-layers.html`) — the caller stamps each
        /// with this layer's resolved name (`MixinRule::layer`) and folds it
        /// into the stylesheet's flat `mixin_rules`, the same place a
        /// top-level `@mixin` lands. Every other nested at-rule kind inside
        /// `@layer` remains unsupported (pre-existing gap, unrelated to this
        /// bug — see `parse_layer_at_rule`'s doc comment).
        mixin_rules: Vec<MixinRule>,
        /// Прочее содержимое блока: `@media`/`@supports` (остаются за этим
        /// layer-ом), вложенные `@layer` (имена относительные — префиксуются
        /// именем внешнего) и layer-независимые at-rules (`@font-face`,
        /// `@keyframes`, `@property`, …), которые вызывающая сторона
        /// поднимает на верхний уровень. Нужно для `@import … layer(x)`:
        /// импортируемый лист оборачивается в `@layer x { … }` и не должен
        /// терять свои `@font-face`/`@keyframes`/`@property`.
        nested: Vec<AtRuleOutcome>,
    },
    Supports(SupportsRule),
    Keyframes(KeyframesRule),
    CounterStyle(CounterStyleRule),
    Page(PageRule),
    Scope(ScopeRule),
    StartingStyle(StartingStyleRule),
    Container(ContainerRule),
    ViewTransition(ViewTransitionRule),
    ColorProfile(ColorProfileRule),
    Function(FunctionRule),
    Mixin(MixinRule),
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

/// Снимает `(`-группу с начала `s` (который начинается сразу ПОСЛЕ `(`):
/// возвращает `(содержимое, остаток после закрывающей ')')`. Скобки внутри
/// строк в кавычках не считаются.
fn take_paren_group(s: &str) -> Option<(&str, &str)> {
    let mut depth = 1usize;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((&s[..i], &s[i + 1..]));
                    }
                }
                _ => {}
            },
        }
    }
    None
}

/// Разбор прелюдии `@import` после URL (CSS Cascade L5 §6.5):
/// `[layer | layer(<name>)]? [supports(<condition>)]? <media-query-list>?`.
/// Возвращает `(layer, supports, остаток-media)`; `None` — прелюдия невалидна
/// (пустое/некорректное имя layer, незакрытая скобка), импорт отбрасывается.
fn parse_import_prelude(
    prelude: &str,
) -> Option<(Option<ImportLayer>, Option<SupportsCondition>, &str)> {
    let mut rest = prelude.trim_start();
    let mut layer = None;
    let mut supports = None;
    if let Some(tail) = strip_prefix_ci(rest, "layer") {
        if let Some(inner_start) = tail.strip_prefix('(') {
            let (inner, after) = take_paren_group(inner_start)?;
            let name = inner.trim();
            if !is_layer_name(name) {
                return None;
            }
            layer = Some(ImportLayer::Named(name.to_string()));
            rest = after.trim_start();
        } else if tail.is_empty() || tail.starts_with(char::is_whitespace) {
            layer = Some(ImportLayer::Anonymous);
            rest = tail.trim_start();
        }
    }
    if let Some(tail) = strip_prefix_ci(rest, "supports(") {
        let (inner, after) = take_paren_group(tail)?;
        let inner = inner.trim();
        // `supports(display: grid)` — голая декларация; всё остальное
        // (`(…)`, `not …`, `selector(…)`) — уже `<supports-condition>`.
        let is_declaration = match (inner.find(':'), inner.find('(')) {
            (Some(colon), Some(paren)) => colon < paren,
            (Some(_), None) => true,
            _ => false,
        };
        supports = Some(if is_declaration {
            parse_supports_condition(&format!("({inner})"))
        } else {
            parse_supports_condition(inner)
        });
        rest = after.trim_start();
    }
    Some((layer, supports, rest))
}

/// `s.strip_prefix(prefix)` без учёта ASCII-регистра.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
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
                .map_or(AtRuleOutcome::None, |f| AtRuleOutcome::FontFace(Box::new(f)));
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
        if name.eq_ignore_ascii_case("mixin") {
            return self
                .parse_mixin_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::Mixin);
        }
        if name.eq_ignore_ascii_case("view-transition") {
            return self
                .parse_view_transition_rule()
                .map_or(AtRuleOutcome::None, AtRuleOutcome::ViewTransition);
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
                let mut mixin_rules = Vec::new();
                let mut nested = Vec::new();
                loop {
                    self.skip_ws_and_comments();
                    match self.peek() {
                        None => break,
                        Some('}') => {
                            self.consume();
                            break;
                        }
                        Some('@') => {
                            // Nested at-rules inside `@layer` are largely
                            // unsupported still (pre-existing gap, out of
                            // scope here) — but `@mixin` is special-cased
                            // (BUG-518 срез 3, `mixin-layers.html`: a mixin
                            // must be findable and layer-priority-ranked by
                            // `@apply` even when declared inside `@layer`).
                            // `parse_at_rule` fully consumes the rule either
                            // way (recognized or not), so discarding every
                            // other outcome here is exactly as
                            // position-correct as the old blanket
                            // `skip_at_rule()`.
                            match self.parse_at_rule() {
                                AtRuleOutcome::Mixin(m) => mixin_rules.push(m),
                                // Layer-независимые at-rules поднимаются на
                                // верхний уровень; `@media`/`@supports` и
                                // вложенные `@layer` остаются за layer-ом
                                // (см. `LayerState::register`).
                                // `@container`/`@scope` внутри `@layer`
                                // по-прежнему отбрасываются: у них нет
                                // layer-привязки в каскаде.
                                o @ (AtRuleOutcome::Property(_)
                                | AtRuleOutcome::FontFace(_)
                                | AtRuleOutcome::FontPaletteValues(_)
                                | AtRuleOutcome::Keyframes(_)
                                | AtRuleOutcome::CounterStyle(_)
                                | AtRuleOutcome::Page(_)
                                | AtRuleOutcome::ColorProfile(_)
                                | AtRuleOutcome::Function(_)
                                | AtRuleOutcome::ViewTransition(_)
                                | AtRuleOutcome::LayerNames(_)
                                | AtRuleOutcome::LayerBlock { .. }
                                | AtRuleOutcome::Media(_)
                                | AtRuleOutcome::Supports(_)) => nested.push(o),
                                _ => {}
                            }
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
                AtRuleOutcome::LayerBlock { name, rules, mixin_rules, nested }
            }
            _ => AtRuleOutcome::None,
        }
    }

    /// Парсит тело `@font-face { ... }` — обычный block declarations,
    /// но с font-face-specific descriptors (font-family / src / weight /
    /// style / stretch / display / unicode-range / variant /
    /// feature-settings / variation-settings / ascent-override /
    /// descent-override / line-gap-override / size-adjust). Прочие имена
    /// игнорируются.
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
        let mut ascent_override: Option<String> = None;
        let mut descent_override: Option<String> = None;
        let mut line_gap_override: Option<String> = None;
        let mut size_adjust: Option<String> = None;

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
                "ascent-override" => ascent_override = Some(d.value.trim().to_string()),
                "descent-override" => descent_override = Some(d.value.trim().to_string()),
                "line-gap-override" => line_gap_override = Some(d.value.trim().to_string()),
                "size-adjust" => size_adjust = Some(d.value.trim().to_string()),
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
            ascent_override,
            descent_override,
            line_gap_override,
            size_adjust,
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
            data: None,
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
        // Прелюдия до `;`: `[layer | layer(<name>)]? [supports(<cond>)]? <media>?`.
        let prelude_start = self.pos;
        while let Some(c) = self.peek() {
            if c == ';' || c == '}' || c == '{' {
                break;
            }
            self.consume();
        }
        let prelude = self.input[prelude_start..self.pos].trim();
        // Сжираем `;` если есть.
        if self.peek() == Some(';') {
            self.consume();
        }
        let (layer, supports, media_str) = parse_import_prelude(prelude)?;
        let media = parse_media_query(media_str);
        Some(ImportRule { url, media, layer, supports })
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
        // Собираем query-string до `{`. CSS Syntax L3 §5.4.2: `;` на верхнем
        // уровне прелюдии (вне скобок/строк) завершает at-правило без блока —
        // BUG-793: без этой остановки скан проглатывал `;` как часть
        // prelude и уходил искать `{` дальше, забирая тело следующего
        // правила себе.
        let query_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '{' || c == ';' {
                break;
            }
            self.consume();
        }
        if self.peek() == Some(';') {
            self.consume();
            return None;
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

    /// Парсит тело `@keyframes <name> { <frame>* }` — CSS Animations L1 §3.
    /// Frame-selector: `from` / `to` / `<percentage>`. Поддерживается
    /// `0%, 50% { ... }` (одна frame с несколькими offset-ами,
    /// разворачивается в две записи). `name` — CSS-ident.
    pub(crate) fn parse_keyframes_rule(&mut self) -> Option<KeyframesRule> {
        self.skip_ws_and_comments();
        // BUG-793: a missing name (`@keyframes;`) must still consume the
        // rule up to its terminator (`;` or a `{…}` block it has no business
        // owning) — `parse_ident()` alone leaves `pos` untouched on failure,
        // which used to strand the parser right before the `;`, corrupting
        // the top-level recovery for everything after it.
        let Some(name) = self.parse_ident() else {
            self.skip_until_block_end();
            return None;
        };
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

    /// Парсит `@view-transition { navigation: auto | none; }` — CSS View
    /// Transitions Module Level 2 §3. No prelude — the block goes straight
    /// after the at-keyword. Unknown descriptors and unrecognized
    /// `navigation` values are ignored (lenient, like every other at-rule
    /// descriptor block here); a missing `navigation` descriptor keeps the
    /// spec default (`None`, no opt-in).
    pub(crate) fn parse_view_transition_rule(&mut self) -> Option<ViewTransitionRule> {
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let declarations = self.parse_declaration_block();
        let mut navigation = ViewTransitionNavigation::None;
        for d in &declarations {
            if d.property.eq_ignore_ascii_case("navigation") {
                let v = d.value.trim();
                if v.eq_ignore_ascii_case("auto") {
                    navigation = ViewTransitionNavigation::Auto;
                } else if v.eq_ignore_ascii_case("none") {
                    navigation = ViewTransitionNavigation::None;
                }
            }
        }
        Some(ViewTransitionRule { navigation })
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
        // BUG-793: `@scope;` has no block — consume the terminating `;`
        // (CSS Syntax L3 §5.4.2) instead of leaving `pos` right before it,
        // which used to make the caller's prelude-scan for the *next*
        // at-rule swallow this `;` as its own content.
        if self.peek() == Some(';') {
            self.consume();
            return None;
        }
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
            } else if c == ';' && depth == 0 {
                // BUG-793: `@container;`/`@container name;` has no block —
                // stop at the top-level `;` (CSS Syntax L3 §5.4.2) instead
                // of scanning past it into the next rule's `{…}`.
                break;
            }
            self.consume();
        }
        if self.peek() == Some(';') {
            self.consume();
            return None;
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
