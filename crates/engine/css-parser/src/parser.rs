//! CSS-парсер (Phase 0+).
//!
//! Поддерживается:
//!   - правила `selector_list { decl_list }`;
//!   - simple selectors: type / class / id / universal / attribute / pseudo-class;
//!   - compound selectors (`p.foo#bar:first-child`);
//!   - complex selectors с combinator-ами: descendant ` `, child `>`,
//!     next-sibling `+`, later-sibling `~`;
//!   - attribute selectors `[name]`, `[name=val]`, `[name~=val]`, `[name|=val]`,
//!     `[name^=val]`, `[name$=val]`, `[name*=val]`;
//!   - structural pseudo-classes:
//!       - `:first-child`, `:last-child`, `:only-child`, `:empty`, `:root`;
//!       - `:first-of-type`, `:last-of-type`, `:only-of-type`;
//!       - `:nth-child(an+b)`, `:nth-last-child(an+b)`,
//!         `:nth-of-type(an+b)`, `:nth-last-of-type(an+b)` — формулы
//!         `an+b`, целые числа, ключевые слова `odd` / `even`;
//!       - `:not(selector-list)` — CSS Selectors L4 §5.4: отрицание
//!         selector-list-а. Внутри разрешены complex-селекторы и nested
//!         `:not`. Матчит элемент, если ни один из селекторов списка ему
//!         не подходит. Specificity = максимум по списку (как у `:is`);
//!       - `:is(selector-list)` / `:where(selector-list)` — CSS4; матчит,
//!         если матчит любой из селекторов списка. Внутри разрешены любые
//!         complex-селекторы. Specificity для `:is` = максимум по списку,
//!         для `:where` = 0.
//!   - interactive pseudo-classes (`:hover`, `:focus`, …) сохраняются как
//!     `PseudoClass::Unsupported(name)` и при матчинге всегда возвращают `false`;
//!   - pseudo-elements `::name` парсятся отдельным узлом, никогда не матчат
//!     (т.к. в DOM им ничего не соответствует);
//!   - комментарии `/* */`, перечисление селекторов через `,`, опциональный
//!     trailing `;`. At-rules (`@media`, `@import`) пропускаются.
//!
//! Не поддерживается (отложено): namespace prefix в селекторах,
//! типизированные значения деклараций (length / color / calc).

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

mod at_rules;
mod declarations;
mod media;
mod selectors;

pub use at_rules::*;
pub use declarations::*;
pub use media::*;
pub use selectors::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
    pub declarations: Vec<Declaration>,
}

/// Process-unique identity of one `Stylesheet`'s **content**.
///
/// Exists so that a consumer caching something derived from a sheet (the
/// cascade's rule index, `lumen_layout::style`) can key that cache by
/// identity instead of by the sheet's address. An address is not an identity:
/// a freed sheet's address is handed straight back to the next allocation, so
/// an address-keyed cache has to be invalidated on every use to stay honest —
/// which is the same as having no cache across passes at all (BUG-341 S21).
///
/// A revision is minted fresh for every `Stylesheet` that comes into existence
/// (parse, `Default`, `Clone`) and is never reused, so two sheets can share one
/// only by one being a snapshot of the other before either was mutated. The
/// counter is `u64`: at one sheet per nanosecond it wraps in 584 years.
///
/// **The invariant a cache relies on**: while a sheet's revision is unchanged,
/// its rules are unchanged. Every in-place mutation must therefore go through
/// [`Stylesheet::merge_from`] or announce itself with
/// [`Stylesheet::mark_mutated`]. This is not left to review — the test
/// `every_stylesheet_mutation_in_the_workspace_announces_itself` scans the
/// workspace sources and fails the build on a direct `push`/`extend`/… into any
/// rule container outside this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StylesheetRevision(u64);

