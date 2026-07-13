# Ph3 — Back/forward cache (bfcache)

**Developer:** P1 · **Branch:** `p1-bfcache-eligibility` (level 1 eligibility slice) · **Size:** L  
**Crates:** `lumen-shell`, `lumen-js`, `lumen-dom`  
**Source:** `docs/plan/phases.md:125` — Phase 3 roadmap item `[P4]`

---

## Status

**Phase 3 (v1.0) — DOM freeze/thaw merged to main 2026-07-02** (reintegration of the
stale `54ecd6c3` slice as a fresh port; mixed-programming session, Laguna M.1 worker
+ reviewer). What landed:

- `navigate_to` freezes the outgoing document: `Document::to_bytes()` into
  `BfCachePayload::Frozen(FrozenPage)` + a clone of the parsed `Stylesheet` into
  the new per-tab shell-side map `Lumen::frozen_styles` (keyed by URL; `Stylesheet`
  is not serializable, so it never enters `FrozenPage`; lazily pruned against
  `bfcache.has_frozen` above 32 entries). HTML-snapshot store remains as the
  fallback when the freeze fails.
- `navigate_back` / `navigate_forward` on a `Frozen` hit call `bfcache_thaw()`:
  restore DOM + stylesheet, reinstall a **fresh** QuickJS runtime over the restored
  document (no script re-run — the DOM keeps all pre-freeze mutations), re-layout,
  restore scroll/title, fire `pageshow(persisted=true)`, skip `reload()` entirely.
  `bfcache_thaw` returns `false` (→ normal reload) on DOM decode failure or an
  evicted stylesheet.
- Live acceptance (MCP window, local HTTP): page A writes a random token into the
  DOM → navigate to B → `history.back()` → the token is byte-identical (a reload
  would have re-run the script and produced a new token) and the address bar is
  back on A.

Still gated on 10C.2 (QuickJS heap serialization): `js_heap` stays `Vec::new()`,
so JS listeners/globals do NOT survive the freeze — the thaw installs a fresh
runtime. Eligibility filters (open WebSocket/EventSource, `Cache-Control:
no-store`, unload handlers) are still `bfcache_eligible() == true` for all pages.
`bfcache_restore_ms` benchmark (step 9) not yet added.

**2026-07-08 — DoD split into two levels.** Level 1 (bfcache v1: eligibility
filters + benchmark + hibernation degradation, ~3 sessions) closes the ROADMAP
task `P3-bfcache`. Level 2 (JS heap survives the freeze) is re-homed under the
V8 migration (`P3-v8`) since 10C.2 is blocked by rquickjs bindings. See
"Definition of done" below.

**2026-07-13 — Eligibility filters (WebSocket/EventSource/unload) landed.**
`bfcache_eligible()` no longer always returns `true`: it now queries
`PersistentJs::has_bfcache_freeze_blocker()` (default `false` for
non-QuickJS runtimes; `QuickPersistentJs` evals `_lumen_bfcache_blocked()`).
`_lumen_bfcache_blocked()` (`crates/js/src/dom.rs`, next to
`_lumen_fire_page_lifecycle`) returns `true` when any `_ws_instances`/
`_sse_instances` entry has `readyState === OPEN`, or a `unload`/`beforeunload`
listener is registered (`_other_win_listeners` bucket or `window.onunload`/
`onbeforeunload` property). Ineligible pages fall back to the existing
HTML-snapshot path — no regression. `Cache-Control: no-store` remains
unimplemented (needs a `LayoutSource` headers field + navigation-pipeline
plumbing, not yet present). 7 new unit tests in `dom.rs`.

---

## Goal

Keep a navigated-away page fully alive in memory — DOM + JS heap + layout tree — so that
pressing Alt+Left / Alt+Right restores it **instantly without a network round-trip or
re-parse**, firing `pageshow` with `event.persisted = true` / `pagehide` with
`event.persisted = false` per HTML Living Standard §8.6.

---

## Current state

### Back/forward navigation model

Back/forward is a two-stack model in `crates/shell/src/main.rs`:

- `nav_back: Vec<NavEntry>` (line 597) — stack of visited pages
- `nav_fwd: Vec<NavEntry>` (line 598) — pages ahead after a back press

