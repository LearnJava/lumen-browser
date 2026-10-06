# BUG-1341 — `display: inline-block` без явной ширины и с блочными детьми без ширины получает ширину контейнера

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/box_tree/` — shrink-to-fit для `inline-block`; тот же приём, что у BUG-1335)

## Симптом

`--dump-layout`, `border-right: 3px solid green` у `inline-block` (поля `offsetWidth`):

| содержимое | `offsetWidth` | ожидается |
|---|---|---|
| один `<span style="display:block;height:20px">` | **1024** | 3 |
| `<div style="height:20px">` внутри `<div style="display:inline-block">` | **1024** | 3 |
| `<span style="display:block;height:20px;width:30px">` | 33 | 33 |

Причина не подтверждена кодом; гипотеза — пустой блок даёт нулевой max-content, и ветка shrink-to-fit пропускается при `0` (как `intrinsic > 0.0` у таблицы, BUG-1335).

## Как найдено

WPT-RUN-14 срез 11: `borders/border-right-applies-to-012.xht`, `border-right-color-applies-to-012.xht`, `border-right-width-applies-to-012.xht`, `background-position-applies-to-012.xht` (4 id): `span#inline-block { border-right: green solid 3px; display: inline-block }` с двумя блочными `span` высотой 0.5in. Эталон — вертикальная линия 3 px, у Lumen — на правом краю окна.

## Что делать

Считать shrink-to-fit как `min(max(min-content, available), max-content)` и для нулевого max-content (ширина рамок и полей, а не `available`).

## Как проверить

`css/CSS2/borders/border-right-applies-to-012.xht`.
