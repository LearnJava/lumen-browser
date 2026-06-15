# BUG-056

**Статус:** FIXED 2026-06-03
**Компонент:** shell
**Файл:** `crates/shell/src/main.rs:2369`

## Описание

font_registry used after move in parse_and_layout: clippy E0382 — font_registry перемещался в Arc::new() до последнего использования face_bytes_for_family в for-loop. Fix: move в Arc после цикла (shell/main.rs:2397-2398). Verified: workspace-clippy зелёный (P5 health-свип 2026-06-03).
