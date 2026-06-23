# Ph4 — Knowledge graph visualization

**Developer:** P1 · **Branch:** `p1-ph4-knowledge-graph` · **Size:** L
**Crates:** `lumen-knowledge`, `lumen-shell` (new panel)

---

## Status

**Phase 4 — after 1.0. Do not start before Phase 3 features land.**

Listed in `docs/plan/phases.md:145` under "Phase 4 — After 1.0":

> **Граф знаний (§12.9)** — визуализация коллекции.

Full design intent: `docs/plan/knowledge.md:132–144` (§12.9).

---

## Goal

Build an interactive **knowledge graph** over the user's collected data: browsing
history, notes, bookmarks, and read-later pages. Nodes are pages / notes; edges
express structural relationships (shared tags, shared domain, note-to-source-URL
citation) and, when the HNSW vector index is available (Phase 3+, see Dependencies),
optional semantic-similarity edges. The graph renders in a dedicated shell panel with
force-directed layout, pan/zoom, click-to-navigate, and filters by date / tag /
content type.

This turns a passive archive into an explorable knowledge map — "what do I know
about X", "which sources are authoritative on Y", "which topics have I neglected
lately".

---

## Current state

### Knowledge entities that become graph nodes

All entities are already persisted and indexed in `lumen-knowledge` (Phase 2, ✅):

| Entity | Struct | File:line | Graph node key |
|---|---|---|---|
| History entry | `SearchHit` | `crates/knowledge/src/fts.rs:28` | `url` |
| Note | `Note` | `crates/knowledge/src/notes.rs:21` | `(id, url)` |
| Read-later page | `ReadLaterEntry` | `crates/knowledge/src/read_later.rs:53` | `url` |
| Bookmark | `Bookmark` | `crates/storage/src/bookmarks.rs:36` | `url` |
| Open tab (live) | `OpenTabHit` | `crates/knowledge/src/open_tabs.rs:36` | `tab_id + url` |

Aggregate store: `DefaultKnowledgeStore` at `crates/knowledge/src/store.rs:33`.

### Relationship data that becomes graph edges

**Tags (structural, available today):**

- `ReadLaterEntry.tags: Vec<String>` — persisted in `read_later_tags` table
  (`crates/knowledge/src/read_later.rs:65`, schema at line 114–122). Two pages
  sharing a tag get an edge with `weight = 1 / tag_frequency`.
- `Bookmark.tags: Vec<String>` — persisted in `bookmark_tags` table
  (`crates/storage/src/bookmarks.rs:43`, schema at lines 16–20). Same tag-sharing
  logic applies.
- Notes do **not** have a `tags` field today — only `url`, `selection`, `comment`.
  Tag support for notes is a proposed addition (see Steps §5 below).

**Domain co-citation (structural, derivable today):**

- Two nodes share a domain when `url_domain(a) == url_domain(b)`. Edge type:
  `SameDomain`, weight 0.5. No new storage — computed at graph-build time by
  extracting `host` from each URL.

**Note-to-source citation (structural, available today):**

- Every `Note` carries `note.url: String` pointing to its source page
  (`crates/knowledge/src/notes.rs:27`). Each note node gets an edge to its source
  URL node with type `NoteOf`, weight 1.0.

**Semantic similarity (optional, blocked on Phase 3 prerequisites):**

- Requires the HNSW vector index from `docs/tasks/p2-knowledge-stemmer-hnsw.md`
  Part B, and the embedding backend from `docs/tasks/ph3-ai-module.md`. Neither
  exists today — no `hnsw`/`candle`/`ort`/`ndarray` in the workspace. When
  available, a `similar(id, k)` call on `DefaultKnowledgeStore` (proposed in
  `p2-knowledge-stemmer-hnsw.md` Steps §6) returns nearest neighbours; those
  become `SemanticSimilarity` edges with cosine-distance weights.

### Panel render pattern (shell)

