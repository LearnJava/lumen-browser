# BUG-1280 — у `<img>` с размером из CSS декодированный размер пишется в оба атрибута `width`/`height` и ломает пропорции

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 3, `css/css-ui`)
**Область:** layout (`crates/engine/layout/src/box_tree/image_requests.rs::apply_intrinsic_size`)

## Симптом

Картинка 100×100 (`css/css-ui/support/orange.png`), `--dump-layout` и `--screenshot`:

| разметка | получено | ожидается |
|---|---|---|
| `<img style="height:50px">` | 100×50 | 50×50 |
| `<img style="width:50px">` | 50×100 | 50×50 |
| `<img style="max-height:20px">` | 100×20 | 20×20 |
| `<img style="padding-right:30px;width:70px">` | 100×100 (border-box) | 100×70 |
| `<img height=50>` | 50×50 | 50×50 — верно |
| `<div style="width:50px"><img style="max-width:100%;height:auto">` | 50×50 | верно |

На снимке картинка действительно растянута (оранжевый прямоугольник 100×50). Дамп показывает `w=100.00` у `<img>`,
которой автор ширину не задавал.

## Причина

`apply_intrinsic_size` дописывает недостающие атрибуты, но смотрит только на атрибуты автора, не на CSS. Если ни
`width`, ни `height` атрибутом не заданы, оба слота получают сырой декодированный размер (ветка `_ =>`). Атрибуты
становятся презентационными подсказками `width: 100px; height: 100px`; авторский CSS перебивает одну сторону, а
вторая остаётся 100px из подсказки — пропорция (CSS 2.1 §10.3.2, §10.6.2, §10.4) уже не применяется, потому что
сторона формально задана. Ветка BUG-734 в `build.rs` (ratio в `aspect_ratio`, сырой размер только при «обе стороны
auto») не помогает: подсказка уже лежит в `style.width`.

## Что делать

Не превращать декодированный размер в подсказку `width`/`height` для слотов, которые автор не задавал атрибутом:
отдать его layout как natural size (как `set_embedded_image` у `<object>`) и выводить отсутствующую сторону из ratio
после каскада и `min-*`/`max-*`. Проверить и shell (`subresources.rs`, `wants_intrinsic`), и драйвер
(`driver/src/session.rs`) — путь общий.

## Как проверить

WPT `css/css-ui/box-sizing-007…025.html` (19 reftest, все `<img>` с `box-sizing` и `min-/max-`), а также
`css/css-sizing`, `css/CSS2/visudet` — не прогонялись, но форма та же.

## Срез 16 (2026-10-07, P2, WPT-RUN-14 `css/css-sizing`)

Тот же корень у `<canvas>`: размеры атрибутов `width`/`height` (по умолчанию 300×150) подставляются в оба измерения, и сторона из CSS не масштабирует вторую. `--dump-layout`: `<canvas width=15 height=15 style="width:20px;height:auto">` → 20×**15** (ожидается 20×20); `<canvas width=15 height=15 style="aspect-ratio:2;width:20px">` → 20×15 (20×10); `<canvas width=10 height=10 style="height:100%">` в `height:100px` → 10×100 (100×100); `<img style="height:20px">` → 60×20 (20×20). `<img style="aspect-ratio:2;width:20px;height:auto">` — 20×10, верно.

WPT: `css-sizing/aspect-ratio/replaced-element-0*` (44 id, 156 из 189 сабтестов), `intrinsic-percent-replaced-0*` (30 id, 29 thick), `replaced-aspect-ratio-*`, `image-min-max-content-intrinsic-size-change-*` (8), `box-sizing-replaced-*` (3), `svg-intrinsic-size-*` — всего кластер «замещаемые» среза: 72 + 44 = 116 id.
