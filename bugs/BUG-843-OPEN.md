# BUG-843 — IndexedDB: нет очереди соединений — второй `open()` с большей версией апгрейдит базу под живым соединением, `versionchange` и `blocked` не приходят никогда

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `idb-no-connection-queue`)
**Область:** `crates/js/src/dom.rs:13023` (`indexedDB.open` — кладёт запрос в `_idb_pending_opens` без проверки других соединений), `crates/js/src/dom.rs:12509` (`IDBDatabase.onversionchange` — поле есть, стрелять его некому), `crates/js/src/dom.rs:12351` (`IDBOpenDBRequest.onblocked` — то же), `crates/js/src/dom.rs:12977` (`_lumen_idb_flush` — апгрейд выполняется безусловно)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
// соединение 1 открыто и НЕ закрыто
db1.onversionchange = () => console.log('versionchange');   // не сработает
var r2 = indexedDB.open(name, 2);
r2.onblocked = () => console.log('blocked');                // не сработает
r2.onsuccess = () => console.log('success');                // сработает сразу
```

Апгрейд проходит немедленно, хотя старое соединение живо.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py --variant idb-versionchange-blocked`
(2026-08-22, dev-release, Linux, коммит `bafa603d9`, `--seconds 6`, страница
жива — 10 тиков):

| ожидалось | получено |
|---|---|
| `idb-versionchange` + `idb-blocked` + `idb-second-success v=2` | `idb-second-upgrade`, `idb-second-success v=2` |

То есть из трёх ожидаемых событий приходит только последнее, и приходит
раньше времени.

Контроль: `deleteDatabase` (`idb-delete-database`) и `createIndex`
(`idb-create-index`) работают полностью.

## Причина (локализована чтением кода)

`indexedDB.open` (`dom.rs:13023`) смотрит только на `_idb_databases[name]` —
на сами открытые соединения не смотрит никто. Поля `onversionchange`
(`:12509`) и `onblocked` (`:12351`) объявлены, но `grep` по воркспейсу не
находит ни одного места, которое бы их вызывало; `IDBDatabase.close()`
поэтому тоже ни на что не влияет.

Спека (Indexed DB §3.3.1 «connection queues», §6.3): перед апгрейдом всем
живым соединениям посылается `versionchange`, и пока хотя бы одно не
закрыто — запрос получает `blocked` и ждёт.

## Масштаб

Маркер `idb-no-connection-queue` в `tests/wpt/timeout_audit.py` — **1 id**
остатка снимка WPT-RUN-5, `IndexedDB/open-request-queue.any.html`: он ждёт
строгую последовательность событий, в которой `versionchange`/`blocked` —
обязательные звенья, поэтому TIMEOUT. Правило намеренно не считает уликой
голую строку `'versionchange'` — это ещё и *режим* upgrade-транзакции,
который пишет половина IndexedDB-набора, ничего при этом не ожидая.

## Направление починки (не предписание)

Вести список живых `IDBDatabase` по имени базы; в `open()` с большей версией
рассылать им `versionchange`, а запрос переводить в состояние ожидания с
`blocked`, пока список не опустеет. `close()` — снимать соединение со списка и
пинать очередь.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   idb-versionchange-blocked` — ожидаются все три маркера в порядке
   `idb-versionchange` → `idb-blocked` → `idb-second-success v=2`.
2. WPT: `run_report.py --all --root IndexedDB` — `open-request-queue` должен
   перестать быть TIMEOUT.
