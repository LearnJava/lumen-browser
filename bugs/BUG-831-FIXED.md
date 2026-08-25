# BUG-831 — строковый обработчик `setTimeout("code", 0)` / `setInterval` молча выбрасывается: код не компилируется, таймер не ставится, id всё равно возвращается

**Статус:** FIXED 2026-08-25 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, есть маркер `timer-string-handler`)
**Область:** `crates/js/src/dom.rs:6809` (`setTimeout`), `crates/js/src/dom.rs:6827` (`setInterval`), `crates/js/src/dom.rs:6847` (третий таймер той же формы)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
setTimeout("console.log('ran')", 50);          // никогда не выполнится
var id = setInterval("console.log('tick')", 100);  // тоже, id при этом валидный
setTimeout(function () { console.log('fn'); }, 50); // контроль: выполняется
```

Ни исключения, ни сообщения в stderr. Возвращённый id ведёт себя как
настоящий (его принимает `clearTimeout`), поэтому со стороны страницы
происходящее неотличимо от таймера, который просто «ещё не сработал».

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py --variant settimeout-string`
(2026-08-22, dev-release, Linux, коммит `762a0cad9`, `--seconds 6`,
страница жива — 11 тиков):

| ожидалось | получено |
|---|---|
| `string-timeout-ran` + `string-interval-ran` | `armed`, `fn-timeout-ran` — и всё |

То есть функциональный обработчик на той же странице срабатывает, а оба
строковых — нет.

Контроль в другую сторону: `--variant import-eval` показывает, что сама
компиляция строки движку доступна (`eval("import('./m.js')…")` работает,
`import-eval value=1`); отваливается именно путь таймера.

## Причина (локализована чтением кода)

```js
function setTimeout(fn, delay) {
    if (typeof fn !== 'function') return 0;   // dom.rs:6810
```

`setInterval` (`:6828`) начинается той же строкой. HTML LS §8.6
(«timer initialization steps») требует обратного: если handler — не
`Function`, он трактуется как строка и компилируется как классический скрипт
с базовым URL документа. Вместо этого шим возвращает `0` — то есть выдаёт
за id значение, которое спека резервирует под «таймер не создан».

## Масштаб

Маркер `timer-string-handler` в `tests/wpt/timeout_audit.py` — **10 id**
остатка WPT-RUN-5. Восемь из них — семейство
`html/semantics/scripting-1/the-script-element/module/dynamic-import/string-compilation-*`,
которое прогоняет один и тот же `import()` через таблицу «evaluator»-ов
(`eval`, `the Function constructor`, `setTimeout`, инлайн-обработчик).
Остальные — `html/webappapis/timers/evil-spec-example.any.html`,
`navigation-timing/nav2-test-navigate-within-document.html`,
`content-security-policy/reporting/report-multiple-violations-02.html`,
`trusted-types/block-string-assignment-to-Window-setTimeout-setInterval.html`.

Все они TIMEOUT, а не FAIL, по двум причинам сразу: `promise_test`-ы
выполняются последовательно, поэтому один мёртвый evaluator забирает весь
файл, и исключения нет вообще — ронять нечего.

## Направление починки (не предписание)

В обеих функциях заменить ранний `return 0` на компиляцию строки: собрать
обработчик через `Function(String(fn))` (или через тот же путь, которым
шим исполняет инлайн-обработчики атрибутов — он рабочий, см. `--variant
import-inline-handler`) и положить его в очередь таймеров. Спека требует
компилировать в момент срабатывания, а не в момент постановки; для
`setInterval` — на каждом срабатывании заново.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant settimeout-string` — ожидаются `string-timeout-ran` и
   `string-interval-ran`.
2. WPT: `run_report.py --all --root html/semantics/scripting-1/the-script-element/module/dynamic-import --recursive`
   — семейство `string-compilation-*` должно перестать быть TIMEOUT.

## Перезамер 2026-08-23 (WPT-RUN-6, срез 27): цена в id, доказанная сервером

`tests/wpt/verify_callback_import_preload_gaps.py --variant string-import-labels`
на `main` = `34cbefd25` повторяет ровно то, что делает
`html/semantics/scripting-1/the-script-element/module/dynamic-import/string-compilation-base-url-inline-*.html`:
пять «вычислителей» компилируют строку с `import()`, каждый со своей меткой в
запросе, так что видно, кто из них отработал:

```
[server saw: GET /vcip-imports-a.js?label=Function,
             GET /vcip-imports-a.js?label=clicked,
             GET /vcip-imports-a.js?label=eval,
             GET /vcip-imports-a.js?label=reflected]
