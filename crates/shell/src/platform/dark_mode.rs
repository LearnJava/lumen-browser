//! OS-level dark-mode detection for `@media (prefers-color-scheme)`.
//!
//! winit already abstracts the per-platform colour-scheme query (Win32
//! `ShouldAppsUseDarkMode` / immersive registry value, macOS `NSAppearance`,
//! Linux `org.freedesktop.appearance` portal or XSettings). We read it via
//! [`winit::window::Window::theme`] at window creation and refresh it on
//! [`winit::event::WindowEvent::ThemeChanged`], so no extra unsafe FFI is
//! needed here — this module only maps winit's [`Theme`] into the boolean the
//! engine's `MediaContext::prefers_dark` expects.

use winit::window::Theme;

/// Maps an OS colour-scheme [`Theme`] to the `prefers-color-scheme: dark`
/// boolean used by `MediaContext`.
///
/// `None` means winit could not determine the OS preference (common on some
/// Linux backends). Per CSS Media Queries L5 §5.2 the `light` keyword is the
/// fallback when no preference is exposed, so an unknown theme maps to `false`.
#[must_use]
pub fn theme_prefers_dark(theme: Option<Theme>) -> bool {
    matches!(theme, Some(Theme::Dark))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_is_dark() {
        assert!(theme_prefers_dark(Some(Theme::Dark)));
    }

    #[test]
    fn light_theme_is_not_dark() {
        assert!(!theme_prefers_dark(Some(Theme::Light)));
    }

    #[test]
    fn unknown_theme_defaults_to_light() {
        assert!(!theme_prefers_dark(None));
    }
}
