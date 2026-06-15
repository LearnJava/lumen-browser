# BUG-039

**Статус:** FIXED 2026-05-26
**Компонент:** paint

## Описание

dashed/dotted border mismatch vs Chrome/Edge: dash ratio 3:1→Skia algo, corner squares→circle quads for dotted, 1px linear SDF AA
