//! Common key code constants for native input injection (§8C).
//!
//! These string codes match the W3C `KeyboardEvent.code` / `KeyboardEvent.key`
//! values dispatched by [`InputCommand::KeyDown`] / [`InputCommand::KeyUp`].
//!
//! All injected key events are created with `isTrusted=true` by the Rust→JS
//! binding `_lumen_dispatch_key_event`, which always sets the flag to `true`.
//! JS `dispatchEvent()` is never used.
//!
//! # Usage
//! ```ignore
//! use lumen_shell::input::{self, native};
//! let (tx, _rx) = input::channel();
//! tx.enter();            // submit a focused form
//! tx.tab();              // focus next element
//! tx.key_down(native::ARROW_DOWN);  // scroll selection
//! ```

/// Enter / Return key.
pub const ENTER: &str = "Enter";

/// Backspace (delete character before cursor).
pub const BACKSPACE: &str = "Backspace";

/// Tab key (focus next focusable element).
pub const TAB: &str = "Tab";

/// Escape key (dismiss dialogs, close menus, blur focused element).
/// For a simple Tab without shift use [`TAB`].
pub const ESCAPE: &str = "Escape";

/// Arrow key — move focus / scroll down.
#[allow(dead_code)]
pub const ARROW_DOWN: &str = "ArrowDown";

/// Arrow key — move focus / scroll up.
#[allow(dead_code)]
pub const ARROW_UP: &str = "ArrowUp";

/// Arrow key — move cursor / scroll left.
#[allow(dead_code)]
pub const ARROW_LEFT: &str = "ArrowLeft";

/// Arrow key — move cursor / scroll right.
#[allow(dead_code)]
pub const ARROW_RIGHT: &str = "ArrowRight";

/// Home key (move cursor to start of line / scroll to top).
#[allow(dead_code)]
pub const HOME: &str = "Home";

/// End key (move cursor to end of line / scroll to bottom).
#[allow(dead_code)]
pub const END: &str = "End";

/// Page Up key.
#[allow(dead_code)]
pub const PAGE_UP: &str = "PageUp";

/// Page Down key.
#[allow(dead_code)]
pub const PAGE_DOWN: &str = "PageDown";

/// Delete key (delete character after cursor).
#[allow(dead_code)]
pub const DELETE: &str = "Delete";

/// Space bar. `KeyboardEvent.key` is `" "` (single space); code is `"Space"`.
#[allow(dead_code)]
pub const SPACE: &str = "Space";

/// Given a key code as used by [`super::InputCommand::KeyDown`], return the matching
/// `KeyboardEvent.key` string (e.g. `"Space"` → `" "`, everything else → same string).
///
/// Per the W3C UI Events Key Values spec, printable single characters are their
/// own key value.  Special keys whose name differs from their code are listed
/// explicitly here.
pub(crate) fn code_to_key(code: &str) -> &str {
    match code {
        "Space" => " ",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_maps_to_space_char() {
        assert_eq!(code_to_key(SPACE), " ");
    }

    #[test]
    fn enter_maps_to_itself() {
        assert_eq!(code_to_key(ENTER), "Enter");
    }

    #[test]
    fn arrow_down_maps_to_itself() {
        assert_eq!(code_to_key(ARROW_DOWN), "ArrowDown");
    }

    #[test]
    fn constants_non_empty() {
        for code in [ENTER, BACKSPACE, TAB, ESCAPE, ARROW_DOWN, ARROW_UP, ARROW_LEFT, ARROW_RIGHT, HOME, END, PAGE_UP, PAGE_DOWN, DELETE, SPACE] {
            assert!(!code.is_empty(), "constant should not be empty");
        }
    }
}
