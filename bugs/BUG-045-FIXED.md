# BUG-045

**Статус:** FIXED 2026-05-29
**Компонент:** layout
**Файл:** `layout/src/stacking.rs:201`

## Описание

backdrop-filter не создавал stacking context: creates_stacking_context() проверял filter, но не backdrop_filter (CSS Filter Effects L2 §2) → box_layer_ops дропал PushBackdropFilter, пустой DL для backdrop-only div. Добавлена проверка + regression-тест
