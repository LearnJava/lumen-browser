# Ph3 — AI module (lumen-ai) + semantic bookmarks

**Developer:** P1
**Branch:** `p1-ph3-ai-module`
**Size:** XL
**Crates:** new `lumen-ai` (feature-flagged), `lumen-knowledge`, `lumen-storage`, `lumen-shell`
**Phase:** 3 (v1.0 target)

---

## Status

In progress. Optional feature, disabled in the default bundle. Step 1 (crate
skeleton, `AiBackend::embed`/`summarise`), Step 2 (`EmbeddingBackend` +
`OllamaEmbeddingBackend`) and Step 3 (`SemanticIndex` linear-scan +
`DefaultKnowledgeStore::search_semantic`) are merged — see `subsystems/ai.md`
§Done and `subsystems/knowledge.md`. Step 3 used the mock/linear-scan
interface Step 0 allows (the referenced HNSW prerequisite doc still does not
exist); a real ANN index is a drop-in replacement for `SemanticIndex` later.
Steps 4-7 not started.

---

## Goal

Implement `§12.5 Local AI layer` and `§12.8 Semantic bookmarks`
([`docs/plan/knowledge.md:68-130`](../plan/knowledge.md)):

1. **`lumen-ai` crate** — optional, behind a `lumen-shell` Cargo feature flag `ai`.
   Provides embeddings, summarisation, and RAG over the user's own browsing history,
   notes, and read-later list. The basic bundle (no `ai` feature) compiles and runs
   without any ML dependency.
2. **Semantic bookmarks** — extends the existing `Bookmarks` store
   ([`crates/storage/src/bookmarks.rs`](../../crates/storage/src/bookmarks.rs)) with
   auto-summary + embedding columns. When the `ai` feature is active, a bookmark
   saved for a loaded page is automatically summarised and embedded. Semantic
   similarity search surfaces these bookmarks in the omnibox even when the query
   wording differs from the stored title. Without `ai`, the bookmark store degrades
   to the current tag-based model — no schema migration required for the basic bundle.

---

## Open question / prerequisite: model and runtime choice

The design in `docs/plan/knowledge.md:79` names two options but defers the decision:

| Option | Pros | Cons |
|---|---|---|
| **A — Ollama HTTP API** (if user has Ollama installed) | Zero ML dependency in Lumen; model management is the user's problem; embedding and generation via a single REST call; easy to swap models. | Requires Ollama running locally; not available on a fresh install; adds a runtime dependency that Lumen cannot control. |
| **B — `candle` (Hugging Face, pure Rust)** | Self-contained, no external process; embedding model (`bge-small-en`) is ~30 MB GGUF; works without Ollama; entirely offline. | Adds a heavy compile-time dep (`candle-core`, `candle-nn`, `candle-transformers`); increases binary size; may be the fifth §5 FFI exception. |
| **C — `llama.cpp` via FFI** | CPU-only, quantised models run on any hardware; community-proven for local LLMs. | C FFI (unsafe); platform-specific build complexity; hard to cross-compile. |
| **D — remote API (cloud)** | Zero local compute; high quality. | Contradicts §12.5 privacy rationale entirely — leaks browsing history to a third party. Not acceptable as primary path. |

**Recommended starting point:** implement the embedding pipeline against an abstract
`EmbeddingBackend` trait (see §Architecture). Provide Option A (Ollama) as the first
concrete backend, because it requires no new compile-time ML deps and lets the rest of
the pipeline (HNSW, RAG, semantic bookmarks) be built and tested first. Option B
(`candle`) is the target for self-contained deployment and can be wired in later without
changing the consumer API.

**Decision must be logged as `docs/decisions/ADR-NNN.md` before the first commit
that adds an ML dependency.** Until then, no crate beyond `lumen-ai` imports an ML
library.

---

## Current state

### Existing stubs (real code, not proposed)

| Symbol | File:line | Notes |
|---|---|---|
| `AiBackend` trait | [`crates/core/src/ext.rs:2918`](../../crates/core/src/ext.rs) | `fn query(&self, prompt: &str) -> String` — Phase 0 synchronous stub; no embedding method yet |
| `NullAiBackend` | [`crates/core/src/ext.rs:2930`](../../crates/core/src/ext.rs) | Returns a human-readable stub; installed in shell by default |
| `AiPanel` | [`crates/shell/src/panels/ai_panel.rs:57`](../../crates/shell/src/panels/ai_panel.rs) | Right-docked 200 px panel; `Ctrl+Shift+A` toggle; calls `AiBackend::query` synchronously |
| `Lumen::ai_backend` field | [`crates/shell/src/main.rs:5599`](../../crates/shell/src/main.rs) | `Box<dyn AiBackend>`; initialised to `NullAiBackend` |
| `ai_backend` wiring | [`crates/shell/src/main.rs:12974-12978`](../../crates/shell/src/main.rs) | AI panel submit dispatches to `self.ai_backend.query` |
| `"local-ai"` plugin capability | [`crates/storage/src/plugins.rs:402`](../../crates/storage/src/plugins.rs) | Capability string already declared; no runtime gating yet |

