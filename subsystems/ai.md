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

### Step 6 — semantic bookmarks (2026-07-15)
- `Bookmark` (`crates/storage/src/bookmarks.rs`) gained nullable
  `summary`/`embedding` columns (`embedding` is an f32-LE blob via
  `embedding_to_bytes`/`embedding_from_bytes`); pre-Step-6 databases are
  migrated in place (`migrate_semantic_columns`, guarded by
  `PRAGMA table_info`). `Bookmarks::set_semantic(url, summary, embedding)`
  writes both fields without touching `add`'s signature.
- `Lumen::bookmark_current_page` (Ctrl+D) calls
  `self.ai_backend.summarise(page_text)` then `.embed(summary)` and stores
  the result. Deliberately **not** `#[cfg(feature = "ai")]`-gated: `ai_backend`
  is always present (default `NullAiBackend`), whose empty
  `summarise`/`embed` already give the "no AI" fallback (skip `set_semantic`)
  without a cfg branch — same pattern as the existing `AiPanel` query call site.
- New `@bookmarks <query>` omnibox prefix (`address_bar.rs` +
  `Lumen::query_omnibox_suggestions`): no `@bookmarks` prefix or FTS5 table
  existed before this step, so text matching is substring-based (title/url/tags,
  same approach as the existing `@tabs` prefix) rather than real BM25;
  cosine-similarity against stored embeddings is layered on top when a query
  embedding is available, and text matches are always ranked above
  semantic-only ones.
- Does **not** wire `Lumen::ai_backend`/`AiPanel` to a real `RagEngine`, and
  does not populate `DefaultKnowledgeStore`'s semantic index from browsing
  history — [`docs/tasks/ph3-ai-module.md`](../docs/tasks/ph3-ai-module.md)'s
  Step 6 bullet list (the actual spec this step was implemented against) does
  not include that wiring; it was only speculatively mentioned in this file's
  previous Deferred note. Still open — see Deferred below.

### Step 7 — omnibox `@ai` prefix (2026-07-15)
- New `OmniboxPrefix::Ai` / `OmniboxSuggestion::Ai { answer }` (`address_bar.rs`)
  and `@ai` branch in `Lumen::query_omnibox_suggestions` (`main.rs`), following
  the same prefix-parsing/dropdown-row pattern as `@bookmarks`. Commit value is
  the sentinel `"ai-answer:noop"`, intercepted in `handle_omnibox_commit` as a
  no-op (nothing to navigate to — the answer is already the row's own text),
  same pattern as `note-viewer:`/`switch-tab:`.
- `Lumen::ai_answer_for` (two `#[cfg(feature = "ai")]`/`#[cfg(not(...))]`
  variants — the first `#[cfg(feature = "ai")]` gate anywhere in
  `crates/shell`): under `ai`, builds a throwaway
  `DefaultKnowledgeStore::open_in_memory()` per query, populates it from
  `self.bookmarks.list_all()`'s stored embeddings (the only
  `DefaultKnowledgeStore`-populatable data this shell has — see the still-open
  "population path" item below), then calls
  `RagEngine::new(5).answer(query, &store, &OllamaEmbeddingBackend::new("nomic-embed-text"),
  &OllamaGenerationBackend::new("phi3:mini"))`. An empty `RagEngine::answer`
  result (Ollama unreachable, per ADR-019's degrade-not-error contract) falls
  back to `self.ai_backend.query(query)` (the `NullAiBackend` stub text).
  Without `ai`, returns a static "AI module not enabled" hint — no `lumen-ai`
  reference at all in that build.
- `ai = ["dep:lumen-ai", "lumen-ai/ollama"]` in `crates/shell/Cargo.toml`:
  `lumen-ai`'s `rag`/`embedding`/`generation` modules are gated behind its own
  `ollama` feature (`default = []`), so shell's `ai` feature has to forward it
  explicitly or `RagEngine` etc. don't exist even with `dep:lumen-ai` on.
- Recomputes the RAG answer synchronously on every keystroke while the `@ai`
  prefix is active (network round-trip to Ollama included) — same synchronous
  per-keystroke shape as `@bookmarks`' embed call, not addressed in this step.
- Does **not** change `Lumen::ai_backend` (still always `NullAiBackend`) or
  wire `AiPanel` to `RagEngine` — out of scope per Step 7's bullet list; see
  Deferred below (still open, `@ai` omnibox uses its own local backend
  instances instead).

## Deferred

- Wire `Lumen::ai_backend`/`AiPanel` to a real `RagEngine` backed by a
  populated `DefaultKnowledgeStore` — needs a design for *what* gets indexed
  and *when* (a "population path" from real browsing history/notes/bookmarks),
  which no step so far has specified. Needs its own task brief before
  implementation. Omnibox `@ai` (Step 7) works around this narrowly by
  populating from bookmark embeddings only, at query time — that is not a
  substitute for a real history/notes population path.
- Real HNSW vector index in `lumen-knowledge` (§12.5) — `SemanticIndex` is
  still the Step 3 linear-scan placeholder; tracked separately in
  `p2-knowledge-stemmer-hnsw.md`.

## Invariants

- The basic bundle (no `ai` feature) must never observe any behavior change —
  `NullAiBackend`'s `embed`/`summarise` return empty via the trait's default
  impls, `cargo check -p lumen-shell` (no `ai`) must stay green.
- No crate outside `lumen-ai` may import an ML dependency.
- The Ollama backend connects only to `127.0.0.1:11434`, hardcoded — no
  user-configurable remote host in this slice (§12.5 privacy invariant).
