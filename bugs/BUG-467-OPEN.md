# BUG-467: CSS Font Loading API (FontFace/FontFaceSet/FontFaceSetLoadEvent) practically unimplemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6770` — `_lumen_get_fonts()`, CSS Fonts
Module Level 4 §11 / CSS Font Loading Module)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`;
расширен срезом 17 — массовый прогон `css/css-font-loading`

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

## Расширение (срез 17, `css/css-font-loading`): вся категория, а не только `.ready`

Массовый прогон `css/css-font-loading` (21 testharness id, 17/21 harness OK,
0/79 сабтестов) показал, что `.ready` — лишь один симптом целиком
отсутствующей CSS Font Loading API. Ни один сабтест не прошёл; читая
`crates/js/src/dom.rs:6771` (`_lumen_get_fonts()`) и грепая `crates/js/src/
dom.rs`/`v8_runtime.rs` на `"FontFace"`/`"FontFaceSet"`/`"FontFaceSetLoadEvent"`
(ноль совпадений вне комментариев), подтверждено: движок не предоставляет ни
одного из трёх глобальных конструкторов, а `document.fonts` — statically-built
plain object лишь с `length`/`item`/`entries`/`forEach`/`Symbol.iterator`, не
настоящий `FontFaceSet`. Раздельные фасеты, все воспроизведены чтением кода +
логом прогона (`.tmp/wpt-css-font-loading.log`):

1. **`FontFace` не определён вовсе** — `new FontFace(family, source)` бросает
   `ReferenceError`, а не строит объект. Доминирующая причина: 12 из 21
   файлов синхронно падают на этом до регистрации ассерта (`empty-family-load`,
   `font-face-reject`, `fontface-descriptor-updates-2` (3 сабтеста),
   `fontface-font-variation-settings-persisted-js-api`, `fontface-fonts-
   loading`, `fontface-invalid-arraybuffer`, `fontface-invalid-family.
   tentative` (9 сабтестов), `fontface-load-in-modal-dialog`, `fontface-
   override-descriptor-getter-setter.sub` (24 сабтеста — все дескрипторы
   `ascentOverride`/`descentOverride`/`lineGapOverride`),
   `fontfacesetloadevent-constructor` (второй сабтест)).
2. **`FontFaceSet` не определён вовсе** — по спеке `FontFaceSet` не должен
   конструироваться (`new FontFaceSet()` обязан бросить `TypeError` — «illegal
   constructor»), но раз идентификатора нет глобально, движок бросает
   `ReferenceError` вместо ожидаемого спекой `TypeError`
   (`historical.html`: `assert_throws_js` требует `TypeError`, получает
   `ReferenceError` — фиксирует, что даже «этого не должно существовать»
   API не смоделировано корректно).
3. **`FontFaceSetLoadEvent` не определён вовсе** (`fontfacesetloadevent-
   constructor.html`, первый сабтест).
4. **`document.fonts` — не `FontFaceSet`, а урезанный read-only снимок**:
   в `_lumen_get_fonts()` (dom.rs:6771) объект `fontSet` определяет только
   `length`/`item`/`entries`/`forEach`/`Symbol.iterator` — отсутствуют ВСЕ
   остальные члены интерфейса `FontFaceSet` (WHATWG-подобный `Set`):
   - **`size` отсутствует вовсе** (только `length`, спека и все evergreen-
     браузеры используют `size`, не `length`) — `fontfaceset-update-after-
     stylesheet-change.html`/`nonexistent-file-url.html` оба падают на первом
     же `assert_equals(document.fonts.size, 1, …)`, `undefined !== 1`,
     не добираясь до самой проверки динамического обновления при
     add/remove `<style>` или до поведения при недостижимом `file://`-источнике
     — те вопросы остаются непроверенными этим прогоном, но сам `size`
     подтверждён отсутствующим чтением кода.
   - `add`/`delete`/`clear`/`has`/`keys` отсутствуют —
     `fontfaceset-clear-css-connected`/`fontfaceset-delete-css-connected`
     («fonts.keys is not a function»), `fontfaceset-has` («fonts.keys is not
     a function or its return value is not iterable»).
   - `load`/`ready` (промисы) отсутствуют — `fontfaceset-load-css-connected`
     («fonts.load is not a function»), `fontfaceset-load-css-wide-keywords`
     (24 сабтеста, TIMEOUT — ошибка ловится, но что-то в цепочке промисов не
     резолвится/не таймаутит корректно), `fontfaceset-load-var` (4 сабтеста,
     TIMEOUT, тот же паттерн).
   - `addEventListener`/`onloading`/`onloadingdone`/`onloadingerror`
     отсутствуют — `fontface-loadingevent.html`: «fontSet.addEventListener is
     not a function».

`idlharness.https.html` — отдельный артефакт: `TEST_END: ERROR` с
`AssertionError: Got results from /css/css-font-loading/historical.html,
expected /css/css-font-loading/idlharness.https.html` — тот же класс, что
BUG-380 (browsing-context reuse не проверяет исход навигации между тестами),
не новый симптом этой категории.

Committed `.ini` для всех 21 непройденных файлов; повторный прогон из
worktree подтверждает 0 unexpected / 0 unexpected passes (79/79 expected).

Пересекается с [BUG-471](bugs/BUG-471-OPEN.md) (CSSOM `insertRule`/`sheet` не
подключены) — если `document.fonts` когда-нибудь станет реактивным на
добавление/удаление `@font-face`-правил через CSSOM, то потребуется тот же
фундамент, что и для BUG-471.

## Срез 26 (`css/css-ruby`, 2026-08-03)

`line-spacing.html` — `document.fonts.load("16px Ahem").then(...)` at top
level, before any `test()` registers: TIMEOUT (`document.fonts.load` is one
of the missing Maplike/loading members this bug already documents). `.ini`
under `tests/wpt/metadata/css/css-ruby/line-spacing.html.ini`,
`expected: TIMEOUT`.

## Срез 31 (`css/css-fonts`, 2026-08-03)

+2 files/100 subtests: global `FontFace` constructor is `undefined`
(`new FontFace(...)` throws `ReferenceError`) — the imperative half of the
Font Loading API, not just `document.fonts`'s read side. Several more
`css-fonts` files this slice TIMEOUT on `Cannot read properties of
undefined (reading 'then')` chained off a `document.fonts.*` call — same
family, not yet individually re-counted per file. `.ini` under
`tests/wpt/metadata/css/css-fonts/`.
