# BUG-1355 — Краштест `block-in-inline-ax-crash.html` под `wptrunner` растит память `lumen.exe` до 5 ГБ и убивается лимитом `--max-browser-gb 4.0` (CRASH)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 12, `css/CSS2` (normal-flow + margin-padding-clear))
**Область:** shell/js (память `lumen.exe` под `wptrunner`; страница `normal-flow/crashtests/block-in-inline-ax-crash.html`)

## Симптом

`run_corpus.py --prefixes css/CSS2/normal-flow,css/CSS2/margin-padding-clear`: `rss-cap-kills.jsonl` — одна запись, `rss_gb 5.15`, `cap_gb 4.0`; тест — `CRASH`, `browser connection lost … WebSocket`. `--dump-layout` и `--screenshot` на той же странице завершаются за 0,4 с без роста памяти (скрипты страницы там не доходят до цикла: `load` в CLI на нужной фазе не наступает). Страница: `setInterval` без задержки, заменяющий листья текстом `"<TAG>…</TAG>"`, и обработчики `DOMNodeInsertedIntoDocument`/`DOMCharacterDataModified` с `insertBefore`/`appendChild`/`extractContents`: цикл «мутация → событие → мутация», с ограничителем только у одного обработчика (`fired_count >= 20`).

## Как найдено

WPT-RUN-14 срез 12: единственный CRASH среза (crashtest 1 из 2). Вне `wptrunner` не воспроизведён, причина не установлена.

## Что делать

Воспроизвести через BiDi (`browsingContext.navigate` и ожидание `load`), выяснить, какой обработчик крутится; ограничить глубину вложенных мутационных событий; не давать `setInterval` с нулевой задержкой вытеснять рендер.

## Как проверить

`run_corpus.py --prefixes css/CSS2/normal-flow/crashtests` — `rss-cap-kills.jsonl` пуст.
