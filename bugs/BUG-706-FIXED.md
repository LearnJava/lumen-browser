# BUG-706: `MessagePort.postMessage`/`.start()` scheduled delivery as a microtask instead of a task — causes a genuine engine hang under a self-rescheduling `MessageChannel` consumer (e.g. React's Scheduler package)

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs:8915-9017` — `MessagePort`/`MessageChannel` in `WEB_API_SHIM`)
**Найден:** while root-causing [BUG-702](BUG-702-OPEN.md) (tbank.ru `react.js`+`platform.js` infinite-loop hang)

## Симптом

`MessagePort.prototype.postMessage` and `MessagePort.prototype.start` scheduled
delivery via `queueMicrotask(...)` (i.e. `Promise.resolve().then(...)`) instead
of a real task, in violation of HTML LS §9.2.3 ("port message queue" is a task
source, not the microtask queue). Standalone use (single channel, one
`postMessage`) was invisible — it just delivered "too early" relative to spec,
with no observable symptom in this codebase's own tests.

The defect becomes a **genuine, unrecoverable infinite loop** when a page runs
a `MessageChannel` consumer whose own message handler conditionally
reschedules itself by calling `postMessage` again — exactly React's
`Scheduler` package (`scheduler.production.min.js`, vendored inside real
React/ReactDOM bundles), whose entire reason for choosing `MessageChannel`
over `Promise` is to get a genuine macrotask boundary between reschedules:

```js
channel.port1.onmessage = performWorkUntilDeadline;
schedulePerformWorkUntilDeadline = function() { port.postMessage(null); };
// performWorkUntilDeadline re-invokes schedulePerformWorkUntilDeadline()
// whenever there is still work and it hasn't exceeded its time slice yet.
```

Because V8's microtask queue is auto-drained to exhaustion inside a single
`eval`/callback invocation (`kAuto` policy; `_lumen_drain_microtasks` in
`v8_runtime.rs:4061-4068` is a no-op, there is no manual drain hook), a
microtask-scheduled `postMessage` reschedule never returns control to Rust's
event loop — Rust's own timer/task tick (`_lumen_tick_timers`, called once per
real event-loop iteration) never runs, so from the shell's side this is
indistinguishable from a hard hang (100% CPU, `--dump-layout` never returns).
A second, independent `MessageChannel`-based producer running concurrently
(in the tbank.ru case: a core-js `Promise`-polyfill-detection routine's own
`setImmediate` fallback, which also uses `MessageChannel`) is what keeps the
combined microtask queue from ever quiescing on its own — see [BUG-702](BUG-702-OPEN.md)
for the full bisection trail that isolated this down to a 54 KB, fully offline
reproduction (`react.js` + a reduced `platform.js` that does nothing but
install the core-js Promise-polyfill-detection code, then require React's
Scheduler module).

## Фикс

`MessagePort.prototype.postMessage` and `MessagePort.prototype.start`
(`dom.rs:8915-8976`) now schedule delivery via `setTimeout(fn, 0)` — the same
real-task mechanism `window.postMessage` already used correctly
(`dom.rs:8178-8199`), feeding the `_lumen_timers`/`_lumen_tick_timers` queue
Rust drains once per real event-loop tick. This makes `MessagePort` delivery
spec-conformant (HTML §9.2.3: same task source as `setTimeout`) and, as a
structural side effect, makes a self-rescheduling `MessageChannel` consumer
yield back to Rust between every reschedule regardless of how many other
producers are also live — breaking the class of hang described above, not
just the one instance found.

5 existing unit tests (`dom::tests::v8_idle_message_clipboard::message_port_*`)
asserted post-delivery state via a second `rt.eval()` alone, relying on the
now-removed "delivery already happened via the automatic end-of-eval
microtask checkpoint" behavior; updated to call `_lumen_tick_timers()` first
(same pattern already used by every other timer-based test in this file).
`cargo test -p lumen-js --features v8-backend --lib dom::` — 1197/1197 green.

Verified against the exact BUG-702 minimal reproduction
(`react.js` + reduced `platform.js`, offline, no network): no longer hangs.
**Does not by itself close BUG-702** — the full, unreduced tbank.ru bundles
still hang with this fix applied; round-2 bisection (same technique, this
fixed binary) narrowed the *remaining* trigger from 501 platform.js modules
down to ~215 before time constraints stopped further narrowing, meaning at
least one more, structurally similar defect exists elsewhere in the 493 KB
bundle. See BUG-702 for the full state and next steps.

Найден и исправлен P3, при диагностике BUG-702, 2026-08-09
