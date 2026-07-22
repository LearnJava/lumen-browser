//! Shell theme tokens: base (light/dark/system) + accent colour preset.
//!
//! `ShellTheme` drives the active-tab accent bar in the tab strip and is
//! exposed through the Appearance section of the settings panel (§O-9).
//! The accent does not affect page CSS (`prefers-color-scheme` is still read
//! from the OS unless the user explicitly locks to light or dark).

use lumen_layout::Color;

use crate::theme_tokens;

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
/// correct surface in both themes — e.g. `tab_active_bg` is `theme_tokens::SURFACE_2`
/// in both themes even though that is the *lightest* dark-theme surface but a
/// *dimmer* light-theme surface (dark-theme elevation gets lighter, light-theme
/// elevation gets greyer — see `docs/design/lumen-v3_3.html` `:root`). Semantic
/// indicator colours (ad-block checkbox, lifecycle badges, container strips,
/// group bars) are intentionally *not* part of the palette: they carry meaning
/// and stay constant across themes.
///
/// Every field below is sourced from [`theme_tokens`] (DS-2,
/// `docs/tasks/p1-design-v3.md`). The design-system prototype only names surface
/// roles for tab strip, omnibox and floating panels/dropdowns/modals — roles with
/// no direct prototype element (`header_bg`, `row_alt_bg`, `item_bg`,
/// `item_selected_bg`, `tab_sleep_bg`, `tab_hibernate_bg`) are pinned to the
/// *nearest* surface tier and documented per-field below; the 3-tier token set
/// is deliberately coarser than the 16-field bespoke palette it replaces, so
/// several roles now legitimately share one physical shade.
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
    /// Background of a panel's title / header bar — one step distinct from
    /// [`overlay_bg`](Self::overlay_bg) so the header reads as a separate band.
    pub header_bg: Color,
    /// Background of alternate (zebra-striped) list rows inside panels.
    pub row_alt_bg: Color,
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
    /// Dark chrome palette, built from [`theme_tokens::dark`].
    pub const DARK: Palette = Palette {
        // `.hbar-row1` background in the prototype.
        tab_bar_bg: theme_tokens::dark::SURFACE_1,
        // `.hbar-tab.active` / `.tab-row.active` background in the prototype.
        tab_active_bg: theme_tokens::dark::SURFACE_2,
        // `.hbar-tab` has no explicit background in the prototype — it shows
        // through to the strip's own `SURFACE_1`, so an inactive tab is the
        // same tier as the bar it sits on.
        tab_inactive_bg: theme_tokens::dark::SURFACE_1,
        // No prototype equivalent (sleeping/hibernated tabs are Lumen-only
        // states, signalled there only via favicon opacity). Nearest darker
        // tier so the dim reads as "receded"; sleep vs. hibernate distinction
        // is carried entirely by the "z"/"Z" badge glyph, not the background.
        tab_sleep_bg: theme_tokens::dark::SURFACE_0,
        tab_hibernate_bg: theme_tokens::dark::SURFACE_0,
        // `.dropdown` / `.popover` / `.modal` background in the prototype —
        // NOT `--overlay-bg` (that token is the translucent scrim *behind* a
        // modal, `.modal-overlay`; Lumen's `overlay_bg` field predates DS-1 and
        // actually means "opaque floating-panel surface", a false-friend name
        // clash with the prototype's `--overlay-bg` custom property).
        overlay_bg: theme_tokens::dark::SURFACE_0,
        header_bg: theme_tokens::dark::SURFACE_1,
        row_alt_bg: theme_tokens::dark::SURFACE_1,
        // Every border in the prototype is `--stroke`.
        overlay_border: theme_tokens::dark::STROKE,
        // `.omnibox` background in the prototype.
        input_bg: theme_tokens::dark::SURFACE_1,
        item_bg: theme_tokens::dark::SURFACE_1,
        // `.dd-row:hover` background in the prototype (no distinct "selected"
        // state exists there, only hover — reused for the selected item too).
        item_selected_bg: theme_tokens::dark::SURFACE_2,
        text: theme_tokens::dark::TEXT_PRIMARY,
        text_dim: theme_tokens::dark::TEXT_SECONDARY,
        divider: theme_tokens::dark::STROKE,
        accent: theme_tokens::profile::PERSONAL,
    };

    /// Light chrome palette, built from [`theme_tokens::light`].
    pub const LIGHT: Palette = Palette {
        tab_bar_bg: theme_tokens::light::SURFACE_1,
        tab_active_bg: theme_tokens::light::SURFACE_2,
        tab_inactive_bg: theme_tokens::light::SURFACE_1,
        tab_sleep_bg: theme_tokens::light::SURFACE_0,
        tab_hibernate_bg: theme_tokens::light::SURFACE_0,
        overlay_bg: theme_tokens::light::SURFACE_0,
        header_bg: theme_tokens::light::SURFACE_1,
        row_alt_bg: theme_tokens::light::SURFACE_1,
        overlay_border: theme_tokens::light::STROKE,
        input_bg: theme_tokens::light::SURFACE_1,
        item_bg: theme_tokens::light::SURFACE_1,
        item_selected_bg: theme_tokens::light::SURFACE_2,
        text: theme_tokens::light::TEXT_PRIMARY,
        text_dim: theme_tokens::light::TEXT_SECONDARY,
        divider: theme_tokens::light::STROKE,
        accent: theme_tokens::profile::PERSONAL,
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

    #[test]
    fn panel_role_tokens_track_brightness() {
        // header_bg / row_alt_bg must follow the theme: light in the light
        // palette, dark in the dark palette (guards against swapped constants).
        // Resolved through the runtime `palette()` path so the comparison isn't
        // a const assertion.
        let dark = ShellTheme { base: ThemeBase::Dark, accent: AccentPreset::Blue }.palette(false);
        let light = ShellTheme { base: ThemeBase::Light, accent: AccentPreset::Blue }.palette(true);
        assert!(light.header_bg.r > dark.header_bg.r);
        assert!(light.row_alt_bg.r > dark.row_alt_bg.r);
    }
}
