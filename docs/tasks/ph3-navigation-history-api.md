# Ph3 — Navigation API + History API

**Developer:** P4 · **Branch:** `p4-ph3-navigation-history` · **Size:** L · **Crates:** `lumen-js`, `lumen-shell`

> Roadmap item: `docs/plan/phases.md:126` — **"Navigation API + History API runtime [P4]"** (Phase 3, v1.0, Browser fundamentals).

---

## Status

**Phase 3 — future.** Do not start until Phase 2 is closed and the user assigns it. This file is a pre-written task spec so the picking session does not re-research.

Important framing: this is **not greenfield**. The classic **History API is already implemented and wired to the real shell back-forward stack** (Phase 0/2 work). The modern **Navigation API exists only as a standalone pure-JS shim** with its own in-memory entry list that is *not* connected to the shell, `location`, or the History stack. The bulk of this task is (a) finishing History API edge cases and (b) wiring the Navigation API shim into the real navigation pipeline.

### Progress (2026-06-25) — Phase 1a (History `go` semantics) DONE

Merged slice (`crates/js/src/dom.rs`, `history.go`):
- `history.go(0)` now reloads the current document (was a silent no-op) — HTML LS history-traversal.
- Non-zero `history.go(n)` delivers popstate through `_lumen_deliver_popstate`, so a `go()` traversal now (a) syncs `location` (`location.pathname`/`href` were stale before) and (b) fires a real `PopStateEvent` instead of an ad-hoc inline object — identical to a shell-driven traversal.
- Tests: `history_go_zero_reloads`, `history_go_updates_location`, `history_go_out_of_bounds_no_popstate`; existing 15 history + 9 popstate tests still green.

**Still remaining** (Phase 1b + Phase 2): cross-document single-authority unification (JS `HistoryState` mirror vs shell `nav_back`/`nav_fwd` drift on multi-step `go` across full-document boundaries), `hashchange`+`popstate` for fragment nav, post-back `history.state` from shell-restored value, and the entire Navigation API wiring (steps 5–10 below). The ROADMAP `P3-navapi` row stays `planned` until those land.

---

## Goal

Ship a coherent, spec-aligned same-document navigation runtime:

1. **History API** — `history.pushState` / `replaceState` / `back` / `forward` / `go`, `history.length`, `history.state`, and the `popstate` event — fully backed by the shell's real back-forward stack (mostly done; close the gaps below).
2. **Navigation API** (HTML LS §7.8) — `window.navigation` singleton whose `currentEntry` / `entries()` / `navigate()` / `back()` / `forward()` / `traverseTo()` and `navigate` / `navigatesuccess` / `navigateerror` / `currententrychange` events reflect and drive the *same* real navigation stack as History — including the `navigate` event firing on link clicks, address-bar navigations, and back/forward (interception = SPA routing).

Both APIs must mutate one shared source of truth: the shell's `nav_back` / `nav_fwd` stacks and `current_history_state_json`, with SPA URL/title changes applied without a page reload.

---

## Current state

### History API — implemented and wired (the good part)

JS shim (`history` object): `crates/js/src/dom.rs:6133` (`pushState` `:6138`, `replaceState` `:6147`, `back`/`forward`/`go` `:6156`, `popstate` delivery `_lumen_deliver_popstate` `:6120`, `var history` literal `:6133`, exposed on `window` `:8289`).

JS-side state mirror `HistoryState` (entries Vec + `current` index): `crates/js/src/dom.rs:34`, with `push` `:50`, `replace` `:56`, `go` `:64`, `length` `:90`. Native bindings `_lumen_history_push/replace/go/length/state_json/url`: `crates/js/src/dom.rs:1056-1093`.

Shell handoff types:
- `NavigateRequest` enum (`location.href=` etc.) — `crates/js/src/dom.rs:100`.
- `HistoryUrlUpdate::{Push,Replace}` (pushState/replaceState → shell) — `crates/js/src/dom.rs:118`; queued via `_lumen_history_push_url` / `_lumen_history_replace_url` `crates/js/src/dom.rs:1099-1113`; drained by `take_history_url_updates` `crates/js/src/lib.rs` (consumed in shell `crates/shell/src/main.rs:7561`).

