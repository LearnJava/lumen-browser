# BUG-145

**Статус:** FIXED 2026-06-12
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

РЕГРЕССИЯ после мержей P2 2026-06-12 (9d691996 PushFilter bounds, BUG-076): TEST-30 18.81%→30.68%, TEST-103 7.33%→49.59% — offscreen-слой фильтра сайзился по bounds, но контент рисуется в page-координатах и composite_filter_layer композитит слой полноэкранным квадом → угол страницы растягивался на весь viewport. Fix: слой снова полноразмерный (bounds игнорируется). TEST-30 17.53%, TEST-103 7.33%, TEST-15 6.58% (без изменений)
