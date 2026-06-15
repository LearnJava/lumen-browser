# BUG-096

**Статус:** FIXED 2026-06-09
**Компонент:** paint/layout
**Файл:** `crates/engine/paint/src/display_list.rs:4811 + crates/engine/layout/src/style.rs`

## Описание

SVG `<path>` stroke tessellation not rendered — TEST-54: 9.50%. Two causes: (1) `emit_svg_shape` 0×0 guard dropped every `<path>` (path bbox is deferred to paint, so the box rect is zero) → exempted Path from the guard; (2) SVG presentation attributes (`fill`/`stroke`/`stroke-width` as XML attrs) were never read into ComputedStyle, so `fill="none" stroke="#e94560"` paths painted as black blobs → added `apply_svg_presentational_hints`.
