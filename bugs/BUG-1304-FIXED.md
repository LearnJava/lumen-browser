# BUG-1304 — `background-attachment: local` ≡ `scroll`: фон не прокручивается вместе с содержимым прокручиваемого бокса и не режется по padding-box контента

**Статус:** FIXED 2026-10-19 (P1; задача `BG-ATTACH-LOCAL`); остаток — [BUG-1653](BUG-1653-OPEN.md)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** layout/paint (`crates/engine/layout/src/style/values/background.rs` разбирает `local`, но `CSS-SPECS.md`: «`local` = `scroll`»; `PushScrollLayer` не покрывает слои фона)

**Тип:** нереализованная функциональность — режим `local` разбирается (`BackgroundAttachment::Local`), но в paint ему нет отдельного пути. Ведётся задачей `BG-ATTACH-LOCAL`.

## Симптом

`<div id=s style="width:100px;height:100px;overflow:scroll;background:linear-gradient(red 0,red 100px,blue 100px,blue 300px) local"><div style="height:400px"></div></div>`, после `s.scrollTop = 100` (`scrollTop` читается как 100, дочерний блок прокручивается: пиксель (20, 20) синий):

| | получено | ожидается |
|---|---|---|
| фон `local` | красный в (20, 20) — градиент остался на месте | синий: слой сдвигается вместе с содержимым |

В `--dump-display-list` градиент идёт вне `PushScrollLayer`. Кроме сдвига, клип: в `attachment-local-clipping-color-1.html` (`overflow:hidden; border:10px dashed; background:green local border-box`) Lumen заливает зелёным и промежутки штриховой рамки, у эталона они белые (проверено на снимках). С `rtl` и `flex-direction: *-reverse` начало прокрутки у правого/нижнего края (`local-attachment-rtl-*`, `local-attachment-flex-*-reverse-*`) — не проверялось отдельно.

## Как найдено

WPT-RUN-14 срез 6: 26 reftest — `background-attachment-local/*` (14), `local-attachment-*` (5), `table-cell-background-local*` (3), `background-attachment-350/353`, `background-attachment-local-hidden`, `background-attachment-fixed-inside-transform-1` — все `thick`.

## Что делать

См. `BG-ATTACH-LOCAL` в `ROADMAP.md`.

## Как проверить

`css/css-backgrounds/background-attachment-local/*.html`, `local-attachment-*.html`, `table-cell-background-local*.html`.

## Исправлено

`background_mask.rs`: `local_bg_scroller` (блок-скроллер `overflow: hidden|scroll|auto` с `local`-слоем) выводит слои и `background-color` под `local`-нижним слоем из обычного прохода и рисует их в `emit_local_background` сразу после `PushScrollLayer`/`PushClipRect` (оба пути — `walk` и `box_layer_ops`). Область позиционирования — `local_scroll_area` (padding-box, разросшийся до прямоугольников всех детей, в том числе влево/вверх для `rtl` и `*-reverse`), клип — padding-box окна. Скруглённый скроллер — неподвижный `PushClipRoundedRect` по внутренним радиусам (`local_round_clip`) вокруг скролл-слоя. Тесты `background_attachment_local_*` (5) в `paint/src/display_list/tests/background_and_layers.rs`. Прогон 30 рефтестов (`--screenshot` + попиксельное сравнение): совпадают 8 → 18 из 30 (clipping-color-1/2/3/6, clipping-image-1/2/3/6, positioning-2/3/5, 350, flex-column-reverse, rtl+row-reverse, …); clipping-color-4/5 расходятся на 1 уровень канала по дуге (сглаживание клипа против `FillRoundedRect`).
