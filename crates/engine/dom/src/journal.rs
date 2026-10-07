//! Content journal of a [`Document`] (BUG-935 slice 55): which nodes had their
//! text, attributes, child list, form state or embedded-resource size changed
//! since the last drain. Consumed by the engine thread's same-tick flush to
//! license box reuse (`ContentDirty::Nodes`).

use std::collections::HashSet;

use crate::{Document, NodeData, NodeId};

/// Record of the nodes whose *content* (text, attributes, child list, form
/// state, embedded-resource size) changed since it was last drained — see
/// [`Document::take_content_journal`]. BUG-935 slice 55.
///
/// Off until the first drain: the parser calls [`Document::get_mut`] once per
/// node it builds, and nobody wants that logged. A clone starts untracked
/// (`Clone` yields the default), because a copy of the journal would not
/// describe what happened to the copy.
#[derive(Debug, Default)]
pub(crate) struct ContentJournal(Option<HashSet<NodeId>>);

impl Clone for ContentJournal {
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl ContentJournal {
    #[inline]
    pub(crate) fn note(&mut self, id: NodeId) {
        if let Some(set) = &mut self.0 {
            set.insert(id);
        }
    }
}

impl Document {
    /// Drain the content journal: the nodes whose text, attributes, child list,
    /// form state or embedded-resource size changed since the previous call
    /// (BUG-935 slice 55). The recording point is [`Self::get_mut`] and the
    /// tree/side-table mutators themselves, so it covers every writer — the
    /// page's JS, the shell's typing and image decoding, the parser — with no
    /// per-caller cooperation. A node that is only *read* through `get_mut`
    /// is journaled too: the record over-approximates, which only costs reuse.
    ///
    /// Returns `None` on the first call, which also starts the recording:
    /// there is no baseline to diff against, so the caller must treat that
    /// cycle as "anything may have changed". After that it returns `Some`,
    /// possibly empty — "nothing changed" — and starts the next interval.
    ///
    /// **One consumer.** The drain is destructive, so exactly one reader may
    /// use it per document — today the engine thread's same-tick flush
    /// (`lumen-js` `style_flush.rs`), which takes it under the document lock so
    /// that nothing can mutate between the drain and the layout it licenses.
    /// A `clone()` of the document starts untracked.
    pub fn take_content_journal(&mut self) -> Option<HashSet<NodeId>> {
        match &mut self.content_journal.0 {
            Some(set) => Some(std::mem::take(set)),
            none @ None => {
                *none = Some(HashSet::new());
                None
            }
        }
    }

    /// Whether any node of `journal` is part of — or borders — a shadow tree,
    /// i.e. whether a flat-tree consumer may not trust the journal alone.
    ///
    /// A light-DOM child assigned to a `<slot>` is rendered under the slot, and
    /// removing it dirties only its old DOM parent (the host), not the slot the
    /// box tree actually holds it under; the spine walk that turns the journal
    /// into "what must be rebuilt" climbs the *flat* tree from the journaled
    /// node and so misses that slot. Rather than model slot assignment, report
    /// a shadow host, a node inside a shadow tree, or a `<slot>` itself, and let
    /// the caller fall back to "anything may have changed".
    pub fn journal_touches_shadow(&self, journal: &HashSet<NodeId>) -> bool {
        self.journal_shadow_touch(journal).is_some()
    }

