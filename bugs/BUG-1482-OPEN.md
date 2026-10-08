# BUG-1482 — `position:absolute; inset:0` на `<button>`/`<input>` не растягивает элемент (CSS Position 3 §5.3: «semi-replaced» элементы растягиваются как не-replaced)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — абсолютно позиционированные `<button>`, `<input>`, `<select>`: «semi-replaced» растяжение по инсетам)

## Симптом

`<button style="position:absolute;inset:0;border:0;padding:0">x</button>` в контейнере 200×100 получает размер по содержимому `8,7×21`, ожидается `200×100`; `<input style="position:absolute;inset:0;box-sizing:border-box">` — `174×21` вместо `200×100`. 3 id `css-position/position-absolute-semi-replaced-stretch-{button,input,other}.html`, все reftest `thick`.

## Проба

Проба (`--mcp`):

| разметка | у нас | ожидается |
|---|---|---|
| `<button>` `position:absolute;inset:0` | `0,0,8.7,21` | `0,0,200,100` |
| `<input>` `position:absolute;inset:0;box-sizing:border-box` | `0,0,174,21` | `0,0,200,100` |

## Как найдено

WPT-RUN-14 срез 21: `css-position/position-absolute-semi-replaced-stretch-button.html`, `-input.html`.

## Что делать

Не считать `button`/`input`/`select`/`textarea` replaced при расчёте `width:auto`/`height:auto` с двумя инсетами (§5.3 исключает их из replaced-правил), применять растяжение по инсетам.

## Как проверить

Таблица выше; `css/css-position/position-absolute-semi-replaced-stretch-button.html`.
