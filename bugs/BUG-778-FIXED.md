# BUG-778 — dedicated/shared-воркеры без `self.close()`, `fetch()`, `XMLHttpRequest`; `importScripts()` у shared-воркера безусловно бросает

**Статус:** FIXED 2026-08-23
**Компонент:** js (`crates/js/src/worker.rs:264-390` — `worker_global_shim`; `crates/js/src/shared_worker.rs:99-212` — `SHARED_WORKER_GLOBAL_SHIM`, `importScripts` на строке 188-190)
**Найден:** P2, WPT-VENDOR-workers, 2026-08-18 — `run_report.py --all --root workers --recursive`

## Исправление (2026-08-23, P1)

Все четыре гэпа закрыты в обоих видах воркера (dedicated `worker.rs`,
shared `shared_worker.rs`):

- **`self.close()`** — новый нативный биндинг `_lumen_worker_self_close`
  выставляет разделяемый `WorkerCloseFlag` (`Arc<AtomicBool>`); цикл
  сообщений в `run_worker_thread_v8`/`run_shared_worker_thread_v8`
  проверяет флаг перед каждым `rx.recv()` и останавливается без обработки
  дальнейших задач (HTML LS §10.2.3/§10.2.4 «close a worker»).
- **`fetch()`/`XMLHttpRequest`** — новый нативный биндинг
  `_lumen_worker_net_fetch(url, method, headers, body_b64)` (общая функция
  `worker_net_fetch_json` в `worker.rs`, переиспользуемая обоими видами
  воркера) синхронно ходит в сеть через тот же `JsFetchProvider`, что уже
  использовал фетч классического скрипта воркера. Новый общий JS-шим
  `WORKER_NET_SHIM` (`worker.rs`) даёт минимальные `Headers`/`Response`/
  `fetch`/`XMLHttpRequest` обоим видам воркера — с собственными base64/UTF-8
  кодеками, не завязанными на `atob`/`btoa` (у shared-воркера их не было
  вовсе).
- **`importScripts()` у SharedWorker** — безусловный `throw` заменён на ту
  же логику, что и у dedicated-воркера: `data:`/`blob:lumen/` разрешаются
  локально, остальное уходит в сеть тем же `_lumen_worker_net_fetch`-мостом.
  `blob:lumen/` для SharedWorker по-прежнему не работает — у него нет
  зеркалирования блобов со страницы, которое есть у dedicated-воркера
  (`WorkerBlobStore`); это не регрессия — оно и раньше не работало,
  безусловный `throw` ловил всё.
- **Расширение WPT-RUN-6 (относительный/path-absolute `importScripts`)** —
  `importScripts()` у ОБОИХ видов воркера теперь разрешает
  относительный/path-absolute URL (`importScripts("/resources/testharness.js")`,
  ровно то, что генерирует обёртка wptrunner для `.worker.html`/
  `.any.worker.html`/`.any.sharedworker.html`) против собственного URL
  воркера — новый глобал `_lumen_worker_base_url`, выставляемый Rust-стороной
  перед вычислением шима. Для dedicated-воркера это резолвленный URL,
  который уже вычисляла его собственная страничная обёртка `Worker()`
  (`_url_resolve(u, _lumen_document_base_url())`) — просто раньше никуда не
  передавался дальше самого фетча скрипта; теперь передаётся вторым
  аргументом в `_lumen_create_worker(script, base_url)`. Для SharedWorker —
  аналогично, третий аргумент `_lumen_sw_connect(key, script, base_url)`.
  Для blob:/data: воркеров `base_url` — пустая строка (нет осмысленного origin).

**Не покрыто:** `importScripts('blob:lumen/…')` внутри `SharedWorker` (см.
выше — отдельная, самостоятельная задача блоб-зеркалирования, не в скоупе
этого бага); `fetch()`/XHR внутри самого service worker'а (у него уже был
собственный, отдельный путь, `sw_worker.rs`, не тронут).

Юнит-тесты: `worker::tests_v8::v8_worker_self_close_stops_further_messages`,
`v8_worker_globals_have_fetch_and_xhr`,
`v8_worker_fetch_reaches_provider_and_decodes_body`,
`v8_worker_xhr_send_reaches_provider`,
`v8_import_scripts_path_absolute_resolves_against_base_url_and_fetches`;
`shared_worker::tests_v8::v8_shared_worker_close_stops_further_messages`,
`v8_shared_worker_global_scope_has_fetch_xhr_close`,
`v8_shared_worker_fetch_reaches_provider`, `v8_shared_worker_xhr_reaches_provider`,
`v8_shared_worker_import_scripts_path_absolute_resolves_against_base_url`.

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

