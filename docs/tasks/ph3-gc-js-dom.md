# Ph3 — GC integration JS ↔ DOM (cross-boundary cycle collection)

**Developer:** P1 + P4 · **Branch:** `p1-ph3-gc-js-dom` · **Size:** L · **Crates:** `lumen-dom`, `lumen-js`

---

## Status

**Phase 3 future item (v1.0).** Not yet started. Recorded here to preserve the
architecture and pre-computed entry points before Phase 3 begins. Roadmap source:
`docs/plan/phases.md:134` — *"GC integration JS ↔ DOM [P1+P4] — cycle collector
между Rust DOM и JS engine. Архитектурная задача при интеграции QuickJS / V8."*

**This pairs with the V8 migration** (`docs/tasks/ph3-v8-migration.md`). The correct
ordering is: settle the wrapper/identity model first (V8 migration introduces a real
`Object` wrapper class with embedder data + GC tracing callbacks), then build the
cycle collector on top of that model. Doing the cycle collector against the current
QuickJS bridge would mean designing for a wrapper mechanism that the V8 migration
then replaces. Do **not** start before Phase 2 closes and v0.5.0 ships, and ideally
not before `ph3-v8-migration.md` has landed its wrapper-class foundation.

This is honest, deep engine integration — not a bolt-on. The core difficulty is
reference cycles that span the Rust↔JS boundary, which neither collector can reclaim
alone (see "The problem").

---

## Goal

Reclaim object graphs that form a cycle crossing the Rust DOM ↔ JS engine boundary,
so that long-lived pages (SPAs that attach/detach DOM repeatedly) do not leak. The
canonical leak: a detached DOM node holds (transitively) a JS event-listener closure
that closes over a JS wrapper of that same node. The DOM side keeps the node alive
"because JS still references it"; the JS side keeps the closure alive "because the DOM
node still references it". Each collector, run alone, sees a live external reference
and refuses to collect — the cycle is immortal.

The deliverable is a tracing pass that treats the Rust DOM arena and the JS heap as
one graph: the JS GC can mark *into* the Rust DOM (following wrapper → node edges),
and the Rust side can enumerate node → JS-handle edges for the JS GC to follow back.
Cycles with no external root are then collectable.

---

## Current state

The DOM and JS engine are coupled today by a **by-value integer bridge**, not by
shared object handles. This is the most important fact for this task — it means the
cycle described above is *currently impossible to express*, and the wiring that would
make it possible is stubbed but inert.

### DOM ownership model — arena, not Rc

- `crates/engine/dom/src/lib.rs:1-8` — module invariant: the whole node graph lives
  in a contiguous `Vec<Node>` arena addressed by `NodeId(u32)`. **No `Rc<RefCell<…>>`
  exists in the graph**; `#![deny(clippy::rc_buffer)]` enforces it (`lib.rs:11`).
- `crates/engine/dom/src/lib.rs:97` — `pub struct NodeId(u32)`.
- `crates/engine/dom/src/lib.rs:193-196` — `pub struct Node { parent: Option<NodeId>,
  children: Vec<NodeId>, data: NodeData }`. Parent/child edges are plain indices.
- `crates/engine/dom/src/lib.rs:922-978` — `pub struct Document` owns `nodes: Vec<Node>`,
  `root: NodeId`, plus side maps (`shadow_roots`, `template_contents`).
- Consequence: the arena is **append-only**. `dead_node_ids()` *identifies* collectable
  nodes but does not free them; compaction is explicitly deferred to Phase 3
  (`lib.rs:1430-1434`). There is no per-node Rust refcount that JS can touch directly —
  only the `js_refs` side map below.

### Existing GC hooks — stubbed, inert

