# BUG-994 — у коллекций DOM нет `item()`: `document.getElementsByTagName(…).item is not a function`

**Статус:** OPEN
**Заведён:** 2026-09-04 (живой прогон корпуса «топ-100 зарубежных»)
**Область:** `crates/js/src/dom.rs` — фабрики коллекций (`getElementsByTagName` / `getElementsByClassName` / `querySelectorAll` / `children`)
**Владелец:** P3

## Симптом

```
Uncaught TypeError: document.getElementsByTagName(...).item is not a function
Uncaught TypeError: a.item is not a function
Uncaught TypeError: t.item is not a function
```

Коллекция ведёт себя как массив: индексный доступ и `length` есть, а методов
интерфейса нет.

## Спека

DOM §4.2.10: `HTMLCollection` обязан иметь `length`, `item(index)` и
`namedItem(name)`; `NodeList` (DOM §4.2.10.1) — `length`, `item(index)`,
`entries()`/`keys()`/`values()`/`forEach()` и `Symbol.iterator`. Ни то, ни другое
не является `Array`, поэтому сайты зовут `item()`, а не `[]`.

## Что говорит измерение

Прогон 100 сайтов 2026-09-04: `a.item is not a function` — **первая** ошибка
консоли на `chatgpt.com` и `reddit.com`; на `airbnb.com` тот же дефект в трёх
формах (`document.getElementsByTagName(...).item`, `t.item`, плюс сопутствующие).
Три сайта разными библиотеками, то есть приём распространённый, а не экзотика.

## Класс дефекта

Тот же, что [BUG-715](BUG-715-OPEN.md) (`DOMTokenList`/`CSSStyleDeclaration`
собраны ad-hoc литералами вместо интерфейсов) и
[BUG-694](BUG-694-OPEN.md) (`URLSearchParams` без `Symbol.iterator`): объект
похож на нужный интерфейс по форме, но не по поведению. Чинить стоит вместе —
корень один, фабрики литералов.

## Объём

Проверить все точки, отдающие коллекцию: `getElementsByTagName`,
`getElementsByClassName`, `getElementsByName`, `querySelectorAll`,
`children`, `childNodes`, `document.images`/`forms`/`scripts`/`links`
(последние три — [BUG-892](BUG-892-OPEN.md), `undefined`). На каждой — `item()`,
`namedItem()` там, где требует спека, и `Symbol.iterator` у `NodeList`.

## Сырые данные

`.tmp/perf-audit/20260904-150604/results.json` (slug `chatgpt`, `reddit`,
`airbnb`), `health.log`.
