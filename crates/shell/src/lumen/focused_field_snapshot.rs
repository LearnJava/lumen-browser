//! BUG-1108: what the redraw path knows about the focused form control,
//! read without ever blocking on the document mutex.
//!
//! The engine thread holds `Document` for a whole off-thread relayout —
//! 11-16 s on github.com (BUG-683 срез 7). A frame that paints the focused
//! field's caret, selection or spelling squiggles used to `lock()` the
//! document for that, so with an `<input>` focused the window froze for the
//! whole cascade. The frame now refreshes this snapshot with `try_lock` and
//! paints from it; while the lock is contended the previous frame's reading
//! for the same document and node stands in, and the relayout's own commit
//! repaints once it is released. Same shape as `Lumen::modal_dialog_cache`.
//!
//! Keyboard input keeps the blocking [`Lumen::typeable_field`]: a typed
//! character has to land in the current DOM, so waiting there is correct.

use std::sync::{Arc, Mutex};

use crate::*;

use super::spell_menu::spell_target_in;
use super::text_input::typeable_field_in;

/// Spell-check target of the focused node — see [`Lumen::spell_target`].
pub(crate) type SpellTarget = (NodeId, String, page_context_menu::SpellTargetKind);

/// Last document reading about the focused node, taken by the redraw path.
#[derive(Debug, Default)]
pub(crate) struct FocusedFieldSnapshot {
    /// `Arc` address of the document it was read from — a navigation or tab
    /// switch changes it, so a reading never leaks into another document.
    doc_key: usize,
    /// The focused node it describes; `None` = empty snapshot.
    nid: Option<NodeId>,
    /// [`Lumen::typeable_field`] of `nid`.
    field: Option<(TypeableField, String)>,
    /// [`Lumen::spell_target`] of `nid`.
    spell: Option<SpellTarget>,
}

impl FocusedFieldSnapshot {
    /// Re-read `nid` from `document` if its lock is free right now. While it
    /// is held elsewhere the previous reading is kept when it describes the
    /// same document and node, and dropped otherwise — a stale value is
    /// better than a frozen window, a value from another field is not.
    pub(crate) fn refresh(&mut self, document: &Arc<Mutex<Document>>, nid: Option<NodeId>) {
        let key = Arc::as_ptr(document) as usize;
        let Some(nid) = nid else {
            *self = Self::default();
            return;
        };
        if let Ok(doc) = document.try_lock() {
            *self = Self {
                doc_key: key,
                nid: Some(nid),
                field: typeable_field_in(&doc, nid),
                spell: spell_target_in(&doc, nid),
            };
        } else if self.doc_key != key || self.nid != Some(nid) {
            *self = Self::default();
        }
    }

    /// Forget everything — no document to read from.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The typeable-field reading for `nid`, if the snapshot describes it.
    pub(crate) fn field(&self, nid: NodeId) -> Option<(TypeableField, String)> {
        self.field.clone().filter(|_| self.nid == Some(nid))
    }

    /// The spell-check target for `nid`, if the snapshot describes it.
    pub(crate) fn spell(&self, nid: NodeId) -> Option<SpellTarget> {
        self.spell.clone().filter(|_| self.nid == Some(nid))
    }
}

impl Lumen {
    /// Refresh [`Self::focused_field_snapshot`] for this frame — call once
    /// per redraw before any `focused_*` paint query reads it.
    pub(crate) fn refresh_focused_field_snapshot(&mut self) {
        match self.layout_source.as_ref() {
            Some(src) => self.focused_field_snapshot.refresh(&src.document, self.focused_node),
            None => self.focused_field_snapshot.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_input() -> (Arc<Mutex<Document>>, NodeId) {
        let doc = lumen_html_parser::parse(r#"<body><input id="q" value="abc"></body>"#);
        let nid = doc.find_by_id("q").unwrap_or_else(|| unreachable!());
        (Arc::new(Mutex::new(doc)), nid)
    }

    #[test]
    fn free_lock_reads_the_field() {
        let (doc, nid) = doc_with_input();
        let mut snap = FocusedFieldSnapshot::default();
        snap.refresh(&doc, Some(nid));
        assert_eq!(snap.field(nid), Some((TypeableField::Input, "abc".to_owned())));
        assert!(snap.spell(nid).is_some());
    }

    /// The defect itself: with the document held by another thread (the
    /// engine's off-thread relayout), `refresh` must return at once and keep
    /// painting the last reading — before the fix this call blocked until
    /// the holder let go.
    #[test]
    fn contended_lock_returns_immediately_with_previous_reading() {
        let (doc, nid) = doc_with_input();
        let mut snap = FocusedFieldSnapshot::default();
        snap.refresh(&doc, Some(nid));

        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let holder_doc = Arc::clone(&doc);
        let holder = std::thread::spawn(move || {
            let _guard = holder_doc.lock();
            let _ = locked_tx.send(());
            let _ = release_rx.recv();
        });
        let _ = locked_rx.recv();

        snap.refresh(&doc, Some(nid));
        assert_eq!(snap.field(nid), Some((TypeableField::Input, "abc".to_owned())));

        // A different focused node has no reading to fall back on.
        let other = NodeId::from_index(nid.index() + 1000);
        snap.refresh(&doc, Some(other));
        assert_eq!(snap.field(other), None);
        assert_eq!(snap.field(nid), None);

        let _ = release_tx.send(());
        let _ = holder.join();
    }

    #[test]
    fn another_document_does_not_inherit_the_reading() {
        let (doc, nid) = doc_with_input();
        let mut snap = FocusedFieldSnapshot::default();
        snap.refresh(&doc, Some(nid));

        let (other_doc, _) = doc_with_input();
        let _guard = other_doc.lock();
        snap.refresh(&other_doc, Some(nid));
        assert_eq!(snap.field(nid), None);
    }

    #[test]
    fn no_focus_clears() {
        let (doc, nid) = doc_with_input();
        let mut snap = FocusedFieldSnapshot::default();
        snap.refresh(&doc, Some(nid));
        snap.refresh(&doc, None);
        assert_eq!(snap.field(nid), None);
        assert_eq!(snap.spell(nid), None);
    }
}
