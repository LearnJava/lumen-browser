//! Конкретная реализация [`KnowledgeStore`] поверх SQLite FTS5 (§12.1–12.4).
//!
//! [`DefaultKnowledgeStore`] агрегирует все четыре подсистемы:
//! - [`crate::fts::HistoryFts`] — FTS5-индекс посещённых страниц (§12.1)
//! - [`crate::notes::Notes`] — заметки с FTS (§12.2)
//! - [`crate::read_later::ReadLater`] — read-later с FTS (§12.3)
//! - [`crate::open_tabs::OpenTabsIndex`] — live in-memory индекс вкладок (§12.4)
//!
//! Все подсистемы доступны через единый трейт [`lumen_core::ext::KnowledgeStore`].
//! Shell и omnibox зависят только от трейта, а не от конкретного типа — это
//! позволяет заменить SQLite на tantivy в Phase 3+ без смены потребителей.

use std::path::Path;

use lumen_core::ext::{
    KnowledgeHistoryHit, KnowledgeNoteHit, KnowledgeReadLaterHit, KnowledgeStore,
    KnowledgeTabHit,
};
use lumen_core::Result;

use crate::fts::HistoryFts;
use crate::notes::Notes;
use crate::open_tabs::OpenTabsIndex;
use crate::read_later::ReadLater;

/// SQLite-backed [`KnowledgeStore`]. One instance per browser process.
///
/// Opens four separate SQLite connections:
/// - `history_fts` at `<base>/history_fts.db` (§12.1)
/// - `notes` at `<base>/notes.db` (§12.2)
/// - `read_later` at `<base>/read_later.db` (§12.3)
/// - `open_tabs` — in-memory only (§12.4)
pub struct DefaultKnowledgeStore {
    history: HistoryFts,
    notes: Notes,
    read_later: ReadLater,
    tabs: OpenTabsIndex,
}

impl std::fmt::Debug for DefaultKnowledgeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultKnowledgeStore").finish()
    }
}

impl DefaultKnowledgeStore {
    /// Open (or create) a `DefaultKnowledgeStore` in `base_dir`.
    ///
    /// Creates `<base_dir>/history_fts.db`, `<base_dir>/notes.db`, and
    /// `<base_dir>/read_later.db` on first use. The tabs index is always
    /// in-memory (no disk file). `base_dir` must exist.
    pub fn open(base_dir: impl AsRef<Path>) -> Result<Self> {
        let base = base_dir.as_ref();
        Ok(Self {
            history: HistoryFts::open(base.join("history_fts.db"))?,
            notes: Notes::open(base.join("notes.db"))?,
            read_later: ReadLater::open(base.join("read_later.db"))?,
            tabs: OpenTabsIndex::new()?,
        })
    }

    /// Create an in-memory `DefaultKnowledgeStore` (tests only).
    ///
    /// All four sub-stores use `open_in_memory`; data is lost on drop.
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            history: HistoryFts::open_in_memory()?,
            notes: Notes::open_in_memory()?,
            read_later: ReadLater::open_in_memory()?,
            tabs: OpenTabsIndex::new()?,
        })
    }

    /// Direct access to the read-later store for status / touch operations
    /// that the trait does not expose (architecture: UI panels do these
    /// directly; omnibox only searches).
    pub fn read_later(&self) -> &ReadLater {
        &self.read_later
    }

    /// Direct access to the notes store for URL-based note listing and
    /// update operations not covered by the search-oriented trait.
    pub fn notes(&self) -> &Notes {
        &self.notes
    }
}

impl KnowledgeStore for DefaultKnowledgeStore {
    // ── History (§12.1) ───────────────────────────────────────────────────

    fn index_history(&self, rowid: i64, url: &str, title: &str, text: &str) -> Result<()> {
        self.history.index(rowid, url, title, text)
    }

    fn unindex_history(&self, rowid: i64) -> Result<()> {
        self.history.unindex(rowid)
    }

    fn search_history(&self, query: &str, limit: i64) -> Result<Vec<KnowledgeHistoryHit>> {
        self.history.search(query, limit).map(|hits| {
            hits.into_iter()
                .map(|h| KnowledgeHistoryHit {
                    rowid: h.rowid,
                    url: h.url,
                    title: h.title,
                    snippet: h.snippet,
                    score: h.score,
                })
                .collect()
        })
    }

    // ── Notes (§12.2) ─────────────────────────────────────────────────────

    fn add_note(
        &self,
        url: &str,
        selection: &str,
        context: &str,
        comment: &str,
        created_at: i64,
    ) -> Result<i64> {
        self.notes.add(url, selection, context, comment, created_at)
    }

    fn delete_note(&self, id: i64) -> Result<()> {
        self.notes.delete(id)
    }

    fn search_notes(&self, query: &str, limit: i64) -> Result<Vec<KnowledgeNoteHit>> {
        self.notes.search(query, limit).map(|hits| {
            hits.into_iter()
                .map(|h| KnowledgeNoteHit {
                    id: h.note.id,
                    url: h.note.url,
                    selection: h.note.selection,
                    comment: h.note.comment,
                    snippet: h.snippet,
                    score: h.score,
                })
                .collect()
        })
    }

    // ── Read-later (§12.3) ────────────────────────────────────────────────

    fn save_read_later(
        &self,
        url: &str,
        title: &str,
        html_snapshot: &[u8],
        text: &str,
        tags: &[String],
        saved_at: i64,
    ) -> Result<i64> {
        self.read_later.save(url, title, html_snapshot, text, tags, saved_at)
    }

