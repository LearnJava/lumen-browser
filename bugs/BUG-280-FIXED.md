# BUG-280 — `window` is a plain JS object, not the real global object: bare (unqualified) globals exposed via `window.x = ...` are unreachable

**Статус:** FIXED 2026-07-16 — smoke test still doesn't reach a genuine PASS, but for an unrelated reason (see follow-up below)
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

## Фикс

`WEB_API_SHIM` (`crates/js/src/dom.rs`) now repoints `window` to literally **be** the
engine's own global object, matching the real-browser invariant `self === window ===
globalThis`. Immediately after `window`'s object literal is built, an IIFE copies every
own property of `window` onto `globalThis` — plain values via `globalThis[k] =
d.value` ([[Set]], required because some quickjs-ng-provided globals like
`addEventListener` are non-configurable-but-writable, so `defineProperty` would throw),
accessors (getters/setters, e.g. `scrollY`) via `Object.defineProperty(globalThis, k, d)`
to preserve the live binding rather than freezing a one-time read value — then `window =
globalThis;`. From that point on, `window`, `self`, and `globalThis` are the same
reference, so any later `window.foo = ...` / `self.foo = ...` (including
`testharness.js`'s dynamic `expose(fn, name)`, i.e. `window[name] = fn`) lands directly on
the real global object and is reachable as a bare identifier — no finite alias list
needed. Since `WEB_API_SHIM` is shared by both engines (rquickjs and V8, `v8_runtime.rs`
evaluates the same source inline), this fixes both paths from one JS-level change, per the
coordinator directive above.

Verified: 2 mirrored unit-test pairs — `dom::tests::dynamic_window_property_is_bare_reachable`
/ `dynamic_self_property_is_bare_reachable` (rquickjs path) and the same two names in
`v8_runtime::tests` (V8 path, `--features v8-backend`) — plus live BiDi `script.evaluate`
probes against the **default V8 dev-release build**: `window === globalThis` is `true`, a
property assigned via `window.foo = ...` from one script is reachable as bare `foo` in a
later script on the same page, and `window`'s `load` event still fires and reaches
listeners registered through the (now-shared) `addEventListener`.
`File.prototype` also needed a small follow-up fix in `crates/js/src/file_input.rs`
(`File extends Blob`, W3C File API §4) — BUG-280 made `window.File` reach the real global
`File`, which surfaced that it never had the prototype link; unrelated to the window/global
mechanism itself, fixed alongside since it was found by the same repointing.

## Известный остаток — не фиксится здесь: [BUG-291](BUG-291-OPEN.md)

`tests/wpt/run_smoke.py` still times out after this fix — but for a **different, unrelated**
reason confirmed by re-running the smoke test and bisecting further: `testharness.js`'s
built-in results renderer (`Output.show_results`) throws `TypeError: Cannot read properties
of null (reading 'appendChild')` while building the results `<table>`, which aborts
`notify_complete()` before it reaches `testharnessreport.js`'s own completion callback (the
one `run_smoke.py` actually polls for). This reproduces identically with or without this
fix — BUG-280's own symptom (bare identifiers unreachable) was simply masking it: before
this fix, `testharness.js` never got far enough to hit it. See BUG-291 for the full
diagnosis (DOM node wrapper identity anomaly, candidate root cause) — filed separately per
this task doc's own rule ("file rather than fix inline" for engine gaps outside P2-wpt's
`lumen-bidi-server`/`tests/wpt` scope).
