//! Shell theme tokens: base (light/dark/system) + accent colour preset.
//!
//! `ShellTheme` drives the active-tab accent bar in the tab strip and is
//! exposed through the Appearance section of the settings panel (§O-9).
//! The accent does not affect page CSS (`prefers-color-scheme` is still read
//! from the OS unless the user explicitly locks to light or dark).

use lumen_layout::Color;

/// Preset accent colours available in the Appearance settings section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccentPreset {
    /// Default blue — `#6480FF`.
    #[default]
    Blue,
    /// Violet — `#9B5DE5`.
    Purple,
    /// Teal — `#2EC4B6`.
    Teal,
    /// Green — `#52B788`.
    Green,
    /// Amber — `#F4A261`.
    Orange,
    /// Rose — `#E63B6F`.
    Rose,
}

impl AccentPreset {
    /// All six presets in display order.
    pub const ALL: [Self; 6] = [
        Self::Blue,
        Self::Purple,
        Self::Teal,
        Self::Green,
        Self::Orange,
        Self::Rose,
    ];

    /// RGB colour for this preset.
    pub fn color(self) -> Color {
        match self {
            Self::Blue   => Color { r: 100, g: 128, b: 255, a: 255 },
            Self::Purple => Color { r: 155, g:  93, b: 229, a: 255 },
            Self::Teal   => Color { r:  46, g: 196, b: 182, a: 255 },
            Self::Green  => Color { r:  82, g: 183, b: 136, a: 255 },
            Self::Orange => Color { r: 244, g: 162, b:  97, a: 255 },
            Self::Rose   => Color { r: 230, g:  59, b: 111, a: 255 },
        }
    }

    /// Short lowercase key, used in settings serialisation.
    pub fn key(self) -> &'static str {
        match self {
            Self::Blue   => "blue",
            Self::Purple => "purple",
            Self::Teal   => "teal",
            Self::Green  => "green",
            Self::Orange => "orange",
            Self::Rose   => "rose",
        }
    }

    /// Parse from the short key.  Unknown key falls back to `Blue`.
    pub fn from_key(s: &str) -> Self {
        match s {
            "purple" => Self::Purple,
            "teal"   => Self::Teal,
            "green"  => Self::Green,
            "orange" => Self::Orange,
            "rose"   => Self::Rose,
            _        => Self::Blue,
        }
    }
}

/// Base brightness mode for the shell chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeBase {
    /// Explicit dark chrome (overrides OS preference).
    Dark,
    /// Explicit light chrome (overrides OS preference).
    Light,
    /// Follow the OS `prefers-color-scheme`.
    #[default]
    System,
}

/// Shell appearance configuration: base brightness + accent colour.
///
/// Serialised as `"<base>"` or `"<base>+<accent>"`, e.g. `"dark+rose"`.
/// `"system"` and `"system+blue"` are equivalent (blue is the default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShellTheme {
    /// Brightness mode.
    pub base: ThemeBase,
    /// Accent preset.
    pub accent: AccentPreset,
}

impl ShellTheme {
    /// Accent colour for the active tab indicator and other chrome highlights.
    pub fn accent_color(self) -> Color {
        self.accent.color()
    }

    /// Whether the chrome should use the dark palette.
    ///
    /// `os_dark` is the current OS preference; it is used only when `base` is
    /// `ThemeBase::System`.
    pub fn is_dark(self, os_dark: bool) -> bool {
        match self.base {
            ThemeBase::Dark   => true,
            ThemeBase::Light  => false,
            ThemeBase::System => os_dark,
        }
    }

    /// Parse from the compact settings string (e.g. `"dark"`, `"light+rose"`).
    pub fn parse(s: &str) -> Self {
        let (base_str, accent_str) = match s.split_once('+') {
            Some((b, a)) => (b, a),
            None         => (s, "blue"),
        };
        let base = match base_str {
            "dark"  => ThemeBase::Dark,
            "light" => ThemeBase::Light,
            _       => ThemeBase::System,
        };
        Self { base, accent: AccentPreset::from_key(accent_str) }
    }

    /// Serialise to the compact settings string.
    pub fn to_settings_str(self) -> String {
        let base = match self.base {
            ThemeBase::Dark   => "dark",
            ThemeBase::Light  => "light",
            ThemeBase::System => "system",
        };
        if self.accent == AccentPreset::Blue {
            base.to_owned()
        } else {
            format!("{}+{}", base, self.accent.key())
        }
    }

    /// Resolve the concrete chrome [`Palette`] for this theme.
    ///
    /// `os_dark` is the current OS `prefers-color-scheme` preference; it is
    /// honoured only when `base` is `ThemeBase::System`. The returned palette's
    /// `accent` is overridden with this theme's selected accent preset so the
    /// active-tab bar and other highlights follow the user's accent choice.
    pub fn palette(self, os_dark: bool) -> Palette {
        let base = if self.is_dark(os_dark) { Palette::DARK } else { Palette::LIGHT };
        Palette { accent: self.accent_color(), ..base }
    }
}

