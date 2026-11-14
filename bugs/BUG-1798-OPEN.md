# BUG-1798 — `canvas.drawElementImage` — заглушка, не рендерит элемент

**Статус:** OPEN (срез 1: ядро реализовано; срез 2: остаток среза 1 снят, хвост WICG — см. «Остаток»)
**Заведён:** 2026-11 (по строке `BUGS.md`; файл создан при ревизии очередей P5 2026-11-12 — ссылка из `BUGS.md` была битой)
**Область:** js (`crates/js/src/canvas2d.rs`, `crates/js/src/shim/web_api_shim_mid.js` — `drawElementImage`), layout (`layoutsubtree`), paint (`rasterize_cpu_element_image`)

## Суть

`canvas.drawElementImage(element, dx, dy)` — заглушка: только помечает canvas tainted/dirty, не рендерит элемент.

## Сделано (срез 2, 2026-11-14)

Снят остаток срез 1 — `css-backdrop-filter-and-mask` 200 px@2:

- **Корень найден не там, где искали.** Кривая градиентной маски, ветки композита backdrop-filter
  (`apply_backdrop_to_stack`: `top=1 base_len=1 src_idx=0` на обеих сторонах) и сам растр element
  image — байт-в-байт одинаковы между страничным рендером и растеризацией поддерева. Дифф вносил
  **блит в canvas-битмап**: `Context2D::draw_image_cropped` (`crates/engine/canvas/src/lib.rs`)
  семплировал источник через `floor((dy/dest_h)·sh)`, и f32-обратный ход `(53/100)·100 = 52.999996`
  клал строку 53 на строку 52 (и 59 на 58) — canvas-битмап дублировал две строки растра, страница
  показывала значения предыдущей строки (±2/255 на 200 пикселях).
- **Починка** — снап до `floor`: `(src + 1e-3).floor()` для обеих осей; реальная дробная выборка
  сдвигается ≤ 0.001 px, NEAREST-семплинг этого не различает. Регресс-тест
  `draw_image_cropped_1to1_preserves_every_row` (lumen-canvas) — каждая строка 1:1-блита обязана
  сохранить индекс; без снапа падает на строках 53/59.
- **guard** в lumen-paint (`backdrop_mask_page_render_matches_element_image`): страничный рендер той
  же команды mask+backdrop-filter обязан совпадать с element-image-растром попиксельно — сюда
  диагностический поиск вышел бы сразу.
- Попутно закрыты: **BUG-1806** (TEST-18/TEST-61 graphic-гейта — те же divergent-строки 1:1-блита;
  оба вернулись к 0.00 %) и последний остаток **BUG-1777** (reftest теперь PASS байт-в-байт).
- Полная категория `draw-element-image/` (89 тестов): 131 expected / 29 unexpected — все 29 из
  документированного хвоста WICG ниже; новых unexpected нет. Ожидания `.ini` обновлены под
  наблюдённое состояние: 4 вложенных testharness-теста (moved-nested, nested-div, triple-nested,
  intersection-observer-visibility) PASS — убран `expected: FAIL`; `nested-layoutsubtree-canvas` —
  стабильный TIMEOUT (promise теста не заселяется при рабочем nested `requestPaint`; висит на
  onpaint-хвосте), `expected: FAIL` → `expected: TIMEOUT` (файл + подтест).

## Сделано (срез 1, 2026-11-14)

Полный конвейер «subtree → display list → CPU-растр → blit»:

