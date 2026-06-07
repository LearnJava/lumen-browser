//! CSS Fonts L4 §13 — `@font-palette-values` palette resolver.
//!
//! Pure computation: given a stylesheet's `font_palette_values` rules and
//! an element's `font-palette` property value (e.g. `"--my-palette"`), resolves
//! the CPAL index overrides to apply when rendering COLR color glyphs.
//!
//! Entry point: [`resolve_font_palette_overrides`].
//!
//! # P4 handoff
//!
//! Add `font_palette: String` to `ComputedStyle` (parsed from `font-palette`
//! CSS property — values: `normal`, `light`, `dark`, or a `<dashed-ident>`
//! like `--my-palette`). In `emit_text_fragments()` in `paint/display_list.rs`,
//! call `resolve_font_palette_overrides(stylesheet, &style.font_palette, &font_family)`
//! to get `Vec<(u16, Color)>` overrides and pass them to the glyph atlas.
//!
//! // CSS: font-palette

use lumen_css_parser::FontPaletteValuesRule;

use crate::style::{parse_color, Color};

/// Resolved CPAL color override: `(palette_index, color)`.
/// Renderer substitutes these colors when painting COLR v0/v1 glyph layers.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteColorOverride {
    /// 0-based CPAL palette entry index to override.
    pub index: u16,
    /// Replacement color for this palette slot.
    pub color: Color,
}

/// Resolves `@font-palette-values` overrides for a given element.
///
/// `palette_name` is the computed value of `font-palette` (e.g. `"--cool"`).
/// `font_family` is the first resolved font family for the element.
///
/// Returns `None` if:
/// - `palette_name` is `"normal"`, `"light"`, or `"dark"` (UA-defined palettes,
///   resolved by the renderer from the CPAL table directly), or
/// - no matching `@font-palette-values` rule exists for this name + family.
///
/// Returns `Some(vec)` with colour overrides to apply on top of `base_palette`.
pub fn resolve_font_palette_overrides(
    rules: &[FontPaletteValuesRule],
    palette_name: &str,
    font_family: &str,
) -> Option<ResolvedFontPalette> {
    if matches!(palette_name, "normal" | "light" | "dark" | "") {
        return None;
    }
    let rule = rules.iter().find(|r| {
        r.name == palette_name
            && r.font_family
                .as_deref()
                .is_none_or(|f| f.eq_ignore_ascii_case(font_family))
    })?;

    let overrides: Vec<PaletteColorOverride> = rule
        .override_colors
        .iter()
        .filter_map(|(idx, color_str)| {
            let color = parse_color(color_str)?;
            Some(PaletteColorOverride { index: *idx, color })
        })
        .collect();

    Some(ResolvedFontPalette {
        base_palette: rule.base_palette,
        overrides,
    })
}

/// Output of [`resolve_font_palette_overrides`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFontPalette {
    /// Which built-in CPAL palette to start from (0 = default). `None` means 0.
    pub base_palette: Option<u16>,
    /// Per-slot color overrides on top of `base_palette`.
    pub overrides: Vec<PaletteColorOverride>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_css_parser::FontPaletteValuesRule;

    fn rule(name: &str, family: Option<&str>, base: Option<u16>, oc: Vec<(u16, &str)>) -> FontPaletteValuesRule {
        FontPaletteValuesRule {
            name: name.to_string(),
            font_family: family.map(str::to_string),
            base_palette: base,
            override_colors: oc.into_iter().map(|(i, s)| (i, s.to_string())).collect(),
        }
    }

    #[test]
    fn returns_none_for_normal() {
        assert!(resolve_font_palette_overrides(&[], "normal", "MyFont").is_none());
    }

    #[test]
    fn returns_none_for_light_dark() {
        assert!(resolve_font_palette_overrides(&[], "light", "X").is_none());
        assert!(resolve_font_palette_overrides(&[], "dark", "X").is_none());
    }

    #[test]
    fn returns_none_when_no_matching_rule() {
        let rules = vec![rule("--other", None, None, vec![])];
        assert!(resolve_font_palette_overrides(&rules, "--my-palette", "F").is_none());
    }

    #[test]
    fn matches_by_name() {
        let rules = vec![rule("--cool", None, Some(1), vec![(0, "#ff0000")])];
        let r = resolve_font_palette_overrides(&rules, "--cool", "AnyFont").unwrap();
        assert_eq!(r.base_palette, Some(1));
        assert_eq!(r.overrides.len(), 1);
        assert_eq!(r.overrides[0].index, 0);
        assert_eq!(r.overrides[0].color.r, 255);
        assert_eq!(r.overrides[0].color.g, 0);
        assert_eq!(r.overrides[0].color.b, 0);
    }

    #[test]
    fn family_filter_case_insensitive() {
        let rules = vec![
            rule("--p", Some("ColorFont"), None, vec![(1, "#00ff00")]),
            rule("--p", Some("OtherFont"), None, vec![(1, "#0000ff")]),
        ];
        let r = resolve_font_palette_overrides(&rules, "--p", "colorfont").unwrap();
        assert_eq!(r.overrides[0].color.g, 255);
    }

    #[test]
    fn no_family_matches_any() {
        let rules = vec![rule("--x", None, None, vec![(0, "#123456")])];
        let r = resolve_font_palette_overrides(&rules, "--x", "whatever").unwrap();
        assert_eq!(r.overrides[0].index, 0);
    }

    #[test]
    fn invalid_color_is_skipped() {
        let rules = vec![rule("--q", None, None, vec![(0, "not-a-color"), (1, "#aabbcc")])];
        let r = resolve_font_palette_overrides(&rules, "--q", "F").unwrap();
        assert_eq!(r.overrides.len(), 1);
        assert_eq!(r.overrides[0].index, 1);
    }
}
