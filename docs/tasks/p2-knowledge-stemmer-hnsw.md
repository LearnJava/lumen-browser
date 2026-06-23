# P2-knowledge — Russian Porter stemmer + HNSW vector index

**Developer:** P1
**Branch:** `p1-p2-knowledge-stemmer-hnsw`
**Size:** L (two independent sub-parts)
**Crates:** `lumen-knowledge`

## Goal

The knowledge-layer **core is already shipped** (F2-5 ✅ 2026-06-22): FTS5 full-text
search over history/notes/read-later, the live `OpenTabsIndex`, focus mode + Pomodoro,
and the `@read-later` / `@tabs` omnibox prefixes. What remains are two **optional ⬜**
quality improvements that were explicitly carved out of "core":

1. **Part A — Russian Porter stemmer.** Improves full-text *recall* by matching
   morphological variants of a word (e.g. query `статья` matching `статьи`, `статью`,
   `статьям`). No machine learning involved — a self-contained string algorithm.
2. **Part B — HNSW vector index.** A semantic / similarity index over notes and history
   so that "find me things *like* this" works even when the wording differs. This is the
   larger piece and is **blocked on an unresolved prerequisite** — Lumen has no embedding
   model (see Part B "Open question").

The two parts are independent: Part A can ship without Part B and vice versa. Recommended
order is A first (smaller, tractable, no prerequisite), B second (or deferred).

The library header already flags both as future work — see
[`crates/knowledge/src/lib.rs:5-8`](../../crates/knowledge/src/lib.rs):

```
//! custom-tokenizer для ё↔е equivalence и русского Porter-stemmer —
//! отдельная задача в Phase 2 (FTS5 supports external tokenizers через
//! C-callback, нам пока хватает дефолтного unicode61).
```

---

## Part A — Russian Porter stemmer

### Current state

All four FTS5 tables are created with the **built-in `unicode61` tokenizer** and no
stemming:

| Table | File:line | Tokenizer | Indexed columns |
|---|---|---|---|
| `history_fts` | [`fts.rs:71-74`](../../crates/knowledge/src/fts.rs) | `unicode61 remove_diacritics 2` | `url, title, text` |
| `notes_fts` | [`notes.rs:79-84`](../../crates/knowledge/src/notes.rs) | `unicode61 remove_diacritics 2` | `selection, comment` (external content over `notes`) |
| `read_later_fts` | [`read_later.rs:124-128`](../../crates/knowledge/src/read_later.rs) | `unicode61 remove_diacritics 2` | (see file) |
| `open_tabs_fts` | [`open_tabs.rs:72-74`](../../crates/knowledge/src/open_tabs.rs) | `unicode61 remove_diacritics 2` | (live, no disk persistence) |

`unicode61` splits on Unicode word boundaries, lowercases, and (with
`remove_diacritics 2`) strips combining marks. It does **not** reduce a word to its stem,
so `статья` and `статьи` are distinct tokens and a search for one misses the other.

The write path (`history_fts`) is a plain Rust insert at
[`fts.rs:87-108`](../../crates/knowledge/src/fts.rs) (`HistoryFts::index`), and the query
path is at [`fts.rs:129-164`](../../crates/knowledge/src/fts.rs) (`HistoryFts::search`).
These are the two places a stemmer must hook so that *indexed terms* and *query terms* are
stemmed with the **same** function (otherwise they never match).

**Two architectural options for *where* the stemmer lives — pick one and document it:**

- **Option A1 — application-side stemming (recommended, no C, no new dep).** Stem in Rust
  *before* writing to FTS5 and *before* building the MATCH query. Keep tokenizer as
  `unicode61`. The index stores stems; queries are stemmed the same way; default FTS5
  matching then works unchanged. Pros: pure Rust, no SQLite C-callback, works on the
  `bundled` build with zero feature flags. Cons: the stored `text` column becomes stemmed
  (snippets would show stems) — so store a **separate stemmed column** for matching and
  keep the original `text` for `snippet()`, or accept stemmed snippets. **Caveat:** the
  `notes_fts` table is an *external-content* FTS5 table populated by SQL triggers
  ([`notes.rs:87-100`](../../crates/knowledge/src/notes.rs)) that copy raw `selection` /
  `comment` columns. Application-side stemming there requires either dropping the triggers
  in favour of manual Rust inserts, or adding stemmed shadow columns to the `notes` base
  table. Note this in the implementation.
