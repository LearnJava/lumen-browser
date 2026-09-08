//! CSS Properties and Values L1 — валидация значения зарегистрированного
//! `@property` против его `syntax`-дескриптора и подстановка `initial-value`.
//!
//! Перенесено батчем SPLIT-ST13 из `crates/engine/layout/src/style.rs`
//! (анкер `fn apply_property_initial_values`) без правок тел.
//!
//! BUG-531: [`validate_against_syntax`] now delegates its actual grammar
//! parsing and value matching to [`crate::style::syntax_string`] (which also
//! backs `CSS.registerProperty()`'s JS-facing validation) instead of the
//! ad hoc per-type checks this file used to carry directly. Behaviour here
//! stays permissive when `syntax` itself doesn't parse — an already-stored
//! `@property` rule with a malformed descriptor is a pre-existing concern
//! this bug doesn't reach — and, unlike the JS-facing entry point, never
//! rejects a font-relative length unit: that restriction only applies to a
//! registration's *initial value*, not to an ordinary declaration on a real
//! element (which resolves `em`/`rem` normally).

use crate::style::syntax_string::cascade_validate_against_syntax;
use crate::style::CustomProps;
use lumen_css_parser::PropertyRule;
use std::collections::HashMap;

/// CSS Properties and Values L1 §1.1: для каждого зарегистрированного
/// custom property, у которого нет значения в `custom_props`, подставляет
/// `initial-value` (если он указан). Невызов для `inherits: true` имени
/// с унаследованным значением — потому что `contains_key` уже возвращает
/// true. Для `inherits: false` имени родительское значение было выпилено
/// в `compute_style` через `retain`.
/// BUG-341 S9: takes [`CustomProps`] rather than the bare map so the
/// copy-on-write copy happens only if a value is really substituted — the common
/// case (no `@property` rules at all, or all of them already resolved) leaves the
/// node sharing its parent's allocation.
pub(in crate::style) fn apply_property_initial_values(
    custom_props: &mut CustomProps,
    registry: &HashMap<&str, &PropertyRule>,
) {
    for (name, p) in registry {
        if custom_props.contains_key(*name) {
            continue;
        }
        if let Some(iv) = &p.initial_value {
            // CSS Properties and Values L1 §1.1: initial-value валидируется
            // против syntax. Per spec — невалидный initial делает @property
            // невалидным целиком; Phase 0 более снисходителен и просто
            // не подставляет неподходящий initial (потомок без декларации
            // получит inherited или ничего).
            if validate_against_syntax(iv, &p.syntax) {
                custom_props.make_mut().insert((*name).to_string(), iv.clone());
            }
        }
    }
}

/// CSS Properties and Values L1 §2 — валидация значения зарегистрированного
/// custom property против `syntax`-дескриптора; делегирует грамматику и
/// сопоставление типов [`crate::style::syntax_string`] (BUG-531). Malformed
/// `syntax` (не должно происходить для `@property`, которое обязано было
/// пройти `parse_property_body`, но на всякий случай) — permissive `true`,
/// чтобы не откатывать ранее принятые declarations.
pub fn validate_against_syntax(value: &str, syntax: &str) -> bool {
    cascade_validate_against_syntax(value, syntax).unwrap_or(true)
}