Panels are floating overlays registered in `crates/shell/src/panels/mod.rs:4-28`.
Each panel follows this pattern (verified from `history_panel.rs` and
`bookmark_panel.rs`):

- State struct (e.g. `HistoryPanel`) lives on the `Lumen` app state struct.
- `build_panel(&state, palette) -> DisplayList` emits `DisplayCommand`s using
  `lumen_paint::DisplayCommand` and `lumen_core::geom::Rect`.
- `hit_test(pos, state) -> Option<PanelAction>` classifies pointer events.
- Toggle via a keyboard shortcut registered in `lumen-shell`.
- Geometry constants (`PANEL_W`, `PANEL_H`, `ROW_H`, …) as `pub const f32` at the
  top of the module.
- `Palette` for theming: `crates/shell/src/panels/themes.rs`.

A graph panel is larger than existing list-panels and requires continuous
interaction (drag-to-pan, scroll-to-zoom). Two render paths are possible:

**Option R1 — Native panel (recommended for Phase 4):** The graph is drawn by
emitting `DisplayCommand::Line`, `DisplayCommand::Circle`, and
`DisplayCommand::Text` items into a `DisplayList`. Force-directed layout runs
CPU-side each frame; positions converge over ~60 iterations and are cached. Pan
offset and zoom scale are stored in the panel state. This keeps the graph inside
the existing shell render pipeline without a new dependency.

**Option R2 — Internal HTML page (`lumen://knowledge-graph`):** The graph renders
as a self-contained HTML page served by the `about:`/`lumen:` URL handler, using
SVG or Canvas 2D. Benefits: richer DOM interaction, can reuse the browser's own
rendering engine. Drawbacks: requires a running page context, JS execution, and
a stable internal API from the engine to the page for querying graph data.
Option R2 is viable but heavier — defer to Phase 4+ if R1 proves sufficient.

**Recommendation:** implement R1 first (pure `DisplayList`). If the interaction
model proves too limited, migrate to R2 in a follow-up.

---

## Architecture

```
lumen-knowledge (no new deps)
  └─ GraphBuilder (proposed, new module crates/knowledge/src/graph.rs)
        ├─ NodeId: enum { History(i64), Note(i64), ReadLater(i64), Bookmark(i64) }
        ├─ EdgeKind: enum { SharedTag(String), SameDomain, NoteOf, SemanticSimilarity(f32) }
        ├─ KnowledgeGraph { nodes: Vec<GraphNode>, edges: Vec<GraphEdge> }
        ├─ build(&DefaultKnowledgeStore) -> KnowledgeGraph
        │    — queries all four substores, deduplicates URL-nodes, builds
        │      tag/domain/note-citation edges; semantic edges optional via
        │      a trait argument (None → skip)
        └─ (proposed) trait SemanticEdgeSource { fn similar(id: NodeId, k: usize) -> Vec<(NodeId, f32)> }
              — satisfied by the HNSW index when available; NullSemanticEdgeSource otherwise

lumen-shell (new panel)
  └─ crates/shell/src/panels/knowledge_graph_panel.rs (proposed)
        ├─ KnowledgeGraphPanel { graph: KnowledgeGraph, positions: Vec<Vec2>,
        │                         pan: Vec2, zoom: f32, selected: Option<NodeId>,
        │                         filter: GraphFilter }
        ├─ GraphFilter { date_from, date_to, tags: Vec<String>,
        │                node_types: EnumSet<NodeKind> }
        ├─ fn build_panel(&self, palette: &Palette) -> DisplayList
        │    — emits circles (nodes), lines (edges), labels (titles);
        │      node colour by NodeKind; edge thickness by weight
        ├─ fn hit_test(pos, &self) -> Option<GraphAction>
        │    { enum GraphAction { SelectNode(NodeId), Pan(Vec2), Zoom(f32),
        │                         ToggleFilter(..), Navigate(String) } }
        ├─ fn step_layout(&mut self)
        │    — one iteration of a force-directed algorithm (Fruchterman–Reingold
        │      or Barnes–Hut; CPU-side, converges in ~60 steps from random seed)
        └─ Toggle: `Ctrl+Shift+G` (proposed; verify no conflict in shell keybindings)
```

