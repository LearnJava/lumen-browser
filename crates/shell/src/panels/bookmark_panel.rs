//! Bookmark manager panel (7-series shell UI, task #22).
//!
//! A floating overlay anchored to the toolbar (top-left of the page viewport)
//! that lets the user browse, open, delete and re-file bookmarks.  Layout:
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ Bookmarks                                  × │  header
//! │ ┌─────────────────────────────────────────┐ │
//! │ │ search…                                  │ │  search box
//! │ └─────────────────────────────────────────┘ │
//! │ ┌──────────┬──────────────────────────────┐ │
//! │ │ All      │ Rust — Title             ×   │ │
//! │ │ /Work    │ https://rust-lang.org/        │ │
//! │ │ /Reading │ ──────────────────────────── │ │  folder tree │ list
//! │ │          │ Example — Title          ×   │ │
//! │ │          │ https://example.com/          │ │
//! │ └──────────┴──────────────────────────────┘ │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! Toggled with `Ctrl+Shift+O`.  The panel is a self-contained overlay (it does
//! not change the page viewport size), following the ad-hoc panel convention of
//! [`super::workspace_panel`] / [`super::sidebar_panel`]: state lives on `Lumen`,
//! [`hit_test`] classifies clicks. The legacy display-list renderer was removed
//! in CC-15-4 - under the engine chrome the panel is `#view-bookmarks`.
//!
//! **Folder filter.** The left column lists "All" plus every distinct folder.
//! Clicking one filters the bookmark list (and the active search query).
//!
//! **Search.** When the search box is focused, typed characters filter the list
//! by case-insensitive substring match against title *and* URL.
//!
//! **Drag-and-drop re-file.** Pressing on a bookmark row begins a drag
//! ([`BookmarkPanel::begin_drag`]); releasing over a folder in the left column
//! moves the bookmark into that folder (persisted via `Bookmarks::set_folder`).
//! Releasing elsewhere opens the bookmark instead (a plain click).

// ── Visual constants ─────────────────────────────────────────────────────────

/// Total panel width in CSS px.
pub const PANEL_WIDTH: f32 = 460.0;

/// Total panel height in CSS px.
pub const PANEL_HEIGHT: f32 = 380.0;

/// Header strip height (title + close button).
const HEADER_H: f32 = 30.0;

/// Search box height.
const SEARCH_H: f32 = 26.0;

/// Width of the left folder-tree column.
const FOLDER_COL_W: f32 = 130.0;

/// Height of a single folder row.
const FOLDER_ROW_H: f32 = 24.0;

/// Height of a single bookmark row (title line + url line).
const BM_ROW_H: f32 = 38.0;

/// Outer padding inside the panel.
const PAD: f32 = 8.0;

/// Width of the trailing "×" delete zone on a bookmark row.
const DELETE_W: f32 = 22.0;

// ── Data types ────────────────────────────────────────────────────────────────

/// Lightweight bookmark entry used for panel rendering (loaded from the
/// `Bookmarks` store on every panel refresh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BmEntry {
    /// Bookmark database id.
    pub id: i64,
    /// Full bookmark URL (used as the storage key and for navigation).
    pub url: String,
    /// Display title (may be empty — the URL is shown as a fallback).
    pub title: String,
    /// Folder path the bookmark belongs to (`""` = root).
    pub folder: String,
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Bookmark manager panel state.
pub struct BookmarkPanel {
    /// `true` while the panel overlay is visible.  Toggled via `Ctrl+Shift+O`.
    pub visible: bool,
    /// `true` while the search box has keyboard focus (typed chars filter the
    /// list rather than triggering global shortcuts).
    pub search_active: bool,
    /// Cached bookmark list — refreshed after every storage mutation.
    pub entries: Vec<BmEntry>,
    /// Distinct folder paths (excluding the root `""`), sorted ascending.
    pub folders: Vec<String>,
    /// Active folder filter.  `None` = show all folders ("All" row).
    pub selected_folder: Option<String>,
    /// Current search query (case-insensitive substring filter).
    pub search: String,
    /// Vertical scroll offset of the bookmark list in CSS px.
    pub scroll_y: f32,
    /// Id of the bookmark currently being dragged, if any.
    pub drag: Option<i64>,
}

