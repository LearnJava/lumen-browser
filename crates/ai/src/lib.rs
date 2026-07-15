//! Optional local AI layer: embeddings, summarisation, RAG over the user's own
//! browsing history, notes, and read-later list (§12.5, §12.8).
//!
//! Feature-gated: `lumen-shell` only depends on this crate under its own `ai`
//! Cargo feature (off by default). All inference runs on-device — see
//! [ADR-019](../../../docs/decisions/ADR-019-ai-module-embedding-backend.md)
//! for the embedding/generation backend choice (Ollama HTTP first).

#[cfg(feature = "ollama")]
pub mod embedding;
#[cfg(feature = "ollama")]
pub mod generation;
#[cfg(feature = "ollama")]
mod http;
#[cfg(feature = "ollama")]
pub mod rag;

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Crate compiles and links against lumen-core. Semantic-bookmark
        // (Step 6) and omnibox `@ai` (Step 7) wiring land next.
    }
}
