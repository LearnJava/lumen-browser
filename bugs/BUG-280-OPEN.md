# BUG-280 — `window` is a plain JS object, not the real global object: bare (unqualified) globals exposed via `window.x = ...` are unreachable

**Статус:** OPEN — blocks P2-wpt S4's smoke test from reaching a clean PASS through `wptrunner`
**Компонент:** js (`crates/js/src/dom.rs`, JS environment bootstrap — `WEB_API_SHIM`)
**Найден:** P2-wpt S4 (`docs/tasks/p2-wpt-integration.md`), running the vendored, unmodified
`resources/testharness.js` against a real page over BiDi.

## Симптом

After BUG-279 (`document.getElementsByTagName`) was fixed, `testharness.js` loads and runs its full
module-level setup without throwing — but the globals it's supposed to expose (`test`,
`add_completion_callback`, `assert_true`, `setup`, …) are still unreachable as bare identifiers from
both `resources/testharnessreport.js` (the P2-wpt S4 shim, `add_completion_callback(...)`) and the test
page's own inline `<script>` (`test(() => {...}, "...")`) — both throw `ReferenceError`-class
`"<name> is not defined"` even though `testharness.js` completed its `expose(test, 'test')` /
`expose(add_completion_callback, 'add_completion_callback')` calls without error.

## Причина (confirmed)

`crates/js/src/dom.rs` builds `window` as **a plain JS object literal** (`var window = {...}`), not the
engine's actual global object — a comment at the `self`/`window`/`globalThis` aliasing block (added for
BUG-233, webpack `self` compat) already says this explicitly: *"The shim builds window as a plain object
literal, so without this block bare `self` is a ReferenceError."*

`testharness.js`'s entire public-API design assumes `global_scope` (bound to `self` via its trailing
`})(self);`) **is** the real global object — universally true in every actual browser/worker (`self ===
window === globalThis`, same object, IS the lexical global). Its `expose(object, name)` helper
(`crates/js/src/dom.rs`-vendored `resources/testharness.js`) does `target[name] = object` where
`target = global_scope`, i.e. `window[name] = object` in Lumen's case — this makes `window.test`,
`window.add_completion_callback`, etc. reachable, but **not** the bare, unqualified identifiers `test`,
`add_completion_callback` that every WPT test (and `testharness.js`'s own shim files) is written to call
directly.

BUG-233's fix (bare `self`/`window`/`globalThis` aliases) and this task's own partial mitigation (bare
`addEventListener`/`removeEventListener`/`dispatchEvent` aliases, added alongside this bug report — see
`WEB_API_SHIM`) work because those specific names are known **in advance** and can be manually aliased
with `var name = window.name.bind(window);`. `testharness.js`'s `expose()` calls are **dynamic and
open-ended** — `add_completion_callback`, `assert_*` (~30 assertion functions), `test`, `async_test`,
`promise_test`, `setup`, `done`, `step_timeout`, `EventWatcher`, `AssertionError`, … — and any other
script (WPT test bodies, real-world sites) can assign new properties onto `window`/`self` at any time and
expect to reference them unqualified afterward. There is no finite list of names to alias in advance.

## Почему это не узкий фикс

Making `window`/`self`/`globalThis` bare-alias-mirror *every* property assigned to them dynamically would
require either:
1. Making `window` literally **be** the QuickJS engine's global object (the correct, real-browser-matching
   architecture) — likely a substantial rework of how `crates/js` bootstraps the JS environment, since
   `window` is currently built as ordinary JS source (`WEB_API_SHIM`) rather than wired to the engine's
   `globalThis` at the native binding layer.
2. A `Proxy`-based (or engine-level `with`-scope-equivalent) global object that forwards unqualified
   identifier lookups to `window`'s properties dynamically — QuickJS/`rquickjs` API support for this needs
   investigation.

Both are JS-engine-architecture decisions outside `lumen-bidi-server`/`tests/wpt` (P2's stated scope,
`docs/tasks/p2-wpt-integration.md`) — this is exactly the kind of engine gap that task doc says to file
rather than fix inline ("Never weaken a vendored test to force a pass").

## Impact beyond WPT

This is not WPT-specific — it breaks **any** real-world script (or embedded library) that defines a
global via `window.foo = ...` / `self.foo = ...` and later calls it as a bare `foo(...)`, a very common
JS authoring pattern (the BUG-233 webpack case is one instance of the same underlying class of bug).

## Repro

1. Build `lumen.exe` (`dev-release`), start `lumen --bidi-port <N>` and a local static HTTP server.
2. Navigate to a page containing: `<script>window.foo = function(){ return 1; }; console.log(typeof
   window.foo, typeof foo);</script>` — the second `typeof` (bare `foo`) is `"undefined"`; the first
   (`window.foo`) is `"function"`.
3. Or: `LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` (P2-wpt S4's smoke driver) —
   fails with `TIMEOUT` waiting for `tests/wpt/resources/testharnessreport.js`'s
   `add_completion_callback` to ever fire, because it's never reachable as a bare identifier.

## Что нужно для закрытия

Investigate whether `rquickjs`/QuickJS supports binding a custom object as the true global object (or a
global `Proxy`), and if so, migrate `window` to be that object instead of a `WEB_API_SHIM`-authored plain
literal — or find another mechanism that makes `window`/`self` properties bare-identifier-reachable
generally, not just for a hardcoded alias list. Re-run `tests/wpt/run_smoke.py` afterward — DoD unblocks
the rest of P2-wpt S4/S5.

## Directive (2026-07-16, coordinator)

**V8 is the default JS engine since the S12 cutover (ADR-018, 2026-07-14).** The
"Investigate whether rquickjs/QuickJS supports…" wording above predates the cutover — do
NOT invest in an rquickjs-specific mechanism: that path is rollback-only (`--features
quickjs`) and is being deleted slice-by-slice (S12b). The fix must land either in the
engine-agnostic `WEB_API_SHIM` (crates/js/src/dom.rs — shared by both engines; a JS-level
fix like repointing `window` to `globalThis` qualifies) or in the V8 path
(`v8_runtime.rs`). Definition of done: `tests/wpt/run_smoke.py` passes on the **default
(V8) build** — not on a `--features quickjs` build.
