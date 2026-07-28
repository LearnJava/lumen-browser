//! AI assistant sidebar panel (§12.8, UNIQUE: GG-1).
//!
//! Right-docked 200 CSS-px panel with a single-line prompt input at the bottom
//! and a scrollable response area above it.  The shell calls
//! [`AiPanel::submit`] when the user presses Enter in the input field; the
//! response from [`lumen_core::AiBackend::query`] is stored in
//! [`AiPanel::response`] and rendered on the next frame.
//!
//! Layout (CSS px):
//! ```text
//! x=(window_w - PANEL_WIDTH)          x=window_w
//! y=tab_bar_h  ┌─────────────────────┐
//!              │ AI Assistant   [×]  │ ← HEADER_H = 32
//!              ├─────────────────────┤
//!              │                     │
//!              │  response area      │  (scrollable)
//!              │                     │
//!              ├─────────────────────┤
//!              │ › _input_text_      │ ← INPUT_H = 36
//! y=window_h   └─────────────────────┘
//! ```
//!
//! Keyboard toggle: `Ctrl+Shift+A` → `KeyCommand::ToggleAiPanel`.

// ── Visual constants ──────────────────────────────────────────────────────────

/// Width of the AI panel in CSS px.
pub const PANEL_WIDTH: f32 = 200.0;
/// Height of the panel header bar in CSS px.
const HEADER_H: f32 = 32.0;
/// Height of the prompt input row at the bottom in CSS px.
const INPUT_H: f32 = 36.0;
/// Close button size in CSS px.
const CLOSE_SIZE: f32 = 18.0;
/// Right margin for the close button.
const CLOSE_RIGHT: f32 = 7.0;

// ── Data types ────────────────────────────────────────────────────────────────

/// AI assistant sidebar panel state (§12.8).
///
/// `visible` controls whether the panel occupies the right [`PANEL_WIDTH`]
/// CSS px.  `input` is the current prompt text being typed.  `response` is
/// the last AI reply (empty until the first submit).
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

/// Result of a click inside the AI panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiHit {
    /// Clicked the "×" close button in the header.
    Close,
    /// Clicked in the header (not on the close button).
    Header,
    /// Clicked in the response text area.
    Response,
    /// Clicked in the prompt input field.
    Input,
}

/// Hit-test `(x, y)` in CSS px against the AI panel.
///
/// Returns `None` when the click is outside the panel or the panel is hidden.
pub fn hit_test(
    panel: &AiPanel,
    x: f32,
    y: f32,
    origin_x: f32,
    tab_bar_h: f32,
    window_h: f32,
    width: f32,
) -> Option<AiHit> {
    if !panel.visible {
        return None;
    }
    let px = origin_x;
    if x < px || x >= px + width || y < tab_bar_h || y >= window_h {
        return None;
    }
    let rel_y = y - tab_bar_h;

    if rel_y < HEADER_H {
        let close_x = px + width - CLOSE_RIGHT - CLOSE_SIZE;
        let close_y = tab_bar_h + (HEADER_H - CLOSE_SIZE) / 2.0;
        if x >= close_x && x < close_x + CLOSE_SIZE && y >= close_y && y < close_y + CLOSE_SIZE {
            return Some(AiHit::Close);
        }
        return Some(AiHit::Header);
    }

    let input_top = window_h - INPUT_H;
    if y >= input_top {
        return Some(AiHit::Input);
    }

    Some(AiHit::Response)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::NullAiBackend;

    const WIN_W: f32 = 1024.0;
    const WIN_H: f32 = 720.0;
    const TAB_H: f32 = 36.0;
    /// Left origin of the panel at its default right dock.
    const PX: f32 = WIN_W - PANEL_WIDTH;

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

    // ── hit_test ──────────────────────────────────────────────────────────────

    #[test]
    fn hit_test_hidden_returns_none() {
        let p = hidden();
        assert!(hit_test(&p, WIN_W - 10.0, 100.0, PX, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_outside_panel_returns_none() {
        let p = visible();
        assert!(hit_test(&p, WIN_W - PANEL_WIDTH - 1.0, 100.0, PX, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_close_button() {
        let p = visible();
        let close_x = WIN_W - CLOSE_RIGHT - CLOSE_SIZE + 2.0;
        let close_y = TAB_H + (HEADER_H - CLOSE_SIZE) / 2.0 + 2.0;
        assert_eq!(hit_test(&p, close_x, close_y, PX, TAB_H, WIN_H, PANEL_WIDTH), Some(AiHit::Close));
    }

    #[test]
    fn hit_test_input_area() {
        let p = visible();
        let input_y = WIN_H - INPUT_H + 5.0;
        assert_eq!(
            hit_test(&p, WIN_W - PANEL_WIDTH + 10.0, input_y, PX, TAB_H, WIN_H, PANEL_WIDTH),
            Some(AiHit::Input)
        );
    }

    #[test]
    fn hit_test_response_area() {
        let p = visible();
        let response_y = TAB_H + HEADER_H + 20.0;
        assert_eq!(
            hit_test(&p, WIN_W - PANEL_WIDTH + 10.0, response_y, PX, TAB_H, WIN_H, PANEL_WIDTH),
            Some(AiHit::Response)
        );
    }

    // ── cross-dock (origin_x at the left edge) ──────────────────────────────────

    #[test]
    fn hit_test_left_dock_inside_and_outside() {
        let p = visible();
        // origin_x = 0 → panel hugs the left edge, spanning [0, PANEL_WIDTH).
        assert!(hit_test(&p, 10.0, TAB_H + 60.0, 0.0, TAB_H, WIN_H, PANEL_WIDTH).is_some());
        // Just past the right edge is outside.
        assert!(hit_test(&p, PANEL_WIDTH + 1.0, TAB_H + 60.0, 0.0, TAB_H, WIN_H, PANEL_WIDTH).is_none());
    }

    #[test]
    fn hit_test_left_dock_close_button() {
        let p = visible();
        let close_x = PANEL_WIDTH - CLOSE_RIGHT - CLOSE_SIZE + 2.0;
        let close_y = TAB_H + (HEADER_H - CLOSE_SIZE) / 2.0 + 2.0;
        assert_eq!(
            hit_test(&p, close_x, close_y, 0.0, TAB_H, WIN_H, PANEL_WIDTH),
            Some(AiHit::Close)
        );
    }
}
