//! AI assistant sidebar panel (§12.8, UNIQUE: GG-1).
//!
//! State only since CC-15-6: the panel is drawn by the engine chrome
//! (`#rightSidebar` in `assets/chrome/chrome.html`), which owns its geometry,
//! header/close button and prompt row. The shell calls [`AiPanel::submit`]
//! when the user presses Enter in the input field; the response from
//! [`lumen_core::AiBackend::query`] is stored in [`AiPanel::response`] and
//! bound into the chrome document on the next `bind_model` pass.
//!
//! Keyboard toggle: `Ctrl+Shift+A` → `KeyCommand::ToggleAiPanel`.

// ── Data types ────────────────────────────────────────────────────────────────

/// AI assistant sidebar panel state (§12.8).
///
/// `visible` controls whether the engine chrome shows `#rightSidebar` in AI
/// mode.  `input` is the current prompt text being typed.  `response` is the
/// last AI reply (empty until the first submit).
pub struct AiPanel {
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Current text in the prompt input field.
    pub input: String,
    /// Last response from the AI backend (empty before first submit).
    pub response: String,
    /// Vertical scroll offset in the response area (CSS px, 0 = top).
    pub scroll_y: f32,
}

impl AiPanel {
    /// Create a new hidden AI panel with empty input and response.
    pub fn new() -> Self {
        Self {
            visible: false,
            input: String::new(),
            response: String::new(),
            scroll_y: 0.0,
        }
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Close the panel (hide; input and response are preserved).
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Append a character to the input field.
    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
    }

    /// Remove the last character from the input field (backspace).
    pub fn backspace(&mut self) {
        self.input.pop();
    }
}

impl Default for AiPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::NullAiBackend;


    fn hidden() -> AiPanel {
        AiPanel::new()
    }

    fn visible() -> AiPanel {
        let mut p = AiPanel::new();
        p.toggle();
        p
    }

    // ── toggle / open / close ─────────────────────────────────────────────────

    #[test]
    fn toggle_shows_and_hides() {
        let mut p = hidden();
        assert!(!p.visible);
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn close_preserves_input_and_response() {
        let mut p = visible();
        p.input = "hello".into();
        p.response = "world".into();
        p.close();
        assert!(!p.visible);
        assert_eq!(p.input, "hello");
        assert_eq!(p.response, "world");
    }

    // ── input editing ─────────────────────────────────────────────────────────

    #[test]
    fn push_char_and_backspace() {
        let mut p = AiPanel::new();
        p.push_char('h');
        p.push_char('i');
        assert_eq!(p.input, "hi");
        p.backspace();
        assert_eq!(p.input, "h");
        p.backspace();
        assert!(p.input.is_empty());
        p.backspace(); // should not panic on empty
        assert!(p.input.is_empty());
    }

    // ── inline submit logic ───────────────────────────────────────────────────

    fn do_submit(panel: &mut AiPanel, backend: &dyn lumen_core::AiBackend) -> String {
        let prompt = panel.input.clone();
        if !prompt.trim().is_empty() {
            panel.response = backend.query(&prompt);
            panel.input.clear();
            panel.scroll_y = 0.0;
        }
        prompt
    }

    #[test]
    fn submit_calls_backend_and_clears_input() {
        let mut p = AiPanel::new();
        p.input = "test prompt".into();
        let submitted = do_submit(&mut p, &NullAiBackend);
        assert_eq!(submitted, "test prompt");
        assert!(p.input.is_empty(), "input should be cleared after submit");
        assert!(!p.response.is_empty(), "response should be filled");
    }

    #[test]
    fn submit_empty_input_is_noop() {
        let mut p = AiPanel::new();
        let submitted = do_submit(&mut p, &NullAiBackend);
        assert!(submitted.is_empty());
        assert!(p.response.is_empty(), "no response for empty prompt");
    }
}
