# BUG-1525 — `gap`, `row-gap`, `column-gap`, `grid-gap`, `grid-row-gap`, `grid-column-gap` не входят в `getComputedStyle`; сериализация `gap: normal normal`, `0`; `auto` и отрицательные значения принимаются

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** css-parser/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; разбор `gap`/`row-gap`/`column-gap`/`grid-*-gap`)

## Симптом

`'gap' in getComputedStyle(el)` — `false`, значение — пустая строка; то же `row-gap`, `column-gap` и три легаси-алиаса `grid-*-gap` (должны быть алиасами `gap`/`row-gap`/`column-gap`). Через `element.style`: `rowGap = "0"` читается как `0` (должно быть `0px`), `-1px` и `auto` принимаются (должны отвергаться), `gap = "normal normal"` не сворачивается в `normal`. Анимация `gap` (12 id `gaps/*-animation-*`) падает на первой же проверке `expected "50px" but got ""` — computed пуст. 39 id, 243 из 293 сабтестов.

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `"gap" in getComputedStyle(t)`, то же `row-gap`, `column-gap`, `grid-gap` | `false` | `true` |
| `style.rowGap = "0"` → `style.rowGap` | `0` | `0px` |
| `style.rowGap = "-1px"`, `"auto"` | принято | отвергнуто |
| `style.gap = "normal normal"` → `style.gap` | `normal normal` | `normal` |
| `style.gridGap = "10px"` → `getComputedStyle(t).gridGap` | `""` | `10px` |


## Как найдено

WPT-RUN-14 срез 24: `css-align/gaps/*` (`gap-parsing-001/002`, `*-gap-parsing-001`, `gap-normal-computed-001`, `legacy-gap-aliases-001`, `*-gap-animation-*`), `css-align/parsing/{gap,row-gap,column-gap,grid-*-gap}-{computed,valid,invalid,shorthand}`. [BUG-1307](BUG-1307-OPEN.md) и [BUG-1402](BUG-1402-OPEN.md) называют `row-gap`/`column-gap` среди пропущенных в карте; здесь — полный набор вместе с грамматикой и алиасами.

## Что делать

Добавить `gap`/`row-gap`/`column-gap` и алиасы `grid-*-gap` в `computed_style_to_map` (resolved: длина в px либо `normal`); канонизировать присваивание (`0` → `0px`, `normal normal` → `normal`); отвергать `auto`, отрицательные длины, `none`.

## Как проверить

`css/css-align/parsing/gap-computed.html`, `gaps/gap-normal-computed-001.html`, `gaps/gap-animation-001.html`.