- `crates/engine/dom/src/lib.rs:955-964` — `js_refs: HashMap<NodeId, u32>` —
  "Counts live JS wrapper objects referencing each `NodeId`." `#[serde(skip)]` (JS
  objects don't survive hibernation). Doc comment says the finalizer wiring is
  **"Phase 3: P3 wires the finalizer callback"** — i.e. not done.
- `crates/engine/dom/src/lib.rs:1361-1365` — `acquire_js_ref(node_id) -> u32`. Doc:
  *"P3 integration point: invoke from `lumen-js` when allocating a QuickJS object whose
  `_nid` property is set for the first time."* **No caller exists** (grep
  `acquire_js_ref` in `crates/js` → zero hits).
- `crates/engine/dom/src/lib.rs:1379-1389` — `release_js_ref(node_id) -> u32`. Doc:
  *"invoke from the `rquickjs` class finalizer registered for DOM wrapper objects."*
  **No such finalizer / class is registered** (see JS side below). Also no caller.
- `crates/engine/dom/src/lib.rs:1395-1397` — `js_ref_count(node_id) -> u32`.
- `crates/engine/dom/src/lib.rs:1408-1422` — `is_detached(node_id) -> bool`.
- `crates/engine/dom/src/lib.rs:1424-1435+` — `dead_node_ids() -> Vec<NodeId>` =
  detached **and** zero JS refs. The intended Phase-2 contract.
- JS-engine-side GC tuning already exists from the merged `p1-js-gc-per-tier` task
  (cross-ref below): `crates/js/src/lib.rs:2038-2053` `run_gc_pass(GcLevel)` calling
  `inner._rt.run_gc()` / `set_gc_threshold(…)`, levels in
  `crates/js/src/gc_policy.rs:11-45`. This is *per-tier heap tuning*, **not** a cycle
  collector and **not** boundary-aware.

### JS wrapper mechanism — fresh plain object per access, holds only an integer

- `crates/js/src/dom.rs:4063-4082+` — `_lumen_make_element(nid)` builds a **brand-new
  plain JS object every call** with `__nid__: nid` (a u32) plus accessor closures that
  call native functions keyed by `nid`. There is **no stable wrapper identity** —
  `el === el` only holds within one expression; two `getElementById` calls return
  distinct objects. Canvas etc. follow the same `__nid__` pattern (`canvas2d.rs:13`).
- There is **no `rquickjs` class registration, no `Trace`/`JsLifetime` impl, no
  `with_finalizer`, no `Class::…`** anywhere in `crates/js/src` (grep for
  `impl Trace`, `#[rquickjs::class]`, `finalizer`, `class_id` → zero structural hits;
  only doc comments and a `:fullscreen` false-positive at `dom.rs:5249`).
- The Rust side therefore holds **no `Persistent` handle to any JS object** for DOM
  wrappers. The boundary is one-directional and value-typed: JS → native call carrying
  a `u32`. Rust never retains a JS callback for a DOM node.

### Where the cycle source (event listeners) actually lives — JS side, keyed by nid

- `crates/js/src/dom.rs:3149-3177` — `_lumen_listeners` is a **JS-side object** keyed
  by `String(nid)+':'+type` → array of handler functions. `_lumen_add_listener`,
  `_lumen_rm_listener`, `_lumen_dispatch` all operate purely in JS.
- `crates/js/src/dom.rs:3524-3525`, `4398-4399` — element wrappers' `addEventListener`
  just call `_lumen_add_listener(nid, type, fn)`.
- There is also a JS-level `EventTarget` shim with a per-object `_listeners` map
  (`crates/js/src/dom.rs:2701-2741`).
- **Consequence:** today the listener closure is reachable from a global JS map
  (`_lumen_listeners`) keyed by an integer. The Rust DOM node does **not** hold the
  closure; it holds nothing JS at all. So removing a node from the arena does not drop
  its listeners (they stay in `_lumen_listeners` until `_lumen_rm_listener` or page
  teardown), and the JS closure stays alive because the global map roots it — a leak,
  but a *JS-internal* one, not yet a true cross-boundary cycle.

---

