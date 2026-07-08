# BUG-273 — Слайдшоу при прокрутке страниц с mix-blend-mode / filter / backdrop-filter (1.5 FPS)

**Статус:** OPEN — корень найден и подтверждён покадровым профилем
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

## Инструменты (влиты вместе с этим багом, остаются в коде)

- `LUMEN_FRAME_LOG=1` — покадровый лог: `[frame] paint …` (femtovg: content/overlay/
  flush/swap + число команд) и `[frame] total …` (шелл: весь RedrawRequested).
  `LUMEN_FRAME_LOG=2` — дополнительно `[frame] top: …` — top-8 типов DisplayCommand
  по времени за кадр. Читается один раз за процесс (`lumen_paint::frame_log_level`).
- `python scripts/scroll_perf.py [page] [--ticks N] [--delta PX]` — поднимает живое
  окно через `--mcp-live-port`, эмулирует прокрутку колесом вниз-вверх, собирает
  `[frame]`-лог и печатает avg/p50/max времени кадра и эффективный FPS.