`NavEntry` (`main.rs:1696`) stores:
- `source: PageSource` — variant identifying the resource (URL, Snapshot, Static, …)
- `scroll_x / scroll_y` — viewport offsets at navigation time
- `display_url: Option<String>` — overrides address bar for pushState entries
- `same_doc_state_json: Option<String>` — `Some` = same-document, `None` = full reload

On `navigate_to()` (`main.rs:12230`) the current page is pushed to `nav_back`, the
forward stack is cleared, and `reload()` is called — **full reload every time**.

### Existing HTML-snapshot bfcache (Phase 0/1)

A shallow bfcache already exists at `crates/storage/src/bfcache.rs`:

- `BfCacheEntry` stores only the raw HTML string + scroll offsets + title.
- `BfCache::store()` is called at navigation time (`main.rs:12233–12244`).
- `BfCache::retrieve()` is tried on back/forward (`main.rs:12317–12333`,
  `12384–12400`).
- On a hit the page is reloaded from a `PageSource::Snapshot` (`main.rs:1685–1688`):
  the HTML is re-parsed, CSS re-applied, scripts re-executed — **not a true freeze**.
- Cache is in-memory LRU, capacity 16 (`main.rs:596`), keyed by URL string.

**Conclusion:** the existing bfcache is an HTML-text cache, not a document freeze.
Back/forward always triggers a full layout+paint+JS re-run from the cached HTML bytes.

### Page teardown on navigation

When `reload()` is called (`main.rs:6445`):
1. `self.js_ctx = None` (`main.rs:6512`) — QuickJS runtime is dropped immediately.
2. `self.layout_source = new_layout_source` — old `Arc<Mutex<Document>>` may drop.
3. New pipeline: `start_streaming_load()` → `LoadEvent::LoadDone` → `apply_loaded_page()`.

There is **no freeze path** — the JS runtime and DOM are always destroyed on navigation.

### JS runtime suspend/resume infrastructure (ADR-008, Invariant 2)

The ADR-008 structural invariants for the tier model are already in place:

- `JsRuntime::pause() / unpause()` — pause JS event loop without freeing heap
  (`crates/core/src/ext.rs:882–891`)
- `JsRuntime::suspend() / resume()` — full heap snapshot (zstd)
  (`crates/core/src/ext.rs:893–905`)
- `SuspendedHeap` struct — compressed snapshot bytes (`ext.rs:912`)
- `QuickJsRuntime::suspend()` — calls `JS_WriteObject` per ADR-004
  (`crates/js/src/lib.rs:2140–2157`)
- The `PersistentJs` trait (`main.rs:1729`) provides the shell-facing JS handle;
  `pause_event_loop() / unpause_event_loop()` exist as no-ops on `NullPersistentJs`
  (`main.rs:2025–2033`) and real implementations on `QuickPersistentJs`
  (`main.rs:2306–2312`).

These are already wired for the T0→T1→T2 tab lifecycle (background tabs), but are
**not used on cross-document navigation**.

### ADR-008 tier model interaction

ADR-008 (`docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md`) explicitly notes:

> "Cross-tab page cache (bfcache): §16 Phase 3 already names bfcache; tier model
> formalizes it as 'navigation that puts current page in T2 with quick T2→T0 restore'."
> (ADR-008:189)

A bfcached page is conceptually a page at **T1/T2** (heap frozen/snapshotted) keyed by
URL + history position rather than by tab ID. The restore SLO from ADR-008 applies:
T2→T0 ≤ 200 ms.

### pageshow / pagehide infrastructure

Both events have a complete JS implementation:

- `PageTransitionEvent` constructor + `persisted` flag: `crates/js/src/dom.rs:2951–2954`
- `_lumen_fire_page_lifecycle(type, persisted)` JS function: `dom.rs:6350–6361`
- `_lumen_bfcache_persisted` global flag: `dom.rs:6346`
- `_pageshow_listeners / _pagehide_listeners` arrays: `dom.rs:6347–6348`
- Unit tests: `dom.rs:14060–14145`

