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
