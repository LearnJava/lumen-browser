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

## Deferred

- Step 2 — `EmbeddingBackend` trait + `OllamaEmbeddingBackend`
  (`POST 127.0.0.1:11434/api/embeddings` over a hand-rolled HTTP/1.1
  `TcpStream` client, no `reqwest`/`hyper` — see ADR-019).
- Step 3 — semantic search over history/notes. **Blocked on an HNSW index in
  `lumen-knowledge`, which does not exist yet** (the brief's referenced
  prerequisite doc `p2-knowledge-stemmer-hnsw.md` is absent from the repo).
  Per the brief's Step 0, proceed with a mock/linear-scan search interface
  first and swap in a real HNSW index later — do not implement HNSW as part
  of this crate.
- Step 4 — `GenerationBackend` trait + `OllamaGenerationBackend` (summarisation).
- Step 5 — `RagEngine`, wired into `AiPanel::submit`.
- Step 6 — semantic bookmarks: `Bookmarks` schema extension
  (`summary`/`embedding` nullable columns), auto-summarise + embed on save.
- Step 7 — omnibox `@ai` prefix routed through `RagEngine::answer`.

## Invariants

- The basic bundle (no `ai` feature) must never observe any behavior change —
  `NullAiBackend`'s `embed`/`summarise` return empty via the trait's default
  impls, `cargo check -p lumen-shell` (no `ai`) must stay green.
- No crate outside `lumen-ai` may import an ML dependency.
- The Ollama backend connects only to `127.0.0.1:11434`, hardcoded — no
  user-configurable remote host in this slice (§12.5 privacy invariant).
