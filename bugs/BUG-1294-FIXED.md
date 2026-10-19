# BUG-1294 — SVG `<pattern>` не реализован: заливка `fill="url(#pattern)"` ничего не рисует

**Статус:** FIXED 2026-10-19 (P3; задача `SVG-PATTERN`)
**Тип:** нереализованная функциональность (закрыта вместе с `SVG-PATTERN`) — элемента `<pattern>` (и `patternTransform`/`patternUnits`/`patternContentUnits`) нет вовсе: `grep -i pattern crates/engine/layout/src/box_tree/svg*.rs` пуст, `SvgPaint` знает `Color`/`Gradient`/`Url`, но не `Pattern`. Градиенты (`LIB-5`) делались отдельной парой типов; для паттерна нужна такая же («семейство», не точечная правка). Ведётся задачей `SVG-PATTERN`.
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout (`box_tree/svg.rs`, `SvgPaint`), paint (`emit_svg_shape` — заливка плиткой)

## Симптом

`--screenshot`, `<svg><defs><pattern id=p patternUnits=userSpaceOnUse width=50 height=100>…</pattern></defs>
<rect width=50 height=50 fill="url(#p)"/></svg>` — прямоугольник не нарисован (белый фон). Градиент в тех же условиях
рисуется. Эталоны WPT `css-transforms/matrix/*` строят «четырёхцветный квадрат» из паттерна и трансформа
`matrix(...)`: тест — красный прямоугольник-«якорь» под паттерном; без паттерна остаётся только красный.

## Как найдено

WPT-RUN-14 срез 5: 71 упавший reftest `css/css-transforms` (`matrix/svg-matrix-0*` 69, `patternTransform/*` 2) использует
`<pattern>` — красный «якорь» остаётся на снимке у 58 из 71 (подсчёт красных пикселей в `--screenshot` 800×600).

## Что делать

См. `SVG-PATTERN` в `ROADMAP.md`.

## Как проверить

`css/css-transforms/matrix/svg-matrix-0{01…48}.html`, `patternTransform/*`, `svg/painting/pattern*` (категория WPT `svg`).

## Исправление

- `SvgPaint::Pattern(Arc<SvgPatternDef>)` (`style/values/box_model.rs`): геометрия плитки, единицы, `patternTransform`, `viewBox`, `Arc<Vec<LayoutBox>>` содержимого. Тип лежит рядом с `SvgGradientDef`, а не в `BoxKind`, чтобы унаследованный `fill: url(#p)` доходил до потомков.
- `box_tree/svg.rs::resolve_svg_pattern`: разбор `<pattern>` с цепочкой `href` (до 8 звеньев, без циклов), каскад содержимого от самого `<pattern>`, а не от фигуры (иначе `fill: url(#p)` попал бы внутрь плитки); `<pattern>` как прямой потомок `<svg>` не рисуется.
- `display_list/svg_text_decoration.rs::emit_svg_pattern_tiles`: клип по контуру фигуры (как у градиентов), затем содержимое по каждой плитке. Только существующие команды (`PushClipRect`, `PushTransform`), обе ветки растра работают без правок. Оси-параллельная сетка (включая зеркало) вшивает матрицу в прямоугольники содержимого через `SvgPatternDef::content_positioned` — края остаются резкими, потому что слой `PushTransform` ресэмплируется как битмап. Повёрнутая/скошенная сетка идёт через слой. Не больше 4096 плиток на заливку.
- Раскладка: отрицательный масштаб вьюпорта нормализует `rect` (`lay_out_svg_element_position`).
- `stroke: url(#pattern)` не рисуется (плоского цвета для него нет).

Проба: `css/css-transforms/matrix/svg-matrix-001…048, 049…069` и `patternTransform/*` через `--screenshot` против эталона, 52 из 71 совпадают (порог 60 пикселей). Остаток: `036…048` — `clip-path: url(#clip)` на `<g>` ([BUG-1652](BUG-1652-OPEN.md)), `064…069` — CSS-синтаксис `matrix(50% …) translate(…)` в атрибуте `transform` ([BUG-1286](BUG-1286-OPEN.md)). GPU-бэкенд отдельно не проверен: новых команд нет.
