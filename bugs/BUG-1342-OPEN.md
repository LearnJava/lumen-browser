# BUG-1342 — Сокращение `border` не сбрасывает `border-color` в `currentcolor`, если цвет не указан

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/style/parse/box_sides.rs::apply_border_shorthand`)

## Симптом

`<style>.a { border: solid 10px } div { color: green; border-color: red }</style><div class=a>` — `.a` (специфичность 0,1,0) перебивает `div` (0,0,1) в `border`, а `border` сбрасывает все три подсвойства, в том числе `border-color` → `currentcolor` (зелёный). Lumen: `getComputedStyle().borderTopColor` = `rgb(255, 0, 0)` — цвет из правила `div` пережил сокращение, у которого есть победа в каскаде. Если цвет в сокращении указан (`border: solid 10px green`) — верно.

## Как найдено

WPT-RUN-14 срез 11: `borders/border-shorthands-002.xht` и `-003.xht` (по `.test { border: solid 1em }` против `div { border-color: red }`).

## Что делать

В `apply_border_shorthand` и `apply_border_side_shorthand` выставлять все три подсвойства (`-width: medium`, `-style: none`, `-color: currentcolor`), затем перекрывать указанными. Заодно: `border-style: solid` без ширины — `medium` (BUG-1298).

## Как проверить

`css/CSS2/borders/border-shorthands-002.xht`.
