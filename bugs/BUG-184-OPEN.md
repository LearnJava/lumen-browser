# BUG-184

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-31 (diff 0.59%)

## Описание

`clip-path`: inset/circle/ellipse/polygon bounding-box clip

## Воспроизведение

`python graphic_tests/run.py --only 31` → FAIL 0.59%

## Как чинить

Уточнить геометрию clip-path форм — проверить точность inset/circle/ellipse/polygon клипирования.
