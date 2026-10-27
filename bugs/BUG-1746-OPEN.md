# BUG-1746 — `self.caches` не определён в Dedicated Worker и SharedWorker

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-SW-SCOUT)
**Область:** js (`crates/js/src/worker.rs`, `crates/js/src/shared_worker.rs`, `crates/js/src/worker_shims.rs`; Cache API — `web_api_shim_mid_b.js` `var caches`, `sw_worker.rs`)

## Симптом

```js
// *.any.worker.html / *.any.sharedworker.html
typeof self.caches;     // "undefined" — в окне и в сервис-воркере есть
```

`cache_test()` из `service-workers/cache-storage/resources/test-helpers.js` в cleanup зовёт `self.caches.delete(...)`: «Cannot read properties of undefined (reading 'delete')» → harness ERROR, все сабтесты файла теряются. В `cache-storage-keys` то же на `keys`.

Тот же класс, что [BUG-1705](BUG-1705-OPEN.md) (`indexedDB` не определён в воркерах): глобалы хранилищ ставятся только в окне и в изоляте SW, `worker.rs`/`shared_worker.rs` их не получают.

## Как найдено

WPT-SW-SCOUT, кластер F (`docs/wpt-vendor-notes/service-workers.md`): 23 id (ERROR 16, OK 7) — все `cache-storage/*.https.any.worker.html` и `*.any.sharedworker.html`, плюс `cache-api-nested-worker.https.html`; потеряно 23.0 балла (6.9 п.п. категории). Варианты `idlharness.https.any.worker.html`/`.sharedworker.html` падают так же (учтены в кластере G, [BUG-1747](BUG-1747-OPEN.md)).

## Что делать

1. Установить `caches` (`CacheStorage`) в глобал Dedicated Worker и SharedWorker тем же шимом, что в окне; origin — origin воркера.
2. Натив `_lumen_cache_*` зарегистрировать в изолятах `worker.rs`/`shared_worker.rs` с доступом к общему `cache_store` (как у изолята SW).
3. Вложенный воркер (`cache-api-nested-worker`) получает тот же `caches`.

Фикс открывает только заголовок: после него сабтесты упрутся в [BUG-1745](BUG-1745-OPEN.md) (форма Cache API).

## Как проверить

`run_corpus.py --prefixes service-workers/cache-storage --processes 6`: у `*.any.worker.html` и `*.any.sharedworker.html` нет ERROR, счёт сабтестов совпадает с `*.any.html`.