/// Resolved chrome colour tokens for the shell UI (tab strip, address bar,
/// floating overlays). Produced by [`ShellTheme::palette`]; a `Dark` and a
/// `Light` variant are provided as the `DARK` / `LIGHT` constants.
///
/// Tokens are role-named (not brightness-named) so the same field maps to the
/// correct surface in both themes — e.g. `tab_active_bg` is the darkest tab in
/// the dark theme but the lightest tab in the light theme. Semantic indicator
/// colours (ad-block checkbox, lifecycle badges, container strips, group bars)
/// are intentionally *not* part of the palette: they carry meaning and stay
/// constant across themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Background of the horizontal tab strip bar.
    pub tab_bar_bg: Color,
    /// Background of the active (foreground) tab.
    pub tab_active_bg: Color,
    /// Background of an inactive tab.
    pub tab_inactive_bg: Color,
    /// Background of a sleeping (BackgroundOld / T2) tab — a dimmer signal.
    pub tab_sleep_bg: Color,
    /// Background of a hibernated (T3) tab — the deepest dim.
    pub tab_hibernate_bg: Color,
    /// Background of a floating overlay panel (address bar, find bar).
    pub overlay_bg: Color,
    /// Border line around floating overlay dropdowns.
    pub overlay_border: Color,
    /// Background of a recessed text input field inside chrome.
    pub input_bg: Color,
    /// Background of an unselected dropdown / list item.
    pub item_bg: Color,
    /// Background of the selected dropdown / list item.
    pub item_selected_bg: Color,
    /// Primary chrome text colour.
    pub text: Color,
    /// Secondary / dimmed chrome text colour.
    pub text_dim: Color,
    /// Divider / subtle separator line colour.
    pub divider: Color,
    /// Active accent colour (overridden per-theme by the selected preset).
    pub accent: Color,
}

impl Palette {
    /// Dark chrome palette. Values reproduce the historical hard-coded dark
    /// constants from `tabs/strip.rs` and `address_bar.rs` so the dark theme is
    /// visually unchanged.
    pub const DARK: Palette = Palette {
        tab_bar_bg:       Color { r:  22, g:  22, b:  26, a: 255 },
        tab_active_bg:    Color { r:  18, g:  18, b:  22, a: 255 },
        tab_inactive_bg:  Color { r:  32, g:  33, b:  36, a: 255 },
        tab_sleep_bg:     Color { r:  26, g:  27, b:  30, a: 255 },
        tab_hibernate_bg: Color { r:  21, g:  21, b:  24, a: 255 },
        overlay_bg:       Color { r:  32, g:  33, b:  36, a: 240 },
        overlay_border:   Color { r:  55, g:  55, b:  70, a: 255 },
        input_bg:         Color { r:  18, g:  18, b:  22, a: 255 },
        item_bg:          Color { r:  26, g:  27, b:  30, a: 245 },
        item_selected_bg: Color { r:  40, g:  72, b: 152, a: 255 },
        text:             Color { r: 232, g: 232, b: 236, a: 255 },
        text_dim:         Color { r: 140, g: 140, b: 148, a: 255 },
        divider:          Color { r:  45, g:  46, b:  52, a: 255 },
        accent:           Color { r: 100, g: 128, b: 255, a: 255 },
    };

    /// Light chrome palette — a soft neutral-grey surface set with dark text.
    pub const LIGHT: Palette = Palette {
        tab_bar_bg:       Color { r: 222, g: 223, b: 227, a: 255 },
        tab_active_bg:    Color { r: 252, g: 252, b: 253, a: 255 },
        tab_inactive_bg:  Color { r: 233, g: 234, b: 238, a: 255 },
        tab_sleep_bg:     Color { r: 224, g: 225, b: 229, a: 255 },
        tab_hibernate_bg: Color { r: 214, g: 215, b: 220, a: 255 },
        overlay_bg:       Color { r: 248, g: 249, b: 251, a: 245 },
        overlay_border:   Color { r: 198, g: 200, b: 208, a: 255 },
        input_bg:         Color { r: 255, g: 255, b: 255, a: 255 },
        item_bg:          Color { r: 244, g: 245, b: 248, a: 248 },
        item_selected_bg: Color { r: 205, g: 222, b: 255, a: 255 },
        text:             Color { r:  28, g:  29, b:  34, a: 255 },
        text_dim:         Color { r: 108, g: 110, b: 120, a: 255 },
        divider:          Color { r: 205, g: 207, b: 214, a: 255 },
        accent:           Color { r: 100, g: 128, b: 255, a: 255 },
    };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_system_blue() {
        let t = ShellTheme::default();
        assert_eq!(t.base, ThemeBase::System);
        assert_eq!(t.accent, AccentPreset::Blue);
    }

