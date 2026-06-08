//! Native input injection for the shell event loop (ADR-007 §8C).
//!
//! [`InputCommand`] lets callers inject synthetic mouse / keyboard / scroll
//! events that flow through the **same `about_to_wait` processing path** as
//! real OS events — click handlers fire JS with `isTrusted=true`, form values
//! update, link navigation works.
//!
//! # Channel architecture
//!
//! ```text
//! caller (BrowserSession, test, MCP tool)
//!   └─ InputSender::click / type_text / key_down / scroll
//!         └─ mpsc::Sender<InputCommand>
//!               ↓
//!         Lumen.input_rx  (drained each about_to_wait)
//!               └─ handle_click_at / dispatch_mouse_move / inject_char
//!                  inject_special_key / scroll_to
//! ```
//!
//! # isTrusted guarantee
//!
//! All injected events are dispatched via the Rust→JS bindings
//! (`_lumen_dispatch_mouse_event`, `_lumen_dispatch_key_event`), which always
//! create events with `isTrusted=true`.  JS `dispatchEvent()` is never used.

pub mod gesture;
pub mod humanlike;
pub mod native;
pub mod vim;

use std::sync::mpsc;

// ── InputCommand ─────────────────────────────────────────────────────────────

/// A single injected input command.
///
/// Sent via [`InputSender`] and consumed by the shell event loop.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum InputCommand {
    /// Synthetic left-button click at the given CSS-pixel coordinates.
    ///
    /// The shell will hit-test `(x, y)`, dispatch a JS `click` `MouseEvent`
    /// with `isTrusted=true`, handle form controls (checkbox, submit), and
    /// follow `<a href>` links — exactly as if the user clicked.
    Click {
        /// CSS-pixel X coordinate (viewport-relative, not page-space).
        x: f32,
        /// CSS-pixel Y coordinate (viewport-relative, not page-space).
        y: f32,
    },

    /// Synthetic mouse-move to the given CSS-pixel coordinates.
    ///
    /// Does **not** trigger a click.  Dispatches a JS `mousemove` `MouseEvent`
    /// with `isTrusted=true` at the target position.  Used by
    /// [`humanlike::HumanLikeSender`] to trace Bézier-curve paths before
    /// the final click.
    MouseMove {
        /// CSS-pixel X coordinate (viewport-relative, not page-space).
        x: f32,
        /// CSS-pixel Y coordinate (viewport-relative, not page-space).
        y: f32,
    },

    /// Type text into the currently focused element.
    ///
    /// For each code point the shell fires `keydown` → `input` → `keyup`
    /// JS events via `_lumen_dispatch_key_event` (isTrusted=true).
    TypeText {
        /// The string to type.
        text: String,
    },

    /// Instantly scroll the viewport to a CSS-pixel position.
    Scroll {
        /// Horizontal scroll offset in CSS pixels (0 = leftmost).
        x: f32,
        /// Vertical scroll offset in CSS pixels (0 = top).
        y: f32,
    },

    /// Press and immediately release a special (non-printable) key.
    ///
    /// Fires `keydown` → `keyup` via `_lumen_dispatch_key_event` (isTrusted=true).
    /// Use [`native`] constants for the `code` string (e.g. `native::ENTER`,
    /// `native::BACKSPACE`).  The `KeyboardEvent.key` value is derived from
    /// `code` via [`native::code_to_key`] ("Space" → `" "`, everything else
    /// passes through unchanged).
    ///
    /// Note: for printable characters use [`TypeText`](InputCommand::TypeText)
    /// which also fires the `input` event required to update `<input>` values.
    KeyDown {
        /// W3C `KeyboardEvent.code` string, e.g. `"Enter"`, `"Backspace"`, `"ArrowDown"`.
        code: String,
    },
}

// ── InputSender / InputReceiver ──────────────────────────────────────────────

/// Sender side of the input injection channel.
///
/// Cloneable and `Send + Sync` — callers on any thread can use this to inject
/// events into the shell event loop without blocking.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InputSender(mpsc::Sender<InputCommand>);

impl InputSender {
    /// Send a synthetic left-click at CSS-pixel coordinates `(x, y)`.
    #[allow(dead_code)]
    pub fn click(&self, x: f32, y: f32) {
        let _ = self.0.send(InputCommand::Click { x, y });
    }

    /// Send a synthetic mouse-move event to CSS-pixel coordinates `(x, y)`.
    #[allow(dead_code)]
    pub fn mouse_move(&self, x: f32, y: f32) {
        let _ = self.0.send(InputCommand::MouseMove { x, y });
    }

    /// Send a synthetic text-typing command.
    #[allow(dead_code)]
    pub fn type_text(&self, text: &str) {
        let _ = self.0.send(InputCommand::TypeText { text: text.to_owned() });
    }

    /// Send a synthetic scroll command to position `(x, y)` in CSS pixels.
    #[allow(dead_code)]
    pub fn scroll(&self, x: f32, y: f32) {
        let _ = self.0.send(InputCommand::Scroll { x, y });
    }