## The problem (cross-boundary cycles)

A reclaimable cross-boundary cycle requires three edges that, together, form a loop
spanning both heaps with **no external root**:

1. **JS wrapper → DOM node.** A JS object that the GC must treat as keeping a specific
   Rust `NodeId` alive (today: only an inert `js_refs` counter; no real edge).
2. **DOM node → JS value.** The Rust DOM node holding a retained JS handle —
   typically an event-listener closure or an expando property (e.g.
   `el.myData = {...}`). **This edge does not exist today** (listeners live in a JS
   global map keyed by nid, not on the node; there are no expandos on the Rust side).
3. **JS value → JS wrapper (closure capture).** The listener closure captures the
   element wrapper variable (`el.addEventListener('click', () => el.foo())`).

When all three exist and the node is detached from the document, the loop is:
`wrapper → node → listener-closure → wrapper`. The JS GC sees the wrapper referenced
(by the node, via edge 2) and won't collect it. The DOM GC (`dead_node_ids`) sees
`js_ref_count > 0` and won't collect the node. **Neither collector alone can break
the cycle — by construction each sees a live "external" reference that is in fact
internal to the cycle.** This is the classic browser DOM/JS leak that Blink solves
with Oilpan (unified heap + tracing) and Gecko historically solved with the XPCOM
cycle collector.

Why it is not yet reproducible in Lumen: edges 1 and 2 are not real. Wrappers are
value-typed (`__nid__` integer) and recreated per access; listeners are rooted in a
JS global map, not on the node. **The first job of this task is therefore to decide
*whether to introduce a real wrapper identity at all*** — because the current
integer-bridge design largely sidesteps cross-boundary cycles. If V8 migration adds a
stable wrapper class (likely, for correct `===` identity and `WeakRef`/expando
support), the cycle becomes real and must be handled. If the integer bridge is kept,
the lighter "leak sweep" path (below) may suffice.

---

## Architecture

Two designs; the V8 migration outcome decides which is needed. **Settle the wrapper
model in `ph3-v8-migration.md` before committing to a path here.**

### Path A — leak sweep (keep value-typed bridge; lighter)

If wrappers stay value-typed (`__nid__` integers, no Rust-held JS handles, listeners
in a JS global map), there is **no true cross-boundary cycle** — only a *JS-internal*
leak: `_lumen_listeners` roots closures for nodes that were detached and will never
fire again. The collector reduces to a coordinated two-phase sweep:

- **P1 (DOM):** on idle GC tick, compute `dead_node_ids()` (detached + zero js_refs),
  and additionally expose **all detached `NodeId`s regardless of js_refs** so JS can
  prune their listener entries.
- **P4 (JS):** after the DOM hands over the detached-id set, delete the matching
  `_lumen_listeners[ String(nid)+':'+* ]` entries, then `run_gc()`. Closures lose
  their last root and are collected. Then the DOM frees the nodes (now zero js_refs)
  and compacts the arena.

This is not a real cycle collector; it is a boundary-coordinated mark of "no live
listener path → safe to drop both sides." Cheaper, no tracing into Rust required.

### Path B — true cross-boundary cycle collector (required if wrappers gain identity)

If V8 migration introduces a real wrapper `Object` with embedder data (the `NodeId`)
and supports expandos / node-held listeners (edge 2 becomes real), implement a
tracing collector that unifies the two heaps:

- **P1 (DOM wrapper hooks):** make every Rust-held JS edge enumerable. For each
  `NodeId`, expose the set of retained JS handles it owns (listener closures, expando
  values) via a tracer callback, e.g. `Document::trace_node_js_edges(node_id, &mut
  dyn FnMut(&JsHandle))`. Wire `acquire_js_ref` / `release_js_ref`
  (`lib.rs:1361/1379`) into the real wrapper allocation + finalizer so `js_refs`
  becomes accurate.
