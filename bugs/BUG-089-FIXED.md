# BUG-089

**Статус:** FIXED 2026-06-09
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

SVG basic shapes not rendered (rect/circle/ellipse/line) — TEST-47: 21.71%; ordered build path no-op'd SvgRoot/SvgShape/SvgText (only walk painted them)
