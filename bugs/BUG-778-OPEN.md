# BUG-778 — dedicated/shared-воркеры без `self.close()`, `fetch()`, `XMLHttpRequest`; `importScripts()` у shared-воркера безусловно бросает

**Статус:** OPEN
**Компонент:** js (`crates/js/src/worker.rs:264-390` — `worker_global_shim`; `crates/js/src/shared_worker.rs:99-212` — `SHARED_WORKER_GLOBAL_SHIM`, `importScripts` на строке 188-190)
**Найден:** P2, WPT-VENDOR-workers, 2026-08-18 — `run_report.py --all --root workers --recursive`

## Симптом

Три независимых отсутствующих члена `WorkerGlobalScope`/
`WindowOrWorkerGlobalScope`, каждый — свой кластер TIMEOUT в прогоне
категории (94/249 harness OK):

1. **`self.close()` отсутствует.** `WorkerGlobalScope-close.html`,
   `interfaces/WorkerGlobalScope/close/sending-messages.html` — TIMEOUT.
   Лог: 4 × `Runtime("close is not defined")`, 1 ×
   `Runtime("self.close is not a function")`.
2. **`fetch()`/`XMLHttpRequest` отсутствуют.** Весь каталог
   `semantics/xhr/{001..006}.html` (6 файлов) + `examples/
   fetch_tests_from_worker.html` — TIMEOUT. Лог: 2 ×
   `Runtime("XMLHttpRequest is not defined")`, 1 ×
   `Runtime("fetch is not defined")`.
3. **`SharedWorkerGlobalScope.importScripts()` жёстко бросает всегда**,
   а не только для неподдерживаемых схем — `shared_worker.rs:188-190`:
   ```js
   globalThis.importScripts = function() {
     throw new Error('importScripts is not supported');
   };
   ```
   тогда как у dedicated-воркера тот же вызов реально работает для
   `data:`/`blob:lumen/` (`worker.rs:349-358`). Лог: 2 ×
   `Runtime("importScripts is not supported")`; затрагивает
   `baseurl/alpha/importScripts-in-sharedworker.html` и любой другой
   тест, ожидающий, что `importScripts` у SharedWorker хотя бы частично
   работает как у Worker.

## Причина

`worker_global_shim` (dedicated, `worker.rs:264`) и
`SHARED_WORKER_GLOBAL_SHIM` (shared, `shared_worker.rs:99`) перечисляют
ровно один и тот же набор членов — `self`, `postMessage`,
`addEventListener`/`removeEventListener`, `console`, `importScripts`
(с разным поведением, см. п.3), таймеры, `queueMicrotask`. Ни в одном из
двух нет `close`, `fetch`, `XMLHttpRequest`. `install_worker_scope_globals_v8`
(`worker.rs:237`, общая часть для всех трёх видов воркеров через
`crate::dom::worker_exposed_shim()`) добавляет только `EventTarget` и
`performance` — тоже без этих трёх.

Сервис-воркер (`sw_worker.rs`) — не тот же случай: `self.fetch` там есть
(`sw_worker.rs:168`, Phase 1 cache-only стаб) правильно, ровно как в спеке
(`fetch` — часть `ServiceWorkerGlobalScope`, `close`/`XMLHttpRequest` в
`ServiceWorkerGlobalScope` действительно не входят, HTML LS §14.5) — этот
баг не про сервис-воркеры, только про dedicated/shared.

## Почему это не только тестовый шум

`close()` — единственный штатный способ для воркера самому завершить
работу изнутри (HTML LS §10.2.3); без него код, ожидающий, что
`self.close()` — функция, падает `TypeError`. `fetch`/`XMLHttpRequest`
внутри воркера — стандартный путь сетевых запросов из воркерного потока
(в частности единственный способ грузить данные без блокировки главного
потока) — их отсутствие означает, что воркеры Lumen не могут делать сетевые
запросы вообще, только пассивно получать сообщения. Несимметричный
`importScripts` между dedicated и shared — расхождение внутри самого
Lumen, не только с спекой: одинаковый код в разных типах воркеров ведёт
себя по-разному без видимой причины.

## Предлагаемый фикс

- `close()`: `globalThis.close = function() { _lumen_worker_self_close(); }`
  — новый нативный биндинг, отправляющий воркеру сигнал остановки (тот же
  путь, что `Worker.prototype.terminate()` использует со стороны страницы,
  `_lumen_worker_terminate`, но инициированный изнутри).
- `fetch`/`XMLHttpRequest`: воркерный поток уже умеет синхронно фетчить
  (тот же `JsFetchProvider`-мост, что использует конструктор `Worker` для
  внешнего URL скрипта, `worker.rs:519-521`) — обернуть его в `fetch()`,
  возвращающий `Promise` (микротаск на воркерном таймер-луп), и в минимальный
  `XMLHttpRequest` поверх того же моста.
- `importScripts` у SharedWorker: заменить безусловный `throw` на тот же
  код, что `worker.rs:349-358` (обёртка над `_lumen_import_scripts_resolve`)
  — сегодня функция и нативный биндинг уже существуют, их просто не
  подключили ко второму шиму.

## Не расследовано в этой сессии

- Поведение `fetch()`/`XHR` внутри сервис-воркера за пределами Phase 1
  cache-only стаба — не тема этого бага, см. существующий комментарий
  `sw_worker.rs:167`.
- `Worker-terminate-forever-during-evaluation.html`/`Worker-termination-
  with-port-messages.html` (тоже в TIMEOUT в этом прогоне) не проверены на
  предмет того, чинит ли их появление `close()`, или там отдельная причина.
