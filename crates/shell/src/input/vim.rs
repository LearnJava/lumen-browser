//! Vim-style keyboard navigation mode (§7B.1).
//!
//! ## Mode overview
//!
//! ```text
//! Normal ──i──→ Insert
//! Insert ──Esc──→ Normal
//! ```
//!
//! **Normal mode** — navigation keys are active:
//! - `j` / `k` — scroll down / up one line
//! - `d` / `u` — scroll half-page down / up (vim Ctrl+D / Ctrl+U analogue)
//! - `g g` — scroll to document top
//! - `G` (Shift+G) — scroll to document bottom
//! - `f` / `F` — open hint mode (same-tab / new-tab)
//! - `t` — alias for `f` (open hint mode)
//! - `/` — open find bar
//! - `y y` — copy current page URL to clipboard (emits [`VimAction::Copy`])
//! - `H` (Shift+H) — history back
//! - `L` (Shift+L) — history forward
//! - `i` — switch to Insert mode
//! - `Ctrl+Alt+V` — deactivate Vim mode entirely
//!
//! **Insert mode** — all keys pass through to the browser (typing in forms etc.).
//! - `Escape` — return to Normal mode
//!
//! ## Integration
//!
//! Store a `VimMode` in the `Lumen` shell struct.  In `handle_key`, before the
//! global `keybinding_for` table, call [`VimMode::feed`] when the mode is active.
//! Act on the returned [`VimAction`].  In Insert mode `feed` returns
//! [`VimAction::PassThrough`] so the key falls through to the normal JS dispatch
//! path.

use winit::keyboard::{KeyCode, ModifiersState};

// ── State ─────────────────────────────────────────────────────────────────────

/// Which sub-mode the Vim keybinding layer is currently in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimState {
    /// Navigation keys active.  Single-key and multi-key sequences are handled.
    Normal,
    /// All keys pass through to the browser / page.  `Escape` returns to Normal.
    Insert,
}

/// First key of a two-key sequence, kept alive for one subsequent keypress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// `g` was pressed; next `g` = scroll top (gg), anything else = discard.
    G,
    /// `y` was pressed; next `y` = copy (yy), anything else = discard.
    Y,
}

// ── Action ────────────────────────────────────────────────────────────────────

/// Decoded action that the caller should execute in response to a keypress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimAction {
    /// Scroll one line down (`j`).
    ScrollDown,
    /// Scroll one line up (`k`).
    ScrollUp,
    /// Scroll half a page down (`d`).
    ScrollHalfPageDown,
    /// Scroll half a page up (`u`).
    ScrollHalfPageUp,
    /// Jump to the beginning of the document (`gg`).
    ScrollTop,
    /// Jump to the end of the document (`G`).
    ScrollBottom,
    /// Open hint mode — follow link in same tab (`f` / `t`).
    OpenHints,
    /// Open hint mode — follow link in new tab (`F`).
    OpenHintsNewTab,
    /// Open the find bar (`/`).
    OpenFind,
    /// Copy the current page URL to clipboard (`yy`).
    Copy,
    /// Navigate history back (`H`).
    HistoryBack,
    /// Navigate history forward (`L`).
    HistoryForward,
    /// The key was consumed to enter Insert mode (`i`).
    EnterInsert,
    /// The key was consumed to exit Insert mode (`Escape` in Insert).
    ExitInsert,
    /// Deactivate Vim mode entirely (`Ctrl+Alt+V`).
    Deactivate,
    /// Key passes through to the browser / page (Insert mode, or unrecognised).
    PassThrough,
    /// Key consumed but has no effect (e.g. dangling `g` after non-`g` key).
    Consumed,
}

// ── VimMode ───────────────────────────────────────────────────────────────────

/// Vim-mode state machine.
///
/// Create once and store in the `Lumen` shell struct.  Call [`VimMode::feed`]
/// on every `KeyboardInput` event when vim mode is active (i.e. when the
/// boolean field `vim_mode: Option<VimMode>` is `Some`).
#[derive(Debug, Clone)]
pub struct VimMode {
    /// Current sub-mode.
    pub state: VimState,
    /// Pending first key of a two-key sequence.
    pending: Option<Pending>,
}

impl VimMode {
    /// Create a new `VimMode` in [`VimState::Normal`].
    pub fn new() -> Self {
        Self { state: VimState::Normal, pending: None }
    }

    /// Feed one physical key event.  Returns the action to take.
    ///
    /// Only call on `ElementState::Pressed` events; key-repeat is allowed for
    /// scroll actions.
    pub fn feed(&mut self, code: KeyCode, mods: ModifiersState) -> VimAction {
        match self.state {
            VimState::Insert => self.feed_insert(code, mods),
            VimState::Normal => self.feed_normal(code, mods),
        }
    }

    // ── Insert mode ───────────────────────────────────────────────────────────