**Force-directed layout algorithm (inside `knowledge_graph_panel.rs`):**

Standard Fruchterman–Reingold:
- Repulsion: O(N²) or Barnes–Hut tree for N > 500.
- Attraction along edges proportional to `weight * ln(distance / ideal_length)`.
- Temperature cooling each step.
- Positions stored as `Vec<Vec2>` in the panel state; recomputed only on graph
  rebuild (filter change, new data). During pan/zoom the layout is frozen.
- For large graphs (> 1000 nodes): cluster by domain first, lay out cluster
  centroids, then lay out nodes within each cluster independently.

**Export (optional, Phase 4+ follow-on):**

`docs/plan/knowledge.md:142` mentions export to Obsidian / Roam Research format.
Not part of this task — file as a separate follow-up once the graph itself lands.

---

## Dependencies

### Hard dependencies (must exist before this task)

| Item | Status | Task file |
|---|---|---|
| `DefaultKnowledgeStore` with history/notes/read-later/tabs | ✅ Phase 2 shipped | `crates/knowledge/src/store.rs:33` |
| `Bookmark` with tags | ✅ Phase 2 shipped | `crates/storage/src/bookmarks.rs:36` |
| Shell `DisplayList` + `DisplayCommand` | ✅ | `crates/paint/src/display_list.rs` |
| `Palette` theming | ✅ | `crates/shell/src/panels/themes.rs` |

### Soft dependencies (enhance the graph but not required for basic version)

| Item | Status | Task file | Role in graph |
|---|---|---|---|
| HNSW vector index + embedder | ⬜ Phase 3 prerequisite | `docs/tasks/p2-knowledge-stemmer-hnsw.md` Part B | `SemanticSimilarity` edges |
| AI module (`lumen-ai`) | ⬜ Phase 3 | `docs/tasks/ph3-ai-module.md` | Higher-quality embeddings for semantic edges |
| Note tags | ⬜ proposed addition | (this task, Step §5) | Tag-based edges involving notes |

The graph ships without semantic edges if HNSW/AI are not available. The
`SemanticEdgeSource` trait (see Architecture) defaults to `NullSemanticEdgeSource`
which returns no edges. When HNSW lands, a real impl replaces it without touching
the rest of the pipeline.

---

## Entry points

All items below marked **(proposed)** do not exist yet.

| Symbol / location | File:line | Notes |
|---|---|---|
| `DefaultKnowledgeStore` | `crates/knowledge/src/store.rs:33` | Query source for all node data |
| `ReadLaterEntry.tags` | `crates/knowledge/src/read_later.rs:65` | Edge source: shared tags |
| `read_later_tags` table | `crates/knowledge/src/read_later.rs:114` | SQL: `SELECT tag FROM read_later_tags WHERE entry_id = ?1` |
| `Bookmark.tags` | `crates/storage/src/bookmarks.rs:43` | Edge source: shared tags |
| `bookmark_tags` table | `crates/storage/src/bookmarks.rs:16` | SQL: `SELECT tag FROM bookmark_tags WHERE bookmark_id = ?1` |
| `Note.url` | `crates/knowledge/src/notes.rs:27` | Edge source: note-to-source citation |
| `crates/knowledge/src/graph.rs` | — | `KnowledgeGraph`, `GraphBuilder`, `NodeId`, `EdgeKind` **(proposed)** |
| `crates/knowledge/src/graph.rs` | — | `SemanticEdgeSource` trait **(proposed)** |
| `crates/shell/src/panels/knowledge_graph_panel.rs` | — | `KnowledgeGraphPanel`, `build_panel`, `hit_test`, `step_layout` **(proposed)** |
| `crates/shell/src/panels/mod.rs:28` | `crates/shell/src/panels/mod.rs:28` | Add `pub mod knowledge_graph_panel;` **(proposed)** |
| Shell `Lumen` app state | `crates/shell/src/main.rs` | Add `knowledge_graph_panel: KnowledgeGraphPanel` field **(proposed)** |
| Shell keybinding `Ctrl+Shift+G` | `crates/shell/src/main.rs` | Wire toggle **(proposed; verify no conflict)** |

