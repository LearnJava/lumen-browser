# BUG-226

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-47 (DEBTOR 2.27%, baseline в KNOWN_DEBTORS)

## Описание

SVG-штрих (`stroke`) на basic shapes (`rect`/`circle`/`ellipse`) рисуется целиком
**внутри** бокса по CSS border-box-модели (`DrawBorder { rect: b.rect }`), тогда как
SVG центрирует штрих на кромке геометрии (SVG 2 §13.7 / SVG 1.1 §11.4): половина
ширины наружу, половина внутрь.

## Воспроизведение

`python graphic_tests/run.py --only 47`. Замер прямоугольника со stroke-width:10
(row 4, `stroke-opacity` demo): видимый orange-core 79×59px (Lumen) против 89×69px
(Edge) — разница ровно ±5px на сторону = stroke-width/2. Тот же сдвиг кромки у всех
обведённых rect/circle/ellipse.

## Как чинить

В `emit_svg_shape` (`crates/engine/paint/src/display_list.rs`), армы `Rect` и
`Circle/Ellipse`: для центрирования штриха надувать `b.rect` на `stroke_w/2` по всем
сторонам перед `DrawBorder` и задавать внешние радиусы как `r + stroke_w/2` (для
скруглённых; внутренние радиусы even-odd-кольца BUG-175 сами дадут `r - stroke_w/2`,
центрлайн = `r`). Fill остаётся на исходном `b.rect`.

**Внимание (риск регрессий):** изменение геометрии штриха затрагивает TEST-54/60/82/119
(их собственные KNOWN_DEBTORS-baseline). Перед закрытием прогнать все SVG-тесты
свежей сборкой (`LUMEN_PROFILE=dev-release`) и сверить, что debtor'ы не выросли.

## Связано

- BUG-189 (FIXED) — линия в том же тесте; остаток после фикса = этот центринг.
- BUG-176 — kappa-AA эллиптических дуг (часть остаточного diff TEST-47).
