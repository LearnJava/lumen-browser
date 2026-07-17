# BUG-291 — `testharness.js`'s built-in results renderer (`Output.show_results`) throws `TypeError: Cannot read properties of null (reading 'appendChild')`, aborting harness completion

**Статус:** FIXED 2026-07-17 — root cause (unstable node-wrapper identity) fixed and verified in isolation; the S4 DoD checkbox this was blocking is now blocked instead by an unrelated, pre-existing BiDi JS-context-install race (see "Остаток" below)
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

## Фикс (2026-07-17)

Node wrappers were never interned: `_lumen_make_element(nid)` (`crates/js/src/dom.rs`) built a brand-new
JS object on every call, so `.lastChild`/`.firstChild`/`.parentElement`/`.children`/etc. minted a fresh
wrapper each access — exactly the `tbody.lastChild === tr` anomaly this bug's investigation isolated.
Confirmed against the DOM arena (`crates/engine/dom/src/lib.rs`): node ids are allocated append-only
(`alloc()`, `NodeId(self.nodes.len())`) with no free-list reuse until a future Phase-3 compaction, and the
whole JS shim (`WEB_API_SHIM`) is re-evaluated from scratch on every navigation/bfcache thaw (fresh V8
isolate) — so caching a wrapper by `nid` for the life of one JS context can never alias a stale wrapper
onto an unrelated later node. Added `_lumen_node_wrappers` (a plain `{nid: wrapper}` object, following the
same per-nid-storage pattern already used for `_validity_msg`/`_input_values`/`_canvas2d_ctxs`):
`_lumen_make_element` now returns the interned wrapper if `nid` was already wrapped, and stores the new
one before returning otherwise. This is the shared, engine-agnostic shim (`WEB_API_SHIM`), so the fix
applies to both the default V8 build and the QuickJS rollback path identically.

As a side effect this also fixes silent loss of expando properties: previously `el.foo = 1; el.foo` (via
two separate accesses of the same underlying node, e.g. through `parentNode.firstChild` twice) could
silently read back `undefined`, since each access was a different object.

Regression test: `dom::tests::repeated_node_access_returns_identical_wrapper`
(`crates/js/src/dom.rs`) — reproduces this bug's own two isolated repros (`tbody.lastChild === tr` identity,
and the `tbody.lastChild.lastChild.appendChild(...)` nested-access pattern from `Output.show_results`)
directly against the shared shim via the QuickJS unit-test harness already used by neighboring tests in
this file (`element_append_and_first_child_round_trip`, `create_element_ns_builds_native_svg_tree`).
`cargo test -p lumen-js --lib` and `cargo clippy -p lumen-js --all-targets -- -D warnings` clean.

## Остаток

Re-running `tests/wpt/run_smoke.py` (and a hand-rolled direct BiDi driver, bypassing wptrunner) after this
fix still times out — but *not* on the `Output.show_results` crash this bug diagnosed. Diagnostics
(`document.readyState` + polling `window.__lumen_wpt_results` directly) show a **separate, pre-existing**
issue: a plain `lumen --bidi-port <N>` process (no other flags) races its own default-homepage navigation
(observed loading `ria.ru`) against the explicit `browsingContext.navigate` the test driver issues — the
homepage load appears to land in the same top-level context *after* the intended navigation, leaving
`window`/`document` pointing at the homepage instead of the test page, so `__lumen_wpt_results` is never
set (reproduces with a freshly emptied `data/` dir, i.e. not `last_session.db`/`settings.db` state — ruled
out explicitly). This matches the class of issue already logged in `CLAUDE.md` → "Known gotchas" ("Live
window BiDi/MCP `script.evaluate` can hang indefinitely..."), reproduced independent of this fix. Filing a
dedicated bug and root-causing the homepage/navigate race is out of scope here (P2-wpt is not this task);
`docs/tasks/p2-wpt-integration.md`'s S4 checkbox note has been updated to point at this new blocker instead
of BUG-291.
