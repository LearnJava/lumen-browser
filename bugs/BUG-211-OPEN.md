# BUG-211

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-93 (diff 4.11%)

## Описание

CSS Basic UI `field-sizing: content` — input/textarea подгоняют размер под содержимое

## Воспроизведение

`python graphic_tests/run.py --only 93` → FAIL 4.11%

## Как чинить

Реализовать field-sizing: content в layout form controls — shrink-to-fit по content width/height.