1. **`layoutsubtree` (layout).** `BoxKind::Canvas` получил поле `layout_subtree`: у `<canvas layoutsubtree>` fallback-дети строятся боками (`box_tree/build.rs`) и раскладываются как блок-флоу внутри replaced-бокса; UA `display` такого канваса — `inline-block` (`style/ua.rs`), чтобы блочные дети не провоцировали block-in-inline сплит replaced-бокса. Без атрибута — прежнее поведение (детей нет). Дети не красятся на странице и не hit-test-ятся: скип в `StackingTree::build` (`stacking.rs`), bucket-обходе `fill_buckets_cached` (`box_layer.rs` — синхронно по SC-id, иначе паника индексов) и обоих хит-тестах (`hit_test.rs`) — `layoutsubtree-no-hit-test` 3/3. `paint_eq`/`incremental` сравнивают новое поле.
2. **Растеризатор (paint).** `cpu_raster::rasterize_cpu_element_image(w, h, cmds, images)` — прозрачное полотно (`clear = Some(Color{a:0})` через `rasterize_cpu_with_fonts`), прямая (не премультиплицированная) альфа на выходе. Попутно закрыт BUG-1799: тонкая обёртка `rasterize_cpu` удалена, её ~45 тестовых вызовов и два вызова `frame_entry.rs` переведены на `rasterize_cpu_with_fonts(..., None, None)` (байт-в-байт та же семантика); clippy `-p lumen-paint --all-targets -D warnings` зелёный.
3. **Натив (js).** `_lumen_canvas2d_draw_element_image`: same-tick flush, свежее дерево по правилу BUG-1202 (`hit_tree_stale` → `incr_basis.layout`, иначе пушнутый `hit_test_tree`), первый по pre-order бокс с `node == nid`; display list — упорядоченным трио (`StackingTree::build` → `PaintOrder::from_tree` → `build_display_list_ordered`) над `LayoutBox::synthetic_paint_root` (новый конструктор в layout: `StackingTree::build` не оценивает триггеры корня, а нарисованный элемент несёт их сам — opacity/mask на самом элементе без обёртки пропадали), команды сдвигаются `translated(-x, -y)` к началу бокса; растр → `draw_image_cropped(dx, dy)` + `taint()` + dirty. Инсталлятор принимает `Option<FlushHandles>` + дерево; рантайм хранит handles последнего `install_dom` в `page_flush_handles` (блок `run` недоступен снаружи). Декодированные `<img>`-битмапы не передаются — внутри картинки растровые изображения рисуют заглушку (`shortcut`).
4. **Шим.** `HTMLCanvasElement.prototype.requestPaint()` — `paint`-событие задачей нулевой задержки (`waitForCanvasPaint` больше не виснет); нечисловые `dx/dy` молча игнорируются (WebIDL).

## Проверка

- Срез 2: `css-backdrop-filter-and-mask` — **PASS** (smoke, байт-в-байт с референсом; на срезе 1 —
  FAIL 200 px@2). Полная категория 89 тестов: 131 expected / 29 unexpected = только хвост WICG.
  Graphic-гейт 172 теста (IPC-hybrid, `results/20261114-102104.json`): новых FAIL нет, diff%
  остальных 22 совпадают со срезом 1 до 4 знака, TEST-18/TEST-61 — 0.00 % (было 6.09/10.82).
- Срез 1: `run_smoke.py` по категории `draw-element-image/`: **24 PASS** (было 0), среди них цель
  BUG-1777 `css-backdrop-filter-and-mask` — пиксельная разница сведена с 9900 px@200 до **200 px@2**
  (fuzzy теста: 3900 px@1 — счётчик проходит, не проходит по max-разнице, см. срез 2), `basic-rect`
  — байт-в-байт с эталоном, `css-backdrop-filter-on-div`, `element-image-fractional-size`,
  `error-conditions`, `draw-element-image-empty`, `layoutsubtree-no-hit-test` 3/3,
  `draw-element-image-zero-size`, `drawing-display-none-fails` и др.
- Юнит: `element_image_is_transparent_with_straight_alpha` (paint), 34 canvas2d-теста js,
  `draw_image_cropped_1to1_preserves_every_row` (canvas), `backdrop_mask_page_render_matches_element_image` (paint).

## Остаток

- **Семейства WICG-длинного хвоста (29 unexpected полной категории):** `onpaint-*` (спецификационные
  `requestPaint`/инвалидация/`changedElements`; сюда же — стабильный TIMEOUT
  `nested-layoutsubtree-canvas`: promise теста не заселяется при рабочем nested `requestPaint`),
  `scale`/`*-zoom` (растр 1:1 CSS px без DPR/масштаба), `blend`/`compositing-*`, `iframe*` (захват
  чужого фрейма), `element-shadow*`/`shadow-*` (теневые деревья), `top-layer*`, `svg-*`,
  `selection-*`, `percent-sizing`, `nested-canvas-invalidation`, `get-element-transform`,
  `requestPaint`-варианты, `css-*-{opacity,mask,transform,clip-path}-on-div` (см. BUG-1810),
  `ink-overflow`, `non-opaque-element`, `opacity-animation`, `anchor-positioning`,
  `changes/changing-size-in-paint-event`, `clips-overflow`. Это отдельные срезы: упорядоченный хвост
  требует либо того же trio с DPR, либо shell-side IPC-захвата.

## Прежняя формулировка (до среза 1)

Реальная реализация: найти LayoutBox по `__nid__`, построить display list поддерева, отрендерить в offscreen bitmap (CPU rasterizer), нарисовать на canvas. Требует передачи layout root в JS-поток или shell-side callback (IPC bitmap).

## Проверка (исходная)

WPT: `tests/wpt/html/canvas/element/manual/draw-element-image/*.tentative.html` — все падали.
