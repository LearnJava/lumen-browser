# BUG-235

**Статус:** FIXED 2026-06-23
**Компонент:** paint (build/clippy)
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

`bg_tile_geometry` (вынесена в `display_list.rs` как `pub(crate)` в ходе
p1-cpu-bg-image) имеет вызовы только в femtovg-бэкенде (`backend-femtovg`) и
CPU-растеризаторе (`cpu-render`). При сборке `lumen-paint` под одной лишь
`backend-wgpu` (дефолт `lumen-driver`) у функции нет живого вызова →
`cargo clippy -D warnings` падает с «function `bg_tile_geometry` is never used».
Пред-существующий дефект (на чистом main `50273319`), блокировал workspace-clippy.

## Как починено

`#[cfg(any(feature = "backend-femtovg", feature = "cpu-render"))]` на функции —
она компилируется только для бэкендов-потребителей; wgpu-only сборка её не видит.
wgpu-renderer тайлит фоны на GPU и `bg_tile_geometry` не использует.
