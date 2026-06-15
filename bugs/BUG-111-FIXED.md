# BUG-111

**Статус:** FIXED 2026-06-08
**Компонент:** paint/shell
**Файл:** `crates/engine/paint/src/display_list.rs + crates/shell/src/*`

## Описание

lumen-paint/shell не компилировались после мержа A-2 CSS Custom Highlight API: (1) дубликат `emit_text_with_highlights` (stub 3-arg vs новый 11-arg), (2) 71× `DrawText` struct initializer missing `highlight_name: None` (display_list, renderer, shell/*, main.rs), (3) осиротевший `///`-блок в style.rs, (4) collapsible_if в тест
