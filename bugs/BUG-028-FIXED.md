# BUG-028

**Статус:** FIXED 2026-05-26
**Компонент:** shell
**Файл:** `crates/shell/src/main.rs`

## Описание

relayout-on-resize + maximized window triggers BUG-027

## Детали

Окно открывается максимизированным, winit сразу стреляет `Resized(~1920×1040)`. `relayout()` пересчитывает с viewport 1920px → BUG-027 проявляется.

**Фикс:** 1) guard в `WindowEvent::Resized` — skip при `size == 0` (минимизация на Windows); 2) defensive guard в `relayout()` при `vp_size <= 0`; 3) BUG-027 FIXED — explicit width больше не игнорируется при любом viewport. Временная мера (убрать `with_maximized`) оставлена: окно стартует 1024×720 для корректной работы графических тестов.

**Компонент:** `lumen-shell` — `Lumen::relayout()` + `WindowEvent::Resized` handler
