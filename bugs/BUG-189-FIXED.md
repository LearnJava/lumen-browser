# BUG-189

**Статус:** FIXED 2026-06-21 (DEBTOR → BUG-226)
**Компонент:** paint
**Тест:** TEST-47 (3.71% → 2.27% DEBTOR)

## Описание

SVG basic shapes — rect/circle/ellipse/line in document flow, viewBox scale.

Доминирующий дефект: диагональный `<line>` (row 1, `x1=180 y1=20 x2=300 y2=100
stroke:#f39c12 stroke-width:6`) рисовался как `FillRect { rect: b.rect }` — то есть
заливал **весь bounding box** сегмента сплошным оранжевым прямоугольником вместо
тонкой диагонали (≈45% в ячейке).

## Причина

`emit_svg_shape` (`crates/engine/paint/src/display_list.rs`), арм `SvgShapeKind::Line`,
эмитил `FillRect` по `b.rect`. `b.rect` для линии — это её doc-space bounding box; для
диагонали это большой прямоугольник.

## Фикс

Арм `Line` теперь штрихует толстый сегмент через `push_thick_segment` →
`DisplayCommand::DrawSvgPath` (как checkmark/path-stroke). `b.rect` уже в doc-space
(учитывает viewBox-scale); знаки user-координат концов (`x1<=x2`, `y1<=y2`) выбирают,
по какой диагонали bbox идёт сегмент. Линия не заливается — только stroke (SVG `<line>`
не имеет fill). Half-width = `stroke_width/2`, butt-cap.

Регресс-тест `svg_diagonal_line_strokes_segment_not_filled_bbox` (display_list.rs):
линия эмитит ровно один stroke `DrawSvgPath` (6 вершин) и НЕ эмитит `FillRect` в
stroke-цвете. Проверено и для `walk`, и для ordered-пути.

Только TEST-47 (+ kitchen-sink final) используют `<line>` → регрессий нет.

## Остаток (→ BUG-226)

2.27% = SVG-штрих не центрирован на кромке: Lumen рисует stroke целиком ВНУТРИ бокса
(border-box DrawBorder), Edge центрирует (½ наружу + ½ внутрь, SVG 2 §13.7). Замер
stroke-opacity-прямоугольника: orange-core 79×59 (Lumen) vs 89×69 (Edge) = ±5px =
stroke-width/2. Затрагивает rect/circle/ellipse во всех SVG-тестах → отдельный
**BUG-226**. Плюс stroke-edge / rounded-corner AA (класс BUG-176). TEST-47 →
KNOWN_DEBTORS (BUG-226, baseline 2.27%).
