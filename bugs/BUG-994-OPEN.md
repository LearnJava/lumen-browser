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

## Живое усиление (P3 2026-09-05, при разборе BUG-993)

Диагностика [BUG-993](BUG-993-FIXED.md) (снимок vs эвристика `broken_render`)
вскрыла на живом `www.cnbc.com` (React-подобный SPA) намного более дорогой
случай того же класса, чем «косметическая ошибка консоли»:

- Через ~40–50с после навигации в консоли появляется необработанный
  `TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))`
  — ровно та форма ошибки, которую V8 даёт на `for...of`/spread над объектом
  без `Symbol.iterator`, чего этому файлу как раз не хватает `NodeList`.
- В ту же секунду вся отрисованная страница обрушивается: `resource://layout`
  падает с 2062 боксов (реальный контент, ~491 КБ снимок) до 57 боксов нулевого
  размера (почти все — голые `<script>`), снимок возвращается к пустому кадру.
- Воспроизведено дважды, и **воспроизведено с `LUMEN_NO_ADBLOCK=1`** — то есть
  не объясняется реакцией сайта на встроенный блокировщик рекламы.
- Похоже на штатное поведение React без Error Boundary («необработанная ошибка
  рендера убирает с экрана всё приложение, а не только сломанный компонент») —
  то есть цена этого класса дефектов не «сайт печатает ошибку в консоль», а
  «страница белеет насмерть посреди сессии».

Конкретный вызывающий код внутри минифицированного бандла cnbc не
локализован (нет sourcemap, нет времени трассировать вручную) — это **не
подтверждённый факт «это тот же баг»**, а сильная косвенная улика (сигнатура
ошибки совпадает с одним из явно недостающих кусков спеки этого файла).
Стоит держать в уме при фиксе: после починки `Symbol.iterator`/`item()` стоит
повторно прогнать `cnbc.com` живым окном и убедиться, что коллапс исчез —
это самый дешёвый способ подтвердить или опровергнуть связь.
