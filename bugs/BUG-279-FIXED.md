# BUG-279 — `document.getElementsByTagName` missing from the live DOM JS bindings

**Статус:** FIXED 2026-07-13 (document-level only — see follow-up below)
**Компонент:** js (`crates/js/src/dom.rs`, the live `document` object literal)
**Найден:** P2-wpt S4 (`docs/tasks/p2-wpt-integration.md`), loading the vendored, unmodified
`resources/testharness.js` against a real page — its own module-level setup
(`WindowTestEnvironment.prototype.test_timeout` / `get_script_url()`) calls
`document.getElementsByTagName(...)` unconditionally, before any test-specific code runs.

## Причина

`document`'s object literal (`crates/js/src/dom.rs`, `var document = {...}`) defines `getElementById`,
`querySelector`, `querySelectorAll` — but never `getElementsByTagName` (nor `getElementsByClassName`).
Confirmed directly: `typeof document.getElementsByTagName` was `"undefined"`, and calling it threw
`TypeError: not a function` (isolated with a scratch probe page + BiDi `script.evaluate`, bisecting
`testharness.js` execution with injected marker statements after ruling out several other hypotheses —
`window.addEventListener`, `setTimeout`/`clearTimeout`, and the `document.getElementsByTagName`-adjacent
shadow-realm check all tested fine in isolation).

`querySelectorAll`'s underlying native binding (`_lumen_query_selector_all`) already accepts a bare tag
name as a valid CSS type selector (and `'*'` as the universal selector) — used elsewhere in this same
file for custom-element tag matching (`_lumen_ce_upgrade_all`) — so no new native binding was needed.

## Фикс

Added `getElementsByTagName: function(tag) { return _lumen_query_selector_all(String(tag)).map(_lumen_make_element); }`
to the `document` object literal, delegating to the same native query `querySelectorAll` already uses.
Returns a static array, not a live `HTMLCollection` — the same simplification `querySelectorAll` already
makes for this codebase; not a new limitation introduced by this fix.

## Известный остаток (не в этом фиксе)

`Element.prototype.getElementsByTagName` (i.e. `someElement.getElementsByTagName(...)`, scoped to a
subtree) is still missing — only `document.getElementsByTagName` was added, since that's what unblocked
`testharness.js`. Note: `Element`'s existing `querySelector`/`querySelectorAll` (`_lumen_make_element`,
`crates/js/src/dom.rs` ~line 4835) call the *unscoped* `_lumen_query_selector`/`_lumen_query_selector_all`
natives directly (no `nid` passed) — appearing to already query document-globally rather than scoped to
the element's subtree, independent of this bug. Not investigated further here (out of scope for P2-wpt).
`document.getElementsByClassName` is also still missing.
