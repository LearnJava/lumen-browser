# BUG-226

**Статус:** FIXED 2026-06-21 (DEBTOR — остаток 1.20% переатрибутирован на BUG-176)
**Компонент:** paint
**Тест:** TEST-47 (2.27% → 1.20%, KNOWN_DEBTORS BUG-176)

## Фикс

`emit_svg_shape` (`crates/engine/paint/src/display_list.rs`), армы `Rect` и
`Circle/Ellipse`: штрих центрируется на кромке геометрии. `b.rect` надувается на
`stroke_w/2` по всем сторонам (`stroke_rect`), внешние радиусы = `r + stroke_w/2`
(для скруглённых; square-углы без радиуса остаются square). `DrawBorder` рисует
внутрь от `stroke_rect` на полную ширину `w`, поэтому внутренняя кромка ложится на
`r − stroke_w/2`, а центрлайн штриха — точно на исходную кромку `b.rect`. Fill
остаётся на исходном `b.rect`. Even-odd-кольцо (BUG-175) само строит внутренний
радиус для скруглённых случаев.

Регресс-тест `svg_rect_stroke_is_centred_on_edge` (display_list.rs): stroke
`DrawBorder.rect` = fill `FillRect.rect`, надутый на w/2 по всем сторонам, ширина
сохранена. Прогон gdigrab dev-release: TEST-47 2.27% → 1.20%, TEST-70 1.63%
(без изменений), TEST-82 2.38% → 2.31% — регрессий нет. TEST-54/60/119 используют
только `<path>` (другая ветка, не затронуты).

## Описание (исходное)

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
