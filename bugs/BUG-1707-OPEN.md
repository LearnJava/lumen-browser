# BUG-1707 — IndexedDB: нет `getAllRecords()`, `getAll()`/`getAllKeys()` не принимают словарь опций

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-IDB-SCOUT)
**Область:** js (`crates/js/src/shim/idb_shim.js` — `IDBObjectStore`/`IDBIndex`)

## Симптом

`store.getAll({query, count, direction})` бросает `DataError: invalid key or range` (словарь трактуется как ключ). `store.getAllRecords()` и `index.getAllRecords()` не определены.

## Как найдено

WPT-IDB-SCOUT, кластер E: 9 id, 172 сабтеста (`getAll-options`, `getAllKeys-options`, `getAllRecords*`). Потолок +0.85 п.п. категории.

## Что делать

Принимать `IDBGetAllOptions` (`query`, `count`, `direction`: `next`/`prev`/…) в `getAll`/`getAllKeys` у стора и индекса; добавить `getAllRecords(options)` → массив `IDBRecord` (`key`, `primaryKey`, `value`). Спецификация — Indexed Database API 3.0.

## Как проверить

`IndexedDB/getAll-options.tentative.any.html`, `getAllRecords.tentative.any.html` проходят.
