# BUG-180

**Статус:** OPEN
**Компонент:** paint/image
**Тест:** TEST-18 (diff 21.21%)

## Описание

`<img>` rendering deviation — src/alt/dimensions/float/inline

## Воспроизведение

`python graphic_tests/run.py --only 18` → FAIL 21.21%

## Как чинить

Расследовать декодирование/отрисовку `<img>` с различными атрибутами (размеры, float, inline-контекст).
