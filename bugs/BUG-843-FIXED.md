# BUG-843 — IndexedDB: нет очереди соединений — второй `open()` с большей версией апгрейдит базу под живым соединением, `versionchange` и `blocked` не приходят никогда

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `idb-no-connection-queue`)
**Область:** `crates/js/src/dom.rs` (`IDB_SHIM`: `indexedDB.open`/`deleteDatabase`, `_idb_process_open`, `_lumen_idb_flush`, `IDBDatabase`, `IDBOpenDBRequest`)
**Владелец:** P1 (`lumen-js`). Заведён P2 в ходе WPT-задачи.

## Симптом

```js
// соединение 1 открыто и НЕ закрыто
db1.onversionchange = () => console.log('versionchange');   // не срабатывал
var r2 = indexedDB.open(name, 2);
r2.onblocked = () => console.log('blocked');                // не срабатывал
r2.onsuccess = () => console.log('success');                // срабатывал сразу
```

Апгрейд проходил немедленно, хотя старое соединение живо.

## Что оказалось на самом деле: заявка называла один дефект, их шесть

Одноразовая юнит-проба **до** правки (печатает фактическое поведение через
`panic!`, приём из BUG-841) нашла к заявленному ещё пять. Ни один из них не
виден из чтения названной в заявке функции.

| # | Дефект | Как обнаружен |
|---|---|---|
| 1 | Очереди соединений нет: `versionchange` не рассылается, `blocked` не стреляет, апгрейд идёт под живым соединением | заявка |
| 2 | `IDBDatabase` **вообще не EventTarget**: `addEventListener`/`removeEventListener` отсутствуют, вызов бросает `TypeError` | проба |
| 3 | `IDBOpenDBRequest.addEventListener('blocked', …)` молча теряется — реестра `_blockedListeners` нет, а базовый `IDBRequest.addEventListener` неизвестные типы игнорирует | проба |
| 4 | `deleteDatabase` удаляла базу **синхронно в момент вызова**, до всякой очереди: живое соединение продолжало работать с удалённой базой (`transaction()` на ней проходил) | проба |
| 5 | `open()` разрешала версию, соединение и признак апгрейда в момент вызова: запрос, стоящий в очереди за удалением, видел базу такой, какой она была до удаления | найдено при реализации очереди — латентный, недостижимый без п.1 |
| 6 | Соединение аварийно завершённой версионной транзакции оставалось зарегистрированным навсегда | **найдено прогоном WPT после первой правки**, а не чтением: две ранее зелёные подпроверки `idbrequest-onupgradeneeded` ушли в TIMEOUT |

Дефект 6 — тот же урок, что в BUG-839: последний дефект нашла не проба, а
прогон корпуса. Проба заводит только те ситуации, которые сама называет; она
не строила упавший апгрейд.

## Починка

Очередь соединений (§3.3.1) целиком в JS-шиме.

**Реестр живых соединений** `_idb_connections` (имя → `[IDBDatabase]`).
Соединение регистрируется в `_idb_process_open` — до `upgradeneeded`, чтобы
удаление, стоящее в очереди за этим открытием, увидело соединение, которое
апгрейд держит.

**Всё решение перенесено с момента вызова на момент, когда очередь дошла до
запроса** (дефекты 4/5): сравнение версий, `VersionError`, создание
`IDBDatabase`, само удаление. Иначе запрос за удалением работает с базой,
которой уже нет.

**Ожидание вместо повторной постановки** (`_idb_park_open`/`_idb_unpark`).
Заблокированный запрос уходит в `_idb_parked_opens`, и вместе с ним блокируется
вся остальная очередь этого имени — §3.3.1 обрабатывает очередь по порядку, и
запрос за заблокированным не имеет права его обогнать. Разбудить парковку может
только `close()`. Это принципиально: вернуть запрос в `_idb_pending_opens`
означало бы такт событийного цикла на каждый оборот, то есть ровно тот
неограниченный слив, который чинил [BUG-842](BUG-842-FIXED.md).

**`close()` — «close pending», а не «closed»** (§3.3.9): соединение перестаёт
принимать транзакции сразу, но продолжает блокировать апгрейд, пока его
собственные транзакции не завершились — иначе апгрейд поедет поверх пишущей
транзакции. Отсюда же и дефект 6: соединение отменённой версионной транзакции
странице не отдают, и оставлять его в реестре нельзя.

