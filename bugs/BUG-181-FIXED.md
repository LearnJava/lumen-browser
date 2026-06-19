# BUG-181

**Статус:** FIXED 2026-06-20
**Компонент:** layout/paint
**Тест:** TEST-19 (diff 9.05% → KNOWN_DEBTORS, BUG-219)

## Описание

`object-fit` basic — fill/contain/cover/none/scale-down + object-position для `<img>`.

## Расследование

Геометрия object-fit **корректна во всех режимах** — это не дефект placement,
а image-resampling parity. Подтверждено пиксельным анализом Edge-эталона vs
Lumen (femtovg-окно):

- **Средние RGB ячеек совпадают** с Edge: cover-center agi = `(65.5,85.5,87.1)`
  Edge vs `(65.6,85.7,87.1)` Lumen (Δ < 0.2).
- **Лучший сдвиг = (0,0)** для всех 9 ячеек (поиск ±3px не нашёл смещения,
  уменьшающего diff) → нет геометрического сдвига.
- **Letterbox корректен**: contain perceptron (852×725 в 180×120) →
  scale=min(0.211, 0.166)=0.166 → placed 141×120, бары .box-градиента по 19.5px
  слева/справа совпадают с Edge.
- **Cover-кроп корректен**: agi 1024×1024 cover → 180×180, центр-кроп по высоте
  (off_y=-30), object-position left-top/right-bottom/25%-75% смещают вырезку
  как в Edge.

Остаток 9.05% локализован в высокочастотном контенте: тонкие линии/текст
perceptron-диаграммы и rusty-текстура agi-картинки ресэмплятся `resize_area_avg`
(box-фильтр) иначе, чем Edge downscale kernel → 54–63% пикселей agi-ячеек
расходятся на >30/255, при идентичном среднем цвете и масштабе. Та же причина,
что и остаток TEST-18 (BUG-219).

## Решение

- Регресс-тесты `bug181_*` в `crates/engine/paint/src/display_list.rs`
  фиксируют геометрию `fit_image_rect` на реальных размерах теста
  (perceptron 852×725, agi 1024×1024 в боксе 180×120) — чтобы будущий
  placement-регресс не спрятался за resampling-шумом.
- `TEST-19 → KNOWN_DEBTORS` (`graphic_tests/run.py`) с baseline 9.05%,
  ссылка на OPEN BUG-219 (image downscale resampling pixel-parity).

## Воспроизведение

`python graphic_tests/run.py --only 19` → DEBTOR ~9% (BUG-219).
