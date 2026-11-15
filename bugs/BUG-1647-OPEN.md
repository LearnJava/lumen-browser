# BUG-1647 — reCAPTCHA v3: `grecaptcha.execute` не возвращает токен — «reCAPTCHA Timeout» и `TypeError … (reading 'length')` в `filter`

**Статус:** OPEN (срез 1 — `TypeError` в anchor-кадре больше не воспроизводится; токен
выдаётся через ~30 с и Google отвечает на него `success:false`)
**Заведён:** 2026-10-19 (P2, COMPAT-CORPUS-4-BASE, `scripts/captcha_stand.py --arm lumen`)
**Область:** js (исполнение `google.com/recaptcha/api2/anchor` в iframe, DOM-вызов внутри `Array.prototype.filter`-подобного хелпера)

## Срез 1 (2026-11-15, P3) — симптом сменился: токен приходит, но невалиден

**Повторный замер.** `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score
--exe target/dev-release/lumen.exe` на `main` `d71c1c09b` (бинарь собран 06:36, кодовых
коммитов после него нет): verdict `не_отрисован`, но **`TypeError … (reading 'length')` в логе
больше нет** — остались только `[unhandled-rejection] Error: reCAPTCHA Timeout (C)` и `(z)`
(по два раза, v3 и enterprise-варианты). Плечо Chrome на тех же страницах/окне —
`пройдено`, балл 0.9.

**Что происходит (`.tmp/cap1647.py`, BiDi, логгер `window.postMessage`/`message`):**

- anchor-кадр грузится (`api2/anchor?…&anchor-ms=20000&execute-ms=30000`), тянет
  `recaptcha__en.js`, `styles__ltr.css`, `logo_48.png` и **`api2/webworker.js`**;
- `grecaptcha.execute(...)` резолвится **через 30,5 с после вызова** (`__t.ok − __t.call`),
  токен короткий (632 символа). Это ровно собственный `execute-ms=30000` из URL anchor'а:
  api.js отдаёт токен после своего таймаута, а не по готовности кадра;
- выданный токен уходит в `recaptcha-v3-verify.php`, и Google отвечает
  `{"success": false, "hostname": "", "challenge_ts": "", "score": null, "action": "", …}` —
  токен невалиден. Страница при этом отрисовывает «Received token from reCAPTCHA service…»
  и «Received response from our backend:», то есть JS-путь самой страницы цел.

**Дифф сети против Chrome** (`.tmp/cap1647_chrome.py` — CDP `Network.*` на том же URL):
Chrome **не** запрашивает `api2/webworker.js` и получает длинный токен (`0cAFcWeA…`, ~1.5 КБ)
за ~1 с; Lumen уходит в worker-ветку сигналов и получает короткий (`HF…`) после таймаута.
Chrome дополнительно делает `POST api2/clr?k=…`; у Lumen запрос `clr` в stderr есть, но метод
не виден — `crates/shell/src/event_sink.rs:27` печатает `→ GET` для любого метода.

**Следующий срез — handshake `api.js ↔ anchor`.** 30-секундное ожидание означает, что
родитель не получает от кадра сигнал готовности/токен вовремя. Для этого нужен логгер,
установленный **до** `api.js`; `script.addPreloadScript` в этой сессии не сработал
([BUG-1811](BUG-1811-FIXED.md)) — починен 2026-11-15 (P3), срез 2 начинается с логгера
`message`/`postMessage` через preload-скрипт.

**Попутно проверено (не причина BUG-1647):**

- `child.postMessage → parent` для **кросс-ориентирного** `<iframe>` доставляется
  (`.tmp/pm_cross.py`: `origin`, `source`, `isTrusted` верные), `load` у обоих кадров есть;
- у **same-origin** `<iframe>` `contentDocument === null`, сообщения в родителя не доходят,
  хотя `load` срабатывает — это семья [BUG-480](BUG-480-OPEN.md), а не этот баг;
- `element.textContent = …` на `<pre>` доходит до `innerText` в статической странице
  (`.tmp/pre_innertext.html`, `--dump-layout`), так что пустые `<pre>` в живой пробе — стейл
  раскладки при чтении `innerText` до принудительного reflow, а не дефект `innerText`.

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
