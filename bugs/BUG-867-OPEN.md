# BUG-867 — событие `connect` у `SharedWorkerGlobalScope` не `MessageEvent` и не несёт `data === ''`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `sw-connect`)
**Область:** `crates/js/src/shared_worker.rs`, `SHARED_WORKER_GLOBAL_SHIM` — объект, передаваемый в `onconnect`, собирается литералом с одним полем `ports`; конструктора `MessageEvent` в воркерной области нет, `data`/`origin`/`source`/`lastEventId` не заполняются
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

По HTML LS §10.2.1 «shared worker» событие `connect` — настоящий
`MessageEvent` с `data` = пустая строка, `origin` = origin документа,
`source` = порт, `ports` = `[port]`. Здесь работает только `ports`:

```js
self.onconnect = function (e) {
    e.data === ''                  // false — undefined
    e instanceof MessageEvent      // false — MessageEvent в воркере не определён
    e.ports.length === 1           // true
};
```

## Прямое измерение

`tests/wpt/verify_worker_port_storage_gaps.py --variant sw-connect`
(2026-08-23, dev-release, Linux, `main` = `c14b8068c`, `--seconds 6`).
Воркер отвечает по `e.ports[0]` тройкой ровно тех проверок, которые
делает `workers/constructors/SharedWorker/connect-event.html`:

```
swc-constructed port=object onerror-settable=true
swc-message [false,false,true]
swc-pinged
swc-message ["pong","ping"]
```

Третий флаг — `true`, то есть порт приходит и раунд-трип в обе стороны
работает; ломаются только форма события и его `data`.

## Почему это TIMEOUT, а не FAIL

`connect-event.html` проверяет все три флага внутри
`t.step_func_done(...)`, вызванного из `port.onmessage`. Исключение
провалившегося `assert_true` там **проглатывается** — доставка сообщения
порта идёт через `MessagePort.prototype._deliver`
(`crates/js/src/dom.rs:11965`), где вызов слушателя обёрнут в голый
`catch(e) {}` ([BUG-871](BUG-871-OPEN.md)). Поэтому `done()` не
достигается и файл висит до таймаута вместо честного провала двух
ассертов. Пара показательная: пока BUG-871 не закрыт, любой дефект,
который тест проверяет из обработчика порта, читается в отчёте как
зависание.

## Масштаб

1 id остатка снимка WPT-RUN-5 (`workers/constructors/SharedWorker/
connect-event.html`), но форма события — предусловие для любого теста,
который читает `event.source`/`event.origin` в `onconnect`.

## Направление починки (не предписание)

Собирать событие тем же путём, что и обычный `MessageEvent` страницы
(`data: ''`, `origin`, `source: port`, `ports: [port]`), и внести
`MessageEvent` в воркерную область — она уже получает общий
`EVENT_TARGET_SHIM`/`PERFORMANCE_SHIM` через
`worker::install_worker_scope_globals_v8` (BUG-401), так что место для
общего фрагмента есть.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant sw-connect` — ожидается `swc-message [true,true,true]`.
2. WPT: `run_report.py --all --root workers/constructors/SharedWorker --recursive`.
