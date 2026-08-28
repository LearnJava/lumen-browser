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

use crate::{LayoutSource, PageSource};

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
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
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

// — SPLIT SH-5: tab-snapshot field helpers moved out of main.rs ————————

/// URL-СЃС‚СЂРѕРєР° РёР· `PageSource` РґР»СЏ Р·Р°РїРёСЃРё РІ СЃРµСЃСЃРёСЋ, РёР»Рё `None` РґР»СЏ `Empty`
/// (РЅРµС‡РµРіРѕ РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°С‚СЊ). `File` в†’ РїСѓС‚СЊ, `Snapshot` в†’ `base_url`.
pub(crate) fn source_url_string(src: &PageSource) -> Option<String> {
    match src {
        PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => None,
        PageSource::File(p) => Some(p.display().to_string()),
        PageSource::Url(u) => Some(u.clone()),
        PageSource::Snapshot { base_url, .. } => Some(base_url.clone()),
    }
}

/// Bincode-СЃРµСЂРёР°Р»РёР·РѕРІР°РЅРЅС‹Р№ `Document` (`Document::to_bytes()`) РґР»СЏ РІРєР»Р°РґРєРё, РёР»Рё
/// РїСѓСЃС‚РѕР№ РІРµРєС‚РѕСЂ, РµСЃР»Рё СЃС‚СЂР°РЅРёС†Р° РЅРµ Р·Р°РіСЂСѓР¶РµРЅР° Р»РёР±Рѕ СЃРµСЂРёР°Р»РёР·Р°С†РёСЏ РЅРµ СѓРґР°Р»Р°СЃСЊ.
/// РџСѓСЃС‚РѕР№ blob РЅР° РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёРё РѕР·РЅР°С‡Р°РµС‚ fresh-navigate РїРѕ URL.
pub(crate) fn dom_blob_of(layout_source: Option<&LayoutSource>) -> Vec<u8> {
    layout_source
        .and_then(|ls| ls.document.lock().ok())
        .and_then(|doc| doc.to_bytes().ok())
        .unwrap_or_default()
}

/// Whether `run_window_mode` should restore the last on-disk session for the
/// initial tab: only for a truly argument-less launch (`source` is
/// [`PageSource::Empty`]) that isn't driven by an automation front-end.
///
/// `automation_mode` is `true` when `--bidi-port`/`--mcp-live-port` was
/// passed вЂ” those launches are documented as opening an empty window and the
/// driver always issues its own first navigation, so restoring a leftover
/// session tab would silently race it (BUG-296).
pub(crate) fn should_restore_session(source: &PageSource, automation_mode: bool) -> bool {
    matches!(source, PageSource::Empty) && !automation_mode
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
