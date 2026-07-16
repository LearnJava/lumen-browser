# BUG-291 — `testharness.js`'s built-in results renderer (`Output.show_results`) throws `TypeError: Cannot read properties of null (reading 'appendChild')`, aborting harness completion

**Статус:** OPEN — blocks `tests/wpt/run_smoke.py` from reaching a genuine PASS/FAIL result even after BUG-280 is fixed
**Компонент:** js (DOM child-node bindings, `crates/js/src/dom.rs`) — most likely `appendChild`/`lastChild`/node-wrapper identity for `createElementNS`-created elements
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

## Что нужно для закрытия

Investigate `appendChild`/`lastChild`/`firstChild` (and the DOM node → JS wrapper mapping in general —
`crates/js/src/dom.rs` and/or `crates/dom`) for `createElementNS`-created elements: confirm whether node
wrappers are cached/interned (`===` should hold for repeated access to the same node) and whether
`lastChild` can return `null`/stale data immediately after a same-tick `appendChild` sequence in a
multi-child subtree. Re-run `tests/wpt/run_smoke.py` afterward — DoD unblocks the S4 checkbox at
`docs/tasks/p2-wpt-integration.md:322` ("A deliberately-failing assertion is observed as FAIL").
