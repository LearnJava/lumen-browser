//! Bundled chrome UI fonts (DS-4).
//!
//! Golos Text (default browser-chrome UI font) and JetBrains Mono (omnibox
//! URL field + DevTools console/inspector/network panels), both SIL OFL 1.1.
//! Shared by every [`crate::backends`] render backend that draws chrome text
//! (`FemtovgBackend` and the wgpu [`crate::renderer::Renderer`]) so the two
//! text-rendering paths agree on which bytes back each reserved family name.
//!
//! Reserved family names recognized by each backend's face/font resolver:
//! `"Golos Text"`, `"Golos Text Medium"`, `"JetBrains Mono"`. Chrome
//! `DrawText` commands that pass an empty `font_family` (every chrome call
//! site today) default to [`GOLOS_TEXT_REGULAR`] — page content never has an
//! empty `font_family` (always populated from the CSS cascade), so this
//! default cannot affect page rendering.

/// Golos Text Regular — default chrome UI font.
pub const GOLOS_TEXT_REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/GolosText-Regular.ttf");

/// Golos Text Medium — reserved family `"Golos Text Medium"`.
pub const GOLOS_TEXT_MEDIUM: &[u8] = include_bytes!("../../../../assets/fonts/GolosText-Medium.ttf");

/// JetBrains Mono Regular — reserved family `"JetBrains Mono"`, used for the
/// omnibox URL field and DevTools console/inspector/network panels.
pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");
