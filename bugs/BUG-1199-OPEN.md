# BUG-1199 — WPT-страница с `window.open()` не отдаёт результаты харнесса (TIMEOUT всего файла)

**Статус:** OPEN
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

## Гипотеза (не проверена)

`window.open` открывает вкладку и делает её активной, а исполнитель wptrunner читает
результаты харнесса из активной вкладки — тестовая страница продолжает работать в фоне, но её
`testharnessreport.js` никто не опрашивает. Проверить: минимальная страница с `test()` и
`window.open('about:blank')` под `run_smoke.py`.
