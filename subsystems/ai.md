# lumen-ai

Optional, feature-flagged local AI layer — embeddings, summarisation, RAG over
the user's own browsing history, notes, and read-later list (§12.5), plus
semantic bookmarks (§12.8).

## Scope

Greenfield, Phase 3. Not compiled into the default bundle: `lumen-shell`
depends on `lumen-ai` only under its own `ai` Cargo feature
(`ai = ["dep:lumen-ai"]`). All inference runs on-device — see
[ADR-019](../docs/decisions/ADR-019-ai-module-embedding-backend.md) for the
backend choice (Ollama HTTP first, `candle` deferred).

Full step-by-step plan — [`docs/tasks/ph3-ai-module.md`](../docs/tasks/ph3-ai-module.md).

## Done

### Step 1 — crate skeleton + feature flag (2026-07-15)
- `crates/ai/` registered in the workspace; `lumen-ai` package with
  `default = []` / `ollama = []` Cargo features.
- `crates/shell` gained the `ai` feature (`dep:lumen-ai`, not in default) and
  an optional `lumen-ai` dependency.
- `lumen_core::ext::AiBackend` extended with `embed(&self, text) -> Vec<f32>`
  and `summarise(&self, text) -> String`, both with default (empty) impls so
  every existing `AiBackend` implementation (`NullAiBackend`) keeps compiling
  unchanged.
- No actual embedding/generation logic yet — the crate is an empty skeleton
  (smoke test only).

### Step 2 — `EmbeddingBackend` trait + `OllamaEmbeddingBackend` (2026-07-15)
- `crates/ai/src/embedding.rs` (gated behind the `ollama` Cargo feature):
  `EmbeddingBackend` trait (`embed(&self, text) -> Result<Vec<f32>, EmbeddingError>`)
  + `OllamaEmbeddingBackend`, which frames a `POST /api/embeddings` request by
  hand over `std::net::TcpStream` (no `reqwest`/`hyper`) and parses the JSON
  `{"embedding": [...]}` response with `serde_json`.
- `OllamaEmbeddingBackend` also implements `lumen_core::ext::AiBackend`:
  `embed` delegates to `EmbeddingBackend::embed` (empty vector on error, per
  the trait's documented contract); `query` returns an empty string —
  chat/generation is `GenerationBackend`'s job (Step 4), not yet implemented.
- Tests mock the Ollama endpoint with a local `TcpListener` (no real Ollama
  process required): happy path, malformed JSON, and missing-field response
  shapes.

### Step 3 — semantic search over history/notes (2026-07-15)
- `SemanticIndex` (`crates/knowledge/src/semantic.rs`) — see
  [`subsystems/knowledge.md`](knowledge.md) §Done for the full description
  (mock/linear-scan nearest-neighbour index, `DefaultKnowledgeStore::search_semantic`).
  `lumen-knowledge` does not depend on `lumen-ai`; callers embed text via
  `lumen_ai::embedding::EmbeddingBackend` themselves.

### Step 4 — `GenerationBackend` trait + `OllamaGenerationBackend` (2026-07-15)
- `crates/ai/src/generation.rs` (gated behind the `ollama` Cargo feature):
  `GenerationBackend` trait (`generate(&self, prompt, context) -> Result<String, GenerationError>`)
  + `OllamaGenerationBackend`, which frames a `POST /api/generate` request
  the same way `OllamaEmbeddingBackend` does — hand-rolled `TcpStream`
  HTTP/1.1 framing, now shared via `crates/ai/src/http.rs`
  (`http_response_body`, factored out of `embedding.rs` in this step).
- `OllamaGenerationBackend` implements `lumen_core::ext::AiBackend`:
  `summarise` delegates via a `"summarise: ..."`-prefixed prompt, `query`
  delegates via `generate(prompt, "")`; both default to an empty string on
  error, per the trait's documented contract. `embed` is not implemented
  (uses the trait's default empty-vector impl) — this backend is
  generation-only.
- Tests mock the Ollama endpoint the same way Step 2 does: response
  parsing, malformed-JSON/missing-field rejection, `AiBackend::summarise`/
  `query` delegation and empty-string error fallback.
- Does not wire a real backend into `Lumen::ai_backend`/`AiPanel` — that is
  Step 5 (`RagEngine`).

### Step 5 — `RagEngine` (2026-07-15)
- `crates/ai/src/rag.rs` (gated behind the `ollama` Cargo feature):
  `RagEngine::new(top_k)` + `RagEngine::answer(prompt, knowledge_store,
  embedding_backend, generation_backend) -> String`. Embeds `prompt`, calls
  `DefaultKnowledgeStore::search_semantic` for the `top_k` nearest indexed
  entries, builds a `"- title (url)"` context string from the hits, and
  delegates to `GenerationBackend::generate(prompt, context)`. Falls back to
  an empty context (bare-prompt generation) when embedding fails or the
  index has no matches — never panics or blocks on a missing index.
- `lumen-ai` gained a new internal workspace dependency on `lumen-knowledge`
  (no cycle: `lumen-knowledge` does not depend on `lumen-ai`) — needed to
  call `DefaultKnowledgeStore::search_semantic` directly, per the
  architecture diagram in `docs/tasks/ph3-ai-module.md`.
- Tests use fixed-vector/echo-context mock backends (no real Ollama
  process): grounds response in nearest hit, limits context to `top_k`,
  falls back to empty context on embedding failure or empty index, returns
  empty string when generation fails.
- Does not wire `RagEngine` into `Lumen::ai_backend`/`AiPanel` — nothing in
  the shell populates `DefaultKnowledgeStore`'s semantic index from real
  browsing history yet (no crate currently constructs a
  `DefaultKnowledgeStore` in `crates/shell` at all: it uses the individual
  `HistoryFts`/`Notes`/`ReadLater` stores directly). Wiring `AiPanel::submit`
  to a real `RagEngine` is deferred until that population path exists —
  tracked as part of Step 6/7 below, not invented here to avoid a
  panel that always "RAG"s over an empty index.

## Deferred

- Step 6 — semantic bookmarks: `Bookmarks` schema extension
  (`summary`/`embedding` nullable columns), auto-summarise + embed on save;
  also where `Lumen::ai_backend`/`AiPanel` first gets wired to a real
  `RagEngine` backed by a populated `DefaultKnowledgeStore`.
- Step 7 — omnibox `@ai` prefix routed through `RagEngine::answer`.

## Invariants

- The basic bundle (no `ai` feature) must never observe any behavior change —
  `NullAiBackend`'s `embed`/`summarise` return empty via the trait's default
  impls, `cargo check -p lumen-shell` (no `ai`) must stay green.
- No crate outside `lumen-ai` may import an ML dependency.
- The Ollama backend connects only to `127.0.0.1:11434`, hardcoded — no
  user-configurable remote host in this slice (§12.5 privacy invariant).
