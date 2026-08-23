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
statement (see [BUG-778](bugs/BUG-778-FIXED.md) for the importScripts gap
itself). Because nothing reaches the parent, `fetch_tests_from_worker()`
just waits out its ~10s per-file timeout: 1210 of 6205 TIMEOUT ids in that
run (19.5%) are this exact mechanism, the single largest TIMEOUT cluster
measured in the whole corpus. Fixing the missing-feature side (BUG-778)
alone would likely convert most of these from TIMEOUT to a fast ERROR/FAIL
(testharness.js still wouldn't load) — this pipeline gap is what's needed on
top for the parent to *observe* that quickly instead of waiting out the
timeout, and would also matter independently for the many `html/webappapis`
files listed above.

**The window `window.onerror`/`'error'`/`onunhandledrejection` reporting core
is now wired (P1, 2026-08-22)** — [BUG-716](bugs/BUG-716-FIXED.md) closed the
promise-rejection half; this slice closes the "report the exception" (HTML LS
§8.1.3.6) half for four of its call sites:

- **Timers** (`setTimeout`/`setInterval`, `_lumen_tick_timers`) and
  **`requestAnimationFrame`** (`_lumen_run_raf_callbacks`) — both used to
  swallow a callback's exception with a bare `catch(e){}`; now call the new
  shim function `_lumen_report_exception(err)` (`crate::dom::WEB_API_SHIM`).
- **`queueMicrotask`** — its callback used to run unwrapped as a
  `Promise.prototype.then` fulfillment handler, so an uncaught throw became an
  `unhandledrejection` on the untouched wrapper promise instead of the spec's
  `'error'` event (`queue-microtask-exceptions.any.html` waits on `'error'`
  and would have hung regardless of BUG-716). Now wrapped in its own
  `try/catch`.
- **Classic `<script>` execution, both insertion paths**: `_lumen_script_execute_classic`
  (DOM-inserted scripts — `createElement('script')` + `appendChild`, and the
  parser's own insertion) now calls `_lumen_report_exception(e)` instead of
  only `_lumen_console_error`; the *initial* page-load loop
  (`crates/shell/src/main.rs`) now calls a new Rust-side counterpart,
  `V8JsRuntime::eval_and_report` (`v8_runtime.rs`), which additionally reads
  `v8::Message` (populated by V8 for both compile *and* runtime errors) for a
  structured filename/line/column and passes it straight through with the
  live exception value — no `eval`/JSON round-trip, same technique BUG-716
  used for the live `PromiseRejectionEvent.reason`.
- **`window.onerror`'s calling convention** was also wrong independently of
  whether it ever fired: `window.dispatchEvent`'s `'error'` branch called it
  with the `Event` object as its sole argument, like every other `on<type>`
  handler. Per WebIDL, `onerror`'s type is `OnErrorEventHandler`, not the
  ordinary `EventHandler` — its "internal raw handler" is called with 5
  positional arguments (`message, source, lineno, colno, error`) when the
  event genuinely is an `ErrorEvent`, and a truthy return value cancels the
  event (`event-handler-processing-algorithm-error/window-runtime-error.html`
  checks exactly this). Both are now implemented.
- 9 new tests (`dom.rs::tests::v8_core::bug591_*`, `v8_runtime.rs::tests::eval_and_report_*`).

**Exceptions from `addEventListener`/`on<type>` DOM event listeners are now
also reported (P1, 2026-08-22)** — HTML LS's "event listener invoke" step
calls "report the exception" for these too; the bare `catch(e){}` sites around
every listener-invocation loop now call `_lumen_report_exception(e)`:
- `_lumen_dispatch`, `_lumen_dispatch_bubble`, `_lumen_dispatch_rich` (native
  element/document listener paths reached from Rust-driven input and from
  `Element.prototype.dispatchEvent`/`ShadowRoot.dispatchEvent`, both of which
  delegate to `_lumen_dispatch`).
- `document.dispatchEvent`'s own listener loop (a separate implementation
  from the "document-level" branch inside `_lumen_dispatch_bubble`/`_lumen_dispatch_rich`).
- `EventTarget.prototype.dispatchEvent` (the pure-JS base class many Web API
  shims `extend`) — through a new `_lumen_et_report(e)` wrapper instead of
  calling `_lumen_report_exception` directly, because this shim is also
  spliced into `WorkerGlobalScope` (`worker_exposed_shim`), which does not
  carry the page-only `_lumen_report_exception` native; the wrapper
  `typeof`-guards the call so a worker-scope `EventTarget` still swallows the
  exception (matching pre-fix behaviour there) instead of throwing a new
  `ReferenceError`.
- `window.dispatchEvent`'s `'load'` branch and its generic `on<type>`/explicit-listener
  branch for every other event type.
- **Deliberately excluded:** `window.dispatchEvent`'s `'error'` branch itself
  stays a bare `catch(e){}` — it is what `_lumen_report_exception` calls into,
  so routing it back through the same function would let a self-rethrowing
  `window.onerror`/`'error'` listener recurse forever. Covered by a regression
  test (`bug591_window_error_listener_exception_does_not_recurse`).
- 6 new tests (`dom.rs::tests::v8_core::bug591_*` ×5,
  `worker.rs::tests_v8::v8_worker_event_target_listener_exception_does_not_reference_error`).

**The Worker parent-side mechanism is now wired too (P1, 2026-08-22)** —
`run_worker_thread_v8`'s `rt.eval(&script)` failure arm (top-level script
error) and `worker_global_shim`'s three previously-bare `catch(e){}` sites
(`_lumen_worker_dispatch_message`'s `onmessage`/`addEventListener('message')`
calls, `_lumen_flush_timers`'s callback loop) now post an error report back
through a new parallel channel (`WorkerErrorQueue`, `worker.rs`) instead of
only `eprintln!`ing to host stderr or silently swallowing. `V8JsRuntime::pump_workers`
drains it alongside the existing message queue and calls a new
`_lumen_deliver_worker_errors` (`WORKER_SHIM`), which fires an `ErrorEvent`
named `error` at the owning `Worker` object — both `worker.onerror` and
`addEventListener('error', …)` now see it, matching the accessor-level fix
[BUG-364](bugs/BUG-364-FIXED.md) already made for the script-fetch-failure
case. `filename`/`lineno`/`colno` are best-effort-parsed from `err.stack`
inside the worker (same technique the page-side `_lumen_report_exception`
uses), since V8's `Error` has no structured location API from script; the
top-level-script-eval path has no `v8::Message` available here (unlike
`eval_and_report`'s page-only path) and reports an empty filename/0/0
alongside the real message text. 2 new tests
(`dom.rs::tests::v8_webworker::worker_top_level_exception_fires_parent_onerror`,
`worker_onmessage_handler_exception_fires_parent_onerror`).
**Not addressed in this slice:** `SharedWorker`'s equivalent mechanism
(`shared_worker.rs`, a separate module with its own thread/message-loop
shape — see its own section below, wired 2026-08-23) and a worker's
`setTimeout`/`queueMicrotask` callbacks specifically (the *flush loop* now
reports, but a callback that itself schedules another timer before throwing
is untested) — narrower gaps, not named by any WPT cluster measured for this
bug so far.

**Module-script top-level runtime errors are now wired too (P1, 2026-08-23)**
— `crate::v8_esm::load_and_evaluate`/`evaluate_entry_module`/`evaluate_module_url`
now return a `ModuleFailure` (`Load` vs `Runtime`) instead of a bare `Err(())`,
distinguishing "the module body never started evaluating" (compile/link/
instantiate failure) from "the module's own top-level body threw"
(`ModuleStatus::Errored` reached after `evaluate()`). Two new `V8JsRuntime`
methods, `eval_module_and_report`/`eval_module_at_and_report`
(`v8_runtime.rs`), call the shim's `_lumen_report_exception` — reusing the
same `v8::Message` filename/lineno/colno extraction `eval_and_report` uses —
only for the `Runtime` variant; a `Load` failure stays unreported here,
matching this section's own "avoid misfiring on an ordinary 404/missing
import" rule. The module-script execution loop (`crates/shell/src/main.rs`,
the genuine top-level page-script boundary) now calls these instead of the
plain trait `eval_module`/`eval_module_at`. 3 new tests
(`v8_runtime.rs::tests::eval_module_and_report_runtime_error_fires_window_error`,
`eval_module_and_report_load_error_does_not_fire_window_error`,
`eval_module_at_and_report_runtime_error_fires_window_error`).

**`SharedWorker`'s equivalent mechanism is now wired too (P1, 2026-08-23)** —
unlike a dedicated `Worker` (exactly one client), an uncaught exception
anywhere in a `SharedWorkerGlobalScope` (top-level script body, `onconnect`,
a port's `onmessage`, a flushed timer callback) must, per HTML LS "report the
exception", fire `error` at *every* connected client's `SharedWorker` object
— not just the one whose message happened to trigger it. `shared_worker.rs`
now tracks a second map alongside the existing per-port message-outbox map,
`error_ports: HashMap<port_id, WorkerErrorQueue>`, populated/removed on the
same `Connect`/`Close` messages; a new `broadcast_shared_worker_error` helper
pushes one report into *every* entry of that map (reusing `worker.rs`'s
`WorkerErrorQueue`/`error_info_json`, now `pub(crate)`, instead of a
duplicate type). Four sites now call it: `run_shared_worker_thread_v8`'s
top-level `rt.eval(&script)` failure arm, and three previously-bare
`catch(e){}` sites in `SHARED_WORKER_GLOBAL_SHIM` (`onconnect`/its listeners,
a worker-side port's `onmessage`/its listeners, the timer-flush loop) via a
new `_lumen_sw_report_exception` helper mirroring the dedicated worker's
`_lumen_report_worker_exception`. On the client side, `V8JsRuntime::pump_shared_workers`
gained a second drain (a new `shared_worker_errors: WorkerErrorQueue` field,
parallel to `shared_worker_outbox`) that calls a new
`_lumen_deliver_shared_worker_errors`, which looks the port id up in a new
`_sharedWorkerInstances` map and fires `ErrorEvent` `'error'` there. This
also closed a pre-existing, unrelated gap while it was being touched: the
client-side `SharedWorker` JS class had **no `addEventListener`/`removeEventListener`
at all** (`AbstractWorker`/`EventTarget` requires them) — `onerror` was a
bare plain property, not an accessor, and the BUG-364 script-fetch-failure
path called it directly instead of through a shared delivery function; both
are now `SharedWorker.prototype` methods mirroring `Worker.prototype`'s
existing `_onerror`/`_errorListeners`/`_deliverError` shape. 3 new tests
(`dom.rs::tests::v8_webworker::shared_worker_onconnect_exception_fires_client_onerror`,
`shared_worker_port_onmessage_exception_broadcasts_to_all_clients` — two
independently-connected clients, only one posts the triggering message, both
observe the broadcast — `shared_worker_error_addeventlistener_also_fires`).
**Not tested here** (spec-ambiguous edge case, not a regression): the very
first client that causes a worker to spawn races its own `Connect` against
that worker's top-level script evaluation — if the script throws
synchronously before the first `connect` task runs, that first client's port
is not yet "entangled" by HTML LS's own definition, so it is unclear whether
even a spec-compliant engine would report to it; every test above instead
throws from `onconnect`/a port handler, where the connecting client is
already registered by construction.

**Not in scope of this slice — still open:**
- `<body onerror>` forwarding to the special 5-arg form on a `Document`/child
  element (`onerroreventhandler.html`'s `check1`/`check3`) — untestable here
  regardless, it needs a cross-frame `<iframe>`, which this engine does not
  support (a separate, unrelated gap).
- Cross-origin "muted errors" (HTML LS's script-error-reporting redaction for
  a script fetched without CORS from another origin) — not implemented; every
  error reports its real message/location regardless of origin.
- `_sw_make_event_target`'s service-worker-registration event target
  (`crates/js/src/dom.rs`, `dispatchEvent: function(evt) { ... }` with no
  `try/catch` at all) and `MediaQueryList.prototype.dispatchEvent`/
  `MessagePort.prototype.dispatchEvent` were left untouched — narrower,
  lower-traffic mechanisms not named by any WPT cluster in this bug's scope.

## Срез 24 WPT-RUN-6 (2026-08-22) — путь window `load` остался немым, и это уже стоит 9 id

После правок 2026-08-22 (таймеры, rAF, `queueMicrotask`, классические
`<script>`, DOM-слушатели, Worker) остался ещё один путь, и он не из
перечисленных в «Что осталось»: **исключение из обработчика `load` окна**.

Замер `tests/wpt/verify_frame_load_media_gaps.py --variant onload-throw`
(dev-release, Linux, коммит `c583a90b4`, `--seconds 5`, страница жива — 9
тиков): обработчик, добавленный через `addEventListener('load', …)`, и
обработчик из атрибута `<body onload>` оба входят (`load-listener-entered`,
`body-attr-entered`) и оба бросают. Не приходит ничего — ни `'error'` на
window, ни `window.onerror`, ни строки на stderr браузера. Причина видна в
коде: `_lumen_apply_ready_state` (`crates/js/src/dom.rs:13816` — цикл по
`_load_listeners`, `:13819` — `window.onload`) вызывает каждый обработчик в
`try { … } catch (e) {}`; там же голые `catch` вокруг window-слушателей
`DOMContentLoaded` (`:13798`) и вокруг `visibilitychange`
(`_lumen_apply_visibility`, `:13835`).

Цена в остатке снимка WPT-RUN-5 — маркер `single-test-load-handler-throw`,
**9 id** (`css/css-shapes/spec-examples/shape-outside-0**.html`): страница
объявляет себя `setup({ single_test: true })`, делает все проверки в функции,
вызванной из `<body onload>`, и зовёт `done()` в конце. `shape-outside` пока
не заворачивает флоаты (`CSS-SPECS.md` #43), первый же `assert_not_equals`
бросает, `done()` не достигается, исключение проглатывается — вместо FAIL
получается NOTRUN и TIMEOUT.

## Срез 2026-08-23 (P1) — путь window `load`/`DOMContentLoaded`/`visibilitychange` подключён

Все четыре голых `catch(e) {}`, найденных срезом 24 выше, теперь зовут
`_lumen_report_exception(e)` — тот же путь, которым уже репортируются
таймеры/rAF/queueMicrotask/DOM-слушатели/воркеры: `_lumen_apply_ready_state`
(`crates/js/src/dom.rs`, цикл по `_domcontentloaded_win_listeners`, цикл по
`_load_listeners`, вызов `window.onload`) и `_lumen_apply_visibility` (цикл по
`_visibilitychange_listeners`). `<body onload>` покрывается тем же местом —
он и раньше форвардился в `window.onload` (`_LUMEN_BODY_FORWARDED_TO_WINDOW`,
`dom.rs:991`), только само присвоенное значение вызывалось без репорта.
`document.dispatchEvent` для `readystatechange`/`DOMContentLoaded` на
document (а не на window) уже был подключён раньше — репортится не этот
путь, а отдельный ручной цикл по window-слушателям, который
`_lumen_apply_ready_state` держит параллельно ради порядка «document
(bubbles), затем window» (HTML LS §8.5).

Не тронуто (внутренний, не пользовательский код): `try { afEl.focus(); }
catch(e) {}` в том же `_lumen_apply_ready_state` вокруг autofocus-фокусировки
(BUG-381) — это вызов движка, а не пользовательский обработчик.

4 новых теста (`dom.rs::tests::v8_core`):
`bug591_window_load_listener_exception_fires_window_error`,
`bug591_window_onload_exception_fires_window_error`,
`bug591_window_domcontentloaded_listener_exception_fires_window_error`,
`bug591_window_visibilitychange_listener_exception_fires_window_error`.

С этим срезом весь ранее известный список путей, кроме двух специально
исключённых из объёма (`<body onerror>` для чужого документа — нужен
`<iframe>`, которого нет; редакция «muted errors» для cross-origin), закрыт.
Остаются вне scope нижнего приоритета: `dispatchEvent` у
service-worker-registration/`MediaQueryList`/`MessagePort` (см. «Не в объёме
этого среза» выше) — узкие, не названные ни одним WPT-кластером механизмы.

## Перезамер 2026-08-23 (WPT-RUN-6, срез 27): остался `requestIdleCallback`

`tests/wpt/verify_callback_import_preload_gaps.py --variant cbx-ric` на
`main` = `34cbefd25`, то есть уже после всех починок 2026-08-22/23. Колбэк
`requestIdleCallback` исполняется (`deadline.timeRemaining` — функция), но
брошенное из него исключение не доходит никуда:

```
ric-ran deadline=function
ric-second-ran
ric-error microBoom      ← queueMicrotask: починен
ric-checked
```

`ric-error ricBoom` нет. Соседний путь — `requestAnimationFrame` — теперь
работает полностью, включая `e.error.message` (вариант `cbx-report`:
`cbx-event type=error message="rafBoom" error="rafBoom"`), то есть
`animation-frames/callback-exception.html` починен, а
`requestidlecallback/callback-exception.html` (та же страница, тот же
ассерт) — нет. Это последний известный движковый колбэк вне починки; список
остальных остатков — в разделе выше.

Побочно тем же вариантом: `'onerror' in window === false`
([BUG-874](BUG-874-OPEN.md)) — детект «есть ли обработчик» в WPT идёт именно
этой идиомой.

## Срез 2026-08-23 (P1) — `requestIdleCallback` и вся остальная шимовая развилка `dom.rs`

Перезамер среза 27 выше называл `requestIdleCallback` последним движковым
колбэком вне починки. Это было верно только для того списка путей, который
предыдущие срезы успели перечислить: `grep` по `dom.rs` на голый
`catch(e) {}` дал **33** места, где шим зовёт пользовательский колбэк и
глотает его исключение, и rIdle — лишь одно из них. Все 33 теперь зовут
`_lumen_report_exception`:

| Путь | Строки (до правки) |
|---|---|
| `requestIdleCallback` | 11930 |
| `MessagePort` (`onmessage` + слушатели) | 12011, 12014 |
| `MediaQueryList.dispatchEvent` | 10706, 10709 |
| `MutationObserver` / `IntersectionObserver` / `ResizeObserver` | 10073, 10240, 10378 |
| `PerformanceObserver` | 11778 |
| `hashchange`, `popstate`, `pageshow`/`pagehide`, window `message` | 7101, 7107, 7638, 7641, 7922, 7926, 10942, 10945 |
| `AbortSignal` `abort` | 7976, 7979 |
| `WebSocket` (`on<type>` + слушатели) | 9811, 9813 |
| фокусная развилка `_lumen_dispatch_focus_event` | 14100, 14105, 14117 |
| поздняя подписка на `load`/`DOMContentLoaded` (микрозадача) | 6287, 10832, 10840 |
| `Animation` `oncancel`/`onfinish`, `document.onfullscreenerror`, `WakeLockSentinel`, `DataTransferItem.getAsString` | 16289, 16376, 15918, 16570, 16571, 788 |

Две из этих строк — `MessagePort` — раньше значились в этом файле как «вне
объёма» вместе с `MediaQueryList`; обе закрыты. `MessagePort` — единственная,
которая идёт не напрямую, а через новый `_lumen_mc_report`: `MESSAGE_CHANNEL_SHIM`
— единственный кусок страничного шима, который отдельно вычисляет и область
сервис-воркера (`sw_worker.rs:703`), а там нет ни `_lumen_report_exception`
(страничная), ни `_lumen_et_report` (обёртка из `EVENT_TARGET_SHIM`, который
сервис-воркер тоже не грузит). Прецедент — `_lumen_et_report` (срез 2026-08-22).

Не тронуто сознательно (не пользовательские колбэки, а внутренние вызовы
движка): `JSON.parse` снапшотов, `new RegExp(pattern)` валидации формы,
`_rs_cancel_fn` и прочие потоковые пути (по спеке Streams ошибка уходит в
возвращённый промис, а не в глобальный обработчик), autofocus-фокусировка,
резолв `location`/`base`, `CLONERS` в `structuredClone`, ветка `'error'`
внутри `window.dispatchEvent` (защита от самозацикливания, покрыта
регрессионным тестом).

Живая проба: `python tests/wpt/verify_callback_import_preload_gaps.py
--variant cbx-ric --binary <абс.путь>/target/dev-release/lumen.exe` печатает
`ric-error ricBoom` (раньше этой строки не было при живых `ric-ran` и
`ric-second-ran`). 5 новых тестов в `dom.rs::tests::v8_core`:
`bug591_request_idle_callback_exception_fires_window_error`,
`bug591_message_port_handler_exception_fires_window_error`,
`bug591_media_query_list_listener_exception_fires_window_error`,
`bug591_performance_observer_callback_exception_fires_window_error`,
`bug591_mutation_observer_callback_exception_fires_window_error`.

Побочно закрыта половина [BUG-840](BUG-840-OPEN.md) (исключение колбэка
`PerformanceObserver` съедалось шимом) — оставшаяся половина того бага
(колбэк получает два аргумента вместо трёх, `resource` не доставляется) к
этому багу отношения не имеет.

**Что осталось у BUG-591 после этого среза:** отдельные шимы вне `dom.rs` —
`worker.rs`, `shared_worker.rs`, `web_audio.rs` (`oncomplete`, названный
[BUG-828](BUG-828-OPEN.md) как причина немоты `webaudio/*`), `xhr.rs`,
`video_bindings.rs`, `wake_lock.rs`, `speech.rs`, `media_stream_recording.rs`,
`notifications_bindings.rs`, `reporting_api.rs`, `screen_orientation.rs`,
`scheduler.rs`, `soft_navigation.rs`, `presentation_api.rs`,
`close_watcher.rs`, `launch_handler.rs`, `virtual_keyboard.rs`,
`media_session.rs`, `cookie_store.rs`, `broadcast_channel.rs` — ~38 мест той
же формы (граница шимов, урок BUG-780: фикс в `WEB_API_SHIM` до отдельного
`rt.eval` не доходит). Плюс прежние два, снятые по объёму: `<body onerror>`
для чужого документа (нужен `<iframe>`) и редакция «muted errors» для
cross-origin.
