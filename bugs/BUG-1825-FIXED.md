# BUG-1825 — `position: sticky` не зажат своим containing block (sticky constraint rectangle)

**Статус:** FIXED 2026-11-19 (P6, §Срез 1): кламп по containing block добавлен в геометрию и все четыре paint-пути; `position-sticky-top/left.html` 3/3, `position-sticky-bottom.html` 3/3 (было 2/3), `left-and-right-overconstrained` +2 подтеста. Остаток overconstrained-семьи (приоритет при обоих инсетах) — [BUG-1831](BUG-1831-OPEN.md)
**Открыт:** 2026-11-18 (P3, при BUG-1468 срез 5)
**Область:** layout/paint (`sticky_correction` в `sticky_snap.rs` + `StickyGeom`, `BeginStickyLayer` в `display_list/commands.rs`; хендлеры и локальные зеркала в `renderer.rs`, `renderer/sticky_geom.rs`, `backends/femtovg_backend.rs`, `cpu_raster.rs`, `display_list/box_layer.rs`)

## Симптом

CSS Position 3 §2.2: sticky constraint rectangle = пересечение scrollport и containing block липкого бокса; бокс зажат в нём и не выходит за края containing block. У нас `sticky_dy`/`sticky_dx` (и зеркальная `sticky_correction` в layout) клампят только по инсетам `top`/`bottom`/`left`/`right` и scrollport-у — containing block не учитывается.

Проба (`position-sticky-top.html`, scroller `overflow:hidden` 100×200, `container` 300×300, sticky `top:50px; height:100px`, поток y=200): при `scrollTop=300` `sticky.offsetTop` = 350 (мы) против 300 (эталон) — бокс уезжает за нижний край containing block (`container`, y=100..400): липкий верх обязан быть `min(scrollTop+50, container.bottom−height)` = `min(350, 300)` = 300.

## Срез 1 (P6, 2026-11-19) — кламп во всех пяти местах

Единая функция `lumen_layout::clamp_stuck_to_containing(pos, size, cb_pos, cb_size)` (min по дальнему краю, max по ближнему — ближний выигрывает при слишком коротком cb, как и «top wins» в `compute_sticky_offset`):

- **Геометрия** (`getBoundingClientRect`/`offsetTop`): `StickyGeom` получил `containing: [f32;4]` + `containing_frames_scroller: bool`; `collect_sticky_geoms` тредит cb тем же правилом Block/FlowRoot, что и `collect_sticky_boxes`. Знак сдвига зависит от того, где cb относительно ближайшего скроллера: cb **обрамляет** скроллер (сам скроллер или выше) — его диапазон в пространстве сдвинутого бокса движется на скролл (`+scroll`), cb **внутри** прокручиваемого контента — диапазон документно-статичен (`+0`). Проверено на обеих геометриях: house-тест `#sc{overflow:auto}` (cb=скроллер) требует `+scroll`, проба карточки (cb-контейнер внутри) требует `+0`.
- **Paint**: `BeginStickyLayer` несёт `containing: Rect` (page-координаты). Эмитят `walk.rs::dispatch` (cb тредится через `Frame::sticky_cb`, правило Block/FlowRoot как в collect-обходах), `svg_text_decoration.rs::walk_with_anim` и порядковый сборщик (`box_layer.rs`: `ContinueFrame::sticky_cb` + `sticky_cb_in_content`; `StickyEmit::Page` несёт cb, `Baked` клампится в `sticky_dy/dx`). Потребители: femtovg и wgpu получают cb в пространстве своего застрявшего значения (femtovg — doc, ambient уже несёт скролл; wgpu — screen, хендлер сдвигает на page-скролл), cpu_raster — screen, сдвиг на запечённый page-скролл в хендлере.
- Кламп применяется **только к реально застрявшей оси** (инсет сработал): незастявший бокс скроллится с контентом и легитимно бывает вне диапазона cb (иначе кламп пинал бы бокс, который по инсетам вообще не липнет).

**Ловушка для будущего:** пространство клампа зависит от позиции cb относительно скроллера (см. выше) — это не «doc против screen», а «обрамляет скроллер против внутри контента»; знак разный в геометрии (`+scroll`) и в baked-пути (`−scroll`), потому что у них разные базовые пространства значения.

**Legacy-walk ограничение (не чинено, то же упрощение, что у инсет-клампа):** флаг «cb внутри контента» тредится только в порядковый сборщик; legacy `walk`/`walk_with_anim` эмитят `containing` без сдвига — для page-level sticky (единственный случай, который в прод-пути доходит до рендереров, вложенный sticky там печётся `Baked`) это точно.

## Как проверить

`tests/wpt/css/css-position/sticky/position-sticky-top.html` (3-е утверждение), `position-sticky-bottom.html`, `-left.html`; юнит-тесты `sticky_correction_clamps_to_containing_block_bottom` / `sticky_correction_unstuck_axis_ignores_containing_block` (`sticky_snap.rs`), femtovg/renderer-тесты обновлены под новую сигнатуру. WPT: top/left 3/3, bottom 3/3 (было 2/3), `left-and-right-overconstrained` 3/4 (было 0/4 в батче; стабильно соло 0/4 — см. BUG-1831 про флак). `.ini`-ожидания top/left перевёрнуты.

## Связи

- Найден P3 при BUG-1468 срез 5. Остаток overconstrained-семьи (обо-инсетный приоритет + недетерминизм `left-and-right-overconstrained`) — [BUG-1831](BUG-1831-OPEN.md).
