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

### Progress (2026-06-25) — Phase 1b-i (`location.hash` setter + `hashchange`) DONE

Merged slice (`crates/js/src/dom.rs`, `location`/`window` shim):
- `location.hash` is now an HTML LS setter (was an inert data property). Assigning it changes only the fragment and performs a **same-document** navigation: no reload.
- The setter updates `location` through a private `_lumen_loc_hash` backing var (so internal `_lumen_location_update` writes bypass the setter), pushes a same-document history entry (`_lumen_history_push` JS mirror + `_lumen_history_push_url` shell), and fires `hashchange` via `_lumen_fire_hashchange` on `window.onhashchange` + `addEventListener('hashchange')`. Setting hash to its current value is a no-op.
- Added `window.onhashchange`. Tests: 8 `location_hash_setter_*`; full `lumen-js` suite green (2295).

### Progress (2026-06-25) — Phase 1b-ii (`location.href=`/`assign()`/`replace()` fragment routing) DONE

Merged slice (`crates/js/src/dom.rs`, `location` shim):
- `location.href=`, `location.assign(url)`, `location.replace(url)` now route through a new `_lumen_navigate_or_fragment(rawUrl, replace)` helper. It resolves the target against the current href (`new URL(url, _lumen_loc_href)`); if the resolved URL differs **only** in its fragment, it performs a same-document navigation (no reload): updates `location`, pushes/replaces a same-document history entry (`_lumen_history_push`/`replace` JS mirror + `_lumen_history_push_url`/`replace_url` shell handoff → existing `same_doc_state_json: Some(_)` machinery), and fires `hashchange`. `replace()` uses Replace; `href=`/`assign()` use Push.
- Any non-fragment difference (path/host/search) or an identical full URL falls through to the existing full-navigation `_lumen_navigate`.
- Reuses the shell's pushState same-document path — **no shell change needed**. Tests: 8 `location_{href,assign,replace}_*fragment*`; full `lumen-js` suite green (2303).

### Progress (2026-06-25) — Phase 1b-iii (link-click fragment nav syncs JS) DONE

Merged slice (`crates/shell/src/main.rs` `navigate_fragment`, `crates/shell/src/links.rs`):
- Clicking `<a href="#id">` previously ran a shell-only `navigate_fragment` (`:target` cascade + scroll) that left the JS side stale: `location.hash`/`href` did not update, no `hashchange` fired, and no session-history entry was pushed (back/forward broken for fragment nav).
- `navigate_fragment` now first routes through the JS `_lumen_navigate_or_fragment(new_url, false)` path (the same helper `location.href=` uses, Phase 1b-ii). New helper `links::fragment_url(current, frag)` builds the target URL (replaces the fragment of the current display URL; empty `frag` strips it). The JS path updates `location`, pushes a same-document history entry (queued `HistoryUrlUpdate` drained into `nav_back`), and fires `hashchange` — then the existing `:target`/scroll logic runs.
- No JS change needed (reuses the Phase 1b-ii helper). Tests: `links::fragment_url_builds_same_document_urls`; `cargo clippy -p lumen-shell` clean, `cargo test -p lumen-shell` green.

### Progress (2026-06-25) — Phase 1b-iv (full-URL same-page link → same-document fragment nav) DONE

Merged slice (`crates/shell/src/links.rs`, `crates/shell/src/main.rs` link-click branches):
- Clicking `<a href="/page#x">` from `/page` previously fell through the `is_navigable_href` branch into a full `navigate_to` reload (the network refetch + fresh document) even though only the fragment differed. The `fragment_only` fast-path only caught bare `#x` hrefs, not full/relative URLs that resolve to the current document.
- New pure helper `links::same_document_fragment(current, resolved) -> Option<String>`: returns the destination fragment (without leading `#`, empty = top of page) iff the two absolute URLs share the same base (everything before the first `#`) but differ in their fragment; `None` for a real cross-document nav or an identical URL (= reload). Mirrors the JS `_lumen_navigate_or_fragment` decision exactly.
- Both link-click sites (`crates/shell/src/main.rs` hit-test click + form-click `Nothing` path) now, after `resolve_href`, check `same_document_fragment(self.current_display_url(), &resolved)`; on `Some(frag)` they route through `navigate_fragment(frag)` (Phase 1b-iii — syncs JS `location`, pushes a same-document history entry, fires `hashchange`, runs `:target`/scroll) instead of `navigate_to`. The click-log outcome for that path is `LinkFragment` rather than `LinkNavigate`.
- Tests: 10 `links::same_document_fragment_*` (add/change/remove fragment, identical-URL reload, different path/host/query, query-preserved-in-base, first-`#`-splits). `cargo clippy -p lumen-shell --all-targets` clean, `cargo test -p lumen-shell links::` green (23/23).

