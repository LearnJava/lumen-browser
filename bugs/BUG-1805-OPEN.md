# BUG-1805 — rutube.ru: нет `navigator.serviceWorker.startMessages` (нестандартный Chrome-API контейнера)

**Статус:** OPEN
**Заведён:** 2026-11-13 (P2, SITES-TRIAGE-3 — разбор зависаний abroad)
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `_sw_container`, рядом `_sw_make_registration`)

## Симптом

`https://rutube.ru/` (живое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, без прокси, прогон
`compat_crawl` 2026-11-13, `.tmp/hang30-rerun/results.jsonl`): обе попытки `TIMEOUT` 41–44 с,
видимый текст 0, у Chrome 17–18 тысяч знаков. В stderr повторяется:

```
[JS error] Uncaught TypeError: navigator.serviceWorker.startMessages is not a function
script error: JS runtime error: navigator.serviceWorker.startMessages is not a function
```

Исключение убивает инициализацию плеера/страницы: headless `--trace-nav` доходит до краски
за 34 с (paint 14,9 с), но `DocumentReady` в живом окне не наступает за 40 с — страница
остаётся без текста.

## Причина

`navigator.serviceWorker.startMessages()` — нестандартный метод контейнера Chrome (включает
доставку `message`-событий от активного service worker до первого подписчика). В шиме
`_sw_container` его нет, как и в `_sw_make_registration`/`_sw_make_worker`; поиск по
`startMessages` по `crates/` пуст. Rutube зовёт его без оговорки — падение фатально для
инициализации.

## Что делать

1. Добавить `startMessages` на `_sw_container` (возврат `undefined`, как у Chrome до
   подписки; DeliveryState «stalled» симулировать не нужно — сейчас сообщений SW нет).
2. Проверить смежные методы контейнера, которые зовут в том же блоке инициализации
   rutube (`getRegistrations`, `ready`) — дозаполнить, если отсутствуют.
3. Перепрогнать rutube живым окном: текст ≥ нескольких тысяч знаков, статус не TIMEOUT.

## Как проверить

`py scripts/compat_crawl.py --corpus <(echo rutube-ru https://rutube.ru/) --env LUMEN_NO_ADBLOCK=1 --restart-every 1 --timeout 40 --chrome-arg=--no-proxy-server` — статус не `TIMEOUT`; в stderr нет `startMessages`.

## Связи

- SITES-TRIAGE-3 (ROADMAP.md:1401), срез SITES-TRIAGE-2 передал rutube косвенно (класс TIMEOUT).
- Регистрации SW: BUG-1226, фрейм `wrapWinFacadeGlobals` — BUG-1743.
