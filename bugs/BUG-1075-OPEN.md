# BUG-1075 — `navigation.navigate('#frag' | '?q')` передаёт оболочке относительный URL без разбора по базе документа: загрузка падает `invalid url: "#frag": relative URL without a base`

**Статус:** OPEN
**Тип:** дефект реализованного кода — шим `navigation.navigate()` отдаёт `url` как есть, оболочка грузит его как абсолютный.
**Заведён:** 2026-09-21 (P2, WPT-RUN-7 срез 45, `navigation-api`)
**Область:** js — `crates/js/src/navigation_api.rs::NAVIGATION_API_SHIM`, `navigate()` (`_lumen_navigation_request(action, url, key, stateJson)`, строка 176: `url` не резолвится против `document.baseURI`); приёмник — оболочка (`crates/shell/src/app/`, обработка очереди Navigation API). Точное место разбора не локализовано.
**Владелец:** P3.

## Симптом

Прогон `navigation-api` (475 id, `--update-expected`, 2026-09-21): **70 id** — harness `ERROR` ещё до первого подтеста, причём это не «API не реализован», а
сбой самой навигации, о котором BiDi докладывает как о неудаче `browsingContext.navigate` самой тестовой страницы. Из 70: **67** — относительный URL
(`relative URL without a base`), 2 — непарсящийся абсолютный (`https://example.com\0mozilla.org`: по спеке `navigate()` обязан синхронно бросить
`SyntaxError` `DOMException`, а не отдавать строку оболочке — тот же корень, шим не разбирает URL), 1 — `file:///` (другая причина, ниже):

```
pid:19584 Reload: #frag
pid:19584 Ошибка загрузки #frag: invalid url: "#frag": relative URL without a base
webdriver.bidi.error.UnknownErrorException: unknown error (navigate: navigation failed: invalid url: "#frag": relative URL without a base)
```

Тесту достаточно вызвать `navigation.navigate('#frag')` (или `'?phase=done'`, `'#1'`, `'#'`) — оболочка получает строку `#frag` без базы, `Reload: #frag`
парсит её как абсолютный URL и валит загрузку. По логу `Reload: <относительный>` встречается 146 раз; значения: `#N`, `#frag`, `#`, `?N`,
`#push`, `#foo`, `?phase=done`, `#second`. Затронуты, в частности, `navigation-api/state/*`, `navigate-event/*`, `navigation-methods/*`.

Тот же класс в `--check`: harness `ERROR` этих файлов записан в baseline как нижняя планка, регрессировать некуда.

## Ожидание

Аргумент `navigate()` разбирается как URL относительно `document.baseURI` (Navigation API §navigate → «parse a URL» против relevant global object's document);
`navigation.navigate('#frag')` — навигация в пределах документа (fragment navigation, `hashchange`, новая запись), а не загрузка.

## Не проверялось

- Работает ли тот же путь через `NavigateEvent.intercept()` после исправления: 70 id — верхняя граница числа, которое исправление сдвинет; часть из них
  упрётся дальше в BUG-639 (`updateCurrentEntry`, `NavigationDestination`).
- `navigation.reload()` (метода в шиме нет вовсе — [BUG-639](BUG-639-OPEN.md)).
- Отдельно в том же прогоне: `navigation.navigate('file:///')` — `network error: file: not a local path` (1 id, `navigate-file-url.html`) — другая причина.

## Связанное

- [BUG-639](BUG-639-OPEN.md) — остальные пробелы Navigation API; этот дефект их маскирует (тест не доходит до подтестов).
