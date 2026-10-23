# BUG-1484 — У `position:absolute` бокса с `width:auto` и заданными `left` процентные `padding-left`/`padding-right` дают ширину 0

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs` — абсолютно позиционированный бокс: процентные горизонтальные `padding-left`/`padding-right` при `width:auto`)

## Симптом

`position:absolute; left:50px; height:100px; padding-left:50%; padding-right:50%` (ширина `auto`, правый инсет `auto`) в контейнере `position:relative` с padding box 50×100: `getBoundingClientRect().width` — 0, ожидается 50 (проценты `padding` — от ширины содержащего блока, 25 + 25; CSS Box 4 §padding-physical). Вертикальные проценты (`padding-top:100%; padding-bottom:100%` при `width:50px`) верны (высота 100), у неабсолютного бокса горизонтальный случай не проверялся. 1 id среза: `position-absolute-padding-percentage.html` (1 из 2 сабтестов).

## Проба

Проба (`--mcp`, контейнер `position:relative;padding:10px;width:30px;height:80px`):

| внутренний `position:absolute` | у нас | ожидается |
|---|---|---|
| `left:50px;top:0;height:100px;padding-left:50%;padding-right:50%` | ширина 0 | 50 |
| `left:50px;width:50px;top:0;padding-top:100%;padding-bottom:100%` | высота 100 | 100 |

## Как найдено

WPT-RUN-14 срез 21: `css-position/position-absolute-padding-percentage.html`.

## Что делать

При `width:auto` и абсолютном боксе учесть процентный горизонтальный `padding` в ширине (shrink-to-fit даёт 0 содержимого, но `padding` — не 0).

## Как проверить

Таблица выше; `css/css-position/position-absolute-padding-percentage.html`.

## Исправление

`place_abs_child` (`multicol_abspos.rs`): intrinsic-ширины (`max_content_outer_width`/`min_content_outer_width`) считают процентный `padding` от 0, а содержащий блок абсолютного бокса определён. К shrink-to-fit ширине добавляется разница «процент от `cb.width` − процент от 0» для `padding-left`/`padding-right`. Кроме того, `avail_w` (зазор между инсетами или shrink-to-fit ширина) был базой процентов `padding`/`margin` самого бокса в `dispatch_box` — теперь, если `avail_w != cb.width`, база передаётся через `UsedSizeOverride::percentage_base = cb.width` (CSS 2.1 §8.1). `percentage_base` добавлен в `UsedSizeOverrideBits` — ключ кэша раскладки. Тест — `box_tree/tests/shrink_to_fit_margins.rs::absolute_auto_width_resolves_percent_horizontal_padding`.
