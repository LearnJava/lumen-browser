# BUG-187

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-34 (diff 4.78%)

## Описание

form controls: input/checkbox/radio/button/textarea/select static rendering

## Воспроизведение

`python graphic_tests/run.py --only 34` → FAIL 4.78%

## Как чинить

Улучшить UA-стили и статическую отрисовку form controls в femtovg_backend.
