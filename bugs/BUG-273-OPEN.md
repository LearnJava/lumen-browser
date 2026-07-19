# BUG-273 — Слайдшоу при прокрутке страниц с mix-blend-mode / filter / backdrop-filter (1.5 FPS)

**Статус:** OPEN — срез 1 (viewport-culling офscreen-групп) влит 2026-07-16; срез 2 (межкадровый кэш bbox-filter-слоёв) влит 2026-07-20; остаётся срез 3 (GPU-композитинг, Phase 3)
**Компонент:** paint (femtovg backend, offscreen-композитинг blend/filter-слоёв)
**Найден:** 2026-07-08, живое окно + `scripts/scroll_perf.py` (новый инструмент, см. ниже)

## Симптом

Прокрутка страницы с элементами `mix-blend-mode` / `filter` / `backdrop-filter` — слайдшоу.
На `graphic_tests/1000000-final.html` (1070 команд display list): **~660 мс на кадр, 1.5 FPS**.
Baseline `samples/page.html` без таких элементов: 4.6 мс/кадр, 200+ FPS — сам объём
display list не проблема.

## Диагностика

Покадровый профиль (`LUMEN_FRAME_LOG=2`, разбивка по типам команд, типичный кадр из ~600 мс):

```
PopBlendMode        388 мс / 5 команд   (~78 мс на слой!)
PushBlendMode       196 мс / 5
PopFilter           110–175 мс / 30
PushBackdropFilter   43–60 мс / 5
— всё остальное (~1000 команд: текст, бордеры, градиенты) — ~10 мс суммарно
```

## Корень

Три усилителя, перемножающиеся друг на друга:

1. **CPU-композитинг через GPU-readback.** `composite_blend_layer` /
   `composite_filter_layer` (femtovg_backend.rs) на **каждый** blend/filter-слой на
   **каждом кадре** делают: `canvas.flush()` (сброс GL-батча) → `canvas.screenshot()`
   (= `glReadPixels` всего framebuffer, синхронная остановка GPU-конвейера) →
   попиксельный float-цикл на CPU (unpremultiply → mix_blend_rgba → premultiply,
   ~800k пикселей) → `create_image` + upload результата обратно. Итого ~78 мс на слой.

2. **Нет viewport-culling.** `FemtovgBackend::render()` исполняет весь display list
   страницы; blend/filter-элементы, находящиеся за тысячи пикселей от вьюпорта,
   оплачиваются полностью на каждом кадре прокрутки, хотя не видны.

3. **Нет межкадрового кэша слоёв.** При чистой прокрутке содержимое страницы не
   меняется (сдвигается только translate), но каждый слой пересчитывается с нуля.
   Модули `display_list_cache` / `layer_cache` в lumen-paint существуют, но
   femtovg-бэкендом не используются.

## Направления фикса (по соотношению эффект/усилие)

1. **Culling по bounds (срез 1).** `PushFilter { bounds: Option<Rect> }` и
   `PushBackdropFilter { bounds: Rect }` уже несут границы; для PushBlendMode их можно
   вычислять эмиттером. Если границы слоя (с учётом scroll) не пересекают viewport —
   пропустить всю группу Push…Pop без offscreen/readback. На тест-странице закрывает
   большинство слоёв: видимыми одновременно остаются 1–2.
2. **Кэш результата слоя (срез 2).** Ключ = content-hash поддерева + положение
   относительно backdrop; при чистом скролле пересчитывать только слои, впервые
   вошедшие во viewport. Для `filter` (не зависит от backdrop) кэш тривиален.
3. **GPU-композитинг (Phase 3).** femtovg не даёт programmable blending —
   полноценный фикс = blend/filter шейдерами в wgpu/vello-бэкенде.

## Срез 1 — viewport-culling офscreen-групп (влит 2026-07-16)

`FemtovgBackend::run_content_pass` теперь проверяет каждую `PushFilter`/
`PushBackdropFilter`/`PushBlendMode`-группу целиком, а не только листовые draw-команды
внутри неё (ADR-016 M0.2 покрывал только последние). Новый `group_bounds(cmd)`
достаёт document-space bbox группы (`PushFilter.bounds: Option<Rect>`,
`PushBackdropFilter.bounds`/`PushBlendMode.bounds: Rect` — оба поля были обязательными
и раньше, `PushBlendMode` получил `bounds` этим срезом, эмиттеры заполняют его из
`LayoutBox::rect`). Если `Self::is_command_culled(bounds)` (тот же тест, что уже
использует ADR-016 M0.2 для листьев — transform-aware AABB против viewport + 256px
slop) возвращает `true`, вся скобка Push…Pop целиком пропускается через
`matching_close` (depth-счётчик на `overlay_partition::layer_delta`, теперь
`pub(crate)`) — ни `acquire_layer`, ни дочерние draw-команды, ни CPU-readback
composite (`composite_blend_layer`/`composite_filter_layer`/`apply_backdrop_filters`)
не выполняются для контента, который всё равно не дал бы видимого пикселя в этом
кадре. Закрывает направление 2 из «Корня» (нет viewport-culling для
offscreen-композит-групп — ADR-016 M0.2 покрывал только листья).

Корректность: 2 новых юнит-теста (`group_bounds_covers_the_three_offscreen_openers`,
`matching_close_skips_nested_brackets`); `cargo test -p lumen-paint --features
backend-femtovg` 926 passed, 0 failed; `cargo clippy -p lumen-paint --all-targets
--features backend-femtovg -- -D warnings` чист. Графические тесты 00/03/30/56/103
(`LUMEN_BACKEND=femtovg`, dev-release) дают числа, побитово совпадающие с main
(4.27%/12.41%/0.03% — идентично прогону без этого среза) — визуальных регрессий нет;
расхождение этих чисел с `KNOWN_DEBTORS`-записями (которые сейчас откалиброваны под
wgpu после BUG-287, а не под femtovg) — отдельный, не связанный с этим срезом дрейф.

