# BUG-1647 — reCAPTCHA v3: `grecaptcha.execute` не возвращает токен — «reCAPTCHA Timeout» и `TypeError … (reading 'length')` в `filter`

**Статус:** OPEN
**Заведён:** 2026-10-19 (P2, COMPAT-CORPUS-4-BASE, `scripts/captcha_stand.py --arm lumen`)
**Область:** js (исполнение `google.com/recaptcha/api2/anchor` в iframe, DOM-вызов внутри `Array.prototype.filter`-подобного хелпера)

## Симптом

Страница `https://recaptcha-demo.appspot.com/recaptcha-v3-request-scores.php`, живое окно `--maximized`, маршрут home-direct,
сборка `dev-release` от 2026-10-19. `grecaptcha.ready()` срабатывает, `grecaptcha.execute(site_key, {action:'examples/v3scores'})`
вызван, на странице остаётся «Received token from » с пустым значением, балла нет (оракул `score` — «не_отрисован», 60 с наблюдения).
В v2-вариантах того же вендора (`recaptcha-v2-checkbox`, `-appspot`, `-invisible`) iframe виджета рисуется — эти страницы «пройдено».

stderr (`.tmp/captcha-stand/<прогон>/live.stderr.recaptcha-v3-score.0.log`, 52 строки ошибок):

```
[JS error] Uncaught TypeError: Cannot read properties of undefined (reading 'length')
    at Object.g [as filter] (<anonymous>:571:460)
    at JK.<anonymous> (<anonymous>:399:283)
    at m6 (<anonymous>:1101:279)
    at q (<anonymous>:313:435)
    at G (<anonymous>:561:125)
    at dispatchEvent … at Object._lumen_tick_timers
[unhandled-rejection] Error: reCAPTCHA Timeout (z) / (C)      — по 4 раза
```

После ошибки скрипт шлёт `GET https://www.google.com/recaptcha/api2/jserrorlogging?script=…/recaptcha/(api2|enterprise)/anchor…`
(Google получает отчёт об ошибке из anchor-кадра — по каждому из двух ключей и обоим вариантам скрипта).
Chrome 148 на этой странице получает балл 0,9 (`docs/perf/runs/2026-10-14-captcha-chrome.json`).

## Что сделать

1. Деобфусцированный вызов `filter` в anchor-кадре (`recaptcha__en.js`, строка 571, столбец 460): понять, какой Web API
   возвращает `undefined` вместо коллекции с `length` (кандидаты — `NodeList`/`HTMLCollection`, `getEntries*`, `navigator.plugins`/`mimeTypes`,
   `document.styleSheets`, `getComputedStyle` итератор). Скрипт вызывается из `dispatchEvent` по таймеру (`message`-событие между кадрами).
2. Исправить Web API (по спеке, не под reCAPTCHA) и поставить тест в `crates/js`.
3. Проверка: `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score` → «пройдено», балл v3 в JSON-записи (`v3_score`).