- **Option A2 — FTS5 external tokenizer via C-callback.** Register a custom tokenizer that
  stems each token. Pros: stemming is transparent to every table, snippets stay original,
  triggers keep working. Cons: requires `unsafe` FFI into the SQLite C API
  (`fts5_api` / `xCreateTokenizer`), which conflicts with the project's "own code =
  transparency" principle and the long-term tantivy migration goal
  ([`docs/plan/tech-stack.md:42`](../../docs/plan/tech-stack.md)). Heavier; only choose if
  A1's external-content caveat proves unworkable.

**Dependency note.** The Russian Porter (Snowball) algorithm is a fixed, well-specified
string transform — **hand-roll it, do not add a crate.** A new dependency would need the
justification block required by `CLAUDE.md` ("No new dep without justification"), and a
~150-line pure-Rust function does not warrant one. Place it in a new module
`crates/knowledge/src/stemmer.rs`.

### Steps

1. Create `crates/knowledge/src/stemmer.rs` with `pub fn stem_ru(word: &str) -> String`
   implementing the Russian Snowball/Porter algorithm (RV/R2 regions; remove PERFECTIVE
   GERUND, REFLEXIVE, ADJECTIVAL, VERB, NOUN, then SUPERLATIVE / `ь` / double-`н`
   endings). Add `pub mod stemmer;` to [`lib.rs`](../../crates/knowledge/src/lib.rs).
2. Add a `stem_text(&str) -> String` helper that splits on Unicode word boundaries,
   stems each Cyrillic token (leave Latin/digits untouched), and rejoins — used by both
   the index and query paths so they stay symmetric.
3. **Index path:** in `HistoryFts::index` ([`fts.rs:87`](../../crates/knowledge/src/fts.rs))
   write the stemmed form. Per Option A1, add a separate `text_stem` (and optionally
   `title_stem`) column to the `history_fts` schema at
   [`fts.rs:71-74`](../../crates/knowledge/src/fts.rs); keep the original `text` for
   `snippet()`. MATCH against the stemmed column.
4. **Query path:** in `HistoryFts::search` ([`fts.rs:129`](../../crates/knowledge/src/fts.rs))
   run the query string through `stem_text` before constructing the FTS5 MATCH. Preserve
   FTS5 operators (`OR`, `"…"`, `^`) — only stem bare word tokens, not operators.
5. Apply the same insert+query stemming to `read_later` ([`read_later.rs:296`](../../crates/knowledge/src/read_later.rs))
   and `open_tabs` ([`open_tabs.rs:88,129`](../../crates/knowledge/src/open_tabs.rs)).
6. For `notes` ([`notes.rs`](../../crates/knowledge/src/notes.rs)): resolve the
   external-content/trigger caveat from Option A1 — either replace the AFTER INSERT/UPDATE
   triggers ([`notes.rs:87-100`](../../crates/knowledge/src/notes.rs)) with manual stemmed
   Rust inserts, or add stemmed shadow columns. Document the choice in the module header.
7. **Optional ё↔е normalization** (mentioned in the lib header): normalize `ё → е` inside
   `stem_text` so `ёлка` / `елка` collapse. Cheap, do it in the same pass.

### Tests

Add to `stemmer.rs` and the existing `#[cfg(test)] mod tests` in
[`fts.rs:178`](../../crates/knowledge/src/fts.rs):

- `stem_ru` unit cases: `статья/статьи/статью/статьям → статј` (same stem); verb forms
  `читать/читаю/читал → чита`; Latin words pass through unchanged; digits unchanged.
- Recall test mirroring `cyrillic_text_search` ([`fts.rs:258`](../../crates/knowledge/src/fts.rs)):
  index text containing `статья`, search `статьи`, assert one hit (fails today).
