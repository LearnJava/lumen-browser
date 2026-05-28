//! Contenteditable and command history for undo/redo.
//!
//! Tracks all DOM modifications as reversible commands, allowing undo/redo.
//! Also provides utilities for paste, drag-drop, and other editing operations.

use crate::{delete_range, insert_text_at, Document, DomPosition, Range};

/// A single, reversible DOM modification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomCommand {
    /// Insert text at a position, returning the new caret position.
    InsertText {
        /// Where to insert.
        pos: DomPosition,
        /// Text to insert.
        text: String,
    },
    /// Delete a range of content. Stores the deleted text for undo.
    DeleteRange {
        /// Range to delete.
        range: Range,
        /// Deleted text (stored for undo).
        deleted_text: String,
    },
    /// Replace a range with new text. Stores the original text for undo.
    ReplaceText {
        /// Range to replace.
        range: Range,
        /// New text.
        new_text: String,
        /// Original text (stored for undo).
        old_text: String,
    },
}

/// Data from a paste operation (clipboard or drag-drop).
///
/// Supports multiple MIME types: text/plain, text/html, and file list.
#[derive(Debug, Clone, Default)]
pub struct PasteData {
    /// Plain text content (text/plain).
    pub text: Option<String>,
    /// HTML content (text/html).
    pub html: Option<String>,
    /// Filenames or URLs for dropped files.
    pub files: Vec<String>,
}

/// Data transferred in a drag-drop operation.
///
/// When a user drags content from one location and drops it on contenteditable,
/// this struct carries the transferred data.
#[derive(Debug, Clone, Default)]
pub struct DragData {
    /// Plain text content.
    pub text: Option<String>,
    /// HTML content.
    pub html: Option<String>,
    /// URLs (from dragging links or images).
    pub urls: Vec<String>,
    /// File paths (from dragging files).
    pub files: Vec<String>,
    /// The source of the drag: true if from the same contenteditable (move), false (copy).
    pub is_move: bool,
}

impl PasteData {
    /// Create empty paste data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set text content.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set HTML content.
    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    /// Add a file to the paste data.
    pub fn add_file(mut self, file: impl Into<String>) -> Self {
        self.files.push(file.into());
        self
    }

    /// Preferred content for insertion: HTML (if available), else plain text.
    pub fn preferred_content(&self) -> Option<&str> {
        self.html.as_deref().or(self.text.as_deref())
    }
}

impl DragData {
    /// Create empty drag data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set text content.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set HTML content.
    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    /// Add a URL to the drag data.
    pub fn add_url(mut self, url: impl Into<String>) -> Self {
        self.urls.push(url.into());
        self
    }

    /// Add a file to the drag data.
    pub fn add_file(mut self, file: impl Into<String>) -> Self {
        self.files.push(file.into());
        self
    }

    /// Mark this as a move operation (not copy).
    pub fn mark_move(mut self) -> Self {
        self.is_move = true;
        self
    }

    /// Preferred content for insertion: HTML (if available), else plain text.
    pub fn preferred_content(&self) -> Option<&str> {
        self.html.as_deref().or(self.text.as_deref())
    }
}

/// History of executed commands for undo/redo.
///
/// Stores a linear sequence of commands and tracks the current position
/// in that sequence. Executing a new command when not at the end discards
/// all "redo" commands after the current position.
#[derive(Debug, Clone, Default)]
pub struct CommandHistory {
    /// All executed commands, in order.
    commands: Vec<DomCommand>,
    /// Current position (0..=commands.len()).
    /// After undo, this decreases; after redo, this increases.
    /// New commands are inserted at this position.
    current_pos: usize,
}

