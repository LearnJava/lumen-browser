# BUG-197

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-69 (diff 3.61%)

## Описание

CSS 2.1 §17.6 `border-spacing`: equal и asymmetric gaps между ячейками таблицы

## Воспроизведение

`python graphic_tests/run.py --only 69` → FAIL 3.61%

## Как чинить

Проверить реализацию asymmetric border-spacing (horizontal ≠ vertical) в table layout.
