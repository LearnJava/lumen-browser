# BUG-842 — IndexedDB: самоперевзводящийся запрос (`keep_alive`) съедает страницу целиком — 16,3 млн итераций за 6 с, ни таймеров, ни рендера, ни остального скрипта

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `idb-keep-alive-spin`)
**Область:** `crates/js/src/dom.rs:12474` (`_idb_flush_txn` — `while (txn._queue.length > 0 && !txn._aborted)`), `crates/js/src/dom.rs:12494` (`_idb_make_request` кладёт новый запрос в тот же `_queue`), `crates/js/src/dom.rs:12466` (`_idb_schedule_flush` — весь флаш идёт одним микротаском)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Идиома, которой WPT держит транзакцию открытой
(`IndexedDB/resources/support.js::keep_alive`, `dom.rs`-независимый код):

```js
function spin() {
    if (!keepSpinning) return;
    tx.objectStore(store_name).get(0).onsuccess = spin;   // один запрос за такт
}
spin();
```

В настоящем браузере это один запрос на оборот цикла событий: таймеры,
рендер и остальные скрипты продолжают работать. В Lumen страница умирает
целиком — не выполняется ни таймер, ни следующий `<script>` документа.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`):

| вариант | ожидалось | получено |
|---|---|---|
| `idb-keep-alive` | `idb-alive-checked spins>1 completed=false` | **ни одной строки вообще**, 0 тиков — страница мертва |
| `idb-spin-unbounded` | `spin-count` растёт И `spin-timer-ran` | `spin-page-start`, `spin-open-success`, затем `spin-count` до **16 340 900** за ~6 с; `spin-timer-ran` — нет, `PROBE tick` — нет, `script-start` следующего скрипта — нет |
| `idb-spin-bounded` (контроль) | 5 × `idb-spin-returned depth=1` | ровно это — при ограниченном числе итераций всё корректно и асинхронно (`maxDepth=1`) |

`.tmp/psig-idb-spin-unbounded.log` — 326 829 строк, из них 326 818 — счётчик
спина: доказательство, что цикл крутится на полной скорости, а никакой другой
источник задач к работе не допущен.

## Причина (локализована чтением кода)

```js
// dom.rs:12471
function _idb_flush_txn(txn) {
    if (txn._finished) return;
    while (txn._queue.length > 0 && !txn._aborted) {
        _idb_dispatch_request(txn._queue.shift());
    }
```

`_idb_dispatch_request` синхронно вызывает `onsuccess` запроса; обработчик
создаёт новый запрос, а `_idb_make_request` кладёт его **в тот же
`txn._queue`** (`dom.rs:12494`). Условие цикла проверяется заново — очередь
снова непуста. Весь флаш при этом выполняется внутри одного микротаска
(`queueMicrotask(_lumen_idb_flush)`, `dom.rs:12468`), поэтому цикл событий
страницы не получает управление никогда.

Спека (Indexed DB §3.1.7 «transaction steps») требует обрабатывать запросы
транзакции как отдельные задачи, а не сливать их в один прогон.

## Почему это важно за пределами IndexedDB

Это идеальный кандидат в источники механизма `hung-browser`
(`timeout_audit.py`, срез 11): зависшая страница не перезапускает браузер,
поэтому все оставшиеся тесты шарда таймаутят молча. `keep_alive` используют
четыре теста подряд в одной категории.

## Масштаб

Маркер `idb-keep-alive-spin` в `tests/wpt/timeout_audit.py` — **5 id**
остатка снимка WPT-RUN-5. Три зовут хелперы `keep_alive(` /
`is_transaction_active(` из `IndexedDB/resources/support.js`
(`event-dispatch-active-flag`, `transaction-deactivation-timing`,
`upgrade-transaction-deactivation-timing`), ещё два пишут тот же цикл руками и
держат так **две** транзакции сразу (`transaction-scheduling-within-database`,
`transaction-scheduling-across-databases`). Маркер читает только собственный
файл теста: `support.js` *определяет* `keep_alive`, и по склеенному тексту
правило забирало 16 id вместо 5.

## Направление починки (не предписание)

Обрабатывать за один флаш ровно один запрос транзакции (или снимок очереди,
взятый до цикла), а продолжение планировать новой задачей — тогда
самоперевзводящийся обработчик даёт один запрос на такт, как и в спеке.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   idb-spin-unbounded` — ожидаются одновременно `spin-count` и
   `spin-timer-ran`, плюс `PROBE tick`.
2. WPT: `run_report.py --all --root IndexedDB` — `event-dispatch-active-flag`
   должен перестать быть TIMEOUT (и перестать топить соседей по шарду).
