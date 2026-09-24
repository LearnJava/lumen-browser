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
mod mixins;
mod selectors;

pub use at_rules::*;
pub use declarations::*;
pub use media::*;
pub use mixins::*;
pub use selectors::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<ComplexSelector>,
    pub declarations: Vec<Declaration>,
}

impl Rule {
    /// `CSSStyleRule.selectorText` (CSSOM §6.5.2) — the rule's selector list
    /// serialised back to CSS text. Re-serialises from the structured
    /// [`ComplexSelector`] list rather than preserving original source
    /// whitespace, same as every browser's CSSOM (CSSOM §6.5.2 defines
    /// `selectorText`'s getter as a serialization, not a source-text echo).
    pub fn selector_text(&self) -> String {
        sels_to_css_str(&self.selectors)
    }

    /// `CSSStyleRule.style.cssText` (CSSOM §6.7.2) for this rule's own
    /// declaration block — `"prop: value; prop2: value2 !important;"`, one
    /// space after the colon, one trailing space before `!important`.
    pub fn style_css_text(&self) -> String {
        self.declarations
            .iter()
            .filter(|d| d.property != MIXIN_APPLY_MARKER)
            .map(Declaration::to_css_text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `CSSStyleRule.cssText` (CSSOM §6.5.2) — `selectorText { style_css_text }`
    /// for an ordinary rule (unchanged single-line format), or, once this
    /// rule's own declarations carry an [`MIXIN_APPLY_MARKER`] (CSS Mixins
    /// L1's `@apply`), the same one-child-per-line multi-line format
    /// [`render_container`] gives `@mixin`/`@result` — confirmed against
    /// `mixin-cssom.tentative.html`'s "serialization of rule with @apply"/
    /// "…and contents argument" subtests. A marker declaration is re-parsed
    /// back into an [`ApplyRule`] ([`parse_apply_call`]) rather than echoing
    /// its captured raw text verbatim, so stray source whitespace around
    /// `@apply` never leaks into the serialization.
    pub fn css_text(&self) -> String {
        let has_apply = self.declarations.iter().any(|d| d.property == MIXIN_APPLY_MARKER);
        if !has_apply {
            return format!("{} {{ {} }}", self.selector_text(), self.style_css_text());
        }
        let children: Vec<String> = self
            .declarations
            .iter()
            .map(|d| {
                if d.property == MIXIN_APPLY_MARKER {
                    parse_apply_call(&d.value).map(|a| a.css_text()).unwrap_or_default()
                } else {
                    d.to_css_text()
                }
            })
            .collect();
        render_container(&self.selector_text(), &children, false)
    }
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
    /// CSS View Transitions Module Level 2 §3 — `@view-transition { navigation:
    /// auto | none; }`. Phase 0: parse+store. Cross-document (MPA) opt-in
    /// detection (both documents must declare `navigation: auto`, same-origin)
    /// and the actual navigation-transition pipeline are shell-side
    /// (`docs/tasks/ph3-view-transitions-mpa.md` срезы 2+).
    pub view_transition_rules: Vec<ViewTransitionRule>,
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
    /// CSS Functions and Mixins L1 — `@mixin --name(<params>) { ... }`.
    /// Author-defined reusable declaration set, invoked as
    /// `@apply --name(<args>)` from a style rule's body (or another
    /// mixin's own `@result`). Flat declarations are evaluated in layout
    /// (`expand_mixin_apply`, `style/substitute.rs`); a nested style rule
    /// inside `@result` is instead materialized into a standalone
    /// top-level entry of this sheet's own `rules` by
    /// [`mixins::collect_mixin_nested_rules`] right after parsing — see
    /// [`MixinRule`]'s doc comment.
    pub mixin_rules: Vec<MixinRule>,
    /// Source order of top-level plain style rules and `@media` blocks, as
    /// tags only (`Style`/`Media`) — the Nth `Style` tag refers to `rules[N]`
    /// among style tags seen so far, same for `Media`/`media_rules`. Exists
    /// because `rules` and `media_rules` are separate `Vec`s with no shared
    /// index space, so nothing else records which came first in the source
    /// (`p { } @media { } div { }` would otherwise be indistinguishable from
    /// `@media { } p { } div { }` once parsing is done). Feeds
    /// [`Stylesheet::cssom_rules`] — `document.styleSheets[i].cssRules`
    /// (CSSOM §6.5) needs original order, other consumers (the cascade) do
    /// not care and keep reading `rules`/`media_rules` directly. Only these
    /// two kinds are tracked for now — CSSOM-1's `cssRules` does not yet
    /// expose `@import`/`@font-face`/`@supports`/etc. as `CSSRule` objects.
    pub top_level_order: Vec<TopLevelRuleKind>,
    /// Byte offset into [`Self::source`] where each [`Self::top_level_order`]
    /// entry's rule began, same length and order as that vec (CSSOM-8
    /// вариант C). Ascending for a freshly parsed sheet; a rule inserted
    /// through [`Self::insert_rule`] gets [`SYNTHETIC_SPAN`] instead, since it
    /// never existed in `source`.
    ///
    /// Private on purpose: it is provenance, not content — the only supported
    /// reader is [`Self::cssom_range_for_source_span`], which is what makes a
    /// per-node CSSOM edit addressable inside the page's single concatenated
    /// cascade parse without re-serialising anything (see that method).
    top_level_spans: Vec<usize>,
    /// The exact text [`parse`] was handed, kept so that a sheet parsed from
    /// a concatenation of several `<style>`/`<link>` bodies can still say
    /// which byte range came from which contributor — see
    /// [`Self::cssom_range_for_source_span`]. `None` for a sheet that was
    /// built rather than parsed ([`Self::default`]).
    ///
    /// `Arc<str>` so [`Self::clone`] stays cheap: the same-tick CSSOM flush
    /// clones the whole cascade sheet on every mutated read.
    source: Option<std::sync::Arc<str>>,
}

/// [`Stylesheet::top_level_spans`] entry for a rule that came from
/// [`Stylesheet::insert_rule`] rather than from parsed source text.
pub const SYNTHETIC_SPAN: usize = usize::MAX;

/// Tag for [`Stylesheet::top_level_order`] — see that field's doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevelRuleKind {
    Style,
    Media,
    /// A top-level (not `@layer`-nested) `@mixin` rule — carries its own
    /// index into [`Stylesheet::mixin_rules`] directly, unlike `Style`/
    /// `Media`'s "Nth tag of this kind → Nth entry of the matching vec"
    /// counting scheme: `mixin_rules` also receives pushes from `@layer`
    /// blocks (`AtRuleOutcome::LayerBlock`), which never get a tag of their
    /// own (a `@layer` block itself has no `cssRules` entry, same as
    /// `@media`'s doc comment already notes for other untracked at-rules),
    /// so a same-kind position count would silently pick the wrong mixin
    /// whenever a layered one sits between two top-level ones in source
    /// order. Stamped once, at push time, in [`parse`]'s single top-level
    /// dispatch site for `AtRuleOutcome::Mixin`.
    Mixin(usize),
}

/// One top-level rule as `document.styleSheets[i].cssRules` sees it — see
/// [`Stylesheet::cssom_rules`].
#[derive(Debug, Clone, Copy)]
pub enum CssomRuleRef<'a> {
    Style(&'a Rule),
    Media(&'a MediaRule),
    /// A top-level `@mixin` — CSS Mixins L1 §cssom (`mixin-cssom.tentative.html`).
    Mixin(&'a MixinRule),
}

/// Failure of [`Stylesheet::insert_rule`]/[`Stylesheet::delete_rule`] — the
/// two DOMException names CSSOM §6.5 assigns each operation's step list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssomRuleMutationError {
    /// `index` is greater than [`Stylesheet::cssom_rules`]'s length
    /// (`insertRule`) or not less than it (`deleteRule`).
    IndexSize,
    /// `insertRule`'s rule text does not parse to exactly one rule this
    /// sheet's `cssom_rules()` can represent (a plain style rule or an
    /// `@media` block) — e.g. it is a declaration, a rule of a kind
    /// `top_level_order` does not track (`@font-face`, `@import`, …), more
    /// than one rule, or unparseable text. Lumen does not model
    /// `insertRule`'s further ordering constraints (`@import` must precede
    /// every other rule) since those rule kinds are not CSSOM-representable
    /// here to begin with.
    Syntax,
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
            view_transition_rules: Vec::new(),
            container_rules: Vec::new(),
            font_palette_values: Vec::new(),
            color_profiles: Vec::new(),
            function_rules: Vec::new(),
            mixin_rules: Vec::new(),
            top_level_order: Vec::new(),
            top_level_spans: Vec::new(),
            source: None,
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
            view_transition_rules: self.view_transition_rules.clone(),
            container_rules: self.container_rules.clone(),
            font_palette_values: self.font_palette_values.clone(),
            color_profiles: self.color_profiles.clone(),
            function_rules: self.function_rules.clone(),
            mixin_rules: self.mixin_rules.clone(),
            top_level_order: self.top_level_order.clone(),
            top_level_spans: self.top_level_spans.clone(),
            source: self.source.clone(),
        }
    }
}

