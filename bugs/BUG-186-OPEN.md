# BUG-186

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-33 (diff 14.89%)

## Описание

multi-column: `column-count`/`column-width` layout + `column-rule` solid/dashed/dotted

## Воспроизведение

`python graphic_tests/run.py --only 33` → FAIL 14.89%

## Как чинить

Расследовать алгоритм многоколоночной раскладки — column balancing, column-rule rendering.