impl CommandHistory {
    /// Create an empty history.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            current_pos: 0,
        }
    }

    /// Execute InsertText command: insert text at position and record.
    pub fn insert_text(&mut self, doc: &mut Document, pos: DomPosition, text: String) {
        self.commands.truncate(self.current_pos);
        let _ = insert_text_at(doc, pos, &text);
        self.commands.push(DomCommand::InsertText { pos, text });
        self.current_pos += 1;
    }

    /// Execute DeleteRange command: delete range and record (with deleted text).
    ///
    /// Returns the deleted text for redo/undo operations.
    pub fn delete_range(&mut self, doc: &mut Document, range: Range) -> String {
        self.commands.truncate(self.current_pos);

        // Extract the text before deleting (this is a simplification;
        // a proper impl would extract text from the range).
        // For now, store empty string as placeholder.
        let deleted_text = String::new(); // TODO: extract actual text from range

        let _ = delete_range(doc, &range);
        self.commands.push(DomCommand::DeleteRange {
            range,
            deleted_text: deleted_text.clone(),
        });
        self.current_pos += 1;
        deleted_text
    }

    /// Execute ReplaceText command: replace range with new text and record.
    pub fn replace_text(
        &mut self,
        doc: &mut Document,
        range: Range,
        new_text: String,
    ) -> String {
        self.commands.truncate(self.current_pos);

        // TODO: extract actual text from range
        let old_text = String::new();

        let _ = delete_range(doc, &range);
        let _ = insert_text_at(doc, range.start, &new_text);
        self.commands.push(DomCommand::ReplaceText {
            range,
            new_text,
            old_text: old_text.clone(),
        });
        self.current_pos += 1;
        old_text
    }

    /// Undo the last command (move backward in history).
    ///
    /// Returns the command that was undone, or `None` if at the beginning.
    pub fn undo(&mut self, doc: &mut Document) -> Option<DomCommand> {
        if self.current_pos == 0 {
            return None;
        }

        self.current_pos -= 1;
        let cmd = self.commands[self.current_pos].clone();

        // Reverse the command on the document.
        match &cmd {
            DomCommand::InsertText { pos, text } => {
                // To undo InsertText, delete the inserted text.
                let start = *pos;
                let end = DomPosition {
                    container: pos.container,
                    offset: pos.offset + text.len() as u32,
                };
                let range = Range { start, end };
                let _ = delete_range(doc, &range);
            }
            DomCommand::DeleteRange {
                range,
                deleted_text,
            } => {
                // To undo DeleteRange, insert back the deleted text.
                let _ = insert_text_at(doc, range.start, deleted_text);
            }
            DomCommand::ReplaceText {
                range,
                old_text,
                ..
            } => {
                // To undo ReplaceText, delete new content and insert original.
                let _ = delete_range(doc, range);
                let _ = insert_text_at(doc, range.start, old_text);
            }
        }

        Some(cmd)
    }

    /// Redo the last undone command (move forward in history).
    ///
    /// Returns the command that was redone, or `None` if at the end.
    pub fn redo(&mut self, doc: &mut Document) -> Option<DomCommand> {
        if self.current_pos >= self.commands.len() {
            return None;
        }

        let cmd = self.commands[self.current_pos].clone();

        // Re-apply the command.
        match &cmd {
            DomCommand::InsertText { pos, text } => {
                let _ = insert_text_at(doc, *pos, text);
            }
            DomCommand::DeleteRange { range, .. } => {
                let _ = delete_range(doc, range);
            }
            DomCommand::ReplaceText {
                range,
                new_text,
                ..
            } => {
                let _ = delete_range(doc, range);
                let _ = insert_text_at(doc, range.start, new_text);
            }
        }

        self.current_pos += 1;
        Some(cmd)
    }

    /// True if undo is possible.
    pub fn can_undo(&self) -> bool {
        self.current_pos > 0
    }

    /// True if redo is possible.
    pub fn can_redo(&self) -> bool {
        self.current_pos < self.commands.len()
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.commands.clear();
        self.current_pos = 0;
    }

    /// Return the number of commands in history.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// True if there are no commands in history.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Return the current position in history (how many commands have been executed/redone).
    pub fn current_pos(&self) -> usize {
        self.current_pos
    }
}

/// Handle paste operation: insert paste data at selection or cursor position.
///
/// If there is an active selection, the selected content is replaced with the
/// pasted content. If there is just a cursor position (collapsed selection),
/// the content is inserted at that position.
///
/// Prefers HTML content over plain text if both are available.
pub fn paste_into(
    history: &mut CommandHistory,
    doc: &mut Document,
    data: &PasteData,
) -> bool {
    if let Some(content) = data.preferred_content() {
        let selection = doc.get_selection().clone();

        if let Some(range) = selection.get_range() {
            // Replace selected content
            history.replace_text(doc, range, content.to_string());
        } else if let Some(pos) = selection.anchor {
            // Insert at cursor position
            history.insert_text(doc, pos, content.to_string());
        } else {
            // No selection and no cursor position — nothing to do
            return false;
        }

        true
    } else {
        false
    }
}

