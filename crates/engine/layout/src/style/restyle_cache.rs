//! PERF-16 срез 4 — the restyle indexes of one stylesheet, kept from pass to pass.
//!
//! [`restyle_state_index`] and [`restyle_node_index`] each scan every selector of the sheet; the
//! chrome's interaction cycle (CC-12) built both on every pass — 0,16 мс of 0,84. They read nothing
//! of the document except whether it has an author shadow root, so an index stays valid for as long
//! as the sheet's [`StylesheetRevision`] and that flag stay the same.
//!
//! [`restyle_state_index`]: super::restyle_state_index
//! [`restyle_node_index`]: super::restyle_node_index

use std::sync::Arc;

use lumen_css_parser::{Stylesheet, StylesheetRevision};
use lumen_dom::Document;

use super::restyle::{restyle_node_index_shared, restyle_state_index_owned, NodeRestyleIndex, StateRestyleIndex};

struct Built {
    revision: StylesheetRevision,
    shadow: bool,
    state: StateRestyleIndex<'static>,
    node: NodeRestyleIndex<'static>,
}

/// The state and node restyle indexes of the last sheet seen, rebuilt when its revision, or the
/// document's shadow-root flag, changes.
#[derive(Default)]
pub struct RestyleIndexCache {
    built: Option<Built>,
    builds: u64,
}

impl RestyleIndexCache {
    /// The indexes of `sheet` in `doc`: the ones kept from the last call when nothing they read
    /// has changed, otherwise ones scanned afresh.
    pub fn indexes(
        &mut self,
        doc: &Document,
        sheet: &Stylesheet,
    ) -> (&StateRestyleIndex<'static>, &NodeRestyleIndex<'static>) {
        let shadow = doc.has_author_shadow_roots();
        let revision = sheet.revision();
        self.built.take_if(|b| b.revision != revision || b.shadow != shadow);
        let builds = &mut self.builds;
        let built = self.built.get_or_insert_with(|| {
            *builds += 1;
            // The node index keeps its sheet for the lazy reader scan, so it needs an owner that
            // outlives the caller's borrow; the copy gets a revision of its own, the key stays the
            // original's.
            let owned = Arc::new(sheet.clone());
            Built {
                revision,
                shadow,
                state: restyle_state_index_owned(doc, sheet),
                node: restyle_node_index_shared(doc, &owned),
            }
        });
        (&built.state, &built.node)
    }

    /// Scans of the sheet made so far (both indexes per scan) — for the count-based gates.
    pub fn builds(&self) -> u64 {
        self.builds
    }
}
