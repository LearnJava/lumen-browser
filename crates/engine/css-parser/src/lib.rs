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
    AttrOp, AttrSelector, Combinator, CompoundSelector, ComplexSelector, Declaration, NthSpec,
    PseudoClass, Rule, SimpleSelector, Specificity, Stylesheet, parse,
};
