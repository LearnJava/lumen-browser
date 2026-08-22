# BUG-813 — исключение внутри запущенного воркера не доходит до страницы: `error` на объекте `Worker` диспатчится только при провале загрузки скрипта

**Статус:** OPEN
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

Это тот же класс, что [BUG-716](BUG-716-FIXED.md)/[BUG-591](BUG-591-OPEN.md)
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
причине — `worker-importscripts` ([BUG-778](BUG-778-OPEN.md), 2 946 id),
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
