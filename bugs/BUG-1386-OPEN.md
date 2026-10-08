# BUG-1386 — `min-/max-width/height` со значениями `max-content`, `fit-content()` и `max-*: min-content` разобраны, но не применяются

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs:664-690` — ветки `Length::Stretch`/`MinContent` для `min-/max-*`; `MaxContent`/`FitContent` и вся ось высоты отсутствуют)

## Симптом

`--dump-layout`, `font:10px/1 Ahem` (шрифт подключён по `docs/probe-method.md`), `.b{width:200px}`:

| разметка | получено | ожидается |
|---|---|---|
| `width:10px; min-width:max-content` + текст `X X X` | 10 | 50 |
| `width:10px; min-width:fit-content(100px)` + два `inline-block` по 60px | 10 | 100 |
| `width:200px; max-width:fit-content(100px)` + то же | 200 | 100 |
| `width:200px; max-width:min-content` + `XX XX` | 200 | 20 |
| `width:200px; max-width:max-content` + `XX XX` | 200 | 50 |
| `height:0; min-height:max-content` + блок 100px | 0 | 100 |
| `height:0; min-height:min-content` + блок 40px | 0 | 40 |
| `height:200px; max-height:min-content` + блок 40px | 200 | 40 |

Работают: `min-width:min-content`, `width/height: min-content|max-content|fit-content`. Значения не отбрасываются парсером (`--dump-layout` печатает `min-w=max-content`), в раскладку не доходят.

## Как найдено

WPT-RUN-14 срез 16: 35 id — `css-sizing/fit-content-length-percentage-007…016` (10 thick), `block-size-with-min-or-max-content-*` (6 thick + 2 table), `fit-content-percentage-padding`, `min-width-max-width-precedence`, `replaced-m{in,ax}-*-min-content`, `vert-block-size-*`. Эталон — «зелёный квадрат 100×100».

## Что делать

Применять `min-content`/`max-content`/`fit-content()` в `min-*`/`max-*` обеих осей так же, как уже сделано для `width`/`height`; для оси высоты — через высоту содержимого блока.

## Как проверить

`css/css-sizing/fit-content-length-percentage-007.html`, `-009.html`, `block-size-with-min-or-max-content-2.html`, `min-width-max-width-precedence.html`.