### Progress (2026-06-25) — Phase 1b-v (fragment traversal fires `hashchange` + `popstate`) DONE

Merged slice (`crates/js/src/dom.rs`, `_lumen_deliver_popstate`):
- Traversing back/forward over a same-document entry that differs only in its fragment now fires **both** `popstate` and `hashchange` (HTML LS §7.4.6). Previously `_lumen_deliver_popstate` (the shell→JS same-document delivery primitive used by `navigate_back`/`navigate_forward`) fired only `popstate`, so a `<a href="#x">` link click followed by Alt+Left did not deliver `hashchange` to the page.
- The helper now captures the old `_lumen_loc_href` fragment before `_lumen_location_update(url)`, computes the new fragment, fires `popstate` as before, then — iff `url` is non-empty and the fragments differ — calls the existing `_lumen_fire_hashchange(oldHref, newHref)` (Phase 1b-i). Order is spec-correct: popstate first, hashchange after. Empty `url` (keep-current) and same-fragment path-only traversals fire no `hashchange`.
- Tests: 4 `deliver_popstate_*hashchange*`; full `lumen-js` suite green (2307).

### Progress (2026-06-26) — Phase 1c (post-back `history.state` sync) DONE

Merged slice (`crates/js/src/dom.rs`, `HistoryState` + `_lumen_deliver_popstate`):
- `_lumen_deliver_popstate(state_json, url)` (the shell→JS traversal-delivery primitive) fired the `popstate` event but never wrote `state_json` into the JS-side `HistoryState` mirror, so `history.state` stayed stale — `null` after a full-document back even though the shell restored a real state object (HTML LS §7.4.6 requires `history.state` to reflect the current entry).
- New `HistoryState::set_state(state_json)` updates **only** the current entry's serialized state (leaves `url` untouched — the popstate `url` arg can be empty = "keep current"). Exposed via native binding `_lumen_history_set_state`; `_lumen_deliver_popstate` now calls it right after `_lumen_location_update`, before listeners run, so handlers reading `history.state` see the restored value.
- Tests: `deliver_popstate_updates_history_state`, `deliver_popstate_empty_url_keeps_url_updates_state` (empty url keeps location, still updates state); full `lumen-js` history+popstate suites green.

### Progress (2026-06-26) — Phase 1d (multi-step `history.go(n)` single authority) DONE

Closes Phase-1 **Step 1**: JS-initiated traversal is now SHELL-AUTHORITATIVE, removing the JS-mirror-vs-shell drift on multi-step `go`.

- **JS** (`crates/js/src/dom.rs`, `crates/js/src/lib.rs`): new `pending_history_traversals` queue + binding `_lumen_history_traverse(delta)` + `JsRuntime::take_history_traversals()`. `history.go(delta)` (non-zero) now moves only the read-cache cursor (`_lumen_history_go`, keeps `history.state`/`length` + pushState truncation correct) and, on success, queues the real `delta` for the shell — it no longer calls `_lumen_deliver_popstate` itself. `back()`/`forward()` route through `go(∓1)`. `go(0)` still reloads.
- **Shell** (`crates/shell/src/main.rs`): `Lumen::navigate_by(delta)` drains the queue in `about_to_wait` and traverses the real `nav_back`/`nav_fwd` stacks as ONE logical step — intermediate entries of a multi-step `go(n)` are shuffled across the cursor without rendering (pure `NavEntry::shift_history_entry`), and only the destination fires `popstate` (same-document) or reloads (full-document) via the existing `navigate_back`/`navigate_forward`. Out-of-range deltas are a no-op. `JsBridge::take_history_traversals` added.
- **Tests**: JS — `history_go_queues_single_step_traversal`, `history_go_multistep_queues_full_delta_and_moves_cache`, `history_go_zero_does_not_queue_traversal`, `history_go_out_of_range_does_not_queue_traversal`; `history_back_fires_popstate_with_previous_state` + `history_go_updates_location` updated to simulate the shell's popstate delivery. Shell — `navigate_by_tests` (pure `shift_history_entry` hop bookkeeping). All green.
- **Known limitation (deferred to Phase 2)**: a multi-step traversal that crosses a full-document boundary yet lands on a same-document entry of a *different* document fires `popstate` without re-rendering that document — the genuine cross-document unification still ahead.

