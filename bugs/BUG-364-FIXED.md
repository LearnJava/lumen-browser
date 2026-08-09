# BUG-364 — `new Worker("script.js")` с внешним URL молча подставляет комментарий вместо скрипта: ни загрузки по сети, ни события `error`

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/worker.rs` — конструктор `Worker`; тот же путь у `SharedWorker`, `crates/js/src/shared_worker.rs`)
**Найден:** P2, WPT-VENDOR-eventsource (2026-07-28), `run_report.py --all --root eventsource --recursive`
**Исправлен:** P3, 2026-08-09

## Симптом (было)

Страница делает `new Worker('eventsource-onmesage.js')` — обычная запись, скрипт
воркера лежит рядом с документом. Объект воркера успешно конструируется, у него
есть `postMessage`/`terminate`/`onmessage`, но скрипт никогда не запрашивался по
сети, событие `error` не возникало, и воркер молча ничего не присылал — вызывающий
код не мог отличить «воркер молчит» от «воркера нет».

## Причина (была)

`worker.rs`'s `Worker()`/`shared_worker.rs`'s `_resolveScript()` recognized only
`data:`/`blob:lumen/` script URLs; any other scheme fell into an `else` branch
that substituted a JS comment for the script body instead of fetching it —a
documented, deliberate gap, but the resulting *silence* (no exception, no
`error` event, no console line) was the defect: a Worker built from an ordinary
external `.js` file — the dominant real-world form — looked identical to one
that was simply idle.

## Фикс

Both `Worker`/`SharedWorker` now fetch external scripts synchronously over the
network via the existing `JsFetchProvider` bridge (`lumen_core::ext`, already
used by `fetch()`/XHR — no new dependency), instead of never touching the
network at all:

- **Rust side** (`worker.rs`): new `pub(crate) fn fetch_worker_script(provider,
  url) -> Option<String>` issues a GET through the injected
  `Option<Arc<dyn JsFetchProvider>>` and returns `Some(body)` only for a 2xx
  status, `None` for any network error or non-2xx. Registered as the native
  `_lumen_worker_fetch_script(url)` (→ `String` or `undefined`) in
  `install_worker_bindings_v8`, which now takes the fetch provider as a new
  parameter. `shared_worker.rs` registers `_lumen_sw_fetch_script` the same way
  and calls the same Rust helper (`crate::worker::fetch_worker_script`) instead
  of duplicating the fetch logic. `v8_runtime.rs::install_dom` clones its
  `fetch_provider` *before* the big `self.run(move |inner| { … })` closure
  takes ownership of the original (that closure already consumes
  `fetch_provider` by value for the plain `fetch()`/XHR bridge), so both
  installers — called after the closure returns — get their own clone.
- **JS side** (`WORKER_SHIM`/`SHARED_WORKER_SHIM`): the external-URL branch
  resolves the URL against the document base (`_url_resolve(u,
  _lumen_document_base_url())` — the same primitives `fetch()`/`<a>` already
  use) and calls the new native. A successful fetch runs the real script as
  before; a failed one — per HTML LS §10.2.6.1 "run a worker" — never spawns
  the worker thread (`Worker._id`/`SharedWorker` stay unconnected) and instead
  queues (`setTimeout(fn, 0)`) a single `error` event with `message`/`filename`
  set to the failed URL. `postMessage`/`terminate` on a never-started `Worker`
  are now no-ops instead of passing a `null` id to the native bridge.
- **New parent-side `onerror` support**: `Worker.prototype` gained an
  `onerror` accessor and `addEventListener('error', …)`/`removeEventListener`
  support (previously `Worker` had none at all — only `onmessage`);
  `SharedWorker`'s existing but previously-dead `this.onerror = null` field is
  now actually invoked. This closes the accessor-level gap that
  [BUG-591](bugs/BUG-591-OPEN.md)'s Worker/SharedWorker reconfirmation flagged
  — but only for the script-fetch-failure path added here.
  **BUG-591 stays OPEN**: an uncaught exception thrown *inside* a worker that
  did start (top-level or from a later `postMessage` handler) still only
  reaches `run_worker_thread_v8`'s `eprintln!` (`worker.rs`, the `rt.eval`
  error arms) and never fires `error` on the parent `Worker` — a different,
  unfixed code path from this bug's script-fetch-failure case.

## Known limitation (not fixed here)

The fetch is **synchronous** (blocks the calling JS thread for the duration of
the GET), unlike the spec's async classic-worker-fetch algorithm — matches the
existing `fetch()`/XHR sync bridge's behavior, not a new tradeoff introduced by
this fix, but worth flagging: a `new Worker(slow-url)` now stalls the page for
as long as the request takes, where before it returned instantly (having done
nothing). `importScripts()` inside a worker thread still only resolves
`data:`/`blob:lumen/` (unchanged, uses a different resolver —
`resolve_import_url` — not the `JsFetchProvider` bridge); only the *initial*
worker script gained network fetch, not scripts it imports.

## Верификация

4 new tests in `crates/js/src/dom.rs::tests::v8_webworker` (full `install_dom`
+ a `FixedFetch` mock `JsFetchProvider`), covering both success and
fetch-failure for both `Worker` and `SharedWorker`:
`worker_external_url_fetches_and_runs_script`,
`worker_external_url_fetch_failure_fires_onerror`,
`shared_worker_external_url_connects_and_echoes`,
`shared_worker_external_url_fetch_failure_fires_onerror`. `cargo test -p
lumen-js --features v8-backend` — 2548/2548 lib tests + 68/68 integration
tests green. `cargo clippy -p lumen-js --features v8-backend --all-targets -- -D
warnings` clean.

## Расхождение с документацией (было)

`CAPABILITIES.md:141` claimed «✅ Web Workers … SharedWorker …» without the
`data:`/`blob:` caveat — updated in the same commit to reflect the fetch (with
its sync-blocking + no-`importScripts`-fetch caveats above).
