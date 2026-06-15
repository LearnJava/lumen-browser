# BUG-130

**Статус:** FIXED 2026-06-13
**Компонент:** paint
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

view-transition-name: named elements must render identically to un-named (no visual effect outside transition) — TEST-81: 32.47%. Root cause was NOT paint: view-transition-name is parse-only (collect_view_transition_names, no display effect) and never altered rendering. The deviation came entirely from BUG-141 (flex align-items:center cross-size in non-wrap container, FIXED 2026-06-13): TEST-81's three boxes sit in a centered flex row (align-items:center, height 718px) and were pinned to y=1 instead of y=260. --dump-layout confirms all three boxes now at y=260, identical geometry, named ≡ un-named. Regression: vt_name_does_not_affect_layout_geometry (lib.rs). Verified via dump-layout (screenshot pipeline calibration unavailable this session, same as BUG-141).
