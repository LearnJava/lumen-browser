//! CSS-парсер для Lumen.
//!
//! Поддерживается `selector_list { decl_list }`, селекторы type / class / id /
//! universal / attribute / pseudo-class, compound и complex selectors с
//! combinator-ами (` `, `>`, `+`, `~`), specificity по CSS3. Structural pseudo:
//! `:first-child` / `:last-child` / `:only-child` / `:empty` / `:root` /
//! `:*-of-type` / `:nth-*(an+b)`. CSS Selectors L4 functional pseudo:
//! `:not(selector-list)` (§5.4 — selector-list с combinator-ами и nested
//! `:not(:not(...))`, specificity = max-of-list), `:is(selector-list)` и
//! `:where(selector-list)` — внутри разрешены любые complex-селекторы;
//! specificity для `:is` / `:not` берётся как максимум по списку, для
//! `:where` — всегда ноль. Декларации хранятся как пары строк (property /
//! value) — типизация значений (length / color / calc / `--var`) появится позже.
//!
//! Не поддерживается (отложено): namespace prefix в селекторах.

pub mod parser;

pub use parser::{
    parse, parse_inline_style, parse_media_query, parse_supports_condition, AttrOp, AttrSelector, ColorScheme,
    Combinator, CompoundSelector, ComplexSelector, ContainerRule, CounterStyleRule, Declaration,
    DirArg, FontFaceRule, FontFaceSource, FontFaceSourceKind, ImportRule, Keyframe, KeyframesRule,
    LayerRule, MediaCondition, MediaContext, MediaFeature, MediaOrientation, MediaQuery,
    MediaQueryClause, MediaRule, NthSpec, PageRule, PropertyRule, PseudoClass, RelativeSelector,
    Rule, ScopeRule,
    SimpleSelector, Specificity, StartingStyleRule, Stylesheet, SupportsCondition, SupportsRule,
};
