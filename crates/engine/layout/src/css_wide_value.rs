//! CSS-wide keyword в значениях `@keyframes` и Web Animations (BUG-1437).
//!
//! `from { tab-size: initial }` и `element.animate({ textIndent: ['inherit', '20px'] })`
//! интерполируют *вычисленное* значение, а не слово. JS-оверлей анимаций
//! (`web_api_shim_tail_b.js`) разворачивает `initial`/`unset` через эти две функции;
//! `inherit` берёт вычисленное значение родителя из снимка.

use crate::selector_query::computed_style_to_map;
use crate::style::ComputedStyle;

/// Initial value свойства `prop` (kebab-case) в том виде, в каком его отдаёт
/// `getComputedStyle()`, или `None`, если снимок такого свойства не несёт.
pub fn initial_computed_value(prop: &str) -> Option<String> {
    thread_local! {
        static ROOT_MAP: std::collections::HashMap<String, String> =
            computed_style_to_map(&ComputedStyle::root());
    }
    ROOT_MAP.with(|m| m.get(prop).cloned())
}

/// Наследуется ли свойство (`unset` = `inherit` для него, иначе `initial`).
pub fn is_inherited_property(prop: &str) -> bool {
    matches!(
        prop,
        "accent-color"
            | "border-collapse"
            | "border-spacing"
            | "caption-side"
            | "caret-color"
            | "caret-shape"
            | "interactivity"
            | "input-security"
            | "caret-animation"
            | "caret"
            | "color"
            | "color-adjust"
            | "color-interpolation"
            | "color-scheme"
            | "cursor"
            | "direction"
            | "dynamic-range-limit"
            | "empty-cells"
            | "fill"
            | "fill-opacity"
            | "fill-rule"
            | "font"
            | "font-family"
            | "font-feature-settings"
            | "font-kerning"
            | "font-language-override"
            | "font-optical-sizing"
            | "font-palette"
            | "font-size"
            | "font-size-adjust"
            | "font-stretch"
            | "font-style"
            | "font-synthesis"
            | "font-variant"
            | "font-variant-alternates"
            | "font-variant-caps"
            | "font-variant-east-asian"
            | "font-variant-emoji"
            | "font-variant-ligatures"
            | "font-variant-numeric"
            | "font-variant-position"
            | "font-variation-settings"
            | "font-weight"
            | "forced-color-adjust"
            | "hanging-punctuation"
            | "hyphenate-character"
            | "hyphenate-limit-chars"
            | "hyphens"
            | "image-orientation"
            | "image-rendering"
            | "letter-spacing"
            | "line-break"
            | "line-height"
            | "list-style"
            | "list-style-image"
            | "list-style-position"
            | "list-style-type"
            | "orphans"
            | "overflow-wrap"
            | "paint-order"
            | "pointer-events"
            | "print-color-adjust"
            | "quotes"
            | "ruby-align"
            | "ruby-position"
            | "scrollbar-color"
            | "shape-rendering"
            | "stroke"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-miterlimit"
            | "stroke-opacity"
            | "stroke-width"
            | "tab-size"
            | "text-align"
            | "text-align-last"
            | "text-anchor"
            | "text-autospace"
            | "text-combine-upright"
            | "text-emphasis"
            | "text-emphasis-color"
            | "text-emphasis-position"
            | "text-emphasis-style"
            | "text-indent"
            | "text-justify"
            | "text-orientation"
            | "text-rendering"
            | "text-shadow"
            | "text-size-adjust"
            | "text-spacing-trim"
            | "text-transform"
            | "text-underline-offset"
            | "text-underline-position"
            | "text-wrap"
            | "text-wrap-mode"
            | "text-wrap-style"
            | "visibility"
            | "white-space"
            | "white-space-collapse"
            | "widows"
            | "word-break"
            | "word-spacing"
            | "word-wrap"
            | "writing-mode"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_values_come_from_the_root_style() {
        assert_eq!(initial_computed_value("tab-size").as_deref(), Some("8"));
        assert_eq!(initial_computed_value("text-indent").as_deref(), Some("0px"));
        assert_eq!(initial_computed_value("line-break").as_deref(), Some("auto"));
        assert_eq!(initial_computed_value("no-such-property"), None);
    }

    #[test]
    fn inheritance_table_separates_text_from_box_properties() {
        assert!(is_inherited_property("tab-size"));
        assert!(is_inherited_property("text-indent"));
        assert!(!is_inherited_property("width"));
        assert!(!is_inherited_property("opacity"));
    }
}
