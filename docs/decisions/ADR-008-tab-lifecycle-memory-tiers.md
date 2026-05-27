# ADR-008: Tab lifecycle and memory tiers

## Status

Accepted

## Date

2026-05-27

## Context

Browser RAM consumption per tab is dominated by four subsystems (industry data 2026, Chrome with ~150-300 MB per typical tab):

| Subsystem | Share | Why heavy |
|---|---|---|
| JS heap | 40-50% | V8 keeps long-lived objects; heap fragmentation; closures retain DOM nodes via bindings. |
| Image decode cache | 20-30% | A `1920×1080` image = 8 MB RGBA. A typical content page references 20-50 images, all decoded eagerly. |
| GPU layer textures | 10-20% | Each stacking context becomes a GPU texture; off-viewport layers retained "in case of scroll-back". |
| DOM + style + layout trees | 15-20% | Computed style snapshot per element; layout boxes; bidi runs; line break opportunities. |

A user with 50 open tabs in Chrome routinely consumes 6-10 GB of RAM. Chrome's Memory Saver (2023) addresses this by suspending background tabs but is opt-in and conservative.

Lumen targets **dramatically lower per-tab footprint** (plan §14.1: <50 MB empty v0.1, <80 MB v1.0; <200 MB for 100 hibernated tabs at v0.1). Hitting these numbers requires **architecting tab lifecycle from the start**, not retrofitting it.

The key insight: a tab is not one thing with one RAM cost. It is **five distinct states** (tiers), each with its own RAM budget, transition triggers, and restore-time SLO. The browser must be designed so that each subsystem (DOM, JS, layout, paint, image cache, GPU) participates correctly in tier transitions.

Retrofit cost is asymmetric — some invariants are cheap to add later, others (DOM data structure, JS engine choice, layout statefulness) become 5-10× more expensive once code is built atop the wrong choice. This ADR is taken **before** Phase 1 finalizes those subsystems, precisely to avoid that cost.

## Decision

Adopt a **five-tier tab lifecycle model** (T0–T4) with explicit per-tier RAM budgets, transition triggers, restore SLOs, and **three structural invariants** on engine subsystems that make the model implementable.

### Tier model

```
T0 Active (foreground, visible)
   ~100-200 MB per tab
   │
   │ tab hidden
   ▼
T1 Background-recent (< 5 min hidden)
   ~30-60 MB per tab
   - JS execution paused (event loop quiet, no scheduled work runs)
   - JS heap retained intact
   - Image decode cache retained
   - Layout tree retained
   - GPU layer textures retained for off-screen tabs only if memory headroom > 50%
   │
   │ idle 5+ min OR OS memory pressure low
   ▼
T2 Background-old (5-30 min hidden)
   ~10-20 MB per tab
   - JS heap snapshot saved to disk (SQLite), heap freed in RAM
   - Image decode cache dropped (sources retained)
   - GPU layer textures fully dropped
   - Layout tree retained (cheap to keep; needed for scroll restore)
   - DOM retained
   │
   │ idle 30+ min OR OS memory pressure medium
   ▼
T3 Hibernated (> 30 min hidden, or memory pressure)
   ~50-200 KB per tab
   - DOM serialized to SQLite (compact arena snapshot + URL + scroll + form state)
   - In RAM: only TabMetadata (URL, title, scroll position, favicon handle)
   - Layout tree dropped
   - JS heap dropped (snapshot already on disk from T2; on resume, re-execute scripts)
   │
   │ tab closed by user
   ▼
T4 Closed-recoverable (history)
   0 RAM
   - Entry in session history + URL FTS index
   - Restorable via Ctrl+Shift+T or @history search
```

### Restore SLO (binding)

| Transition | Target time | What happens |
|---|---|---|
| T1 → T0 | ≤ 50 ms | Resume JS event loop, no re-paint needed |
| T2 → T0 | ≤ 200 ms | Restore JS heap from disk, re-decode visible images, re-upload GPU layers |
| T3 → T0 | ≤ 1500 ms | Deserialize DOM from SQLite, re-run scripts, full layout + paint |
| T4 → T0 | network-bound | Equivalent to fresh navigation |

These are user-visible SLOs. Each transition has its own benchmark in `lumen-bench` (RAM-axis subtask in 9G.3); regression > 20% on a transition is a release-blocker.

### Transition triggers

Tiers are not driven by a single timer. The transition rule is **OR-of-conditions**:

1. **Idle timeout** — configurable per user (defaults: T0→T1 immediate-on-hide; T1→T2 at 5 min; T2→T3 at 30 min).
2. **OS memory pressure** — `MemoryPressureSource` trait (`lumen-core::ext`, new) emits `Low / Medium / High` from OS-specific APIs (Win32 `QueryMemoryResourceNotification`; Linux `/proc/pressure/memory` PSI; macOS `dispatch_source_create(MEMORYPRESSURE)`). On `Medium` — accelerate T1→T2 to all background tabs older than 1 min. On `High` — force T2→T3 on all background tabs older than 5 min, regardless of timer.
3. **LRU within budget** — global RAM budget for renderer process (user-configurable, default 1 GB or 25% of system RAM, whichever smaller). When exceeded, oldest-touched background tab is demoted by one tier.
4. **User pin** — pinned tabs (§12.13) never transition past T1, even under memory pressure (acceptable trade-off; user explicitly opted in).