    fn feed_insert(&mut self, code: KeyCode, mods: ModifiersState) -> VimAction {
        if code == KeyCode::Escape && mods.is_empty() {
            self.state = VimState::Normal;
            return VimAction::ExitInsert;
        }
        // Deactivate still works in Insert mode.
        if self.is_deactivate(code, mods) {
            return VimAction::Deactivate;
        }
        VimAction::PassThrough
    }

    // ── Normal mode ───────────────────────────────────────────────────────────

    fn feed_normal(&mut self, code: KeyCode, mods: ModifiersState) -> VimAction {
        // Always handle deactivate first.
        if self.is_deactivate(code, mods) {
            self.pending = None;
            return VimAction::Deactivate;
        }

        let no_mods = mods.is_empty();
        let shift_only = mods == ModifiersState::SHIFT;

        // Resolve pending two-key sequences.
        if let Some(pending) = self.pending.take() {
            match pending {
                Pending::G => {
                    if code == KeyCode::KeyG && no_mods {
                        return VimAction::ScrollTop; // gg
                    }
                    // Non-`g` after `g`: discard pending, re-process this key.
                    // Fall through with pending = None (already taken).
                }
                Pending::Y => {
                    if code == KeyCode::KeyY && no_mods {
                        return VimAction::Copy; // yy
                    }
                    // Non-`y` after `y`: discard.
                    // Fall through.
                }
            }
            // Key didn't complete the sequence — fall through without pending.
        }

        match code {
            // Escape in Normal mode: swallow the key so it doesn't reach the
            // global keybinding table (which maps Escape → close the browser).
            KeyCode::Escape if no_mods => VimAction::Consumed,
            // Scroll
            KeyCode::KeyJ if no_mods => VimAction::ScrollDown,
            KeyCode::KeyK if no_mods => VimAction::ScrollUp,
            KeyCode::KeyD if no_mods => VimAction::ScrollHalfPageDown,
            KeyCode::KeyU if no_mods => VimAction::ScrollHalfPageUp,
            // gg — first key
            KeyCode::KeyG if no_mods => {
                self.pending = Some(Pending::G);
                VimAction::Consumed
            }
            // G (Shift+G) — bottom
            KeyCode::KeyG if shift_only => VimAction::ScrollBottom,
            // Hints
            KeyCode::KeyF if no_mods => VimAction::OpenHints,
            KeyCode::KeyF if shift_only => VimAction::OpenHintsNewTab,
            KeyCode::KeyT if no_mods => VimAction::OpenHints,
            // Find
            KeyCode::Slash if no_mods => VimAction::OpenFind,
            // Copy — first key
            KeyCode::KeyY if no_mods => {
                self.pending = Some(Pending::Y);
                VimAction::Consumed
            }
            // History
            KeyCode::KeyH if shift_only => VimAction::HistoryBack,
            KeyCode::KeyL if shift_only => VimAction::HistoryForward,
            // Enter insert mode
            KeyCode::KeyI if no_mods => {
                self.state = VimState::Insert;
                VimAction::EnterInsert
            }
            _ => VimAction::PassThrough,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn is_deactivate(&self, code: KeyCode, mods: ModifiersState) -> bool {
        let ctrl_alt = mods == (ModifiersState::CONTROL | ModifiersState::ALT);
        code == KeyCode::KeyV && ctrl_alt
    }
}

impl Default for VimMode {
    fn default() -> Self {
        Self::new()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn nm() -> ModifiersState { ModifiersState::empty() }
    fn shift() -> ModifiersState { ModifiersState::SHIFT }
    fn ctrl_alt() -> ModifiersState { ModifiersState::CONTROL | ModifiersState::ALT }

    fn vm() -> VimMode { VimMode::new() }

    // ── Basic scroll keys ─────────────────────────────────────────────────────

    #[test]
    fn j_scrolls_down() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyJ, nm()), VimAction::ScrollDown);
    }

