# BUG-182

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-24 (diff 0.98%)

## Описание

`vertical-align` inline y-offset + inline-block positioning

## Воспроизведение

`python graphic_tests/run.py --only 24` → FAIL 0.98%

## Как чинить

Расследовать вычисление baseline offset для inline/inline-block элементов с различными vertical-align значениями.
