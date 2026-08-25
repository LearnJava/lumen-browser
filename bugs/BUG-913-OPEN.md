# BUG-913 — IndexedDB: `abort()` не откатывает уже применённые записи

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, живым замером в ходе [BUG-841](BUG-841-FIXED.md))
**Область:** `crates/js/src/dom.rs` — `_idb_flush_txn` (ветка `_aborted` гасит
очередь и стреляет `abort`, но не трогает данные), `IDBObjectStore.prototype._write`
и соседние мутирующие действия (пишут прямо в `store.records` без журнала отмены)
**Владелец:** P1 (`lumen-js`)

## Симптом

```js
var tx = db.transaction('s', 'readwrite');
var w = tx.objectStore('s').put('rolled', 20);
w.onsuccess = function() { tx.abort(); };   // прерываем после записи
// позже, в новой транзакции:
store.get(20)   // → 'rolled'  (спека требует undefined)
```

## Прямое измерение (2026-08-25, юнит-проба, `cargo test -p lumen-js --features v8-backend`)

`rolled-back=false` — значение, записанное запросом, который успел
выполниться до `abort()`, переживает откат. Значение, записанное запросом,
который на момент `abort()` ещё стоял в очереди, не появляется — но лишь
потому, что запрос не выполнялся вовсе, а не потому, что был откат.

## Причина

Indexed DB §3.4.5 «abort a transaction», шаг 1: вернуть базу в состояние на
момент старта транзакции. В шиме нет ни журнала отмены, ни снапшота: все
мутации (`_write`, `delete`, `clear`, `cursor.update`/`delete`, а для
versionchange ещё `createObjectStore`/`deleteObjectStore`/`createIndex`/
`deleteIndex`) правят `_idb_databases` на месте, и ветка `_aborted` в
`_idb_flush_txn` про них ничего не знает.

Одно и то же место отвечает и за пользовательский `abort()`, и за аварийный
откат по необработанной ошибке запроса (`_idb_dispatch_request` выставляет
`txn._aborted`), — то есть после `ConstraintError` половина пакета записей
тоже остаётся в базе.

## Масштаб

Семейство `transaction-abort-*-revert` (4 файла: generator/index-metadata/
multiple-metadata/object-store-metadata) плюс `idbtransaction_abort`,
`abort-in-initial-upgradeneeded`, `idbindex-rename-abort`,
`idbobjectstore-rename-abort`.

## Направление починки (не предписание)

Ленивый снапшот стора при первой мутации внутри транзакции (записи —
поверхностные клоны `{key, value}`, плюс `keyGenerator`), для versionchange —
снапшот всей карты `data.stores` и `version`; восстановление в ветке `_aborted`
перед доставкой событий.
