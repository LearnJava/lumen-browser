# BUG-1520 — `<rt>` не получает UA-значение `font-size: 50%`: аннотация ruby рисуется шрифтом основного текста

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** layout (`crates/engine/layout/src/style/ua.rs` — UA-таблица стилей `<rt>`; `crates/engine/layout/src/ruby.rs`)

## Симптом

У `<rt>` в UA-таблице нет `font-size: 50%` (UA-таблица стилей HTML Rendering, раздел о ruby: `rt { font-size: 50%; … }`; CSS Ruby 1, UA-стиль): вычисленный `font-size` — как у родителя. Все тесты, у которых эталон строится на `<ruby>…<rt>●</rt></ruby>` как «образец меток выделения» (`text-emphasis-*`), получают в эталоне метки вдвое крупнее настоящих.

## Проба

Проба (`--mcp`):

| разметка | `getComputedStyle(rt).fontSize` | ожидается |
|---|---|---|
| `<div style="font:40px Arial"><ruby>試<rt>x</rt></ruby></div>` | `40px` | `20px` |
| `<div lang=ja style="line-height:5"><ruby>試<rt>x</rt></ruby></div>` (16 px по умолчанию) | `16px` | `8px` |

`--screenshot` эталона `text-emphasis-position-property-002-ref.html` рисует `●●●●●` крупными кружками, тест — мелкими; тест с `rt{font-size:50%}`, добавленным в эталон, рисует мелкие метки, но снимки всё равно не совпадают (метки смещены по вертикали на 4–5 px по снимкам) — см. BUG-1521.

## Как найдено

WPT-RUN-14 срез 23: `css-text-decor/text-emphasis-*` — 122 из 131 reftest «text-emphasis» имеют эталон на `<ruby>`; причина: размер эталона. Не единственная причина падения этих id (BUG-1521).

## Что делать

Добавить `rt { font-size: 50% }` в UA-стиль (и `rtc`); проверить, что `ruby.rs` не дублирует уменьшение.

## Как проверить

Таблица выше; после правки перепрогнать `css/css-text-decor/text-emphasis-position-property-002.html` и посмотреть остаток по BUG-1521.
