# BUG-665 — Scheduler API shim: `TaskSignal.any()` missing entirely, `setPriority()` doesn't reorder already-queued tasks, `scheduler.yield()` ignores `signal`/`priority`, `prioritychange` event has no `target`, abort during a sync callback body is dropped

**Статус:** OPEN
**Компонент:** js (`crates/js/src/scheduler.rs` — whole file, Phase 0 Scheduler API shim)
**Найден:** P2, WPT-VENDOR-scheduler (2026-08-05), `run_report.py --all --root scheduler --recursive` real run

## Live run signal

```
tests: 31/37 harness OK; subtests: 22/64 passed
```

`postTask()`'s straight-line happy path (result/throw/delay/run-order, no priority
juggling) is solid — the first 7 files all pass 4/4-1/1. Every remaining failure traces to
one of five independent gaps in `crates/js/src/scheduler.rs`, all consequences of the file's
own documented "Phase 0: scheduling deferred via `queueMicrotask`/`setTimeout`... no real
priority queue" design (line 14-15). None of these five are new architectural surprises —
they're the concrete test-visible cost of that documented shortcut, not yet tracked as a bug
before this run.

## 1. `TaskSignal.any()` static method doesn't exist at all — largest single class (10 of 22 unique FAILs)

`scheduler.rs:35`-`86` defines the `TaskSignal` constructor and prototype but never adds a
static `any(signals, {priority}?)` (Scheduler API §4.2 — composes multiple abort/priority
signals into one dependent `TaskSignal`, used to make a task's priority/abort track several
sources at once). Every test that calls it fails identically:

```
TypeError: TaskSignal.any is not a function
```

Hits: all of `task-signal-any-abort.tentative.any.js` (3 subtests),
`task-signal-any-priority.tentative.any.js` (9 subtests),
`task-signal-any-post-task-run-order.tentative.any.js` (3 subtests) — 10 of the category's
21 unique FAIL/TIMEOUT lines.

## 2. `TaskController.setPriority()` fires the `prioritychange` event but the scheduler never reorders in-flight tasks

```js
// scheduler.rs:114-154 — postTask's Phase 0 dispatch, decided ONCE at enqueue time
if (delay > 0) { setTimeout(run, delay); }
else if (priority === 'user-blocking') { queueMicrotask(run); }
else if (priority === 'background') { setTimeout(run, 200); }
else { setTimeout(run, 0); }
```

Priority only ever picks *which primitive* (`queueMicrotask` vs. two different
`setTimeout` delays) a task is scheduled with, at the moment `postTask()` is called. There
is no shared priority queue a later `controller.setPriority()` call could re-sort — the
signal's `prioritychange` event fires (readable via `addEventListener`/`onprioritychange`),
but nothing downstream consults it. Spec (§3.3, "Scheduler task scheduling algorithm") requires
task order to reflect the *current* priority of each task's signal at the moment each queue is
drained, i.e. a `setPriority()` call must actually be able to move a task ahead of/behind
others still pending. Four dedicated tests fail on exactly this:
`task-controller-setPriority1.any.js`, `task-controller-setPriority2.any.js`,
`task-controller-setPriority-repeated.any.js` (both of its subtests),
`task-controller-setPriority-recursive.any.js` — each asserts a specific run order after one
or more `setPriority()` calls and gets the original enqueue-time order back instead.

## 3. `scheduler.yield()` takes no arguments — ignores `{signal, priority}` entirely

```js
yield: function() {
  return new Promise(function(resolve) { setTimeout(resolve, 0); });
},
```
(`scheduler.rs:158`-`160`) Spec §8.5 defines `scheduler.yield({signal, priority}?)`: with no
signal, the continuation inherits the *currently running task's* priority/signal; with an
explicit signal, the continuation must reject with the signal's abort reason once aborted,
and reorder against other `user-visible`-vs-inherited-priority work per the composite
priority. Lumen's `yield()` accepts and reads nothing — it is a bare `setTimeout(resolve, 0)`
with no way to ever reject and no priority awareness at all. This alone explains three
distinct failure shapes across `tentative/yield/*`:

- **Never rejects on abort** — `yield-abort.any.js` (all 3 subtests): `assert_unreached:
  Should have rejected: undefined Reached unreachable code` — `promise_rejects_dom(t,
  'AbortError', scheduler.yield())` can never pass because the returned promise has no abort
  wiring to reject with.
