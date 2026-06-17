# BUG-214

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-110 (diff 2.47%)

## Описание

CSS UI `accent-color` — тинт чекбокса/радио/range/progress

## Воспроизведение

`python graphic_tests/run.py --only 110` → FAIL 2.47%

## Как чинить

Реализовать accent-color в form control renderer — применять цвет акцента к checkbox/radio/range/progress.