impl PartialEq for Stylesheet {
    /// Content equality. The revision is identity, not content, and two sheets
    /// parsed from the same CSS must compare equal.
    ///
    /// `source`/`top_level_spans` are deliberately **excluded** for the same
    /// reason: they are provenance. [`Stylesheet::insert_rule`] relies on this
    /// — it checks a freshly parsed one-rule sheet against
    /// [`Stylesheet::default`] to prove nothing else came out of the text, and
    /// a parsed sheet always carries a `source` while `default()` never does,
    /// so comparing it would make that check fail for every input.
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
            && self.view_transition_rules == other.view_transition_rules
            && self.container_rules == other.container_rules
            && self.font_palette_values == other.font_palette_values
            && self.color_profiles == other.color_profiles
            && self.function_rules == other.function_rules
            && self.mixin_rules == other.mixin_rules
            && self.top_level_order == other.top_level_order
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
            view_transition_rules,
            container_rules,
            font_palette_values,
            color_profiles,
            function_rules,
            mixin_rules,
            top_level_order,
            top_level_spans: _,
            source: _,
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
        self.view_transition_rules.extend(view_transition_rules);
        self.container_rules.extend(container_rules);
        self.font_palette_values.extend(font_palette_values);
        self.color_profiles.extend(color_profiles);
        self.function_rules.extend(function_rules);
        self.mixin_rules.extend(mixin_rules);
        // Plain concatenation is correct here (no index rebasing needed):
        // `top_level_order` only ever stores tags, not indices, and
        // `rules`/`media_rules` are extended in this same call — the two
        // vecs grow together, so a reader counting tags of each kind from
        // the start of the (now-longer) list still lands on the right
        // `rules[N]`/`media_rules[N]` after the merge. See the field's doc.
        // The merged-in tags carry no usable provenance: `other`'s spans are
        // offsets into `other`'s own source, which is a different string from
        // `self.source`. Tagging them `SYNTHETIC_SPAN` keeps the two vecs the
        // same length (every reader indexes them in lockstep) and makes
        // `cssom_range_for_source_span` simply never attribute a merged rule
        // to a byte range of `self.source` — a miss, not a wrong answer.
        self.top_level_spans
            .extend(std::iter::repeat_n(SYNTHETIC_SPAN, top_level_order.len()));
        self.top_level_order.extend(top_level_order);
        self.mark_mutated();
    }

    /// `document.styleSheets[i].cssRules` (CSSOM §6.5) in original source
    /// order — interleaves [`Self::rules`] and [`Self::media_rules`] using
    /// [`Self::top_level_order`]. Silently drops a tag whose backing vec ran
    /// short instead of panicking; that can only happen if some caller wrote
    /// to `top_level_order`/`rules`/`media_rules` directly instead of through
    /// [`Self::merge_from`], which the workspace-mutation gate
    /// (`every_stylesheet_mutation_in_the_workspace_announces_itself`)
    /// already forbids outside this file.
    pub fn cssom_rules(&self) -> Vec<CssomRuleRef<'_>> {
        let mut out = Vec::with_capacity(self.top_level_order.len());
        let mut style_idx = 0usize;
        let mut media_idx = 0usize;
        for kind in &self.top_level_order {
            match kind {
                TopLevelRuleKind::Style => {
                    if let Some(r) = self.rules.get(style_idx) {
                        out.push(CssomRuleRef::Style(r));
                    }
                    style_idx += 1;
                }
                TopLevelRuleKind::Media => {
                    if let Some(r) = self.media_rules.get(media_idx) {
                        out.push(CssomRuleRef::Media(r));
                    }
                    media_idx += 1;
                }
                // Own index, not a position count — see the variant's doc
                // comment for why `mixin_rules` can't reuse the Style/Media
                // scheme.
                TopLevelRuleKind::Mixin(idx) => {
                    if let Some(m) = self.mixin_rules.get(*idx) {
                        out.push(CssomRuleRef::Mixin(m));
                    }
                }
            }
        }
        out
    }

    /// `CSSStyleSheet.insertRule(rule, index)` (CSSOM §6.5). Parses `rule_text`
    /// on its own; it must yield exactly one rule of a kind
    /// [`Self::cssom_rules`] can represent (a style rule or an `@media`
    /// block) and nothing else, or this returns
    /// [`CssomRuleMutationError::Syntax`]. `index` may equal the current rule
    /// count (append) but not exceed it, or this returns
    /// [`CssomRuleMutationError::IndexSize`] — matching CSSOM's step order,
    /// index is checked before the rule text.
    ///
    /// On success, returns `index` (CSSOM's "return the index at which
    /// rule was inserted") and mints a new revision.
    pub fn insert_rule(
        &mut self,
        rule_text: &str,
        index: usize,
    ) -> Result<usize, CssomRuleMutationError> {
        if index > self.top_level_order.len() {
            return Err(CssomRuleMutationError::IndexSize);
        }
        let mut parsed = parse(rule_text);
        if parsed.top_level_order.len() != 1 {
            return Err(CssomRuleMutationError::Syntax);
        }
        let kind = parsed.top_level_order[0];
        let style_rule = parsed.rules.pop();
        let media_rule = parsed.media_rules.pop();
        parsed.top_level_order.clear();
        parsed.rules.clear();
        parsed.media_rules.clear();
        // Nothing else came out of parsing this text — no stray
        // `@font-face`/`@import`/`@property`/etc. alongside the one rule.
        if parsed != Stylesheet::default() {
            return Err(CssomRuleMutationError::Syntax);
        }
        let sub_index =
            self.top_level_order[..index].iter().filter(|k| **k == kind).count();
        match (kind, style_rule, media_rule) {
            (TopLevelRuleKind::Style, Some(rule), _) => self.rules.insert(sub_index, rule),
            (TopLevelRuleKind::Media, _, Some(rule)) => self.media_rules.insert(sub_index, rule),
            _ => return Err(CssomRuleMutationError::Syntax),
        }
        self.top_level_order.insert(index, kind);
        // Never existed in `source` — see `SYNTHETIC_SPAN`.
        self.top_level_spans.insert(index, SYNTHETIC_SPAN);
        self.mark_mutated();
        Ok(index)
    }

    /// `CSSStyleSheet.deleteRule(index)` (CSSOM §6.5).
    pub fn delete_rule(&mut self, index: usize) -> Result<(), CssomRuleMutationError> {
        let Some(&kind) = self.top_level_order.get(index) else {
            return Err(CssomRuleMutationError::IndexSize);
        };
        // `Mixin`'s own embedded index makes this count wrong for it (see
        // `TopLevelRuleKind::Mixin`'s doc comment — every tag's index is
        // distinct, so `**k == kind` never matches a different mixin's tag);
        // only the `Style`/`Media` arms below read it.
        let sub_index =
            self.top_level_order[..index].iter().filter(|k| **k == kind).count();
        self.top_level_order.remove(index);
        // Kept in lockstep with `top_level_order` — same index space.
        if index < self.top_level_spans.len() {
            self.top_level_spans.remove(index);
        }
        match kind {
            TopLevelRuleKind::Style => {
                self.rules.remove(sub_index);
            }
            TopLevelRuleKind::Media => {
                self.media_rules.remove(sub_index);
            }
            TopLevelRuleKind::Mixin(midx) => {
                self.mixin_rules.remove(midx);
                // Removing shifts every later mixin down one slot — keep
                // every other `Mixin` tag pointing at the same `MixinRule`
                // it did before this deletion.
                for k in self.top_level_order.iter_mut() {
                    if let TopLevelRuleKind::Mixin(j) = k
                        && *j > midx
                    {
                        *j -= 1;
                    }
                }
            }
        }
        self.mark_mutated();
        Ok(())
    }

    /// `CSSStyleRule.style`'s write half (CSSOM §6.7.2) for a TOP-LEVEL style
    /// rule of THIS (owned, not constructed) sheet — CSSOM-8. Replaces the
    /// rule's whole declaration list by reparsing `css_text` as a bare
    /// declaration list ([`parse_inline_style`], the same grammar an
    /// element's `style=""` attribute uses), mirroring [`Self::insert_rule`]'s
    /// reparse-and-replace shape. Only a plain style rule at `index` is
    /// addressable this way — a `@media`/`@mixin` slot answers `Syntax`
    /// (media's own nested rules go through [`Self::set_media_child_style_text`]
    /// instead; a mixin's `@result`/nested-rule declarations are out of this
    /// slice's scope).
    pub fn set_rule_style_text(
        &mut self,
        index: usize,
        css_text: &str,
    ) -> Result<(), CssomRuleMutationError> {
        let Some(&kind) = self.top_level_order.get(index) else {
            return Err(CssomRuleMutationError::IndexSize);
        };
        if kind != TopLevelRuleKind::Style {
            return Err(CssomRuleMutationError::Syntax);
        }
        let sub_index = self.top_level_order[..index].iter().filter(|k| **k == kind).count();
        let Some(rule) = self.rules.get_mut(sub_index) else {
            return Err(CssomRuleMutationError::Syntax);
        };
        rule.declarations = parse_inline_style(css_text);
        self.mark_mutated();
        Ok(())
    }

    /// Sibling of [`Self::set_rule_style_text`] for a style rule nested
    /// inside a top-level `@media` block: `media_index` is that block's own
    /// top-level position (as in [`Self::cssom_rules`]), `child_index` its
    /// position inside the block's own rule list.
    pub fn set_media_child_style_text(
        &mut self,
        media_index: usize,
        child_index: usize,
        css_text: &str,
    ) -> Result<(), CssomRuleMutationError> {
        let Some(&kind) = self.top_level_order.get(media_index) else {
            return Err(CssomRuleMutationError::IndexSize);
        };
        if kind != TopLevelRuleKind::Media {
            return Err(CssomRuleMutationError::Syntax);
        }
        let sub_index =
            self.top_level_order[..media_index].iter().filter(|k| **k == kind).count();
        let Some(media_rule) = self.media_rules.get_mut(sub_index) else {
            return Err(CssomRuleMutationError::Syntax);
        };
        let Some(child) = media_rule.rules.get_mut(child_index) else {
            return Err(CssomRuleMutationError::IndexSize);
        };
        child.declarations = parse_inline_style(css_text);
        self.mark_mutated();
        Ok(())
    }

    /// Whether the top-level `@mixin` at `index` has an `@result` block at
    /// all — `CSSMixinRule.cssRules.length` (CSS Mixins L1 §cssom) is `1`
    /// when this is `true` (the sole child is `@result` itself, addressed as
    /// the empty path — see [`Self::mixin_result_child_count`]), `0`
    /// otherwise. `false` if `index` is out of range or not a `Mixin` tag.
    pub fn mixin_has_result(&self, index: usize) -> bool {
        let Some(&kind) = self.top_level_order.get(index) else { return false };
        let TopLevelRuleKind::Mixin(midx) = kind else { return false };
        self.mixin_rules.get(midx).is_some_and(|m| m.result.is_some())
    }

    /// `@result`'s own `.cssRules.length` (`path = []`, the entry point into
    /// a `@mixin`'s result tree) — or, for a non-empty `path`, the resolved
    /// node's own `childCount` ([`mixins::MixinResultNodeInfo::child_count`]).
    /// `None` if `index` is out of range, not a `Mixin` tag, the mixin has no
    /// `@result`, or a non-empty `path` does not resolve.
    pub fn mixin_result_child_count(&self, index: usize, path: &[usize]) -> Option<usize> {
        if !path.is_empty() {
            return Some(self.mixin_result_node_info(index, path)?.child_count);
        }
        let &kind = self.top_level_order.get(index)?;
        let TopLevelRuleKind::Mixin(midx) = kind else { return None };
        Some(self.mixin_rules.get(midx)?.result_child_count())
    }

    /// One CSSOM-addressable node inside the `@result` tree of the top-level
    /// `@mixin` at `index` — see [`mixins::resolve_result_node`]. `None` if
    /// `index` is out of range, not a `Mixin` tag, or `path` does not
    /// resolve.
    pub fn mixin_result_node_info(&self, index: usize, path: &[usize]) -> Option<MixinResultNodeInfo> {
        let &kind = self.top_level_order.get(index)?;
        let TopLevelRuleKind::Mixin(midx) = kind else { return None };
        self.mixin_rules.get(midx)?.result_node_info(path)
    }

    /// `.style`'s write half for a node inside a top-level `@mixin`'s
    /// `@result` tree — CSSOM-8, вложенные правила (the remainder of the
    /// slice that closed top-level/`@media` addressing: see
    /// [`Self::set_rule_style_text`]'s doc comment for that half).
    /// `path` — see [`mixins::resolve_result_node`].
    pub fn set_mixin_result_style(
        &mut self,
        index: usize,
        path: &[usize],
        css_text: &str,
    ) -> Result<(), CssomRuleMutationError> {
        let Some(&kind) = self.top_level_order.get(index) else {
            return Err(CssomRuleMutationError::IndexSize);
        };
        let TopLevelRuleKind::Mixin(midx) = kind else {
            return Err(CssomRuleMutationError::Syntax);
        };
        let Some(mixin) = self.mixin_rules.get_mut(midx) else {
            return Err(CssomRuleMutationError::Syntax);
        };
        if mixin.set_result_style(path, css_text) {
            self.mark_mutated();
            Ok(())
        } else {
            Err(CssomRuleMutationError::Syntax)
        }
    }

    /// `CSSGroupingRule.insertRule` on a TOP-LEVEL style rule's own body,
    /// restricted to inserting an `@apply` statement (CSS Mixins L1) — CSSOM-8's
    /// third and last nested-rule shape (`mixin-invalidation.tentative.html`'s
    /// "invalidation on adding @apply rule"). See
    /// [`Rule::insert_apply_marker`] for why this is `@apply`-only rather
    /// than a general nested-style-rule insertion, and for what `index`
    /// addresses.
    pub fn insert_rule_body_apply(
        &mut self,
        rule_index: usize,
        index: usize,
        rule_text: &str,
    ) -> Result<usize, CssomRuleMutationError> {
        let Some(&kind) = self.top_level_order.get(rule_index) else {
            return Err(CssomRuleMutationError::IndexSize);
        };
        if kind != TopLevelRuleKind::Style {
            return Err(CssomRuleMutationError::Syntax);
        }
        let sub_index = self.top_level_order[..rule_index].iter().filter(|k| **k == kind).count();
        let Some(rule) = self.rules.get_mut(sub_index) else {
            return Err(CssomRuleMutationError::Syntax);
        };
        let result = rule.insert_apply_marker(index, rule_text)?;
        self.mark_mutated();
        Ok(result)
    }

    /// The exact text [`parse`] built this sheet from, if it was parsed at all
    /// — see [`Self::source`].
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Byte range of `needle` inside this sheet's [`Self::source`], searched
    /// from `from` and retried from the start on a miss.
    ///
    /// This is how a per-node CSSOM edit finds itself inside the page's single
    /// concatenated cascade parse: the shell builds that parse's input as
    /// `imports_prefix + every <style> body + every <link> body`
    /// (`crates/shell/src/page_pipeline.rs`'s `build_page_cascade`) and copies
    /// each contributor in verbatim, so a node's own source text is a literal
    /// substring of the cascade's source. `from` lets a caller walking the
    /// node registry in document order keep a cursor, so two `<style>`
    /// elements with byte-identical bodies resolve to their own occurrence
    /// rather than both to the first one.
    ///
    /// An empty `needle` returns `None` rather than a zero-length match at
    /// `from`: an empty `<style>` contributes no bytes at all, so there is no
    /// occurrence to distinguish it from any other position, and answering
    /// would silently anchor its edits onto a neighbour's rules.
    pub fn locate_embedded_source(&self, needle: &str, from: usize) -> Option<(usize, usize)> {
        if needle.is_empty() {
            return None;
        }
        let src = self.source.as_deref()?;
        let at = src
            .get(from..)
            .and_then(|tail| tail.find(needle).map(|i| i + from))
            .or_else(|| src.find(needle))?;
        Some((at, at + needle.len()))
    }

    /// Translate a byte range of [`Self::source`] into the `cssRules` index
    /// range the rules from that range occupy — `(base, count)`.
    ///
    /// `base` is how many `cssRules` entries begin before `start`, i.e. the
    /// index the first rule of that range sits at; `count` is how many begin
    /// inside `[start, end)`. Together they are the address translation CSSOM-8
    /// вариант C needs: a `<style>` element's own `cssRules[i]` is this
    /// sheet's `cssRules[base + i]`, with no serialisation of anything and no
    /// assumption about how many sheets or `@import`s preceded it.
    ///
    /// Only meaningful on a **freshly parsed** sheet, where
    /// [`Self::top_level_spans`] is ascending and free of [`SYNTHETIC_SPAN`].
    /// Callers that mutate (the same-tick flush) clone the pristine cascade
    /// sheet, translate against the clone's untouched provenance, and throw
    /// the clone away — they never translate against an already-patched sheet.
    pub fn cssom_range_for_source_span(&self, start: usize, end: usize) -> (usize, usize) {
        let mut base = 0usize;
        let mut count = 0usize;
        for &span in &self.top_level_spans {
            if span == SYNTHETIC_SPAN {
                continue;
            }
            if span < start {
                base += 1;
            } else if span < end {
                count += 1;
            }
        }
        (base, count)
    }

    /// Replay a node's recorded CSSOM edits onto this sheet, with every
    /// top-level index shifted by `base` (from
    /// [`Self::cssom_range_for_source_span`]).
    ///
    /// Returns `false` if any single op did not apply, having still applied
    /// the rest. Each op goes through the very same public mutator the live
    /// native called on the node's own sheet, so a replayed edit and the
    /// `cssRules` the page can read back cannot drift apart in semantics —
    /// only in index base.
    ///
    /// CSSOM-8 срез 12: a [`CssomOp::SetMixinResultStyle`] that lands on a
    /// real nested `& {...}` rule (as opposed to `@result`'s own leading
    /// declarations run) mutates [`Self::mixin_rules`]`[..].result`, but that
    /// is not what the cascade sees — [`parse`] bakes every such nested rule
    /// into a literal, already-combined-selector top-level [`Rule`] once, at
    /// parse time ([`mixins::collect_mixin_nested_rules`]'s doc comment
    /// explains why it cannot run incrementally). Left alone, the baked copy
    /// goes stale the moment the source `result` item it was built from is
    /// edited: the write is visible to `.cssText`/a re-read through the
    /// mixin-result path, but never reaches the element `@apply` applied it
    /// to. [`Self::rerun_mixin_nested_rules`] re-derives the whole baked tail
    /// from the now-current `mixin_rules`, so this replays that step whenever
    /// at least one op actually touched a mixin's `result`.
    pub fn replay_cssom_ops(&mut self, base: usize, ops: &[CssomOp]) -> bool {
        let mut all_ok = true;
        let mut mixin_result_touched = false;
        for op in ops {
            let outcome = match op {
                CssomOp::InsertRule { index, text } => {
                    self.insert_rule(text, base + index).map(|_| ())
                }
                CssomOp::DeleteRule { index } => self.delete_rule(base + index),
                CssomOp::SetRuleStyle { index, css_text } => {
                    self.set_rule_style_text(base + index, css_text)
                }
                CssomOp::SetMediaChildStyle { media_index, child_index, css_text } => {
                    self.set_media_child_style_text(base + media_index, *child_index, css_text)
                }
                CssomOp::SetMixinResultStyle { mixin_index, path, css_text } => {
                    let outcome = self.set_mixin_result_style(base + mixin_index, path, css_text);
                    mixin_result_touched |= outcome.is_ok();
                    outcome
                }
                CssomOp::InsertRuleBodyApply { rule_index, index, text } => {
                    self.insert_rule_body_apply(base + rule_index, *index, text).map(|_| ())
                }
            };
            all_ok &= outcome.is_ok();
        }
        if mixin_result_touched {
            self.rerun_mixin_nested_rules();
        }
        all_ok
    }

    /// Re-derive every baked nested-rule copy [`mixins::collect_mixin_nested_rules`]
    /// appended at parse time, discarding the stale tail first.
    ///
    /// [`Self::rules`]`[0..n]` always holds exactly the `n` tags
    /// [`Self::top_level_order`] carries as [`TopLevelRuleKind::Style`], in the
    /// same relative order (every mutator that inserts/removes a `Style` tag —
    /// [`Self::insert_rule`]/[`Self::delete_rule`] — keeps the two in lockstep
    /// by construction); anything past that prefix is baked, untagged output
    /// of a previous [`mixins::collect_mixin_nested_rules`] run, safe to drop
    /// and rebuild wholesale. Mints a new revision — see [`Self::mark_mutated`].
    fn rerun_mixin_nested_rules(&mut self) {
        let tagged = self
            .top_level_order
            .iter()
            .filter(|k| **k == TopLevelRuleKind::Style)
            .count();
        self.rules.truncate(tagged);
        let extra = mixins::collect_mixin_nested_rules(self);
        self.rules.extend(extra);
        self.mark_mutated();
    }
}

