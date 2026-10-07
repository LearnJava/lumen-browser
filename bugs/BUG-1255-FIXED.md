# BUG-1255 — Процентная `height` flex-элемента не разрешается против определённой высоты контейнера: в `display:flex;flex-direction:column;height:100px` ребёнок с 

**Статус:** OPEN
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs` — процентная высота flex-элемента)

## Симптом

Процентная `height` flex-элемента не разрешается против определённой высоты контейнера: в `display:flex;flex-direction:column;height:100px` ребёнок с `height:50%` получает `h=0` (должно быть 50); вложенные `height:100%` в `flex:1` у контейнера с `height:80px` раздувается до 300 (`height expected 80 but got 300`). WPT-RUN-14-S1: кластер «percentage heights» — 58 id (`percentage-heights-*`, `flexbox-definite-sizes-*`, `dynamic-isize-change-*`; оценка по именам), причина подтверждена пробой `height:50%`.

## Описание

Проба `--dump-layout`: `<div style="display:flex;flex-direction:column;width:100px;height:100px"><div style="height:50%;background:green"></div></div>` — ребёнок `rect=(0,0,100,0)`, ожидается `100×50`.

## Как найдено

WPT-RUN-14 срез 1; `percentage-heights-011.html`, `flex-minimum-height-flex-items-010.html` (`but got 300` — запасная высота).

## Как проверить

`css/css-flexbox/percentage-heights-*.html`.
