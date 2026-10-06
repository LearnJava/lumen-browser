# BUG-1340 — Единицы `ex` и `ch` — константы 0,546 em / 0,631 em, а не метрики шрифта

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** layout (`crates/engine/layout/src/style/values/length.rs:171` — `FONT_CH_EX`; значение не заполняется из шрифта бокса, откат `0.5 × em`)

## Симптом

`<div style="height:1ex">`, `font: 20px/1 …`:

| шрифт | `1ex` | ожидается | `1ch` | ожидается |
|---|---|---|---|---|
| Ahem | 10.918 | **16** (0.8 em) | 12.617 | **20** (1 em) |
| serif | 10.918 | по метрике гарнитуры (число другое) | 12.617 | по метрике |
| monospace | 10.918 | по метрике | 12.617 | по метрике |

Три разных шрифта дают одно и то же число, пропорциональное `em` (0,5459 / 0,6309): метрика шрифта не читается.

## Как найдено

WPT-RUN-14 срез 11: 14 id (`borders/border-{top,right,bottom,left}-width-{080,083,084}.xht`, `background-position-001/002.xht`) — `border-bottom-width: 1ex` / `6ex` при `font: 20px/1 Ahem`; эталон — картинка высотой 16 / 96 px.

## Что делать

Заполнять `FONT_CH_EX` из x-height и ширины «0» выбранной гарнитуры (OS/2 `sxHeight`, `hmtx` для U+0030); откат `0.5 em` — только когда шрифт недоступен. Тот же путь нужен `rex`/`rch` (`ROOT_FONT_METRICS`).

## Как проверить

`css/CSS2/borders/border-bottom-width-080.xht`.
