//! WHATWG Compat Standard §3 «CSS Simple Aliases» (COMPAT-CSS-ALIAS).
//!
//! 51 имя `-webkit-*` — настоящие алиасы беспрефиксных свойств: у них один
//! и тот же computed value, `cssText` и `getComputedStyle` отдают каноническое
//! имя (как Chrome). Поэтому имя разрешается один раз при разборе декларации
//! ([`canonical_property_name`], вызывается из `parse_declaration`), и весь
//! каскад видит только беспрефиксную форму; `-webkit-transform: x; transform: y`
//! в одном блоке подчиняются обычному порядку источника.
//!
//! Не входят: `-webkit-appearance`, `-webkit-user-select`, `-webkit-line-clamp`,
//! `-webkit-box-orient`, `-webkit-text-size-adjust` — у них свои дуги в `apply_*`
//! (это не простые алиасы Compat §3).

/// `(алиас, каноническое имя)`, оба в нижнем регистре. Список —
/// <https://compat.spec.whatwg.org/#css-simple-aliases>.
pub const COMPAT_CSS_ALIASES: &[(&str, &str)] = &[
    ("-webkit-align-content", "align-content"),
    ("-webkit-align-items", "align-items"),
    ("-webkit-align-self", "align-self"),
    ("-webkit-animation", "animation"),
    ("-webkit-animation-delay", "animation-delay"),
    ("-webkit-animation-direction", "animation-direction"),
    ("-webkit-animation-duration", "animation-duration"),
    ("-webkit-animation-fill-mode", "animation-fill-mode"),
    ("-webkit-animation-iteration-count", "animation-iteration-count"),
    ("-webkit-animation-name", "animation-name"),
    ("-webkit-animation-play-state", "animation-play-state"),
    ("-webkit-animation-timing-function", "animation-timing-function"),
    ("-webkit-backface-visibility", "backface-visibility"),
    ("-webkit-background-clip", "background-clip"),
    ("-webkit-background-origin", "background-origin"),
    ("-webkit-background-size", "background-size"),
    ("-webkit-border-bottom-left-radius", "border-bottom-left-radius"),
    ("-webkit-border-bottom-right-radius", "border-bottom-right-radius"),
    ("-webkit-border-radius", "border-radius"),
    ("-webkit-border-top-left-radius", "border-top-left-radius"),
    ("-webkit-border-top-right-radius", "border-top-right-radius"),
    ("-webkit-box-shadow", "box-shadow"),
    ("-webkit-box-sizing", "box-sizing"),
    ("-webkit-filter", "filter"),
    ("-webkit-flex", "flex"),
    ("-webkit-flex-basis", "flex-basis"),
    ("-webkit-flex-direction", "flex-direction"),
    ("-webkit-flex-flow", "flex-flow"),
    ("-webkit-flex-grow", "flex-grow"),
    ("-webkit-flex-shrink", "flex-shrink"),
    ("-webkit-flex-wrap", "flex-wrap"),
    ("-webkit-justify-content", "justify-content"),
    ("-webkit-mask", "mask"),
    ("-webkit-mask-clip", "mask-clip"),
    ("-webkit-mask-composite", "mask-composite"),
    ("-webkit-mask-image", "mask-image"),
    ("-webkit-mask-origin", "mask-origin"),
    ("-webkit-mask-position", "mask-position"),
    ("-webkit-mask-repeat", "mask-repeat"),
    ("-webkit-mask-size", "mask-size"),
    ("-webkit-order", "order"),
    ("-webkit-perspective", "perspective"),
    ("-webkit-perspective-origin", "perspective-origin"),
    ("-webkit-transform", "transform"),
    ("-webkit-transform-origin", "transform-origin"),
    ("-webkit-transform-style", "transform-style"),
    ("-webkit-transition", "transition"),
    ("-webkit-transition-delay", "transition-delay"),
    ("-webkit-transition-duration", "transition-duration"),
    ("-webkit-transition-property", "transition-property"),
    ("-webkit-transition-timing-function", "transition-timing-function"),
];

/// Каноническое имя для одного из 51 алиаса Compat §3 (`None` — не алиас).
/// Сравнение ASCII-регистронезависимое (имена свойств CSS нечувствительны
/// к регистру); результат всегда в нижнем регистре.
pub fn canonical_property_name(name: &str) -> Option<&'static str> {
    // Быстрый отказ: все алиасы начинаются с `-webkit-`; подавляющее
    // большинство деклараций проходит без поиска по таблице.
    if !name.get(..8)?.eq_ignore_ascii_case("-webkit-") {
        return None;
    }
    COMPAT_CSS_ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
        .map(|&(_, canon)| canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_aliases_each_map_to_the_unprefixed_name() {
        assert_eq!(COMPAT_CSS_ALIASES.len(), 51);
        for &(alias, canon) in COMPAT_CSS_ALIASES {
            assert_eq!(alias.strip_prefix("-webkit-"), Some(canon));
            assert_eq!(canonical_property_name(alias), Some(canon));
            assert_eq!(canonical_property_name(&alias.to_ascii_uppercase()), Some(canon));
        }
    }

    #[test]
    fn non_aliases_pass_through() {
        for n in ["color", "-webkit-appearance", "-webkit-line-clamp", "-moz-transform", "-web", "--x", "é"] {
            assert_eq!(canonical_property_name(n), None);
        }
    }

    #[test]
    fn every_alias_target_is_a_supported_property() {
        for &(alias, canon) in COMPAT_CSS_ALIASES {
            assert!(crate::SUPPORTED_PROPERTIES.contains(&canon), "{canon} ({alias}) not supported");
        }
    }
}
