//! Contenteditable and command history for undo/redo.
//!
//! Tracks all DOM modifications as reversible commands, allowing undo/redo.

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
}
