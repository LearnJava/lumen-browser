//! CSS Properties and Values L1 — валидация значения зарегистрированного
//! `@property` против его `syntax`-дескриптора и подстановка `initial-value`.
//!
//! Перенесено батчем SPLIT-ST13 из `crates/engine/layout/src/style.rs`
//! (анкер `fn apply_property_initial_values`) без правок тел.

use crate::style::{parse_color, parse_css_wide_keyword, parse_length, CustomProps, Length};
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

/// CSS Properties and Values L1 §2 — упрощённая валидация значения
/// custom property против `syntax`-дескриптора.
///
/// Поддерживаются:
/// - `*` — универсал (любое значение проходит);
/// - `<length>` — px, em, rem, vh, vw, vmin, vmax (но не `%`);
/// - `<percentage>` — число с суффиксом `%`;
/// - `<length-percentage>` — union;
/// - `<color>` — любая форма, которую парсит `parse_color`;
/// - `<integer>` — целое со знаком;
/// - `<number>` — число с плавающей точкой;
/// - `<angle>` — `deg` / `rad` / `turn` / `grad`;
/// - `<time>` — `s` / `ms` (CSS Values L4 §8);
/// - `<resolution>` — `dpi` / `dpcm` / `dppx` / `x` (CSS Values L4 §9.1);
/// - `<custom-ident>` — идентификатор, не совпадающий с CSS-wide keyword.
///
/// Union через `|` — match если хоть одна альтернатива принимает. Прочие
/// типы (`<image>`, `<url>`, `<transform-function>`, и т.д.) и multipliers
/// (`+`, `#`) в Phase 0 трактуются как universal — возвращают `true`,
/// чтобы не отбраковывать корректные value у потребителей этих типов.
pub fn validate_against_syntax(value: &str, syntax: &str) -> bool {
    let syntax = syntax.trim();
    if syntax == "*" {
        return true;
    }
    let value = value.trim();
    // Union по `|`.
    for alt in syntax.split('|') {
        let alt = alt.trim();
        let matched = match alt {
            "<length>" => matches_syntax_length(value),
            "<percentage>" => matches_syntax_percentage(value),
            "<length-percentage>" => {
                matches_syntax_length(value) || matches_syntax_percentage(value)
            }
            "<color>" => parse_color(value).is_some(),
            "<integer>" => matches_syntax_integer(value),
            "<number>" => matches_syntax_number(value),
            "<angle>" => matches_syntax_angle(value),
            "<time>" => matches_syntax_time(value),
            "<resolution>" => matches_syntax_resolution(value),
            "<custom-ident>" => matches_syntax_custom_ident(value),
            // Неизвестный тип — permissive, чтобы не блокировать корректные
            // declarations с пока-неподдержанными syntax-формами.
            _ => true,
        };
        if matched {
            return true;
        }
    }
    false
}

fn matches_syntax_length(value: &str) -> bool {
    // <length> = px/em/rem/vh/vw/vmin/vmax/calc(...) — без `%`.
    match parse_length(value) {
        Some(Length::Percent(_)) => false,
        Some(_) => true,
        None => false,
    }
}

fn matches_syntax_percentage(value: &str) -> bool {
    matches!(parse_length(value), Some(Length::Percent(_)))
}

fn matches_syntax_integer(value: &str) -> bool {
    value.parse::<i64>().is_ok()
}

fn matches_syntax_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

fn matches_syntax_angle(value: &str) -> bool {
    // Number + один из суффиксов: deg, rad, turn, grad.
    for suffix in ["deg", "rad", "turn", "grad"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_time(value: &str) -> bool {
    // CSS Values L4 §8 — <time> с суффиксами `s` или `ms`.
    // Порядок важен: `ms` проверяем раньше `s`, иначе `200ms` распарсится
    // как 200m + остаток `s` (а `200m` не валидный number → false).
    for suffix in ["ms", "s"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_resolution(value: &str) -> bool {
    // CSS Values L4 §9.1 — <resolution> с суффиксами `dppx`/`dpcm`/`dpi`/`x`.
    // `dppx` проверяем раньше `dpi`/`dpcm` (длинный суффикс), `x` — последним
    // (резервный alias dppx; HTML5 media queries).
    for suffix in ["dppx", "dpcm", "dpi", "x"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_syntax_custom_ident(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // CSS-wide keywords нельзя использовать как custom-ident.
    if parse_css_wide_keyword(value).is_some() {
        return false;
    }
    // Также запрещены `default` (CSS spec) и `none` в большинстве контекстов.
    // Простая проверка: ident начинается с letter / `_` / `-`, дальше —
    // alphanumeric / `-` / `_`. ASCII-only для простоты.
    let mut chars = value.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
