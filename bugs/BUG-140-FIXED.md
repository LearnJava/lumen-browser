# BUG-140

**Статус:** FIXED 2026-06-13
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

INTERACTION TEST-109 (clip-path×transform×radius) 14.10%→4.80%, юнит TEST-31 2.34%→PASS 0.43%. Три дефекта: (1) circle(40%)/polygon(50% …) с процентами молча отбрасывались парсером; (2) clip-path аппроксимировался bounding box-ом (circle→квадрат, polygon→no-op); (3) эмитился СНАРУЖИ PushTransform — клип не переносился transform-ом (CSS Masking L1 §9). Fix: ShapeValue (px/%), PushClipPath{ResolvedClipShape} внутри PushTransform; femtovg — offscreen-слой + fill_path по трансформированному пути; cpu_raster — слой + альфа-покрытие; wgpu fallback — bbox-scissor. Остаток 4.80% — c3: margin-collapse → BUG-151