### Existing knowledge stores (what AI augments)

| Store | File:line | Indexed content |
|---|---|---|
| `HistoryFts` (FTS5) | [`crates/knowledge/src/fts.rs:43`](../../crates/knowledge/src/fts.rs) | `url, title, text` of visited pages |
| `Notes` | [`crates/knowledge/src/notes.rs:41`](../../crates/knowledge/src/notes.rs) | User text selections + comments |
| `ReadLater` | [`crates/knowledge/src/read_later.rs:75`](../../crates/knowledge/src/read_later.rs) | Saved pages with status |
| `Bookmarks` | [`crates/storage/src/bookmarks.rs:36`](../../crates/storage/src/bookmarks.rs) | `url, title, folder, tags` — no summary/embedding columns yet |
| `DefaultKnowledgeStore` | [`crates/knowledge/src/store.rs:33`](../../crates/knowledge/src/store.rs) | Aggregates `HistoryFts` + `Notes`; no AI augmentation yet |

### Cross-reference: HNSW vector index

[`docs/tasks/p2-knowledge-stemmer-hnsw.md`](p2-knowledge-stemmer-hnsw.md) — **Part B**
covers adding an HNSW approximate-nearest-neighbour index to `lumen-knowledge`.
That task is a **prerequisite for semantic search** in this module. The `lumen-ai`
crate consumes the HNSW index — it does not own or duplicate it. Do not implement
HNSW here; coordinate with or complete that task first (it is blocked on the same
"embedding source" open question as this one).

### Confirmed absence of ML code

A grep over all `.rs` files for `\bai\b|llm|embedding|onnx|candle|llama|hnsw|HNSW`
returns only:
- the `AiBackend` / `NullAiBackend` stubs above
- the `"local-ai"` capability string in plugins
- the word "embedding" in unrelated font/JS contexts
- no HNSW or ML runtime imports

The `lumen-ai` crate does not exist yet; it is greenfield.

---

## Architecture

```
lumen-shell (feature = "ai")
  └─ lumen-ai  ←─────────── new crate, feature-gated
        ├─ EmbeddingBackend trait   (embed(text) -> Vec<f32>)
        ├─ GenerationBackend trait  (generate(prompt, context) -> String)
        ├─ OllamaBackend            (HTTP to localhost:11434)
        ├─ CandleBackend            (optional, heavier dep; Phase 3+)
        └─ RagEngine
              ├─ consumes HNSW index from lumen-knowledge
              └─ consumes KnowledgeStore::search_history / search_notes

lumen-knowledge  (no ML dep; provides HNSW + FTS to consumers)
  └─ HnswIndex (from p2-knowledge-stemmer-hnsw task)

lumen-storage::Bookmarks  (extended with semantic columns)
  ├─ summary: Option<String>         — auto or manual
  ├─ embedding: Option<Vec<u8>>      — f32 blob (null when ai feature off)
  └─ schema migration: ALTER TABLE … ADD COLUMN (nullable, safe)

lumen-core::ext::AiBackend (existing, extended)
  ├─ fn query(&self, prompt: &str) -> String         (existing)
  ├─ fn embed(&self, text: &str) -> Vec<f32>         (proposed addition)
  └─ fn summarise(&self, text: &str) -> String       (proposed addition)
```

**Feature-flag contract:**

```toml
# crates/lumen-ai/Cargo.toml
[features]
default = []
ollama = []          # Ollama HTTP backend (no extra Rust deps)
candle = ["dep:candle-core", "dep:candle-transformers"]  # self-contained

# crates/shell/Cargo.toml
[features]
ai = ["dep:lumen-ai"]
default = ["backend-femtovg", "backend-wgpu", "quickjs"]  # ai NOT in default
```

`#[cfg(feature = "ai")]` guards in `lumen-shell` replace `NullAiBackend` with a real
implementation. All call sites that hold `Box<dyn AiBackend>` are unchanged.

**Basic bundle (no `ai` feature):**
- `NullAiBackend` remains installed; AI panel renders and shows the stub message.
- `Bookmarks.summary` and `.embedding` columns exist (nullable) but are never written.
- No `lumen-ai` crate is compiled.

---

## Entry points

All items below marked **(proposed)** do not exist yet.