- Symmetry test: a query in nominative finds a document stored in a declined form and
  vice versa.
- ё↔е test (if implemented): index `ёлка`, search `елка`, assert a hit.
- Regression: existing tests in [`fts.rs:186-319`](../../crates/knowledge/src/fts.rs) must
  still pass (Latin `rust`, OR queries, limit, clear, overwrite).

---

## Part B — HNSW vector index

### Open question / prerequisite — RESOLVE BEFORE CODING

HNSW indexes *vectors*. Lumen has **no embedding model** — there is no built-in ML
runtime, no ONNX, no transformer weights, and bundling one (tens-to-hundreds of MB) would
contradict the "lightweight" identity of the browser. **The embedding source must be
decided first; HNSW is meaningless without it.** Lay the options out honestly to the user:

- **Option B1 — TF-IDF / hashing vectors (pure Rust, no model, recommended starting
  point).** Build sparse-then-densified bag-of-words vectors (TF-IDF over the stemmed
  vocabulary from Part A, or feature-hashing into a fixed dimension). Pros: zero new heavy
  deps, fully transparent, reuses the Part A stemmer. Cons: "semantic" similarity is only
  lexical — it finds documents sharing *words*, not *meaning*; synonyms won't match. This
  is closer to "more-like-this" than true semantic search. Honest framing: it is a
  meaningful upgrade over keyword FTS for clustering/related-notes, but is **not** an LLM
  embedding.
- **Option B2 — tiny local embedding model.** A small quantized sentence-embedding model
  via a Rust inference crate (e.g. `candle`/`ort`). Pros: real semantic vectors. Cons:
  large provisional dependency + model weights to ship/download, startup cost, audit
  surface — needs a full dependency-justification block and likely its own ADR. Probably
  **out of scope** for this task; flag as a follow-up.
- **Option B3 — defer.** Ship Part A only; leave HNSW as ⬜ until an embedding strategy is
  approved. Legitimate given there is no model today.

**Recommendation:** if Part B is pursued at all in this task, do **B1** (TF-IDF/hashing)
behind a trait so B2 can replace it later. Otherwise B3.

### Current state

Data that an index would embed already exists, all keyed by `id`/`rowid` for cross-linking:

| Source | Struct (file:line) | Embeddable fields |
|---|---|---|
| History | `SearchHit` / `HistoryWithFts::index_text` ([`history.rs:52`](../../crates/knowledge/src/history.rs)) | `url, title, text` |
| Notes | `Note` ([`notes.rs:21-23`](../../crates/knowledge/src/notes.rs)) | `selection`, `comment` |
| Read-later | `ReadLaterEntry` ([`read_later.rs:53-60`](../../crates/knowledge/src/read_later.rs)) | `url, title, text` |
| Open tabs | `OpenTabHit` ([`open_tabs.rs:36-43`](../../crates/knowledge/src/open_tabs.rs)) | live `url, title` (no persistence — likely skip) |

There is **no** vector store, embedding code, or HNSW crate in the workspace today
(verified: no `hnsw`/`instant-distance`/`usearch`/`candle`/`ort`/`ndarray` in
`Cargo.toml` or `Cargo.lock`). `lumen-knowledge` deps are only `lumen-core`,
`lumen-storage`, `rusqlite (bundled)` ([`Cargo.toml:10-13`](../../crates/knowledge/Cargo.toml)).

### Steps

1. **Resolve the prerequisite above with the user.** Do not write HNSW code until the
   embedding source (B1/B2/B3) is chosen. If B3, stop here and ship Part A only.
2. Define a trait anchor (so the embedding/index backend is swappable per
   [`docs/plan/tech-stack.md:42`](../../docs/plan/tech-stack.md) drop-in policy):
   `trait Embedder { fn embed(&self, text: &str) -> Vec<f32>; }` and
   `trait VectorIndex { fn add(&mut self, id: i64, v: &[f32]); fn nearest(&self, v: &[f32], k: usize) -> Vec<(i64, f32)>; }`.
   Place in `crates/knowledge/src/vector.rs`.
