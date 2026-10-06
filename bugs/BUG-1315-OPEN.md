# BUG-1315 — CSSOM-присваивание невалидного значения grid-свойству не отклоняется

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** js — `crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand` (`:3093`): у ключей без зарегистрированной грамматики значение «проходит без изменений» (комментарий над функцией); у `grid-*` грамматики нет. Тот же механизм, что [BUG-1297](BUG-1297-OPEN.md) (`background-*`) и [BUG-563](BUG-563-OPEN.md) (anchor).

## Симптом

`el.style.cssText = ''; el.style[prop] = val; el.style[prop]` (`run_report.py --all`, одна страница-проба):

| свойство = значение | получено | ожидается |
|---|---|---|
| `gridTemplateColumns = "-10px"` | `-10px` | `""` |
| `gridTemplateColumns = "10px 10pxx"` | `10px 10pxx` | `""` |
| `gridAutoFlow = "row row"` | `row row` | `""` |
| `gridRow = "5 / 8 / 3"` | `5 / 8 / 3` | `""` |
| `gridTemplateAreas = '"a b" "c"'` | `"a b" "c"` | `""` (не прямоугольник) |
| `gridAutoColumns = "none"` | `none` | `""` |
| `flexGrow = "-1"` | `-1` | `""` |
| `flowTolerance = "foo"` | `foo` | `""` |
| контроль: `width = "-10px"` / `"10pxx"`, `color = "nocolor"`, `marginTop = "foo"` | `""` | `""` |

Контрольные четыре отклоняются, то есть дефект не в самом `style`, а в отсутствии грамматики у grid-свойств (и `flex-grow`).

## Как найдено

WPT-RUN-14 срез 8: `css/css-grid/parsing/*-invalid.html` — 11 id (`expected "" but got "-10px"`, `…but got "5 8"`, `…but got "none none"`, `…but got "auto"`); `subgrid/grid-template-invalid.html` (22) — вместе 12 id, 338 сабтестов; `grid-lanes/tentative/parsing/{flow-tolerance,grid-lanes-pack,grid-lanes-direction,grid-lanes-shorthand}-invalid.html` (4 id, 51 сабтест) учтены в кластере grid-lanes. Обратная сторона — `*-valid`/`*-computed` тех же файлов — описана в [BUG-1307](BUG-1307-OPEN.md).

## Что делать

Грамматика для `grid-template-columns/rows` (`none | <track-list> | <auto-track-list> | subgrid …`, отрицательные длины недопустимы), `grid-template-areas` (прямоугольные именованные области), `grid-auto-columns/rows` (без `none`), `grid-auto-flow`, `grid-row/column(-start/-end)`, `grid-area`, шорткаты `grid`/`grid-template`, `flex-grow/shrink` (неотрицательное число) — в `_lumen_canonicalize_longhand` или, лучше, вызов разборщика движка (`crates/engine/layout/src/style/values/flexgrid.rs`), чтобы CSSOM и каскад не расходились.

## Как проверить

`css/css-grid/parsing/grid-template-columns-invalid.html`, `grid-row-invalid.html`, `grid-auto-flow-invalid.html`, `grid-template-areas-invalid.html`, `subgrid/grid-template-invalid.html`.