**However, the shell never calls `_lumen_fire_page_lifecycle` on navigation.**  
`eval_js` calls for `pageshow`/`pagehide` are absent from `main.rs`. The JS
infrastructure exists, the shell wiring does not.

### WebSocket / open connection tracking

`WebSocket` constructor exists in the JS shim (`dom.rs:7506`), backed by
`JsWebSocketProvider` (`crates/core/src/ext.rs:1793`). A per-tab WebSocket registry
is a `HashMap<u32, Box<dyn JsWebSocketSession>>` inside `QuickJsRuntime`
(`dom.rs:1407`). Whether any WebSockets are currently open is not exposed as a
bfcache eligibility check — needs to be added.

---

## Architecture

### Concept: freeze on navigate-away, thaw on navigate-back

Replace the HTML-text snapshot in `BfCacheEntry` with a **frozen document state**:
- DOM arena snapshot (`Document::to_bytes()` — already used by hibernation in
  `tab_lifecycle/hibernate.rs`)
- Suspended JS heap (`SuspendedHeap` from `JsRuntime::suspend()`)
- Layout tree (retain `LayoutBox` + `DisplayList`) — avoids re-layout on restore
- Scroll position
- CSS stylesheet source (already in `LayoutSource`)

On back/forward navigate the shell skips `reload()` entirely, thaws the frozen state,
and fires `pageshow` with `persisted = true`.

### BfCacheEntry upgrade (proposed)

Current `BfCacheEntry` (`crates/storage/src/bfcache.rs:15`):
```rust
pub struct BfCacheEntry {
    pub url: String,
    pub html: String,   // ← replace with frozen state below
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub title: Option<String>,
}
```

Proposed upgrade (add `FrozenPage` variant alongside existing HTML fallback):
```rust
// [PROPOSED] — crates/storage/src/bfcache.rs
pub enum BfCachePayload {
    /// Phase 3 full freeze: DOM + JS heap + layout.
    Frozen(FrozenPage),
    /// Phase 0/1 fallback: re-parse HTML (no JS heap).
    HtmlSnapshot(String),
}

pub struct FrozenPage {
    /// Serialized DOM arena (bincode via Document::to_bytes()).
    pub dom_bytes: Vec<u8>,
    /// Suspended QuickJS heap (zstd-compressed, ≤5 MB).
    pub js_heap: SuspendedHeap,
    /// Retained layout tree root — skips re-layout on restore.
    pub layout_box: Option<lumen_layout::LayoutBox>,
    /// Pre-built display list — skips re-paint on restore.
    pub display_list: Vec<lumen_paint::DisplayCommand>,
    /// Inline CSS stylesheet source (re-parsed on restore; cheap).
    pub css_source: String,
}
```

### Eligibility rules (proposed)

A page is **ineligible** for the full freeze if any of the following are true at the
moment of navigation:

1. **`unload` / `beforeunload` event handlers registered** — spec disqualifies pages
   that register these (they signal side-effectful teardown); currently neither event
   is fired in Lumen (no handler detection needed immediately, but the hook must be
   added). [PROPOSED]
2. **Open WebSocket connections** — the JS WebSocket registry (`dom.rs:1407`) must be
   queried; if any session is in `OPEN` state, fall back to HTML snapshot. [PROPOSED]
3. **Open `EventSource` connections** — similar to WebSocket (`dom.rs:6257`). [PROPOSED]
4. **`Cache-Control: no-store`** response header — a header already fetched; needs
   a flag in `LayoutSource`. [PROPOSED]
5. **Pending keepalive `fetch()`** (Beacon semantics — `dom.rs:7344`) — already
   distinguished in the fetch shim; expose a pending-count accessor. [PROPOSED]

Ineligible pages continue to use the existing HTML-snapshot path (no regression).

### Thaw path (proposed): navigate_back / navigate_forward

On a bfcache hit with a `FrozenPage`:

1. **Skip `reload()`** — do not start the async streaming pipeline.
2. **Restore DOM** — deserialize `dom_bytes` back to `Arc<Mutex<Document>>`.
3. **Resume JS** — call `JsRuntime::resume(frozen.js_heap)` to restore heap and
   re-issue handles.
