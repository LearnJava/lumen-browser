# BUG-291 — `testharness.js`'s built-in results renderer (`Output.show_results`) throws `TypeError: Cannot read properties of null (reading 'appendChild')`, aborting harness completion

**Статус:** FIXED 2026-07-17
**Компонент:** js (`crates/js/src/dom.rs`, `crates/engine/layout/src/selector_query.rs`) — `Element`/`DocumentFragment`/`ShadowRoot.querySelector(All)` scoping, plus missing `insertAdjacentText`/`insertAdjacentElement`
**Найден:** P2-wpt S4/S5 (`docs/tasks/p2-wpt-integration.md`), re-running `tests/wpt/run_smoke.py` after landing the BUG-280 fix (`window === globalThis`)

## Симптом

With BUG-280 fixed (`window`/`self`/`globalThis` are now the same object, so `testharness.js`'s
`expose()`-based public API is bare-reachable), `tests/wpt/run_smoke.py` against
`/dom/nodes/Element-hasAttribute.html` still times out — `window.__lumen_wpt_results`
(`tests/wpt/resources/testharnessreport.js`'s `add_completion_callback` payload) is never set, even
though `document.readyState` reaches `"complete"` and the page's `test()` calls run and report results
normally (`add_result_callback` fires as expected).

## Причина (confirmed via BiDi `script.evaluate` bisection)

`testharness.js` registers **two** `add_completion_callback` handlers on a `WindowTestEnvironment`:
1. `WindowTestEnvironment.prototype.on_tests_ready` (registered first) → `Output.prototype.show_results`
   (renders the built-in HTML results table into the page).
2. `tests/wpt/resources/testharnessreport.js`'s own callback (registered second, by the page's own
   `<script>`) → sets `window.__lumen_wpt_results`.

`notify_complete()` iterates registered completion callbacks via a plain `forEach` with **no per-callback
try/catch**. Instrumenting a scratch copy of `testharness.js` (temporary, not committed) confirmed:
`tests.all_done()` correctly returns `true` and `tests.complete()` is called, but it throws before
returning:

```
TypeError: Cannot read properties of null (reading 'appendChild')
    at Output.show_results (testharness.js, the tbody.lastChild.lastChild.appendChild(...) line)
    at Tests.notify_complete
    at all_complete (Tests.complete)
```

Because callback #1 throws, callback #2 (`testharnessreport.js`'s, which is what `run_smoke.py` actually
polls for) **never runs** — the harness silently never reports a result, and `run_smoke.py` times out
waiting on a global that will never be set. This is unrelated to BUG-280: it reproduces identically
regardless of `window`/`globalThis` identity, and is a second, independent engine gap that BUG-280's fix
only *exposed* (previously, `testharness.js` never even reached this point — `expose()`'s targets were
unreachable as bare identifiers, so the harness never started successfully at all).

`Output.show_results` builds one `<tr>` per test result via a small recursive `["tag", {attrs},
children...]` → DOM tree-builder (`make_dom`/`make_dom_single`, all `document.createElementNS` +
`appendChild`), then immediately does `tbody.lastChild.lastChild.appendChild(...)` to attach a nested
assertions node — `tbody.lastChild.lastChild` evaluates to `null` there, i.e. either the `<tr>` (or one of
its `<td>` children) doesn't have the expected `lastChild` after the builder finishes appending it.

**A related DOM anomaly, found while isolating this, may be the same root cause or a contributing one:**
node references returned by repeated property access are not stable under `===`. Repro (minimal, isolated
from testharness.js — a scratch page, not committed):

```js
var tr = document.createElementNS(ns, "tr");
var td = document.createElementNS(ns, "td");
tr.appendChild(td);
tbody.appendChild(tr);
console.log(tbody.lastChild === tr); // false in Lumen — same underlying node, different JS wrapper object
```

`tbody.lastChild` returns a *new* JS wrapper for the same underlying native node rather than the same
object identity as `tr` — every `.lastChild`/`.firstChild`/etc. access apparently mints a fresh wrapper
instead of reusing a cached/interned one. This alone doesn't reproduce the `null` crash in isolation (a
standalone minimal repro of the exact `tbody.appendChild(makeTr()); tbody.lastChild.lastChild.appendChild(...)`
pattern, without the rest of `testharness.js`'s `render()`/`substitute()` pipeline, did **not** crash —
so the trigger needs more of that pipeline, or multi-row/accumulated `tbody` state, to reproduce standalone).
Worth checking regardless: any JS code (WPT tests, real-world sites) that compares DOM nodes by
reference (`nodeA === nodeB`, `Array.prototype.indexOf`, `Set`/`Map` keyed by node) will silently
misbehave if node wrappers aren't stable.

