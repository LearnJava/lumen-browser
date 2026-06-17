# BUG-194

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-65 (diff 1.33%)

## Описание

Flexbox `align-content`: flex-start/end/center/space-between/space-around/space-evenly/stretch multi-line

## Воспроизведение

`python graphic_tests/run.py --only 65` → FAIL 1.33%

## Как чинить

Расследовать остаточное отклонение align-content после BUG-165 — multi-line edge cases.