Не устраняет: срез только пропускает уже-невидимые группы; видимые (даже частично)
offscreen-группы по-прежнему платят полный CPU-readback-composite каждый кадр —
остаётся направлениям 2 (межкадровый кэш слоя) и 3 (GPU-композитинг, Phase 3) ниже.

## Срез 2 — межкадровый кэш bbox-filter-слоёв (влит 2026-07-20)

Направление 2 «Кэш результата слоя», но строго в **безопасном подмножестве**: только
`PushFilter`-группы с colour-matrix-цепочкой **без blur** (`grayscale`/`sepia`/
`brightness`/`invert`/`contrast`/`saturate`/`hue-rotate`, `filter_is_bbox_cacheable`).
Это ровно тот путь, где `PushFilter` берёт bbox-слой (срез 14) и
`composite_filter_layer` платит дорогой `screenshot()` (GPU-readback) + попиксельный
CPU colour-matrix-цикл + re-upload на каждом кадре. Blur-цепочки остаются
full-framebuffer (их пиксели **не** scroll-инвариантны — контент запечён в
screen-space, см. `FilterLayerEntry::bbox`), backdrop-filter/blend не трогаются
(backdrop-зависимы — не направление 2).

**Почему кэшируемо.** У bbox-пути Push делает `translate(-x0/scale, -y0/scale)` в
локальное пространство bbox, поэтому пиксели слоя scroll-инвариантны: при прокрутке
меняется только экранная позиция `(x0, y0)`, пересчитываемая заново каждый кадр из
текущего transform. Ключ кэша — content-hash скобки `PushFilter…PopFilter`
(`hash_command_into`, тот же примитив, что у `hash_content`; scroll в команды не
запекается, поэтому хэш стабилен при прокрутке). Хранится готовая (post-filter)
CPU-upload-текстура (`filter_layer_cache: HashMap<u64, CachedFilterLayer>`), живущая
между кадрами вне scratch-пулов — как `retained_band`.

**Корректность.** Попадание требует совпадения не только хэша, но и device-размеров
`(w, h)` **и** sub-device-pixel фазы (`FilterCacheMeta`, восьмые доли device px):
частично-видимая группа имеет clamped-размеры, дрейфующие покадрово → промах →
перерисовка; фаза гарантирует, что переиспользованная текстура пиксель-в-пиксель
совпадает со свежим рендером (при целочисленном по device скролле — обычный случай на
scale 1.0 — фаза постоянна). Блит на попадании (`blit_cached_filter_layer`) — точная
копия Step 4 `composite_filter_layer` (`reset_transform` + image-fill в device-rect),
так что hit ≡ miss по построению. Store в хвосте `composite_filter_layer` происходит
**после** Step-4-блита, поэтому кадр-промах побитово совпадает с прежним поведением.
Инвалидация — в `invalidate_scroll_cache` (resize/DPI/навигация/докачка картинки —
события, меняющие пиксели без смены content-hash), в lock-step со scroll-blit-полосой;
чистая прокрутка её не вызывает → кэш переживает скролл. Kill-switch
`LUMEN_NO_FILTER_CACHE=1`. LRU-эвикция по бюджету 32 МБ / 128 записей, эвиктнутые
текстуры — в `filter_layer_pending_delete` (удаление после flush).

Измерения (`scripts/scroll_perf.py`, `LUMEN_BACKEND=femtovg`, dev-release,
синтетическая страница из 10 colour-matrix-карточек, delta 40px). При **выключенном**
scroll-blit (`LUMEN_SCROLL_BLIT=0`, каждый кадр — полный Repaint, чтобы изолировать
эффект): медиана кадра **124.2 мс → 100.1 мс (−19%)**; 40 кадров с попаданиями, 143
попадания; PopFilter на кадре-промахе ~106 мс/16 → ~35–52 мс на кадрах с 3–4
попаданиями (composite для закэшированных групп пропущен). При **включённом**
scroll-blit (default) выигрыш реализуется только на Repaint-кадрах при пересечении
границ полосы — чистую in-band-прокрутку уже покрывает ADR-016 M3 blit-полоса (медиана
~0.6 мс), поэтому на смешанной `1000000-final.html` (blur + backdrop-filter доминируют,
вне подмножества среза) медиана без изменений (регрессии нет: 3.80 vs 3.92 мс).

Диагностика: `LUMEN_FRAME_LOG=2` теперь печатает `filter cache hits: N` в строке
`[frame] top:`.

Не устраняет: blur/backdrop-filter/blend composite (scroll-вариантны или
backdrop-зависимы) и первое появление каждой группы — остаётся направлению 3
(GPU-композитинг шейдерами, Phase 3).

## Инструменты (влиты вместе с этим багом, остаются в коде)

- `LUMEN_FRAME_LOG=1` — покадровый лог: `[frame] paint …` (femtovg: content/overlay/
  flush/swap + число команд) и `[frame] total …` (шелл: весь RedrawRequested).
  `LUMEN_FRAME_LOG=2` — дополнительно `[frame] top: …` — top-8 типов DisplayCommand
  по времени за кадр. Читается один раз за процесс (`lumen_paint::frame_log_level`).
- `python scripts/scroll_perf.py [page] [--ticks N] [--delta PX]` — поднимает живое
  окно через `--mcp-live-port`, эмулирует прокрутку колесом вниз-вверх, собирает
  `[frame]`-лог и печатает avg/p50/max времени кадра и эффективный FPS.
