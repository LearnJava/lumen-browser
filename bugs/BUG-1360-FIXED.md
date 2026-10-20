# BUG-1360 — `position:absolute` с `left/right:auto` в контейнере `direction: rtl`: статическая позиция у левого края, а не у правого

**Статус:** FIXED 2026-10-20 (P3)
**Исправление:** `static_x_is_right_edge` (`multicol_abspos.rs`): block-flow родитель с `direction: rtl` отдаёт `container_right` как static_x, `place_abs_child` вычитает ширину бокса и `margin-right`. Тесты: `abs_auto_margins::rtl_*`.
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — статическая позиция abspos по `direction`)

## Симптом

`--dump-layout`: `<div style="direction:rtl;position:relative;width:200px;height:100px"><div style="position:absolute">X</div></div>` (Ahem 20 px):

| случай | получено | ожидается (§10.3.7: статическая позиция `right` при `direction: rtl`) |
|---|---|---|
| `left:auto; right:auto; width:auto` | `Block rect=(0,0,20,20)` | `x=180` |
| `right:-0px; width:20px` | `x=180` (верно, `right` задан) | — |

Бокс без явных `left`/`right` прижимается к левому краю контейнера независимо от `direction`.

## Как найдено

WPT-RUN-14 срез 13: `positioning/absolute-non-replaced-width-*`, `absolute-replaced-width-*`, `right-*` с `direction: rtl` и `left: auto; right: auto` — 19 id по правилу («rtl» + `position:absolute` в стиле, thick). Пример `positioning/absolute-non-replaced-width-002.xht` (ожидается синий квадрат в правом верхнем углу, Lumen — в левом). Связано с [BUG-1321](BUG-1321-FIXED.md) (атрибут `dir` не задаёт `direction`) — но здесь `direction` задан CSS.

## Что делать

Статическая позиция по `direction`: в rtl — у правого края контейнера (минус ширина бокса).

## Как проверить

`css/CSS2/positioning/absolute-non-replaced-width-002.xht`.
