//! CSS-парсер для Lumen.
//!
//! Phase 0 — минимальный парсер правил `selector_list { decl_list }`.
//! Селекторы: type / class / id / universal. Декларации хранятся как пары
//! строк (property/value). At-rules (`@media`, `@import`) и неизвестные
//! комбинаторы пропускаются.
//!
//! Не поддерживается (отложено): pseudo-classes / pseudo-elements,
//! descendant / child / sibling combinators, attribute selectors,
//! типизированные значения, calc(), переменные `--foo`, специфичность.

pub mod parser;

pub use parser::{Declaration, Rule, Selector, Stylesheet, parse};
