# BUG-467: `document.fonts.ready` (FontFaceSet.ready) not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6770` — `_lumen_get_fonts()`, CSS Fonts
Module Level 4 §11.4)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`

## Симптом

Оба файла, гейтящие раскладку через `document.fonts.ready.then(...)`
(`positioning/inline-static-position-001.html`,
`linebox/vertical-align-top-bottom-001.html`), TIMEOUT: гарнес не завершается
вовсе, ни один тест не регистрируется.

`_lumen_get_fonts()` (dom.rs:6770) строит объект `fontSet` с `length`/`item`/
`entries`/`forEach`/`Symbol.iterator`, но нигде не определяет свойство
`ready`. `document.fonts.ready` — `undefined`, `.then(...)` на нём бросает
синхронный `TypeError` внутри верхнеуровневого `<script>`, до того как
`testharness.js` успевает зарегистрировать хоть один `test()` — отсюда именно
TIMEOUT (гарнес молчит), а не чистый FAIL.

## Влияние вне WPT

`document.fonts.ready` — стандартный способ дождаться, что все `@font-face`
подключились, прежде чем измерять раскладку/рисовать canvas-текст (частый
паттерн в веб-шрифтовых библиотеках и в самих WPT-тестах CSS Fonts/Text).
Пересекается с уже задокументированным в `docs/wpt-status.md` (строка
`fonts`) ограничением, что `url()`-источники `@font-face` подгружаются
асинхронно фоновым потоком — `fonts.ready` мог бы стать штатным способом
дождаться этого события из живого (не headless-однослотового) JS, но сейчас
такого способа нет вовсе.

## .ini

`tests/wpt/metadata/css/CSS2/{positioning/inline-static-position-001,linebox/vertical-align-top-bottom-001}.html.ini`
— `expected: TIMEOUT` на уровне теста (сабтестов нет — гарнес не долетает до
`test()`).