- **Wrong interleaving with postTask/timers/idle callbacks** — every ordering assertion in
  `yield-priority-posttask.any.js`, `yield-priority-timers.any.js`,
  `yield-priority-idle-callbacks.html`, `yield-inherit-across-promises.any.js` (priority-string
  and signal variants), `yield-scheduling-state-cleared.any.js` fails with a permuted-but-not-
  matching order string (e.g. expected `"ub1,ub2,y0,y1,y2,y3,uv1,uv2,bg1,bg2"`, got
  `"ub1,ub2,y0,uv1,uv2,y1,y2,y3,bg1,bg2"`) — because `yield()`'s continuation is always a flat
  `setTimeout(fn, 0)`, i.e. always `user-visible`-equivalent timing, it can never sort ahead of
  or behind other work the way a priority-aware continuation would.
- **Cross-frame propagation TIMEOUTs** — `yield-same-origin-propagation.html`,
  `yield-scripted-subframe-propagation.html`, `yield-cross-origin-propagation.html` — separate
  from this bug; these three additionally depend on `<iframe>` having its own browsing context
  ([BUG-480](BUG-480-OPEN.md), reconfirmed here, not re-analyzed).

`yield-then-detach.html`'s `TypeError: Cannot read properties of null (reading
'DOMException')` is a distinct, likely BUG-480-adjacent iframe-teardown issue, not re-analyzed
here.

## 4. `prioritychange` event object has no `target` — spec requires the firing `TaskSignal` itself

```js
TaskSignal.prototype._setPriority = function(newPriority) {
  var prev = this._priority;
  this._priority = newPriority;
  var evt = { type: 'prioritychange', previousPriority: prev };   // <- no `target`
  ...
```
(`scheduler.rs:64`-`73`) `task-signal-onprioritychange.any.js` asserts
`event.target.priority === 'background'` after `setPriority('background')` — fails with
`Cannot read properties of undefined (reading 'priority')` because `evt.target` is `undefined`.
A plain object literal is used instead of a real `Event`, so it never gets the `target`
`EventTarget` normally stamps on dispatch.

## 5. Abort raised synchronously *inside* the running callback body is silently dropped

```js
var run = function() {
  if (signal && abortHandler) signal.removeEventListener('abort', abortHandler);  // <- unregistered BEFORE callback runs
  if (signal && signal.aborted) { reject(signal.reason); return; }
  try { resolve(callback()); } catch(e) { reject(e); }
};
```
(`scheduler.rs:135`-`139`) `post-task-with-abort-signal-in-handler.any.js`'s first subtest
posts a task whose callback body itself calls `controller.abort()`:
```js
scheduler.postTask(() => { controller.abort(); }, { signal })
```
expects `AbortError` rejection. Instead: the abort listener is removed *before* `callback()`
runs, so when the callback synchronously calls `controller.abort()`, `_abort()` sets
`signal.aborted = true` and fires the (now-listenerless) `'abort'` event to nobody, then
`run()` falls through to `resolve(callback())` — resolving with `undefined` instead of
rejecting. Spec (§3.2, task-processing step) requires checking `signal.aborted` again *after*
the callback returns, not only in the narrow pre-`removeEventListener` window. The companion
async-callback subtest (`await` a tick, then abort) is not in the FAIL list — likely masked by
the same iframe/timing gap elsewhere in the run, not verified further this session.

## Что НЕ является причиной этого бага

- `yield-inherit-across-promises.any.js`'s `TypeError: fetch: network error for
  /common/blank.html` (4 subtests) — unrelated: `/common/*` is a shared cross-category WPT
  resource that isn't vendored per the established convention (only the current category's own
  directory is vendored), not an engine defect.
- The three cross-frame `yield-*-propagation.html` TIMEOUTs and `yield-then-detach.html` —
  [BUG-480](BUG-480-OPEN.md) territory (`<iframe>` has no separate browsing context),
  reconfirmed but not the subject of this bug.
- `post-task-then-detach.html` / `post-task-with-signal-from-detached-iframe.html` TIMEOUTs —
  same BUG-480 iframe gap.

## Предлагаемый фикс

Findings 1 and 4 are small, self-contained additions (a static `TaskSignal.any()` factory
building a dependent signal that mirrors the strongest-priority/first-aborted source; adding
`target: this` to the `prioritychange`/`abort` event objects). Finding 5 is a one-line reorder
(check `signal.aborted` — or re-add the listener — after `callback()` returns, not only
before it runs). Findings 2 and 3 are the real architectural gap the file's own header comment
already flags: Phase 0's "one `setTimeout`/`queueMicrotask` call per task, decided once at
enqueue time" has no shared ordering structure for `setPriority()` or `yield()` to hook into —
fixing them properly needs an actual per-priority task queue that `postTask`, `yield`, and
`setPriority` all share (drain `user-blocking` before `user-visible` before `background` at
each checkpoint, re-sorting on `prioritychange`), not a patch to the current three-branch
`if`/`else`.
