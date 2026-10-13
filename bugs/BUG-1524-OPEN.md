# BUG-1524 — `align-content`/`justify-content`/`align-items`/`justify-items`/`align-self`/`justify-self`/`place-*`: CSSOM принимает недопустимое, не знает `left`/`safe`/`legacy`, отдаёт `auto` вместо `normal`, `place-*` нет в `getComputedStyle`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/apply/layout.rs` — разбор `align-*`/`justify-*`; `selector_query.rs::computed_style_to_map`)

## Симптом

Начальное значение `align-content`/`justify-content`/`align-items`/`justify-items` читается как `auto` (должно быть `normal`); `flex-start`/`self-start` в resolved-значении сворачиваются в `start`, `flex-end`/`self-end` — в `end` (resolved-значение должно сохранять ключевое слово); `left`/`right` для `justify-*`, `safe center`/`center safe`/`unsafe end`, `legacy`/`legacy left`, `anchor-center` отвергаются, хотя допустимы; `auto` у `align-content`/`justify-content`, `space-around` у `align-self`/`justify-self` принимаются, хотя недопустимы; `first baseline` не сворачивается в `baseline`; `place-content`/`place-items`/`place-self` не входят в `getComputedStyle`; по сообщениям сабтестов шортхенд не раскладывается на лонгхенды (`align-items expanded value expected "normal" but got ""`). 79 id, 1 304 из 2 743 сабтестов.

## Проба

Проба (`--mcp`, `<div id=t style="width:100px;height:100px">`, `el.style[p] = v`, чтение `[style[p], getComputedStyle(el)[p]]`):

| присваивание | у нас | ожидается |
|---|---|---|
| начальное `alignContent` / `justifyContent` / `alignItems` / `justifyItems` (computed) | `auto` | `normal` |
| `alignContent = "flex-start"` (computed) | `start` | `flex-start` |
| `alignItems = "self-start"` (computed) | `start` | `self-start` |
| `alignContent = "auto"` | принято, `auto` | отвергнуто (`""`) |
| `alignSelf = "space-around"`, `justifySelf = "space-around"` | принято | отвергнуто |
| `alignContent = "safe center"`, `"center safe"`; `justifySelf = "left"`; `justifyItems = "legacy left"`; `alignSelf = "anchor-center"`; `alignSelf = "safe start"` | отвергнуто (`""`) | принято |
| `alignContent = "first baseline"` | `first baseline` | `baseline` |
| `placeContent = "center"`; `"place-content" in getComputedStyle(el)` | `style.placeContent` — `center`; computed `""`, `in` — `false` | `in` — `true` |
| `placeContent = "left"`, `placeSelf = "auto left"` | отвергнуто | принято |


## Как найдено

WPT-RUN-14 срез 24: `css-align/parsing/*` (`align-*`/`justify-*`/`place-*` `-computed`/`-valid`/`-invalid`/`-shorthand`, около 50 id), `default-alignment/parse-*`, `place-items-shorthand-*`, `shorthand-serialization-001`, `content-distribution/parse-*`, `place-content-shorthand-*`, `self-alignment/parse-*`, `place-self-shorthand-*`, `inheritance.html`. Класс тот же, что [BUG-1297](BUG-1297-OPEN.md)/[BUG-1391](BUG-1391-OPEN.md): `element.style` не валидирует и не канонизирует. Пересекается с [BUG-1307](BUG-1307-OPEN.md) (`justify-items` в карте).

## Что делать

Привести разбор значений к CSS Box Alignment 3 §«Value Definitions»: `normal` как начальное, `<overflow-position>? <self-position>`, `left`/`right`, `legacy`, `<baseline-position>` (`first`/`last`), `anchor-center` у `*-self`/`*-items`; resolved-значение не сворачивать; `place-*` ввести в `computed_style_to_map` и раскладывать шортхенд. Недопустимое отвергать.

## Как проверить

Таблица выше; `css/css-align/parsing/align-content-computed.html`, `parsing/place-items-computed.html`, `content-distribution/parse-justify-content-003.html`.
