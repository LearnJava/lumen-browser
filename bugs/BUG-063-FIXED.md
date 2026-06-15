# BUG-063

**Статус:** FIXED 2026-06-04
**Компонент:** layout
**Файл:** `crates/engine/layout/src/mathml.rs:88`

## Описание

clippy: manual_clamp → scale.clamp(), удалён #[expect(dead_code)], collapsible_if схлопнуты, unneeded_struct_pattern убран