- **P4 (JS engine integration + cycle collector):** register the wrapper class with a
  **GC trace callback** (V8 `EmbedderRootsHandler` / `v8::TracedReference`, or
  QuickJS `JS_MarkFunc`). During GC mark, follow wrapper → `NodeId`, then ask the DOM
  to enumerate that node's JS edges (P1 hook), marking them reachable. Run the unified
  mark from real roots (JS globals, document tree). Anything unmarked on either side
  is a cross-boundary cycle → collect: drop JS objects, then `dead_node_ids` frees the
  Rust nodes. This is the Oilpan-style unified-heap approach adapted to a split heap
  via embedder tracing.

The split P1=hooks / P4=engine+algorithm matches the roadmap line.

---

## Team split (P1 / P4)

**P1 — DOM wrapper hooks (`lumen-dom`):**
- Wire `acquire_js_ref` / `release_js_ref` to the real wrapper lifecycle (only
  meaningful once P4/V8 provides a wrapper with a finalizer).
- Path A: expose detached-node-id enumeration (incl. nodes with live js_refs) for the
  listener sweep.
- Path B: add `trace_node_js_edges` (proposed) so the JS GC can mark into Rust; make
  every node→JS edge (listeners, expandos) enumerable.
- Implement arena free-list / compaction so freed `NodeId`s are actually reclaimed
  (currently deferred, `lib.rs:1430`).

**P4 — JS engine integration + cycle collector (`lumen-js`):**
- Decide wrapper identity model jointly with `ph3-v8-migration.md`.
- Path A: prune `_lumen_listeners` for detached ids, then `run_gc_pass`.
- Path B: register the wrapper class + GC trace callback; implement the unified
  mark/collect; call the P1 trace hook during marking.
- Extend `gc_policy` so the cycle pass runs on idle / T2 transitions alongside
  existing tier tuning.

---

## Cross-references

- **`docs/tasks/ph3-v8-migration.md`** — **blocking dependency / pairing.** The
  wrapper-identity model V8 establishes decides Path A vs Path B. Do the wrapper-class
  foundation there first.
- **`docs/tasks/p1-js-gc-per-tier.md`** (merged) — provides `run_gc_pass(GcLevel)` and
  `gc_policy::GcLevel` (per-tier heap *tuning*). This task **builds on** it but is a
  different concern (cycle reclamation, not threshold tuning). Do not duplicate the
  tier machinery; call into it.

---

## Entry points (real file:line; "proposed" = to be created)

- `crates/engine/dom/src/lib.rs:955` — `js_refs` map (wire it for real).
- `crates/engine/dom/src/lib.rs:1361` — `acquire_js_ref` (find the first real caller).
- `crates/engine/dom/src/lib.rs:1379` — `release_js_ref` (call from finalizer).
- `crates/engine/dom/src/lib.rs:1395` — `js_ref_count`.
- `crates/engine/dom/src/lib.rs:1408` — `is_detached`.
- `crates/engine/dom/src/lib.rs:1424` — `dead_node_ids` (collection gate).
- `crates/engine/dom/src/lib.rs:1430` — deferred arena compaction note (implement here).
- `crates/js/src/dom.rs:4063` — `_lumen_make_element` (wrapper construction site;
  where a real wrapper class would replace the plain object).
- `crates/js/src/dom.rs:3149-3177` — `_lumen_listeners` store (Path A prune target;
  Path B edge-2 source if listeners move onto nodes).
- `crates/js/src/lib.rs:2038` — `run_gc_pass` (extend with cycle pass).
- `crates/js/src/gc_policy.rs:11` — `GcLevel` (add a cycle-collect trigger if needed).
- **Proposed** `Document::trace_node_js_edges(node_id, visitor)` in
  `crates/engine/dom/src/lib.rs` — Path B tracer hook.
- **Proposed** `Document::detached_node_ids() -> Vec<NodeId>` (all detached, ignoring
  js_refs) in `crates/engine/dom/src/lib.rs` — Path A sweep input.
