# BUG-591: global uncaught-exception / unhandled-rejection reporting pipeline is entirely unwired

**Статус:** OPEN
**Компонент:** js/V8 host layer (`crates/js/src/v8_runtime.rs` -- `TryCatch` is used locally around each `eval`/module-eval call, `grep -c "TryCatch" v8_runtime.rs` finds no scope that reports back to page script; `crates/js/src/dom.rs` -- `window.onerror`/`window.onunhandledrejection` are never assigned by any engine hook, `reportError()` at `dom.rs:12864` exists but is only invoked *manually* by page script, never from the V8 host on a real uncaught exception; `PromiseRejectionEvent`/`'unhandledrejection'` do not exist anywhere -- `grep -n "unhandledrejection\|PromiseRejectionEvent" crates/js/src/*.rs` is empty)
**Найден:** P2, WPT-VENDOR-html-webappapis, 2026-08-04

## Симптом

None of the following ever fire, for any of: a compile (syntax) error, a runtime
exception, or an unhandled promise rejection, regardless of where the code that
throws/rejects originates (`<script>`, `<script src>`, `setTimeout`/
`setInterval`, `requestAnimationFrame`, `queueMicrotask`, an event handler
content attribute, a Worker):

- `window.onerror` (assigned as a property)
- `window.addEventListener('error', ...)`
- `<body onerror="...">` (HTML LS special global-event-handler forwarding)
- `window.onunhandledrejection` / `window.addEventListener('unhandledrejection', ...)`

```
FAIL window.onerror - runtime error in <script> - assert_true: ran expected true got false
FAIL <body onerror> - runtime error in <script> - assert_true: ran expected true got false
```
(`html/webappapis/scripting/events/onerroreventhandler.html`,
`event-handler-attributes-body-window.html`, self-contained, no iframe)

```
TIMEOUT html/webappapis/scripting/processing-model-2/unhandled-promise-rejections/promise-rejection-events.html
TIMEOUT html/webappapis/animation-frames/callback-exception.html
TIMEOUT html/webappapis/microtask-queuing/queue-microtask-exceptions.any.html
```

## Причина

