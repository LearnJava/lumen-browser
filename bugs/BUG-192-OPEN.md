# BUG-192

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-55 (diff 0.89%)

## Описание

`<video>` replaced element — grey placeholder; UA 300×150; CSS dimensions; border-radius

## Воспроизведение

`python graphic_tests/run.py --only 55` → FAIL 0.89%

## Как чинить

Уточнить отрисовку placeholder для `<video>` — размеры по умолчанию 300×150, border-radius, цвет.