---

## Steps

### Step 1 — `KnowledgeGraph` data model in `lumen-knowledge`

Create `crates/knowledge/src/graph.rs`:

- `NodeId` enum: `History(i64)`, `Note(i64)`, `ReadLater(i64)`, `Bookmark(i64)`.
- `GraphNode { id: NodeId, url: String, title: String, kind: NodeKind,
               created_at: i64, tags: Vec<String> }`.
- `EdgeKind` enum: `SharedTag(String)`, `SameDomain`, `NoteOf`,
  `SemanticSimilarity(f32)`.
- `GraphEdge { from: NodeId, to: NodeId, kind: EdgeKind, weight: f32 }`.
- `KnowledgeGraph { nodes: Vec<GraphNode>, edges: Vec<GraphEdge> }` with methods
  `node_count()`, `edge_count()`, `neighbours(NodeId) -> &[GraphEdge]`.
- `trait SemanticEdgeSource { fn similar(&self, id: &NodeId, k: usize) -> Vec<(NodeId, f32)>; }`
  with a `NullSemanticEdgeSource` no-op impl.
- Add `pub mod graph;` to `crates/knowledge/src/lib.rs`.

### Step 2 — `GraphBuilder` querying all stores

Still in `crates/knowledge/src/graph.rs`, implement:

```rust
pub struct GraphBuilder;
impl GraphBuilder {
    pub fn build(store: &DefaultKnowledgeStore, semantic: &dyn SemanticEdgeSource) -> KnowledgeGraph
}
```

- **History nodes:** `store.search_history("*", limit)` with a large limit, or
  add a `list_history(limit)` method to `DefaultKnowledgeStore` if `search_history`
  requires a non-empty query.
- **Note nodes:** `store.notes().recent(limit)` (`Notes::recent` at
  `crates/knowledge/src/notes.rs:204`).
- **Read-later nodes:** `store.read_later().list_by_status(ReadStatus::Unread, limit)`
  and `list_by_status(ReadStatus::Read, limit)` (`crates/knowledge/src/read_later.rs:272`).
- **Bookmark nodes:** requires exposing a `list_all(limit)` method on `Bookmarks`
  (`crates/storage/src/bookmarks.rs`) — add it if not present.
- **Deduplication:** use `url` as the canonical identity. If a URL appears in multiple
  sources (e.g. both history and bookmarks), merge into a single `GraphNode` with the
  union of their tags; edges point to the merged node.
- **Tag edges:** for each pair of nodes sharing a tag, emit `SharedTag(tag)` with
  weight `1.0 / tag_freq`. Aggregate by tag first (`HashMap<tag, Vec<NodeId>>`),
  then emit edges; skip tags appearing in only one node.
- **Domain edges:** group nodes by `url_domain(url)` (extract `host` via `url::Url`
  or a hand-rolled split on `://` and `/`). Emit `SameDomain` edges within each
  group, capped at `max_per_domain = 20` to prevent hub-and-spoke explosion for
  large domains.
- **Note-of edges:** for each `Note` node, emit `NoteOf` to the URL node it cites.
- **Semantic edges:** call `semantic.similar(&id, k=5)` for each node; emit
  `SemanticSimilarity(score)` edges. With `NullSemanticEdgeSource` this is a no-op.

### Step 3 — Force-directed layout in the panel

