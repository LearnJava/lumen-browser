# BUG-134

**Статус:** FIXED 2026-06-15
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

INTERACTION TEST-103 (filter×transform): ложная регрессия — 29.11% измерены на устаревшем бинаре (прогон cf54c92d). Свежая сборка от 46c1605c даёт PASS 0.04% (повторно проверено вручную --only 103). Реально закрыт каскадом BUG-146 (FLIP_Y слоя фильтра) + последующими фиксами femtovg.
