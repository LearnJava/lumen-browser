# BUG-1753 — IndexedDB: БД, созданная открытием с прерванным первым upgrade, остаётся в `indexedDB.databases()`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-IDB-RUNNER)
**Область:** js (`crates/js/src/shim/idb_shim.js` — ветка `if (txn._aborted)` обработки открытия, `IDBFactory.databases`)

## Симптом

`IndexedDB/abort-in-initial-upgradeneeded.any.html` — harness ERROR, `Unhandled rejection: assert_equals: expected 0 but got 1`:

```js
const rq = indexedDB.open('fresh-db', 2);          // БД ещё нет
rq.onupgradeneeded = e => e.target.transaction.abort();
rq.onerror = () => indexedDB.databases().then(dbs => console.log(dbs.length)); // 1, ожидается 0
```

Раньше (разведка WPT-IDB-SCOUT, кластер N) тест давал 1…231 — там к собственной БД добавлялись БД параллельных тестов того же origin. После WPT-IDB-RUNNER тест идёт в `serial lane` (`tests/wpt/serial_tests.txt`: один браузер, пустой `data/idb`), и остаётся ровно 1 — собственная БД теста. Прогоны `.tmp/wpt-idbr1`, `.tmp/wpt-idbr2` слота `p2-work` (`dev-release` слота `p2-work` от 2026-10-27), сырые события — `IndexedDB (serial).raw.jsonl`.

## Причина

Открытие новой БД заводит запись `_idb_databases[name] = { name, version: 0, stores: {} }` до upgrade. При `abort()` upgrade-транзакции ветка `if (txn._aborted)` откатывает схему (`_idb_revert_txn`) и версию — к 0, но запись не удаляет. `databases()` перечисляет `_idb_databases` целиком, включая записи с `version: 0`. По IndexedDB 3 («abort an upgrade transaction», `databases()`) БД, созданная этим открытием, после отката не существует: Chrome и Firefox её удаляют, тест ждёт пустой список.

## Что делать

1. В ветке `if (txn._aborted)` открытия: если `entry.oldVersion === 0` (БД создана этим открытием) — `delete _idb_databases[name]` и отметить удаление для зеркала SQLite (тот же путь, что у `deleteDatabase`, чтобы запись не вернулась из `data/idb` при следующем запуске).
2. Для страховки `databases()` не перечисляет записи с `version === 0` (БД в процессе первого открытия).

## Как проверить

- `run_smoke.py /IndexedDB/abort-in-initial-upgradeneeded.any.html` — harness OK, 1/1 PASS.
- Модульный тест шима: `open(new, 2)` → `abort()` в `onupgradeneeded` → `databases()` пуст, повторный `open(new, 1)` даёт `upgradeneeded` с `oldVersion 0`.