V8 exceptions raised while running an already-established callback (a timer
firing, an rAF callback, a queued microtask, a `<script>` element's own body)
are caught locally with a `TryCatch` deep inside `v8_runtime.rs` purely to
convert them into a Rust `JsError` for the calling Rust code -- there is no
`v8::Isolate::set_capture_message_for_uncaught_exceptions` /
`v8::Context::set_promise_hook`-style bridge that turns a caught exception (or
a promise settling as rejected with no `.catch`) back into a page-visible
`ErrorEvent`/`PromiseRejectionEvent` dispatched on `window`. `reportError()`
(HTML LS §8.1.3.6) exists and works, but only because page script calls it
directly -- it is never invoked by the engine itself on a genuine uncaught
error, so it does not substitute for the missing hook.
`PromiseRejectionEvent` as a constructible interface does not exist at all.

## Масштаб

The single largest failure cluster of the `html/webappapis` slice by a wide
margin -- at least 35 distinct test files across `scripting/events/*`
(`onerroreventhandler.html`, `event-handler-attributes-*`,
`event-handler-processing-algorithm-error/*`, `compile-event-handler-*`),
`scripting/processing-model-2/*` (`compile-error-cross-origin-*`,
`runtime-error-cross-origin-*`, `unhandled-promise-rejections/*`),
`animation-frames/*`, `microtask-queuing/*`, and `timers/*-report-exception`.
Every one of these tests is unreachable past its first assertion without this
pipeline, so the true count of affected subtests across the whole WPT corpus
(any category whose tests rely on `window.onerror`/`unhandledrejection` firing
to detect an unexpected failure, not just to test the feature itself) is
almost certainly larger than what this one slice shows.

**Reconfirmed for the `Worker`/`SharedWorker` parent-side error mechanism**
(P2, WPT-VENDOR-secure-contexts, 2026-08-06) — a distinct HTML LS mechanism
from the in-worker `window.onerror` case above (HTML LS "runtime script
errors": an uncaught top-level worker exception must dispatch an `ErrorEvent`
named `error` at the owning `Worker` object). Confirmed by source read +
live probe (`--mcp-live-port`, `new Worker("data:text/javascript,throw new
Error('boom')")`): `run_worker_thread_v8` (`crates/js/src/worker.rs:699-702`)
catches the top-level script's `Err` from `rt.eval(&script)` only to
`eprintln!` it to the host's stderr — nothing is posted back through the
worker's reply channel, so the parent never learns the worker's script
failed. On the JS side, `Worker.prototype` (`worker.rs:502-521`) defines
only an `onmessage` accessor; there is no `onerror` accessor at all (so
`worker.onerror = fn` is a dead plain-property write, never invoked), and
`Worker.prototype.addEventListener` only recognizes `type === 'message'` —
`addEventListener('error', fn)` is silently a no-op, not even queued. Net
effect: a worker that throws at top level (or in a later `postMessage`
handler — same `rt.eval` error path) looks to the parent exactly like a
worker that never posts anything back — an unrecoverable silent hang,
reproduced live with both `throw new Error(...)` and a bare
`ReferenceError` (`postMessage(isSecureContext)`, the literal expression
`secure-contexts/basic-dedicated-worker.html`'s `w7` subtest uses) at a
worker's top level. This is the mechanism `tests/wpt/secure-contexts`'s
`basic-dedicated-worker*.html`/`basic-shared-worker*.html` rely on for
their `data:` URL worker subtest and is one contributor (alongside
[BUG-364](bugs/BUG-364-FIXED.md), which blocks that category's non-`data:`
worker subtests separately) to that category's 0/8 harness OK.

**Partially addressed by the [BUG-364](bugs/BUG-364-FIXED.md) fix (P3,
2026-08-09):** the accessor-level gap described above — `Worker.prototype`
having no `onerror` at all and `addEventListener('error', …)` being a silent
no-op — is now closed; both exist and fire for the script-fetch-failure case
BUG-364 added. **The core mechanism reconfirmed here is still broken**:
`run_worker_thread_v8`'s `rt.eval(&script)` error arms (top-level script
error, and the same path for a later `postMessage` handler) still only
`eprintln!` to the host's stderr and never post anything back through the
worker's reply channel — an uncaught exception inside an *already-started*
worker still cannot reach the parent's `error` handler. This bug stays OPEN
for that mechanism.

**Corpus-scale confirmation (P2, WPT-RUN-6, 2026-08-20/21):** the
`eprintln!`-only path is directly visible, one line per occurrence, in every
`.tmp/wpt-corpus/*.log` from the completed WPT-RUN-5 Windows run —
`[worker-0] v8 script error: Runtime("importScripts: cannot load script:
/resources/testharness.js")` / `[shared-worker] v8 script error:
Runtime("importScripts is not supported")` — the exact stderr line this
section describes, at the exact moment `.any.worker.html`/`.worker.html`/
`.any.sharedworker.html`'s wptrunner-generated bootstrap fails on its first
statement (see [BUG-778](bugs/BUG-778-OPEN.md) for the importScripts gap
itself). Because nothing reaches the parent, `fetch_tests_from_worker()`
just waits out its ~10s per-file timeout: 1210 of 6205 TIMEOUT ids in that
run (19.5%) are this exact mechanism, the single largest TIMEOUT cluster
measured in the whole corpus. Fixing the missing-feature side (BUG-778)
alone would likely convert most of these from TIMEOUT to a fast ERROR/FAIL
(testharness.js still wouldn't load) — this pipeline gap is what's needed on
top for the parent to *observe* that quickly instead of waiting out the
timeout, and would also matter independently for the many `html/webappapis`
files listed above.
