# BUG-830 — строковый обработчик `setTimeout("code", 0)` / `setInterval` молча выбрасывается: код не компилируется, таймер не ставится, id всё равно возвращается

**Статус:** OPEN
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
