# BUG-1304 — `background-attachment: local` ≡ `scroll`: фон не прокручивается вместе с содержимым прокручиваемого бокса и не режется по padding-box контента

**Статус:** OPEN (ДОРАБОТКА → [BG-ATTACH-LOCAL](../ROADMAP.md))
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