    #[test]
    fn k_scrolls_up() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyK, nm()), VimAction::ScrollUp);
    }

    #[test]
    fn d_half_page_down() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyD, nm()), VimAction::ScrollHalfPageDown);
    }

    #[test]
    fn u_half_page_up() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyU, nm()), VimAction::ScrollHalfPageUp);
    }

    // ── gg sequence ──────────────────────────────────────────────────────────

    #[test]
    fn gg_scrolls_top() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyG, nm()), VimAction::Consumed);
        assert_eq!(m.pending, Some(Pending::G));
        assert_eq!(m.feed(KeyCode::KeyG, nm()), VimAction::ScrollTop);
        assert_eq!(m.pending, None);
    }

    #[test]
    fn g_then_non_g_discards_pending() {
        let mut m = vm();
        m.feed(KeyCode::KeyG, nm()); // pending = G
        // Second key is 'j' (not 'g') — pending discarded, 'j' executes
        let action = m.feed(KeyCode::KeyJ, nm());
        assert_eq!(action, VimAction::ScrollDown);
        assert_eq!(m.pending, None);
    }

    #[test]
    fn shift_g_scrolls_bottom() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyG, shift()), VimAction::ScrollBottom);
    }

    // ── yy sequence ──────────────────────────────────────────────────────────

    #[test]
    fn yy_copies() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyY, nm()), VimAction::Consumed);
        assert_eq!(m.feed(KeyCode::KeyY, nm()), VimAction::Copy);
        assert_eq!(m.pending, None);
    }

    #[test]
    fn y_then_non_y_discards_pending() {
        let mut m = vm();
        m.feed(KeyCode::KeyY, nm());
        let action = m.feed(KeyCode::KeyK, nm()); // 'k' after 'y' — pending discarded
        assert_eq!(action, VimAction::ScrollUp);
        assert_eq!(m.pending, None);
    }

    // ── Hints and find ───────────────────────────────────────────────────────

    #[test]
    fn f_opens_hints() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyF, nm()), VimAction::OpenHints);
    }

    #[test]
    fn shift_f_opens_hints_new_tab() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyF, shift()), VimAction::OpenHintsNewTab);
    }

    #[test]
    fn t_opens_hints() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyT, nm()), VimAction::OpenHints);
    }

    #[test]
    fn slash_opens_find() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::Slash, nm()), VimAction::OpenFind);
    }

    // ── History navigation ───────────────────────────────────────────────────

    #[test]
    fn shift_h_history_back() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyH, shift()), VimAction::HistoryBack);
    }

    #[test]
    fn shift_l_history_forward() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyL, shift()), VimAction::HistoryForward);
    }

    // ── Insert mode transitions ──────────────────────────────────────────────

    #[test]
    fn i_enters_insert_mode() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyI, nm()), VimAction::EnterInsert);
        assert_eq!(m.state, VimState::Insert);
    }

    #[test]
    fn escape_in_insert_returns_to_normal() {
        let mut m = vm();
        m.feed(KeyCode::KeyI, nm());
        assert_eq!(m.feed(KeyCode::Escape, nm()), VimAction::ExitInsert);
        assert_eq!(m.state, VimState::Normal);
    }

    #[test]
    fn keys_pass_through_in_insert_mode() {
        let mut m = vm();
        m.feed(KeyCode::KeyI, nm()); // enter insert
        assert_eq!(m.feed(KeyCode::KeyJ, nm()), VimAction::PassThrough);
        assert_eq!(m.feed(KeyCode::KeyK, nm()), VimAction::PassThrough);
        assert_eq!(m.feed(KeyCode::KeyG, nm()), VimAction::PassThrough);
        // Still in insert mode
        assert_eq!(m.state, VimState::Insert);
    }

    #[test]
    fn navigation_works_again_after_returning_to_normal() {
        let mut m = vm();
        m.feed(KeyCode::KeyI, nm());
        m.feed(KeyCode::Escape, nm());
        assert_eq!(m.feed(KeyCode::KeyJ, nm()), VimAction::ScrollDown);
    }

    // ── Deactivate ───────────────────────────────────────────────────────────

    #[test]
    fn ctrl_alt_v_deactivates_in_normal() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyV, ctrl_alt()), VimAction::Deactivate);
    }

    #[test]
    fn ctrl_alt_v_deactivates_in_insert() {
        let mut m = vm();
        m.feed(KeyCode::KeyI, nm());
        assert_eq!(m.feed(KeyCode::KeyV, ctrl_alt()), VimAction::Deactivate);
    }

    #[test]
    fn ctrl_alt_v_clears_pending() {
        let mut m = vm();
        m.feed(KeyCode::KeyG, nm()); // pending = G
        m.feed(KeyCode::KeyV, ctrl_alt()); // deactivate
        assert_eq!(m.pending, None);
    }

    // ── Pass-through in normal ────────────────────────────────────────────────

    #[test]
    fn escape_consumed_in_normal_mode() {
        // Escape must NOT propagate to global keybindings (which would close browser).
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::Escape, nm()), VimAction::Consumed);
        assert_eq!(m.state, VimState::Normal); // still Normal, not closed
    }

    #[test]
    fn unknown_key_passes_through_in_normal() {
        let mut m = vm();
        assert_eq!(m.feed(KeyCode::KeyZ, nm()), VimAction::PassThrough);
    }

    #[test]
    fn ctrl_r_passes_through_in_normal() {
        let mut m = vm();
        let ctrl = ModifiersState::CONTROL;
        assert_eq!(m.feed(KeyCode::KeyR, ctrl), VimAction::PassThrough);
    }

    // ── Repeat scroll (held key) ─────────────────────────────────────────────

    #[test]
    fn repeated_j_all_scroll_down() {
        let mut m = vm();
        for _ in 0..5 {
            assert_eq!(m.feed(KeyCode::KeyJ, nm()), VimAction::ScrollDown);
        }
    }
}
