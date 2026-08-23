# BUG-871 — исключение из слушателя `message` (порт, воркер, окно) проглатывается: ни `window.onerror`, ни `'error'`, ни `worker.onerror`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `listener-exception`)
**Область:** `crates/js/src/dom.rs:11965` и `:11968` — `MessagePort.prototype._deliver` зовёт `_onmessage` и каждого `addEventListener`-слушателя внутри голого `catch(e) {}`; `crates/js/src/dom.rs:10896` — `try { window.onmessage(ev); } catch(e) {}`; доставка сообщения от воркера в `Worker.prototype`-обработчик страницы устроена так же
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Три пути доставки сообщений остались вне починки
[BUG-591](BUG-591-OPEN.md) от 2026-08-22/23: исключение, брошенное из
обработчика `message`, не долетает никуда — ни в `window.onerror`, ни в
слушатель `'error'`, ни (для воркера) в `worker.onerror`, ни на stderr.

## Прямое измерение

`tests/wpt/verify_worker_port_storage_gaps.py --variant listener-exception`
(2026-08-23, dev-release, Linux, `main` = `c14b8068c` — то есть **после**
обоих коммитов BUG-591 от 2026-08-22/23, `--seconds 9`). Страница ставит
`window.onerror` и слушатель `'error'`, затем бросает из трёх разных
обработчиков `message` — порта, воркера и окна:

```
le-port-listener-ran
le-worker-listener-ran
le-window-listener-ran
le-checked
```

Все три обработчика вошли и бросили; ни одного `le-window-onerror`,
`le-error-event` или `le-worker-onerror` за 17 тиков.

Что уже работает и с этим не путать: исключение **внутри** воркера
(в его собственном `onmessage`, в теле скрипта, в его таймере) доходит
до `worker.onerror` — это закрыто BUG-591/BUG-778 и покрыто тестом
`worker_onmessage_handler_exception_fires_parent_onerror`
(`crates/js/src/dom.rs:37751`). Здесь речь о зеркальной половине:
обработчик на **стороне страницы**.

## Почему это важно за пределами самих тестов

`testharness.js` докладывает провал ассерта через слушатель `error`
(`resources/testharness.js:5074`). Значит любой тест, который проверяет
что-либо из `port.onmessage`/`worker.onmessage`/`window.onmessage` —
а для механизмов передачи сообщений это единственное место, где можно
проверить, — не проваливается, а виснет до таймаута. Показательная пара:
[BUG-867](BUG-867-OPEN.md) (у события `connect` неверные `data` и тип)
измерен через `port.onmessage`, и его тест `connect-event.html` стоит в
остатке как TIMEOUT, хотя по существу это два провалившихся ассерта.

## Масштаб

Отдельного кластера id у бага нет — он **скрывает** чужие. Прямо на нём
стоят `workers/constructors/SharedWorker/connect-event.html`,
`workers/Worker-messageport.html`, `webmessaging/with-ports/*`; в целом
это тот же счёт, который BUG-591 ведёт по `swallowed_errors`.

## Направление починки (не предписание)

Заменить три голых `catch(e) {}` на `_lumen_report_exception` — тем же
приёмом, что применён к `_lumen_apply_ready_state`/`_lumen_apply_visibility`
2026-08-23. Осторожность нужна ровно там же, где её уже проявили: ветка
`'error'` внутри `window.dispatchEvent` оставлена голой, чтобы
самоповторяющийся `window.onerror` не рекурсировал в себя.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant listener-exception` — ожидаются три `le-window-onerror`.
2. WPT: `run_report.py --all --root workers/constructors/SharedWorker --recursive`.