/// One recorded CSSOM write against an owned sheet — CSSOM-8 вариант C.
///
/// The page cascade is an independent parse of every `<style>`/`<link>` body
/// concatenated together, so a write applied to one node's own
/// `Stylesheet` (which is what `document.styleSheets[i]` hands out) does not
/// reach it. Rather than serialising the mutated node back to CSS text and
/// re-parsing the page — which would need a byte-exact writer for every
/// at-rule CSSOM cannot represent, and would corrupt the whole page's styles
/// if that writer were ever wrong — each write is also recorded here and
/// **replayed** onto the freshly parsed cascade sheet on demand
/// ([`Stylesheet::replay_cssom_ops`]). A wrong address can then only misplace
/// the one edit it describes, and the page's own CSS is never rewritten.
///
/// Indices are in the owning node's own `cssRules` space; the base that maps
/// them into the cascade's space is resolved at replay time, so a recorded op
/// survives any number of cascade rebuilds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssomOp {
    /// `CSSStyleSheet.insertRule(text, index)`.
    InsertRule {
        /// Position in the owning node's own `cssRules`.
        index: usize,
        /// The rule text exactly as JS passed it.
        text: String,
    },
    /// `CSSStyleSheet.deleteRule(index)`.
    DeleteRule {
        /// Position in the owning node's own `cssRules`.
        index: usize,
    },
    /// A top-level `CSSStyleRule.style` write.
    SetRuleStyle {
        /// Position in the owning node's own `cssRules`.
        index: usize,
        /// The rule's whole new declaration list.
        css_text: String,
    },
    /// A `CSSStyleRule.style` write on a rule nested in a top-level `@media`.
    SetMediaChildStyle {
        /// The `@media` block's own position in the node's `cssRules`.
        media_index: usize,
        /// The rule's position inside that block (not rebased — a `@media`
        /// block's children are addressed relative to the block itself).
        child_index: usize,
        /// The rule's whole new declaration list.
        css_text: String,
    },
    /// A `.style` write on a node inside a top-level `@mixin`'s `@result`
    /// tree (CSSOM-8, вложенные правила).
    SetMixinResultStyle {
        /// The `@mixin`'s own position in the node's `cssRules`.
        mixin_index: usize,
        /// Path from `@result`'s own children down to the target node —
        /// not rebased, structural (see [`Stylesheet::set_mixin_result_style`]'s
        /// doc comment).
        path: Vec<usize>,
        /// The node's whole new declaration list.
        css_text: String,
    },
    /// `CSSGroupingRule.insertRule` of an `@apply` statement into a
    /// TOP-LEVEL style rule's own body.
    InsertRuleBodyApply {
        /// The owning style rule's position in the node's `cssRules`.
        rule_index: usize,
        /// Position in that rule's own `@apply`-marker sub-list — see
        /// [`Rule::insert_apply_marker`].
        index: usize,
        /// The rule text exactly as JS passed it.
        text: String,
    },
}

