//! Filling the browser's own data panels from storage: workspaces, the
//! profile menu, read-later, bookmarks and history, plus the write side of
//! bookmarking whatever page is open.
//!
//! Every method here is the same shape - query `lumen-storage` for the active
//! profile, turn rows into the panel widget's item type, ask for a redraw. The
//! active profile is what makes it shell state rather than storage: an
//! anonymous profile reads and writes a different cookie jar and a different
//! set of rows, and only `Lumen` knows which one is current.

use crate::*;

impl Lumen {
    /// Reload workspace list from SQLite storage into the panel cache.
    ///
    /// Call this after every `Workspaces::create`, `rename`, or `delete` so
    /// the panel renders up-to-date chips on the next redraw.
    pub(crate) fn refresh_workspaces(&mut self) {
        let entries = self
            .workspaces
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|w| {
                let accent = panels::workspace_panel::parse_ws_color(&w.color);
                panels::workspace_panel::WsEntry {
                    id: w.id,
                    name: w.name,
                    accent,
                }
            })
            .collect();
        self.workspace_panel.set_workspaces(entries);
    }

    /// Reload the profile list from `ProfileRegistry` into the dropdown's
    /// cache (DS-14). Cheap вЂ” the registry only ever holds a handful of
    /// rows вЂ” called each time the dropdown opens so it reflects any
    /// external edit to `profiles.db` between sessions.
    pub(crate) fn refresh_profile_menu_entries(&mut self) {
        let entries: Vec<panels::profile_menu::ProfileEntry> = self
            .profiles
            .list_all()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, p)| panels::profile_menu::ProfileEntry {
                id: p.id,
                name: p.name.clone(),
                color: panels::profile_menu::color_for_profile(&p.name, i),
            })
            .collect();
        self.profile_menu.set_entries(entries);
    }

    /// `true` while the active profile is the seeded Anonymous profile
    /// (DS-16 ephemeral slice, ADR-020) вЂ” gates history writes, the
    /// history-panel banner, and which cookie jar navigation uses.
    pub(crate) fn active_profile_is_anonymous(&self) -> bool {
        self.profile_menu
            .active_entry()
            .is_some_and(|e| panels::profile_menu::is_anonymous(&e.name))
    }

    /// Cookie jar used for outgoing HTTP requests on the active tab: the
    /// shared jar for every profile except Anonymous, which gets its own
    /// ephemeral jar (DS-16) so its cookies never leak into вЂ” or persist
    /// past вЂ” any other profile's browsing.
    pub(crate) fn active_cookie_jar(&self) -> Arc<lumen_storage::CookieJar> {
        if self.active_profile_is_anonymous() {
            Arc::clone(&self.anonymous_cookie_jar)
        } else {
            Arc::clone(&self.cookie_jar)
        }
    }

    pub(crate) fn refresh_read_later(&mut self) {
        let mut entries = self
            .read_later_store
            .list_by_status(lumen_knowledge::ReadStatus::Unread, 50)
            .unwrap_or_default();
        entries.extend(
            self.read_later_store
                .list_by_status(lumen_knowledge::ReadStatus::Read, 50)
                .unwrap_or_default(),
        );
        self.read_later_panel.refresh(entries);
    }

    pub(crate) fn refresh_bookmarks(&mut self) {
        let entries = self
            .bookmarks
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|b| panels::bookmark_panel::BmEntry {
                id: b.id,
                url: b.url,
                title: b.title,
                folder: b.folder,
            })
            .collect();
        self.bookmark_panel.set_data(entries);
    }

    /// Reload the history panel data from `history_store`.
    ///
    /// When `history_panel.query` is non-empty, uses `HistoryFts::search` for
    /// full-text matching. Otherwise falls back to `History::recent(50)`.
    pub(crate) fn refresh_history(&mut self) {
        let query = self.history_panel.query.trim().to_owned();
        let items: Vec<panels::history_panel::HistoryItem> = if query.is_empty() {
            self.history_store
                .recent(50)
                .unwrap_or_default()
                .into_iter()
                .map(|e| panels::history_panel::HistoryItem {
                    id: e.id,
                    url: e.url,
                    title: e.title,
                    visit_date: e.visit_date,
                    visit_count: e.visit_count,
                })
                .collect()
        } else {
            self.history_fts
                .search(&query, 50)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(i, hit)| panels::history_panel::HistoryItem {
                    id: i as i64 + 1,
                    url: hit.url,
                    title: hit.title,
                    visit_date: 0,
                    visit_count: 1,
                })
                .collect()
        };
        self.history_panel.set_items(items);
    }

    /// Add the current page to bookmarks (Ctrl+D).
    ///
    /// No-op when the current page has no URL (e.g. blank tab). The active tab
    /// title is used when available, otherwise the URL stands in as the title.
    ///
    /// Also populates the AI summary/embedding (В§12.8, Step 6) via
    /// [`Self::ai_backend`]: with the default [`lumen_core::NullAiBackend`]
    /// `summarise`/`embed` return empty, so `set_semantic` is simply skipped вЂ”
    /// no `feature = "ai"` gate needed here.
    pub(crate) fn bookmark_current_page(&mut self) {
        let url = self.current_display_url().to_owned();
        if url.is_empty() {
            return;
        };
        let title = self
            .tab_strip
            .tabs
            .get(self.tab_strip.active)
            .map(|t| t.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| url.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = self.bookmarks.add(&url, &title, "", &[], "", now);
        let summary = self.ai_backend.summarise(&self.current_page_text());
        if !summary.is_empty() {
            let embedding = self.ai_backend.embed(&summary);
            let embedding_bytes = (!embedding.is_empty())
                .then(|| lumen_storage::bookmarks::embedding_to_bytes(&embedding));
            let _ = self
                .bookmarks
                .set_semantic(&url, Some(&summary), embedding_bytes.as_deref());
        }
        if self.bookmark_panel.visible {
            self.refresh_bookmarks();
        }
    }
}
