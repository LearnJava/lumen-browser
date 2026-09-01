//! The internal `about:newtab` start page, shell side: building its HTML and
//! applying the actions its tiles link to.
//!
//! The page itself - tile merging, the HTML template and the `about:newtab?...`
//! link grammar - lives in `crate::newtab`; what is here is the shell side that
//! reads the pinned tiles and the history store, and that turns a clicked tile
//! action into a store write plus a reload.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour, order
//! of operations and method bodies are unchanged; only the module path and
//! visibility (`fn` -> `pub(crate) fn`, required for callers in other
//! modules) differ.

use crate::*;

impl Lumen {
    /// Build a fresh `about:newtab` [`PageSource::Static`] from pinned tiles
    /// (`newtab_tiles`) plus a top-sites filler from `history_store`
    /// (DS-11). Pinned tiles always come first, in their stored order; an
    /// empty/failed read of either store just yields fewer tiles.
    pub(crate) fn build_newtab_source(&self) -> PageSource {
        let pinned: Vec<newtab::TopSite> = self
            .newtab_tiles
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|t| newtab::TopSite { url: t.url, title: t.title, pinned: true })
            .collect();
        let top_sites: Vec<newtab::TopSite> = self
            .history_store
            .most_visited(newtab::MAX_TILES as i64)
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let title = if e.title.trim().is_empty() {
                    e.url.clone()
                } else {
                    e.title
                };
                newtab::TopSite { url: e.url, title, pinned: false }
            })
            .collect();
        let sites = newtab::merge_tiles(&pinned, &top_sites);
        PageSource::Static {
            html: newtab::build_newtab_html(&sites),
            url: newtab::NEWTAB_URL.to_owned(),
        }
    }

    /// Apply a [`newtab::NewtabAction`] parsed from a clicked `about:newtab?...`
    /// link (pin/unpin toggle, the "+" tile, or "Restore closed"), then reload
    /// the newtab page with the updated tile set.
    ///
    /// `RestoreClosed` reuses the cross-restart session-restore mechanism
    /// (`restore_session`, backed by `session_store`) — Lumen has no separate
    /// per-tab "closed tabs" stack, so this reopens the last persisted session
    /// snapshot wholesale instead of undoing a single tab close.
    pub(crate) fn apply_newtab_action(&mut self, action: newtab::NewtabAction) {
        match action {
            newtab::NewtabAction::Pin { url, title } => {
                let _ = self.newtab_tiles.pin(&url, &title);
            }
            newtab::NewtabAction::Unpin { url } => {
                let _ = self.newtab_tiles.unpin(&url);
            }
            newtab::NewtabAction::PinCurrent => {
                if let Some(prev) = self.nav_back.last()
                    && let Some(url) = prev.source.url_str()
                {
                    let title = self
                        .history_store
                        .get(url)
                        .ok()
                        .flatten()
                        .map(|e| e.title)
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or_else(|| url.to_owned());
                    let _ = self.newtab_tiles.pin(url, &title);
                }
            }
            newtab::NewtabAction::RestoreClosed => {
                self.restore_session();
                self.request_redraw();
                return;
            }
        }
        self.navigate_to(self.build_newtab_source());
    }
}
