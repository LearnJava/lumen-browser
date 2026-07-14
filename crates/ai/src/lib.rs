//! Optional local AI layer: embeddings, summarisation, RAG over the user's own
//! browsing history, notes, and read-later list (§12.5, §12.8).
//!
//! Feature-gated: `lumen-shell` only depends on this crate under its own `ai`
//! Cargo feature (off by default). All inference runs on-device — see
//! [ADR-019](../../../docs/decisions/ADR-019-ai-module-embedding-backend.md)
//! for the embedding/generation backend choice (Ollama HTTP first).

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Crate compiles and links against lumen-core. Real coverage lands with
        // the embedding/generation backends (docs/tasks/ph3-ai-module.md Step 2+).
    }
}
