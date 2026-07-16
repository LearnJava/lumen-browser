# BUG-273 — Слайдшоу при прокрутке страниц с mix-blend-mode / filter / backdrop-filter (1.5 FPS)

**Статус:** OPEN — срез 1 (viewport-culling офscreen-групп) влит 2026-07-16; остаются срезы 2 (кэш слоя) и 3 (GPU-композитинг)
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

## Инструменты (влиты вместе с этим багом, остаются в коде)

- `LUMEN_FRAME_LOG=1` — покадровый лог: `[frame] paint …` (femtovg: content/overlay/
  flush/swap + число команд) и `[frame] total …` (шелл: весь RedrawRequested).
  `LUMEN_FRAME_LOG=2` — дополнительно `[frame] top: …` — top-8 типов DisplayCommand
  по времени за кадр. Читается один раз за процесс (`lumen_paint::frame_log_level`).
- `python scripts/scroll_perf.py [page] [--ticks N] [--delta PX]` — поднимает живое
  окно через `--mcp-live-port`, эмулирует прокрутку колесом вниз-вверх, собирает
  `[frame]`-лог и печатает avg/p50/max времени кадра и эффективный FPS.