## Расширение WPT-RUN-6 (2026-08-20/21): dedicated-воркерный `importScripts` — тоже блокирующий гэп, не только shared

Пункт 3 выше описывает `SharedWorkerGlobalScope.importScripts()` как
безусловно бросающий, противопоставляя его dedicated-воркеру, где вызов
"реально работает для `data:`/`blob:lumen/`" — это верно буквально, но
**не для `http(s):`**, а именно `http(s):`-абсолютный путь — это то, что
генерирует сам wptrunner для *любого* `.any.worker.html`/`.worker.html`/
`.any.sharedworker.html`: обёртка, которую сервер строит из `.any.js`,
начинается с `importScripts("/resources/testharness.js")`. Разбор
TIMEOUT-кластера по всему прогону WPT-RUN-5 (Windows-снимок, 479/479
шардов, `docs/wpt/runs/2026-08-20-windows-partial.json` + сырые отчёты
`.tmp/wpt-corpus/`) показал, что это — **крупнейший отдельный механизм
TIMEOUT во всём корпусе**:

| суффикс теста | TIMEOUT | всего | доля |
|---|---|---|---|
| `.worker.html` | 699 | 711 | 98.3% |
| `.any.worker.html` | 317 | 317 | 100.0% |
| `.any.sharedworker.html` | 194 | 196 | 99.0% |

Итого **1210 id** (≈19.5% всех 6205 TIMEOUT прогона) — фон TIMEOUT по
всему прогону 15.2%. Прямое подтверждение из логов шардов (не
предположение — дословная строка стдерр движка, `.tmp/wpt-corpus/*.log`):

```
[worker-0] v8 script error: Runtime("importScripts: cannot load script: /resources/testharness.js")
[shared-worker] v8 script error: Runtime("importScripts is not supported")
```

Оба случая — сам движок логирует ошибку немедленно (значит, это не
зависание сети и не таймаут фетча), но исключение никуда дальше стдерр не
уходит: обёртка `fetch_tests_from_worker()` в родительском фрейме ждёт
сообщение, которое никогда не придёт, и тест падает только по истечении
файлового таймаута (~10с/тест, совпадает с измеренными `duration` в
`test_end`). Это тот же паттерн "исключение внутри воркера не долетает до
родителя", что уже целиком описан в [BUG-591](BUG-591-OPEN.md) — здесь
он не отдельная причина, а то, почему гэп ниже проявляется как TIMEOUT,
а не как быстрый `ERROR`.

**Не покрыто существующим "Предлагаемый фикс" выше**: пункт про
`importScripts` у SharedWorker предполагает, что нужно просто подключить
код dedicated-воркера (`worker.rs:349-358`) — но этот код сам ограничен
`data:`/`blob:lumen/` и не решит проблему для `http(s):`. Правильный
фикс для обоих — тот же путь, что уже реализован для сервис-воркера
(`sw_worker.rs:201-216`, синхронный фетч по сети через существующий
`JsFetchProvider`-мост) — не новая машинерия, а перенос уже работающего
паттерна на `worker.rs`/`shared_worker.rs`.

`https.any.worker`/`any.serviceworker`/`https.any`/`https.window`
суффиксы (тоже с высоким TIMEOUT) сюда **не входят** — они обслуживаются
через `https://127.0.0.1:18443` и объясняются отдельным, уже открытым
[BUG-792](BUG-792-OPEN.md) (потеря тела close-delimited https-ответа), не
этим механизмом; подтверждено прямым чтением `Reload:`-строк лога —
`.worker.html`/`.any.worker.html`/`.any.sharedworker.html` во всех
просмотренных случаях идут по обычному `http://127.0.0.1:18300`.

Инструмент измерения (черновой, не закоммичен): `.tmp/timeout_audit.py` в
сессии `p2-wpt-run-6` — классифицирует TIMEOUT по суффиксу файла теста
поверх `.tmp/wpt-corpus/*.json`/`*.raw.jsonl` (часть шардов — 166 из
479 — не оставляют `.json`-сводку и разбираются из `.raw.jsonl`,
построчного лога `test_end`-событий wptrunner).

## Не расследовано в этой сессии

- Поведение `fetch()`/`XHR` внутри сервис-воркера за пределами Phase 1
  cache-only стаба — не тема этого бага, см. существующий комментарий
  `sw_worker.rs:167`.
- `Worker-terminate-forever-during-evaluation.html`/`Worker-termination-
  with-port-messages.html` (тоже в TIMEOUT в этом прогоне) не проверены на
  предмет того, чинит ли их появление `close()`, или там отдельная причина.