/// One `<style>`/`<link rel=stylesheet>` DOM node paired with its own parsed
/// sheet — the per-element granularity `document.styleSheets`/`element.sheet`
/// (CSSOM-1) need, as opposed to a page's single merged cascade [`Stylesheet`].
///
/// `node` is a raw node-id index rather than `lumen_dom::NodeId`: this crate
/// must not depend on `lumen_dom` (siblings in the architecture layering —
/// see the "Архитектурный пробел" section of
/// `docs/tasks/p1-cssom-1-stylesheets.md`), and every consumer of this type
/// already crosses that same node-id-as-`u32` boundary at the JS binding
/// layer. Built by `crates/shell/src/stylesheets.rs::build_stylesheet_node_registry`,
/// read by `crates/js`'s CSSOM-1 срез 3 natives.
#[derive(Debug, Clone)]
pub struct StylesheetNodeEntry {
    /// Owner `<style>`/`<link>` node id.
    pub node: u32,
    /// This element's own parsed sheet — independent of the page's merged
    /// cascade sheet.
    pub sheet: std::sync::Arc<Stylesheet>,
    /// `CSSStyleSheet.disabled`. Always `false` until CSSOM-1 срез 4.
    pub disabled: bool,
}

pub fn parse(input: &str) -> Stylesheet {
    let mut sheet = Parser::new(input).parse_stylesheet();
    // CSS Mixins L1: a nested style rule inside a mixin's `@result` can only
    // be resolved once the whole sheet — every `@apply` call site and every
    // (possibly forward-referenced) `@mixin` — is known. See
    // `mixins::collect_mixin_nested_rules`'s doc comment for why the extra
    // rules are appended here rather than by that function itself.
    let extra = mixins::collect_mixin_nested_rules(&sheet);
    // Appended without a `top_level_order` tag on purpose (they are not
    // `cssRules` entries of their own), so `top_level_spans` stays aligned.
    sheet.rules.extend(extra);
    // CSSOM-8 вариант C: remember the exact text, so a sheet parsed from
    // several concatenated `<style>`/`<link>` bodies can still map a byte
    // range back to a `cssRules` index range.
    sheet.source = Some(std::sync::Arc::from(input));
    sheet
}

