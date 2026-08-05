# BUG-649: Worker global scope has no `navigator` at all — every `navigator.*` API is unreachable from inside a Worker

**Статус:** OPEN
**Компонент:** js (`crates/js/src/worker.rs:227-352` — `worker_global_shim`; `crates/js/src/worker.rs:719-752` — `install_worker_globals_v8`)
**Найден:** P2, WPT-VENDOR-permissions, 2026-08-05

## Симптом

`permissions` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root permissions --recursive`, ~1 мин 39 с, 14 id):
**5/14 harness OK, 40/40 сабтестов**.

`crashtests/permissions-query-worker.window.html` формально прошёл (1/1,
Unexpected 0 — crashtest проверяет только «не упало», не поведение), но лог
показывает живой дефект внутри воркера:

```
[worker-0] v8 script error: Runtime("navigator is not defined")
```

Тест-скрипт (`permissions-query-worker.window.js`):

```js
const worker = new Worker(URL.createObjectURL(new Blob([`
  postMessage("load");
  while (true) {
    navigator.permissions.query({ name: "geolocation" });
  }
`])));
```

`postMessage("load")` успевает выполниться первым, поэтому основной поток
получает сигнал и терминирует воркер раньше, чем инфинит-луп успевает
хоть раз пройти цикл — `navigator.permissions.query(...)` бросает
`ReferenceError: navigator is not defined` на первой же итерации, скрипт
воркера обрывается, но `Worker`-объект и канал `postMessage` продолжают
жить как ни в чём не бывало. Тест зелёный **случайно** — он проверяет
устойчивость к бесконечному циклу с `permissions.query`, а получает
устойчивость к мгновенно выброшенному `ReferenceError`, что не то же
самое поведение и не то, что спека собирается тестировать.

Прямое доказательство первопричины — `install_worker_globals_v8`
(`worker.rs:719-752`) регистрирует только `_lumen_worker_post_reply`,
`_lumen_worker_console_log`, `_lumen_import_scripts_resolve`,
`atob`/`btoa`, и затем один раз вызывает `worker_global_shim` (не
`install_dom`/полный список нативов главной страницы). `worker_global_shim`
(`worker.rs:227-352`) определяет `self`, `name`, `postMessage`,
`onmessage`/`addEventListener('message')`, `console`, `importScripts`,
`setTimeout`/`setInterval`/`queueMicrotask` — но нигде не определяет
`navigator`. Любой `navigator.*` вызов внутри Worker/DedicatedWorkerScope
бросает `ReferenceError` до того, как достигает своего собственного
дефекта (валидация аргументов, разрешения и т.д.) — это не специфично для
Permissions API, это блокирует **весь** `navigator`-поверхностный API
изнутри воркера (userAgent/language/hardwareConcurrency/onLine/storage/
locks/serviceWorker/permissions/…).

## Причина

Спека (HTML LS §10.1.5.1, `WorkerGlobalScope` подмешивает `NavigatorID`/
`WorkerNavigator` через `navigator` атрибут) требует `navigator` на
каждом `WorkerGlobalScope`. Реализация Lumen ставит воркеру совершенно
отдельный, гораздо более узкий набор глобалов (`worker_global_shim`),
собранный вручную под конкретные уже реализованные фичи
(`postMessage`/`importScripts`/таймеры) — `navigator` в этот список
просто не попал ни в каком виде (ни `WorkerNavigator`-объект, ни хотя бы
заглушка).

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root permissions --recursive
```
или живой пробой (`--mcp-live-port`, страница-хост с `<script>`):
```js
var w = new Worker(URL.createObjectURL(new Blob(
  ['postMessage(typeof navigator)'])));
w.onmessage = e => console.log('typeof navigator in worker:', e.data);
// spec: "object". Lumen: postMessage never runs — throws ReferenceError first.
```

## Реконфирмации / связанные категории (не заведены отдельно)

Остальные 9 из 14 id категории `permissions` не дошли до собственной
логики: 5 TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer`
(`event-model.https.html`, `permissions-cg.https.html`,
`permissions-garbage-collect.https.html`,
`permissions-query-permissions-policy-attribute.https.sub.html`,
`worker.https.html`), 3 ERROR на уже задокументированном
эффекте BUG-380 (браузинг-контекст переиспользуется, следующий тест
после TIMEOUT-соседа получает результаты предыдущего — `edge-cases.https.html`,
`non-fully-active.https.html`, `revocation.https.html`), 1 TIMEOUT на
невендоренных общих ресурсах (`idlharness.any.html` — 404 на
`/resources/idlharness.js` и `/resources/WebIDLParser.js`, не относится
к самой категории).
