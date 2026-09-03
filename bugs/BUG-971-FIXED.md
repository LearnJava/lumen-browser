# BUG-971 — forward same-document fragment navigation never fires `popstate`
(only `hashchange`)

**Статус:** FIXED 2026-09-03
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid_b.js::_lumen_navigate_or_fragment`
and `_lumen_set_location_hash`)
**Найден:** P2, WPT-RUN-6 срез 56, живой пробой (`.tmp/probe56_popstate_sources.html`,
`.tmp/probe56_anchor_fragment.html`, не закоммичены — минимальные страницы без
`testharness.js`)

## Фикс

Both forward-navigation code paths now call a shared `_lumen_dispatch_popstate(state)`
(factored out of `_lumen_deliver_popstate`'s own dispatch, which already did
this correctly for the traversal path) before firing `hashchange`:
`_lumen_navigate_or_fragment` (covers `location.href =`/`.assign()`/
`.replace()` and the default activation of `<a href="#x">`, both push and
replace) and the dedicated `location.hash =` setter,
`_lumen_set_location_hash` — a second, independent path with the identical
gap discovered while re-measuring the first fix (`location.hash =` does not
funnel through `_lumen_navigate_or_fragment` at all). `pushState`/
`replaceState` stay untouched, per spec.

Re-measured (`probe56_popstate_sources.html`): three same-document
navigations in one page (script `location.hash =`, `<a>` click, then
`history.back()`) now produce three `popstate`s in order, one per
navigation, where before the fix only the `back()` call produced one.

The real WPT id this bug was found through
(`anchor-fragment-history-back-on-click.html`) no longer hangs after the
fix (`harness-complete status=0`, was `status=2`), but its one subtest now
FAILs on a second, distinct defect uncovered only once the hang stopped
masking it: `history.back()`'s target entry is resolved against the nav
stack's state at drain time rather than at call time, so a same-document
push racing the traversal in the same synchronous turn shifts the target by
one — filed separately as [BUG-973](BUG-973-OPEN.md), not fixed here.

## Механизм

Two independent code paths deliver a same-document fragment change, and only
one of them fires `popstate`:

- **Forward push** — `location.hash =`/`location.href =`/`location.assign()`
  to a same-document target, or the default activation behaviour of
  `<a href="#x">`, all funnel through `_lumen_navigate_or_fragment`
  (`web_api_shim_mid_b.js:200`). It updates `location`, pushes a history
  entry (`_lumen_history_push`/`_lumen_history_push_url`) and calls
  `_lumen_fire_hashchange` — **`PopStateEvent` is never constructed on this
  path.**
- **Shell-driven traversal** — `history.back()`/`forward()`/`go(n)` reach
  `_lumen_deliver_popstate` (`web_api_shim_mid_b.js:827`), which explicitly
  documents the spec requirement in its own comment ("HTML LS §7.4.6:
  traversing between two entries that differ only in their fragment fires
  popstate AND hashchange, popstate first") and does fire both.

HTML LS §7.4.6 does not carve out an exception for the *first* time an entry
is reached — "update the current entry" (used by both a plain navigation to
a new entry and a traversal to an existing one) fires `popstate` in either
direction. Lumen only implements the traversal half.

## Прямое измерение

Two minimal pages served over `http.server` (no `testharness.js`, `location:
http://127.0.0.1:8912/...`), dev-release, Linux, `main` = `8a750386e`:

1. `probe56_popstate_sources.html` — three same-document navigations in
   sequence, one `onpopstate` counter: `location.hash = '#a'` (script), then
   `document.createElement('a')` with `href="#b"`, appended to `<body>` and
   `.click()`-ed, then `history.back()`. Result: **exactly one** `popstate`
   fires (`popstate #1 hash=#a`), triggered by the `back()` call; the two
   forward pushes (script hash-set and anchor click) update `location` and
   push new history entries — a follow-up probe
   (`probe56_history_length.html`) reads `history.length` going `1 → 2 → 3`
   across the same two pushes, so the entries are real, not silently
   dropped — but dispatch nothing to `onpopstate`.
2. `probe56_anchor_fragment.html` — mirrors the WPT test's own shape
   (`anchor.onclick` calls `history.back()`, the anchor's own default action
   then navigates the hash forward): `onclick fired, calling
   history.back()` logs before `after click (sync), hash=#3`, so the click's
   default fragment navigation to `#3` completes synchronously inside
   `anchor.click()` — but the only `popstate` observed afterward reports
   `hash=#2`, i.e. the *one* traversal-path event from the queued
   `history.back()`, landing on the entry created by `location.hash = '#2'`
   two navigations earlier. The push to `#3` never produces a `popstate` of
   its own.

## Кого это держит

Real trigger:
`html/browsers/browsing-the-web/overlapping-navigations-and-traversals/anchor-fragment-history-back-on-click.html`.
Its `navigationsPromise` resolves only after two `popstate` events
(`["#3", "#1"]`); since the forward push to `#3` never fires one, the count
never reaches two and the `promise_test` hangs on its own `await` until the
harness's internal timeout — matches the WPT-RUN-5 corpus TIMEOUT signature
for this id.

Any page relying on `popstate` (not just `hashchange`) to react to a forward
fragment navigation — a common SPA-router pattern that listens once for both
— silently loses the forward half of that pair.

## Классификация WPT-RUN-6

Classified via `_exact_id_marker` in `timeout_audit.py`
(`fragment-nav-no-popstate`).
