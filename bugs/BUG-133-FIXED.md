# BUG-133

**Статус:** FIXED 2026-06-12
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

INTERACTION TEST-102 (opacity×z-index) 17.04%→0.00%: femtovg PushOpacity применял set_global_alpha per-draw вместо групповой offscreen-композиции — двойной бленд перекрытий, просвечивание negative-z сквозь сиблингов, вложенная opacity заменялась вместо умножения; теперь offscreen-слой (FLIP_Y) + один композит с групповой alpha
