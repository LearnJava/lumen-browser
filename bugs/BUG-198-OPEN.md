# BUG-198

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-70 (diff 7.82%)

## Описание

`object-fit`/`object-position` для SVG: fill/contain/cover/none/scale-down + viewBox scaling

## Воспроизведение

`python graphic_tests/run.py --only 70` → FAIL 7.82%

## Как чинить

Расследовать остаток после BUG-110 — object-position точность + viewBox scaling при cover/contain.
