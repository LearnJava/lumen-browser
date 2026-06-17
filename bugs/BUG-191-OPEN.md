# BUG-191

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-52 (diff 5.83%)

## Описание

`text-shadow` blur — PushFilter Blur wrapping: 4px/10px/20px sigma + multi-shadow

## Воспроизведение

`python graphic_tests/run.py --only 52` → FAIL 5.83%

## Как чинить

Расследовать PushFilter Blur для text-shadow — sigma scaling, multi-shadow stacking, offscreen layer sizing.