### Three structural invariants (binding on engine subsystems)

These invariants **must be enforced from the start**. Each one cited an ADR-008 dependency in its source file (e.g., `// ADR-008: DOM is arena, no Rc<RefCell> in node graph`).

#### Invariant 1 — DOM is a serializable arena, not an `Rc<RefCell>` graph

`lumen-dom` stores nodes in `Vec<Node>` keyed by `NodeId(u32)` (or `NonZeroU32` for niche optimization). Parent / child / sibling are `NodeId` indices, not `Rc<Node>` pointers. No interior mutability through `RefCell` in the node graph; mutations go through the arena's `&mut self` API.

**Consequence:** DOM can be serialized via `bincode::serialize(&arena)` for T3 hibernation. **No** retrofit of `Rc<RefCell>` → arena late in the project (which would be 8+ weeks of cross-crate refactor per the assessment in chapter discussion).

**Allowed exception:** JS↔DOM bindings need stable handles for callbacks; resolved via a `JsHandle ↔ NodeId` map in `lumen-js`, not by leaking `Rc<Node>` into the JS side.

#### Invariant 2 — JS runtime supports suspend / resume

`JsRuntime` trait in `lumen-core::ext` (defined in ADR-004) is extended with:

```rust
trait JsRuntime {
    // existing methods …

    /// Capture full execution state to a serializable snapshot.
    /// Pauses execution; existing handles become invalid until resume.
    fn suspend(&mut self) -> Result<SuspendedHeap, Error>;

    /// Restore execution state from a snapshot. New handles are issued.
    fn resume(snapshot: SuspendedHeap) -> Result<Self, Error>;

    /// Pause event loop without freeing heap (T0 → T1).
    fn pause(&mut self);

    /// Resume paused event loop (T1 → T0).
    fn unpause(&mut self);
}
```

QuickJS supports this naturally (`JS_WriteObject` / `JS_ReadObject` for serialization; explicit microtask queue control for pause/resume). V8 does NOT support full heap serialization out of the box. **This invariant locks JS engine choice to QuickJS for Phase 0-2 and requires careful evaluation before V8 migration in Phase 3** (re-evaluation criterion: V8 snapshot API + custom serializer can simulate `suspend()`; if not, V8 is incompatible with T2/T3 and ADR-004 must be revisited).

#### Invariant 3 — Layout and paint are pure functions of (DOM, stylesheet, viewport)

