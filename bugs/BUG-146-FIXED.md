# BUG-146

**Статус:** FIXED 2026-06-12
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

TEST-15 box-shadow регрессия 1.06%→6.58% — корень: PA-2 (мерж 085d5b8d, не PA-3/PA-4/BUG-123; ночные прогоны 1.06% гоняли устаревший бинарь). Blur-only цепочка composite_filter_layer композитит blur-FBO напрямую на GPU без screenshot-round-trip, без FLIP_Y тени рисовались вертикально зеркально внизу страницы. Fix: FLIP_Y на слое фильтра и blur-destination (filter_image ориентацию памяти сохраняет). TEST-15 0.00%, TEST-30 18.81→16.42%, TEST-103 7.33→3.15%, TEST-49 без изменений
