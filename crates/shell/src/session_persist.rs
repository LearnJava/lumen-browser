//! Cross-restart tab session persistence (§10I).
//!
//! On window close the shell serialises every open tab (URL + title + scroll +
//! DOM via `Document::to_bytes()`) into a single SQLite file via
//! [`lumen_storage::SessionStore`]. On the next launch — when the user started
//! the browser with no explicit page argument — the saved tabs are reopened:
//! the previously-active tab loads fresh through the normal pipeline, while the
//! background tabs are parked using the same hibernation machinery
//! (`hibernated_tabs` + `TabSnapshotStore`) so switching to one reconstructs it
//! from its stored DOM blob without a network round-trip.
//!
//! The store path is relative to the working directory, mirroring the existing
//! `last_session.lsession` JSON export. A separate file keeps the SQLite,
//! DOM-carrying session distinct from the portable JSON backup.

use lumen_storage::{PersistedTab, SessionStore};

/// On-disk file holding the last session for cross-restart restore.
///
/// Sits next to `last_session.lsession` (the portable JSON export); this one is
/// the engine's own SQLite memory and additionally carries serialised DOM.
pub const SESSION_DB_PATH: &str = "last_session.db";

/// Open the session store at [`SESSION_DB_PATH`], falling back to an in-memory
/// store if the file cannot be opened (read-only directory, locked file, …).
///
/// An in-memory fallback means session restore silently no-ops for that run
/// rather than aborting startup — losing the saved session is preferable to
/// failing to launch.
#[must_use]
pub fn open_store() -> SessionStore {
    SessionStore::open(SESSION_DB_PATH).unwrap_or_else(|e| {
        eprintln!("session: не удалось открыть {SESSION_DB_PATH}: {e}; сессия не сохранится");
        SessionStore::open_in_memory().expect("in-memory session store")
    })
}

/// Index of the tab to make active after restore: the first `is_active` tab, or
/// `0` when none is flagged (matches `session_export::active_tab` semantics).
///
/// Returns `0` for an empty slice; callers guard against restoring nothing.
#[must_use]
pub fn active_index(tabs: &[PersistedTab]) -> usize {
    tabs.iter().position(|t| t.is_active).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str, active: bool) -> PersistedTab {
        PersistedTab {
            url: url.into(),
            title: String::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            is_active: active,
            dom_blob: vec![],
        }
    }

    #[test]
    fn active_index_picks_flagged_tab() {
        let tabs = vec![tab("a", false), tab("b", true), tab("c", false)];
        assert_eq!(active_index(&tabs), 1);
    }

    #[test]
    fn active_index_defaults_to_first() {
        let tabs = vec![tab("a", false), tab("b", false)];
        assert_eq!(active_index(&tabs), 0);
    }

    #[test]
    fn active_index_empty_is_zero() {
        assert_eq!(active_index(&[]), 0);
    }
}