/// Source of [`StylesheetRevision`] values. Starts at 1 so that 0 is available
/// to consumers as a "no sheet seen yet" sentinel.
static NEXT_STYLESHEET_REVISION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl StylesheetRevision {
    /// Mints a revision no other `Stylesheet` has held or will hold.
    fn fresh() -> Self {
        Self(NEXT_STYLESHEET_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

#[derive(Debug)]
pub struct Stylesheet {
    /// Content identity — see [`StylesheetRevision`]. Private: it is minted,
    /// never chosen, and a hand-set value would silently license a stale cache.
    revision: StylesheetRevision,
    pub rules: Vec<Rule>,
    /// Зарегистрированные `@property`-правила. Порядок соответствует
    /// исходному CSS; повтор имени — последнее объявление побеждает (по
    /// CSS Properties and Values L1 §1.1).
    pub properties: Vec<PropertyRule>,
    /// `@media`-правила. Каждое содержит query и список вложенных rules.
    /// Применяются в каскаде только если `query.matches(ctx)` — см.
    /// `MediaQuery::matches`. Порядок source-position для tie-breaking
    /// в каскаде сохраняется через position в `Vec` (но фактическая
    /// специфика media rules в Phase 0 layout-у мерджится «как обычные»).
    pub media_rules: Vec<MediaRule>,
    /// `@import url("...");` декларации. Парсер собирает URL и опц.
    /// media-query (`@import url("a") screen and (min-width: 600px);`).
    /// Сам fetch и инкорпорация в каскад — задача потребителя (shell),
    /// потому что это требует сетевой/файловой загрузки. Phase 0:
    /// парсер только извлекает список, fetch отложен.
    pub imports: Vec<ImportRule>,
    /// `@font-face` правила. CSS Fonts L4 §4. Parser извлекает family,
    /// src, weight, style, display, unicode-range; реальная загрузка
    /// и регистрация в font-matcher — задача shell.
    pub font_faces: Vec<FontFaceRule>,
    /// CSS Cascade L5 §6.4 — порядок объявления layer-имён через
    /// statement-form `@layer base, components, utilities;`. В этом
    /// списке имена в **обратном** cascade-приоритете: первый имя имеет
    /// наименьший приоритет; unlayered rules выигрывают у всех layered.
    /// Анонимные layer-блоки (без имени) попадают сюда же с
    /// generated-именем `__anon_<n>__`.
    pub layer_order: Vec<String>,
    /// CSS Cascade L5 — block-form `@layer name { rules }`. Каждая
    /// запись — отдельный блок (повторное упоминание одного имени —
    /// отдельные записи; cascade-приоритет внутри layer-а — source-order).
    /// Phase 0 интеграция в каскад отложена — текущий compute_style
    /// итерирует только `rules`/`media_rules`. Здесь только parse+store.
    pub layers: Vec<LayerRule>,
    /// CSS Conditional Rules L3 §2 — `@supports (cond) { rules }`. Условие
    /// типизировано как [`SupportsCondition`]; вложенные rules применяются
    /// если `condition.evaluate(...)` истинно. Phase 0: parse+store +
    /// evaluator на основе списка известных property-имён; реальная
    /// интеграция в каскад — следующая задача (см. media_rules).
    pub supports_rules: Vec<SupportsRule>,
    /// CSS Animations L1 §3 — `@keyframes name { 0% {...} 50% {...} ... }`.
    /// Frames хранятся как `(offset_percent, declarations)`. Phase 0:
    /// parse+store; реальный animation runtime (interpolation, timing
    /// functions, animation-name связывание) отложен.
    pub keyframes: Vec<KeyframesRule>,
    /// CSS Counter Styles L3 §2 — `@counter-style name { ... }`. Phase 0:
    /// parse+store как `Vec<(name, declarations)>`. Реальное применение
    /// (список как кастомные markers через list-style-type) отложено.
    pub counter_styles: Vec<CounterStyleRule>,
    /// CSS Paged Media L3 §3 — `@page <selector>? { ... }`. Phase 0:
    /// parse+store. Реальная pagination — отдельная задача (Phase 2+).
    pub page_rules: Vec<PageRule>,
    /// CSS Cascade L6 — `@scope (<root>) [to (<limit>)] { rules }`. Phase 0:
    /// parse+store; реальная scope-фильтрация в каскаде отложена.
    pub scope_rules: Vec<ScopeRule>,
    /// CSS Transitions L2 §3.4 — `@starting-style { rules }`. Phase 0:
    /// parse+store. Применение при первом match (transition-from-display)
    /// отложено вместе с реальным transition runtime.
    pub starting_style_rules: Vec<StartingStyleRule>,
    /// CSS Containment L3 §3 — `@container <name>? (cond) { rules }`.
    /// Условие хранится как сырая строка (типизация query — отложена,
    /// нужна полная media-query-like grammar для container features).
    pub container_rules: Vec<ContainerRule>,
    /// CSS Fonts L4 §13 — `@font-palette-values --name { ... }`. Phase 0:
    /// parse+store. Matching against `font-palette` property and CPAL index
    /// resolution happen in layout (`resolve_font_palette_for_family`).
    pub font_palette_values: Vec<FontPaletteValuesRule>,
    /// CSS Color L5 §4 — `@color-profile --name { src: ...; rendering-intent: ...; }`.
    /// Phase 0: parse+store. Matching against `color(--name ...)` and used-value
    /// resolution happen in layout (`resolve_color_profile`); real ICC transform
    /// is deferred — channels are treated as already-sRGB.
    pub color_profiles: Vec<ColorProfileRule>,
    /// CSS Functions and Mixins L1 — `@function --name(<params>) { decls }`.
    /// Author-defined custom function, invoked as `--name(<args>)` from any
    /// property value. Parsing covers positional parameters with optional
    /// defaults and a raw `returns` type; evaluation (positional argument
    /// binding, local `--x` declarations, `result` substitution) happens in
    /// layout (`expand_custom_functions`, style.rs). Conditional group rules
    /// inside the body (`@media`, `@container`) are not yet supported.
    pub function_rules: Vec<FunctionRule>,
}

impl Default for Stylesheet {
    /// An empty sheet — with its own revision, like any other sheet. Two
    /// `Stylesheet::default()` values are `==` but not the same sheet, and a
    /// cache keyed by revision must not confuse them: the first may be filled
    /// in afterwards through its public fields (which is what the workspace
    /// gate makes visible).
    fn default() -> Self {
        Self {
            revision: StylesheetRevision::fresh(),
            rules: Vec::new(),
            properties: Vec::new(),
            media_rules: Vec::new(),
            imports: Vec::new(),
            font_faces: Vec::new(),
            layer_order: Vec::new(),
            layers: Vec::new(),
            supports_rules: Vec::new(),
            keyframes: Vec::new(),
            counter_styles: Vec::new(),
            page_rules: Vec::new(),
            scope_rules: Vec::new(),
            starting_style_rules: Vec::new(),
            container_rules: Vec::new(),
            font_palette_values: Vec::new(),
            color_profiles: Vec::new(),
            function_rules: Vec::new(),
        }
    }
}

impl Clone for Stylesheet {
    /// Copies the content and mints a **new** revision.
    ///
    /// Hand-written rather than derived on purpose: the clone is a separate
    /// sheet that its owner may mutate independently, so letting it inherit the
    /// original's revision would let a mutation of one silently authorise a
    /// cached index for the other. Sharing a revision is only sound while both
    /// are frozen, and nothing here can promise that.
    fn clone(&self) -> Self {
        Self {
            revision: StylesheetRevision::fresh(),
            rules: self.rules.clone(),
            properties: self.properties.clone(),
            media_rules: self.media_rules.clone(),
            imports: self.imports.clone(),
            font_faces: self.font_faces.clone(),
            layer_order: self.layer_order.clone(),
            layers: self.layers.clone(),
            supports_rules: self.supports_rules.clone(),
            keyframes: self.keyframes.clone(),
            counter_styles: self.counter_styles.clone(),
            page_rules: self.page_rules.clone(),
            scope_rules: self.scope_rules.clone(),
            starting_style_rules: self.starting_style_rules.clone(),
            container_rules: self.container_rules.clone(),
            font_palette_values: self.font_palette_values.clone(),
            color_profiles: self.color_profiles.clone(),
            function_rules: self.function_rules.clone(),
        }
    }
}

impl PartialEq for Stylesheet {
    /// Content equality. The revision is identity, not content, and two sheets
    /// parsed from the same CSS must compare equal.
    fn eq(&self, other: &Self) -> bool {
        self.rules == other.rules
            && self.properties == other.properties
            && self.media_rules == other.media_rules
            && self.imports == other.imports
            && self.font_faces == other.font_faces
            && self.layer_order == other.layer_order
            && self.layers == other.layers
            && self.supports_rules == other.supports_rules
            && self.keyframes == other.keyframes
            && self.counter_styles == other.counter_styles
            && self.page_rules == other.page_rules
            && self.scope_rules == other.scope_rules
            && self.starting_style_rules == other.starting_style_rules
            && self.container_rules == other.container_rules
            && self.font_palette_values == other.font_palette_values
            && self.color_profiles == other.color_profiles
            && self.function_rules == other.function_rules
    }
}

impl Stylesheet {
    /// This sheet's content identity — see [`StylesheetRevision`].
    pub fn revision(&self) -> StylesheetRevision {
        self.revision
    }

    /// Declares that this sheet's rules were changed in place, invalidating
    /// every cache keyed by [`Stylesheet::revision`].
    ///
    /// Needed only when rules are reached through the public fields directly;
    /// [`Stylesheet::merge_from`] already does it.
    pub fn mark_mutated(&mut self) {
        self.revision = StylesheetRevision::fresh();
    }

    /// Appends every rule of `other` to this sheet and mints a new revision.
    ///
    /// This is how a sheet grows while it is being streamed in
    /// (`LoadEvent::CssLoaded`): each `<link>`/`<style>` that finishes loading
    /// is merged into the sheet the next paint uses. Written here, beside the
    /// field list, because the previous hand-rolled version at the call site
    /// listed the fields it knew about and had fallen two behind
    /// (`color_profiles`, `function_rules` — so a streamed `@color-profile` or
    /// `@function` was silently dropped).
    pub fn merge_from(&mut self, other: Stylesheet) {
        let Stylesheet {
            revision: _,
            rules,
            properties,
            media_rules,
            imports,
            font_faces,
            layer_order,
            layers,
            supports_rules,
            keyframes,
            counter_styles,
            page_rules,
            scope_rules,
            starting_style_rules,
            container_rules,
            font_palette_values,
            color_profiles,
            function_rules,
        } = other;
        self.rules.extend(rules);
        self.properties.extend(properties);
        self.media_rules.extend(media_rules);
        self.imports.extend(imports);
        self.font_faces.extend(font_faces);
        self.layer_order.extend(layer_order);
        self.layers.extend(layers);
        self.supports_rules.extend(supports_rules);
        self.keyframes.extend(keyframes);
        self.counter_styles.extend(counter_styles);
        self.page_rules.extend(page_rules);
        self.scope_rules.extend(scope_rules);
        self.starting_style_rules.extend(starting_style_rules);
        self.container_rules.extend(container_rules);
        self.font_palette_values.extend(font_palette_values);
        self.color_profiles.extend(color_profiles);
        self.function_rules.extend(function_rules);
        self.mark_mutated();
    }
}

pub fn parse(input: &str) -> Stylesheet {
    Parser::new(input).parse_stylesheet()
}

/// Парсит содержимое HTML-атрибута `style="..."` — declaration-list без
/// окружающих фигурных скобок (CSS Style Attributes §2).
/// Используется для подключения inline-стилей к каскаду в `lumen-layout`
/// со specificity (1,0,0,0) согласно CSS Cascade L4 §6.4.3.
pub fn parse_inline_style(input: &str) -> Vec<Declaration> {
    Parser::new(input).parse_declaration_block()
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    /// At-rules, всплывающие из тела top-level conditional-group rule (сейчас
    /// только `@container`), которые должны попасть в stylesheet-уровневые
    /// коллекции, но не могут быть возвращены через одиночный `AtRuleOutcome`
    /// из [`Self::parse_at_rule`]. [`Self::parse_stylesheet`] опустошает буфер
    /// после каждого top-level `@`-правила.
    bubbled: Vec<AtRuleOutcome>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            bubbled: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn consume(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while let Some(c) = self.peek() {
                if c.is_whitespace() {
                    self.consume();
                } else {
                    break;
                }
            }
            if self.rest().starts_with("/*") {
                self.pos += 2;
                while !self.rest().starts_with("*/") && self.pos < self.input.len() {
                    self.consume();
                }
                if self.rest().starts_with("*/") {
                    self.pos += 2;
                }
            } else {
                break;
            }
        }
    }

    /// Возвращает true, если был whitespace или comment, и продвигает позицию.
    fn skip_ws_and_comments_track(&mut self) -> bool {
        let start = self.pos;
        self.skip_ws_and_comments();
        self.pos != start
    }

    fn parse_stylesheet(&mut self) -> Stylesheet {
        let mut rules = Vec::new();
        let mut properties = Vec::new();
        let mut media_rules = Vec::new();
        let mut imports = Vec::new();
        let mut font_faces = Vec::new();
        let mut font_palette_values: Vec<FontPaletteValuesRule> = Vec::new();
        let mut layer_order: Vec<String> = Vec::new();
        let mut layers: Vec<LayerRule> = Vec::new();
        let mut supports_rules: Vec<SupportsRule> = Vec::new();
        let mut keyframes: Vec<KeyframesRule> = Vec::new();
        let mut counter_styles: Vec<CounterStyleRule> = Vec::new();
        let mut page_rules: Vec<PageRule> = Vec::new();
        let mut scope_rules: Vec<ScopeRule> = Vec::new();
        let mut starting_style_rules: Vec<StartingStyleRule> = Vec::new();
        let mut container_rules: Vec<ContainerRule> = Vec::new();
        let mut color_profiles: Vec<ColorProfileRule> = Vec::new();
        let mut function_rules: Vec<FunctionRule> = Vec::new();
        let mut anon_counter: usize = 0;
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('@') => {
                    let primary = self.parse_at_rule();
                    // Primary-outcome + at-rules, всплывшие из тела top-level
                    // conditional-group rule (сейчас @container) через `bubbled`.
                    let mut outcomes = std::mem::take(&mut self.bubbled);
                    outcomes.insert(0, primary);
                    for outcome in outcomes {
                        match outcome {
                            AtRuleOutcome::Property(p) => properties.push(p),
                            AtRuleOutcome::Media(m) => media_rules.push(m),
                            AtRuleOutcome::Import(i) => imports.push(i),
                            AtRuleOutcome::FontFace(f) => font_faces.push(f),
                            AtRuleOutcome::FontPaletteValues(fp) => {
                                font_palette_values.push(fp)
                            }
                            AtRuleOutcome::ColorProfile(cp) => color_profiles.push(cp),
                            AtRuleOutcome::Function(f) => function_rules.push(f),
                            AtRuleOutcome::LayerNames(names) => {
                                for n in names {
                                    if !layer_order.iter().any(|e| e == &n) {
                                        layer_order.push(n);
                                    }
                                }
                            }
                            AtRuleOutcome::LayerBlock { name, rules: lr } => {
                                let resolved_name = name.unwrap_or_else(|| {
                                    anon_counter += 1;
                                    format!("__anon_{anon_counter}__")
                                });
                                if !layer_order.iter().any(|e| e == &resolved_name) {
                                    layer_order.push(resolved_name.clone());
                                }
                                layers.push(LayerRule {
                                    name: resolved_name,
                                    rules: lr,
                                });
                            }
                            AtRuleOutcome::Supports(s) => supports_rules.push(s),
                            AtRuleOutcome::Keyframes(k) => keyframes.push(k),
                            AtRuleOutcome::CounterStyle(c) => counter_styles.push(c),
                            AtRuleOutcome::Page(p) => page_rules.push(p),
                            AtRuleOutcome::Scope(s) => scope_rules.push(s),
                            AtRuleOutcome::StartingStyle(s) => {
                                starting_style_rules.push(s)
                            }
                            AtRuleOutcome::Container(c) => container_rules.push(c),
                            AtRuleOutcome::None => {}
                        }
                    }
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, nested_at)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested); // CSS Nesting L1: flat-expanded nested rules
                        // CSS Nesting L1 §5: nested at-rules bubble up into the stylesheet.
                        for at in nested_at {
                            match at {
                                AtRuleOutcome::Media(m) => media_rules.push(m),
                                AtRuleOutcome::Supports(s) => supports_rules.push(s),
                                AtRuleOutcome::LayerNames(names) => {
                                    for n in names {
                                        if !layer_order.iter().any(|e| e == &n) {
                                            layer_order.push(n);
                                        }
                                    }
                                }
                                AtRuleOutcome::LayerBlock { name, rules: lr } => {
                                    let resolved = name.unwrap_or_else(|| {
                                        anon_counter += 1;
                                        format!("__anon_{anon_counter}__")
                                    });
                                    if !layer_order.iter().any(|e| e == &resolved) {
                                        layer_order.push(resolved.clone());
                                    }
                                    layers.push(LayerRule { name: resolved, rules: lr });
                                }
                                AtRuleOutcome::Container(c) => container_rules.push(c),
                                AtRuleOutcome::Scope(s) => scope_rules.push(s),
                                _ => {}
                            }
                        }
                    } else if self.pos == before {
                        // Защита от бесконечного цикла: parse_rule не сдвинул
                        // позицию — принудительно проглатываем один символ.
                        self.consume();
                    }
                }
            }
        }
        Stylesheet {
            revision: StylesheetRevision::fresh(),
            rules,
            properties,
            media_rules,
            imports,
            font_faces,
            font_palette_values,
            layer_order,
            layers,
            supports_rules,
            keyframes,
            counter_styles,
            page_rules,
            scope_rules,
            starting_style_rules,
            container_rules,
            color_profiles,
            function_rules,
        }
    }

    fn parse_rule(&mut self) -> Option<(Rule, Vec<Rule>, Vec<AtRuleOutcome>)> {
        let start = self.pos;
        let selectors = self.parse_selector_list();
        self.skip_ws_and_comments();
        if selectors.is_empty() || self.peek() != Some('{') {
            if self.pos == start {
                self.consume();
            }
            self.recover_to_block_end();
            return None;
        }
        self.consume(); // '{'
        let (declarations, nested, at_rules) =
            self.parse_declaration_block_with_nesting(&selectors);
        Some((Rule { selectors, declarations }, nested, at_rules))
    }

    /// CSS Nesting L1 §3–§5 — parse declaration block that may contain nested rules and at-rules.
    /// Returns (declarations, flattened nested rules, nested at-rules).
    ///
    /// Handles:
    /// - `& selector { }` — explicit nesting with `&`
    /// - `.child { }`, `#id { }`, `[attr] { }`, `:hover { }`, `* { }` — implicit descendant nesting
    /// - `> .child { }`, `+ .sib { }`, `~ .sib { }` — implicit relative-combinator nesting
    /// - `@media / @supports / @layer / @container { }` — nested at-rules
    fn parse_declaration_block_with_nesting(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Declaration>, Vec<Rule>, Vec<AtRuleOutcome>) {
        let mut decls = Vec::new();
        let mut nested: Vec<Rule> = Vec::new();
        let mut at_rules: Vec<AtRuleOutcome> = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                    continue;
                }
                Some('&') => {
                    // Explicit nesting with `&`.
                    let (r, a) = self.parse_nested_rule_amp(parent_sels);
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §4: implicit descendant — `.foo {}`, `#id {}`, `[attr] {}`,
                // `:pseudo {}`, `* {}` cannot start a property name, so treat as nested rule.
                Some('.') | Some('#') | Some('[') | Some(':') | Some('*') => {
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, None);
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §4: implicit relative — `> .foo {}`, `+ .sib {}`, `~ .sib {}`.
                Some('>') | Some('+') | Some('~') => {
                    // SAFETY: we just peeked this char, consume() cannot return None here.
                    let c = self.consume().unwrap_or('>');
                    let comb = match c {
                        '+' => Combinator::NextSibling,
                        '~' => Combinator::LaterSibling,
                        _ => Combinator::Child, // '>'
                    };
                    self.skip_ws_and_comments();
                    let (r, a) = self.parse_implicit_nested_rule(parent_sels, Some(comb));
                    nested.extend(r);
                    at_rules.extend(a);
                }
                // CSS Nesting L1 §5: nested at-rule.
                Some('@') => {
                    let ats = self.parse_nested_at_rule(parent_sels);
                    at_rules.extend(ats);
                }
                _ => match self.parse_declaration() {
                    Some(d) => decls.push(d),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        (decls, nested, at_rules)
    }

    /// Parse `& [combinator] selector-list { declarations }` and expand into flat rules.
    /// The `&` has already been peeked but not consumed.
    fn parse_nested_rule_amp(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        self.consume(); // consume '&'
        let had_ws = self.skip_ws_and_comments_track();
        // Determine if there's an explicit combinator after &.
        let combinator: Option<Combinator> = match self.peek() {
            Some('>') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::Child) }
            Some('+') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::NextSibling) }
            Some('~') => { self.consume(); self.skip_ws_and_comments(); Some(Combinator::LaterSibling) }
            Some('{') => None, // bare `& { }` — same element as parent
            _ if had_ws => Some(Combinator::Descendant),
            _ => None, // `&.class` / `&[attr]` / `&#id` — compound join
        };
        // Parse the selector list that follows (may be empty for bare `& { }`).
        let nested_sels: Vec<ComplexSelector> = if self.peek() == Some('{') {
            vec![] // bare `& { }` — same element
        } else {
            let s = self.parse_selector_list();
            if s.is_empty() {
                self.recover_to_block_end();
                return (vec![], vec![]);
            }
            s
        };
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        // Expand: combine each parent selector with each nested selector.
        let expanded_sels = if nested_sels.is_empty() {
            parent_sels.to_vec() // bare `& { }` = same as parent
        } else {
            expand_nesting(parent_sels, combinator, &nested_sels)
        };
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut result = vec![Rule { selectors: expanded_sels, declarations }];
        result.extend(sub_nested);
        (result, sub_at)
    }

    /// CSS Nesting L1 §4: implicit nesting — `.child { }` inside a rule block
    /// is treated as `& .child { }` (descendant). Called when we see a selector-
    /// start token (`.`, `#`, `[`, `:`, `*`) without an explicit `&`.
    /// `combinator` — pre-parsed explicit combinator (`>`, `+`, `~`), or `None`
    /// for implicit descendant.
    fn parse_implicit_nested_rule(
        &mut self,
        parent_sels: &[ComplexSelector],
        combinator: Option<Combinator>,
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        let nested_sels = self.parse_selector_list();
        if nested_sels.is_empty() {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return (vec![], vec![]);
        }
        self.consume(); // '{'
        // Implicit nesting without explicit combinator → descendant.
        let comb = combinator.unwrap_or(Combinator::Descendant);
        let expanded_sels = expand_nesting(parent_sels, Some(comb), &nested_sels);
        let (declarations, sub_nested, sub_at) =
            self.parse_declaration_block_with_nesting(&expanded_sels);
        let mut rules = vec![Rule { selectors: expanded_sels, declarations }];
        rules.extend(sub_nested);
        (rules, sub_at)
    }

    /// Парсит тело group at-rule (`@container`/`@media`/…), которое не вложено
    /// ни в какое qualified-правило (`parent_sels` пуст) — то есть ведёт себя
    /// как обычный rule-list stylesheet-уровня: bare-объявления здесь
    /// невалидны (нет селектора, к которому их привязать), поэтому любой
    /// не-`@`-токен — это обычное qualified-правило с произвольным селектором
    /// (включая голый type-селектор вроде `p`, который CSS Nesting L1 §4
    /// запрещает как неоднозначный только внутри уже открытого style-правила).
    /// Вложенные at-rules всплывают отдельным `Vec` (плоская модель). Курсор
    /// должен стоять сразу после `{`, потребляет закрывающую `}`.
    fn parse_bare_group_body(&mut self) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        let mut rules = Vec::new();
        let mut at_rules = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some('@') => {
                    at_rules.extend(self.parse_nested_at_rule(&[]));
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, nested_at)) = self.parse_rule() {
                        rules.push(rule);
                        rules.extend(nested);
                        at_rules.extend(nested_at);
                    } else if self.pos == before {
                        self.consume();
                    }
                }
            }
        }
        (rules, at_rules)
    }

    /// Парсит тело nested conditional-group at-rule (после уже consume-нутого
    /// `{`) с полной рекурсией CSS Nesting L1 §5: bare-декларации сворачиваются
    /// в синтетическое правило с селекторами `parent_sels`, вложенные правила
    /// добавляются следом, вложенные at-rules возвращаются отдельным `Vec`
    /// (всплывают на stylesheet-уровень). Общий код для веток `@media` /
    /// `@supports` / `@layer` / `@container` / `@scope` в
    /// [`Self::parse_nested_at_rule`]. Курсор должен стоять сразу после `{`.
    /// Пустой `parent_sels` означает, что мы на самом деле не вложены ни в
    /// какое qualified-правило (например, `@media` внутри `@container` на
    /// stylesheet-уровне) — тогда делегирует в [`Self::parse_bare_group_body`],
    /// у которой другая грамматика тела (rule-list, а не declarations+nesting).
    fn parse_nested_group_body(
        &mut self,
        parent_sels: &[ComplexSelector],
    ) -> (Vec<Rule>, Vec<AtRuleOutcome>) {
        if parent_sels.is_empty() {
            return self.parse_bare_group_body();
        }
        let (decls, inner_rules, inner_at) =
            self.parse_declaration_block_with_nesting(parent_sels);
        let mut rules = Vec::new();
        if !decls.is_empty() {
            rules.push(Rule {
                selectors: parent_sels.to_vec(),
                declarations: decls,
            });
        }
        rules.extend(inner_rules);
        (rules, inner_at)
    }

    /// CSS Nesting L1 §5: nested at-rule inside a qualified rule.
    /// Example: `.parent { @media (min-width: 800px) { color: red; } }`
    /// expands to: `@media (min-width: 800px) { .parent { color: red; } }`.
    /// Supports `@media`, `@supports`, `@layer`, `@container`, `@scope`.
    fn parse_nested_at_rule(&mut self, parent_sels: &[ComplexSelector]) -> Vec<AtRuleOutcome> {
        let start = self.pos;
        self.consume(); // '@'
        let name = self.parse_ident().unwrap_or_default();
        self.skip_ws_and_comments();

        if name.eq_ignore_ascii_case("media") {
            let query_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' {
                    break;
                }
                self.consume();
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let query_str = self.input[query_start..self.pos].trim();
            let query = parse_media_query(query_str);
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes = vec![AtRuleOutcome::Media(MediaRule { query, rules })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("supports") {
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
                return vec![];
            }
            let cond_str = self.input[cond_start..self.pos].trim();
            let condition = parse_supports_condition(cond_str);
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::Supports(SupportsRule { condition, rules })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("layer") {
            let names_start = self.pos;
            while let Some(c) = self.peek() {
                if c == '{' || c == ';' {
                    break;
                }
                self.consume();
            }
            let prelude = self.input[names_start..self.pos].trim();
            if self.peek() == Some(';') {
                self.consume();
                return vec![];
            }
            if self.peek() != Some('{') {
                return vec![];
            }
            let layer_name = if prelude.is_empty() {
                None
            } else {
                Some(prelude.to_string())
            };
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::LayerBlock { name: layer_name, rules }];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("container") {
            // CSS Containment L3 §3: опциональное имя перед condition — тот же
            // разбор прелюдии, что и для top-level `@container`.
            let Some((cont_name, condition)) = self.parse_container_prelude() else {
                return vec![];
            };
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes = vec![AtRuleOutcome::Container(ContainerRule {
                name: cont_name,
                condition,
                rules,
            })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        if name.eq_ignore_ascii_case("scope") {
            // CSS Cascade L6 §3: `@scope (<root>)? [to (<limit>)]?` вложенный в
            // qualified-правило. Прелюдия — тот же разбор, что и для top-level
            // `@scope`; тело — рекурсивный declaration-block с `parent_sels`.
            let (root, limit) = self.parse_scope_prelude();
            self.skip_ws_and_comments();
            if self.peek() != Some('{') {
                return vec![];
            }
            self.consume(); // '{'
            let (rules, inner_at) = self.parse_nested_group_body(parent_sels);
            let mut outcomes =
                vec![AtRuleOutcome::Scope(ScopeRule { root, limit, rules })];
            outcomes.extend(inner_at);
            return outcomes;
        }

        // Unknown nested at-rule — skip the block.
        self.pos = start;
        self.skip_at_rule();
        vec![]
    }

    fn recover_to_block_end(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                '{' => {
                    self.consume();
                    self.skip_block();
                    return;
                }
                ';' => {
                    self.consume();
                    return;
                }
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_ident(&mut self) -> Option<String> {
        let first = self.peek()?;
        if !is_ident_start(first) {
            return None;
        }
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.consume();
                s.push(c);
            } else {
                break;
            }
        }
        Some(s)
    }

}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '-' || c >= '\u{00A0}'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// CSS Nesting L1 §3 — expand `& (combinator) nested` into concrete selectors.
///
/// `combinator = None`  → compound join (e.g. `&.foo` → `parent.foo`)
/// `combinator = Some(c)` → `parent c nested` (e.g. `& span` → `parent descendant span`)
fn expand_nesting(
    parents: &[ComplexSelector],
    combinator: Option<Combinator>,
    nested: &[ComplexSelector],
) -> Vec<ComplexSelector> {
    let mut result = Vec::new();
    for parent in parents {
        for n in nested {
            let expanded = match combinator {
                None => {
                    // `&.foo` → merge parent head with nested head, keep tails.
                    let mut head = parent.head.clone();
                    head.parts.extend_from_slice(&n.head.parts);
                    let mut tail = parent.tail.clone();
                    tail.extend_from_slice(&n.tail);
                    ComplexSelector { head, tail }
                }
                Some(comb) => {
                    // `& span` → parent + (comb, nested_head) + nested_tail
                    let mut tail = parent.tail.clone();
                    tail.push((comb, n.head.clone()));
                    tail.extend_from_slice(&n.tail);
                    ComplexSelector { head: parent.head.clone(), tail }
                }
            };
            result.push(expanded);
        }
    }
    result
}

#[cfg(test)]
#[path = "parser/tests/revision.rs"]
mod revision_tests;

#[cfg(test)]
#[path = "parser/tests/selectors.rs"]
mod selectors_tests;
#[cfg(test)]
pub(crate) use selectors_tests::one;

#[cfg(test)]
#[path = "parser/tests/at_rules.rs"]
mod at_rules_tests;

#[cfg(test)]
#[path = "parser/tests/nesting.rs"]
mod nesting_tests;
