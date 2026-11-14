# BUG-1798 — `canvas.drawElementImage` — заглушка, не рендерит элемент

**Статус:** OPEN (срез 1: ядро реализовано, базовые тесты зелёные; длинный хвост WICG — см. «Остаток»)
**Заведён:** 2026-11 (по строке `BUGS.md`; файл создан при ревизии очередей P5 2026-11-12 — ссылка из `BUGS.md` была битой)
**Область:** js (`crates/js/src/canvas2d.rs`, `crates/js/src/shim/web_api_shim_mid.js` — `drawElementImage`), layout (`layoutsubtree`), paint (`rasterize_cpu_element_image`)

## Суть

`canvas.drawElementImage(element, dx, dy)` — заглушка: только помечает canvas tainted/dirty, не рендерит элемент.

## Сделано (срез 1, 2026-11-14)

Полный конвейер «subtree → display list → CPU-растр → blit»:

1. **`layoutsubtree` (layout).** `BoxKind::Canvas` получил поле `layout_subtree`: у `<canvas layoutsubtree>` fallback-дети строятся боками (`box_tree/build.rs`) и раскладываются как блок-флоу внутри replaced-бокса; UA `display` такого канваса — `inline-block` (`style/ua.rs`), чтобы блочные дети не провоцировали block-in-inline сплит replaced-бокса. Без атрибута — прежнее поведение (детей нет). Дети не красятся на странице и не hit-test-ятся: скип в `StackingTree::build` (`stacking.rs`), bucket-обходе `fill_buckets_cached` (`box_layer.rs` — синхронно по SC-id, иначе паника индексов) и обоих хит-тестах (`hit_test.rs`) — `layoutsubtree-no-hit-test` 3/3. `paint_eq`/`incremental` сравнивают новое поле.
2. **Растеризатор (paint).** `cpu_raster::rasterize_cpu_element_image(w, h, cmds, images)` — прозрачное полотно (`clear = Some(Color{a:0})` через `rasterize_cpu_with_fonts`), прямая (не премультиплицированная) альфа на выходе. Попутно закрыт BUG-1799: тонкая обёртка `rasterize_cpu` удалена, её ~45 тестовых вызовов и два вызова `frame_entry.rs` переведены на `rasterize_cpu_with_fonts(..., None, None)` (байт-в-байт та же семантика); clippy `-p lumen-paint --all-targets -D warnings` зелёный.
3. **Натив (js).** `_lumen_canvas2d_draw_element_image`: same-tick flush, свежее дерево по правилу BUG-1202 (`hit_tree_stale` → `incr_basis.layout`, иначе пушнутый `hit_test_tree`), первый по pre-order бокс с `node == nid`; display list — упорядоченным трио (`StackingTree::build` → `PaintOrder::from_tree` → `build_display_list_ordered`) над `LayoutBox::synthetic_paint_root` (новый конструктор в layout: `StackingTree::build` не оценивает триггеры корня, а нарисованный элемент несёт их сам — opacity/mask на самом элементе без обёртки пропадали), команды сдвигаются `translated(-x, -y)` к началу бокса; растр → `draw_image_cropped(dx, dy)` + `taint()` + dirty. Инсталлятор принимает `Option<FlushHandles>` + дерево; рантайм хранит handles последнего `install_dom` в `page_flush_handles` (блок `run` недоступен снаружи). Декодированные `<img>`-битмапы не передаются — внутри картинки растровые изображения рисуют заглушку (`shortcut`).
4. **Шим.** `HTMLCanvasElement.prototype.requestPaint()` — `paint`-событие задачей нулевой задержки (`waitForCanvasPaint` больше не виснет); нечисловые `dx/dy` молча игнорируются (WebIDL).

## Проверка

- `run_smoke.py` по категории `draw-element-image/`: **24 PASS** (было 0), среди них цель BUG-1777 `css-backdrop-filter-and-mask` — пиксельная разница сведена с 9900 px@200 до **200 px@2** (fuzzy теста: 3900 px@1 — счётчик проходит, не проходит по max-разнице, см. «Остаток»), `basic-rect` — байт-в-байт с эталоном, `css-backdrop-filter-on-div`, `element-image-fractional-size`, `error-conditions`, `draw-element-image-empty`, `layoutsubtree-no-hit-test` 3/3, `draw-element-image-zero-size`, `drawing-display-none-fails` и др.
- Юнит: `element_image_is_transparent_with_straight_alpha` (paint), 34 canvas2d-теста js.

## Остаток

- **`css-backdrop-filter-and-mask`: 200 px@2** (лимит 1). Остаточная полоса ±2/255 высотой ~7 строк в зоне градиентной маски — расхождение округления композита градиентной маски между растром 100×100 и страничным полотном; нужен либо шаг точности в `MaskSpec`-композите, либо пересмотр fuzzy.
- **`css-opacity-on-div`/`css-mask-on-div`: эталонная сторона** — дефект страницы, не фичи: `box_can_own_stacking_context` (`stacking.rs`) не содержит `Canvas`/`Video`/`Iframe`/`Audio`, поэтому own `opacity`/`mask-image` на replaced-боксе молча игнорируются (реф канвас с `opacity: 0.5` рисуется непрозрачным). Завести отдельным багом.
- **Семейства WICG-длинного хвоста:** `onpaint-*` (спецификационные `requestPaint`/инвалидация/`changedElements`), `scale`/`*-zoom` (растр 1:1 CSS px без DPR/масштаба), `blend`/`compositing-*`, `iframe*` (захват чужого фрейма), `element-shadow*`/`shadow-*` (теневые деревья), `top-layer*`, `svg-*`, `selection-*`, `percent-sizing`, `nested-canvas-invalidation`, `get-element-transform`, `requestPaint`-варианты. Это отдельные срезы: упорядоченный хвост требует либо того же trio с DPR, либо shell-side IPC-захвата.

## Прежняя формулировка (до среза 1)

Реальная реализация: найти LayoutBox по `__nid__`, построить display list поддерева, отрендерить в offscreen bitmap (CPU rasterizer), нарисовать на canvas. Требует передачи layout root в JS-поток или shell-side callback (IPC bitmap).

## Проверка (исходная)

WPT: `tests/wpt/html/canvas/element/manual/draw-element-image/*.tentative.html` — все падали.
