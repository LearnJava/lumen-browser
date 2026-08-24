# BUG-813 — исключение внутри запущенного воркера не доходит до страницы: `error` на объекте `Worker` диспатчится только при провале загрузки скрипта

**Статус:** FIXED 2026-08-24 (P1)
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 18 — 21 TIMEOUT остатка, механизм `worker-no-error-event`)
**Область:** `crates/js/src/worker.rs:533-552` (единственное место, откуда зовутся `_onerror`/`_errorListeners` — ветка `script === null`), `crates/js/src/worker.rs:386-393` (`_lumen_flush_timers` глушит исключения колбэков), `crates/js/src/worker.rs:834` (Rust-цикл доставки сообщений)
**Владелец:** P1/P3 (движок, `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Страница заставляет воркер бросить и ждёт `worker.onerror` — по HTML LS
§10.2.6 «runtime script errors» ошибка должна прийти на объект `Worker`
как `ErrorEvent`. Не приходит ничего:

```js
// workers/Worker_ErrorEvent_*.htm, сокращённо
var w = new Worker("support/throw.js");     // скрипт: onmessage = () => { throw new Error("x"); }
w.onerror = t.step_func_done(e => assert_equals(e.message, "x"));
w.postMessage("boom");                       // ← дальше тишина, до таймаута враннера
```

То же и для «внутреннего» `onerror` воркера, который по спеке может
вернуть `false`, чтобы ошибка пошла дальше на страницу.

## Прямое измерение

`tests/wpt/verify_csp_url_worker_gaps.py` (2026-08-21, коммит `41ee56b73`,
`--seconds 6`; страница жива все 11 тиков):

| проба | ожидалось | получено |
|---|---|---|
| `worker-postmessage` — обычный echo-воркер | `ready`, `echo:hi` | `ready`, `echo:hi` ✔ (доставка сообщений работает) |
| `worker-throw` — `onmessage` бросает, страница ждёт `onerror` и слушателя `error` | `worker-error message=…` | **ничего** |
| `worker-onerror-inside` — воркер сам ловит, постит детали и возвращает `false` | `inner-onerror:…` + `worker-error` | **ничего** |

Контроль в первой строке существен: сообщения между страницей и воркером
ходят в обе стороны, значит дефект узкий — не «воркеры не работают», а
«ошибка воркера никуда не идёт». Третья строка показывает, что и сам
`onerror` внутри воркера не вызывается: `postMessage` из него не пришёл.

## Причина (локализована чтением кода)

В шиме `Worker` (`worker.rs`) поля `_onerror` и `_errorListeners`
заполняются нормально (`worker.rs:584-606`), но читаются ровно из одного
места — `worker.rs:541-549`, внутри ветки `if (script === null)`, то есть
когда провалилась *загрузка* скрипта воркера
([BUG-364](BUG-364-FIXED.md), путь HTML LS §10.2.6.1 «run a worker»).
Для уже запущенного воркера пути наверх нет вовсе:

* `_lumen_flush_timers` (`worker.rs:386-393`) выполняет колбэки в
  `try { … } catch(e) {}` — исключение таймера гасится на месте;
* Rust-цикл доставки сообщений (`worker.rs:834`) не переносит исключение
  из контекста воркера в контекст страницы;
* `WorkerGlobalScope.onerror` в шиме воркера
  (`worker_global_shim`, `worker.rs:270-397`) не определён ни разу.

Это тот же класс, что [BUG-716](BUG-716-FIXED.md)/[BUG-591](BUG-591-FIXED.md)
на главном потоке (исключение не становится событием), но отдельный дефект:
там нет моста `TryCatch` → `error` на `window`, здесь — моста «контекст
воркера → `ErrorEvent` на объекте `Worker`».

## Масштаб

Механизм `worker-no-error-event` забирает **21 id** остатка снимка
WPT-RUN-5 (`workers/modules` 4, `content-security-policy/inside-worker` 2,
семейство `workers/WorkerGlobalScope_ErrorEvent_*` и
`workers/SharedWorker-script-error.html` — по 1). Счёт консервативен:
маркер требует обеих половин (`new Worker(` **и** слушатель `error`), а
большая часть каталога `workers/` в снимке отработала по более ранней
причине — `worker-importscripts` ([BUG-778](BUG-778-FIXED.md), 2 946 id),
потому что `testharness.js` внутри воркера подключается через
`importScripts`.

Практическое следствие вне WPT: приложение, которое считает воркер
упавшим по `worker.onerror`, на Lumen не узнает об отказе никогда —
воркер просто перестаёт отвечать.

## Направление починки (не предписание)

Ловить исключение там, где выполняется код воркера (диспетчер сообщений и
`_lumen_flush_timers`), и переносить `message`/`filename`/`lineno`/`colno`
в `ErrorEvent` на объекте `Worker` через существующий реестр
`_workerRegistry`. Порядок по спеке: сначала `WorkerGlobalScope.onerror`
внутри воркера (его тоже нужно завести), и только если он не вернул
`false` — «error event» на объекте `Worker`. Замена `catch(e) {}` на
передачу наружу — минимальный первый шаг, который сам по себе делает
поведение наблюдаемым.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_csp_url_worker_gaps.py
   --variant worker-throw` печатает `worker-error message=boom`.
2. `--variant worker-onerror-inside` печатает сначала `inner-onerror:…`,
   затем `worker-error`.
3. WPT: `run_report.py --all --root workers --recursive` — семейство
   `WorkerGlobalScope_ErrorEvent_*` перестаёт висеть.

## Фикс (2026-08-24, P1)

Порядок из HTML LS §8.1.3.6 → §10.2.6 заведён целиком: исключение сперва
становится событием в **собственной области видимости воркера**, и только
если её обработчик не отменил его — уходит владеющему объекту `Worker`.

* `worker_global_shim` (`crates/js/src/worker.rs`) и
  `SHARED_WORKER_GLOBAL_SHIM` (`shared_worker.rs`) получили `onerror`
  (OnErrorEventHandler: пять legacy-аргументов, отмена по `return true`) и
  `addEventListener('error')` с настоящим `ErrorEvent` и `preventDefault()`.
* `WORKER_ERROR_EVENT_SHIM` — локальный `ErrorEvent` для области воркера.
  Страничная иерархия `Event` живёт в `WEB_API_SHIM_MID`, который тянет за
  собой `document`/`window`; выносить её отдельно — задача крупнее этого
  бага. Следствие, которое надо знать: внутри воркера
  `errorEvent instanceof Event` — `false`.
* `V8JsRuntime::eval_and_report_via` / `eval_module_at_and_report_via`: имя
  репортёра стало параметром, потому что у воркера нет
  `_lumen_report_exception`. Заодно два скопированных блока разбора
  `v8::Message` в `eval_and_report` свёрнуты в тот же макрос
  (`report_exception_via!`), что уже обслуживал модульный путь.
* Верхнеуровневый скрипт обоих видов воркера идёт через эти варианты, а не
  через `try`/`catch` вокруг тела: так сохраняются настоящее брошенное
  значение и структурная позиция из `v8::Message`, а верхнеуровневые
  `let`/`const`/объявления функций остаются в глобальной области. Это же
  условие требуется двум сабтестам
  `shared-worker-runtime-error-is-not-parse-error.html`, где `onerror`
  назначается строкой выше бросающего `eval('1 + ;')`.
* Rust-фоллбэк остаётся только для провала **загрузки** модуля, который до
  JS-репортёра не доходит вовсе; отличаются они по флагу, который ставит
  сам репортёр.
* Защита от рекурсии: исключение, брошенное самим обработчиком ошибки, не
  вызывает `error` повторно, но и не глотается (форма `catch(e) {}`, за
  которой охотился [BUG-591](BUG-591-FIXED.md)) — уходит наверх напрямую.
* `filename` подставляется из `location.href`, когда V8 отдаёт
  `<anonymous>`: классический скрипт воркера компилируется без имени
  ресурса, а `Worker_ErrorEvent_filename.htm` ждёт абсолютный URL воркера.

Три вещи вскрылись на страничной стороне и починены здесь же — до фикса их
нельзя было увидеть, потому что до страницы не доходило ни одного события:
`Object.prototype.toString` у `ErrorEvent` отвечал `[object Object]`
(`assert_class_string` читает именно его), конструктор вызывался без `new`,
и у `Worker` не было `dispatchEvent` — хотя `Worker` это `AbstractWorker`,
то есть `EventTarget`.

### Замер

Живая проба (`verify_csp_url_worker_gaps.py`, dev-release, 8 с) — все три
строки таблицы «Прямое измерение» выше перевернулись:

| проба | было | стало |
|---|---|---|
| `worker-throw` | ничего | `worker-error message=boom` + `worker-error-listener` |
| `worker-onerror-inside` | ничего | `worker-message data=inner-onerror:boom`, затем `worker-error message=boom` |
| `worker-postmessage` (контроль) | ready/echo | без изменений |

WPT (`run_smoke.py`, 11 файлов семейства + `Worker_dispatchEvent_ErrorEvent`):
**17/17 сабтестов, 12/12 файлов OK** — `Worker_ErrorEvent_{message,filename,
lineno,type,bubbles_cancelable,error}`, `WorkerGlobalScope_ErrorEvent_{message,
filename,lineno,colno}`, `Worker_dispatchEvent_ErrorEvent`. Плюс 12 новых
юнит-тестов, включая два сквозных через реальный поток воркера.

Гонять семейство приходится `run_smoke.py` (`run_report.py` не видит `.htm`)
и с временно занулённым портом `wss` в `tests/wpt/config.json` — иначе
запуск падает на `ssl.wrap_socket` ещё до первого теста (готча CLAUDE.md);
в этом слоте нет `tests/wpt/.venv`, а системный `pywebsocket3` лежит вне
рабочей границы проекта и патчить его нельзя.

### Что осталось за рамками

`SharedWorker` **не должен** получать `error` за runtime-ошибку — только за
провал загрузки скрипта (HTML LS §10.2.6, `SharedWorker-script-error.html`,
`SharedWorker-exception-propagation.html`). Рассылка клиентам заведена
[BUG-591](BUG-591-FIXED.md) и этим фиксом не тронута; выделена в
[BUG-905](BUG-905-OPEN.md), потому что это отдельное правило
распространения, а не проглоченное исключение, и корректная починка требует
развести фазу компиляции и фазу исполнения в `eval_and_report_via`.

Таймеры воркера по-прежнему выполняются только при доставке сообщения
([BUG-815](BUG-815-FIXED.md)) — это соседний дефект, здесь не трогался;
закрыт отдельно 2026-08-24.
