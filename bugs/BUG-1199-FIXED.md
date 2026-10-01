# BUG-1199 — WPT-страница с `window.open()` не отдаёт результаты харнесса (TIMEOUT всего файла)

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-27 (P1, при закрытии [BUG-585](BUG-585-FIXED.md) / GAP-ORIGIN)
**Область:** shell / wptrunner (`tools/wptrunner/wptrunner/executors/executorlumen.py` —
ожидание `testharnessreport.js`), открытие вкладки по `window.open`

## Симптом

`html/browsers/origin/api/origin-from-window.window.html` и
`origin-from-messageevent.window.html` — `TIMEOUT … Timed out waiting for testharnessreport.js
results`, ни одного сабтеста в отчёте, хотя синхронные сабтесты этих файлов (например
`Origin.from(window) returns a tuple origin.`) не зависят от окон. В обоих файлах есть
`window.open("/html/browsers/windows/resources/post-to-opener.html")`; в логе прогона сразу
после него:

```
Reload: http://localhost:18300/html/browsers/windows/resources/post-to-opener.html
Reload: http://127.0.0.1:18300/html/browsers/windows/resources/post-to-opener.html
```

и до таймаута больше ничего. Файлы того же каталога без `window.open` отрабатывают.

## Гипотеза (подтверждена)

`window.open` открывает вкладку и делает её активной, а исполнитель wptrunner читает
результаты харнесса из активной вкладки — тестовая страница продолжает работать в фоне, но её
`testharnessreport.js` никто не опрашивает. Проверить: минимальная страница с `test()` и
`window.open('about:blank')` под `run_smoke.py`.

## Причина

Гипотеза верна. Шелл исполнял каждую `AutomationCommand` над **активной** вкладкой.
`window.open()` (`Lumen::open_new_tab`) делает попап активным и паркует вкладку теста в
`bg_tabs`; её рантайм там продолжает тикать (GAP-NAVCTX срез 15), и `testharnessreport.js`
кладёт результат на `window` вкладки теста, но `script.evaluate` исполнителя опрашивал
попап — `RESULTS_GLOBAL` там не появлялся никогда.

## Исправление

`Lumen::automation_tab` — стабильный id вкладки, которую загрузил последний
`AutomationCommand::Navigate`/`NewTab` (`crates/shell/src/lumen/state.rs`). В
`crates/shell/src/app/about_to_wait.rs`:

- `Eval` при фоновой вкладке автоматизации (`Lumen::automation_tab_in_background`,
  `TabStrip::inactive_index_of`) исполняется синхронно на её запаркованном
  `PageSnapshot::js_ctx` — тот же вызов, которым per-tick насос уже качает сообщения
  `postMessage` в фоновые вкладки;
- `Navigate` сначала возвращает вкладку автоматизации на передний план (`switch_tab`), иначе
  следующий тест загрузился бы в попап, а опрос шёл бы в старую вкладку.

`Click`/`Type`/`Scroll`/`Screenshot`/`Query` по-прежнему адресуют активную вкладку — граница
записана в [`subsystems/driver.md`](../subsystems/driver.md) §Invariants.

## Проверка

Живой прогон `tests/wpt/run_smoke.py` (dev-release):

| Файл | До | После |
|---|---|---|
| `origin-from-window.window.html` | TIMEOUT, 0 сабтестов | TIMEOUT, 4/7 PASS |
| `origin-from-messageevent.window.html` | TIMEOUT, 0 сабтестов | TIMEOUT, 4/6 PASS |

Теперь TIMEOUT файла выставляет сам харнесс по своим таймаутам, а раннер результаты читает.
Оставшиеся TIMEOUT-сабтесты — дефекты движка, не раннера:

- оба сабтеста с окнами: два `window.open()` подряд не доставляют опенеру ни одного
  сообщения, хотя одиночный попап доставляет (`e.source === w` верно) —
  [BUG-1212](BUG-1212-OPEN.md);
- `returns an opaque origin for a data URL source`: `postMessage` родителю из первого
  синхронного скрипта `data:`-фрейма не доходит (проба: родитель не получил ничего) — по
  симптому это [BUG-1188](BUG-1188-FIXED.md); отложенную отправку из `data:`-фрейма проба не
  проверяла.

Регрессионный тест: `tabs::strip::tests::inactive_index_of_skips_active_and_closed_tabs`.