| Symbol / location | File:line | Notes |
|---|---|---|
| `AiBackend::query` | [`crates/core/src/ext.rs:2923`](../../crates/core/src/ext.rs) | Exists; add `embed` + `summarise` methods here |
| `NullAiBackend` | [`crates/core/src/ext.rs:2930`](../../crates/core/src/ext.rs) | Extend with no-op `embed` / `summarise` impls |
| `Lumen::ai_backend` field | [`crates/shell/src/main.rs:5599`](../../crates/shell/src/main.rs) | Wire real backend under `#[cfg(feature = "ai")]` |
| `AiPanel::submit` | [`crates/shell/src/panels/ai_panel.rs`](../../crates/shell/src/panels/ai_panel.rs) | Add RAG-augmented prompt path **(proposed)** |
| `crates/lumen-ai/` | — | New crate **(proposed)** |
| `crates/lumen-ai/src/embedding.rs` | — | `EmbeddingBackend` trait + impls **(proposed)** |
| `crates/lumen-ai/src/generation.rs` | — | `GenerationBackend` trait **(proposed)** |
| `crates/lumen-ai/src/rag.rs` | — | RAG engine over HNSW + KnowledgeStore **(proposed)** |
| `crates/lumen-ai/src/ollama.rs` | — | Ollama HTTP client impl **(proposed)** |
| `Bookmarks` struct | [`crates/storage/src/bookmarks.rs:36`](../../crates/storage/src/bookmarks.rs) | Add `summary`, `embedding` fields **(proposed)** |
| `DefaultKnowledgeStore` | [`crates/knowledge/src/store.rs:33`](../../crates/knowledge/src/store.rs) | Add `embed_on_index` hook **(proposed)** |
| Omnibox `@ai` prefix | [`crates/shell/src/main.rs`](../../crates/shell/src/main.rs) | New omnibox prefix alongside `@history`, `@tabs` **(proposed)** |

---

## Steps

### Step 0 — Prerequisites

- Confirm `p2-knowledge-stemmer-hnsw.md` Part B (HNSW index) is complete, or agree on
  a mock interface that can be swapped in later.
- Decide on embedding backend (ADR) and write `docs/decisions/ADR-NNN.md`.

### Step 1 — New `lumen-ai` crate skeleton + feature flag

- `cargo new --lib crates/lumen-ai`; add to workspace `members`.
- Add `[features] default = [] / ollama = [] / candle = [...]` to
  `crates/lumen-ai/Cargo.toml`.
- Add `[features] ai = ["dep:lumen-ai"]` to `crates/shell/Cargo.toml`.
- Add `lumen-ai = { path = "crates/lumen-ai", optional = true }` to shell deps.
- Extend `AiBackend` in `crates/core/src/ext.rs:2918` with `embed` and `summarise`
  methods (default impls returning empty/stub values so `NullAiBackend` still compiles
  without changes).
- All builds (with and without `ai`) must pass `cargo check`.

### Step 2 — Embedding backend (Ollama first)

- Define `EmbeddingBackend` trait in `crates/lumen-ai/src/embedding.rs`.
- Implement `OllamaEmbeddingBackend` calling `POST localhost:11434/api/embeddings`
  (`model: "nomic-embed-text"` or configurable).
- Implement `AiBackend::embed` for `OllamaEmbeddingBackend` by delegating.
- Unit test: mock the HTTP endpoint with a static JSON fixture; assert vector length.

### Step 3 — Semantic search over history / notes

- Wire `OllamaEmbeddingBackend` to the HNSW index in `lumen-knowledge`
  (see `p2-knowledge-stemmer-hnsw.md` Part B for the `HnswIndex` interface).
- Add `DefaultKnowledgeStore::search_semantic(query_vec, limit)` **(proposed)** that
  calls `HnswIndex::nearest(query_vec, limit)` and returns `KnowledgeHistoryHit`s.
- Test: index 10 synthetic entries, embed a query, assert top hit is the closest entry.

### Step 4 — Summarisation

- Define `GenerationBackend` trait in `crates/lumen-ai/src/generation.rs`.
- Implement `OllamaGenerationBackend` calling `POST localhost:11434/api/generate`
  (`model: "phi3:mini"` or configurable, with a `summarise:` system prompt).
- Implement `AiBackend::summarise` by delegating.
- Unit test: mock the HTTP endpoint; assert non-empty string returned.

### Step 5 — RAG engine

- Implement `RagEngine` in `crates/lumen-ai/src/rag.rs`:
  - `fn answer(prompt, knowledge_store, embedding_backend, generation_backend) -> String`
  - Embed the prompt → query HNSW → retrieve top-K chunks from `KnowledgeStore` →
    build context string → call `GenerationBackend::generate(prompt, context)`.
- Wire into `AiPanel::submit`: under `#[cfg(feature = "ai")]`, pass the RAG-augmented
  response instead of the bare `NullAiBackend::query` stub.
