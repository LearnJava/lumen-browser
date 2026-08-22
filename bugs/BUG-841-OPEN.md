# BUG-841 — IndexedDB: транзакция без запросов не завершается никогда, а `abort()` не заканчивает транзакцию синхронно

**Статус:** OPEN
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