Create `crates/shell/src/panels/knowledge_graph_panel.rs`:

- `KnowledgeGraphPanel` state struct: holds `KnowledgeGraph`, `Vec<Vec2>` positions,
  `pan: Vec2`, `zoom: f32`, `selected: Option<NodeId>`, `GraphFilter`, and a
  `layout_converged: bool` flag.
- `fn step_layout(&mut self)`: one Fruchterman–Reingold iteration. Temperature
  decreases each call; mark `layout_converged` when temperature < epsilon. Cap at
  `MAX_LAYOUT_STEPS = 120`.
- `fn rebuild(&mut self, store: &DefaultKnowledgeStore)`: call `GraphBuilder::build`,
  reset positions to random inside the viewport, reset temperature.
- `fn build_panel(&self, palette: &Palette) -> DisplayList`:
  - Background fill (panel rectangle).
  - For each edge: `DisplayCommand::Line` with alpha proportional to `weight`;
    colour by `EdgeKind` (`SameDomain` = dim, `SharedTag` = accent, `NoteOf` = bright,
    `SemanticSimilarity` = purple).
  - For each node: `DisplayCommand::Circle` (or filled rect) scaled by `degree + 1`;
    colour by `NodeKind`. Clip label to `node_radius * 2` chars.
  - Selected node: highlight ring.
  - Overlay: filter chips row (top bar), legend (bottom-left), node info card (when
    selected).
- `fn hit_test(pos: Vec2, &self) -> Option<GraphAction>`: check node circles in order
  of decreasing `degree` (larger nodes are easier targets). Return `Pan` for drag on
  background, `Zoom` for scroll.

### Step 4 — Wire into shell

- Add `pub mod knowledge_graph_panel;` to `crates/shell/src/panels/mod.rs`.
- Add `knowledge_graph_panel: KnowledgeGraphPanel` field to the `Lumen` app state
  struct in `crates/shell/src/main.rs`.
- Initialise with `KnowledgeGraphPanel::new()` in the browser startup path.
- Add `Ctrl+Shift+G` toggle handler alongside existing panel toggles.
- In the `about_to_wait` / render loop: if the panel is visible and
  `!layout_converged`, call `step_layout()` and request a redraw.
- On panel open: if graph is stale (data changed since last build), call `rebuild`.
- In the event handler for `GraphAction::Navigate(url)`: open that URL in the
  current or new tab (reuse the existing tab-open path).

### Step 5 — (Proposed) Add tag support to `Notes`

Currently `Note` has no `tags` field (`crates/knowledge/src/notes.rs:21`).
Tag-based edges involving notes are only possible if notes carry tags.

Options:
- **Option A:** Add `tags: Vec<String>` to `Note` and a `note_tags` table
  (pattern matching `read_later_tags`). Implement `Notes::set_tags(id, &[String])`.
  Wire into the UI (note editor / context menu).
- **Option B:** Skip note tags for Phase 4. Notes still participate via `NoteOf`
  domain/citation edges. Tags can be added later without changing the graph model.

**Recommendation:** Option B for the initial implementation. Note that if Option A
is chosen, the `note_tags` table follows the same `(note_id, tag)` pattern as
`read_later_tags` at `crates/knowledge/src/read_later.rs:114-122`.

### Step 6 — Performance guard for large graphs

> Realistic upper bound: a heavy user with 10k history entries + 200 notes +
> 500 bookmarks + 300 read-later items = ~11k nodes before deduplication.
> Tag edges scale O(nodes × avg_tags²) which can blow up.

Mitigations (implement at Step 2):

- **Node cap per source:** default `HISTORY_LIMIT = 500`, `NOTES_LIMIT = 200`,
  `BOOKMARKS_LIMIT = 500`, `READ_LATER_LIMIT = 200`. Expose as `GraphFilter` fields.
- **Tag-edge cap:** emit at most `MAX_TAG_EDGES_PER_TAG = 30` edges per tag
  (pick nodes with highest visit_count / created_at recency).
