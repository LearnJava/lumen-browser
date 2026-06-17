# BUG-210

**Статус:** FIXED 2026-06-18
**Компонент:** layout (style.rs)
**Тест:** TEST-92 (15.59% → 0.90% → KNOWN_DEBTORS)

## Описание

CSS Color 4 §6.2 system colors (Canvas/ButtonFace/Highlight/GrayText/AccentColor и др.)
парсились и резолвились (`SystemColor::parse` + `resolve_system_colors_in_style` уже
существовали с коммита 3abcedf8), но `system_color()` возвращала значения, заметно
расходящиеся с тем, что рисует Edge в light-теме без forced-colors.

## Корень

Значения в таблице `system_color()` (light-схема) были подобраны «на глаз» и не
совпадали с Edge:

| keyword | было (Lumen) | Edge (эталон) |
|---|---|---|
| Highlight | (181,215,255) | (0,120,215) |
| HighlightText | (0,0,0) | (255,255,255) |
| LinkText | (0,0,238) | (0,102,204) |
| VisitedText | (85,26,139) | (0,102,204) |
| ActiveText | (255,0,0) | (0,102,204) |
| ButtonBorder | (118,118,118) | (0,0,0) |
| GrayText | (128,128,128) | (109,109,109) |
| AccentColor | (0,95,204) | (0,117,255) |
| ButtonFace | (239,239,239) | (240,240,240) |

Плюс deprecated CSS2 keywords (ThreeDHighlight/ThreeDShadow/Scrollbar) резолвились
в собственные «3D»-значения, тогда как Edge маппит их на стандартные (CSS Color 4
§6.3): ThreeD* → ButtonBorder (#000 в light), Scrollbar → Canvas (#fff).

## Фикс

`crates/engine/layout/src/style.rs` — обновлены light-значения в `system_color()`
под Edge-эталон (значения сэмплированы из reference-скриншота TEST-92) + deprecated
keywords приведены к стандартным эквивалентам. Регресс-тест
`system_color_light_values_match_edge` пинит точные значения.

`dump-layout` подтверждает: раскладка идеальна (164px border-box, gap 4px, целые
координаты), все hex точны. Остаток 0.90% — gdigrab суб-пиксельный сдвиг (~+3px на
1000px) на границах ячеек vs пиксельное округление Edge → BUG-124, TEST-92 в
KNOWN_DEBTORS.

## Воспроизведение (до фикса)

`python graphic_tests/run.py --only 92` → FAIL 15.59%
