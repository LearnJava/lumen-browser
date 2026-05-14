//! CSS-парсер для Lumen.
//!
//! Поддерживается `selector_list { decl_list }`, селекторы type / class / id /
//! universal / attribute / pseudo-class, compound и complex selectors с
//! combinator-ами (` `, `>`, `+`, `~`), specificity по CSS3. Structural pseudo:
//! `:first-child` / `:last-child` / `:only-child` / `:empty` / `:root` /
//! `:*-of-type` / `:nth-*(an+b)` / `:not(compound)`. CSS4 functional pseudo:
//! `:is(selector-list)` и `:where(selector-list)` — внутри разрешены любые
//! complex-селекторы; specificity для `:is` берётся как максимум по списку,
//! для `:where` — всегда ноль. Декларации хранятся как пары строк (property /
//! value) — типизация значений (length / color / calc / `--var`) появится позже.
//!
//! Не поддерживается (отложено): `:has(...)`, `:not(complex)` со списком
//! селекторов или combinator-ами, case-insensitive `[attr=val i]`, namespace
//! prefix в селекторах.

pub mod parser;

pub use parser::{
    parse, parse_media_query, parse_supports_condition, AttrOp, AttrSelector, ColorScheme,
    Combinator, CompoundSelector, ComplexSelector, ContainerRule, CounterStyleRule, Declaration,
    FontFaceRule, FontFaceSource, FontFaceSourceKind, ImportRule, Keyframe, KeyframesRule,
    LayerRule, MediaCondition, MediaContext, MediaFeature, MediaOrientation, MediaQuery,
    MediaRule, NthSpec, PageRule, PropertyRule, PseudoClass, RelativeSelector, Rule, ScopeRule,
    SimpleSelector, Specificity, StartingStyleRule, Stylesheet, SupportsCondition, SupportsRule,
};
