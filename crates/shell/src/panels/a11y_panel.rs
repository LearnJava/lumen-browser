//! Accessibility settings panel state (E-2).
//!
//! Opened by `Ctrl+Shift+Q`; rendered by the engine chrome as
//! `#accessOverlay` (`assets/chrome/chrome.html`, UX-CHROME-PANELS). It exposes
//! four preferences from [`lumen_storage::A11yPrefs`]: font size multiplier,
//! prefers-reduced-motion, forced colors and cursor size.
//!
//! The panel holds a [`lumen_storage::A11yPrefsSnapshot`] as a working draft.
//! On close the caller persists it via [`lumen_storage::A11yPrefs::apply_snapshot`]
//! and re-delivers media changes to the JS engine (`Lumen::close_a11y_panel`).

use lumen_storage::A11yPrefsSnapshot;

// ── Panel state ───────────────────────────────────────────────────────────────

/// Accessibility settings panel state.
///
/// `visible` gates rendering and hit-testing. `draft` holds in-progress edits
/// that are persisted only when the user closes the panel via the `×` button
/// or the `Ctrl+Shift+Q` toggle.
pub struct A11yPanel {
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Working copy of the accessibility preferences; persisted on close.
    pub draft: A11yPrefsSnapshot,
}

impl A11yPanel {
    /// Create a new hidden panel with default preferences.
    pub fn new() -> Self {
        Self {
            visible: false,
            draft: A11yPrefsSnapshot::default(),
        }
    }

    /// Toggle panel visibility.
    ///
    /// Note: prefer using the main.rs key handler directly since opening the
    /// panel should also call `load_draft` first.
    #[allow(dead_code)]
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Load current preferences into the draft so edits start from persisted values.
    pub fn load_draft(&mut self, snap: A11yPrefsSnapshot) {
        self.draft = snap;
    }
}

impl Default for A11yPanel {
    fn default() -> Self {
        Self::new()
    }
}
