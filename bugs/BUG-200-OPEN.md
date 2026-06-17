# BUG-200

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-80 (diff 9.89%)

## Описание

CSS Tables `border-collapse`: separate vs collapse + mixed border widths + cell backgrounds

## Воспроизведение

`python graphic_tests/run.py --only 80` → FAIL 9.89%

## Как чинить

Расследовать остаток после BUG-129 — paint-side varied-width границы при collapse, cell background ordering.
