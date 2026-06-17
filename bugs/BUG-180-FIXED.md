# BUG-180

**Статус:** FIXED 2026-06-17
**Компонент:** layout
**Тест:** TEST-18 (21.21% → 2.11%)

## Описание

`<img>` rendering deviation — каждый ряд картинок в сетке уезжал вверх
относительно Edge, ошибка копилась вниз по странице (~4px на ряд).

## Корень

Голый `<img>` — inline-level replaced-элемент, baseline-выровненный по
умолчанию. Его line-box (а значит и content-height блока-обёртки) опускается
ниже картинки на descent strut'а — классический «image bottom gap» (CSS 2.1
§10.8). Lumen в Phase 0 раскладывает одиночный `<img>` как block-flow ребёнка
(`is_inline_content`/`default_display` мапят img→Block), поэтому это
sub-baseline-пространство терялось: каждый блок-обёртка картинки был на ~descent
px короче, и в сетке `.frame > img` (TEST-18) расхождение копилось как
вертикальный дрейф вверх.

Подтверждено пиксель-замером: Edge рисует img + 3px padding + **4px** тёмного
фона рамки ниже картинки; Lumen — только img + 3px padding.

## Фикс

`box_tree.rs:5527` — после baseline-выровненного replaced-ребёнка
(`Image`/`Video`/`Canvas`/`Iframe`) в блочном потоке добавляем
`measurer.descent_px(b.style.font_size)` к `child_y`. Только для
`vertical-align: baseline`; top/middle/bottom позиционируют replaced-бокс иначе
и gap не получают.

Тесты: `block_with_inline_image_includes_baseline_descent_gap`,
`block_with_top_aligned_image_has_no_descent_gap` (lib.rs).

## Остаток

2.11% = тонкий image-resampling AA по всем фото (ядро downscale у Lumen
(area-avg) ≠ Edge). Структурного дефекта нет (>60-порог diff всего 0.43%).
→ **BUG-219**, TEST-18 в `KNOWN_DEBTORS`.

## Воспроизведение

`python graphic_tests/run.py --only 18 --no-cache` → было FAIL 21.21%,
стало 2.11% (DEBTOR).
