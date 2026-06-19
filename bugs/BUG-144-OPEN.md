# BUG-144

**Статус:** OPEN (частично исправлен — row-flip)
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

CSS filter / backdrop-filter visual rendering (TEST-30).

## Исправлено 2026-06-17 — row-flip backdrop-filter (16.42% → 10.48%)

Карточки `backdrop-filter` (row 4) рисовались в неверном ряду:
элемент с bounds `y=439, h=102` в вьюпорте 718px появлялся на `y≈177`
(`718 − (439+102) = 177` — чистый вертикальный флип). Причина: `elem_id` —
GPU-FBO, в который рендерится содержимое элемента и который затем сэмплируется
как `Paint::image` в `composite_backdrop_filter_layer`. Он создавался с одним
лишь `PREMULTIPLIED`, без `FLIP_Y`, поэтому bottom-up строки FBO сэмплировались
вверх ногами (как opacity/filter offscreen-слои до BUG-133/BUG-146). Фикс:
`elem_id` создаётся через `offscreen_layer_image_flags()` (`PREMULTIPLIED |
FLIP_Y`). `filtered_backdrop_id` остаётся без флага — это CPU-upload (top-down).
В Lumen `backdrop-filter` всегда внутри offscreen-слоя (требование `from_level
>= 2`), так что `prev_render_target` — всегда FBO, и флип нужен всегда.

Тест: `offscreen_layer_flags_flip_y_and_premultiplied` (расширен doc).
TEST-30 → KNOWN_DEBTORS (`BUG-144`, 10.5).

## Gradient hard-stop (row 2) — исправлено 2026-06-19 (BUG-085, 10.48% → 7.56%)

`linear-gradient(to right, #e53e3e 50%, #38a169 50%)` рисовал только красную
половину — femtovg не дозаполнял хвост за последним стопом. Фикс в
`femtovg_stops` (см. BUG-085): последний цвет продлевается до 1.0.

## Остаток (DEBTOR, 7.56%)

1. **Filter pixel-parity (rows 1-3):** grayscale/sepia/brightness/invert/
   contrast/saturate/hue-rotate/blur не совпадают с Edge пиксель-в-пиксель.
2. **Backdrop захватывается тёмным (row 4):** карточки с `backdrop-filter`
   показывают тёмный фон вместо отфильтрованного градиента — `screenshot()`
   внутри opacity-FBO не отдаёт содержимое слоя (отдельный дефект PA-4).
