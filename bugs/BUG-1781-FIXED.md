# BUG-1781 — `fetch(blob:…)` внутри Worker: `unsupported scheme: blob`

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (`crates/js/src/worker_net.rs`, `worker_shims.rs`, `worker.rs`; сеть не тронута)

## Источник: сайты

b2b-center-ru (вес 2, `text_len` 1 при 8808 у Chrome), uchi-ru (вес 2, базовый прогон; не в срезе из 30) — **2 сайта, вес кластера 4**. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

## Симптом

stderr: `[worker] fetch: blob:https://b2b-center.ru/<uuid>: network error: unsupported scheme: blob`. Воркер, которому страница передала `blob:`-адрес, получает отказ сети — приложение остаётся с пустым телом (b2b-center: `ready` 4,7 с, текст 1).

## Диагноз

Воркер живёт в собственном V8-изоляте и не видит JS-стор `_object_url_store` страницы, где лежат blob-записи (`web_api_shim_mid_c2.js`; в `worker_exposed_shim` файл MID_C2 не входит). Его `fetch`/`XHR` (`worker_net_shim.js` → натив `_lumen_worker_net_fetch` → `worker_net.rs::net_fetch`) уходили прямо в `JsFetchProvider`, где `crates/network` отвергает схему (`unsupported scheme: blob`). Главный поток то же самое решает JS-веткой до сети (`web_api_shim_mid_b3.js`, BUG-1126) — поэтому там работало.

## Исправление

Один путь для обеих форм (и `fetch()`, и `XMLHttpRequest` воркера идут через один натив):

1. **Регистрация** (`worker_shims.rs`, WORKER_SHIM): обёртка `URL.createObjectURL` раньше клала в Rust-стор `WorkerBlobStore` только текст `text/*`-блобов (для `importScripts`); теперь каждый блоб пишется байтами и MIME-типом нативом `_lumen_register_worker_blob_bytes(url, mime, bytes)`.
2. **Хранилище** (`worker.rs`): значение `WorkerBlobStore` сменило тип со `String` на `WorkerBlobEntry { bytes, mime }` — ближе к File API Blob URL Store; `importScripts` (`resolve_import_url`) декодирует те же байты как текст, поведение не менялось.
3. **Ответ на `blob:`** (`worker_net.rs::net_fetch`): ветка Fetch §4.2 scheme fetch до обращения к сети — только GET; запись по URL без фрагмента; ответ 200/OK с `Content-Length`/`Content-Type` из записи (те же заголовки, что у страницы в `_lumen_response_from_blob`); отозванный/неизвестный URL и не-GET — сетевая ошибка (`TypeError: Failed to fetch`). Сеть не вызывается вовсе.

## Тесты

- `worker_net_tests::fetch_serves_blob_urls_from_the_shared_store` — найденный URL (в т.ч. с фрагментом) отдаёт байты и MIME, неизвестный URL и POST дают `TypeError`, сеть не видит ни одного запроса.
- `worker::tests_v8::v8_create_object_url_wrapper_registers_blob_bytes_into_the_store` — пинит контракт обёртки `createObjectURL` → натив (единственный писатель стора; его `try/catch` иначе проглотил бы расхождение).
- Существующие тесты `importScripts('blob:…')` и e2e-спавн воркеров остались зелёными (155 в срезе `worker_net`/`worker::tests`/`shared_worker`).

## Известные пределы

- `SharedWorker`: стор у его scope всегда пустой (как и `importScripts('blob:…')` там же — страницa не прокидывает в него blob-записи); `fetch(blob:)` в shared-воркере отвечает сетевой ошибкой.
- Service worker (`sw_worker.rs`) держит собственный `fetch` для `respondWith` — не затронут.
- Отзыв `URL.revokeObjectURL` после спавна воркера не синхронизируется в стор (запись остаётся); Chrome в этом случае тоже отдаёт уже снятую копию не для всех сценариев — расхождение теоретическое, до запроса из воркера b2b-center/uchi.ru отзыв не доходит.

## Как проверить

b2b-center.ru и uchi.ru в живом окне не пишут `unsupported scheme: blob`; unit-тесты выше.
