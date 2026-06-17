# BUG-215

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-113 (diff 1.41%)

## Описание

CSS Shapes `shape-outside: path()` — float обтекание треугольного SVG-контура

## Воспроизведение

`python graphic_tests/run.py --only 113` → FAIL 1.41%

## Как чинить

Реализовать shape-outside: path() в float layout — вычислять line exclusions по SVG path контуру.
