//! DevTools JS console panel (§7E.5).
//!
//! Captures `console.log/warn/error` output from JS and renders a scrollable
//! list of messages as a viewport-locked overlay.  Toggle with `F12`.
//!
//! # Architecture
//!
//! Messages are buffered in `QuickJsRuntime::console_messages` (Arc<Mutex<Vec<(u8, String)>>>)
//! and drained each `about_to_wait` into `ConsolePanel::push`.  The shell calls
//! `build_console_panel` in the overlay compositing step, after the download bar.
//!
//! # Layout
//!
//! The panel is anchored to the bottom of the window, full width, up to
//! `MAX_VISIBLE_LINES` rows of `LINE_H` height each, plus a header bar.
//! Messages are displayed newest-last (scroll_offset = 0 shows the tail).

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: Color = Color { r: 24, g: 24, b: 28, a: 240 };
const HEADER_BG: Color = Color { r: 32, g: 33, b: 38, a: 255 };
const FG_LOG: Color = Color { r: 210, g: 212, b: 218, a: 255 };
const FG_WARN: Color = Color { r: 250, g: 200, b: 60, a: 255 };
const FG_ERROR: Color = Color { r: 237, g: 80, b: 80, a: 255 };
const FG_DIM: Color = Color { r: 130, g: 132, b: 140, a: 255 };
const WARN_BG: Color = Color { r: 45, g: 40, b: 20, a: 255 };
const ERROR_BG: Color = Color { r: 45, g: 20, b: 20, a: 255 };
const CLEAR_BG: Color = Color { r: 42, g: 44, b: 50, a: 255 };

// ── Layout constants ──────────────────────────────────────────────────────────

const HEADER_H: f32 = 32.0;
const LINE_H: f32 = 20.0;
const FONT_SIZE: f32 = 12.0;
const H_PAD: f32 = 10.0;
/// Maximum number of log lines visible without scrolling.
const MAX_VISIBLE_LINES: usize = 12;
/// Hard cap on stored messages (oldest are dropped when exceeded).
const MAX_STORED_MESSAGES: usize = 500;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Severity level of a console message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    /// `console.log` / `console.info` / `console.debug`.
    Log,
    /// `console.warn`.
    Warn,
    /// `console.error`.
    Error,
}

impl ConsoleLevel {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Warn,
            2 => Self::Error,
            _ => Self::Log,
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Log => "",
            Self::Warn => "[warn] ",
            Self::Error => "[error] ",
        }
    }

    fn fg_color(self) -> Color {
        match self {
            Self::Log => FG_LOG,
            Self::Warn => FG_WARN,
            Self::Error => FG_ERROR,
        }
    }

    fn row_bg(self) -> Option<Color> {
        match self {
            Self::Warn => Some(WARN_BG),
            Self::Error => Some(ERROR_BG),
            Self::Log => None,
        }
    }
}

/// A single captured console message.
#[derive(Debug, Clone)]
pub struct ConsoleMessage {
    /// Severity level (log / warn / error).
    pub level: ConsoleLevel,
    /// Message text (already joined with spaces by the JS shim).
    pub text: String,
}

// ── Panel ─────────────────────────────────────────────────────────────────────

/// DevTools JS console panel.
///
/// Stores the last [`MAX_STORED_MESSAGES`] console messages and renders a
/// scrollable bottom overlay.  Toggled with `F12`.
pub struct ConsolePanel {
    messages: Vec<ConsoleMessage>,
    /// How many lines to skip from the bottom (0 = show tail; scrolling up increases).
    pub scroll_offset: usize,
    /// Whether the panel is currently shown.
    pub visible: bool,
}

