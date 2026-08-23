# WPT vendor notes — `workers`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-workers`, `docs/wpt-status.md`), scope ⬜ (confirmed — both
dedicated `Worker` and `SharedWorker` are real, real-thread implementations,
`crates/js/src/worker.rs`/`shared_worker.rs`, not stubs).

Same pinned upstream commit `35be3b44`, `git sparse-checkout add workers`
at that commit, `LICENSE-WPT.md` copied from the sibling `window-management`
category — 606 files (605 upstream + license), 287 glob ids. Cheap by every
predictor: 0 `name="variant"` hits, 14 `.https.` files (2.3%), 8
`testdriver.js` hits (1.3%), self-contained (out-of-category deps are just
`/common/utils.js` ×4, `/common/dispatcher/dispatcher.js` ×4). Confirmed
cheap in practice: **~29 minutes wall-clock, single process, 287 ids**.

### Run result

`run_report.py --all --root workers --recursive`: **94/249 harness OK,
153/877 subtests passed**. Despite the cheap predictors, 144 of the 155
non-OK harness results are TIMEOUT (11 ERROR), because a bare-bones
`WorkerGlobalScope` implementation means most worker scripts throw a
synchronous `ReferenceError` before ever calling `postMessage` — the parent
never sees an error (no `onerror` path for a script that dies mid-load),
so the harness only sees the external timeout. Runtime error tally from the
log (`Runtime("...")`): 21 × `location is not defined`, 28 × `Cannot read
properties of undefined (reading 'href')` (same root, chained access), 7 ×
`navigator is not defined`, 7 × `Cannot use import statement outside a
module`, 5 × `XMLHttpRequest`/`fetch is not defined` combined, 4+1 ×
`close`-related, 2 × `importScripts is not supported`.

Three distinct, filed root causes, each isolated by reading the actual
worker-side support script (not guessed from the log alone):

- **[BUG-776](../../bugs/BUG-776-OPEN.md)** — `self.location`
  (`WorkerLocation`) and `self.navigator` (`WorkerNavigator`) are missing
  entirely from both `worker_global_shim` (`worker.rs:264`, dedicated) and
  `SHARED_WORKER_GLOBAL_SHIM` (`shared_worker.rs:99`, shared); `navigator`
  is also missing from the service-worker scope (`sw_worker.rs`), though
  that scope does have a real `WorkerLocation` (`sw_worker.rs:456`). Direct
  cause of 31 files: all `WorkerLocation_*.htm`/`WorkerNavigator_*.htm`,
  `interfaces/WorkerGlobalScope/location/*`,
  `interfaces/WorkerUtils/navigator/*`.
- **[BUG-777](../../bugs/BUG-777-OPEN.md)** — `Worker`/`SharedWorker`
  constructors never read their `options` argument at all: `function
  Worker(url)` (`worker.rs:483`) takes one parameter; `function
  SharedWorker(url, name)` (`shared_worker.rs:302`) always coerces the
  second argument to a string via `String(name)`. `{type: 'module'}` is
  therefore silently dropped in both — module workers don't exist, every
  worker script is always evaluated as a classic script, so any `import`
  statement throws immediately. Direct cause of the entire `workers/
  modules/` directory (24 files) plus
  `SharedWorker-extendedLifetime-named-module.html`.
- **[BUG-778](../../bugs/BUG-778-FIXED.md)** — three further
  `WorkerGlobalScope`/`WindowOrWorkerGlobalScope` members are missing from
  dedicated+shared workers: `close()` (`WorkerGlobalScope-close.html` +
  1 more), `fetch()`/`XMLHttpRequest` (all 6 files of `semantics/xhr/*`
  + `examples/fetch_tests_from_worker.html`), and — a Lumen-internal
  inconsistency rather than a spec gap — `SharedWorkerGlobalScope.
  importScripts()` (`shared_worker.rs:188-190`) unconditionally throws
  `Error('importScripts is not supported')`, while the dedicated-worker
  version of the same call (`worker.rs:349-358`) actually works for
  `data:`/`blob:lumen/` URLs.

**Not re-filed — already on record:** the `onerror`/`Worker_ErrorEvent_*`
cluster (7 files under `interfaces/WorkerGlobalScope/onerror/` plus 6
`Worker_ErrorEvent_*.htm`) reconfirms the already-open
[BUG-591](../../bugs/BUG-591-FIXED.md) (an exception thrown *inside* an
already-started worker never reaches the parent's `error` handler) — read
each failing test's onerror-handling code before attributing it here,
per the project's "don't describe a failure mode from intuition" rule.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ подтверждён перед вендорингом (`Worker`/`SharedWorker` — реальные
потоки, `crates/js/src/worker.rs`/`shared_worker.rs`, не заглушки).
Вендорена целиком 2026-08-18 (коммит `35be3b44`, `tests/wpt/workers/`,
606 файлов, 287 id) — дёшево по всем предикторам (0 variant, 14 `.https.`,
8 testdriver), подтверждено на практике: ~29 мин, один процесс.

`run_report.py --all --root workers --recursive` — **94/249 harness OK,
153/877 сабтестов**. Заголовочная дешевизна прогона обманчива: 144 из 155
неуспехов — TIMEOUT, потому что урезанный `WorkerGlobalScope` роняет
воркерный скрипт `ReferenceError`-ом ещё до первого `postMessage`, а
исключение никуда не долетает — харнесс видит только внешний таймаут, а не
ошибку. Три подтверждённых, заведённых корня: [BUG-776](../bugs/BUG-776-OPEN.md)
(нет `self.location`/`self.navigator` в dedicated/shared-воркерах, 31 файл
напрямую), [BUG-777](../bugs/BUG-777-OPEN.md) (конструкторы `Worker`/
`SharedWorker` не читают `options` — модульных воркеров не существует, весь
каталог `modules/`, 24 файла) и [BUG-778](../bugs/BUG-778-FIXED.md) (нет
`close()`/`fetch()`/`XMLHttpRequest`, плюс `importScripts()` у SharedWorker
безусловно бросает, тогда как у dedicated-воркера работает). Кластер
`onerror/*`/`Worker_ErrorEvent_*` — уже задокументированный
[BUG-591](../bugs/BUG-591-FIXED.md), новым не заведён.
