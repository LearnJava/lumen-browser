# BUG-110

**Статус:** FIXED 2026-06-14
**Компонент:** layout/paint
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

object-fit: SVG viewBox scaling (fill/contain/cover/none/scale-down) ~8% deviation — TEST-70: 8.03%. Two defects: (1) object-fit:fill routed through preserveAspectRatio (letterbox) instead of non-uniform stretch — now always uses compute_object_fit_transform (Edge overrides preserveAspectRatio with object-fit for inline SVG); (2) SVG viewport did not clip overflowing content (cover/oversized viewBox) — added UA-default overflow:hidden clip in both walk and box_layer_ops/ordered paint paths