    #[test]
    fn parse_dark() {
        let t = ShellTheme::parse("dark");
        assert_eq!(t.base, ThemeBase::Dark);
        assert_eq!(t.accent, AccentPreset::Blue);
    }

    #[test]
    fn parse_light_plus_rose() {
        let t = ShellTheme::parse("light+rose");
        assert_eq!(t.base, ThemeBase::Light);
        assert_eq!(t.accent, AccentPreset::Rose);
    }

    #[test]
    fn parse_system_plus_teal() {
        let t = ShellTheme::parse("system+teal");
        assert_eq!(t.base, ThemeBase::System);
        assert_eq!(t.accent, AccentPreset::Teal);
    }

    #[test]
    fn parse_unknown_is_system_blue() {
        let t = ShellTheme::parse("invalid");
        assert_eq!(t.base, ThemeBase::System);
        assert_eq!(t.accent, AccentPreset::Blue);
    }

    #[test]
    fn to_settings_str_dark_blue_omits_accent() {
        let t = ShellTheme { base: ThemeBase::Dark, accent: AccentPreset::Blue };
        assert_eq!(t.to_settings_str(), "dark");
    }

    #[test]
    fn to_settings_str_light_plus_green() {
        let t = ShellTheme { base: ThemeBase::Light, accent: AccentPreset::Green };
        assert_eq!(t.to_settings_str(), "light+green");
    }

    #[test]
    fn roundtrip_parse_to_str() {
        for &acc in &AccentPreset::ALL {
            for base in [ThemeBase::Dark, ThemeBase::Light, ThemeBase::System] {
                let theme = ShellTheme { base, accent: acc };
                let s = theme.to_settings_str();
                let parsed = ShellTheme::parse(&s);
                assert_eq!(parsed, theme, "roundtrip failed for {s}");
            }
        }
    }

    #[test]
    fn is_dark_system_respects_os() {
        let t = ShellTheme { base: ThemeBase::System, accent: AccentPreset::Blue };
        assert!(t.is_dark(true));
        assert!(!t.is_dark(false));
    }

    #[test]
    fn is_dark_explicit_ignores_os() {
        let dark = ShellTheme { base: ThemeBase::Dark, accent: AccentPreset::Blue };
        assert!(dark.is_dark(false));
        let light = ShellTheme { base: ThemeBase::Light, accent: AccentPreset::Blue };
        assert!(!light.is_dark(true));
    }

    #[test]
    fn accent_presets_all_opaque() {
        for preset in AccentPreset::ALL {
            assert_eq!(preset.color().a, 255, "{:?} must be opaque", preset);
        }
    }

    #[test]
    fn accent_preset_roundtrip_key() {
        for preset in AccentPreset::ALL {
            assert_eq!(AccentPreset::from_key(preset.key()), preset);
        }
    }

    #[test]
    fn palette_dark_theme_uses_dark_base() {
        let t = ShellTheme { base: ThemeBase::Dark, accent: AccentPreset::Blue };
        let p = t.palette(false); // os_dark ignored for explicit Dark
        assert_eq!(p.tab_bar_bg, Palette::DARK.tab_bar_bg);
        assert_eq!(p.text, Palette::DARK.text);
    }

    #[test]
    fn palette_light_theme_uses_light_base() {
        let t = ShellTheme { base: ThemeBase::Light, accent: AccentPreset::Blue };
        let p = t.palette(true); // os_dark ignored for explicit Light
        assert_eq!(p.tab_bar_bg, Palette::LIGHT.tab_bar_bg);
        assert_eq!(p.text, Palette::LIGHT.text);
    }

    #[test]
    fn palette_system_follows_os() {
        let t = ShellTheme { base: ThemeBase::System, accent: AccentPreset::Blue };
        assert_eq!(t.palette(true).tab_bar_bg, Palette::DARK.tab_bar_bg);
        assert_eq!(t.palette(false).tab_bar_bg, Palette::LIGHT.tab_bar_bg);
    }

    #[test]
    fn palette_accent_follows_preset() {
        let t = ShellTheme { base: ThemeBase::Dark, accent: AccentPreset::Rose };
        assert_eq!(t.palette(true).accent, AccentPreset::Rose.color());
    }

    #[test]
    fn palette_light_is_brighter_than_dark() {
        // The light bar must actually be light, the dark bar dark — guards
        // against accidentally swapping the two constants. Resolved through the
        // runtime `palette()` path so the comparison isn't a const assertion.
        let dark = ShellTheme { base: ThemeBase::Dark, accent: AccentPreset::Blue }.palette(false);
        let light = ShellTheme { base: ThemeBase::Light, accent: AccentPreset::Blue }.palette(true);
        assert!(light.tab_bar_bg.r > dark.tab_bar_bg.r);
        assert!(light.text.r < dark.text.r);
    }
}
