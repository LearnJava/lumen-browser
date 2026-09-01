# BUG-883 — `window.open()` (и клик по `<a target=_blank>`) убивает документ-вызыватель: ни один его таймер больше не срабатывает

**Статус:** OPEN (ДОРАБОТКА → [GAP-NAVCTX](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-NAVCTX` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 28 — живой замер, вариант `win-open-freeze`)
**Область:** shell (`crates/shell/src/main.rs` — дренаж popup-запросов `_lumen_window_open`, `open_new_tab()`), js (`crates/js/src/dom.rs:12190-12220` — заглушка, которую `window.open` возвращает)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

После `window.open(url)` документ, который её вызвал, перестаёт исполняться
целиком: ни `setInterval`, ни `setTimeout`, поставленные **до** вызова, больше
не срабатывают, `visibilitychange` не приходит, `document.visibilityState`
остаётся `"visible"` — страница не «уходит в фон», она просто больше никогда
не получает управления. Открываемый документ при этом **загружается** (запрос
виден на сервере пробы), исполняет свои скрипты и логирует в тот же stderr.

Это отдельный барьер, стоящий **раньше** [BUG-797](BUG-797-OPEN.md) (заглушка
`window.open` без реального `opener`/`postMessage`): даже если бы канал был,
вызывающая сторона уже мертва и ответ услышать некому.

Тот же эффект даёт клик по `<a target="_blank">` (вариант
`win-anchor-target`): ноль тиков у страницы-инициатора, документ по ссылке
загружен.

## Прямое измерение

`tests/wpt/verify_window_history_jsurl_gaps.py --variant win-open-freeze`
(2026-08-23, dev-release, Linux, `main` = `0dc60692d`). Страница бьётся
таймером каждые 400 мс, на 2-й секунде вызывает `open()`, и держит два
одиночных таймера на 3,5 с и 6 с:

```
beat 1 vis=visible hidden=false
beat 2 … beat 3 … beat 4 … beat 5
opening at beat 5
opened w=object beat=5
child-ran search=?from=freeze opener=null parent-is-self=true name=undefined
[server saw: GET /vwjh-child.html?from=freeze]
```

Дальше — тишина: ни `beat 6`, ни `post-open-timer` (3,5 с), ни `late-timer`
(6 с) за оставшиеся 8 секунд. Контроль — вариант `control` того же прогона
даёт 14 тиков за 8 с, вариант `unload-nav` показывает, что обычная навигация
`location.href = …` ведёт себя так же (документ заменяется), то есть
`open()` обрабатывается как навигация **текущего** окна, а не как создание
вспомогательного контекста.

Смежные измерения того же прогона (вариант `win-open`/`win-open-detail`):
возвращаемая заглушка — обычный объект (`typeof w === "object"`,
`w === window` → `false`, `w.name` = переданное имя, `w.location.href` =
`about:blank` для `open()` без аргументов, `w.document` — `undefined`), а у
самого окна `window.closed` и `window.name` — `undefined`
([BUG-887](BUG-887-OPEN.md)). В открытом документе `window.opener === null` и
`window.parent === window`.

## Цена по WPT

Семь id остатка WPT-RUN-5, у которых ожидание стоит сразу за `open()`:

`html/browsers/browsing-the-web/navigating-across-documents/cross-origin-top-navigation-with-user-activation.window.html`,
`…-with-user-activation-in-parent.window.html`,
`…-without-user-activation.window.html`,
`…-without-user-activation-nested.window.html`,
`…/same-origin-top-navigation-without-user-activation.window.html`,
`html/browsers/windows/noreferrer-null-opener.html`,
`html/browsers/browsing-the-web/unloading-documents/prompt-and-unload-script-closeable.html`
(последний — вместе с [BUG-887](BUG-887-OPEN.md)).

Нижняя граница: всё семейство `RemoteContext`/`common/dispatcher` строится на
`open()`, и по BUG-797 оно уже числится за отдельным механизмом.

## Что дальше

HTML LS §7.2.2 «window open steps» создаёт **новый** browsing context;
исходный документ остаётся полностью живым и активным. Чинить вместе с
BUG-797: пока `open()` реализована как навигация текущего окна, ни канал, ни
`opener` смысла не имеют.
