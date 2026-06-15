# BUG-060

**Статус:** FIXED 2026-06-04
**Компонент:** font
**Файл:** `crates/engine/font/src/woff2.rs:125`

## Описание

WOFF2-декодер обрывается с «unexpected end of font data» для cnn_sans_condensed-bold.woff2, cnn_sans_condensed-medium.woff2, cnn_sans_display-v1.woff2 — корень: точки контуров читались из glyph_stream вместо nPoints_stream, координаты — из glyph_stream вместо flag_stream; исправлено routing потоков согласно WOFF2 spec §5.3