`lumen-layout::lay_out(&dom, &stylesheet, viewport) -> LayoutTree` has no hidden mutable state. No global counters, no `static` caches inside the layout pass, no animation tick state inside the layout function (animation state lives in `lumen-shell::runtime` and is passed in as part of stylesheet's resolved values).

`lumen-paint::display_list(&layout_tree, &viewport) -> DisplayList` similarly pure.

**Consequence:** T2 → T0 restore is just re-calling the function — no audit of hidden state needed at resume. **Anti-pattern guard:** PR introducing `static MUT`, lazy_static, or `OnceCell` in `lumen-layout` / `lumen-paint` is a blocker unless explicitly justified as cross-tab cache (with eviction policy).

**Allowed exception:** glyph atlas, font metrics cache, image decode cache — these are cross-tab caches living in their own crates (`lumen-font`, `lumen-image`), not inside layout/paint, and have explicit eviction APIs that participate in tier transitions.

### Memory budget enforcement

Each tier has a soft budget (target) and hard budget (PR-fail in bench):

| Tier | Soft (v0.1) | Hard (v0.1) | Soft (v1.0) | Hard (v1.0) |
|---|---|---|---|---|
| T0 simple page | 80 MB | 100 MB | 150 MB | 200 MB |
| T0 heavy page (Habr article) | 150 MB | 200 MB | 250 MB | 350 MB |
| T1 | 40 MB | 60 MB | 60 MB | 100 MB |
| T2 | 15 MB | 25 MB | 25 MB | 40 MB |
| T3 | 200 KB | 1 MB | 200 KB | 2 MB |

`lumen-bench` extended with RAM-axis (`peak_rss`, `steady_state`, `tier_transition_rss`) and CI gate (task 9G.3) fails PR on > 5% regression of `T0 simple page` median, > 5% regression of `T2` steady-state, or > 20% regression of any restore SLO.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| One uniform tab state (no tiers), rely on OS swap | OS swap is slow (HDD: seconds; SSD: ~100ms) and unpredictable. User scroll on a 50-tab browser stalls. Industry has moved away from this since 2023. |
| Two tiers only (Active / Hibernated) | Skips the cheap middle (T1 = paused-but-resident, ~50ms restore). Forces all backgrounded tabs to expensive full hibernation. Chrome Memory Saver actually has 3-4 internal tiers for the same reason; calling it "two tiers" hides complexity. |
| Process-per-tab as the only memory boundary | Process boundaries are great for security (site isolation, Phase 2) but heavy: each process has its own JS runtime, code pages duplication, IPC overhead. We need tiers within a process AND processes between origins. They are orthogonal mechanisms. |
| `Rc<RefCell>` DOM (Servo-style early) | Cannot serialize for T3 without invasive workarounds. Servo team has documented this regret. We avoid by choosing arena from day one. |
| V8 in Phase 0 | V8 does not support full heap serialization; T2/T3 become impossible without huge engineering effort. QuickJS first, V8 deferred to Phase 3 (ADR-004) is the right call for this reason, beyond just startup performance. |
| Tier transitions driven only by timer | Misses the OS-memory-pressure case (user opens a memory-heavy app, browser keeps hoarding RAM until OOM). Must combine timer + LRU + pressure. |

## Consequences

- **Positive:**
  - 50-tab Lumen targets ~400 MB total RAM (vs Chrome 6-10 GB) — order-of-magnitude difference, primary product differentiator alongside privacy.
  - Tier model gives a vocabulary for performance work: "this PR regresses T2 by 12%" is actionable; "this PR uses more RAM" is not.
  - Three structural invariants prevent the most expensive class of late-stage refactors (DOM, JS, layout architecture).
  - QuickJS-first choice (ADR-004) is now reinforced by a second independent reason: tier model requires it.
  - DOM arena requirement aligns with what `lumen-dom` is already moving toward (per existing code review); this ADR makes it formally binding.
  - Pure layout/paint enables future optimizations (incremental layout, off-thread layout, server-side rendering of HTML for static generation) without architectural debt.

- **Negative / trade-offs:**
  - Three subsystems carry ADR-008 constraints in their code. Cross-cutting concern, but each spot is small (one comment + one trait method).
  - JS migration to V8 in Phase 3 is now gated on supporting `suspend()` — if V8 cannot, ADR-004 must be reconsidered. Mitigated: V8 snapshot API + Heap object enumeration likely sufficient; investigation due before Phase 3 starts.
  - Tier transition logic is non-trivial code (~1500 LoC estimate in `lumen-shell::tab_lifecycle`) including the three-condition OR (timer / pressure / LRU). Mitigated: well-isolated module, can be unit-tested without full browser.
  - Restore time on T3 → T0 (~1.5s) is visibly slower than T1 → T0 (~50ms). Users may not understand why "some tabs are slow". Mitigated: UI affordance — show a "Z" or fade-icon on tabs in T2/T3 so user knows what to expect.
  - QuickJS suspend snapshot can be large (heap dump). T2 may use significant disk space per tab. Mitigated: zstd compression on snapshot; cap at 5 MB/tab disk (drop heap entirely past that, accept slower T3 → T0).

- **Future / graduation triggers:**
  - **V8 migration (Phase 3 / ADR-004 revisit):** before starting, prototype `JsRuntime::suspend()` on V8. If feasible in < 4 weeks of work, proceed; if not, defer V8 indefinitely.
  - **Sixth tier (T-1 ultra-active):** if user studies show frequent same-tab interaction patterns, a "T-1 hot" tier with pre-warmed JIT, pre-decoded above-fold images, GPU layers already uploaded — might be worth adding. Not v0.1 / v1.0 scope.
  - **Cross-tab page cache (bfcache):** §16 Phase 3 already names bfcache; tier model formalizes it as "navigation that puts current page in T2 with quick T2→T0 restore". Concrete design when bfcache PR opens.
  - **Android port (mentioned as out-of-scope earlier):** Android lifecycle (onPause / onStop / onDestroy) maps to tier transitions cleanly. If Android port begins, no new architecture needed.
  - **OS-level memory pressure standardization:** all three platforms have it; trait `MemoryPressureSource` is the abstraction. If new APIs appear (e.g., Linux PSI gets a finer event model), the trait stays stable.

## Performance gate (binding)

Tier transitions and per-tier budgets are tracked by `lumen-bench` RAM-axis (extends existing time-axis from ADR-006/007). The CI gate (task 9G.3) fails a PR on any of:

- `T0 simple page` median peak RSS regresses > 5% vs `bench/baseline.json`.
- `T2` steady-state RSS regresses > 5%.
- Any tier transition restore time regresses > 20%.
- Any hard budget (from table above) is exceeded on `samples/page.html` or `samples/heavy.html` (new, representative of Habr-style content article).

Baseline includes both time and RAM dimensions; an automation/anti-detection PR (per ADR-006/007) and a tab-lifecycle PR both pass through the same gate but trip different metrics.

This ADR's invariants are the **only** way Lumen can hit the v0.1 RAM targets (§14.1: <50 MB empty tab, <200 MB for 100 hibernated tabs). Without this model, those targets are aspirational marketing; with it, they become measurable engineering goals.
