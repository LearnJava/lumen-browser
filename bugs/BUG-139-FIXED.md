# BUG-139

**Статус:** FIXED 2026-06-12
**Компонент:** paint
**Файл:** `crates/engine/layout/src/stacking.rs, crates/engine/paint/src/display_list.rs`

## Описание

INTERACTION TEST-108 (вложенные transform) 4.62%: PopTransform родителя эмитировался в PaintPhase::InlineContent до рендера дочерних SC → вложение не работало. Фикс: CloseLayer (фаза 8) в stacking.rs, bucket.post перенесён туда же.
