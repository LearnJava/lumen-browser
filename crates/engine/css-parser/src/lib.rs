//! CSS-парсер для Lumen.
//!
//! Поддерживается `selector_list { decl_list }`, селекторы type / class / id /
//! universal / attribute / pseudo-class, compound и complex selectors с
//! combinator-ами (` `, `>`, `+`, `~`), specificity по CSS3. Декларации
//! хранятся как пары строк (property / value) — типизация значений (length /
//! color / calc / `--var`) появится позже.
//!
//! Не поддерживается (отложено): функциональные pseudo (`:nth-child`, `:not`),
//! case-insensitive `[attr=val i]`, namespace prefix в селекторах.

pub mod parser;

pub use parser::{
    AttrOp, AttrSelector, Combinator, CompoundSelector, ComplexSelector, Declaration, PseudoClass,
    Rule, SimpleSelector, Specificity, Stylesheet, parse,
};