Shell back-forward stack (the **real** source of truth):
- `struct NavEntry { source, scroll_x, scroll_y, display_url, same_doc_state_json }` — `crates/shell/src/main.rs:1696`. `same_doc_state_json: Some(_)` marks a `pushState` same-document entry; `None` marks a full-document navigation.
- `nav_back: Vec<NavEntry>` `crates/shell/src/main.rs:3636`; `current_history_state_json: String` `crates/shell/src/main.rs:3673` / `:5808`.
- pushState/replaceState applied to the real stack: `crates/shell/src/main.rs:7559-7584` (pushState pushes a `NavEntry` with `same_doc_state_json`, updates `display_url`; replaceState updates URL + state only).
- `navigate_back` `crates/shell/src/main.rs:12274` and `navigate_forward` `crates/shell/src/main.rs:12344`: pop a `NavEntry`; if `same_doc_state_json.is_some()` → fire `popstate` via `js.fire_popstate(state_json, url)` (`crates/shell/src/main.rs:2249`, trait `:1947`) and update the address bar **without reload**; else full-document reload (with bfcache lookup).
- Session persistence of the stack: `crates/shell/src/main.rs:14834` (snapshot) / `:14907` (restore) / `:14950` (`current_history_state_json`).

Tests already present: `crates/js/src/dom.rs:12863-13001` (length/state/push/replace/back/forward/popstate/forward-truncation) and `:14936-15020` (`HistoryUrlUpdate` enqueue, `_lumen_deliver_popstate`).

**History gaps to close (verify against code before assuming done):**
- `history.go(0)` is a no-op (`HistoryState::go` returns `false` for delta 0, `crates/js/src/dom.rs:65`) — spec says `go(0)` reloads the current document. Decide: wire to shell reload or document as intentional deviation.
- Multi-step `history.go(n)` for `|n| > 1` goes through the JS-side `HistoryState` only (`_lumen_history_go` `crates/js/src/dom.rs:1075`) and fires popstate from JS (`crates/js/src/dom.rs:6159`); confirm this stays in sync with the shell `nav_back`/`nav_fwd` stacks for multi-step jumps, or route multi-step go through the shell like `navigate_back`/`navigate_forward` do for single steps. **This is the most likely real bug** — the JS `HistoryState` and the shell stacks are two mirrors that can drift on multi-step `go`.
- `popstate` is *not* fired for hash-only fragment navigation (`crates/shell/src/main.rs:11419` handles same-page fragment nav) — spec fires `hashchange` + `popstate`. Verify `hashchange` path and whether `popstate` should accompany it.
- `history.state` after a real (full-document) back/forward must reflect `current_history_state_json` restored by the shell; confirm `_lumen_history_state_json` returns the shell-restored value, not the stale JS mirror.

### Navigation API — pure-JS shim, NOT wired (the work)

`crates/js/src/navigation_api.rs` (installed at `crates/js/src/lib.rs:1107`). Defines `window.navigation` (`navigation_api.rs:276`) + `globalThis.navigation` (`:285`), classes `NavigationHistoryEntry` (`:21`), `NavigateEvent` with `intercept()` (`:56`/`:88`), `Navigation extends EventTarget` (`:104`) with `navigate()` (`:130`), `back/forward/traverseTo/_traverseBy` (`:198-268`).

**Why it is not real:**
- The singleton keeps its **own** `_entries` array + `_currentIndex` (`navigation_api.rs:107-119`), seeded with a single entry from `window.location.href`. It is never reconciled with the shell `nav_back`/`nav_fwd` stacks or with the History API `HistoryState`.
- `navigation.navigate(url)` performs an **in-JS-only** entry mutation (`navigation_api.rs:161-185`) — it never emits a `NavigateRequest` to the shell, so no real page load or address-bar update happens.
- `navigation.back()/forward()/traverseTo()` only move `_currentIndex` (`navigation_api.rs:251`) — they do not call the shell's `navigate_back`/`navigate_forward`, and they don't fire `popstate` on the History side.
- The `navigate` event **never fires for real navigations** (link clicks at `crates/shell/src/main.rs:13282`, address-bar nav, Alt+Left/Right, `location.href=`). Interception (`NavigateEvent.intercept()`) therefore does nothing useful — SPA routing via the Navigation API is impossible today.
- No `_lumen_navigation_*` native bindings exist (grep confirms only `_lumen_history_*` and `_lumen_navigate` exist). There is no shell→JS delivery path for `navigate`/`currententrychange`.

There is a smoke test only: `typeof window.navigation === 'object'` (`crates/js/src/dom.rs:23626`).

### Related, do-not-break

`soft_navigation.rs` (`crates/js/src/soft_navigation.rs:56`) expects a shell hook to fire a `PerformanceSoftNavigationEntry` after a qualifying pushState / Navigation-API navigation — wiring the navigate path is the natural place to also feed soft-navigation timing. `crates/js/src/dom.rs:6117` `_lumen_deliver_popstate` is the existing shell→JS same-document delivery primitive to mirror.

