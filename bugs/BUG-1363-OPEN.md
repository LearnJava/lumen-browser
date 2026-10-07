# BUG-1363 — Фон корневого `<html>` не распространяется на canvas, если у `<html>` есть `position: absolute` / `display: table`: закрашивается только его бокс

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout/paint (`crates/engine/layout/src/box_tree/entry.rs::canvas_background_color`, `:801` — берёт первый `Block|FlowRoot` потомок корня; у `html` с `position:absolute`/`display:table` бокс другого вида или иной порядок)

## Симптом

CSS Backgrounds 3 §2.11.2: фон корневого элемента закрашивает весь canvas независимо от `position`/`display` корня. `--dump-display-list`, вьюпорт 400×300:

| `<html style=…>` | получено | ожидается |
|---|---|---|
| `background:yellow; border:10px solid black` | `FillRect (0,0,400,320)` — на весь canvas (верно) | — |
| `position:absolute; left:100px; top:100px; width:100px; height:100px; background:yellow; border:10px solid black` | `FillRect (100,100,120,120)` — только бокс html | жёлтый на весь canvas, чёрная рамка в `(100,100,120,120)` |
| то же + `display:table` | `FillRect (100,100,120,120)` | то же |

## Как найдено

WPT-RUN-14 срез 13: `abspos/abspos-containing-block-initial-004a…004f.xht` и `-005a…005d.xht` (8 id): эталон `-004-ref.xht` — жёлтый canvas 800×600 (475 600 px `ffff00`) с чёрной рамкой, Lumen — жёлтый только внутри рамки (10 000 px). Правило отнесения: `abspos-containing-block-initial-*` в `abspos`; пробой подтверждены `004c` и минимальная страница выше.

## Что делать

Определять фон canvas от корневого элемента независимо от его `position`/`display` (в `canvas_background_color` не полагаться на вид первого бокса-ребёнка корня).

## Как проверить

`css/CSS2/abspos/abspos-containing-block-initial-004c.xht`.
