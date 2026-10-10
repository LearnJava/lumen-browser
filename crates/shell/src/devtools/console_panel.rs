//! DevTools JS console panel (§7E.5).
//!
//! Captures `console.log/warn/error` output from JS and renders a scrollable
//! list of messages; drawn by `#devtools` in the engine chrome
//! (`lumen_chrome::bind_console`, UX-CHROME-PANELS).  Toggle with `F12`.
//!
//! # Architecture
//!
//! Messages are buffered in `QuickJsRuntime::console_messages` (Arc<Mutex<Vec<(u8, String)>>>)
//! and drained each `about_to_wait` into `ConsolePanel::push`.  The shell snapshots the
//! tail of the buffer into `ChromeConsoleModel`.
//!
//! # Layout
//!
//! Messages are displayed newest-last (scroll_offset = 0 shows the tail).

// ── Layout constants ──────────────────────────────────────────────────────────

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
    ///
    /// Called on every navigation so `AutomationCommand::ConsoleLog` (DEVX-1)
    /// reflects only the current page's console output.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    /// All stored messages, oldest first — feeds `AutomationCommand::ConsoleLog` (DEVX-1).
    pub fn messages(&self) -> &[ConsoleMessage] {
        &self.messages
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
    fn console_level_from_u8() {
        assert_eq!(ConsoleLevel::from_u8(0), ConsoleLevel::Log);
        assert_eq!(ConsoleLevel::from_u8(1), ConsoleLevel::Warn);
        assert_eq!(ConsoleLevel::from_u8(2), ConsoleLevel::Error);
        assert_eq!(ConsoleLevel::from_u8(99), ConsoleLevel::Log);
    }
}