/// Парсит содержимое HTML-атрибута `style="..."` — declaration-list без
/// окружающих фигурных скобок (CSS Style Attributes §2).
/// Используется для подключения inline-стилей к каскаду в `lumen-layout`
/// со specificity (1,0,0,0) согласно CSS Cascade L4 §6.4.3.
pub fn parse_inline_style(input: &str) -> Vec<Declaration> {
    Parser::new(input).parse_declaration_block()
}

/// Re-parses the raw text captured for a [`MIXIN_APPLY_MARKER`] declaration
/// (or an `@apply` found inside a mixin's own `@result`) back into an
/// [`ApplyRule`] — used by the layout crate's cascade-time mixin expansion,
/// which cannot call the parser's internal `Parser::parse_apply_rule`
/// directly. `input` is expected to already be positioned right after the
/// `apply` ident (i.e. the exact slice `parse_declaration_block_with_nesting`
/// stored), so this parses `[<name>][(<args>)] [{ <block> }] [;]`.
pub fn parse_apply_call(input: &str) -> Option<ApplyRule> {
    Parser::new(input).parse_apply_rule()
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

    /// Lookahead beyond the current position, `n` code points ahead
    /// (`n == 0` is equivalent to [`Self::peek`]).
    fn peek_at(&self, n: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(n)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while let Some(c) = self.peek() {
                // CSS Syntax §4.2 "whitespace": exactly these 5 ASCII
                // characters, not Rust's Unicode `White_Space` (which also
                // matches e.g. U+000B VT / U+0085 NEL, wrongly acting as a
                // descendant combinator — BUG-510).
                if matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ') {
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
        let mut view_transition_rules: Vec<ViewTransitionRule> = Vec::new();
        let mut container_rules: Vec<ContainerRule> = Vec::new();
        let mut color_profiles: Vec<ColorProfileRule> = Vec::new();
        let mut function_rules: Vec<FunctionRule> = Vec::new();
        let mut mixin_rules: Vec<MixinRule> = Vec::new();
        let mut top_level_order: Vec<TopLevelRuleKind> = Vec::new();
        let mut top_level_spans: Vec<usize> = Vec::new();
        let mut anon_counter: usize = 0;
        loop {
            self.skip_ws_and_comments();
            // CSSOM-8 вариант C: one `rule_start` per top-level construct,
            // stamped onto every tag that construct produced once the arms
            // below are done. Recorded here rather than at each
            // `top_level_order.push` site — there are eleven of them across
            // two match arms (a style rule flat-expands its CSS-Nesting
            // children and its bubbled at-rules into sibling tags), and they
            // all share this one source position by construction.
            let rule_start = self.pos;
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
                            AtRuleOutcome::Media(m) => {
                                media_rules.push(m);
                                top_level_order.push(TopLevelRuleKind::Media);
                            }
                            AtRuleOutcome::Import(i) => imports.push(i),
                            AtRuleOutcome::FontFace(f) => font_faces.push(*f),
                            AtRuleOutcome::FontPaletteValues(fp) => {
                                font_palette_values.push(fp)
                            }
                            AtRuleOutcome::ColorProfile(cp) => color_profiles.push(cp),
                            AtRuleOutcome::Function(f) => function_rules.push(f),
                            AtRuleOutcome::Mixin(m) => {
                                // Own `mixin_rules` index, stamped before the
                                // push — `@layer`-nested `@mixin`s (below)
                                // share this same vec but push with no tag
                                // at all, so a position count can't be used
                                // here (see `TopLevelRuleKind::Mixin`'s doc).
                                top_level_order
                                    .push(TopLevelRuleKind::Mixin(mixin_rules.len()));
                                mixin_rules.push(m);
                            }
                            AtRuleOutcome::LayerNames(names) => {
                                for n in names {
                                    if !layer_order.iter().any(|e| e == &n) {
                                        layer_order.push(n);
                                    }
                                }
                            }
                            AtRuleOutcome::LayerBlock { name, rules: lr, mixin_rules: lmr } => {
                                let resolved_name = name.unwrap_or_else(|| {
                                    anon_counter += 1;
                                    format!("__anon_{anon_counter}__")
                                });
                                if !layer_order.iter().any(|e| e == &resolved_name) {
                                    layer_order.push(resolved_name.clone());
                                }
                                for mut m in lmr {
                                    m.layer = Some(resolved_name.clone());
                                    mixin_rules.push(m);
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
                            AtRuleOutcome::ViewTransition(v) => {
                                view_transition_rules.push(v)
                            }
                            AtRuleOutcome::None => {}
                        }
                    }
                }
                Some(_) => {
                    let before = self.pos;
                    if let Some((rule, nested, nested_at)) = self.parse_rule() {
                        rules.push(rule);
                        top_level_order.push(TopLevelRuleKind::Style);
                        // CSS Nesting L1: flat-expanded nested rules, each its own
                        // top-level `cssRules` entry right after the parent (best
                        // approximation available — this engine does not keep a
                        // nested rule as a child of its parent's own `cssRules`).
                        for r in nested {
                            rules.push(r);
                            top_level_order.push(TopLevelRuleKind::Style);
                        }
                        // CSS Nesting L1 §5: nested at-rules bubble up into the stylesheet.
                        for at in nested_at {
                            match at {
                                AtRuleOutcome::Media(m) => {
                                    media_rules.push(m);
                                    top_level_order.push(TopLevelRuleKind::Media);
                                }
                                AtRuleOutcome::Supports(s) => supports_rules.push(s),
                                AtRuleOutcome::LayerNames(names) => {
                                    for n in names {
                                        if !layer_order.iter().any(|e| e == &n) {
                                            layer_order.push(n);
                                        }
                                    }
                                }
                                AtRuleOutcome::LayerBlock { name, rules: lr, mixin_rules: lmr } => {
                                    let resolved = name.unwrap_or_else(|| {
                                        anon_counter += 1;
                                        format!("__anon_{anon_counter}__")
                                    });
                                    if !layer_order.iter().any(|e| e == &resolved) {
                                        layer_order.push(resolved.clone());
                                    }
                                    for mut m in lmr {
                                        m.layer = Some(resolved.clone());
                                        mixin_rules.push(m);
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
            // Stamp `rule_start` onto every tag this iteration appended. The
            // `None => break` arm leaves the loop before reaching here, and it
            // appends nothing, so the two vecs cannot drift apart.
            while top_level_spans.len() < top_level_order.len() {
                top_level_spans.push(rule_start);
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
            view_transition_rules,
            container_rules,
            color_profiles,
            function_rules,
            mixin_rules,
            top_level_order,
            top_level_spans,
            // Filled by `parse` — `parse_stylesheet` is also reached from
            // contexts that have no standalone source string to attribute.
            source: None,
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
                // CSS Nesting L1 §5: nested at-rule — except `@apply`
                // (CSS Mixins L1), which is a declaration-position at-rule,
                // not a nested conditional-group rule: pushed as a marker
                // `Declaration` (see `MIXIN_APPLY_MARKER`'s doc comment) so
                // it keeps its exact source position relative to sibling
                // declarations for cascade ordering.
                Some('@') => {
                    let at_start = self.pos;
                    self.consume(); // '@'
                    let ident = self.parse_ident().unwrap_or_default();
                    if ident.eq_ignore_ascii_case("apply") {
                        let raw_start = self.pos;
                        if self.parse_apply_rule().is_some() {
                            let raw = self.input[raw_start..self.pos].to_string();
                            decls.push(Declaration {
                                property: MIXIN_APPLY_MARKER.to_string(),
                                value: raw,
                                important: false,
                            });
                        }
                    } else {
                        self.pos = at_start;
                        let ats = self.parse_nested_at_rule(parent_sels);
                        at_rules.extend(ats);
                    }
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
            let mut outcomes = vec![AtRuleOutcome::LayerBlock {
                name: layer_name,
                rules,
                // `@layer` nested inside an ordinary style rule's own body
                // (CSS Nesting) goes through `parse_nested_group_body`, not
                // `parse_layer_at_rule`'s block-form loop — this slice's
                // `@mixin`-inside-`@layer` special case (BUG-518 срез 3)
                // only covers the latter; a `@mixin` here remains
                // unsupported, same as before this change.
                mixin_rules: Vec::new(),
            }];
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

    /// CSS Syntax L3 §5.4.3 "consume a qualified rule": a `}` ends the
    /// *enclosing* block, so recovery from a malformed prelude stops right
    /// before it and leaves it for the caller's own loop — it is not part of
    /// the discarded construct.
    ///
    /// BUG-1068: consuming it desynchronised the parser for the rest of the
    /// sheet. A declaration starting with a non-ident character — the IE7
    /// star hack `*zoom:1`, still shipped by minified vendor bundles — enters
    /// [`Self::parse_implicit_nested_rule`] (CSS Nesting L1 §4 lets a nested
    /// rule start with `*`), finds no `{`, and recovers; swallowing the
    /// block's own `}` made every *following* top-level rule parse as a
    /// nested `parent descendant` rule that matches nothing. One such
    /// declaration in `rust-lang.org`'s vendor bundle (`.cf{*zoom:1}`, byte
    /// 7355 of 73 375) cost the page all 650+ layout utilities after it, so
    /// the site rendered unstyled — see `bugs/BUG-1068-FIXED.md`.
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
                '}' => return,
                _ => {
                    self.consume();
                }
            }
        }
    }

    fn parse_ident(&mut self) -> Option<String> {
        let first = self.peek()?;
        if !is_ident_start(first) && !self.at_escape_start() {
            return None;
        }
        let mut s = String::new();
        loop {
            match self.peek() {
                Some('\\') if self.at_escape_start() => {
                    self.consume();
                    if let Some(c) = self.consume_escaped_code_point() {
                        s.push(c);
                    }
                }
                Some(c) if is_ident_continue(c) => {
                    self.consume();
                    s.push(c);
                }
                _ => break,
            }
        }
        Some(s)
    }

    /// CSS Syntax L3 §4.3.8 "check if two code points are a valid escape":
    /// a `\` starts an escape unless it is immediately followed by a
    /// newline (or nothing).
    fn at_escape_start(&self) -> bool {
        self.peek() == Some('\\') && !matches!(self.peek_at(1), None | Some('\n'))
    }

    /// CSS Syntax L3 §4.3.7 "consume an escaped code point". Caller has
    /// already consumed the leading `\`.
    fn consume_escaped_code_point(&mut self) -> Option<char> {
        match self.peek() {
            Some(c) if c.is_ascii_hexdigit() => {
                let mut hex = String::new();
                while hex.len() < 6 {
                    match self.peek() {
                        Some(h) if h.is_ascii_hexdigit() => {
                            hex.push(h);
                            self.consume();
                        }
                        _ => break,
                    }
                }
                // A single trailing whitespace code point terminates the
                // escape without becoming part of the ident itself.
                match self.peek() {
                    Some('\r') => {
                        self.consume();
                        if self.peek() == Some('\n') {
                            self.consume();
                        }
                    }
                    Some('\t' | '\n' | '\x0C' | ' ') => {
                        self.consume();
                    }
                    _ => {}
                }
                let code = u32::from_str_radix(&hex, 16).unwrap_or(0);
                if code == 0 || char::from_u32(code).is_none() || (0xD800..=0xDFFF).contains(&code)
                {
                    Some('\u{FFFD}')
                } else {
                    char::from_u32(code)
                }
            }
            Some(c) => {
                self.consume();
                Some(c)
            }
            None => Some('\u{FFFD}'),
        }
    }

}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '-' || c >= '\u{00A0}'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// Hard cap on selectors a single [`expand_nesting`] call can produce.
///
/// CSS Nesting L1 doesn't bound cartesian growth (`parents.len() *
/// nested.len()`), and the expanded list becomes the `parents` of the next
/// nesting level — so on malformed input where recovery keeps entering
/// [`Parser::parse_implicit_nested_rule`] instead of terminating, the
/// selector count compounds multiplicatively *per level of nesting depth*
/// instead of growing additively with input size. A 676-byte fuzzer
/// minimization reached 50 MiB / ×74 000 blowup this way (BUG-788). Real
/// stylesheets never come close to four figures of selectors from nesting
/// alone, so truncating here only ever discards pathological expansion, not
/// legitimate rules.
const MAX_EXPANDED_SELECTORS: usize = 1024;

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
    'outer: for parent in parents {
        for n in nested {
            if result.len() >= MAX_EXPANDED_SELECTORS {
                break 'outer;
            }
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

#[cfg(test)]
#[path = "parser/tests/recovery.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "parser/tests/view_transitions.rs"]
mod view_transitions_tests;
