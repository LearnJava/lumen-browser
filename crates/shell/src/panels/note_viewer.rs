//! Note viewer overlay panel (§12.2, GG-2).
//!
//! Floating overlay that shows a single annotation (selection + comment + source
//! URL) fetched from `lumen_knowledge::Notes` after the user picks an
//! `@notes`-search result from the omnibox dropdown and presses Enter.
//!
//! Rendered by `#noteOverlay` in the engine chrome (`bind_note`); this module
//! only keeps the state. `Escape` and the modal's close button
//! (`data-action="close-note"`) hide it. Opened via the `note-viewer:<note_id>`
//! URL scheme processed in `handle_omnibox_commit`.

use lumen_chrome::ChromeNoteModel;

/// Floating overlay for displaying a single user annotation.
///
/// Opened by `handle_omnibox_commit` when the committed value is
/// `note-viewer:<id>`. Dismissed by `Escape` or clicking [×].
pub struct NoteViewerPanel {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Database id of the displayed note.
    pub note_id: i64,
    /// Source URL the note was created on.
    pub url: String,
    /// The highlighted text selection from the source page.
    pub selection: String,
    /// Optional user comment attached to the note.
    pub comment: String,
}

impl NoteViewerPanel {
    /// Create a hidden panel with empty state.
    pub fn new() -> Self {
        Self {
            visible: false,
            note_id: 0,
            url: String::new(),
            selection: String::new(),
            comment: String::new(),
        }
    }

    /// Show the panel populated with the given note data.
    pub fn open(&mut self, note_id: i64, url: &str, selection: &str, comment: &str) {
        self.visible = true;
        self.note_id = note_id;
        self.url = url.to_owned();
        self.selection = selection.to_owned();
        self.comment = comment.to_owned();
    }

    /// Hide the panel (data is preserved for re-open).
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Snapshot for `#noteOverlay`.
    pub fn chrome_model(&self) -> ChromeNoteModel {
        ChromeNoteModel {
            open: self.visible,
            url: self.url.clone(),
            selection: self.selection.clone(),
            comment: self.comment.clone(),
        }
    }
}

impl Default for NoteViewerPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel() -> NoteViewerPanel {
        let mut p = NoteViewerPanel::new();
        p.open(7, "https://example.com/a", "quoted text", "my comment");
        p
    }

    #[test]
    fn new_is_hidden() {
        assert!(!NoteViewerPanel::new().visible);
    }

    #[test]
    fn open_sets_fields_and_visible() {
        let p = make_panel();
        assert!(p.visible);
        assert_eq!(p.note_id, 7);
        assert_eq!(p.url, "https://example.com/a");
        assert_eq!(p.selection, "quoted text");
        assert_eq!(p.comment, "my comment");
    }

    #[test]
    fn close_hides_panel() {
        let mut p = make_panel();
        p.close();
        assert!(!p.visible);
        assert!(!p.chrome_model().open);
    }

    #[test]
    fn chrome_model_mirrors_fields() {
        let m = make_panel().chrome_model();
        assert!(m.open);
        assert_eq!(m.url, "https://example.com/a");
        assert_eq!(m.selection, "quoted text");
        assert_eq!(m.comment, "my comment");
    }
}
