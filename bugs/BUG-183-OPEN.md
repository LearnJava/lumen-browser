# BUG-183

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-26 (diff 17.74%)

## Описание

`mask-image` с linear/radial gradient mask — не реализован

## Воспроизведение

`python graphic_tests/run.py --only 26` → FAIL 17.74%

## Как чинить

Реализовать mask-image в femtovg_backend: применять gradient-маску как alpha-channel через offscreen FBO.
