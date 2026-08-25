# BUG-914 — IndexedDB: события `error`/`abort` не всплывают с запроса на транзакцию и на соединение

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, чтением кода в ходе [BUG-841](BUG-841-FIXED.md))
**Область:** `crates/js/src/dom.rs` — `_idb_dispatch_request` (стреляет `error`
только по самому `IDBRequest`), `_idb_fire_txn` (стреляет `complete`/`abort`
только по самой `IDBTransaction`)
**Владелец:** P1 (`lumen-js`)

## Симптом

```js
tx.onabort = function() { /* вызывается */ };
db.onabort  = function() { /* НЕ вызывается никогда */ };
db.onerror  = function() { /* НЕ вызывается никогда */ };
```

## Причина

Indexed DB §3.5.6: `error` у `IDBRequest` и `abort` у `IDBTransaction` —
всплывающие события, путь всплытия `request → transaction → database`. Событие
`error` в `_idb_dispatch_request` строится с `bubbles: true`, но диспетчер
обходит только слушателей самого запроса; `_idb_fire_txn` — только слушателей
самой транзакции. `IDBDatabase.onabort`/`onerror` объявлены в конструкторе и не
вызываются ниоткуда. `IDBDatabase.addEventListener`/`removeEventListener`
появились в [BUG-843](BUG-843-FIXED.md) (2026-08-25) и принимают в том числе
`abort`/`error` — но диспетчера у этих двух типов по-прежнему нет, так что
регистрация теперь не бросает `TypeError` и всё так же ничего не даёт.

Своя механика диспетчеризации у IndexedDB-шима (а не общий `_lumen_dispatch`)
— причина, по которой сюда не дотянулась ни одна из общих правок событийного
пути.

## Масштаб

`transaction_bubble-and-capture.any.js` целиком; проверка `db.onabort` есть в
`idbtransaction_abort.any.js` («Abort event should fire during transaction») и
в двух подтестах `idbobjectstore_createIndex.any.js`, где `t.done()` висит
именно на `db.onabort`. Ожидаемые последовательности в
`idbobjectstore_createIndex` («Event order when unique constraint is
triggered») перечисляют `transaction.error` и `db.error` явно.

## Направление починки (не предписание)

Дать `IDBRequest`/`IDBTransaction`/`IDBDatabase` общий путь всплытия внутри
шима (цепочка `request.transaction` → `transaction.db`) и `addEventListener`
у `IDBDatabase`; `preventDefault` на всплывшем `error` уже учитывается в
`_idb_dispatch_request` и должен продолжать гасить аварийный откат транзакции.
