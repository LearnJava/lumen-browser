# BUG-120

**Статус:** FIXED 2026-06-10
**Компонент:** layout/text
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

C0 control chars (e.g. U+0001) in body text render as a visible 1-line text box (19.2px line at 16px font) — Edge/Chromium renders them invisible/zero-advance, no line box. Divergence discovered via BUG-119 (corrupted test pages shifted content 20px in Lumen but not in Edge). Fix: invisible Cc (except tab/LF/CR) stripped at inline-segment level (`is_invisible_control`/`strip_invisible_controls`); control-only text no longer opens an inline run.
