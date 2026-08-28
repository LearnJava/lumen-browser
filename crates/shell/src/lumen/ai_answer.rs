//! Answering an `@ai <query>` omnibox prompt from what the browser knows
//! (12.5, 12.8).
//!
//! Two bodies, one per build: with `--features ai` the answer is grounded in
//! bookmark embeddings through `lumen-ai`, without it the same method returns
//! a static hint row - the pair must move together, since the cfg is what
//! makes exactly one of them exist.

use crate::*;

impl Lumen {
    /// Concatenated visible text of the current page, for AI summarisation
    /// (В§12.8). Empty string when there's no layout tree yet.
    pub(crate) fn current_page_text(&self) -> String {
        let Some(lb) = &self.layout_box else {
            return String::new();
        };
        lumen_layout::collect_visible_text(lb)
            .into_iter()
            .map(|f| f.text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Answer an `@ai <query>` omnibox prompt (В§12.5, Step 7).
    ///
    /// Grounds the answer in bookmark embeddings (В§12.8) вЂ” the only
    /// `DefaultKnowledgeStore`-populatable data this shell has today; wiring a
    /// real browsing-history population path is deferred (see
    /// `subsystems/ai.md` В§Deferred, needs its own task brief). Rebuilds an
    /// in-memory `DefaultKnowledgeStore` per query rather than caching one on
    /// `Lumen`, mirroring `query_omnibox_suggestions`'s existing synchronous
    /// per-keystroke `@bookmarks` embed call.
    #[cfg(feature = "ai")]
    pub(crate) fn ai_answer_for(&self, query: &str) -> String {
        use lumen_ai::embedding::OllamaEmbeddingBackend;
        use lumen_ai::generation::OllamaGenerationBackend;
        use lumen_ai::rag::RagEngine;
        use lumen_knowledge::DefaultKnowledgeStore;

        let Ok(store) = DefaultKnowledgeStore::open_in_memory() else {
            return self.ai_backend.query(query);
        };
        if let Ok(bookmarks) = self.bookmarks.list_all() {
            for b in bookmarks {
                if let Some(embedding) = &b.embedding {
                    store.index_semantic(
                        b.id,
                        &b.url,
                        &b.title,
                        lumen_storage::bookmarks::embedding_from_bytes(embedding),
                    );
                }
            }
        }
        let embedding_backend = OllamaEmbeddingBackend::new("nomic-embed-text");
        let generation_backend = OllamaGenerationBackend::new("phi3:mini");
        let answer = RagEngine::new(5).answer(query, &store, &embedding_backend, &generation_backend);
        // Ollama unreachable/erroring в†’ fall back to the NullAiBackend stub
        // message, matching ADR-019's documented degrade-not-error contract.
        if answer.is_empty() { self.ai_backend.query(query) } else { answer }
    }

    /// `--features ai` not compiled in: static hint row, no `lumen-ai` calls.
    #[cfg(not(feature = "ai"))]
    pub(crate) fn ai_answer_for(&self, _query: &str) -> String {
        "AI module not enabled вЂ” rebuild with `cargo build --features ai` \
         (requires a local Ollama daemon, see ADR-019)."
            .to_owned()
    }
}