4. **Fire `pagehide`** on the outgoing page (if it has a live `js_ctx`) with
   `persisted = true` (the page going into the cache stays alive). [PROPOSED]
5. **Set layout/display_list** — drop re-layout entirely if `layout_box` is present
   (viewport size must match; otherwise force re-layout).
6. **Fire `pageshow`** on the restored page with `persisted = true` via
   `js_ctx.eval_js("_lumen_fire_page_lifecycle('pageshow', true)")`. [PROPOSED]
7. **Restore scroll** — from `BfCacheEntry.scroll_x / scroll_y`.

### Fire pagehide on navigate-away (proposed)

In `navigate_to()` (`main.rs:12230`), before dropping `self.js_ctx`:
```rust
// [PROPOSED] — main.rs inside navigate_to(), before js_ctx = None
if let Some(js) = &self.js_ctx {
    // persisted=true if the page is going into bfcache; false if ineligible.
    let persisted = bfcache_eligible(&self);
    js.eval_js(if persisted {
        "_lumen_fire_page_lifecycle('pagehide', true)"
    } else {
        "_lumen_fire_page_lifecycle('pagehide', false)"
    });
}
```

### Memory tier interaction (ADR-008)

A bfcached page occupies T2-equivalent memory:
- DOM bytes (serialized arena) — cheap, matches T3 hibernation format.
- JS heap (suspended, ≤5 MB zstd) — same cap as ADR-008 T2 heap snapshot.
- Layout tree — present in T2 (ADR-008: "layout tree retained" at T2).

If the tab lifecycle manager evicts a tab to T3 (Hibernated), the bfcache entry for
that tab's navigation history should be degraded to the HTML-snapshot fallback and the
`FrozenPage` memory freed. `TabLifecycleManager` (`tab_lifecycle/manager.rs`) will
need a `degrade_bfcache_entries(tab_id)` call-out. [PROPOSED]

---

## Entry points

| File | Line | Note |
|---|---|---|
| `crates/storage/src/bfcache.rs` | 15 | `BfCacheEntry` struct — upgrade payload |
| `crates/shell/src/main.rs` | 12230 | `navigate_to()` — add `pagehide` fire + freeze |
| `crates/shell/src/main.rs` | 12274 | `navigate_back()` — add thaw + `pageshow` fire |
| `crates/shell/src/main.rs` | 12344 | `navigate_forward()` — add thaw + `pageshow` fire |
| `crates/shell/src/main.rs` | 6512 | `apply_loaded_page()` — skip when thawing |
| `crates/shell/src/main.rs` | 596 | `BfCache::new(16)` — consider separate capacity |
| `crates/js/src/dom.rs` | 6350 | `_lumen_fire_page_lifecycle` — already present |
| `crates/js/src/dom.rs` | ~6944 | `_lumen_bfcache_blocked()` — WS/SSE/unload eligibility check (2026-07-13) |
| `crates/shell/src/main.rs` | ~2451 | `PersistentJs::has_bfcache_freeze_blocker()` — trait method (2026-07-13) |
| `crates/core/src/ext.rs` | 882 | `JsRuntime::pause/unpause/suspend/resume` — already present |
| `crates/shell/src/tab_lifecycle/manager.rs` | — | Add `degrade_bfcache_entries()` [PROPOSED] |
| `crates/storage/src/bfcache.rs` / `crates/shell/src/main.rs` (`LayoutSource`) | — | No `Cache-Control` capture yet — needed for `no-store` filter [PROPOSED] |

Lines marked **[PROPOSED]** do not exist yet; all others are real file:line refs.

---

## Steps

1. **Add `FrozenPage` and upgrade `BfCacheEntry`** in `crates/storage/src/bfcache.rs`.
   Keep `HtmlSnapshot` fallback so existing back/forward still works while full freeze
   is being wired. Add unit tests for store/retrieve of both variants.

2. **Add eligibility check** (`bfcache_eligible()`) in `main.rs`. Initial implementation:
   always return `true` (full freeze for all pages). Ineligibility filters added
   incrementally in later micro-steps.

