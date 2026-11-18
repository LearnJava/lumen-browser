# BUG-1825 — `position: sticky` не зажат своим containing block (sticky constraint rectangle)

**Статус:** OPEN
**Заведён:** 2026-11-18 (P3, при BUG-1468 срез 5)
**Область:** layout/paint (`sticky_dy`/`sticky_dx` в `display_list/box_layer.rs`, `backends/femtovg_backend.rs`, `renderer/sticky_geom.rs`, `cpu_raster.rs`; геометрия — `sticky_snap.rs::sticky_correction`)

## Симптом

CSS Position 3 §2.2: sticky constraint rectangle = пересечение scrollport и containing block липкого бокса; бокс зажат в нём и не выходит за края containing block. У нас `sticky_dy`/`sticky_dx` (и зеркальная `sticky_correction` в layout) клампят только по инсетам `top`/`bottom`/`left`/`right` и scrollport-у — containing block не учитывается.

Проба (`position-sticky-top.html`, scroller `overflow:hidden` 100×200, `container` 300×300, sticky `top:50px; height:100px`, поток y=200): при `scrollTop=300` `sticky.offsetTop` = 350 (мы) против 300 (эталон) — бокс уезжает за нижний край containing block (`container`, y=100..400): липкий верх обязан быть `min(scrollTop+50, container.bottom−height)` = `min(350, 300)` = 300.

## Fix shape

В sticky-алгоритм (все пять мест: 4 paint + геометрия) добавить кламп по containing block — ближайшему block/flow-root предку липкого бокса (`collect_sticky_boxes` в `sticky_snap.rs` уже считает `containing_rect`): по Y липкий верх ∈ [containing_block.top, containing_block.bottom − height], по X — [left, right − width]. Единая функция, не точечная правка — paint и геометрия должны совпадать.

## Как проверить

`tests/wpt/css/css-position/sticky/position-sticky-top.html` (3-е утверждение), `position-sticky-bottom.html`, `position-sticky-constrained-*.html`; A/B через `--screenshot`/`getBoundingClientRect`.