- **Proposed** wrapper-class registration + GC trace callback in `crates/js/src/dom.rs`
  (Path B) — depends on V8 migration's wrapper class.

---

## Steps

1. **Gate.** Confirm Phase 2 closed (v0.5.0 shipped) and `ph3-v8-migration.md`'s
   wrapper-class foundation has landed (or explicitly decide to stay on the integer
   bridge → Path A). Read both task files.
2. **Reproduce the leak (write the failing test first).** Author a JS snippet:
   create element, `addEventListener('click', closure_capturing_el)`, detach it, drop
   all JS references, force GC. Assert via instrumentation that the node + closure are
   *not* reclaimed today. This pins down which path applies (if nothing leaks under
   the integer bridge, Path A; if it does once V8 wrappers exist, Path B).
3. **P1: wire the refcount.** Make `acquire_js_ref`/`release_js_ref` accurate against
   the real wrapper lifecycle. Add `detached_node_ids()` (Path A) and/or
   `trace_node_js_edges` (Path B).
4. **P4: implement the chosen collector.** Path A: prune `_lumen_listeners` +
   `run_gc`. Path B: register wrapper class + trace callback + unified mark/collect.
5. **P1: arena reclamation.** Implement free-list/compaction so `dead_node_ids` →
   actual `Vec<Node>` slot reuse (NodeId stability rules documented).
6. **Wire the trigger.** Run the cycle pass on idle / T2 via `gc_policy`, reusing the
   per-tier machinery, not duplicating it.
7. Update `CAPABILITIES.md` (JS/DOM GC row), `subsystems/dom.md`, `subsystems/js.md`,
   `SYMBOLS.md`. Doc-comment every new pub item.

## Tests (leak detection)

- **Cycle reclamation:** the step-2 repro, after a full GC pass, asserts the node is
  in `dead_node_ids()` / freed and the closure is collected (instrument via a
  finalize counter or heap-object count delta from `heap_snapshot`).
- **No false free:** an *attached* node with a live listener is **never** collected;
  a detached node still referenced by a live (reachable) JS variable is **never**
  collected.
- **Path A sweep:** after detaching N nodes with listeners and one GC tick,
  `_lumen_listeners` has no entries for those nids and `js_ref_count` is 0.
- **Refcount accuracy:** `acquire_js_ref`/`release_js_ref` balance to 0 after wrappers
  are GC'd (requires the real finalizer; gate behind V8 wrapper class).
- **Arena compaction:** freeing nodes reuses slots without dangling `NodeId`s into
  live nodes; existing arena invariants (`lib.rs:1-8`) still hold.
- **Stress/no-leak:** attach/detach 10k nodes-with-listeners in a loop; heap-object
  count returns to baseline after GC (regression guard for the leak).

## Definition of done

- [ ] Wrapper-identity model decided jointly with `ph3-v8-migration.md`; Path A or B
      chosen and recorded in this file.
- [ ] The step-2 leak repro is reclaimed after a GC pass (was leaking before).
- [ ] `acquire_js_ref`/`release_js_ref` wired to the real wrapper lifecycle and
      balance to zero in tests (or Path A: `_lumen_listeners` pruned for detached nids).
- [ ] No false collection: attached-with-listener and detached-but-JS-reachable nodes
      survive GC (tests green).
- [ ] Arena free-list/compaction implemented; `dead_node_ids` results are actually
      reclaimed; arena invariants intact.
- [ ] Cycle pass triggered via `gc_policy` on idle/T2 without duplicating tier tuning.
- [ ] `cargo clippy -p lumen-dom --all-targets -- -D warnings` and
      `-p lumen-js` clean; `cargo test -p lumen-dom` and `-p lumen-js` pass.
- [ ] Docs updated: `CAPABILITIES.md`, `subsystems/dom.md`, `subsystems/js.md`,
      `SYMBOLS.md`; all new pub items doc-commented.