---

## Architecture

Single source of truth = the shell's `nav_back` / `nav_fwd` / `current_history_state_json`. Both JS APIs are **views/controllers** over it, not independent stores.

### A. Bind Navigation API to the real stack

1. **Replace the in-JS `_entries` store** with shell-backed accessors, mirroring how History uses `_lumen_history_*`. Add native bindings (proposed): `_lumen_navigation_entries_json()`, `_lumen_navigation_current_index()`, `_lumen_navigation_can_go_back/forward()`. The shell builds these from `nav_back` + current + `nav_fwd`, assigning each `NavEntry` a stable `key`/`id` (add `key: String` to `NavEntry`, proposed).
2. **Route `navigation.navigate(url, {state, replace, history})`** to the shell: emit a `NavigateRequest::Push/Replace` (existing enum, `crates/js/src/dom.rs:100`) for cross-document, or a `HistoryUrlUpdate`-style same-document update when the `navigate` handler calls `intercept()`. `navigation.back/forward/traverseTo` call the shell's `navigate_back`/`navigate_forward` (or a new `navigate_to_index`, proposed) instead of moving a local index.

### B. Fire the `navigate` event on real navigations

The shell must dispatch a JS `navigate` event **before** committing any navigation it initiates — link clicks (`crates/shell/src/main.rs:13282`), address-bar, `location.href=`, Alt+Left/Right. Add a shell→JS delivery primitive (proposed) `_lumen_deliver_navigate(navigation_type, destination_url, can_intercept, hash_change)` that constructs a `NavigateEvent`, dispatches it on `window.navigation`, and reports back whether `preventDefault()` / `intercept()` was called.

- If `intercept({handler})` was called → treat as **same-document**: do NOT reload; run the handler promise; update `display_url` + `current_history_state_json`; push a same-doc `NavEntry`; fire `navigatesuccess` + `currententrychange`; optionally feed `soft_navigation`.
- If `preventDefault()` (no intercept) → cancel the navigation entirely; fire `navigateerror`.
- Otherwise → proceed with the normal (full-document) navigation, then fire `navigatesuccess` + `currententrychange` after commit.

This requires the navigate dispatch to happen synchronously enough to read the interception decision before the shell decides reload-vs-not. Mirror the existing `take_*` round-trip pattern (`take_navigate_request` / `take_history_url_updates`) — JS handler queues an "intercepted" flag the shell drains.

### C. popstate ↔ Navigation events unified

When `navigate_back`/`navigate_forward` (`crates/shell/src/main.rs:12274` / `:12344`) handle a same-document entry, in addition to `fire_popstate` they must fire the Navigation API `navigate` (type `traverse`) + `currententrychange`. Keep a single shell call that drives both APIs to avoid drift.

### D. SPA URL/title update without reload

Already works for `pushState`/`replaceState` (`crates/shell/src/main.rs:7559`). Extend the same path so an intercepted Navigation-API navigation updates `display_url` and `document.title` (find the title-update site; address bar uses `display_url`) **without** entering `reload()`. Title: grep `document.title` setter / window title sync in shell.

---

## Entry points (real file:line; *proposed* = to be added)

History (existing):
- `crates/js/src/dom.rs:6133` — `history` JS object.
- `crates/js/src/dom.rs:34` — `HistoryState` JS-side mirror.
- `crates/js/src/dom.rs:100` — `NavigateRequest`; `:118` — `HistoryUrlUpdate`.
- `crates/shell/src/main.rs:1696` — `NavEntry` (add `key`/`id` *proposed*).
- `crates/shell/src/main.rs:7559` — pushState/replaceState → real stack.
- `crates/shell/src/main.rs:12274` / `:12344` — `navigate_back` / `navigate_forward`.
- `crates/shell/src/main.rs:2249` / trait `:1947` — `fire_popstate`.

Navigation (existing shim + proposed wiring):
- `crates/js/src/navigation_api.rs:104` — `Navigation` class (rewrite store to shell-backed).
- `crates/js/src/navigation_api.rs:130` — `navigate()` (route to shell).
- `crates/js/src/navigation_api.rs:216` — `_traverseBy` (route to shell).
- `crates/js/src/lib.rs:1107` — install site.
- *proposed* `crates/js/src/navigation_api.rs` — `_lumen_navigation_*` native bindings + `_lumen_deliver_navigate` shim entry.
- *proposed* `crates/shell/src/main.rs` — `dispatch_navigate_event(...)` before each navigation; `navigate_to_index(...)`; `take_navigation_intercept()` drain (mirror `take_history_url_updates`).
- `crates/shell/src/main.rs:13282` — link click (call dispatch_navigate first).
- `crates/js/src/soft_navigation.rs:56` — feed soft-navigation timing from the intercepted path.

