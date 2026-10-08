# BUG-1422 — `position: relative` с `top`/`bottom` в процентах: база — ширина контейнера, а не высота; при `height: auto` смещение не должно применяться

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs::relative_offset`, `:1887`)

## Симптом

`--dump-layout`, контейнер `width:300px` (+ `height:100px` где указано), потомок `position:relative; height:5px; width:5px`:

| разметка | y потомка | ожидается |
|---|---|---|
| `top:10%` в контейнере h=100 | **30** | 10 |
| `left:10%` в контейнере w=300 (x) | 30 | 30 |
| `bottom:10%` в контейнере h=100 | **−30** | −10 |
| `top:10%` в контейнере `height:auto` | **30** | 0 |
| `top:calc(10px + 10%)` в контейнере w=500 h=100 | 60 | 20 |
| `top:10%` у `position:absolute` в `position:relative` h=100 (контроль) | 10 | 10 |

База процента для `top`/`bottom` берётся из ширины содержащего блока (число `cb`), не из высоты.

## Как найдено

WPT-RUN-14 срез 19: `css-values/calc-offsets-relative-top-1.html` и `calc-offsets-relative-bottom-1.html` (FAIL, `thick`).
В срезе 5 (`css-transforms`) процентные смещения не проверялись.

## Что делать

Передавать в `relative_offset` ширину и высоту содержащего блока отдельно; при неопределённой высоте (`auto`) считать
`top`/`bottom` в процентах за `auto`.

## Как проверить

Страница из таблицы; `css/css-values/calc-offsets-relative-top-1.html`, `…bottom-1.html`.