    fn search_read_later(&self, query: &str, limit: i64) -> Result<Vec<KnowledgeReadLaterHit>> {
        self.read_later.search(query, limit).map(|hits| {
            hits.into_iter()
                .map(|h| KnowledgeReadLaterHit {
                    id: h.entry.id,
                    url: h.entry.url,
                    title: h.entry.title,
                    snippet: h.snippet,
                    score: h.score,
                })
                .collect()
        })
    }

    // ── Open tabs (§12.4) ─────────────────────────────────────────────────

    fn index_tab(&self, tab_id: i64, url: &str, title: &str, text: &str) -> Result<()> {
        self.tabs.index_tab(tab_id, url, title, text)
    }

    fn remove_tab(&self, tab_id: i64) -> Result<()> {
        self.tabs.remove_tab(tab_id)
    }

    fn search_tabs(&self, query: &str, limit: i64) -> Result<Vec<KnowledgeTabHit>> {
        self.tabs.search(query, limit).map(|hits| {
            hits.into_iter()
                .map(|h| KnowledgeTabHit {
                    tab_id: h.tab_id,
                    url: h.url,
                    title: h.title,
                    snippet: h.snippet,
                    score: h.score,
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::ext::KnowledgeStore as KS;

    use super::*;

    fn make() -> DefaultKnowledgeStore {
        DefaultKnowledgeStore::open_in_memory().unwrap()
    }

    // ── history ───────────────────────────────────────────────────────────

    #[test]
    fn history_index_search_basic() {
        let s = make();
        s.index_history(1, "https://rust-lang.org/", "Rust", "systems programming language")
            .unwrap();
        let hits = s.search_history("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rowid, 1);
        assert_eq!(hits[0].url, "https://rust-lang.org/");
    }

    #[test]
    fn history_unindex_removes_entry() {
        let s = make();
        s.index_history(1, "https://a/", "t", "hello").unwrap();
        s.unindex_history(1).unwrap();
        assert!(s.search_history("hello", 10).unwrap().is_empty());
    }

    #[test]
    fn history_search_empty_on_miss() {
        let s = make();
        s.index_history(1, "https://a/", "t", "apple").unwrap();
        assert!(s.search_history("banana", 10).unwrap().is_empty());
    }

    // ── notes ─────────────────────────────────────────────────────────────

    #[test]
    fn notes_add_search_basic() {
        let s = make();
        let id = s
            .add_note("https://a/", "Rust is great", "paragraph", "interesting", 100)
            .unwrap();
        assert!(id > 0);
        let hits = s.search_notes("Rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
        assert_eq!(hits[0].selection, "Rust is great");
    }

    #[test]
    fn notes_delete_removes_from_search() {
        let s = make();
        let id = s.add_note("https://a/", "removable", "", "", 100).unwrap();
        s.delete_note(id).unwrap();
        assert!(s.search_notes("removable", 10).unwrap().is_empty());
    }

    #[test]
    fn notes_search_by_comment() {
        let s = make();
        s.add_note("https://a/", "boring text", "", "insightful remark", 100).unwrap();
        assert_eq!(s.search_notes("insightful", 10).unwrap().len(), 1);
    }

    #[test]
    fn notes_snippet_field_populated() {
        let s = make();
        s.add_note("https://a/", "Rust ownership model", "", "", 100).unwrap();
        let hits = s.search_notes("ownership", 10).unwrap();
        assert_eq!(hits.len(), 1);
        let snip_lc = hits[0].snippet.to_lowercase();
        assert!(snip_lc.contains("**ownership**"), "snippet = {}", hits[0].snippet);
    }

    // ── read-later ────────────────────────────────────────────────────────

    #[test]
    fn read_later_save_search_basic() {
        let s = make();
        let id = s
            .save_read_later(
                "https://example.com/article",
                "Example Article",
                b"<html>...</html>",
                "article about Rust async",
                &[],
                100,
            )
            .unwrap();
        assert!(id > 0);
        let hits = s.search_read_later("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
        assert_eq!(hits[0].title, "Example Article");
    }

    #[test]
    fn read_later_search_empty_on_miss() {
        let s = make();
        s.save_read_later("https://a/", "t", b"", "apple text", &[], 100).unwrap();
        assert!(s.search_read_later("banana", 10).unwrap().is_empty());
    }

    // ── tabs ──────────────────────────────────────────────────────────────

    #[test]
    fn tabs_index_search_basic() {
        let s = make();
        s.index_tab(7, "https://docs.rs/serde", "Serde docs", "serialization framework")
            .unwrap();
        let hits = s.search_tabs("serialization", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tab_id, 7);
        assert_eq!(hits[0].url, "https://docs.rs/serde");
    }

    #[test]
    fn tabs_remove_drops_from_index() {
        let s = make();
        s.index_tab(3, "https://a/", "T", "closable content").unwrap();
        s.remove_tab(3).unwrap();
        assert!(s.search_tabs("closable", 10).unwrap().is_empty());
    }

    #[test]
    fn tabs_reindex_replaces_same_tab() {
        let s = make();
        s.index_tab(1, "https://old/", "Old", "old content").unwrap();
        s.index_tab(1, "https://new/", "New", "new content").unwrap();
        assert!(s.search_tabs("old", 10).unwrap().is_empty());
        assert_eq!(s.search_tabs("new", 10).unwrap().len(), 1);
    }

    // ── cross-subsystem: object-safe dyn usage ────────────────────────────

    #[test]
    fn trait_object_usage() {
        let store: Box<dyn KS> = Box::new(make());
        store
            .index_history(1, "https://a/", "title", "text")
            .unwrap();
        let hits = store.search_history("text", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "title");
    }
}