impl Default for ConsolePanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsolePanel {
    /// Create a new, empty, hidden console panel.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            visible: false,
        }
    }

    /// Push a batch of `(level_u8, text)` entries drained from the JS runtime.
    ///
    /// `level` encoding: 0=log, 1=warn, 2=error (matches `QuickJsRuntime::console_messages`).
    /// Oldest messages are dropped when the buffer exceeds [`MAX_STORED_MESSAGES`].
    pub fn push_batch(&mut self, batch: Vec<(u8, String)>) {
        for (level, text) in batch {
            self.messages.push(ConsoleMessage {
                level: ConsoleLevel::from_u8(level),
                text,
            });
        }
        // Drop oldest if over cap.
        if self.messages.len() > MAX_STORED_MESSAGES {
            let drop = self.messages.len() - MAX_STORED_MESSAGES;
            self.messages.drain(..drop);
            // Clamp scroll offset in case we dropped messages that were above the view.
            self.scroll_offset = self.scroll_offset.saturating_sub(drop);
        }
    }

    /// Clear all stored messages and reset scroll.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Number of stored messages.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// `true` when no messages are stored.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Scroll up by `n` lines (towards older messages).
    #[allow(dead_code)]
    pub fn scroll_up(&mut self, n: usize) {
        let max = self.messages.len().saturating_sub(MAX_VISIBLE_LINES);
        self.scroll_offset = (self.scroll_offset + n).min(max);
    }

    /// Scroll down by `n` lines (towards newer messages).
    #[allow(dead_code)]
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the viewport-locked console panel overlay.
///
/// Returns an empty `DisplayList` when `panel.visible` is `false`.
/// `(win_w, win_h)` are the window dimensions in CSS pixels (same units used
/// by all other shell overlay builders).
pub fn build_console_panel(panel: &ConsolePanel, (win_w, win_h): (u32, u32)) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }

    let visible_count = panel.messages.len().min(MAX_VISIBLE_LINES);
    let panel_h = HEADER_H + visible_count as f32 * LINE_H;
    let panel_y = win_h as f32 - panel_h;
    let panel_w = win_w as f32;

    let mut out: DisplayList = Vec::with_capacity(4 + visible_count * 3);

    // Background
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, panel_y, panel_w, panel_h),
        color: BG,
    });

    // Header bar
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, panel_y, panel_w, HEADER_H),
        color: HEADER_BG,
    });

    // Header label
    out.push(make_text(
        format!("Console ({} messages)", panel.messages.len()),
        H_PAD,
        panel_y + (HEADER_H - FONT_SIZE) / 2.0,
        panel_w * 0.5,
        FONT_SIZE,
        FG_DIM,
    ));

    // "Clear / F12" hint in header
    out.push(make_text(
        "F12 to close".to_string(),
        panel_w - 120.0,
        panel_y + (HEADER_H - FONT_SIZE) / 2.0,
        110.0,
        FONT_SIZE,
        FG_DIM,
    ));

    // Clear button background hint (rightmost 50px)
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_w - 56.0, panel_y + 4.0, 48.0, HEADER_H - 8.0),
        color: CLEAR_BG,
    });
    out.push(make_text(
        "Clear".to_string(),
        panel_w - 50.0,
        panel_y + (HEADER_H - FONT_SIZE) / 2.0,
        44.0,
        FONT_SIZE,
        FG_DIM,
    ));

    // Message rows — show the last MAX_VISIBLE_LINES, respecting scroll_offset.
    let total = panel.messages.len();
    let end = total.saturating_sub(panel.scroll_offset);
    let start = end.saturating_sub(MAX_VISIBLE_LINES);

    for (i, msg) in panel.messages[start..end].iter().enumerate() {
        let row_y = panel_y + HEADER_H + i as f32 * LINE_H;

        // Row background for warn/error
        if let Some(rbg) = msg.level.row_bg() {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(0.0, row_y, panel_w, LINE_H),
                color: rbg,
            });
        }

        let label = format!("{}{}", msg.level.prefix(), msg.text);
        out.push(make_text(
            label,
            H_PAD,
            row_y + (LINE_H - FONT_SIZE) / 2.0,
            panel_w - H_PAD * 2.0,
            FONT_SIZE,
            msg.level.fg_color(),
        ));
    }

    // Scroll indicator if messages overflow visible area
    if total > MAX_VISIBLE_LINES {
        let indicator = if panel.scroll_offset > 0 {
            format!("↑↓  {}/{}", end, total)
        } else {
            format!("{}/{}", total, total)
        };
        // Draw at the far right of the header
        out.push(make_text(
            indicator,
            panel_w - 130.0,
            panel_y + (HEADER_H - FONT_SIZE) / 2.0,
            70.0,
            FONT_SIZE,
            FG_DIM,
        ));
    }

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_text(text: String, x: f32, y: f32, w: f32, font_size: f32, color: Color) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel_with(msgs: &[(u8, &str)]) -> ConsolePanel {
        let mut p = ConsolePanel::new();
        let batch: Vec<(u8, String)> = msgs.iter().map(|(l, s)| (*l, s.to_string())).collect();
        p.push_batch(batch);
        p
    }

    #[test]
    fn new_panel_empty_hidden() {
        let p = ConsolePanel::new();
        assert!(p.is_empty());
        assert!(!p.visible);
        assert_eq!(p.scroll_offset, 0);
    }

    #[test]
    fn toggle_visibility() {
        let mut p = ConsolePanel::new();
        assert!(!p.visible);
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn push_batch_stores_messages() {
        let p = make_panel_with(&[(0, "hello"), (1, "warning"), (2, "error msg")]);
        assert_eq!(p.len(), 3);
        assert_eq!(p.messages[0].level, ConsoleLevel::Log);
        assert_eq!(p.messages[1].level, ConsoleLevel::Warn);
        assert_eq!(p.messages[2].level, ConsoleLevel::Error);
        assert_eq!(p.messages[0].text, "hello");
    }

    #[test]
    fn clear_resets_state() {
        let mut p = make_panel_with(&[(0, "a"), (0, "b")]);
        p.scroll_offset = 1;
        p.clear();
        assert!(p.is_empty());
        assert_eq!(p.scroll_offset, 0);
    }

    #[test]
    fn push_batch_respects_max_stored() {
        let mut p = ConsolePanel::new();
        let batch: Vec<(u8, String)> = (0..MAX_STORED_MESSAGES + 10)
            .map(|i| (0u8, format!("msg {i}")))
            .collect();
        p.push_batch(batch);
        assert_eq!(p.len(), MAX_STORED_MESSAGES);
        // Oldest dropped — first kept message should be msg 10
        assert!(p.messages[0].text.contains("10"));
    }

    #[test]
    fn scroll_up_down_clamps() {
        let msgs: Vec<(u8, &str)> = (0..20).map(|_| (0u8, "x")).collect();
        let mut p = make_panel_with(&msgs);
        p.scroll_up(5);
        assert_eq!(p.scroll_offset, 5);
        p.scroll_down(10);
        assert_eq!(p.scroll_offset, 0);
        // Scrolling up more than available clamps to max
        p.scroll_up(9999);
        assert_eq!(p.scroll_offset, 20 - MAX_VISIBLE_LINES);
    }

    #[test]
    fn build_hidden_returns_empty() {
        let p = ConsolePanel::new(); // visible = false
        assert!(build_console_panel(&p, (1280, 800)).is_empty());
    }

    #[test]
    fn build_visible_empty_has_header() {
        let mut p = ConsolePanel::new();
        p.toggle();
        let dl = build_console_panel(&p, (1280, 800));
        assert!(!dl.is_empty());
        let has_console_label = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Console"))
        });
        assert!(has_console_label);
    }

    #[test]
    fn build_shows_log_message() {
        let mut p = make_panel_with(&[(0, "hello world")]);
        p.toggle();
        let dl = build_console_panel(&p, (1280, 800));
        let has_msg = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("hello world"))
        });
        assert!(has_msg);
    }

    #[test]
    fn build_shows_warn_prefix() {
        let mut p = make_panel_with(&[(1, "watch out")]);
        p.toggle();
        let dl = build_console_panel(&p, (1280, 800));
        let has_warn = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("[warn]"))
        });
        assert!(has_warn);
    }

    #[test]
    fn build_shows_error_prefix() {
        let mut p = make_panel_with(&[(2, "bad stuff")]);
        p.toggle();
        let dl = build_console_panel(&p, (1280, 800));
        let has_err = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("[error]"))
        });
        assert!(has_err);
    }

    #[test]
    fn build_caps_at_max_visible_lines() {
        let msgs: Vec<(u8, &str)> = (0..MAX_VISIBLE_LINES + 5).map(|_| (0u8, "x")).collect();
        let mut p = make_panel_with(&msgs);
        p.toggle();
        let dl = build_console_panel(&p, (1280, 800));
        // Count text entries containing "x" (message text rows only, not header)
        let row_count = dl.iter().filter(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.as_str() == "x")
        }).count();
        assert_eq!(row_count, MAX_VISIBLE_LINES);
    }

    #[test]
    fn console_level_from_u8() {
        assert_eq!(ConsoleLevel::from_u8(0), ConsoleLevel::Log);
        assert_eq!(ConsoleLevel::from_u8(1), ConsoleLevel::Warn);
        assert_eq!(ConsoleLevel::from_u8(2), ConsoleLevel::Error);
        assert_eq!(ConsoleLevel::from_u8(99), ConsoleLevel::Log);
    }
}