- Integration test: end-to-end with a mock `EmbeddingBackend` + `GenerationBackend`.

### Step 6 — Semantic bookmarks (`§12.8`)

- Extend `crates/storage/src/bookmarks.rs:36` (`Bookmark` struct):
  - Add `pub summary: Option<String>` and `pub embedding: Option<Vec<u8>>` (f32 blob).
  - SQL schema: `ALTER TABLE bookmarks ADD COLUMN summary TEXT` and
    `ALTER TABLE bookmarks ADD COLUMN embedding BLOB` (both nullable; applied on first
    open via `IF NOT EXISTS` check or schema version). Existing rows stay valid.
- Extend `Bookmarks::save(url, title, folder, tags, summary, embedding)` to accept the
  new fields (or add a `set_semantic(id, summary, embedding)` method).
- In `lumen-shell`, when the user adds a bookmark for a loaded page (`Ctrl+D` or
  equivalent) and `#[cfg(feature = "ai")]`:
  - Extract the visible page text (already available via DOM).
  - Call `AiBackend::summarise(text)` → store as `summary`.
  - Call `AiBackend::embed(summary)` → store as `embedding` blob.
- Omnibox `@bookmarks` query: if `ai` feature is active and an embedding is available
  for the query term, perform cosine-similarity ranking alongside BM25.
- Fallback (no `ai`): save with `summary = None`, `embedding = None`; tag-based search
  unchanged.
- Tests:
  - `bookmarks.rs` unit: save with non-null summary/embedding → get round-trips correctly.
  - `bookmarks.rs` unit: save without summary/embedding (basic mode) → no schema error.

### Step 7 — Omnibox `@ai` prefix

- Add `@ai` as a recognised omnibox prefix in shell (alongside existing `@history`,
  `@tabs`, `@read-later` prefixes).
- When typed, route the query through `RagEngine::answer` and display the result in
  the omnibox dropdown as a single AI-answer row.
- Under `#[cfg(not(feature = "ai"))]`, the `@ai` prefix shows a "AI module not
  enabled" hint row.

---

## Privacy notes

All inference runs **on-device** (Ollama endpoint on `127.0.0.1` or `candle` in-process).
No browsing data leaves the machine. This is the core privacy invariant from `§12.5`.

- The Ollama backend connects only to `127.0.0.1:11434`. Any attempt to reconfigure
  it to a remote host must require an explicit user opt-in and a settings UI warning.
- Embeddings stored in `bookmarks.embedding` are opaque blobs — not human-readable —
  but represent semantic content of the page. Treat them with the same care as history.
- Plugin capability `"local-ai"` (already declared in
  [`crates/storage/src/plugins.rs:402`](../../crates/storage/src/plugins.rs)) gates
  WASM plugins from requesting embeddings/generation; enforce at the plugin API layer.

---

## Tests

| Test | Location | Scope |
|---|---|---|
| `null_backend_is_object_safe` (existing) | `crates/core/src/ext.rs:2944` | Compile-time object-safety of `AiBackend` |
| `null_backend_embed_returns_empty` | `crates/core/src/ext.rs` (add) | No-op embed stub |
| `ollama_embedding_backend_mock` | `crates/lumen-ai/src/embedding.rs` | HTTP mock; assert vector length |
| `rag_engine_integration` | `crates/lumen-ai/src/rag.rs` | Mock embed + generation; end-to-end |
| `semantic_bookmark_round_trip` | `crates/storage/src/bookmarks.rs` | Save + retrieve summary/embedding |
| `semantic_bookmark_nullable` | `crates/storage/src/bookmarks.rs` | Basic mode (no AI columns) still works |
| `knowledge_search_semantic` | `crates/knowledge/src/store.rs` | HNSW query returns expected hit |

---

## Definition of done

- [ ] `lumen-ai` crate compiles cleanly under `--features ai`; absent without it.
- [ ] `cargo check -p lumen-shell` passes without `ai` feature (basic bundle).
- [ ] `cargo check -p lumen-shell --features ai` passes.
- [ ] `cargo clippy -p lumen-ai -- -D warnings` clean.
- [ ] All new unit tests pass.
- [ ] `AiPanel` in the shell shows a RAG-augmented response when `ai` is enabled and
  Ollama is running (manual smoke test).
- [ ] Semantic bookmark: saving a bookmark with `ai` feature auto-populates `summary`.
- [ ] Schema migration is safe: opening an existing bookmark DB without `ai` does not
  fail or corrupt data.
- [ ] ADR logged for the embedding runtime decision.
- [ ] `CAPABILITIES.md` updated: `§12.5` and `§12.8` rows changed from ⬜ to ✅.
- [ ] `docs/plan/knowledge.md:78` cross-reference updated to reflect which backend
  landed.
