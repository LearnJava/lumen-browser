# BUG-201

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-82 (diff 5.00%)

## Описание

SVG `<use>`: clone shapes/groups/symbols из `<defs>`, x/y offset, xlink:href, nested chains

## Воспроизведение

`python graphic_tests/run.py --only 82` → FAIL 5.00%

## Как чинить

Расследовать клонирование SVG `<use>` — transform inherit, nested `<use>` chains, symbol viewport.