```

`?label=setTimeout` отсутствует: строковый обработчик таймера не
компилируется, промис первого `promise_test` не оседает никогда, а
`promise_test`-ы идут последовательно — поэтому оба файла (`-inline-classic`
и `-inline-module`) уходят в TIMEOUT целиком, хотя четыре из пяти их
сабтестов движку по силам. Это самое частое имя зависшего сабтеста во всём
остатке WPT-RUN-5.

## Починено P1 2026-08-25

Ранний `return 0` заменён на компиляцию строки — в обоих шимах сразу, потому
что дыра одна и та же в двух местах: страничный `WEB_API_SHIM`
(`crates/js/src/dom.rs`, `setTimeout`/`setInterval`) и воркерный
`WORKER_TIMERS_SHIM` (`crates/js/src/worker.rs`, общий `_schedule` обеих
флейворов). §8.6 — это `WindowOrWorkerGlobalScope`, а не окно, и
CLAUDE.md уже фиксировал воркерную половину как «дропается ровно так же».

**Чем компилируется — и почему не `Function(src)`.** Спека требует
*классический скрипт*, то есть глобальную область: `setTimeout('var x = 1')`
обязан создать глобальную `x`. `new Function(src)` даёт функциональную
область и это объявление теряет, а прямой `eval(src)` внутри шима выполнил бы
код в области самой `setTimeout`. Обе беды снимает **косвенный** eval —
`(0, eval)(src)`: он по определению вычисляет в глобальной области и в
sloppy-режиме, ровно как классический скрипт. Юнит-тест
`bug831_string_timeout_runs_in_global_scope` держит именно это различие, а не
факт срабатывания.

**Когда компилируется.** Строка запоминается замыканием и компилируется в
момент *срабатывания*, а не постановки — то есть у `setInterval` заново на
каждом тике, как требует §8.6. Наблюдаемое следствие проверяет
`bug831_string_handler_compiles_at_fire_not_at_schedule`: синтаксически
битая строка ставится в очередь без ошибки, а её падение остаётся внутри
цикла таймеров (репортится как исключение колбэка, BUG-591) и не роняет
остальные таймеры того же тика.

**Что ещё изменилось на границе.** Возвращаемый id теперь настоящий: раньше
отдавался литеральный `0`, который спека резервирует под «таймер не создан»,
и `clearTimeout(0)` при этом ничего не отменял. Хвостовые аргументы
строковому обработчику не передаются (§8.6 шаг 8 отдаёт их только
Function-обработчику), поэтому в воркерном `_schedule` список `args`
обнуляется перед постановкой — иначе `apply` передал бы их в никуда.

**Замеры (dev-release, Windows, 2026-08-25).**

1. `verify_navigation_form_import_gaps.py --variant settimeout-string`:
   `armed, script-start, string-timeout-ran, string-interval-ran,
   fn-timeout-ran` — оба строковых маркера пришли, при живой странице
   (5 тиков). До починки их не было ни одного.
2. `verify_callback_import_preload_gaps.py --variant string-import-labels`
   (та же проба, которой срез 27 доказывал цену через сервер):
   `[server saw: GET /vcip-imports-a.js?label=Function, ?label=clicked,
   ?label=eval, ?label=reflected, ?label=setTimeout]` — недостающая пятая
   метка на месте, то есть `import()` внутри строкового обработчика ещё и
   резолвится относительно базового URL документа, а не теряется.
3. Юнит-тесты: 5 на странице (`bug831_*` в `dom.rs`) и 2 в воркере
   (`v8_worker_string_*` в `worker.rs`).

**Не входило.** `requestAnimationFrame` тем же ранним `return 0` отвечает на
не-функцию — но там WebIDL-колбэк, и правильный ответ — `TypeError`, а не
компиляция строки, это отдельный дефект. Страничный `setTimeout` по-прежнему
теряет хвостовые аргументы у *функционального* обработчика ([BUG-908](BUG-908-OPEN.md)) —
воркерный их передаёт с BUG-815. Стаб `setTimeout` в scope сервис-воркера
(`sw_worker.rs`, синхронный вызов `fn()`) не тронут: строка там бросит
`TypeError`, но это Phase-1-заглушка целиком.
