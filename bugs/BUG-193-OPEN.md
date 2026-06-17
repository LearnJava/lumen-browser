# BUG-193

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-64 (diff 13.89%)

## Описание

CSS 2.1 §17 Table layout: `border-spacing`, cell backgrounds/borders, `col_span`/`row_span`

## Воспроизведение

`python graphic_tests/run.py --only 64` → FAIL 13.89%

## Как чинить

Расследовать table layout — border-spacing применение, colspan/rowspan геометрию, cell backgrounds.
