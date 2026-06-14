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

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

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

const BG: Color = Color { r: 20, g: 22, b: 30, a: 255 };
const HEADER_BG: Color = Color { r: 30, g: 33, b: 44, a: 255 };
const INPUT_BG: Color = Color { r: 28, g: 31, b: 41, a: 255 };
const INPUT_BORDER: Color = Color { r: 70, g: 90, b: 130, a: 255 };
const BORDER: Color = Color { r: 48, g: 52, b: 68, a: 255 };
const TEXT_MAIN: Color = Color { r: 210, g: 212, b: 224, a: 255 };
const TEXT_DIM: Color = Color { r: 110, g: 116, b: 136, a: 255 };
const TEXT_RESPONSE: Color = Color { r: 190, g: 195, b: 210, a: 255 };
const CLOSE_BG: Color = Color { r: 55, g: 58, b: 76, a: 200 };
const CLOSE_FG: Color = Color { r: 150, g: 154, b: 168, a: 255 };
const CURSOR_COLOR: Color = Color { r: 80, g: 140, b: 220, a: 255 };

const FONT_SZ: f32 = 11.0;
const INPUT_FONT_SZ: f32 = 11.5;

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
    window_w: f32,
    tab_bar_h: f32,
    window_h: f32,
) -> Option<AiHit> {
    if !panel.visible {
        return None;
    }
    let px = window_w - PANEL_WIDTH;
    if x < px || x >= window_w || y < tab_bar_h || y >= window_h {
        return None;
    }
    let rel_y = y - tab_bar_h;

    if rel_y < HEADER_H {
        let close_x = px + PANEL_WIDTH - CLOSE_RIGHT - CLOSE_SIZE;
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

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the AI sidebar panel.
///
/// Renders from `x = (window_w − PANEL_WIDTH)` to `x = window_w` and from
/// `y = tab_bar_h` to `y = window_h`.
pub fn build_panel(panel: &AiPanel, window_w: f32, tab_bar_h: f32, window_h: f32) -> DisplayList {
    if !panel.visible {
        return DisplayList::new();
    }

    let px = window_w - PANEL_WIDTH;
    let panel_h = window_h - tab_bar_h;
    let response_y = tab_bar_h + HEADER_H;
    let response_h = (panel_h - HEADER_H - INPUT_H).max(0.0);
    let input_y = window_h - INPUT_H;

    let mut out = DisplayList::with_capacity(32);

    // Panel background.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, PANEL_WIDTH, panel_h),
        color: BG,
    });
    // Left border divider.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, tab_bar_h, 1.0, panel_h),
        color: BORDER,
    });

    // ── Header ────────────────────────────────────────────────────────────────
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, tab_bar_h, PANEL_WIDTH - 1.0, HEADER_H),
        color: HEADER_BG,
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, tab_bar_h + HEADER_H - 1.0, PANEL_WIDTH - 1.0, 1.0),
        color: BORDER,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 10.0, tab_bar_h + 9.0, PANEL_WIDTH - CLOSE_SIZE - CLOSE_RIGHT * 2.0 - 14.0, FONT_SZ * 1.4),
        text: "AI Assistant".to_owned(),
        font_size: FONT_SZ,
        color: TEXT_MAIN,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });
    let close_x = px + PANEL_WIDTH - CLOSE_RIGHT - CLOSE_SIZE;
    let close_y = tab_bar_h + (HEADER_H - CLOSE_SIZE) / 2.0;
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(close_x, close_y, CLOSE_SIZE, CLOSE_SIZE),
        radii: CornerRadii { tl: 3.0, tl_y: 3.0, tr: 3.0, tr_y: 3.0, br: 3.0, br_y: 3.0, bl: 3.0, bl_y: 3.0 },
        color: CLOSE_BG,
    });
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(close_x + 3.0, close_y + 1.0, CLOSE_SIZE - 6.0, CLOSE_SIZE - 2.0),
        text: "×".to_owned(),
        font_size: 13.0,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    // ── Response area ─────────────────────────────────────────────────────────
    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(px + 1.0, response_y, PANEL_WIDTH - 1.0, response_h),
    });
    if panel.response.is_empty() {
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(px + 10.0, response_y + 16.0, PANEL_WIDTH - 20.0, FONT_SZ * 1.4),
            text: "Ask anything…".to_owned(),
            font_size: FONT_SZ,
            color: TEXT_DIM,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Italic,
            font_variation_axes: Vec::new(),
            tab_size: 0.0,
            highlight_name: None,
        });
    } else {
        let line_h = FONT_SZ * 1.5;
        let mut line_y = response_y + 10.0 - panel.scroll_y;
        for line in panel.response.lines() {
            if line_y + line_h >= response_y && line_y < response_y + response_h {
                out.push(DisplayCommand::DrawText {
                    rect: Rect::new(px + 10.0, line_y, PANEL_WIDTH - 20.0, line_h),
                    text: line.to_owned(),
                    font_size: FONT_SZ,
                    color: TEXT_RESPONSE,
                    font_family: Vec::new(),
                    font_weight: FontWeight::NORMAL,
                    font_style: FontStyle::Normal,
                    font_variation_axes: Vec::new(),
                    tab_size: 0.0,
                    highlight_name: None,
                });
            }
            line_y += line_h;
        }
    }
    out.push(DisplayCommand::PopClip);

    // Divider above input.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, input_y - 1.0, PANEL_WIDTH - 1.0, 1.0),
        color: BORDER,
    });

    // ── Input row ─────────────────────────────────────────────────────────────
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 1.0, input_y, PANEL_WIDTH - 1.0, INPUT_H),
        color: INPUT_BG,
    });
    // Input border highlight (top edge).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + 8.0, input_y + 6.0, PANEL_WIDTH - 16.0, 1.0),
        color: INPUT_BORDER,
    });
    // Prompt caret prefix "›".
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 8.0, input_y + 10.0, 12.0, INPUT_FONT_SZ * 1.4),
        text: "›".to_owned(),
        font_size: INPUT_FONT_SZ,
        color: CURSOR_COLOR,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });
    let input_display = if panel.input.is_empty() {
        "type a prompt…".to_owned()
    } else {
        truncate_label(&panel.input, 22)
    };
    let input_color = if panel.input.is_empty() { TEXT_DIM } else { TEXT_MAIN };
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(px + 22.0, input_y + 10.0, PANEL_WIDTH - 30.0, INPUT_FONT_SZ * 1.4),
        text: input_display,
        font_size: INPUT_FONT_SZ,
        color: input_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: if panel.input.is_empty() { FontStyle::Italic } else { FontStyle::Normal },
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    });

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn truncate_label(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{collected}…")
    } else {
        collected
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::NullAiBackend;

    const WIN_W: f32 = 1024.0;
    const WIN_H: f32 = 720.0;
    const TAB_H: f32 = 36.0;

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
        assert!(hit_test(&p, WIN_W - 10.0, 100.0, WIN_W, TAB_H, WIN_H).is_none());
    }

    #[test]
    fn hit_test_outside_panel_returns_none() {
        let p = visible();
        assert!(hit_test(&p, WIN_W - PANEL_WIDTH - 1.0, 100.0, WIN_W, TAB_H, WIN_H).is_none());
    }

    #[test]
    fn hit_test_close_button() {
        let p = visible();
        let close_x = WIN_W - CLOSE_RIGHT - CLOSE_SIZE + 2.0;
        let close_y = TAB_H + (HEADER_H - CLOSE_SIZE) / 2.0 + 2.0;
        assert_eq!(hit_test(&p, close_x, close_y, WIN_W, TAB_H, WIN_H), Some(AiHit::Close));
    }

    #[test]
    fn hit_test_input_area() {
        let p = visible();
        let input_y = WIN_H - INPUT_H + 5.0;
        assert_eq!(
            hit_test(&p, WIN_W - PANEL_WIDTH + 10.0, input_y, WIN_W, TAB_H, WIN_H),
            Some(AiHit::Input)
        );
    }

    #[test]
    fn hit_test_response_area() {
        let p = visible();
        let response_y = TAB_H + HEADER_H + 20.0;
        assert_eq!(
            hit_test(&p, WIN_W - PANEL_WIDTH + 10.0, response_y, WIN_W, TAB_H, WIN_H),
            Some(AiHit::Response)
        );
    }

    // ── build_panel ───────────────────────────────────────────────────────────

    #[test]
    fn build_panel_hidden_is_empty() {
        let p = hidden();
        assert!(build_panel(&p, WIN_W, TAB_H, WIN_H).is_empty());
    }

    #[test]
    fn build_panel_visible_has_header_text() {
        let p = visible();
        let dl = build_panel(&p, WIN_W, TAB_H, WIN_H);
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "AI Assistant")
        });
        assert!(has_title);
    }
}