    /// The first node of `journal` that [`Self::journal_touches_shadow`] objects to.
    ///
    /// BUG-935 slice 97: a host whose UA shadow tree has no `<slot>` (`<video>`/`<audio>`) does
    /// not object. Nothing in its light tree is ever part of the flat tree, so there is no slot
    /// the box tree could hold a child under that the journal would miss; the host itself is
    /// journaled like any element. Before this every flush that touched a `<video>` (a player
    /// script rewriting its attributes) lost the whole content record and re-collected every
    /// box of the page.
    pub fn journal_shadow_touch(&self, journal: &HashSet<NodeId>) -> Option<NodeId> {
        journal.iter().copied().find(|&id| {
            let host_with_slots = match self.shadow_roots.get(&id) {
                Some(sr) => !self.ua_shadow_roots.contains_key(sr) || !self.nodes[sr.index()].children.is_empty(),
                None => false,
            };
            if host_with_slots || self.ua_shadow_roots.contains_key(&id) {
                return true;
            }
            let Some(mut cur) = self.nodes.get(id.index()) else {
                return false;
            };
            loop {
                let is_slot = matches!(&cur.data, NodeData::Element { name, .. } if name.local == "slot");
                if is_slot || matches!(cur.data, NodeData::ShadowRoot { .. }) {
                    return true;
                }
                match cur.parent.and_then(|p| self.nodes.get(p.index())) {
                    Some(parent) => cur = parent,
                    None => return false,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QualName, ShadowRootMode};

    fn journal_doc() -> (Document, NodeId, NodeId, NodeId) {
        let mut doc = Document::new();
        let root = doc.root();
        let parent = doc.create_element(QualName::html("div"));
        let a = doc.create_element(QualName::html("p"));
        let b = doc.create_element(QualName::html("p"));
        doc.append_child(root, parent);
        doc.append_child(parent, a);
        doc.append_child(parent, b);
        (doc, parent, a, b)
    }

    #[test]
    fn content_journal_first_drain_has_no_baseline_then_tracks() {
        let (mut doc, _, a, _) = journal_doc();
        assert!(doc.take_content_journal().is_none(), "first drain starts the record, no baseline yet");
        assert_eq!(doc.take_content_journal(), Some(HashSet::new()), "nothing happened since");
        doc.get_mut(a);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([a])));
        assert_eq!(doc.take_content_journal(), Some(HashSet::new()), "a drain empties the record");
    }

    #[test]
    fn content_journal_records_tree_mutations_on_both_ends() {
        let (mut doc, parent, a, b) = journal_doc();
        let c = doc.create_element(QualName::html("p"));
        doc.take_content_journal();

        doc.append_child(parent, c);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([parent, c])));

        doc.detach(a);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([parent, a])));

        doc.insert_before(a, b);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([parent, a])));

        doc.insert_after(b, a);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([parent, a])));
    }

    #[test]
    fn content_journal_records_form_state_and_embedded_size() {
        let (mut doc, _, a, b) = journal_doc();
        doc.take_content_journal();
        doc.set_control_value(a, "x");
        doc.set_control_checked(b, true);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([a, b])));
        doc.clear_control_value(a);
        doc.clear_control_checked(b);
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([a, b])));
        assert!(doc.set_embedded_image(a, "u", 1, 2));
        assert!(!doc.set_embedded_image(a, "u", 1, 2), "an unchanged entry is not a mutation");
        assert_eq!(doc.take_content_journal(), Some(HashSet::from([a])));
    }

    #[test]
    fn content_journal_is_not_inherited_by_a_clone() {
        let (mut doc, _, a, _) = journal_doc();
        doc.take_content_journal();
        doc.get_mut(a);
        let mut copy = doc.clone();
        assert!(copy.take_content_journal().is_none(), "a copy has no baseline of its own");
    }

    #[test]
    fn journal_touches_shadow_flags_hosts_slots_and_shadow_contents_only() {
        let (mut doc, parent, a, b) = journal_doc();
        let plain = HashSet::from([parent, a, b]);
        assert!(!doc.journal_touches_shadow(&plain));

        let slot = doc.create_element(QualName::html("slot"));
        doc.append_child(b, slot);
        assert!(doc.journal_touches_shadow(&HashSet::from([slot])));
        assert!(doc.journal_touches_shadow(&HashSet::from([b, slot])));
        doc.detach(slot);

        let sr = doc.attach_shadow(a, ShadowRootMode::Open);
        let inner = doc.create_element(QualName::html("span"));
        doc.append_child(sr, inner);
        assert!(doc.journal_touches_shadow(&HashSet::from([a])), "a shadow host");
        assert!(doc.journal_touches_shadow(&HashSet::from([inner])), "a node inside a shadow tree");
        assert!(!doc.journal_touches_shadow(&HashSet::from([parent, b])), "siblings of a host are plain");
    }

    #[test]
    fn journal_touches_shadow_ignores_a_slotless_ua_host_but_not_one_with_slots() {
        let mut doc = Document::new();
        let root = doc.root();
        let video = doc.create_element(QualName::html("video"));
        let audio = doc.create_element(QualName::html("audio"));
        let details = doc.create_element(QualName::html("details"));
        let select = doc.create_element(QualName::html("select"));
        for host in [video, audio, details, select] {
            doc.append_child(root, host);
        }
        let child = doc.create_element(QualName::html("source"));
        doc.append_child(video, child);
        assert!(!doc.journal_touches_shadow(&HashSet::from([video, audio, child])), "no slot, nothing slotted");
        assert_eq!(doc.journal_shadow_touch(&HashSet::from([video, details])), Some(details));
        assert!(doc.journal_touches_shadow(&HashSet::from([select])), "a slotted host");
    }
}
