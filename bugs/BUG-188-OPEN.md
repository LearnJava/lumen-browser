# BUG-188

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-46 (diff 4.63%)

## Описание

CSS Transforms L2 — individual `translate` / `rotate` / `scale` properties

## Воспроизведение

`python graphic_tests/run.py --only 46` → FAIL 4.63%

## Как чинить

Расследовать применение individual transform properties — порядок комбинирования с matrix transform.
