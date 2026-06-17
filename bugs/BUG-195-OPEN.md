# BUG-195

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-66 (diff 1.07%)

## Описание

`::selection` pseudo-element: background-color + color override

## Воспроизведение

`python graphic_tests/run.py --only 66` → FAIL 1.07%

## Как чинить

Расследовать остаток TEST-66 (после BUG-108) — возможно связан с ::selection или border-radius AA (rule 3).