3. **Wire `pagehide` in `navigate_to()`** (`main.rs:12230`) before `self.js_ctx` is
   set to `None`. Fire with `persisted = true` when eligible, `false` when not.

4. **Freeze on navigate-away**: call `js.suspend()` to get `SuspendedHeap`, call
   `layout_source.document.lock().to_bytes()` for DOM, capture `layout_box` and
   `display_list` into a `FrozenPage`. Store as `BfCacheEntry::Frozen(...)`.

5. **Thaw on navigate-back/forward**: in `navigate_back()` and `navigate_forward()`,
   when `bfcache.retrieve(url)` returns `BfCachePayload::Frozen(page)`:
   - restore DOM from `page.dom_bytes`
   - call `QuickJsRuntime::resume(page.js_heap)` — reassemble `PersistentJs`
   - skip `reload()` and `apply_loaded_page()`
   - set `self.layout_box`, `self.display_list` directly
   - fire `pageshow` with `persisted = true`

6. **Fire `pageshow` on initial page load** with `persisted = false` (currently not
   fired at all). Hook into `apply_loaded_page()` after JS context is live.

7. **Add WebSocket eligibility filter**: expose `open_ws_count()` from the WebSocket
   registry in `QuickJsRuntime`; fall back to HTML snapshot when count > 0.

8. **Add T2→T3 bfcache degradation**: when `TabLifecycleManager` hibernates a tab,
   clear `FrozenPage` entries for that tab, keeping only URL + title + scroll
   (re-parse path). Add `degrade_bfcache_entries(tab_id)` to `manager.rs`.

9. **Measure restore latency**: add a `bfcache_restore_ms` metric to
   `lumen-bench` (target ≤ 50 ms for `FrozenPage`, per ADR-008 T1→T0 SLO — bfcache
   restore should be at least as fast as a background tab activation).

10. **Add `beforeunload` / `unload` detection stub**: track whether the page's JS
    registered either handler; mark ineligible if true. Can be a simple boolean flag
    in `PersistentJs`.

---

## Risks

| Risk | Mitigation |
|---|---|
| **QuickJS heap resume drops timers / intervals** | `suspend()` + `resume()` round-trip currently strips scheduled timers (`setInterval` state is not in heap snapshot — see `crates/js/src/lib.rs:2140`). For bfcache this is acceptable: spec says timers are paused while cached. Re-document this contract. |
| **DOM arena deserialization version skew** | If Lumen version changes between freeze and thaw (upgrade mid-session) the `bincode` layout may not match. Add a version tag to `FrozenPage`. On version mismatch fall back to HTML re-parse. |
| **Memory pressure: too many frozen pages** | `BfCache::new(16)` — 16 full frozen pages may consume significant RAM. Consider a smaller limit for `FrozenPage` (e.g., 4) with HTML-snapshot overflow. Or cap total frozen bytes. |
| **JS heap size > 5 MB** | `suspend()` in `lib.rs:2140` returns the raw `JS_WriteObject` bytes. zstd compression applied downstream (ADR-008). If compressed > 5 MB, fall back to HTML snapshot (same cap as tab hibernation). |
| **EventSource keeps server connection alive** | EventSource connections (`dom.rs:6257`) are not tracked centrally. An open SSE connection while "frozen" would hang. Mark ineligible; add `open_sse_count()` accessor. |
| **Layout tree invalid after viewport resize** | If viewport size changes while page is frozen, the retained `LayoutBox` is stale. Compare viewport dimensions on thaw; if changed, force re-layout before paint. |
| **WebSocket connection teardown** | When a page enters bfcache with `persisted = true`, spec says the WebSocket should remain open (page is "frozen", not "unloaded"). Current implementation would allow this only if ineligibility check is skipped. Start conservatively: ineligible if any WS is open. |

---

## Tests

- **Unit — `crates/storage/src/bfcache.rs`**: `FrozenPage` round-trip store/retrieve;
  LRU eviction still works with `Frozen` variant; `HtmlSnapshot` fallback path.
- **Unit — `crates/js/src/dom.rs`**: `pageshow` fires with `persisted = true` after
  `_lumen_fire_page_lifecycle('pageshow', true)` (already passing at `dom.rs:14084`).
  `pagehide` with `persisted = true` (new test).
