# WPT vendor notes — `IndexedDB`

## Vendoring (`tests/wpt/VENDOR.md`)

Third full test category, added 2026-07-22 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-IndexedDB`, `docs/wpt-status.md`). Same pinned commit, `git sparse-checkout` at the same commit hash, 245 files (includes upstream `crashtests/` subdir and `.tentative.` variants, kept as-is). Most tests import in-category `resources/support.js` (vendored); a few reference out-of-category helpers (`/common/*.js`, `/service-workers/*`) not vendored — same documented survey gap as FileAPI.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-07-22 (коммит `c8cad69f`, `tests/wpt/IndexedDB/`, 245 файлов; `common/`/`service-workers/`-хелперы, на которые ссылаются немногие тесты, НЕ довендорены). Прогон `run_report.py --all --root IndexedDB --recursive`: 230 id получили результат, 197/230 harness OK (33 не добежали — почти все TIMEOUT/внешний hang, 1 `SKIP` из-за отсутствующей поддержки `testdriver.js` в минимальном исполнителе), 393/1225 сабтестов passed. Замеченные кластеры провалов (не заведены как BUG-NNN — первый проход, см. методологию выше): `IDBCursor.continuePrimaryKey` отсутствует (`TypeError: cursor.continuePrimaryKey is not a function`); брошенные ошибки в ряде путей курсора/ключей (`DataError`/`ReadOnlyError`/`TransactionInactiveError`) не всегда настоящие `DOMException` — у части `.code`/`.name` не совпадает с ожидаемым (напр. `indexedDB.cmp()` с BigInt-ключом кидает объект без `code`); транзакции: «Expected success event, but got upgradeneeded event» в ряде сценариев после `abort()` — расхождение в жизненном цикле версий; BigInt как ключ не поддержан (`TypeError: Do not know how to serialize a BigInt` во внутреннем JSON-based персисте) и структурное клонирование с циклами тоже падает на persist (`Converting circular structure to JSON`) — намекает, что внутренний слой персистентности IndexedDB сериализует значения через JSON, а не через настоящий structured-clone


## Разведка WPT-IDB-SCOUT (2026-10-26)

Статус: **закрыта** (2026-10-26). Код не менялся. Дефекты — BUG-1705…1710 и [BUG-914](../../bugs/BUG-914-OPEN.md) (указатели в `STATUS-P3.md`), фича — `IDB-EVENTS` (P1, `ROADMAP.md`), раннер — `WPT-IDB-RUNNER` (P2).

**Прогон** (`run_corpus.py --prefixes IndexedDB --processes 7`, `dev-release` от `main` `6a43c1d0d`): 813 id, **12.02 %**, harness OK 556, сабтесты 582/3 692 (статусы: OK 556, TIMEOUT 211, ERROR 46). Повтор на чистом `target/dev-release/data/idb` — 12.05 %, то есть накопленное состояние IDB счёт почти не двигает (но ломает `indexedDB.databases()`, см. кластер N). Данные: `.tmp/wpt-idb2/` (первый), `.tmp/wpt-idb3/` (чистый) в слоте `p2-work`; классификатор — `.tmp/idb_cl.py`; пробы — `.tmp/probes/zz-scout-probe*.{html,any.js}` (в `tests/wpt/IndexedDB/` их держать нельзя — попадут в корпус).

**Главное.** 71 п.п. из 88 потерянных — три «варианта-близнеца», а не дефекты API: `indexedDB` **не определён** в `DedicatedWorker` и `SharedWorker` (`typeof indexedDB`, `IDBFactory`, `IDBKeyRange`, `IDBDatabase` — `undefined`; в ServiceWorker шим есть — `sw_worker.rs:779`), а все 184 `*.any.serviceworker.html` — TIMEOUT по [BUG-1226](../../bugs/BUG-1226-OPEN.md) (так же в 641 id всего корпуса). Потолок по близнецам (если вариант достигнет доли своего `.any.html`-близнеца): воркер +92.8 б. (+11.4 п.п.), shared +80 б. (+9.8 п.п.), SW +80 б. (+9.8 п.п., только после BUG-1226).

### Таблица кластеров

Балл — потолок (Σ потерянных долей сабтестов); кластеры перекрываются, реальный прирост ниже. «Движок» = `crates/js/src/shim/idb_shim.js` (JS-шим IndexedDB: `IDBRequest`/`IDBTransaction` — **не `EventTarget`**, события — самодельные объекты `_idb_make_event`, слушатели — живые массивы).

| Кл. | Группа | id | Сабтестов | Статус | Причина (представитель) | Балл (п.п.) |
|---|---|---|---|---|---|---|
| A | `*.any.worker.html`: `indexedDB`/`IDBKeyRange` не определены | 214 | 1 205 | OK(harness)/ERROR | движок: шим не ставится в `worker.rs` (`install_worker_scope_globals_v8`), в отличие от `sw_worker.rs`. Проба w1 | 212.6 (26.1) |
| B | `*.any.sharedworker.html`: то же | 184 | 1 020 | OK/ERROR/TIMEOUT | движок: `shared_worker.rs` без шима | 182.6 (22.5) |
| C | `*.any.serviceworker.html` | 184 | 0 | TIMEOUT | BUG-1226 (регистрация `scope: "does/not/exist"`, `importScripts` от scope); общая для корпуса | 184.0 (22.6) |
| D | `Expected success event, but got upgradeneeded event` | 48 | 242 | FAIL → часто TIMEOUT всего теста | движок: `_idb_dispatch_request` обходит **живой** массив слушателей, поэтому `EventWatcher`, добавленный *внутри* `onupgradeneeded` (`promiseForRequest` в `migrateNamedDatabase`), получает то же `upgradeneeded`. Проба p1: `calls=1`. По спецификации список слушателей клонируется перед диспетчем | 45.5 (5.6) |
| E | `getAll*({query,count,direction})` + `getAllRecords()` | 9 | 172 | FAIL | движок: нет `IDBObjectStore/IDBIndex.getAllRecords`, `getAll(options)` → `DataError: invalid key or range`. Проба p2 | 6.9 (0.85) |
| F | исключения не бросаются/не `DOMException` (после `abort()` у `cursor.advance()`, удалённый store/index, `put` без ключа и т.п.) | 45 | 129 | FAIL | движок: проверки состояния в курсоре/индексе/сторе неполные. Проба p6: `advance()` после `abort()` — «no throw» | 22.3 (2.7) |
| G | валидация `keyPath` | 3 | 26 | FAIL | движок: `createObjectStore(…, {keyPath:'j a'})`, `'a..b'`, `'.a'` — «ok», должен быть `SyntaxError`. Проба p4 | 1.3 (0.16) |
| H | `idlharness.any.html` | 1 | 161 | FAIL | движок: нет глобалов `IDBFactory` и `IDBVersionChangeEvent`; свойства-данные вместо аксессоров (`IDBRequest.result` — own), интерфейсные объекты перечислимы, `.length`/`.name` методов. Проба p5 | 0.8 (0.1) |
| I | бинарные ключи (`ArrayBuffer`/view), `indexedDB.cmp` | 4 | 29 | FAIL | движок: `_idb_is_valid_key` не знает бинарных ключей (`DataError: invalid key`; `put` с `Uint8Array`-ключом → abort). Проба p3 | 3.5 (0.43) |
| J | порядок проверок (`*-exception-order`) | 13 | 45 | FAIL | движок: `InvalidStateError`/`TransactionInactiveError`/`DataError` проверяются в другом порядке | 8.2 (1.0) |
| K | события: исключение в обработчике, bubbling, active-флаг | 9 | 41 | FAIL | движок: [BUG-914](../../bugs/BUG-914-OPEN.md) (проба p8: `db.onerror`/`tx.onerror` = 0 вызовов), исключение в слушателе не отменяет транзакцию, транзакция активна «в следующей задаче» | 7.0 (0.86) |
| L | `close()` в `upgradeneeded` | 2 | 13 | FAIL | движок: `open` после `db.close()` в апгрейде обязан дать `AbortError`, шим шлёт `success` | 1.4 (0.18) |
| M | атрибуты `[[Writable]]` | 5 | 15 | FAIL | движок: `cursor.direction/key/primaryKey/source` — own-свойства данных | 4.3 (0.53) |
| N | harness TIMEOUT/ERROR без сабтестов | 15 | 0 | TIMEOUT/ERROR | раннер: `indexedDB.databases()` видит БД параллельных тестов того же origin (`abort-in-initial-upgradeneeded`: `dbs.length` 1…231 вместо 0) и БД, накопленные с прошлых прогонов (`target/dev-release/data/idb`: 63 МБ после одного прогона, 450 МБ WAL после двух). Лечится `--processes 1` для `databases()`-тестов и чисткой `data/idb` перед прогоном | 1.0 (0.12) |
| O | opaque-origin / partitioned / cross-realm `.html` | 11 | 33 | TIMEOUT/FAIL | смесь: sandbox-iframe не бросает `SecurityError` (движок), `idb-partitioned-*`, `*-cross-realm-methods` зависают (BiDi 20 с) | 10.0 (1.2) |
| P | `structured-clone.any.html` (6 вариантов) + `parallel-cursors-upgrade.any.html` | 7 | 0 | ERROR | `navigate … DocumentReady` > 30 с: страница не возвращает управление при загрузке. Для `structured-clone` bisect-пробой сужено: зависание **после** строки ~301 `new File([...])` (сами `File`, `input.files`, `Event`, `MessageChannel` по отдельности в порядке); участок 301–335 (`cloneObjectTest(File…)`, `cloneFailureTest`) не добит | ≈7 (0.9) |
| Q | прочее (по одному-двум id: `clone-before-keypath-eval`, `keygenerator` 53 бита, `cursor.request`, `continuePrimaryKey`, `blob-*`) | 44 | 102 | FAIL | разнородно | 23.1 (2.85) |

### Находки вне кластеров

- **Микротаск после `success` опаздывает** (проба p7b): `_idb_flush_txn` рассылает запросы и коммитит транзакцию в одном синхронном проходе, поэтому `await`/`.then()` после события видит завершённую транзакцию (`TransactionInactiveError`, `complete` пришёл раньше микротаска). Нужен microtask checkpoint между событием и деактивацией. Прячется за кластером D: после его правки promise-стиль тестов упрётся сюда.
- `IDBRequest.addEventListener` знает только `success`/`error`, `IDBTransaction` — `complete`/`abort`; нет `dispatchEvent`, `once`, `handleEvent`, наследования от `Event`.
- Персистентность — «непрозрачный снимок» всей БД origin: рост `data/idb` растёт с числом тестов в одном origin.
- `DOMMatrix is not defined` в воркере (5 id `structured-clone.any.worker.html`) и `crypto` в воркере ([BUG-1688](../../bugs/BUG-1688-FIXED.md)) — не IDB.

### Оформление (сделано)

| Кластер | Куда ушло |
|---|---|
| A, B | [BUG-1705](../../bugs/BUG-1705-OPEN.md) — шим IndexedDB не ставится в Dedicated/SharedWorker |
| C | [BUG-1226](../../bugs/BUG-1226-OPEN.md) (общая для корпуса) |
| D | [BUG-1706](../../bugs/BUG-1706-OPEN.md) + `IDB-EVENTS` |
| E | [BUG-1707](../../bugs/BUG-1707-OPEN.md) |
| G | [BUG-1708](../../bugs/BUG-1708-OPEN.md) |
| I | [BUG-1709](../../bugs/BUG-1709-OPEN.md) |
| P | [BUG-1710](../../bugs/BUG-1710-OPEN.md) |
| F, H, J, K, L, M | `IDB-EVENTS` (P1, `ROADMAP.md`); K — также [BUG-914](../../bugs/BUG-914-OPEN.md) (дописана микротаск-находка) |
| N | `WPT-IDB-RUNNER` (P2, сделано 2026-10-27, см. ниже) — serial lane и чистка `data/idb`; остаток — [BUG-1753](../../bugs/BUG-1753-OPEN.md) |
| O, Q | не заведены: разнородный хвост по одному-двум id; брать после `IDB-EVENTS` повторной разведкой |

## Раннер: serial lane и чистка `data/idb` (WPT-IDB-RUNNER, 2026-10-27)

**Правило.** У всех тестов прогона общие origin-ы wptserve и одно хранилище IndexedDB — `<каталог бинаря>/data/idb` (`crates/shell/src/storage_stores.rs`; отдельного профиля у браузера нет). Поэтому `tests/wpt/run_corpus.py`:

- перед прогоном очищает `data/idb` целиком (`--keep-idb` — не чистить);
- тесты из `tests/wpt/serial_tests.txt` — те, что читают или стирают всё хранилище origin-а (`indexedDB.databases()`, `deleteAllDatabases` из `support-promises.js`, `cross-origin-helper-frame.html`), — выносит из общих шардов в юнит `serial lane`: после тяжёлой полосы, один, на 1 браузере, перед ним `data/idb` очищается ещё раз. Из знаменателя ничего не исключается; `--no-serial-lane` — по-старому.

Сейчас в списке 9 id: `abort-in-initial-upgradeneeded.any{,.worker,.sharedworker,.serviceworker}.html`, `get-databases.any{,.worker}.html`, `idbfactory-origin-isolation.html`, `idbfactory-databases-opaque-origin.html`, `database-names-by-origin.html`. Новый тест с `databases()` — строкой в список (каждый `.any`-вариант отдельно).

**Проверка** (`--prefixes IndexedDB --processes 7`, `dev-release` слота `p2-work` от 2026-10-27; данные — `.tmp/wpt-idbr{1,2,3}/` слота):

| Прогон | Балл | % | harness OK | Сабтесты | Статусы |
|---|---|---|---|---|---|
| 1 | 98.33 | 12.09 | 569 | 585/3 805 | OK 569, TIMEOUT 203, ERROR 41 |
| 2 | 97.33 | 11.97 | 560 | 584/3 697 | OK 560, TIMEOUT 207, ERROR 46 |
| 3 | 98.33 | 12.09 | 570 | 585/3 832 | OK 570, TIMEOUT 203, ERROR 40 |

Прогоны 1 и 3 совпали по баллу (`run_diff.py`: 0 хуже, 0 лучше, 4 смены статуса без сабтестов). Между 2 и 3 (и 1 и 2) балл сменил один тест — `open-request-queue.any.html` (OK 1/1 ↔ TIMEOUT): у него собственное имя БД, хранилище соседей его не задевает, это нагрузочный флейк 7 браузеров. Остальные смены статуса — без сабтестов (`structured-clone.any.html?*` — [BUG-1710](../../bugs/BUG-1710-OPEN.md), `*-metadata-revert`, `reading-autoincrement-*`). Тесты serial lane в трёх прогонах дают одинаковые статусы и сабтесты.

**Что осталось от кластера N.** `abort-in-initial-upgradeneeded.any.html` в serial lane на пустом хранилище стабильно ERROR `expected 0 but got 1`: это уже не соседи (было 1…231), а собственная БД теста, которую шим не удаляет после отката первого upgrade, — [BUG-1753](../../bugs/BUG-1753-OPEN.md). Остальные тесты списка упираются в известные кластеры: D (`get-databases`, `idbfactory-origin-isolation`, `database-names-by-origin` — `Expected success event, but got upgradeneeded`), A/B/C (воркерные варианты), O (`idbfactory-databases-opaque-origin` — sandbox не бросает `SecurityError`). Остальные 11 id кластера N (`idb-partitioned-*`, `*-opaque-origin`, `structured-clone?81-…`, `idlharness`, `keypath`, `name-scopes`, `idbobjectstore_createIndex`, `worker-termination-aborts-upgrade`) к хранилищу не относятся и раннером не лечатся.

**Рост хранилища за один прогон.** После прогона `IndexedDB` на очищенном `data/idb` — 172 МБ (прогон 1) и 707 МБ (прогон 2): персистентность пишет снимок всей БД origin-а (см. «Находки вне кластеров»), и объём WAL зависит от порядка тестов. Чистка перед прогоном и перед serial lane это гасит; само поведение — движок.
