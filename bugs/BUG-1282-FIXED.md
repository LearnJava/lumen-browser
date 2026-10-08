# BUG-1282 — `position: absolute` с обеими инсетами по оси: `auto`-поля не распределяются, бокс прижат к началу

**Статус:** FIXED 2026-10-08
**Исправление:** `place_abs_child` — при обеих инсетах и заданном размере по оси свободное место уходит в `auto`-поля (`auto_margin_shift`: оба `auto` — поровну без отрицательных, одно — остаток); тесты `box_tree/tests/abs_auto_margins.rs`.
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 3, `css/css-ui`)
**Область:** layout (`crates/engine/layout/src/box_tree/multicol_abspos.rs::place_abs_child`)

## Симптом

CB `position: relative; 100×100`, ребёнок `position: absolute; width: 70px; height: 70px` (`--dump-display-list`,
`body{margin:0}`):

| инсеты и поля ребёнка | получено | ожидается |
|---|---|---|
| `inset: 0; margin: auto` | (0, 0) | (15, 15) — центр |
| `left: 0; right: 0; margin: 0 auto` | x = 0 | x = 15 |
| `left: 0; right: 0; margin-left: auto; margin-right: 5px` | x = 0 | x = 25 |

Не зависит от `position` CB (`relative`/`absolute`/`fixed`) и от вьюпорта. Это самый распространённый способ
центрировать модальное окно или иконку (`position:absolute; inset:0; margin:auto`).

## Причина

`place_abs_child` берёт поля через `resolve_or_zero` (`auto` → 0) и ставит бокс в `cb.x + left + margin_left`
независимо от остальных членов уравнения. CSS 2.1 §10.3.7 / §10.6.4 (CSS Position L3 §5.1): при заданных `left`,
`right` и `width` свободное место уходит в `auto`-поля — оба `auto` → поровну, одно `auto` → ему остаток; то же по
вертикали для `top`/`bottom`/`height`. Ветки нет.

## Как проверить

WPT `css/css-ui/box-sizing-003.html`, `box-sizing-005.html` (с `box-sizing: border-box`, 2 reftest). Тестов на
§10.3.7 в `css/CSS2/positioning` и `css/css-position` больше — не прогонялись.
