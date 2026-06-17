# BUG-190

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-49 (diff 2.39%)

## Описание

`background-blend-mode`: multiply/screen/overlay/darken/lighten/difference/exclusion/color-dodge/luminosity

## Воспроизведение

`python graphic_tests/run.py --only 49` → FAIL 2.39%

## Как чинить

Расследовать остаточные отклонения blend-mode после BUG-091 — точность CPU-blend операций.