- **Domain-edge cap:** `MAX_DOMAIN_EDGES = 20` (see Step 2).
- **Layout:** use Barnes–Hut approximation (quad-tree repulsion) when
  `node_count > 300`. A simple quad-tree is ~60 lines of Rust.

---

## Tests

### `crates/knowledge/src/graph.rs`

- `build_empty_store_returns_empty_graph`: `DefaultKnowledgeStore::open_in_memory()`,
  `GraphBuilder::build` returns 0 nodes and 0 edges.
- `history_nodes_appear_as_nodes`: index 3 history entries, build graph, assert 3
  `History` nodes.
- `shared_tag_creates_edge`: save 2 read-later entries with tag `"rust"`, build graph,
  assert at least one `SharedTag("rust")` edge between them.
- `note_of_edge`: add a note for URL `"https://example.com/"`, ensure history has that
  URL, build graph, assert a `NoteOf` edge from the note node to the URL node.
- `domain_edge`: index 3 history entries under `"https://rust-lang.org/X"`, assert
  `SameDomain` edges within the group.
- `url_deduplication`: add the same URL to both history and bookmarks, assert it
  produces a single `GraphNode`, not two.
- `null_semantic_source_no_edges`: `NullSemanticEdgeSource` produces zero
  `SemanticSimilarity` edges regardless of node count.
- `node_cap_respected`: index 600 history entries with `HISTORY_LIMIT = 500`, assert
  `graph.nodes.iter().filter(|n| matches!(n.id, NodeId::History(_))).count() <= 500`.

### `crates/shell/src/panels/knowledge_graph_panel.rs`

- `step_layout_moves_nodes`: build a 5-node graph, run `step_layout` 10 times, assert
  that positions differ from the random initial positions.
- `layout_converges`: run `step_layout` `MAX_LAYOUT_STEPS` times, assert
  `layout_converged == true`.
- `build_panel_nonempty`: build a 3-node graph, call `build_panel`, assert
  `DisplayList` is non-empty.
- `hit_test_node_selection`: place a node at a known position, `hit_test` at that
  position returns `GraphAction::SelectNode(..)`.
- `hit_test_background_pan`: `hit_test` at a position not covered by any node returns
  `GraphAction::Pan(..)` or `None`.

---

## Definition of done

- [ ] `crates/knowledge/src/graph.rs` with `KnowledgeGraph`, `GraphBuilder`,
      `NodeId`, `EdgeKind`, `SemanticEdgeSource` + `NullSemanticEdgeSource`.
      All unit tests pass.
- [ ] `GraphBuilder::build` populates history / notes / read-later / bookmark nodes
      with tag / domain / note-citation edges. URL deduplication works.
- [ ] `crates/shell/src/panels/knowledge_graph_panel.rs` with Fruchterman–Reingold
      layout, `build_panel` (DisplayList), `hit_test`, `rebuild`.
- [ ] Shell wiring: `Ctrl+Shift+G` toggle, `Navigate` action opens URL, layout steps
      run per frame while panel is open and `!layout_converged`.
- [ ] Semantic edges are absent but the `SemanticEdgeSource` trait is in place for a
      future HNSW drop-in.
- [ ] Performance guards (node caps, tag-edge cap, domain-edge cap) prevent runaway
      builds on large stores.
- [ ] `cargo clippy -p lumen-knowledge --all-targets -- -D warnings` clean.
- [ ] `cargo clippy -p lumen-shell --all-targets -- -D warnings` clean.
- [ ] `cargo test -p lumen-knowledge`, `cargo test -p lumen-shell` pass.
- [ ] `CAPABILITIES.md` §12.9 updated ⬜ → ✅.
- [ ] `subsystems/knowledge.md` and `subsystems/shell.md` updated.
- [ ] `SYMBOLS.md` regenerated (`python scripts/gen_symbols.py`).