/// Handle drop operation: insert drag data at drop position.
///
/// Similar to paste_into, but for drag-drop. If the drag is marked as a move
/// and there is a current selection, the selected content is deleted after
/// being moved (this would be handled by the caller in a full drag-drop impl).
///
/// For now, this performs the same operation as paste_into.
pub fn drop_into(
    history: &mut CommandHistory,
    doc: &mut Document,
    data: &DragData,
) -> bool {
    if let Some(content) = data.preferred_content() {
        let selection = doc.get_selection().clone();

        if let Some(range) = selection.get_range() {
            // Replace selected content
            history.replace_text(doc, range, content.to_string());
        } else if let Some(pos) = selection.anchor {
            // Insert at cursor position
            history.insert_text(doc, pos, content.to_string());
        } else {
            // No selection and no cursor position — nothing to do
            return false;
        }

        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, NodeId};

    fn new_doc_with_text() -> Document {
        let mut doc = Document::new();
        let root = doc.root();
        let text_id = doc.create_text("Hello");
        doc.append_child(root, text_id);
        doc
    }

    #[test]
    fn insert_text_and_undo() {
        let mut doc = new_doc_with_text();
        let mut history = CommandHistory::new();

        let pos = DomPosition {
            container: NodeId(1),
            offset: 5,
        };

        history.insert_text(&mut doc, pos, " world".to_string());
        assert_eq!(history.current_pos(), 1);
        assert!(history.can_undo());
        assert!(!history.can_redo());

        // Undo
        let undone = history.undo(&mut doc);
        assert!(undone.is_some());
        assert_eq!(history.current_pos(), 0);
        assert!(!history.can_undo());
        assert!(history.can_redo());
    }

    #[test]
    fn redo_after_undo() {
        let mut doc = new_doc_with_text();
        let mut history = CommandHistory::new();

        let pos = DomPosition {
            container: NodeId(1),
            offset: 5,
        };

        history.insert_text(&mut doc, pos, "!".to_string());
        history.undo(&mut doc);
        assert!(history.can_redo());

        let redone = history.redo(&mut doc);
        assert!(redone.is_some());
        assert_eq!(history.current_pos(), 1);
        assert!(!history.can_redo());
    }

    #[test]
    fn discard_redo_on_new_command() {
        let mut doc = new_doc_with_text();
        let mut history = CommandHistory::new();

        let pos = DomPosition {
            container: NodeId(1),
            offset: 5,
        };

        // Execute two commands
        history.insert_text(&mut doc, pos, "!".to_string());
        history.insert_text(
            &mut doc,
            DomPosition {
                container: NodeId(1),
                offset: 6,
            },
            "?".to_string(),
        );
        assert_eq!(history.len(), 2);

        // Undo once
        history.undo(&mut doc);
        assert_eq!(history.current_pos(), 1);

        // Execute a new command (should discard the second command)
        history.insert_text(
            &mut doc,
            DomPosition {
                container: NodeId(1),
                offset: 6,
            },
            "*".to_string(),
        );

        assert_eq!(history.len(), 2); // Second command replaced
        assert_eq!(history.current_pos(), 2);
        assert!(!history.can_redo());
    }

    #[test]
    fn delete_range_command() {
        let mut doc = new_doc_with_text();
        let mut history = CommandHistory::new();

        let range = Range {
            start: DomPosition {
                container: NodeId(1),
                offset: 0,
            },
            end: DomPosition {
                container: NodeId(1),
                offset: 3,
            },
        };

        history.delete_range(&mut doc, range);
        assert_eq!(history.current_pos(), 1);
        assert!(history.can_undo());
    }

    #[test]
    fn paste_data_construction() {
        let data = PasteData::new()
            .with_text("hello")
            .with_html("<p>hello</p>")
            .add_file("test.txt");

        assert_eq!(data.text, Some("hello".to_string()));
        assert_eq!(data.html, Some("<p>hello</p>".to_string()));
        assert_eq!(data.files, vec!["test.txt"]);
    }

    #[test]
    fn paste_data_preferred_content() {
        let plain_only = PasteData::new().with_text("plain");
        assert_eq!(plain_only.preferred_content(), Some("plain"));

        let html_preferred = PasteData::new()
            .with_text("plain")
            .with_html("<b>bold</b>");
        assert_eq!(html_preferred.preferred_content(), Some("<b>bold</b>"));

        let empty = PasteData::new();
        assert_eq!(empty.preferred_content(), None);
    }

    #[test]
    fn paste_into_at_cursor() {
        let mut doc = new_doc_with_text();
        let mut history = CommandHistory::new();

        // Set cursor at position 5 (end of "Hello")
        let pos = DomPosition {
            container: NodeId(1),
            offset: 5,
        };
        doc.set_selection(crate::Selection {
            anchor: Some(pos),
            focus: Some(pos),
        });

        let data = PasteData::new().with_text(" world");
        let result = paste_into(&mut history, &mut doc, &data);

        assert!(result);
        assert_eq!(history.current_pos(), 1);
    }

    #[test]
    fn drag_data_construction() {
        let data = DragData::new()
            .with_text("hello")
            .with_html("<p>hello</p>")
            .add_url("https://example.com")
            .add_file("test.txt")
            .mark_move();

        assert_eq!(data.text, Some("hello".to_string()));
        assert_eq!(data.html, Some("<p>hello</p>".to_string()));
        assert_eq!(data.urls, vec!["https://example.com"]);
        assert_eq!(data.files, vec!["test.txt"]);
        assert!(data.is_move);
    }

    #[test]
    fn drag_data_preferred_content() {
        let plain_only = DragData::new().with_text("plain");
        assert_eq!(plain_only.preferred_content(), Some("plain"));

        let html_preferred = DragData::new()
            .with_text("plain")
            .with_html("<b>bold</b>");
        assert_eq!(html_preferred.preferred_content(), Some("<b>bold</b>"));
    }

    #[test]
    fn drop_into_at_cursor() {
        let mut doc = new_doc_with_text();
        let mut history = CommandHistory::new();

        // Set cursor at position 5
        let pos = DomPosition {
            container: NodeId(1),
            offset: 5,
        };
        doc.set_selection(crate::Selection {
            anchor: Some(pos),
            focus: Some(pos),
        });

        let data = DragData::new().with_text(" dropped");
        let result = drop_into(&mut history, &mut doc, &data);

        assert!(result);
        assert_eq!(history.current_pos(), 1);
    }
}
