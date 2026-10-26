# BUG-1705 — `indexedDB`, `IDBKeyRange`, `IDBDatabase` не определены в DedicatedWorker и SharedWorker

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-IDB-SCOUT)
**Область:** js (`crates/js/src/worker.rs` — `install_worker_scope_globals_v8`; `crates/js/src/shared_worker.rs`; шим `crates/js/src/shim/idb_shim.js`)

## Симптом

В dedicated- и shared-воркере `typeof indexedDB`, `typeof IDBKeyRange`, `typeof IDBDatabase`, `typeof IDBFactory` — `"undefined"`. В ServiceWorker шим ставится (`crates/js/src/sw_worker.rs`, около строки 779), в window — тоже.

## Как найдено

WPT-IDB-SCOUT, кластеры A и B (`docs/wpt-vendor-notes/IndexedDB.md`): 214 id `*.any.worker.html` (1 205 сабтестов) и 184 id `*.any.sharedworker.html` (1 020 сабтестов) падают сразу на первой же обращении к `indexedDB`. Это 71 п.п. из 88 потерянных в категории `IndexedDB`; потолок — +11.4 п.п. категории (worker) и +9.8 п.п. (shared), если вариант достигнет доли своего `.any.html`-близнеца.

## Что делать

Установить IndexedDB-шим в глобал воркера так же, как в `sw_worker.rs`: `indexedDB`, `IDBFactory`, `IDBKeyRange`, `IDBDatabase`, `IDBObjectStore`, `IDBIndex`, `IDBCursor*`, `IDBTransaction`, `IDBRequest`, `IDBOpenDBRequest`. Хранилище — то же, что у окна того же origin (проверить, что снимок `data/idb` не затирается двумя realm'ами одновременно).

## Как проверить

В `*.any.worker.html` `typeof indexedDB === "object"`; `run_corpus.py --prefixes IndexedDB` — доля `*.any.worker.html` и `*.any.sharedworker.html` становится сопоставимой с `*.any.html`.