`IDBDatabase` получил `addEventListener`/`removeEventListener` (типы
`versionchange`/`abort`/`error`/`close`; диспетчер сегодня есть только у
`versionchange` — остальные три ждут [BUG-914](BUG-914-OPEN.md)), а
`IDBOpenDBRequest` — реестр `blocked` и собственный `removeEventListener`.
Заодно `success` у `deleteDatabase` несёт `oldVersion`/`newVersion: null`.

## Замеры

**Живая проба** (`verify_perf_idb_sse_gaps.py --variant idb-versionchange-blocked`,
Windows, dev-release, `--seconds 8`):

| до | после |
|---|---|
| `idb-second-upgrade`, `idb-second-success v=2` | `idb-versionchange`, `idb-blocked`, `idb-second-upgrade`, `idb-second-success v=2` |

Контроли `idb-delete-database`, `idb-tx-empty` — без изменений.

**WPT, вся категория** `run_report.py --all --root IndexedDB --recursive`,
A/B на одной машине (Windows, dev-release, Vulkan), 231 id:

| | до | после |
|---|---|---|
| harness OK | 209 | 203 |
| подтестов PASS | 418 | 421 |

- `IndexedDB/open-request-queue.any.html` — **TIMEOUT → OK**, единственный id
  маркера `idb-no-connection-queue`, ради которого заявка и заведена.
- `+3` подтеста: обе `idbdatabase_close` «Unblock the delete database request» /
  «Unblock the version change transaction created by an open database request» и
  `idbfactory_deleteDatabase` «deleteDatabase() request should have no source…».

## Остаточное — что стоило категории 7 id и втрое больше времени

Это не побочный эффект, а сама спека: заблокированный запрос ждёт, а страница,
которая не закрывает соединение, ждёт вечно. Телеметрия (временный
`_lumen_console_error` в `_idb_park_open`/`_lumen_idb_flush`/`_idb_close_connection`)
показала на `idb-explicit-commit`: **12 парковок, 0 вызовов `close()`** —
`support-promises.js` закрывает соединение из `add_cleanup`, который
регистрируется в `.then()` цепочки, а цепочка на этом движке не доходит до конца
из-за прежних, посторонних дефектов IndexedDB. В настоящем браузере эти
подпроверки проходят, соединение закрывается и удаление идёт дальше.

Из 7 ушедших в TIMEOUT (`get-databases`, `idb-explicit-commit`,
`idbdatabase_createObjectStore`, `idbobjectstore-rename-store`, `name-scopes`,
`reading-autoincrement-indexes`, `transaction-abort-index-metadata-revert`)
**пять — чистая медленность, а не иной результат**: с `--timeout-multiplier 4`
они снова `Test OK` с побайтово теми же вердиктами подтестов. Постоянно висят
двое — `name-scopes` и `get-databases`, оба на утёкшем соединении.

Стоимость по времени: 406 с → 1231 с на категорию, замедление размазано по всем
231 id (0.1 с → 17.4 с у `idb-explicit-commit`). Причина второго порядка —
заблокированное удаление означает, что база остаётся в `_idb_databases`, а
снапшот пересериализуется и схема перезеркаливается на **каждом** грязном флаше,
O(все базы × все хранилища). Это отдельный, уже существовавший дефект —
[BUG-917](BUG-917-OPEN.md).

Прочее, намеренно не сделанное: `dispatchEvent` у `IDBDatabase`/`IDBRequest`/
`IDBTransaction` по-прежнему нет (у шима своя диспетчеризация, общая заявка —
[BUG-914](BUG-914-OPEN.md)); принудительное закрытие соединения и событие
`close` не реализованы — `onclose` остаётся объявленным полем без источника.

## Как проверить

1. `cargo test -p lumen-js --features v8-backend --lib idb` — 40 тестов, из них
   семь на очередь соединений (`idb_upgrade_sends_versionchange_and_waits_for_close`,
   `idb_upgrade_is_not_blocked_by_a_closed_connection`,
   `idb_versionchange_handler_may_close_inline`,
   `idb_delete_database_blocks_on_an_open_connection`,
   `idb_open_behind_a_delete_sees_the_deleted_database`,
   `idb_open_and_delete_requests_form_a_fifo_queue`,
   `idb_close_waits_for_a_running_transaction`).
2. `verify_perf_idb_sse_gaps.py --variant idb-versionchange-blocked` — три
   маркера в порядке `idb-versionchange` → `idb-blocked` → `idb-second-success v=2`.
3. WPT: `run_report.py --all --root IndexedDB --recursive` — `open-request-queue`
   должен быть OK.
