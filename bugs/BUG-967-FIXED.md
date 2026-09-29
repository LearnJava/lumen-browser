# BUG-967: `<style>@import ...</style>` never fires `load`/`error` and its
`@import` is never fetched when the element arrives via `cloneNode`+`appendChild`
after the initial parse

**Статус:** FIXED 2026-09-29 (P6)
**Дата:** 2026-09-03
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` §4.14 "update a
style block") + shell (`crates/shell/src/relayout.rs::refresh_dynamic_css`)
**Найден:** P2, WPT-RUN-6 срез 54, живой пробой

## Механизм

`<style>` "update a style block" (§4.14) only fires `load`/`error` — and only
fetches its `@import` URLs (`_lumen_style_load_imports`,
`web_api_shim_mid.js:8770`) — when `_lumen_style_update_block` is reached
through one of three triggers, all of which a `cloneNode(true)`-then-append
insertion misses:

1. `document.createElement`/`createElementNS` calls `_lumen_resource_track`
   (`web_api_shim_mid.js:7882`/`7901` → `8159`), which records the element's
   nid as pending in `_lumen_resource_pending`; the wrapped `appendChild`/
   `insertBefore` later finds it there via `_lumen_resource_try_prepare`
   (~line 8879) and prepares it. **`cloneNode` never calls
   `_lumen_resource_track`** — it goes through a native `_lumen_clone_subtree`
   (`web_api_shim_mid.js:5932-5935`), so a cloned `<style>`'s nid is never
   registered as pending.
2. `_lumen_style_children_changed(parent)` (`web_api_shim_mid.js:8818`,
   wired into the same `appendChild`/`insertBefore`/`textContent=`/
   `innerHTML=` hooks) only fires when the **mutated node itself** is already
   a tracked `<style>` element (e.g. `styleEl.appendChild(textNode)`).
   Appending a clone into `<head>`/anywhere calls it with `parent` = the
   insertion target, not the style element, so it's a no-op for this case.
3. `_lumen_style_blocks_scan()` (`web_api_shim_mid.js:8829`) walks the whole
   tree exactly **once**, at `readyState → 'interactive'`
   (`web_api_shim_tail_b.js:579`) — a `<style>` inserted after that point,
   however it got into the tree, is simply never seen by it.

On the Rust side, `refresh_dynamic_css` (`crates/shell/src/relayout.rs:53-88`)
does notice a new/changed `<style>` block (fingerprint diff) and reparses the
merged inline CSS (`lumen_css_parser::parse`, line 77 — the
"CSS пересобран после правки `<style>`: N правил" log line), so an `@import`
inside it *is* parsed into `Stylesheet::imports`. But nothing in that function
reads `sheet.imports` — the field is dropped, by design ("Сеть не трогается:
`@import` внутри нового листа останется неразрешённым", lines 50-52) — so the
network fetch never happens here either. A cloned `<style>`'s `@import`
therefore falls through **every** path that could resolve it: not the JS
per-element trigger (never tracked), not the JS children-changed trigger (not
itself the mutated node), not the one-shot parser scan (already ran), and not
the Rust relayout path (deliberately network-inert).

`<link rel=stylesheet>` does not have this gap: the shell rebuilds its list of
`<link>` hrefs by walking the **whole current document tree**
(`collect_link_hrefs`, used by `load_linked_stylesheets`,
`crates/shell/src/stylesheets.rs:66-98`) on every cascade pass, so a cloned
`<link>` is found and fetched regardless of how it entered the tree — there is
no equivalent full-tree rescan for `<style>` blocks.

## Симптом

Confirmed live (`--mcp-live-port`, `tests/wpt/verify_slice54_gaps.py` +
ad hoc diagnostics, 2026-09-03, `main` = `64b92633c`):

- A `<style>` built with `document.createElement('style')` and appended
  directly (`head.appendChild(style)`) fires `load` and issues the `@import`
  fetch — works.
- A `<style>` nested one level inside a **freshly-created** wrapper `<div>`
  (`createElement`, no `cloneNode`), then the wrapper appended in one call —
  also works (rules out "nested inside a just-appended subtree" alone).
- A `<style>` obtained from `<template>.content.cloneNode(true)` (the exact
  upstream idiom — a `<template>` written in markup, its content cloned, the
  clone appended into a live subtree) never fetches its `@import` and never
  fires `load`/`error` — hangs forever. Reduced repro:
  `tests/wpt/css/css-cascade/scope-implicit-external.html`'s second
  `promise_test` ("`@scope` with external stylesheet through `@import`"):

  ```
  harness-complete status=2 tests=2
    @scope with external stylesheet through link element:1   (FAIL, unrelated — see below)
    @scope with external stylesheet through @import:2          (TIMEOUT)
  ```

  Only **one** `GET .../scope.css` is logged for the whole page (from the
  `<link>` subtest); the `<style>`+`@import` subtest never issues a second
  request at all, and `eprintln!("CSS пересобран после правки <style>: {}
  правил")` prints `0 правил` for it — the reparse happens, the import URL is
  parsed, and both are then discarded.

Real-world trigger: `/css/css-cascade/scope-implicit-external.html`, subtest
"`@scope` with external stylesheet through `@import`" — matches the TIMEOUT
signature recorded for this id in the WPT-RUN-5 corpus snapshot.

## Отдельная (не эта) находка того же файла

The sibling subtest ("through link element") does **not** hang — its `<link>`
fires `load` and the promise settles — but it FAILs an assertion:
`getComputedStyle(div).zIndex` reads `"2"` for the two top-level `.outside`
divs, which the test expects to stay `"auto"`. The stylesheet's rule is
`@scope { :scope { z-index:1 } .a { z-index:2 } }` (implicit scope root, no
`(<scope-start>)` prelude) — `.outside` shares class `a` with the in-scope
elements, so this looks like the implicit-root `@scope` boundary not being
honored for an externally-loaded (`<link>`) stylesheet, letting the scoped
`.a` rule leak past its intended root
(`crates/engine/layout/src/style/env.rs:239-277`,
"implicit scope = document root: every element is in scope unless it sits
[...]" — CSS Cascade-6 §3 defines the implicit root as *the parent of the
style sheet's owner node*, not the document root). Not investigated further
this slice — it's a FAIL, not the TIMEOUT this slice was triaging — but worth
a bug of its own; noted here rather than silently absorbed into BUG-967's
classification (`docs/probe-method.md` §3: one id, ≥1 defect).

## Масштаб

Any CSS-in-JS pattern that builds a `<style>` block from a `<template>` (or
otherwise via `cloneNode`) and relies on its `load`/`error` event or on its
`@import` actually being resolved will hang or silently miss styles — this is
the same shape as the already-fixed BUG-703 (`<link>` insertion not
signalling load) and BUG-804 (parser-inserted resource events), but for the
`cloneNode` insertion path specifically, which neither of those covered.

## Что нужно

Either give cloned `<style>`/resource-bearing elements the same
"already-started" bookkeeping `createElement` gets (teach the native
`_lumen_clone_subtree` clone path to call `_lumen_resource_track` for any
clone of a `script`/`link`/`track`/`source`/`style` node), or add a
full-tree-rescan trigger for `<style>` blocks analogous to
`collect_link_hrefs` so a `<style>` reachable from the document is found
regardless of how it got there — and only then decide whether
`refresh_dynamic_css`'s "relayout never touches the network" rule should grow
a narrow exception for `@import`, or whether the JS-side fetch
(`_lumen_style_load_imports`) is meant to be the sole source of truth for
`<style>`-level imports (it already handles the network side end-to-end; the
Rust side parsing `sheet.imports` and doing nothing with it may just be dead
code worth removing once the JS side owns this fully).

## Классификация WPT-RUN-6

Attributed via `_exact_id_marker("/css/css-cascade/scope-implicit-external.html")`
in `tests/wpt/timeout_audit.py` (marker `style-clone-import-not-fetched`).

## Фикс (2026-09-29, P6)

Taught the three `cloneNode`/`importNode` call sites that go through the
native `_lumen_clone_subtree` (`web_api_shim_mid.js` — `DocumentFragment
.cloneNode`, `Element.prototype.cloneNode`, `Document.importNode`) to walk the
cloned subtree and register any `<style>` descendant in
`_lumen_resource_pending` as `'style'`, the same bookkeeping
`_lumen_resource_track` gives a `createElement`d one
(`_lumen_track_cloned_style_blocks`, new function next to
`_lumen_resource_track`). No native/Rust change: `_lumen_clone_subtree`
returns only the new root, so the walk uses the existing `_lumen_get_children`
binding recursively — the only thing missing was the JS-side "already
started" flag a clone never got.

Unlike `<script>`/`<link>`, `<style>` has no spec "already started" flag to
preserve across a clone (§4.14 has no such state), so tracking every clone
unconditionally is correct: each copy independently runs "update a style
block" once inserted, exactly like one built with `createElement`. Scripts/
links/tracks/etc. are deliberately NOT extended the same way — their
"already started" semantics are why they stay out of `_lumen_resource_track`
for parser/clone origins (see the comment above `_lumen_resource_pending`),
and that is a separate question from this bug.

Once a cloned `<style>`'s nid is pending, the existing insertion hook
(`_lumen_resource_after_insert`'s ancestor fallback loop, `web_api_shim_mid.js`
~14297-14301) picks it up on `appendChild`/`insertBefore` regardless of
whether the fragment or one of its children is the literal argument passed to
the native call — no change needed there.

Verified live (`tests/wpt/verify_slice54_gaps.py --binary target/dev-release/lumen.exe
--id /css/css-cascade/scope-implicit-external.html`, dev-release,
`p6-bug-967-style-clone-import`): before the fix, `harness-complete status=2`
(TIMEOUT) with the `@import` subtest marked `:2`. After: `harness-complete
status=0 tests=2 @scope with external stylesheet through link element:1|@scope
with external stylesheet through @import:1` — both subtests now complete (no
more hang); both still FAIL the same assertion, which is the pre-existing
`@scope` implicit-root leak noted above and now filed on its own as
[BUG-1216](BUG-1216-OPEN.md) (previously masked by this bug's TIMEOUT for the
`@import` subtest).