### Progress (2026-06-26) — Phase 2a (navigate event dispatch + traverseTo) DONE

Merged slice (`crates/js/src/navigation_api.rs`, `crates/shell/src/main.rs`):
- Added `_lumen_dispatch_navigate(type, url, canIntercept, hashChange)` JS shim: constructs a `NavigateEvent` with `navigationType`, `signal`, `destination` URL, and dispatches it on `window.navigation`. Exposed as `globalThis._lumen_dispatch_navigate`.
- Shell calls `_lumen_dispatch_navigate('push|replace|fragment|traverse', ...)` before every real navigation: `navigate_to` (`push`), `navigate_replace` (`replace`), `navigate_fragment` (`fragment`, `hashChange: true`), and same-document back/forward (`traverse`). This gives the page a `navigate` event with correct `navigationType` before the navigation commits.
- Implemented `Lumen::navigate_to_key(key)`: looks up `nav_key` in `nav_back` (searching from most-recent) and `nav_fwd`, computes steps, and delegates to `navigate_by(steps)`. This makes `navigation.traverseTo(key)` a real shell-driven key lookup, no longer a no-op.
- Wired `traverseTo(key)` in `about_to_wait`: the `action_code == 4 (TraverseTo)` branch now passes `key` to `navigate_to_key` instead of ignoring it.
- `cargo check` green for `lumen-shell` + `lumen-js`; `cargo clippy -p lumen-shell` clean.

**Still remaining** (Phase 2b–d): intercept round-trip (preventDefault/intercept → cancel or same-document without reload), `navigatesuccess`/`navigateerror`/`currententrychange` events, and cross-document edge unification for multi-step traversal.

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

**Phase 1 — History API completion (smaller, lower risk):** ✅ DONE
1. ~~Audit JS `HistoryState` vs shell `nav_back`/`nav_fwd` for multi-step `history.go(n)` drift~~ → shell-authoritative `navigate_by` + read-cache JS `HistoryState`.
2. ~~Decide and implement `history.go(0)`~~ → reloads the current document.
3. ~~Confirm `hashchange` + `popstate` for fragment navigation~~ → unified in `_lumen_navigate_or_fragment`.
4. ~~Confirm `history.state` reflects shell-restored `current_history_state_json`~~ → `HistoryState::set_state` wired.

**Phase 2 — Navigation API wiring:**
5. ~~Add `key`/`id` to `NavEntry`; build shell-side entry list + index accessors. Add `_lumen_navigation_*` native bindings. Rewrite the shim's `_entries`/`_currentIndex` to read from the shell.~~ ✅ DONE
6. ~~Route `navigation.navigate()` / `back()` / `forward()` / `traverseTo()` to the shell~~ ✅ DONE for navigate/back/forward; `traverseTo(key)` now does real key lookup via `navigate_to_key` + `navigate_by`.
7. ~~Implement `dispatch_navigate_event`~~ ✅ DONE (`_lumen_dispatch_navigate` fires `NavigateEvent` before navigation). Intercept result round-trip deferred to 7b.
8. ~~Implement intercept → same-document path~~ ✅ DONE — `_lumen_dispatch_navigate` reports intercept/cancel; shell skips reload, runs handler via `_lumen_run_navigate_handler`, commits same-document `NavEntry` from `InterceptedSuccess`, fires `navigatesuccess` + `currententrychange`.
9. ~~Unify same-document back/forward to fire both `popstate` and Navigation `navigate(traverse)` + `currententrychange`~~ ✅ DONE — `navigate_back`/`navigate_forward` fire `popstate` + `currententrychange` on same-document traversals; `traverse` event fires via `_lumen_dispatch_navigate`.
10. ~~SPA title update without reload~~ ✅ DONE — intercepted `InterceptedSuccess` payload updates `Lumen::title` + `window.set_title` from handler result without reload.

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
- `navigate` event fires for link clicks, address-bar navigations, `location.href=`, and back/forward, with correct `navigationType`. (shell-authoritative dispatch added; intercept round-trip deferred)
- `traverseTo(key)` performs real key lookup in the shell history stacks. ✅
- `NavigateEvent.intercept()` + `preventDefault()` round-trip — shell honors cancellation / same-document intercept. **Next**. 
- History API gaps (multi-step `go`, `go(0)`, fragment `popstate`, post-back `history.state`) resolved.
- `cargo check` / `clippy` green for `lumen-shell` + `lumen-js`.
- `CAPABILITIES.md` History/Navigation rows updated; `docs/plan/phases.md:126` item marked done when Phase 2 complete.
