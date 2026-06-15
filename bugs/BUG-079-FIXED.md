# BUG-079

**Статус:** FIXED 2026-06-14
**Компонент:** layout
**Файл:** `crates/engine/layout/src/style.rs:16426`

## Описание

quirks-bgcolor: TEST-20 8.79%→0.03%. Реальная причина — НЕ bgcolor (table-cells и legacy «garbage!»→cyan уже корректны), а ошибочное применение hashless-hex quirk к шортхенду `background: ff4444`. Edge сбрасывает его как невалидный (quirk применим только к лонгхендам background-color/color/border-*-color), Lumen же красил 5 swatch-ов. Фикс: parse_single_bg_layer парсит цвет с quirks=false
