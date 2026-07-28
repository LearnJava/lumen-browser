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
//! **Drag-and-drop re-file** and the folder/delete click targets were removed
//! in CC-15-6 together with the legacy overlay's mouse handling — the engine
//! chrome's `#view-bookmarks` wires none of them yet (see BUG-415). What is
//! left here is state the chrome model reads: the entry list, folder set,
//! active filter and search query.

// ── Visual constants ─────────────────────────────────────────────────────────

/// Total panel height in CSS px.
pub const PANEL_HEIGHT: f32 = 380.0;

/// Header strip height (title + close button).
const HEADER_H: f32 = 30.0;

/// Search box height.
const SEARCH_H: f32 = 26.0;

/// Height of a single bookmark row (title line + url line).
const BM_ROW_H: f32 = 38.0;

/// Outer padding inside the panel.
const PAD: f32 = 8.0;

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
        }
    }

    /// Flip visibility.  Resets the search focus when hiding.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.search_active = false;
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        p.toggle(); // now hidden
        assert!(!p.visible);
        assert!(!p.search_active);
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
}
