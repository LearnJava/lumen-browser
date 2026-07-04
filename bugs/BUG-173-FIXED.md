# BUG-173

**Статус:** FIXED 2026-07-04
**Компонент:** paint
**Файл:** `crates/engine/paint/src/svg_path.rs`, `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

Остаточная дельта SVG `<path>` vs Edge после фикса BUG-102 (advanced stroke
attributes). TEST-54: 2.30%, TEST-60: 1.41%, TEST-119: 0.81% (порог 0.5%) — все
были в `KNOWN_DEBTORS` (`run.py`). Геометрия штриха была корректна (ширина, dash,
caps, joins, гладкие кривые), остаток складывался из четырёх независимых причин:

1. **Triangle-soup AA-швы.** `DrawSvgPath` нёс плоский список треугольников;
   `femtovg_backend` заливал каждый треугольник отдельным закрытым сабпасом →
   антиалиасинг по общим внутренним рёбрам соседних треугольников не сокращался
   → слабая диагональ внутри каждого quad'а.
2. **Stroke-edge AA.** Граница штрих↔фон антиалиасилась иначе, чем у Edge.
3. **Self-intersecting fill.** `tessellate_fill` (ear-clipping) не заполнял
   самопересекающиеся контуры (галстук-бабочка `X` в TEST-54).
4. **Dash-on-curve.** Раскладка штрихов по flattened-кривой слегка расходилась
   по фазе с Edge.

## Решение

Все причины устранены переводом SVG-заливки и обводки с пре-тесселлированного
triangle-soup на нативный рендер по сырым контурам — двумя срезами:

- **Fill-сторона (2026-06-30, BUG-247-срез):** nonzero `<path>`/`<polygon>`
  заливка рендерится нативно по контурам (`DrawSvgFill` → femtovg `fill_path`
  + cpu_raster tiny_skia `FillRule::Winding`) вместо триангуляции. AA ложится
  только на истинную границу (причина 1, fill); self-intersecting nonzero
  заливается по winding, а не ear_clip per-contour → bowtie исчез (причина 3).
- **Stroke-сторона (2026-07-04, срез d87dae63):** обводка переведена с
  `DrawSvgPath` (triangle-soup) на новую команду `DrawSvgStroke` (сырые контуры
  + `StrokeParams`): femtovg штрихует нативно через `stroke_path` — AA только на
  истинной границе штриха, без внутренних швов на кривых/пунктире (причины 1
  stroke, 2, 4); дэш-нарезка тем же `apply_dash_pattern`, что и fallback.
  CPU/wgpu ре-тесселлируют через `tessellate_stroke_ex` (бит-идентично старому
  выводу — регресс-тест `svg_stroke_cpu_matches_tessellated_path`).

## Измерение (свежий full-build gdigrab, 2026-07-04)

| Тест | Было (baseline) | Стало | Порог |
|---|---|---|---|
| TEST-54 (`<path>` stroke) | 1.14% | **0.26%** | 0.5% ✅ |
| TEST-60 (stroke advanced/dash) | 1.41% | **0.40%** | 0.5% ✅ |
| TEST-119 (paint-order thick stroke) | 0.81% | **0.38%** | 0.5% ✅ |

Все три теста ниже строгого порога 0.5% → записи `54`/`60`/`119` удалены из
`KNOWN_DEBTORS`. TEST-119 дополнительно зависела от BUG-262 (FIXED 2026-06-29,
`svg_paint_matrix` разведён от `svg_transform`) — регрессия paint-order
0.81%→16.52% там была layout-дефектом, а не AA-остатком stroke.

Инхерентный femtovg-vs-Edge AA повёрнутых `<rect>`/circle/ellipse-колец и
высокочастотного dash остаётся якорем **BUG-247** (TEST-134/136/137/138) — это
отдельный OPEN-должник, не относящийся к `<path>` fill/stroke.