    /// Press and release a special key identified by its W3C `KeyboardEvent.code`.
    ///
    /// Use [`native`] constants for the code string: `native::ENTER`,
    /// `native::BACKSPACE`, `native::TAB`, `native::ARROW_DOWN`, etc.
    /// Fires `keydown` → `keyup` with `isTrusted=true`.
    #[allow(dead_code)]
    pub fn key_down(&self, code: &str) {
        let _ = self.0.send(InputCommand::KeyDown { code: code.to_owned() });
    }

    /// Press Enter in the focused element (submits forms, confirms dialogs).
    #[allow(dead_code)]
    pub fn enter(&self) {
        self.key_down(native::ENTER);
    }

    /// Press Backspace in the focused element (deletes character before cursor).
    #[allow(dead_code)]
    pub fn backspace(&self) {
        self.key_down(native::BACKSPACE);
    }

    /// Press Tab (move focus to the next focusable element).
    #[allow(dead_code)]
    pub fn tab(&self) {
        self.key_down(native::TAB);
    }

    /// Press Escape (dismiss dialogs, close menus, blur focused element).
    #[allow(dead_code)]
    pub fn escape(&self) {
        self.key_down(native::ESCAPE);
    }
}

/// Receiver side of the input injection channel.
///
/// Stored in [`Lumen`](crate::Lumen) and drained on each `about_to_wait`.
pub struct InputReceiver(mpsc::Receiver<InputCommand>);

impl InputReceiver {
    /// Non-blocking drain: returns all pending commands without blocking.
    pub fn drain(&self) -> Vec<InputCommand> {
        self.0.try_iter().collect()
    }
}

/// Create a new input injection channel.
///
/// Returns `(sender, receiver)`.  Store the receiver in [`Lumen`] and hand
/// the sender to callers (BrowserSession, MCP, tests).
pub fn channel() -> (InputSender, InputReceiver) {
    let (tx, rx) = mpsc::channel();
    (InputSender(tx), InputReceiver(rx))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_click_roundtrip() {
        let (tx, rx) = channel();
        tx.click(100.0, 200.0);
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::Click { x, y } => {
                assert!((x - 100.0).abs() < f32::EPSILON);
                assert!((y - 200.0).abs() < f32::EPSILON);
            }
            _ => panic!("expected Click"),
        }
    }

    #[test]
    fn channel_mouse_move_roundtrip() {
        let (tx, rx) = channel();
        tx.mouse_move(50.0, 75.0);
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::MouseMove { x, y } => {
                assert!((x - 50.0).abs() < f32::EPSILON);
                assert!((y - 75.0).abs() < f32::EPSILON);
            }
            _ => panic!("expected MouseMove"),
        }
    }

    #[test]
    fn channel_type_text_roundtrip() {
        let (tx, rx) = channel();
        tx.type_text("hello");
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::TypeText { text } => assert_eq!(text, "hello"),
            _ => panic!("expected TypeText"),
        }
    }

    #[test]
    fn channel_scroll_roundtrip() {
        let (tx, rx) = channel();
        tx.scroll(0.0, 300.0);
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::Scroll { x, y } => {
                assert!((x - 0.0).abs() < f32::EPSILON);
                assert!((y - 300.0).abs() < f32::EPSILON);
            }
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn drain_empty_returns_empty() {
        let (_tx, rx) = channel();
        assert!(rx.drain().is_empty());
    }

    #[test]
    fn sender_clone_sends_to_same_receiver() {
        let (tx, rx) = channel();
        let tx2 = tx.clone();
        tx.click(1.0, 2.0);
        tx2.click(3.0, 4.0);
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn multiple_commands_preserved_in_order() {
        let (tx, rx) = channel();
        tx.click(10.0, 20.0);
        tx.type_text("abc");
        tx.scroll(0.0, 100.0);
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(cmds[0], InputCommand::Click { .. }));
        assert!(matches!(cmds[1], InputCommand::TypeText { .. }));
        assert!(matches!(cmds[2], InputCommand::Scroll { .. }));
    }

    #[test]
    fn drain_after_sender_drop_returns_empty() {
        let (tx, rx) = channel();
        drop(tx);
        assert!(rx.drain().is_empty());
    }

    #[test]
    fn key_down_roundtrip() {
        let (tx, rx) = channel();
        tx.key_down("Enter");
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::KeyDown { code } => assert_eq!(code, "Enter"),
            _ => panic!("expected KeyDown"),
        }
    }

    #[test]
    fn enter_convenience_sends_key_down_enter() {
        let (tx, rx) = channel();
        tx.enter();
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::KeyDown { code } => assert_eq!(code, "Enter"),
            _ => panic!("expected KeyDown(Enter)"),
        }
    }

    #[test]
    fn backspace_convenience_sends_key_down_backspace() {
        let (tx, rx) = channel();
        tx.backspace();
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            InputCommand::KeyDown { code } => assert_eq!(code, "Backspace"),
            _ => panic!("expected KeyDown(Backspace)"),
        }
    }

    #[test]
    fn tab_and_escape_convenience() {
        let (tx, rx) = channel();
        tx.tab();
        tx.escape();
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 2);
        match (&cmds[0], &cmds[1]) {
            (InputCommand::KeyDown { code: a }, InputCommand::KeyDown { code: b }) => {
                assert_eq!(a, "Tab");
                assert_eq!(b, "Escape");
            }
            _ => panic!("expected two KeyDown commands"),
        }
    }
}