---

## Steps

**Phase 1 — History API completion (smaller, lower risk):**
1. Audit JS `HistoryState` vs shell `nav_back`/`nav_fwd` for multi-step `history.go(n)` drift; route multi-step `go` through the shell so a single stack is authoritative (the JS mirror becomes a read cache). Add regression tests.
2. Decide and implement `history.go(0)` (reload vs documented no-op).
3. Confirm `hashchange` + `popstate` for fragment navigation (`crates/shell/src/main.rs:11419`).
4. Confirm `history.state` reflects shell-restored `current_history_state_json` after full back/forward.

**Phase 2 — Navigation API wiring (the bulk):**
5. Add `key`/`id` to `NavEntry`; build shell-side entry list + index accessors. Add `_lumen_navigation_*` native bindings. Rewrite the shim's `_entries`/`_currentIndex` to read from the shell.
6. Route `navigation.navigate()` / `back()` / `forward()` / `traverseTo()` to the shell (`NavigateRequest` / `navigate_back` / `navigate_forward` / new `navigate_to_index`).
7. Implement `dispatch_navigate_event` + the intercept round-trip; call it before link-click / address-bar / `location.href=` / Alt+Left-Right navigations.
8. Implement intercept → same-document path (no reload; run handler; update URL/title/state; fire `navigatesuccess` + `currententrychange`; feed soft-navigation).
9. Unify same-document back/forward to fire both `popstate` and Navigation `navigate(traverse)` + `currententrychange`.
10. SPA title update without reload.

---

## Tests

JS-level (`crates/js/src/dom.rs` / `crates/js/src/navigation_api.rs` test modules — follow existing style at `dom.rs:12863`):
- History multi-step `go(2)` / `go(-2)` updates `history.state` + `history.length` consistently; popstate fires once per step or once for the jump per spec.
- `navigation.entries()` length and `currentEntry.index` track `history.length` / current after pushState, navigate, and back/forward.
- `navigation.navigate(url)` returns `{committed, finished}` promises that resolve; `navigatesuccess` fires; `currententrychange` fires.
- `navigate` event fires with correct `navigationType` (`push`/`replace`/`traverse`/`reload`); `intercept({handler})` suppresses reload and resolves `finished` after the handler.
- `preventDefault()` on `navigate` cancels and fires `navigateerror`.
- `currentEntry.getState()` round-trips the state passed to `navigate({state})`.

Shell-level (mock JS context, mirror `take_history_url_updates` tests):
- A link click produces a `navigate` dispatch; an intercepting handler yields a same-document `NavEntry` (no `reload()` call), updates `display_url`, no network fetch.
- `navigation.back()` after an intercepted navigation fires `popstate` AND Navigation `navigate(traverse)`.
- Session snapshot/restore round-trips Navigation entry `key`/`id`.

No graphic test (no visual surface). Do not add a `graphic_tests/` entry.

---

## Definition of done

- One authoritative navigation stack: History API and Navigation API both read/write the shell's `nav_back`/`nav_fwd`/`current_history_state_json`; the JS `HistoryState` and Navigation `_entries` are read caches, not independent stores (no drift on multi-step `go`).
- `navigate` event fires for link clicks, address-bar navigations, `location.href=`, and back/forward, with correct `navigationType`.
- `NavigateEvent.intercept()` performs a real same-document navigation: no reload, URL + title + `history.state` updated, `navigatesuccess` + `currententrychange` fired, `popstate` fired on traverse — i.e. SPA routing works end to end.
- `preventDefault()` cancels navigation and fires `navigateerror`.
- History API gaps (multi-step `go`, `go(0)`, fragment `popstate`, post-back `history.state`) resolved or explicitly documented as deviations with a `// BUG-NNN` filed.
- `cargo clippy -p lumen-js --all-targets -- -D warnings` and `-p lumen-shell` clean; `cargo test -p lumen-js` green.
- `CAPABILITIES.md` History/Navigation rows updated; `docs/plan/phases.md:126` item marked done; `subsystems/` JS crate file updated; `SYMBOLS.md` regenerated if public API changed.
