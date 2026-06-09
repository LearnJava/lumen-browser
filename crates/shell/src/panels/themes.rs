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
}
