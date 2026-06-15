# BUG-088

**Статус:** FIXED 2026-06-12
**Компонент:** css-parser/layout
**Файл:** `crates/engine/layout/src/style.rs:10832–10866, property_trees.rs:679–687`

## Описание

individual CSS transform properties (translate/rotate/scale) rendering diverges — TEST-46: 4.63% (improved from 9.57%); code fully implemented in apply_declaration + property_trees, remaining gap is rasterization-quality (antialiasing + pixel-snapping scope). Individual properties correctly parsed, composed in order (translate×rotate×scale), applied via PushTransform. Classified as Phase 4+ task (antialiasing refinement), not implementation-gap.
