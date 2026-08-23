# BUG-776 — `WorkerGlobalScope` не даёт ни `self.location`, ни `self.navigator` в dedicated- и shared-воркерах

**Статус:** OPEN
**Компонент:** js (`crates/js/src/worker.rs:264` — `worker_global_shim`, IIFE не объявляет `location`/`navigator`; `crates/js/src/shared_worker.rs:99` — `SHARED_WORKER_GLOBAL_SHIM`, та же дыра; `crates/js/src/sw_worker.rs:456` — `WorkerLocation` в сервис-воркере есть, `navigator` там же отсутствует)
**Найден:** P2, WPT-VENDOR-workers, 2026-08-18 — `run_report.py --all --root workers --recursive`

## Симптом

Любой воркерный скрипт, читающий `location.*` или `navigator.*` (стандартный
паттерн `WorkerLocation`/`WorkerNavigator` тестов, а также вспомогательных
скриптов других тестов категории), падает синхронно ещё до первого
`postMessage` — исключение никуда не долетает (у воркера нет своего окна,
`onerror` родителя эту ошибку не видит), поэтому WPT-харнесс видит не FAIL,
а **TIMEOUT**. Прогон `run_report.py --all --root workers --recursive`
(287 id, ~29 мин): **94/249 harness OK, 153/877 сабтестов**; в логе —
21 × `Runtime("location is not defined")`, 28 ×
`Runtime("Cannot read properties of undefined (reading 'href')")` (то же самое
через `location.href`) и 7 × `Runtime("navigator is not defined")`. Список
файлов, зависших именно на этом:

```
WorkerLocation.htm, WorkerLocation_hash.htm, WorkerLocation_hash_encoding.htm,
WorkerLocation_hash_nonexist.htm, WorkerLocation_host.htm, WorkerLocation_hostname.htm,
WorkerLocation_href.htm, WorkerLocation_pathname.htm, WorkerLocation_port.htm,
WorkerLocation_protocol.htm, WorkerLocation_search.htm, WorkerLocation_search_empty.htm,
WorkerLocation_search_fragment.htm, WorkerLocation_search_nonexist.htm,
WorkerLocation-origin.sub.window.html,
interfaces/WorkerGlobalScope/location/{members,setting-members,redirect,
  redirect-module,redirect-sharedworker,worker-separate-file}.html,
WorkerNavigator_appName.htm, WorkerNavigator_appVersion.htm, WorkerNavigator_onLine.htm,
WorkerNavigator_platform.htm, WorkerNavigator_userAgent.htm,
WorkerNavigator_userAgentData.http.html,
interfaces/WorkerUtils/navigator/{002,003,004,005,006,007,language}.html
```

31 файл напрямую, плюс вероятный вклад в соседние кластеры
(`SharedWorker-extendedLifetime*`, `constructors/*`), чьи вспомогательные
скрипты тоже читают `location`/`navigator` попутно.

## Причина

Пример (`support/WorkerLocation.js`, воркерный скрипт теста):

```js
var obj = new Object();
obj.location = location.toString();   // ReferenceError: location is not defined
...
postMessage(obj);                      // никогда не выполняется
```

`worker_global_shim` (`worker.rs:264-390`) объявляет `self`, `name`,
`postMessage`, `onmessage`, `addEventListener`/`removeEventListener`,
`console`, `importScripts`, таймеры, `queueMicrotask` — `location` и
`navigator` в списке нет. `SHARED_WORKER_GLOBAL_SHIM`
(`shared_worker.rs:99-212`) — тот же набор минус `name`/`onmessage`
(есть `onconnect`), тоже без `location`/`navigator`. Оба зовут
`install_worker_scope_globals_v8`/`crate::dom::worker_exposed_shim()`
(`worker.rs:237-249`) для общей части (`EventTarget`, `performance`,
BUG-401) — но и та тоже не заводит `location`/`navigator`
(`grep -rn "location\|navigator" crates/js/src/dom.rs` в области
`worker_exposed_shim` — ноль совпадений).