## Repro

1. Build `lumen.exe` (`dev-release`), the P2-wpt S4 BUG-280 fix must already be applied (`window ===
   globalThis`).
2. `LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` — times out waiting for
   `testharnessreport.js` results.
3. Or drive `tests/wpt/dom/nodes/Element-hasAttribute.html` directly over BiDi (`script.evaluate`) and
   poll `window.__lumen_wpt_results` — stays `null` indefinitely; `document.readyState` reaches
   `"complete"` immediately.

## Root cause (found by exact reproduction, not the `===`-identity anomaly above)

Copying `Output.show_results`'s real `render`/`substitute`/`make_dom` pipeline verbatim into an isolated
Rust-level test (see Fix) and running it against a `section > table > tbody` tree built **off-document**
(exactly what `show_results` does — the whole results table is assembled before being appended to `log`)
reproduced the crash immediately, on the very first row: `section.querySelector("tbody")` itself returned
`null`.

Cause: `Element.prototype.querySelector`/`querySelectorAll` (and the `ShadowRoot`/`DocumentFragment`
equivalents) all called the same native `_lumen_query_selector(_all)`, which takes **only a selector
string** — no scope node — and always searches from `doc.root()`
(`crates/engine/layout/src/selector_query.rs::query_all`). For an element that's part of the live
document this over-broadly searches the *whole page*, not just the element's descendants (a separate,
now also-fixed spec violation); for a **detached** subtree (no path to `doc.root()` at all) it finds
nothing, ever, and silently returns `null` — no error, no signal that anything is wrong. `tbody =
section.querySelector("tbody")` being `null` is exactly what makes the next line,
`tbody.appendChild(...)`, throw `Cannot read properties of null (reading 'appendChild')` — matching the
observed symptom precisely (this call, not the `tbody.lastChild.lastChild.appendChild(...)` call further
down that the original stack trace pointed at — both throw the identical message, and the crash on the
first row happens before that second call is ever reached).

The `===`-identity anomaly documented above is real (see Fix) but is **not** what caused this crash — a
faithful minimal repro without any `===` comparisons reproduced it, and disproving the identity theory
required actually copying the real vendored code path.

## Fix

1. **`Element`/`DocumentFragment`/`ShadowRoot.querySelector(All)` are now scoped** to the calling node's
   descendants (DOM Parentnode §4.2.5), not the whole document: new
   `lumen_layout::query_all_scoped(doc, scope, sel)` walks only `scope`'s subtree (excluding `scope`
   itself), with new natives `_lumen_query_selector_scoped`/`_lumen_query_selector_all_scoped` in both
   engines. `document.querySelector(All)` is unchanged (still whole-document, which is correct for
   `Document`). This also fixes the case that actually crashed: querying inside a subtree not yet attached
   to the document.
2. **`insertAdjacentText`/`insertAdjacentElement`** (HTML LS §4.9.2) were entirely missing — found because
   fixing (1) let `Output.show_results` reach `get_asserts_output`, which calls
   `asserts_output.querySelector("summary").insertAdjacentText("afterend", "No asserts ran")`
   unconditionally for every test with no recorded asserts. Added, delegating to the existing
   `before`/`after`/`prepend`/`append` methods.
3. **Node-wrapper identity** (the `===` anomaly above) is also fixed: `_lumen_make_element` now interns
   wrappers in a `_lumen_element_wrappers[nid]` cache instead of minting a fresh object every call,
   purged per-nid by the existing idle `_lumen_gc_collect` tick alongside `_input_values`/`_canvas2d_ctxs`.
   Real-world JS that compares DOM nodes by reference (`testharness.js`'s own results renderer among it)
   now behaves like a real engine.

Verified: a Rust-level reproduction of the exact `Output.show_results`/`get_asserts_output` code path
(`crates/js/src/v8_runtime.rs::bug291_testharness_results_table_pattern_does_not_throw`) no longer throws;
a standalone BiDi probe against a really-spawned `lumen.exe` (dev-release, default V8 backend) driving
`/dom/nodes/Element-hasAttribute.html` reaches `window.__lumen_wpt_results` within ~1-2s. Full
`cargo test -p lumen-js` (both engines, 2310 + 2433 tests) and `cargo test -p lumen-layout` green, no
regressions.

**Not fully closed:** `tests/wpt/run_smoke.py` (the full `wptrunner` harness, its own `wptserve` +
multiprocess executor) still times out on the same page for a reason unrelated to this bug — the
standalone probe above proves the page itself now completes quickly. Tracked separately as
[BUG-295](BUG-295-OPEN.md); the S4 checkbox at `docs/tasks/p2-wpt-integration.md:328` stays open until
that's resolved.
