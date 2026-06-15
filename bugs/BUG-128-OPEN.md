# BUG-128

**Статус:** OPEN
**Компонент:** font
**Файл:** `crates/engine/paint/src/display_list.rs:5547`

## Описание

text-underline TEST-79: 6.78%. РАССЛЕДОВАНО 2026-06-14 (P3): подчёркивание НЕ geometry-баг — вертикаль в пределах 1–2px от Edge, ~3px gap text→underline в обоих. Расхождение целиком из-за дефолтного шрифта: Edge рендерит serif (Times), Lumen — Inter (sans). Из 5.80% CPU-диффа 4.35% — глифы/ширина текста (нередуцируемо), лишь 1.46% в полосах underline и тоже font-width-driven. Блокировано задачей text/font-parity (deferred, см. правило «Ignore text for now»); кандидат в KNOWN_DEBTORS, а не в paint-фикс
