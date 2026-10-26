# BUG-1722 — фон выделения короче строки

**Статус:** OPEN
**Заведён:** 2026-10-26 (P6, попутно с BUG-1687)
**Область:** layout/paint (`crates/engine/layout/src/highlight.rs`, ширина `HighlightPiece`)

## Симптом

На `tests/wpt/selection/shadow-dom/cross-shadow-boundary-1.html` фон `::selection` у «OuterText» не закрывает
последнюю букву (и в тесте, и в эталоне). На PASS рефтеста не влияет — ширина одинакова с обеих сторон.

## Как проверить

`lumen --viewport 400x160 --screenshot out.png tests/wpt/selection/shadow-dom/cross-shadow-boundary-1.html` —
правый край голубого фона заканчивается раньше правого края текста. Сравнить ширину куска с шириной слова
шейпера (TEXTQ-5: раскладка меряет слова шейпером).
