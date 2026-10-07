# BUG-1387 — `height: stretch` (и `block-size: stretch`) прибавляет border и padding к уже растянутому размеру

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — разрешение `Length::Stretch` по высоте; для `width` — `layout_dispatch.rs:610`, там верно)

## Симптом

`--dump-layout`, контейнер `block-size:50px; inline-size:40px; margin:5px; border:2px; padding-block:5px; padding-inline:3px; display:inline-block`, ребёнок `margin-block:2px 3px; border:3px solid; padding:2px; inline-size:20px; block-size:stretch`:

| вариант | высота border-box, получено | ожидается |
|---|---|---|
| `block-size:stretch`, `height:stretch` | 55 | 45 (= 50 − 2 − 3) |
| `min-block-size:stretch` | 55 | 45 |
| `max-block-size:stretch` | 55 | 45 |

Без border/padding (`margin:10px 0`, контейнер `height:100px`) — 80, верно. То есть растянутая величина трактуется как высота content-box, и border+padding (10 px) добавляются сверху. CSS Sizing 4 §4.1: для `stretch` border-box = размер содержащего блока − поля.

## Как найдено

WPT-RUN-14 срез 16: `css-sizing/stretch/stretch-{block,min-block,max-block}-size-00*`, `stretch-alias-*-block-size-*` — 34–36 из 46 сабтестов в каждом; `stretch/block-height-001…008` (4 thick), `stretch-inline-size-002/003` (по вертикальным `writing-mode` — не разобрано). Всего 54 id, 696 из 981 сабтеста.

## Что делать

В ветке `Length::Stretch` по высоте (как для `width`, `layout_dispatch.rs:610`) вычитать из доступной высоты поля, а border+padding не прибавлять при `box-sizing: content-box`.

## Как проверить

`css/css-sizing/stretch/stretch-block-size-001.html` (36 из 46 сабтестов).