`sw_worker.rs` — единственная область с реальным `WorkerLocation`
(комментарий на `sw_worker.rs:47`: «`WorkerLocation` целиком, а не два
поля» — сервис-воркеры получают его специально, `scope`-путь оборачивается
в полноценный объект на `sw_worker.rs:456-498`), но `navigator` нет и там —
`grep -n navigator crates/js/src/sw_worker.rs` пуст.

## Почему это не только тестовый шум

`WorkerLocation`/`WorkerNavigator` — часть базового `WorkerGlobalScope`
(HTML LS §10.2.3/§10.1.6), не экзотика: любой воркер, читающий свой origin
для CORS-проверок, строящий абсолютные URL из `location.href`, логирующий
`navigator.userAgent`, или просто копирующий код между окном и воркером
(частый паттерн, ради которого и существует миксин
`WindowOrWorkerGlobalScope`) падает на первой строке. `CAPABILITIES.md`
(секция «Workers/Concurrency») не заявляет ни `location`, ни `navigator` —
дрейфа документации нет, это честно неисследованный пробел, а не
регресс.

## Предлагаемый фикс

- `location`: сконструировать `WorkerLocation`-подобный объект (getter-и
  `href`/`origin`/`protocol`/`host`/`hostname`/`port`/`pathname`/`search`/
  `hash`, `toString()`) из URL воркерного скрипта в момент его создания —
  тем же приёмом, что уже применён в `sw_worker.rs:456` для scope-пути
  сервис-воркера, просто источник берётся другой (сам `url`, а не `scope`).
  Для dedicated-воркера URL уже резолвится в `Worker.prototype`-конструкторе
  (`worker.rs:519` `_url_resolve`) — тот же резолв нужно передать в
  `install_worker_globals_v8`/`worker_global_shim`.
- `navigator`: минимум `WorkerNavigator` (`appName`/`appVersion`/`platform`/
  `userAgent`/`onLine`/`language`) — можно переиспользовать значения,
  которыми уже отвечает `navigator` страницы (`dom.rs`), не дублируя
  константы.

## Перенесено из BUG-814 (слит как дубликат 2026-08-22)

Тот же дефект был заведён повторно 2026-08-21 (WPT-RUN-6, срез 18) как
[BUG-814](BUG-814-DUPLICATE.md), с более узким охватом (dedicated + shared,
без сервис-воркера). Уникальное из него:

**Живая проба** — `tests/wpt/verify_csp_url_worker_gaps.py --variant
worker-navigator` (коммит `41ee56b73`): скрипт воркера постит `typeof`
каждого имени — единственное чтение, которое само не бросает:

```
worker-message data=typeof navigator=undefined self=object
                    location=undefined setTimeout=function
```

`--variant worker-async-postmessage`: воркер вида
`(async () => { postMessage(navigator.platform) })()` — как в
`workers/support/WorkerNavigator.js` — не печатает **ничего**, тогда как
echo-воркер в том же прогоне отвечает нормально. Тишина, а не ошибка, —
следствие [BUG-813](BUG-813-OPEN.md) (исключение из запущенного воркера
наружу не выходит).

**Корпусной счёт.** Механизм `worker-navigator-missing` (`timeout_audit.py`)
забирает **6 id** остатка снимка WPT-RUN-5 — всё семейство
`workers/WorkerNavigator_*`. Оценка снизу: каталог `workers/` в снимке в
основном отработал по `worker-importscripts` ([BUG-778](BUG-778-FIXED.md)),
до чтения `navigator` тесты не доходят. При этом самый дешёвый в починке
пункт среза — объект чисто информационный, вычислять нечего.

**Как проверить фикс** (в дополнение к прогону категории):
`--variant worker-navigator` печатает `typeof navigator=object
location=object`, `--variant worker-async-postmessage` — `async:<platform>,true`.

## Не расследовано в этой сессии

- `WorkerLocation-origin.sub.window.html`, `redirect*` — проверяют смену
  `location` при HTTP-редиректе воркерного скрипта; после появления самого
  объекта могут вскрыть отдельный баг про то, что редирект не отслеживается.
- Влияние на кластер `SharedWorker-extendedLifetime-*`/`constructors/*`
  (обе TIMEOUT-группы в этом же прогоне) не подтверждено построчным чтением
  их support-скриптов — не исключено, что там другая причина.
