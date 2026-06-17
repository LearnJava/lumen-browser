# BUG-189

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-47 (diff 3.71%)

## Описание

SVG basic shapes — rect/circle/ellipse/line in document flow, viewBox scale

## Воспроизведение

`python graphic_tests/run.py --only 47` → FAIL 3.71%

## Как чинить

Расследовать остаточные отклонения SVG basic shapes после BUG-089 — viewBox scaling и AA.