3. **Embedder (B1):** implement a TF-IDF / hashing embedder reusing `stem_text` from
   Part A for tokenization. Fixed dimension; L2-normalize so dot-product = cosine.
4. **HNSW index:** either hand-roll a minimal HNSW (greedy multi-layer graph,
   configurable `M` / `ef_construction` / `ef_search`) — substantial but pure Rust — or
   add a vetted pure-Rust crate (e.g. `instant-distance`, zero-`unsafe`) **with the full
   dependency-justification block** required by `CLAUDE.md` (category: provisional;
   trait-anchor: `VectorIndex`; graduation criterion: replace with tantivy/own index once
   FTS migrates). Document the decision.
5. **Persistence:** decide whether the index is rebuilt on startup from the existing FTS
   tables (simplest — embeddings are cheap with B1) or serialized to disk under the
   portable browser data dir. Rebuild-on-startup is recommended for B1.
6. **Surface:** add a `similar(id, k)` / `semantic_search(query, k)` method to
   `DefaultKnowledgeStore` ([`store.rs:33`](../../crates/knowledge/src/store.rs)) and,
   optionally, an omnibox prefix (e.g. `@similar`) mirroring the existing
   `@read-later` / `@tabs` prefixes. Shell wiring is **P3's** — describe the integration
   point in the commit body, do not edit `lumen-shell`.

### Tests

- `Embedder`: identical text → identical vector; disjoint texts → low cosine; overlapping
  vocabulary → higher cosine. Vectors are L2-normalized.
- `VectorIndex`: insert N known vectors, `nearest` returns the true top-k (compare against
  brute-force cosine on a small N for correctness).
- End-to-end on in-memory store: index three notes, query "more like note 1", assert note 1
  (or its lexical neighbour) ranks first.
- Honest negative-coverage note in the test module: with B1, pure synonym queries are
  expected to miss — document this so a future B2 swap has a clear target.

---

## Definition of done

**Part A (Russian Porter stemmer):**
- [ ] `stemmer.rs` with `stem_ru` + `stem_text`, pure Rust, no new dependency.
- [ ] Stemming applied symmetrically on both index and query paths for `history_fts`,
      `read_later_fts`, `open_tabs_fts`, and `notes_fts` (external-content caveat resolved
      and documented).
- [ ] Original text preserved for `snippet()` (or stemmed snippets explicitly accepted).
- [ ] Optional ё↔е normalization decided (done or noted as out-of-scope).
- [ ] Unit + recall tests pass; all pre-existing `fts.rs` tests still green.
- [ ] `cargo clippy -p lumen-knowledge --all-targets -- -D warnings`,
      `cargo test -p lumen-knowledge`.
- [ ] `CAPABILITIES.md` / `subsystems/knowledge.md` updated; lib-header note at
      [`lib.rs:5-8`](../../crates/knowledge/src/lib.rs) updated to reflect shipped stemmer.

**Part B (HNSW vector index):**
- [ ] Embedding-source decision (B1/B2/B3) recorded with the user before coding; if B2 or
      a new crate, a dependency-justification block (and ADR if warranted) is written.
- [ ] `Embedder` + `VectorIndex` traits define a swappable backend (drop-in per tech-stack
      policy).
- [ ] B1 TF-IDF/hashing embedder + HNSW (or `instant-distance`) implemented, reusing the
      Part A stemmer for tokenization.
- [ ] `similar` / `semantic_search` exposed on `DefaultKnowledgeStore`; shell integration
      point described in the commit body for P3 (no `lumen-shell` edits here).
- [ ] Embedder/index/end-to-end tests pass, including the honest synonym-miss note.
- [ ] `cargo clippy -p lumen-knowledge --all-targets -- -D warnings`,
      `cargo test -p lumen-knowledge`.
- [ ] Docs updated; ⬜ → ✅ (or ⬜ → 🟡 if B1-only) in `CAPABILITIES.md`.

**Note:** Part A and Part B can be merged independently — do not block A on B.