impl BookmarkPanel {
    /// Create a new (hidden) panel with an empty bookmark list.
    pub fn new() -> Self {
        Self {
            visible: false,
            search_active: false,
            entries: Vec::new(),
            folders: Vec::new(),
            selected_folder: None,
            search: String::new(),
            scroll_y: 0.0,
            drag: None,
        }
    }

    /// Flip visibility.  Resets transient state (search focus, drag) when hiding.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.search_active = false;
            self.drag = None;
        }
    }

    /// Replace the cached bookmark list and recompute the folder set.
    pub fn set_data(&mut self, entries: Vec<BmEntry>) {
        let mut folders: Vec<String> = entries
            .iter()
            .map(|e| e.folder.clone())
            .filter(|f| !f.is_empty())
            .collect();
        folders.sort();
        folders.dedup();
        self.folders = folders;
        self.entries = entries;
        // Drop a stale folder filter that no longer exists.
        if let Some(ref f) = self.selected_folder
            && !self.folders.contains(f)
        {
            self.selected_folder = None;
        }
    }

    /// Bookmarks visible under the current folder filter and search query, in
    /// display order.
    pub fn visible_entries(&self) -> Vec<&BmEntry> {
        let needle = self.search.to_lowercase();
        self.entries
            .iter()
            .filter(|e| match &self.selected_folder {
                Some(f) => &e.folder == f,
                None => true,
            })
            .filter(|e| {
                needle.is_empty()
                    || e.title.to_lowercase().contains(&needle)
                    || e.url.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Append typed text to the search query (called while `search_active`).
    pub fn append_search(&mut self, text: &str) {
        self.search.push_str(text);
        self.scroll_y = 0.0;
    }

    /// Delete the last character of the search query.
    pub fn backspace_search(&mut self) {
        self.search.pop();
        self.scroll_y = 0.0;
    }

    /// Begin dragging the bookmark with the given id.
    pub fn begin_drag(&mut self, id: i64) {
        self.drag = Some(id);
    }

    /// Take (and clear) the dragged bookmark id, if a drag is in progress.
    pub fn take_drag(&mut self) -> Option<i64> {
        self.drag.take()
    }

    /// Scroll the bookmark list by `dy` CSS px, clamped to `[0, max]` where
    /// `max` is derived from the number of visible rows and the fixed list
    /// viewport height.
    pub fn scroll_by(&mut self, dy: f32) {
        let content_h = self.visible_entries().len() as f32 * BM_ROW_H;
        let max = (content_h - LIST_VIEWPORT_H).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max);
    }
}

/// Height of the scrollable bookmark-list viewport (panel body) in CSS px.
pub const LIST_VIEWPORT_H: f32 = PANEL_HEIGHT - PAD - (HEADER_H + PAD + SEARCH_H + PAD);

impl Default for BookmarkPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the bookmark panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkHit {
    /// Close the panel ("×" in the header).
    Close,
    /// Focus the search box.
    FocusSearch,
    /// Select a folder filter.  `None` = the "All" row.
    SelectFolder(Option<String>),
    /// Body of a bookmark row (open on click / drag source).  Carries the id.
    Bookmark(i64),
    /// Trailing "×" delete zone of a bookmark row.  Carries the id.
    DeleteBookmark(i64),
    /// Inside the panel but no actionable target.
    Empty,
}

/// Hit-test a click at CSS-px `(x, y)` against the panel anchored with its
/// top-left corner at `(ax, ay)`.  Returns `None` when outside the panel.
pub fn hit_test(panel: &BookmarkPanel, x: f32, y: f32, ax: f32, ay: f32) -> Option<BookmarkHit> {
    if x < ax || x >= ax + PANEL_WIDTH || y < ay || y >= ay + PANEL_HEIGHT {
        return None;
    }
    let lx = x - ax;
    let ly = y - ay;

    // Header: close button is the right HEADER_H square.
    if ly < HEADER_H {
        if lx >= PANEL_WIDTH - HEADER_H {
            return Some(BookmarkHit::Close);
        }
        return Some(BookmarkHit::Empty);
    }

    // Search box.
    let search_top = HEADER_H + PAD;
    if ly >= search_top && ly < search_top + SEARCH_H {
        return Some(BookmarkHit::FocusSearch);
    }

    // Body: folder column (left) | bookmark list (right).
    let body_top = search_top + SEARCH_H + PAD;
    if ly < body_top {
        return Some(BookmarkHit::Empty);
    }

    if lx < FOLDER_COL_W {
        // Folder rows: "All" first, then each folder.
        let row = ((ly - body_top) / FOLDER_ROW_H) as usize;
        if row == 0 {
            return Some(BookmarkHit::SelectFolder(None));
        }
        let fi = row - 1;
        if fi < panel.folders.len() {
            return Some(BookmarkHit::SelectFolder(Some(panel.folders[fi].clone())));
        }
        return Some(BookmarkHit::Empty);
    }

    // Bookmark list.
    let visible = panel.visible_entries();
    let rel_y = ly - body_top + panel.scroll_y;
    let row = (rel_y / BM_ROW_H) as usize;
    if let Some(entry) = visible.get(row) {
        // Trailing delete zone (panel-local right edge is PANEL_WIDTH - PAD).
        if lx >= PANEL_WIDTH - PAD - DELETE_W {
            return Some(BookmarkHit::DeleteBookmark(entry.id));
        }
        return Some(BookmarkHit::Bookmark(entry.id));
    }
    Some(BookmarkHit::Empty)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const AX: f32 = 8.0;
    const AY: f32 = 40.0;

    fn entry(id: i64, url: &str, title: &str, folder: &str) -> BmEntry {
        BmEntry {
            id,
            url: url.to_owned(),
            title: title.to_owned(),
            folder: folder.to_owned(),
        }
    }

    fn sample() -> BookmarkPanel {
        let mut p = BookmarkPanel::new();
        p.visible = true;
        p.set_data(vec![
            entry(1, "https://rust-lang.org/", "Rust", "/Work"),
            entry(2, "https://example.com/", "Example", "/Reading"),
            entry(3, "https://docs.rs/", "Docs", "/Work"),
            entry(4, "https://root.example/", "Root", ""),
        ]);
        p
    }

    // ── State ──────────────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden() {
        assert!(!BookmarkPanel::new().visible);
    }

    #[test]
    fn toggle_resets_transient_state_on_hide() {
        let mut p = sample();
        p.search_active = true;
        p.begin_drag(1);
        p.toggle(); // now hidden
        assert!(!p.visible);
        assert!(!p.search_active);
        assert_eq!(p.drag, None);
    }

    #[test]
    fn set_data_computes_distinct_sorted_folders() {
        let p = sample();
        assert_eq!(p.folders, vec!["/Reading".to_string(), "/Work".to_string()]);
    }

    #[test]
    fn set_data_clears_stale_folder_filter() {
        let mut p = sample();
        p.selected_folder = Some("/Gone".to_string());
        p.set_data(vec![entry(1, "https://a/", "A", "/Work")]);
        assert_eq!(p.selected_folder, None);
    }

    // ── Filtering ────────────────────────────────────────────────────────────

    #[test]
    fn visible_all_folders_when_none_selected() {
        let p = sample();
        assert_eq!(p.visible_entries().len(), 4);
    }

    #[test]
    fn visible_filtered_by_folder() {
        let mut p = sample();
        p.selected_folder = Some("/Work".to_string());
        let v = p.visible_entries();
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|e| e.folder == "/Work"));
    }

    #[test]
    fn search_filters_by_title_and_url_case_insensitive() {
        let mut p = sample();
        // Case-insensitive title match: "Example" (id=2). The needle is specific
        // enough not to also match the root entry's url ("root.example").
        p.append_search("EXAMPLE.COM");
        let v = p.visible_entries();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, 2);
    }

    #[test]
    fn search_matches_url_substring() {
        let mut p = sample();
        p.append_search("docs.rs");
        let v = p.visible_entries();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, 3);
    }

    #[test]
    fn backspace_search_shortens_query() {
        let mut p = sample();
        p.append_search("rust");
        p.backspace_search();
        assert_eq!(p.search, "rus");
    }

    // ── Drag ───────────────────────────────────────────────────────────────────

    #[test]
    fn drag_begin_then_take() {
        let mut p = sample();
        p.begin_drag(3);
        assert_eq!(p.drag, Some(3));
        assert_eq!(p.take_drag(), Some(3));
        assert_eq!(p.drag, None);
        assert_eq!(p.take_drag(), None);
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_returns_none() {
        let p = sample();
        assert_eq!(hit_test(&p, AX - 1.0, AY + 10.0, AX, AY), None);
        assert_eq!(hit_test(&p, AX + 10.0, AY + PANEL_HEIGHT + 1.0, AX, AY), None);
    }

    #[test]
    fn hit_close_button() {
        let p = sample();
        let hit = hit_test(&p, AX + PANEL_WIDTH - 5.0, AY + 10.0, AX, AY);
        assert_eq!(hit, Some(BookmarkHit::Close));
    }

    #[test]
    fn hit_search_box() {
        let p = sample();
        let y = AY + HEADER_H + PAD + SEARCH_H * 0.5;
        let hit = hit_test(&p, AX + 60.0, y, AX, AY);
        assert_eq!(hit, Some(BookmarkHit::FocusSearch));
    }

    #[test]
    fn hit_all_folder_row() {
        let p = sample();
        let body_top = AY + HEADER_H + PAD + SEARCH_H + PAD;
        let y = body_top + FOLDER_ROW_H * 0.5;
        let hit = hit_test(&p, AX + 20.0, y, AX, AY);
        assert_eq!(hit, Some(BookmarkHit::SelectFolder(None)));
    }

    #[test]
    fn hit_specific_folder_row() {
        let p = sample();
        let body_top = AY + HEADER_H + PAD + SEARCH_H + PAD;
        // Row index 1 = first folder ("/Reading").
        let y = body_top + FOLDER_ROW_H * 1.5;
        let hit = hit_test(&p, AX + 20.0, y, AX, AY);
        assert_eq!(hit, Some(BookmarkHit::SelectFolder(Some("/Reading".to_string()))));
    }

    #[test]
    fn hit_bookmark_row_body() {
        let p = sample();
        let body_top = AY + HEADER_H + PAD + SEARCH_H + PAD;
        let y = body_top + BM_ROW_H * 0.5;
        // First visible entry (folder sort: root entry id=4 is in display order
        // as stored — visible_entries preserves entries order).
        let x = AX + FOLDER_COL_W + 20.0;
        let hit = hit_test(&p, x, y, AX, AY);
        let first_id = p.visible_entries()[0].id;
        assert_eq!(hit, Some(BookmarkHit::Bookmark(first_id)));
    }

    #[test]
    fn hit_bookmark_delete_zone() {
        let p = sample();
        let body_top = AY + HEADER_H + PAD + SEARCH_H + PAD;
        let y = body_top + BM_ROW_H * 0.5;
        let x = AX + PANEL_WIDTH - PAD - DELETE_W * 0.5;
        let hit = hit_test(&p, x, y, AX, AY);
        let first_id = p.visible_entries()[0].id;
        assert_eq!(hit, Some(BookmarkHit::DeleteBookmark(first_id)));
    }

}
