# BUG-841 — IndexedDB: транзакция без запросов не завершается никогда, а `abort()` не заканчивает транзакцию синхронно

**Статус:** FIXED 2026-08-25 (P1, ветка `p1-bug841-idb-empty-txn`)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `idb-transaction-never-completes`)
**Область:** `crates/js/src/dom.rs:12548` (`IDBDatabase.prototype.transaction` — создаёт транзакцию, но не ставит её в очередь), `crates/js/src/dom.rs:12471` (`_idb_flush_txn` — единственное место, где выставляется `_finished` и стреляет `complete`), `crates/js/src/dom.rs:12434` (`IDBTransaction.prototype.abort`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var tx = db.transaction('store', 'readonly');
tx.oncomplete = () => console.log('complete');   // не сработает никогда:
                                                 // в транзакции нет ни одного запроса
```

```js
var tx = db.transaction('store', 'readwrite');
tx.abort();
tx.objectStore('store');    // не бросает; спека требует InvalidStateError —
                            // после abort() транзакция уже finished
```

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`, страницы живы — 10–11 тиков):

| вариант | ожидалось | получено |
|---|---|---|
| `idb-tx-empty` | `idb-empty-complete` | `idb-empty-armed` — и всё |
| `idb-tx-abort` | `idb-abort-fired` + `idb-store-after-abort-threw InvalidStateError` | `idb-store-after-abort-ok`, затем `idb-abort-fired` |

Контроль в другую сторону: транзакция **с** запросом завершается корректно и
в правильном порядке — `idb-tx-complete` даёт `idb-put-success` → `idb-tx-complete`,
`idb-tx-ordering` даёт ровно спековую последовательность
`rq1.onsuccess, tx1.oncomplete, rq2.onsuccess, tx2.oncomplete`, а
`idb-tx-deactivation` корректно бросает `TransactionInactiveError`.

## Причина (локализована чтением кода)

`_idb_flush_txn` — единственный путь к `complete`/`abort`, и попасть в него
можно только через `_idb_schedule_txn`, который зовут ровно два места:
`_idb_make_request` (`dom.rs:12494`) и `IDBTransaction.prototype.abort`. То
есть транзакция ставится в очередь **первым запросом внутри неё**; транзакция
без запросов не ставится никогда:

```js
// dom.rs:12548
IDBDatabase.prototype.transaction = function(storeNames, mode) {
    …
    return new IDBTransaction(this, storeNames, mode || 'readonly');   // и всё
};
```

Спека (Indexed DB §3.1.7) требует обратного: транзакция создаётся активной, а
как только управление возвращается в цикл событий и в ней нет незавершённых
запросов — она коммитится, вне зависимости от того, было ли в ней хоть что-то.

Второй фасет — `abort()` только помечает `_aborted = true` и планирует флаш,
а `_finished` выставляется позже, внутри `_idb_flush_txn`. Поэтому в том же
такте `objectStore()` (`dom.rs:12424`, проверка `if (this._finished)`) ещё
проходит.

## Масштаб

Маркер `idb-empty-transaction` в `tests/wpt/timeout_audit.py` — **3 id**
остатка снимка WPT-RUN-5: `IndexedDB/transaction-lifetime-empty.any.html`
(«Multiple transactions without requests complete in the expected order» —
две из трёх транзакций пустые), `idbtransaction-objectStore-exception-order.any.html`
и `idbobjectstore_createIndex.any.html` (у обоих проверка живёт в
`tx.oncomplete` транзакции, в которую не отправлено ни одного запроса). Все
три TIMEOUT, а не FAIL: тест ждёт `oncomplete`, которого не будет.

## Направление починки (не предписание)

