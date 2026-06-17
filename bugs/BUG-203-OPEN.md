# BUG-203

**Статус:** OPEN
**Компонент:** paint
**Тест:** TEST-84 (diff 5.88%)

## Описание

`text-decoration-skip-ink`: auto/none/all — underline gaps over glyph descenders

## Воспроизведение

`python graphic_tests/run.py --only 84` → FAIL 5.88%

## Как чинить

Расследовать остаток после G-4 (text-decoration-skip-ink) — gap-ширина над descenders, точность пересечений.