- **Integration — shell**: `navigate_to()` + `navigate_back()` round-trip — measure
  that `pageshow.persisted` is `true` in the JS context after back; measure that
  `pagehide.persisted` was `true` when navigating away.
- **Regression — `lumen-bench`**: `bfcache_restore_ms` benchmark. Goal: ≤ 50 ms on
  `samples/page.html`.
- **Eligibility — shell**: navigate to page with open WebSocket → navigate away →
  back; verify that HTML-snapshot path was used (not `FrozenPage`).

---

## Definition of done

Split into two levels (2026-07-08). **Level 1 closes the ROADMAP task** (`P3-bfcache`
→ done); Level 2 is re-homed under the V8 migration track because QuickJS heap
serialization (10C.2) is blocked by rquickjs bindings — no number of P1 sessions
unblocks it.

### Level 1 — bfcache v1 (DOM freeze/thaw, no JS heap)

Done on main (merged 2026-07-02):

- [x] `BfCacheEntry` carries `FrozenPage` (DOM bytes; layout re-built on thaw,
      JS heap deferred to Level 2).
- [x] `navigate_to()` fires `pagehide(persisted=true)` before freezing eligible pages.
- [x] `navigate_back()` / `navigate_forward()` thaw `FrozenPage` without calling
      `reload()`.
- [x] `pageshow(persisted=true)` fires after thaw; `pageshow(persisted=false)` fires on
      normal load.
- [x] HTML-snapshot fallback still works for ineligible pages (no regression).

Remaining (~2 sessions, see Steps 8–9):

- [x] WebSocket-open pages fall back to HTML snapshot (step 7). 2026-07-13:
      `_lumen_bfcache_blocked()` (`crates/js/src/dom.rs`) checks
      `_ws_instances`/`_sse_instances` for `readyState === OPEN`;
      `PersistentJs::has_bfcache_freeze_blocker` (`main.rs`) queries it via
      `eval_js_value`; `bfcache_eligible()` now calls it through
      `route_query_js` instead of always returning `true`.
- [x] `beforeunload`/`unload` handler detection → ineligible (step 10). 2026-07-13:
      same `_lumen_bfcache_blocked()` — `addEventListener('unload'|'beforeunload', …)`
      lands in `_other_win_listeners` (no dedicated case in `dom.rs`'s
      `addEventListener`, so it falls to the generic bucket already); plain
      `window.onunload`/`onbeforeunload` property assignment checked directly.
- [x] Open EventSource → ineligible. 2026-07-13: covered by the same
      `_lumen_bfcache_blocked()` check (`_sse_instances`, `readyState === OPEN`).
- [ ] `Cache-Control: no-store` → ineligible. Not yet plumbed: `LayoutSource`
      has no headers field and nothing captures `Cache-Control` per-navigation
      today (see "Entry points" below for the gap). Left as a TODO on
      `bfcache_eligible()`.
- [ ] `bfcache_restore_ms` benchmark added; P50 ≤ 50 ms on `samples/page.html` (step 9).
- [ ] T2→T3 degradation: `degrade_bfcache_entries(tab_id)` on tab hibernation (step 8).
- [x] `cargo clippy -p lumen-shell -p lumen-js --all-targets -D warnings` clean.
- [x] All existing back/forward shell tests pass.

### Level 2 — live JS state survives the freeze (gated on 10C.2 / V8)

Not part of the ROADMAP `P3-bfcache` task. Lands with the V8 migration
(`P3-v8`, `docs/tasks/ph3-v8-migration.md` — ValueSerializer), or earlier if
10C.2 (QuickJS heap serialization) is ever unblocked in rquickjs.

- [ ] `FrozenPage.js_heap` is populated on freeze (currently `Vec::new()`,
      `crates/shell/src/main.rs` freeze block in `navigate_to()`).
- [ ] Thaw resumes the suspended heap instead of installing a fresh runtime —
      listeners and globals survive back/forward (timers may be dropped per spec:
      paused while cached).
- [ ] Heap > 5 MB compressed → fall back to HTML snapshot (same cap as tab
      hibernation, ADR-008).
