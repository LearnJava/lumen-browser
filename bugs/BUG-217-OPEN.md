# BUG-217

**Статус:** OPEN
**Компонент:** css-parser
**Тест:** TEST-120 (diff 3.26%)

## Описание

Media Queries L5 `prefers-contrast`/`prefers-reduced-data` — matched-свотчи не совпадают с Edge

## Воспроизведение

`python graphic_tests/run.py --only 120` → FAIL 3.26%

## Как чинить

Реализовать парсинг и матчинг prefers-contrast/prefers-reduced-data в media query evaluator.
