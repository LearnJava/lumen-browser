# ADR-019: lumen-ai embedding/generation backend — Ollama HTTP first, candle deferred

## Status

Accepted

## Date

2026-07-15

## Context

`docs/plan/knowledge.md` §12.5 calls for an optional local AI layer (embeddings,
summarisation, RAG over the user's own history/notes/read-later) and §12.8 for
semantic bookmarks. The design left the embedding/generation runtime open
(`docs/tasks/ph3-ai-module.md` §"Open question"), listing four options: Ollama
HTTP API, `candle` (pure-Rust, in-process), `llama.cpp` FFI, and a remote cloud
API.

The core privacy invariant (§12.5) rules out remote APIs outright — browsing
history must never leave the device. That leaves a choice between an
external local process (Ollama) and an in-process ML runtime (`candle` or
`llama.cpp` FFI).

`lumen-ai` is a brand new, feature-flagged crate — nothing outside it may
depend on an ML library, and the basic bundle (default build, no `ai`
feature) must stay exactly as it is today: no new heavy deps, no binary size
growth, `NullAiBackend` unchanged.

## Decision

Implement the embedding/generation pipeline (`EmbeddingBackend` /
`GenerationBackend` traits) against an abstract trait first, and ship the
**Ollama HTTP backend** (`POST http://127.0.0.1:11434/api/embeddings` and
`/api/generate`) as the only concrete backend for the first slice of this
task. `candle` stays a documented future option behind its own Cargo feature
(`candle = ["dep:candle-core", "dep:candle-transformers"]`) and is not
implemented in this slice — the trait boundary is designed so it can be
added later without touching `RagEngine` or any consumer code.

Rationale for Ollama first:

- Zero new compile-time ML dependency, and zero new HTTP-client dependency:
  Ollama's REST API is plain HTTP on `127.0.0.1` (no TLS), so the `ollama`
  feature talks to it over a `std::net::TcpStream` with hand-rolled
  HTTP/1.1 request/response framing — no `reqwest`/`hyper` needed. JSON
  (de)serialization reuses `serde`/`serde_json`, already a permanent
  workspace dependency (`crates/js`, `crates/driver`, `crates/mcp`).
- Lets the rest of the pipeline (RAG engine, semantic bookmarks, omnibox
  `@ai` prefix) be built and tested against a stable trait surface before
  committing to an in-process runtime and its build/binary-size cost.
- `candle` remains the target for a fully self-contained deployment (no
  external process required) and can be wired in behind the same traits
  without an API change for consumers — this ADR intentionally defers that
  decision rather than rejecting it.

The Ollama backend connects only to `127.0.0.1` by construction (hardcoded
host, no user-supplied URL in this slice). If the user does not have Ollama
running, `EmbeddingBackend`/`GenerationBackend` calls return an error and
`AiPanel`/omnibox `@ai` fall back to the existing `NullAiBackend` stub
message — the basic bundle must not regress when `ai` is enabled but Ollama
is absent.

## Alternatives considered

| Alternative | Why rejected (for this slice) |
|---|---|
| `candle` (in-process, pure Rust) | Adds a heavy compile-time ML dependency (`candle-core`, `candle-nn`, `candle-transformers`) and GGUF model bundling before the rest of the pipeline (RAG, semantic bookmarks) has even been built against a stable trait. Deferred, not rejected — see Consequences/Future. |
| `llama.cpp` via FFI | C FFI (`unsafe`), platform-specific build complexity, hard to cross-compile — worse cost/benefit than `candle` for the same "in-process" goal. |
| Remote cloud API | Violates the §12.5 privacy invariant outright — browsing history would leave the device. Not acceptable as a primary or fallback path. |

## Consequences

- **Positive:** `lumen-ai` ships with zero new ML compile-time dependencies;
  basic bundle (`cargo check -p lumen-shell`, no `ai` feature) is completely
  unaffected. The `EmbeddingBackend`/`GenerationBackend` trait boundary lets
  the RAG engine, semantic bookmarks, and omnibox `@ai` prefix be built and
  tested with a mock backend, independent of which runtime ships later.
- **Negative / trade-offs:** the `ai` feature is only useful to users who
  already run Ollama locally — this is a real gap for a "batteries included"
  AI feature, but is an acceptable Phase 3 starting point per the brief.
  `getPredictedEvents`-style stub behavior does not apply here, but
  similarly: without Ollama running, `ai`-enabled builds silently degrade to
  `NullAiBackend` output rather than erroring loudly — acceptable because
  the basic (non-`ai`) bundle never observes this at all.
- **Future:** graduation trigger for adding the `candle` backend — once the
  Ollama-backed pipeline (RAG engine + semantic bookmarks) is functionally
  complete and has test coverage, wire `CandleBackend` behind the existing
  traits for users who want a self-contained, no-external-process build.
  No consumer-facing API changes are expected at that point.
