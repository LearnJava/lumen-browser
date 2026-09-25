# BUG-1176 — quora: управляемый челлендж Cloudflare падает на `(0,eval)(…)` и не проходит

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, по ходу закрытия [BUG-493](BUG-493-FIXED.md); видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`, Chrome 153 тем же способом).
**Область:** js — не локализовано. Падает оркестратор челленджа
(`https://www.quora.com/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=…`, 3 строки,
~230 КБ, обфусцирован) на тике таймера (`_lumen_tick_timers`).

## Симптом

`https://www.quora.com/` → `403` + страница «Just a moment...». Дальше в Lumen по кругу:

```
→ GET …/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=…   ← 200
[JS error] [Cloudflare Turnstile] Unhandled error: Cannot read properties of null (reading 'eval') , with debug info: …
[JS error] Uncaught TypeError: Cannot read properties of null (reading 'eval')
    at wE.dh (eval at _lumen_script_execute_classic …, <anonymous>:1:98664)
    at wE.run (…:1:59443)
    …
    at _lumen_tick_timers (…)
→ GET …/cdn-cgi/challenge-platform/h/b/eb/…/managed                      ← 200
→ GET https://challenges.cloudflare.com/turnstile/v0/b/…/api.js?onload=…&render=explicit
→ GET https://www.quora.com/                                              ← 403
```

— и так четыре цикла за 20 с. Документ так и остаётся челленджем: 46 узлов, `title`
«Just a moment...». Chrome 153 с чистым профилем проходит челлендж без участия пользователя:
приложение quora, 178 узлов, 0 ошибок в консоли.

## Что известно

- Оркестратор содержит `(0,eval)(…)` (непрямой `eval`) и `document.createElement`; строк `iframe`
  и `contentWindow` в нём нет, они собираются динамически.
- Вставленный скриптом `<iframe>` в Lumen **синхронно** после `appendChild` не имеет
  `contentWindow` (`null`), `contentDocument === null`; в Chrome — `object`, и
  `iframe.contentWindow.eval('1+1') === 2` (проба `.tmp/b493/ifr.js` на `http://127.0.0.1`,
  в слоте `p6-work`). Это известный остаток [BUG-480](BUG-480-OPEN.md) §Реальные сайты
  (samsung, w3schools). `null.eval` — самый вероятный кандидат, но **не подтверждён**: стек указывает
  в обфусцированный код, его соответствие `iframe.contentWindow.eval` не установлено.

## Что сделать

1. Локализовать: перехватить `HTMLIFrameElement.prototype.contentWindow`
   (и `window.frames`, `document.defaultView` у фреймового документа) до скриптов челленджа,
   записать, где впервые возвращается `null`; сравнить с Chrome.
2. Если это BUG-480 — поднять синхронный `about:blank`-контекст у вставленного фрейма (или
   переадресовать к владельцу дорожки FRAME), иначе чинить найденное.

Критерий: `https://www.quora.com/` в Lumen доходит до приложения (десятки узлов → сотни,
`title` не «Just a moment...»), `reading 'eval'` в stderr нет.
