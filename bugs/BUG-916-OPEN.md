# BUG-916 — IndexedDB: запрос выполняется против схемы на момент ДОСТАВКИ, а не на момент постановки в очередь

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, прогоном WPT при проверке [BUG-841](BUG-841-FIXED.md))
**Область:** `crates/js/src/dom.rs` — отложенная модель выполнения: `_idb_make_request` кладёт замыкание, `_idb_dispatch_request` выполняет его при флаше; `IDBObjectStore.prototype.createIndex`/`deleteIndex` правят `store.indexes` **синхронно**
**Владелец:** P1 (`lumen-js`)

## Симптом

```js
store.add({animal: 'Unicorn'}, 1);
store.createIndex('index', 'animal', {unique: true});
store.add({animal: 'Unicorn'}, 2);   // ожидается ConstraintError
store.deleteIndex('index');          // синхронно, до флаша
store.add({animal: 'Unicorn'}, 3);   // ожидается success
```

Получаем `success` на всех трёх: к моменту, когда флаш выполняет второй `add`,
индекса уже нет — его удалила строка, написанная позже, но выполненная раньше.

## Прямое измерение (2026-08-25, dev-release, `run_smoke.py`)

`/IndexedDB/idbobjectstore_createIndex.any.html`, подтест «Event ordering for a
later deleted index»: ожидалось
`["rq_add1.success", "rq_add2.error: ConstraintError", "rq_add3.success", …]`,
получено `["rq_add1.success", "rq_add2.success", "rq_add3.success", …]`.

## Причина

Модель шима: действие запроса (чтение/запись) — это замыкание, выполняемое при
доставке, в FIFO-порядке транзакции. Для **данных** это верно и специально так
сделано (см. `subsystems/js.md`, «Deferred execution model»). Но схема
(`indexes`, а также `createObjectStore`/`deleteObjectStore`) меняется
**синхронно**, в момент вызова, — то есть к моменту доставки любого запроса
транзакции схема уже такая, какой её оставил конец скрипта. Спека же (Indexed
DB §3.2.9, §3.2.10) считает `createIndex`/`deleteIndex` тоже операциями
транзакции: индекс существует для запросов, поставленных после его создания и
до его удаления.

## Масштаб

Проверки порядка событий в `idbobjectstore_createIndex.any.html` (минимум два
подтеста), плюс всё, где уникальный индекс создаётся посреди транзакции с уже
поставленными записями. Не путать с [BUG-914](BUG-914-OPEN.md) (всплытие
событий) — там тот же файл, но другой механизм.

## Направление починки (не предписание)

Ставить схемные мутации в ту же очередь запросов (действие, выполняемое при
доставке), сохраняя синхронным только возврат обёртки `IDBIndex`/
`IDBObjectStore` — так спека и устроена: объект существует сразу, операция
применяется в очереди.
