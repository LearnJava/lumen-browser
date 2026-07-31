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
//! (the legacy tab-bar paint/dispatch in `main.rs`) dead code. `hit_test_panel`
//! stays — it is reachable from the click-outside-panel path unconditionally.
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

const HEADER_H: f32 = 32.0;
const ROW_H: f32 = 44.0;
const PANEL_W: f32 = 320.0;

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

/// Hit result from the archive button or panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveHit {
    /// Clicked ↺ restore on the entry with this id.
    Restore(usize),
    /// Clicked × dismiss on the entry with this id.
    Dismiss(usize),
    /// Clicked inside the panel body (no specific control) — swallows event.
    Inside,
    /// Clicked outside the panel — should close it.
    Outside,
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

// ── Geometry helpers ───────────────────────────────────────────────────────────

/// Right edge of the archive panel when anchored to the window right.
fn panel_left(window_w: f32) -> f32 {
    window_w - PANEL_W
}

fn panel_height(n_entries: usize) -> f32 {
    let visible = n_entries.min(MAX_VISIBLE_ROWS);
    HEADER_H + visible as f32 * ROW_H
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Hit-test the archive panel when it is open.
///
/// Returns `None` if the panel is hidden.  Otherwise returns an [`ArchiveHit`]
/// variant that describes what was clicked.
pub fn hit_test_panel(
    archive: &TabArchive,
    x: f32,
    y: f32,
    window_w: f32,
    tab_bar_h: f32,
) -> Option<ArchiveHit> {
    if !archive.visible {
        return None;
    }
    let pl = panel_left(window_w);
    let pt = tab_bar_h;
    let ph = panel_height(archive.entries.len());
    let pb = pt + ph;

    // Click outside panel bounds → dismiss.
    if x < pl || x >= window_w || y < pt || y >= pb {
        return Some(ArchiveHit::Outside);
    }

    let rel_y = y - pt;
    if rel_y < HEADER_H {
        // Header row — swallow click.
        return Some(ArchiveHit::Inside);
    }

    let row_y = rel_y - HEADER_H;
    let row_local = (row_y / ROW_H) as usize;
    let entry_idx = archive.scroll_row + row_local;

    let Some(entry) = archive.entries.get(entry_idx) else {
        return Some(ArchiveHit::Inside);
    };
    let entry_id = entry.id;

    // Restore button occupies the left 28 px of the row.
    // Dismiss button occupies the right 28 px of the row.
    if x < pl + 28.0 {
        return Some(ArchiveHit::Restore(entry_id));
    }
    if x >= window_w - 28.0 {
        return Some(ArchiveHit::Dismiss(entry_id));
    }

    Some(ArchiveHit::Inside)
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
    fn hit_test_panel_returns_none_when_closed() {
        let a = TabArchive::new();
        assert!(hit_test_panel(&a, 900.0, 50.0, 1024.0, 36.0).is_none());
    }

    #[test]
    fn hit_test_panel_outside_returns_outside() {
        let mut a = TabArchive::new();
        a.push(make_tab(1, "https://a.com"));
        a.visible = true;
        // x far left of panel — outside.
        let hit = hit_test_panel(&a, 100.0, 50.0, 1024.0, 36.0);
        assert_eq!(hit, Some(ArchiveHit::Outside));
    }

    #[test]
    fn hit_test_panel_restore_button_detected() {
        let mut a = TabArchive::new();
        a.push(make_tab(5, "https://example.com"));
        a.visible = true;
        // Row 0 starts at y = tab_bar_h + HEADER_H = 36 + 32 = 68.
        // Restore button: x < panel_left + 28 = (1024-320) + 28 = 732.
        let hit = hit_test_panel(&a, 710.0, 70.0, 1024.0, 36.0);
        assert_eq!(hit, Some(ArchiveHit::Restore(5)));
    }

    #[test]
    fn hit_test_panel_dismiss_button_detected() {
        let mut a = TabArchive::new();
        a.push(make_tab(7, "https://example.com"));
        a.visible = true;
        // Dismiss button: x >= window_w - 28 = 1024 - 28 = 996.
        let hit = hit_test_panel(&a, 1010.0, 70.0, 1024.0, 36.0);
        assert_eq!(hit, Some(ArchiveHit::Dismiss(7)));
    }

    #[test]
    fn archive_after_ms_is_twelve_hours() {
        assert_eq!(ARCHIVE_AFTER_MS, 43_200_000.0);
    }
}