Ставить транзакцию в очередь прямо в `IDBDatabase.prototype.transaction`
(тогда пустая транзакция коммитится на ближайшем флаше), а в `abort()`
выставлять `_finished = true` синхронно, оставив асинхронной только доставку
события `abort`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   idb-tx-empty --variant idb-tx-abort` — ожидаются `idb-empty-complete` и
   `idb-store-after-abort-threw InvalidStateError`.
2. WPT: `run_report.py --all --root IndexedDB` — `transaction-lifetime-empty`
   должен перестать быть TIMEOUT.

## Как починено (2026-08-25, P1)

Замер перед правкой (юнит-проба в `crates/js/src/dom.rs`, прогон
`cargo test -p lumen-js --features v8-backend`) подтвердил обе заявленные
грани — и нашёл **ещё четыре**, все в том же жизненном цикле транзакции:

| проверка | до | после |
|---|---|---|
| пустая транзакция | `oncomplete` не стреляет | `complete` |
| `objectStore()` после `abort()` | проходит | `InvalidStateError` |
| второй `abort()` | проходит | `InvalidStateError` |
| запрос, стоявший в очереди при `abort()` | `readyState` навсегда `pending`, ни `error`, ни `success` | `error: AbortError`, `readyState: done` |
| `db.transaction('s', 'bogus'/'versionchange')` | принимается | `TypeError` |
| `tx.commit()` | нет метода | есть, дальнейшие запросы → `TransactionInactiveError` |
| порядок трёх транзакций (WPT-кейс) | `rq1,rq2,tx1` | `rq1,rq2,tx1,tx2,tx3` |

**Три вещи о форме починки.**

Первая: состояние «finished» и «терминальное событие доставлено» — это два
разных флага. `abort()` обязан выставить `_finished` **синхронно** (спека
§3.4), но `_idb_flush_txn` открывался проверкой `if (txn._finished) return`, то
есть синхронный `_finished` заодно отменил бы доставку самого события `abort`.
Поэтому появился `_settled`; на него же переведён `_idb_process_open`, который
завершает versionchange-транзакцию в обход `_idb_flush_txn` (`abort()` внутри
`onupgradeneeded` кладёт её ещё и в `_idb_active_txns`, и без `_settled`
терминальное событие выстрелило бы дважды).

Вторая — то, чего заявка не называла и что сломано ровно тем же механизмом:
запрос, оставшийся в очереди в момент `abort()`, просто выбрасывался
(`txn._queue = []`). Это та же болезнь одним уровнем ниже: «объект никогда не
приходит в терминальное состояние», только для `IDBRequest`, а не для
транзакции. §3.4.5 шаг 3 требует выставить каждому `AbortError` и выстрелить
`error`; сделано отдельной функцией `_idb_abort_txn_requests`, а не через
`_idb_dispatch_request`, потому что действие запроса выполнять уже нельзя, а
его `error`-событие не должно повторно ронять уже падающую транзакцию.

Третья: постановка в очередь переехала из `_idb_make_request` в
`IDBDatabase.prototype.transaction`, и это же само собой починило порядок
коммитов — `_idb_active_txns` FIFO, так что транзакции теперь встают в него в
порядке создания, а не в порядке первого запроса. Ровно этого и требует
`transaction-lifetime-empty` («Multiple transactions without requests complete
in the expected order»).

Порядок исключений в `transaction()` выправлен по §3.3.4: проверка enum-а
`mode` — до всего (WebIDL конвертирует аргумент раньше первого шага алгоритма),
`NotFoundError` — **раньше** `TypeError` на `'versionchange'` (это валидное
значение enum-а, его отвергает шаг 5), `InvalidAccessError` на пустой список —
после проверки имён.

**Остаток, вынесен отдельно:** `abort()` не откатывает уже применённые записи
([BUG-913](BUG-913-FIXED.md)), а события
`error`/`abort` не всплывают с запроса на транзакцию и на соединение
(`db.onabort`/`db.onerror` не вызываются — [BUG-914](BUG-914-OPEN.md)).

## Проверка на WPT (2026-08-25, dev-release, `run_smoke.py`, три id из «Масштаба»)

| id | было | стало |
|---|---|---|
| `transaction-lifetime-empty.any.html` | TIMEOUT | **Test OK, 2/2 подтеста, 0 unexpected** |
| `idbtransaction-objectStore-exception-order.any.html` | TIMEOUT | Test OK (харнесс доходит до конца), 0/1 — порядок `InvalidStateError` перед `NotFoundError` уже правильный, подтест валится только на форме исключения ([BUG-915](BUG-915-OPEN.md)) |
| `idbobjectstore_createIndex.any.html` | TIMEOUT | TIMEOUT, 8/21 подтестов; остаток — [BUG-914](BUG-914-OPEN.md) (`t.done()` висит на `db.onabort`) и BUG-915 |

Прогон нашёл ещё два дефекта, к этому багу не относящихся и заведённых
отдельно: BUG-915 (весь шим бросает `Error`, а не `DOMException`, так что
`assert_throws_dom` отвергает правильное поведение) и [BUG-916](BUG-916-OPEN.md)
(запрос выполняется против схемы на момент доставки, а не постановки в очередь:
индекс, удалённый последней строкой скрипта, не действует и на запросы,
поставленные до удаления).

Тесты: `idb_empty_transaction_commits`,
`idb_empty_transactions_commit_in_creation_order`,
`idb_abort_finishes_transaction_synchronously`,
`idb_abort_settles_queued_requests_with_abort_error`,
`idb_transaction_commit_refuses_further_requests`,
`idb_transaction_rejects_invalid_mode` (`crates/js/src/dom.rs`).
