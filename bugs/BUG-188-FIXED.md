# BUG-188

**Статус:** FIXED 2026-06-21 (DEBTOR — KNOWN_DEBTORS BUG-128)
**Компонент:** layout/paint
**Тест:** TEST-46 (4.63% → 1.96%)

## Описание

CSS Transforms L2 — individual `translate` / `rotate` / `scale` properties.

## Расследование

Прежние «4.63%» — от устаревшего бинаря (стандартная ловушка `run.py` без `--build`).
Свежая сборка: TEST-46 = 1.96%.

Пиксельный замер центроидов/bbox всех трансформированных боксов (Edge vs Lumen
gdigrab-снимок, тол. 18, разбивка по y-полосам ряда):

| Бокс | свойство | Edge центроид | Lumen центроид | вердикт |
|---|---|---|---|---|
| t-translate | `translate: 20px 10px` | (99.9, 92.2) | (99.8, 92.4) | пиксель-в-пиксель |
| t-rotate | `rotate: 45deg` | (80.5, 205.3) | (80.5, 205.1) | пиксель-в-пиксель |
| t-scale-uniform | `scale: 1.4` | (80.5, 308.0) | (80.5, 308.0) | идентично |
| t-scale-xy | `scale: 1.6 0.7` | (80.5, 410.5) | (80.5, 410.5) | идентично |
| t-translate-only-x | `translate: 30px` | (430.5, 410.5) | (430.5, 410.5) | идентично |
| t-all-three | translate+rotate+scale | (90.5, 510.5) | (90.5, 510.5) | идентично |
| **t-individual-plus-transform** | translate+scale+`transform:rotate` | (437.5, 520.5) | (419.0, 520.5) | **сдвиг X 18.5px** |

Все individual-свойства (включая комбинацию всех трёх) рендерятся идентично Edge.
Единственное расхождение — teal-бокс `t-individual-plus-transform`, но его **форма,
масштаб и поворот корректны** (площадь 5064 vs 5076, bbox 85×85 vs 86×87, y-центроид
совпадает) — отличается только X-позиция на 18.5px.

Причина: teal-бокс стоит в flex-ряду **после** monospace-метки
`translate+rotate+scale combined`. Lumen рендерит `font-family: monospace` через
Inter-fallback (нет реального моноширинного шрифта), ширина метки отличается от Edge
→ flex-раскладка кладёт следующий бокс на другую X. То есть сдвиг — downstream
font-parity, а не дефект трансформа.

Композиция individual + `transform` спек-корректна (CSS Transforms L2 §3):
`forward_box_transform` (`property_trees.rs:664`) применяет
translate → rotate → scale → transform вокруг общего `transform-origin`-pivot.
Закреплено регресс-тестом
`individual_plus_transform_composes_translate_then_scale_then_rotate` (`lib.rs`).

## Остаток (1.96%)

Целиком font-parity (класс BUG-128):
1. 8 monospace-меток рисуются с другой шириной/начертанием (прямой text-diff) — основная масса.
2. Косвенный сдвиг teal-бокса на ~18px из-за ширины предшествующей метки в flex-ряду.

Даже при исправлении позиции teal-бокса 8 текстовых меток держат diff > 0.5%.
TEST-46 → KNOWN_DEBTORS (BUG-128, baseline 1.96%). Правок production-кода не было
(трансформ уже корректен); добавлен только регресс-тест, фиксирующий спек-инвариант.
