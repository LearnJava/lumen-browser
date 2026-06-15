# BUG-149

**Статус:** FIXED 2026-06-13
**Компонент:** test/snapshot
**Файл:** `graphic_tests/snapshots/cpu/`

## Описание

snapshot_cpu красный на main (f22e2204, без локальных изменений): 21-border-style — 36780 differing bytes (12260 px), 33-multi-column — 738; эталоны устарели после c87474a4 (PA-5: dashed/dotted бордеры cpu_raster через dash_math) — рендер изменён намеренно, эталоны не перегенерированы; незамечено, т.к. SAVE_CPU_SNAPSHOTS=1-прогоны скипают сравнение. Fix (ветка p3-bug140-clip-transform): эталоны перегенерированы, бит-в-бит стабильны между прогонами, гейт зелёный
