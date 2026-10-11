//! Tab auto-archive (7A.5): hides tabs inactive for > 12 h from the strip.
//!
//! Auto-archive is a UI-only concept: when a background tab has not been
//! activated for [`ARCHIVE_AFTER_MS`] milliseconds, it is removed from the
//! visible [`TabStrip`] and its title + URL are stored in [`TabArchive`].
//! The full [`PageSnapshot`] is evicted from `bg_tabs` to free memory.
//!
//! Restoration opens a fresh navigation to the stored URL — the full page
//! state is not preserved (that is the job of the T3 hibernate track 10).
//!
//! CC-15-3: the archive toolbar button + drop-down panel painters
//! (`build_button`, `build_panel`) and their hit-test (`hit_test_button`)
//! were removed once the engine-drawn chrome (CC-4) made their sole caller
//! (the legacy tab-bar paint/dispatch in `main.rs`) dead code.
//! UX-CHROME-PANELS slice 19: `hit_test_panel` is gone too — it guessed a
//! 320 px panel and stole clicks the chrome dispatch had not consumed.
//!
//! BUG-408: the button + panel painters above were legacy-only and are not
//! reintroduced — the engine chrome renders `#archivePanel` instead
//! (`lumen_chrome::model::bind_archive`, `assets/chrome/chrome.html`),
//! dispatched via `ChromeAction::ToggleArchive`/`ArchiveRestore`/
//! `ArchiveDismiss` in `crates/shell/src/main.rs`.

use crate::tabs::containers::ContainerKind;

// ── Constants ──────────────────────────────────────────────────────────────────

/// Width of the archive button appended to the right of the tab bar, in CSS px.
pub const ARCHIVE_BTN_W: f32 = 36.0;

/// Background tabs idle for longer than this threshold are auto-archived.
/// Value: 12 hours in session-elapsed milliseconds.
pub const ARCHIVE_AFTER_MS: f64 = 12.0 * 3600.0 * 1000.0;

/// Maximum rows shown in one page of the archive panel without scrolling.
const MAX_VISIBLE_ROWS: usize = 8;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A tab that was auto-archived and removed from the visible tab strip.
pub struct ArchivedTab {
    /// Original tab ID (for reference only — not reused on restore).
    pub id: usize,
    /// Display title at the time of archiving.
    pub title: String,
    /// Page URL string; empty for blank/file tabs without a navigable URL.
    pub url: String,
    /// Container colour class of the archived tab.
    ///
    /// Rendered as a 3 px left-side colour strip in the archive panel row,
    /// identical to the border-top strip in the tab bar (7D.2).
    pub container: ContainerKind,
}

/// State of the tab archive system.
pub struct TabArchive {
    /// Archived tab entries, newest-first.
    pub entries: Vec<ArchivedTab>,
    /// Whether the archive panel is currently visible.
    pub visible: bool,
    /// Index of the first visible row when the list overflows.
    pub scroll_row: usize,
}

impl Default for TabArchive {
    fn default() -> Self {
        Self::new()
    }
}

impl TabArchive {
    /// Create an empty archive with the panel closed.
    pub fn new() -> Self {
        Self { entries: Vec::new(), visible: false, scroll_row: 0 }
    }

    /// Push a newly-archived tab (prepend — newest entry shown first).
    pub fn push(&mut self, tab: ArchivedTab) {
        self.entries.insert(0, tab);
    }

    /// Remove and return the archived entry with the given original tab `id`.
    pub fn take(&mut self, id: usize) -> Option<ArchivedTab> {
        let pos = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(pos))
    }

    /// Close panel without clearing entries.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Flip panel visibility — mirrors `download::DownloadManager::toggle_visible`.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Scroll up by one row (clamped at zero).
    #[allow(dead_code)]
    pub fn scroll_up(&mut self) {
        self.scroll_row = self.scroll_row.saturating_sub(1);
    }

    /// Scroll down by one row (clamped at last page).
    #[allow(dead_code)]
    pub fn scroll_down(&mut self) {
        let max_row = self.entries.len().saturating_sub(MAX_VISIBLE_ROWS);
        if self.scroll_row < max_row {
            self.scroll_row += 1;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tab(id: usize, url: &str) -> ArchivedTab {
        ArchivedTab {
            id,
            title: format!("Tab {id}"),
            url: url.to_owned(),
            container: ContainerKind::None,
        }
    }

    #[test]
    fn new_archive_is_empty() {
        let a = TabArchive::new();
        assert_eq!(a.entries.len(), 0);
        assert!(!a.visible);
    }

    #[test]
    fn push_prepends_newest_first() {
        let mut a = TabArchive::new();
        a.push(make_tab(1, "https://a.com"));
        a.push(make_tab(2, "https://b.com"));
        assert_eq!(a.entries[0].id, 2); // newest first
        assert_eq!(a.entries[1].id, 1);
    }

    #[test]
    fn take_removes_by_id() {
        let mut a = TabArchive::new();
        a.push(make_tab(10, "https://x.com"));
        a.push(make_tab(20, "https://y.com"));
        let removed = a.take(10).unwrap();
        assert_eq!(removed.id, 10);
        assert_eq!(a.entries.len(), 1);
        assert_eq!(a.entries[0].id, 20);
    }

    #[test]
    fn take_missing_id_returns_none() {
        let mut a = TabArchive::new();
        assert!(a.take(99).is_none());
    }

    #[test]
    fn scroll_down_clamps_at_last_page() {
        let mut a = TabArchive::new();
        for i in 0..10 {
            a.push(make_tab(i, ""));
        }
        // MAX_VISIBLE_ROWS = 8, so max scroll_row = 10 - 8 = 2.
        for _ in 0..20 {
            a.scroll_down();
        }
        assert_eq!(a.scroll_row, 2);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let mut a = TabArchive::new();
        a.scroll_row = 0;
        a.scroll_up();
        assert_eq!(a.scroll_row, 0);
    }

    #[test]
    fn archive_after_ms_is_twelve_hours() {
        assert_eq!(ARCHIVE_AFTER_MS, 43_200_000.0);
    }
}
