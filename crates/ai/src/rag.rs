//! RAG (retrieval-augmented generation) engine (ADR-019,
//! `docs/tasks/ph3-ai-module.md` Step 5).
//!
//! [`RagEngine::answer`] grounds a generated answer in the user's own
//! browsing history: it embeds the prompt, retrieves the most semantically
//! similar entries from a [`DefaultKnowledgeStore`]'s in-memory semantic
//! index, and passes them as context to a [`GenerationBackend`]. Shell
//! wiring (`AiPanel::submit`, `Lumen::ai_backend`) is deferred to a later
//! step — see `docs/tasks/ph3-ai-module.md` Status, same pattern as Steps
//! 3-4 deferring shell wiring to this one.

use lumen_knowledge::DefaultKnowledgeStore;

use crate::embedding::EmbeddingBackend;
use crate::generation::GenerationBackend;

/// Retrieval-augmented generation over a [`DefaultKnowledgeStore`]'s
/// semantic index (§12.5, §12.8).
pub struct RagEngine {
    /// Maximum number of retrieved history entries to include as context.
    top_k: i64,
}

impl RagEngine {
    /// New engine that retrieves up to `top_k` context entries per query.
    pub fn new(top_k: i64) -> Self {
        Self { top_k }
    }

    /// Answer `prompt`, grounding the response in the `top_k` most
    /// semantically similar entries of `knowledge_store`.
    ///
    /// Embeds `prompt` via `embedding_backend`, looks up nearest neighbours
    /// in `knowledge_store`'s semantic index, builds a context string from
    /// their titles and URLs, and delegates to
    /// `generation_backend.generate(prompt, context)`. Falls back to an
    /// empty context (bare prompt, no grounding) when embedding fails or no
    /// entries are indexed yet — matching `GenerationBackend`'s documented
    /// "no answer available" contract, this never panics or blocks on a
    /// missing index.
    pub fn answer(
        &self,
        prompt: &str,
        knowledge_store: &DefaultKnowledgeStore,
        embedding_backend: &dyn EmbeddingBackend,
        generation_backend: &dyn GenerationBackend,
    ) -> String {
        let context = self.retrieve_context(prompt, knowledge_store, embedding_backend);
        generation_backend.generate(prompt, &context).unwrap_or_default()
    }

    /// Build the context string passed to `generate` from the nearest
    /// semantic-index entries, or an empty string if embedding fails or the
    /// index has no matches.
    fn retrieve_context(
        &self,
        prompt: &str,
        knowledge_store: &DefaultKnowledgeStore,
        embedding_backend: &dyn EmbeddingBackend,
    ) -> String {
        let Ok(query_vector) = embedding_backend.embed(prompt) else {
            return String::new();
        };
        let hits = knowledge_store.search_semantic(&query_vector, self.top_k);
        hits.iter()
            .map(|hit| format!("- {} ({})", hit.title, hit.url))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::EmbeddingError;
    use crate::generation::GenerationError;

    /// Fixed-vector embedding backend: returns a caller-supplied vector,
    /// ignoring the input text (deterministic top-K retrieval in tests).
    struct FixedEmbeddingBackend {
        vector: Vec<f32>,
    }

    impl EmbeddingBackend for FixedEmbeddingBackend {
        fn embed(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(self.vector.clone())
        }
    }

    /// Embedding backend that always fails (simulates an unreachable model).
    struct FailingEmbeddingBackend;

    impl EmbeddingBackend for FailingEmbeddingBackend {
        fn embed(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Err(EmbeddingError::InvalidResponse("simulated failure".to_owned()))
        }
    }

    /// Generation backend that echoes the context it was given, so tests can
    /// assert on exactly what `RagEngine` retrieved.
    struct EchoContextBackend;

    impl GenerationBackend for EchoContextBackend {
        fn generate(&self, _prompt: &str, context: &str) -> Result<String, GenerationError> {
            Ok(format!("context: [{context}]"))
        }
    }

    fn store_with_entries() -> DefaultKnowledgeStore {
        let store = DefaultKnowledgeStore::open_in_memory().expect("in-memory store");
        store.index_semantic(1, "https://a.example", "Entry A", vec![1.0, 0.0]);
        store.index_semantic(2, "https://b.example", "Entry B", vec![0.0, 1.0]);
        store
    }

    #[test]
    fn answer_grounds_response_in_nearest_semantic_hit() {
        let store = store_with_entries();
        let embedding = FixedEmbeddingBackend { vector: vec![1.0, 0.0] };
        let generation = EchoContextBackend;
        let engine = RagEngine::new(1);

        let response = engine.answer("what did I read?", &store, &embedding, &generation);

        assert_eq!(response, "context: [- Entry A (https://a.example)]");
    }

    #[test]
    fn answer_limits_context_to_top_k() {
        let store = store_with_entries();
        // Equidistant from both entries: cosine similarity ties, both returned
        // when top_k allows it.
        let embedding = FixedEmbeddingBackend { vector: vec![1.0, 1.0] };
        let generation = EchoContextBackend;
        let engine = RagEngine::new(1);

        let response = engine.answer("prompt", &store, &embedding, &generation);

        // Exactly one context line, not both.
        assert_eq!(response.matches('-').count(), 1);
    }

    #[test]
    fn answer_falls_back_to_empty_context_on_embedding_failure() {
        let store = store_with_entries();
        let embedding = FailingEmbeddingBackend;
        let generation = EchoContextBackend;
        let engine = RagEngine::new(5);

        let response = engine.answer("prompt", &store, &embedding, &generation);

        assert_eq!(response, "context: []");
    }

    #[test]
    fn answer_falls_back_to_empty_context_on_empty_index() {
        let store = DefaultKnowledgeStore::open_in_memory().expect("in-memory store");
        let embedding = FixedEmbeddingBackend { vector: vec![1.0, 0.0] };
        let generation = EchoContextBackend;
        let engine = RagEngine::new(5);

        let response = engine.answer("prompt", &store, &embedding, &generation);

        assert_eq!(response, "context: []");
    }

    #[test]
    fn answer_returns_empty_string_when_generation_fails() {
        struct FailingGenerationBackend;
        impl GenerationBackend for FailingGenerationBackend {
            fn generate(&self, _prompt: &str, _context: &str) -> Result<String, GenerationError> {
                Err(GenerationError::InvalidResponse("simulated failure".to_owned()))
            }
        }

        let store = store_with_entries();
        let embedding = FixedEmbeddingBackend { vector: vec![1.0, 0.0] };
        let generation = FailingGenerationBackend;
        let engine = RagEngine::new(1);

        let response = engine.answer("prompt", &store, &embedding, &generation);

        assert_eq!(response, "");
    }
}
